//! # Ferric Type Inference (M3)
//!
//! Replaces `ferric_typecheck` with a Hindley-Milner inference engine using
//! Algorithm J — fresh type variables, unification with the occurs check,
//! and let-generalisation. Public surface is identical to the previous stage:
//! a single `typecheck` entry point taking `(ast, resolve, interner)` and
//! returning a `TypeResult`.

use std::collections::{HashMap, HashSet};

use ferric_common::{
    BinOp, Expr, Interner, Item, Literal, NamedArg, NodeId, Param, ParseResult,
    RequireStmt, ResolveResult, ShellPart, Span, Stmt, Symbol, Ty, TyVar, TypeAnnotation,
    TypeError, TypeResult, TypeScheme, UnOp,
};

/// Type-checks (and infers) a parsed AST with resolution information.
///
/// This is the single public entry point for the inference stage. It mirrors
/// the M1 `ferric_typecheck::typecheck` signature so swapping the crate is a
/// one-line change in `main.rs`.
pub fn typecheck(
    ast: &ParseResult,
    resolve: &ResolveResult,
    interner: &Interner,
) -> TypeResult {
    let mut infer = TypeInfer::new(ast, resolve, interner);
    infer.infer_program();
    infer.finish()
}

// ============================================================================
// Substitution
// ============================================================================

/// A type substitution: a partial map from type variables to types.
#[derive(Debug, Default, Clone)]
struct Substitution {
    map: HashMap<TyVar, Ty>,
}

impl Substitution {
    fn new() -> Self {
        Self::default()
    }

    /// Resolves a type by repeatedly walking the substitution until a fixed
    /// point is reached. The returned type still contains structural variables
    /// that have not been bound, but no `Var(v)` whose `v` is mapped.
    fn apply(&self, ty: &Ty) -> Ty {
        match ty {
            Ty::Var(v) => match self.map.get(v) {
                Some(t) => self.apply(t),
                None => Ty::Var(*v),
            },
            Ty::Fn { params, ret } => Ty::Fn {
                params: params.iter().map(|p| self.apply(p)).collect(),
                ret: Box::new(self.apply(ret)),
            },
            other => other.clone(),
        }
    }

    fn extend(&mut self, var: TyVar, ty: Ty) {
        self.map.insert(var, ty);
    }
}

/// Returns true if `var` appears anywhere within `ty` after substitution.
fn occurs(var: TyVar, ty: &Ty, subst: &Substitution) -> bool {
    let ty = subst.apply(ty);
    occurs_raw(var, &ty)
}

fn occurs_raw(var: TyVar, ty: &Ty) -> bool {
    match ty {
        Ty::Var(v) => *v == var,
        Ty::Fn { params, ret } => {
            params.iter().any(|t| occurs_raw(var, t)) || occurs_raw(var, ret)
        }
        _ => false,
    }
}

/// Collects free type variables of a type (under the given substitution).
fn free_vars_in(ty: &Ty, subst: &Substitution, out: &mut HashSet<TyVar>) {
    let ty = subst.apply(ty);
    free_vars_raw(&ty, out);
}

fn free_vars_raw(ty: &Ty, out: &mut HashSet<TyVar>) {
    match ty {
        Ty::Var(v) => {
            out.insert(*v);
        }
        Ty::Fn { params, ret } => {
            for p in params {
                free_vars_raw(p, out);
            }
            free_vars_raw(ret, out);
        }
        _ => {}
    }
}

/// Returns true if any unresolved type variable still appears in the type.
fn has_type_vars(ty: &Ty, subst: &Substitution) -> bool {
    let mut s = HashSet::new();
    free_vars_in(ty, subst, &mut s);
    !s.is_empty()
}

// ============================================================================
// Type environment
// ============================================================================

/// A scoped binding environment. Each scope maps user-visible names (Symbols)
/// to type schemes. Shadowing is handled by pushing a new scope.
#[derive(Debug, Default)]
struct InferEnv {
    scopes: Vec<HashMap<Symbol, TypeScheme>>,
    /// All free type variables introduced by *monomorphic* bindings currently
    /// in scope. Used to compute generalisation correctly: only variables not
    /// free in the environment may be quantified.
    monomorphic_vars: Vec<HashSet<TyVar>>,
}

impl InferEnv {
    fn new() -> Self {
        let mut env = Self::default();
        env.push_scope();
        env
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
        self.monomorphic_vars.push(HashSet::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
        self.monomorphic_vars.pop();
    }

    fn define(&mut self, name: Symbol, scheme: TypeScheme) {
        self.scopes.last_mut().unwrap().insert(name, scheme);
    }

    fn lookup(&self, name: Symbol) -> Option<&TypeScheme> {
        for scope in self.scopes.iter().rev() {
            if let Some(s) = scope.get(&name) {
                return Some(s);
            }
        }
        None
    }

    /// Records that a type variable is monomorphically pinned (e.g., a
    /// function parameter): it must not be generalised by a nested `let`.
    fn pin_monomorphic_vars(&mut self, vars: impl IntoIterator<Item = TyVar>) {
        let frame = self.monomorphic_vars.last_mut().unwrap();
        for v in vars {
            frame.insert(v);
        }
    }

    /// All currently-pinned monomorphic type variables across every scope.
    fn all_monomorphic_vars(&self) -> HashSet<TyVar> {
        let mut all = HashSet::new();
        for s in &self.monomorphic_vars {
            all.extend(s.iter().copied());
        }
        all
    }
}

// ============================================================================
// Inferencer
// ============================================================================

struct TypeInfer<'a> {
    ast: &'a ParseResult,
    resolve: &'a ResolveResult,
    interner: &'a Interner,

    next_tyvar: u32,
    subst: Substitution,
    env: InferEnv,

    /// Within the body of the current function, the expected return type.
    /// `None` at the top level (script statements).
    current_fn_ret: Option<Ty>,

    /// Stable mapping from a "generic" type-name symbol (e.g. `T`) to a
    /// fresh type variable, scoped to the function being processed.
    generic_aliases: HashMap<Symbol, TyVar>,

    /// Output: every node's resolved type.
    node_types: HashMap<NodeId, Ty>,
    /// Shell interpolation expression IDs that need a Str-or-Int post-check.
    shell_interp_nodes: Vec<(NodeId, Span)>,
    /// Output: type errors encountered.
    errors: Vec<TypeError>,
}

impl<'a> TypeInfer<'a> {
    fn new(
        ast: &'a ParseResult,
        resolve: &'a ResolveResult,
        interner: &'a Interner,
    ) -> Self {
        Self {
            ast,
            resolve,
            interner,
            next_tyvar: 0,
            subst: Substitution::new(),
            env: InferEnv::new(),
            current_fn_ret: None,
            generic_aliases: HashMap::new(),
            node_types: HashMap::new(),
            shell_interp_nodes: Vec::new(),
            errors: Vec::new(),
        }
    }

    fn fresh_tyvar(&mut self) -> Ty {
        let v = TyVar(self.next_tyvar);
        self.next_tyvar += 1;
        Ty::Var(v)
    }

    fn fresh_var_only(&mut self) -> TyVar {
        let v = TyVar(self.next_tyvar);
        self.next_tyvar += 1;
        v
    }

    // ------------------------------------------------------------------
    // Top-level
    // ------------------------------------------------------------------

    fn infer_program(&mut self) {
        // 1. Register native function signatures so calls to stdlib like
        //    `println(s: ...)` can be checked.
        self.register_native_signatures();

        // 2. Pre-pass: collect every user-defined function's signature so
        //    forward references and recursion type-check correctly.
        let mut fn_aliases: Vec<HashMap<Symbol, TyVar>> = Vec::new();
        for item in &self.ast.items {
            if let Item::FnDef { name, params, ret_ty, .. } = item {
                let mut aliases = HashMap::new();
                let param_tys: Vec<Ty> = params
                    .iter()
                    .map(|p| self.resolve_type_annotation(&p.ty, &mut aliases))
                    .collect();
                let ret_ty_resolved = self.resolve_type_annotation(ret_ty, &mut aliases);
                let fn_ty = Ty::Fn {
                    params: param_tys,
                    ret: Box::new(ret_ty_resolved),
                };
                let scheme = TypeScheme {
                    forall: aliases.values().copied().collect(),
                    ty: fn_ty,
                };
                self.env.define(*name, scheme);
                fn_aliases.push(aliases);
            }
        }

        // 3. Walk items: check each function body and each script statement.
        let mut fn_idx = 0;
        for item in &self.ast.items {
            match item {
                Item::FnDef { id, params, ret_ty, body, .. } => {
                    let aliases = fn_aliases[fn_idx].clone();
                    fn_idx += 1;
                    self.check_fn_def(*id, params, ret_ty, body, aliases);
                }
                Item::Script { stmt, .. } => {
                    self.check_stmt(stmt);
                }
            }
        }
    }

    /// Registers the stdlib native signatures (looked up by name in the
    /// interner). Names that were never interned are silently skipped —
    /// they cannot appear in the AST.
    fn register_native_signatures(&mut self) {
        let natives: &[(&str, &[Ty], Ty)] = &[
            ("println", &[Ty::Str], Ty::Unit),
            ("print", &[Ty::Str], Ty::Unit),
            ("int_to_str", &[Ty::Int], Ty::Str),
            ("float_to_str", &[Ty::Float], Ty::Str),
            ("bool_to_str", &[Ty::Bool], Ty::Str),
            ("int_to_float", &[Ty::Int], Ty::Float),
            ("shell_stdout", &[Ty::ShellOutput], Ty::Str),
            ("shell_exit_code", &[Ty::ShellOutput], Ty::Int),
        ];
        for (name, params, ret) in natives {
            if let Some(sym) = self.interner.lookup(name) {
                let fn_ty = Ty::Fn {
                    params: params.to_vec(),
                    ret: Box::new(ret.clone()),
                };
                self.env.define(sym, TypeScheme::monomorphic(fn_ty));
            }
        }
    }

    fn check_fn_def(
        &mut self,
        id: NodeId,
        params: &[Param],
        ret_ty: &TypeAnnotation,
        body: &Expr,
        aliases: HashMap<Symbol, TyVar>,
    ) {
        // Re-resolve param/return types using the same aliases captured during
        // the pre-pass so all occurrences of a generic name (e.g. `T`) refer
        // to the same TyVar.
        let mut aliases = aliases;
        let param_tys: Vec<Ty> = params
            .iter()
            .map(|p| self.resolve_type_annotation(&p.ty, &mut aliases))
            .collect();
        let ret_ty_resolved = self.resolve_type_annotation(ret_ty, &mut aliases);

        // Defaults are evaluated in the enclosing scope, so check them before
        // we push the function-body scope.
        for (param, declared_ty) in params.iter().zip(param_tys.iter()) {
            if let Some(default) = &param.default {
                let default_ty = self.infer_expr(default);
                self.unify(declared_ty, &default_ty, default.span());
            }
        }

        // Function-body scope.
        self.env.push_scope();
        // Pin every type variable that appears in any parameter type so a
        // nested `let` cannot generalise over it.
        for p in &param_tys {
            let mut fv = HashSet::new();
            free_vars_in(p, &self.subst, &mut fv);
            self.env.pin_monomorphic_vars(fv);
        }

        for (param, declared_ty) in params.iter().zip(param_tys.iter()) {
            self.env
                .define(param.name, TypeScheme::monomorphic(declared_ty.clone()));
        }

        let prev_ret = self.current_fn_ret.take();
        self.current_fn_ret = Some(ret_ty_resolved.clone());

        let prev_aliases = std::mem::replace(&mut self.generic_aliases, aliases);

        let body_ty = self.infer_expr(body);
        self.unify(&ret_ty_resolved, &body_ty, body.span());

        self.generic_aliases = prev_aliases;
        self.current_fn_ret = prev_ret;
        self.env.pop_scope();

        // Record the function definition's own type at its NodeId.
        let fn_ty = Ty::Fn {
            params: param_tys,
            ret: Box::new(ret_ty_resolved),
        };
        self.node_types.insert(id, fn_ty);
    }

    // ------------------------------------------------------------------
    // Type annotations
    // ------------------------------------------------------------------

    fn resolve_type_annotation(
        &mut self,
        ty: &TypeAnnotation,
        aliases: &mut HashMap<Symbol, TyVar>,
    ) -> Ty {
        match ty {
            TypeAnnotation::Named(sym) => {
                let name = self.interner.resolve(*sym);
                match name {
                    "Int" => Ty::Int,
                    "Float" => Ty::Float,
                    "Str" => Ty::Str,
                    "Bool" => Ty::Bool,
                    "Unit" | "" => Ty::Unit,
                    "ShellOutput" => Ty::ShellOutput,
                    _ => {
                        // Treat any unknown identifier as a generic type
                        // variable, consistent across one function signature.
                        let var = *aliases.entry(*sym).or_insert_with(|| {
                            let v = TyVar(self.next_tyvar);
                            self.next_tyvar += 1;
                            v
                        });
                        Ty::Var(var)
                    }
                }
            }
        }
    }

    // ------------------------------------------------------------------
    // Statements
    // ------------------------------------------------------------------

    fn check_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let { name, ty, init, id, span, .. } => {
                let init_ty = self.infer_expr(init);

                let bound_ty = if let Some(ann) = ty {
                    let mut tmp = HashMap::new();
                    let declared = self.resolve_type_annotation(ann, &mut tmp);
                    self.unify(&declared, &init_ty, *span);
                    declared
                } else {
                    init_ty.clone()
                };

                // Generalise over any free vars not pinned by the surrounding
                // monomorphic environment — classic let-polymorphism.
                let scheme = self.generalize(&bound_ty);
                self.env.define(*name, scheme);

                self.node_types.insert(*id, self.subst.apply(&bound_ty));
            }
            Stmt::Assign { target, value, span, .. } => {
                let value_ty = self.infer_expr(value);
                let target_ty = self.infer_expr(target);
                self.unify(&target_ty, &value_ty, *span);
            }
            Stmt::Expr { expr } => {
                self.infer_expr(expr);
            }
            Stmt::Require(req) => {
                self.check_require(req);
            }
        }
    }

    fn check_require(&mut self, req: &RequireStmt) {
        let expr_ty = self.infer_expr(&req.expr);
        if !self.unify_or(
            &expr_ty,
            &Ty::Bool,
            req.expr.span(),
            |found| TypeError::RequireNonBool { found, span: req.expr.span() },
        ) {
            // unify_or already pushed an error, no further action.
        }

        if let Some(msg) = &req.message {
            let msg_ty = self.infer_expr(msg);
            self.unify_or(&msg_ty, &Ty::Str, msg.span(), |found| {
                TypeError::RequireMessageNonStr { found, span: msg.span() }
            });
        }

        if let Some(set_fn) = &req.set_fn {
            let set_fn_ty = self.infer_expr(set_fn);
            let expected = Ty::Fn {
                params: vec![],
                ret: Box::new(Ty::Unit),
            };
            self.unify_or(&set_fn_ty, &expected, set_fn.span(), |found| {
                TypeError::RequireSetType { found, span: set_fn.span() }
            });
        }
    }

    /// Like `unify`, but if unification fails the caller-supplied error is
    /// emitted instead of a generic mismatch. Returns whether unification
    /// succeeded.
    fn unify_or(
        &mut self,
        a: &Ty,
        b: &Ty,
        _span: Span,
        make_err: impl FnOnce(Ty) -> TypeError,
    ) -> bool {
        match self.try_unify(a, b) {
            Ok(()) => true,
            Err(()) => {
                let found = self.subst.apply(a);
                self.errors.push(make_err(found));
                false
            }
        }
    }

    // ------------------------------------------------------------------
    // Expressions
    // ------------------------------------------------------------------

    fn infer_expr(&mut self, expr: &Expr) -> Ty {
        let ty = match expr {
            Expr::Literal { value, id, .. } => {
                let lit_ty = match value {
                    Literal::Int(_) => Ty::Int,
                    Literal::Float(_) => Ty::Float,
                    Literal::Str(_) => Ty::Str,
                    Literal::Bool(_) => Ty::Bool,
                    Literal::Unit => Ty::Unit,
                };
                self.node_types.insert(*id, lit_ty.clone());
                lit_ty
            }

            Expr::Variable { name, id, span: _ } => {
                let scheme = self.env.lookup(*name).cloned();
                // If the resolver already reported this name as undefined we
                // stay quiet here: the resolver error is enough, and a noisy
                // CannotInfer follow-on would only confuse the diagnostic.
                let ty = match scheme {
                    Some(s) => self.instantiate(&s),
                    None => self.fresh_tyvar(),
                };
                self.node_types.insert(*id, ty.clone());
                ty
            }

            Expr::Binary { op, left, right, id, span } => {
                let left_ty = self.infer_expr(left);
                let right_ty = self.infer_expr(right);
                let result_ty = self.infer_binary_op(*op, &left_ty, &right_ty, *span);
                self.node_types.insert(*id, result_ty.clone());
                result_ty
            }

            Expr::Unary { op, expr: inner, id, span } => {
                let inner_ty = self.infer_expr(inner);
                let result_ty = self.infer_unary_op(*op, &inner_ty, *span);
                self.node_types.insert(*id, result_ty.clone());
                result_ty
            }

            Expr::Call { callee, args, id, span } => {
                let callee_ty = self.infer_expr(callee);
                // Use canonical args (definition order, defaults inserted) when
                // available — every direct call to a known function has them.
                let arg_tys: Vec<Ty> = if let Some(canon) =
                    self.resolve.canonical_call_args.get(id).cloned()
                {
                    self.infer_args(&canon)
                } else {
                    self.infer_args(args)
                };

                let ret_ty = self.fresh_tyvar();
                let expected_fn_ty = Ty::Fn {
                    params: arg_tys.clone(),
                    ret: Box::new(ret_ty.clone()),
                };

                // A friendlier diagnostic when the callee isn't a function or
                // when arg counts disagree — fall back to plain unify otherwise.
                let callee_resolved = self.subst.apply(&callee_ty);
                match &callee_resolved {
                    Ty::Fn { params, .. } if params.len() != arg_tys.len() => {
                        self.errors.push(TypeError::WrongArgumentCount {
                            expected: params.len(),
                            found: arg_tys.len(),
                            span: *span,
                        });
                    }
                    Ty::Int | Ty::Float | Ty::Bool | Ty::Str | Ty::Unit | Ty::ShellOutput => {
                        self.errors.push(TypeError::NotCallable {
                            ty: callee_resolved.clone(),
                            span: *span,
                        });
                    }
                    _ => {
                        self.unify(&callee_ty, &expected_fn_ty, *span);
                    }
                }

                self.node_types.insert(*id, ret_ty.clone());
                ret_ty
            }

            Expr::If { cond, then_branch, else_branch, id, span } => {
                let cond_ty = self.infer_expr(cond);
                self.unify(&Ty::Bool, &cond_ty, cond.span());

                let then_ty = self.infer_expr(then_branch);
                let result_ty = if let Some(else_expr) = else_branch {
                    let else_ty = self.infer_expr(else_expr);
                    self.unify(&then_ty, &else_ty, *span);
                    then_ty
                } else {
                    self.unify(&then_ty, &Ty::Unit, then_branch.span());
                    Ty::Unit
                };

                self.node_types.insert(*id, result_ty.clone());
                result_ty
            }

            Expr::Block { stmts, expr: tail, id, .. } => {
                self.env.push_scope();
                for stmt in stmts {
                    self.check_stmt(stmt);
                }
                let block_ty = if let Some(e) = tail {
                    self.infer_expr(e)
                } else {
                    Ty::Unit
                };
                self.env.pop_scope();
                self.node_types.insert(*id, block_ty.clone());
                block_ty
            }

            Expr::Return { expr: ret_expr, id, span } => {
                let ret_ty = if let Some(e) = ret_expr {
                    self.infer_expr(e)
                } else {
                    Ty::Unit
                };
                if let Some(expected) = self.current_fn_ret.clone() {
                    self.unify(&expected, &ret_ty, *span);
                }
                // `return` diverges — represent as a fresh variable so it
                // unifies with whichever branch type is needed.
                let diverges = self.fresh_tyvar();
                self.node_types.insert(*id, diverges.clone());
                diverges
            }

            Expr::While { cond, body, id, .. } => {
                let cond_ty = self.infer_expr(cond);
                self.unify(&Ty::Bool, &cond_ty, cond.span());
                self.infer_expr(body);
                self.node_types.insert(*id, Ty::Unit);
                Ty::Unit
            }

            Expr::Loop { body, id, .. } => {
                self.infer_expr(body);
                self.node_types.insert(*id, Ty::Unit);
                Ty::Unit
            }

            Expr::Break { id, .. } | Expr::Continue { id, .. } => {
                let diverges = self.fresh_tyvar();
                self.node_types.insert(*id, diverges.clone());
                diverges
            }

            Expr::Closure { params, body, id, .. } => {
                self.env.push_scope();
                let mut aliases = std::mem::take(&mut self.generic_aliases);
                let param_tys: Vec<Ty> = params
                    .iter()
                    .map(|p| self.resolve_type_annotation(&p.ty, &mut aliases))
                    .collect();
                self.generic_aliases = aliases;

                for p in &param_tys {
                    let mut fv = HashSet::new();
                    free_vars_in(p, &self.subst, &mut fv);
                    self.env.pin_monomorphic_vars(fv);
                }
                for (param, ty) in params.iter().zip(param_tys.iter()) {
                    self.env
                        .define(param.name, TypeScheme::monomorphic(ty.clone()));
                }

                let body_ty = self.infer_expr(body);
                self.env.pop_scope();

                let closure_ty = Ty::Fn {
                    params: param_tys,
                    ret: Box::new(body_ty),
                };
                self.node_types.insert(*id, closure_ty.clone());
                closure_ty
            }

            Expr::Shell { parts, id, .. } => {
                for part in parts {
                    if let ShellPart::Interpolated(inner) = part {
                        let inner_ty = self.infer_expr(inner);
                        self.shell_interp_nodes.push((inner.id(), inner.span()));
                        // Don't unify here — Str OR Int is a runtime-safe ad-hoc
                        // overload (the compiler dispatches on the concrete type).
                        // Validate at the post-pass once inference has settled.
                        let _ = inner_ty;
                    }
                }
                self.node_types.insert(*id, Ty::ShellOutput);
                Ty::ShellOutput
            }
        };

        ty
    }

    fn infer_args(&mut self, args: &[NamedArg]) -> Vec<Ty> {
        args.iter().map(|a| self.infer_expr(&a.value)).collect()
    }

    // ------------------------------------------------------------------
    // Operators
    // ------------------------------------------------------------------

    fn infer_binary_op(&mut self, op: BinOp, left: &Ty, right: &Ty, span: Span) -> Ty {
        use BinOp::*;
        match op {
            Add | Sub | Mul | Div | Rem => {
                // Operands must agree.
                self.unify(left, right, span);
                let unified = self.subst.apply(left);
                match unified {
                    Ty::Int | Ty::Float => unified,
                    Ty::Str if matches!(op, Add) => Ty::Str,
                    Ty::Var(_) => {
                        // Defer: we'll catch leftover vars in the post-pass.
                        unified
                    }
                    other => {
                        self.errors.push(TypeError::IncompatibleTypes {
                            operation: format!("{:?}", op),
                            left: other.clone(),
                            right: other,
                            span,
                        });
                        self.fresh_tyvar()
                    }
                }
            }

            Eq | Ne | Lt | Le | Gt | Ge => {
                self.unify(left, right, span);
                Ty::Bool
            }

            And | Or => {
                self.unify(&Ty::Bool, left, span);
                self.unify(&Ty::Bool, right, span);
                Ty::Bool
            }
        }
    }

    fn infer_unary_op(&mut self, op: UnOp, operand: &Ty, span: Span) -> Ty {
        use UnOp::*;
        match op {
            Neg => {
                let unified = self.subst.apply(operand);
                match unified {
                    Ty::Int | Ty::Float => unified,
                    Ty::Var(_) => {
                        // Defer; if it stays a var, we'll default to Int below.
                        unified
                    }
                    other => {
                        self.errors.push(TypeError::IncompatibleTypes {
                            operation: "Neg".to_string(),
                            left: other.clone(),
                            right: other.clone(),
                            span,
                        });
                        other
                    }
                }
            }
            Not => {
                self.unify(&Ty::Bool, operand, span);
                Ty::Bool
            }
        }
    }

    // ------------------------------------------------------------------
    // Unification
    // ------------------------------------------------------------------

    fn unify(&mut self, a: &Ty, b: &Ty, span: Span) {
        if let Err(()) = self.try_unify(a, b) {
            // Surface a structural error if we haven't already pushed a more
            // specific one (e.g. InfiniteType, NotCallable).
            let lhs = self.subst.apply(a);
            let rhs = self.subst.apply(b);
            self.errors.push(TypeError::Mismatch {
                expected: lhs,
                found: rhs,
                span,
            });
        }
    }

    fn try_unify(&mut self, a: &Ty, b: &Ty) -> Result<(), ()> {
        let a = self.subst.apply(a);
        let b = self.subst.apply(b);
        match (a, b) {
            (Ty::Int, Ty::Int)
            | (Ty::Float, Ty::Float)
            | (Ty::Bool, Ty::Bool)
            | (Ty::Str, Ty::Str)
            | (Ty::Unit, Ty::Unit)
            | (Ty::ShellOutput, Ty::ShellOutput) => Ok(()),

            (Ty::Var(v1), Ty::Var(v2)) if v1 == v2 => Ok(()),

            (Ty::Var(v), t) | (t, Ty::Var(v)) => {
                if matches!(&t, Ty::Var(v2) if *v2 == v) {
                    return Ok(());
                }
                if occurs(v, &t, &self.subst) {
                    self.errors.push(TypeError::InfiniteType {
                        var: v,
                        ty: self.subst.apply(&t),
                        span: Span::new(0, 0),
                    });
                    Err(())
                } else {
                    self.subst.extend(v, t);
                    Ok(())
                }
            }

            (
                Ty::Fn { params: p1, ret: r1 },
                Ty::Fn { params: p2, ret: r2 },
            ) => {
                if p1.len() != p2.len() {
                    return Err(());
                }
                for (t1, t2) in p1.iter().zip(p2.iter()) {
                    self.try_unify(t1, t2)?;
                }
                self.try_unify(&r1, &r2)
            }

            _ => Err(()),
        }
    }

    // ------------------------------------------------------------------
    // Generalisation / Instantiation
    // ------------------------------------------------------------------

    fn generalize(&self, ty: &Ty) -> TypeScheme {
        let resolved = self.subst.apply(ty);
        let mut free = HashSet::new();
        free_vars_in(&resolved, &self.subst, &mut free);
        let env_vars = self.env.all_monomorphic_vars();
        let quantified: Vec<TyVar> = free.difference(&env_vars).copied().collect();
        TypeScheme {
            forall: quantified,
            ty: resolved,
        }
    }

    fn instantiate(&mut self, scheme: &TypeScheme) -> Ty {
        if scheme.forall.is_empty() {
            return self.subst.apply(&scheme.ty);
        }
        let mapping: HashMap<TyVar, Ty> = scheme
            .forall
            .iter()
            .map(|v| {
                let fresh = self.fresh_var_only();
                (*v, Ty::Var(fresh))
            })
            .collect();
        rename_vars(&scheme.ty, &mapping)
    }

    // ------------------------------------------------------------------
    // Finalisation
    // ------------------------------------------------------------------

    fn finish(mut self) -> TypeResult {
        // Apply final substitution to every recorded node type. Any node still
        // containing a type variable becomes a CannotInfer error — except
        // for diverging expressions (return/break/continue) whose type is
        // unconstrained on purpose; those are unused if their fresh var never
        // unified with anything concrete, in which case we default them to Unit
        // before emitting an error.
        let mut resolved: HashMap<NodeId, Ty> = HashMap::new();
        let pending: Vec<(NodeId, Ty)> = self
            .node_types
            .iter()
            .map(|(k, v)| (*k, v.clone()))
            .collect();
        for (id, ty) in pending {
            let mut applied = self.subst.apply(&ty);
            if has_type_vars(&applied, &self.subst) {
                // Default unresolved variables that arise from arithmetic or
                // diverging expressions to Int / Unit respectively. This keeps
                // simple programs (e.g. `let x = 1 + 2`) free of spurious
                // CannotInfer errors when the operands are themselves vars
                // chained back to Int via unification.
                applied = default_remaining_vars(&applied);
                if has_type_vars(&applied, &self.subst) {
                    // Shouldn't happen after defaulting, but guard anyway.
                    let span = span_of_node(self.ast, id).unwrap_or(Span::new(0, 0));
                    self.errors.push(TypeError::CannotInfer { span });
                }
            }
            resolved.insert(id, applied);
        }

        // Validate shell interpolation parts — must be Str or Int.
        for (id, span) in &self.shell_interp_nodes {
            if let Some(ty) = resolved.get(id) {
                match ty {
                    Ty::Str | Ty::Int => {}
                    other => {
                        self.errors.push(TypeError::ShellInterpType {
                            found: other.clone(),
                            span: *span,
                        });
                    }
                }
            }
        }

        TypeResult::new(resolved, self.errors)
    }
}

/// Substitutes type variables in `ty` according to `mapping`, leaving any
/// variable not in the map untouched. Used for fresh-renaming a scheme during
/// instantiation.
fn rename_vars(ty: &Ty, mapping: &HashMap<TyVar, Ty>) -> Ty {
    match ty {
        Ty::Var(v) => mapping.get(v).cloned().unwrap_or(Ty::Var(*v)),
        Ty::Fn { params, ret } => Ty::Fn {
            params: params.iter().map(|p| rename_vars(p, mapping)).collect(),
            ret: Box::new(rename_vars(ret, mapping)),
        },
        other => other.clone(),
    }
}

/// Recursively replaces any remaining `Ty::Var` with a sensible default. Used
/// at the end of inference for expressions whose variable never got pinned —
/// arithmetic that's only ever used through an arithmetic-polymorphic path,
/// or diverging branches that flow nowhere.
fn default_remaining_vars(ty: &Ty) -> Ty {
    match ty {
        Ty::Var(_) => Ty::Int,
        Ty::Fn { params, ret } => Ty::Fn {
            params: params.iter().map(default_remaining_vars).collect(),
            ret: Box::new(default_remaining_vars(ret)),
        },
        other => other.clone(),
    }
}

/// Best-effort lookup of an AST node's span. Walks the AST until the node is
/// found; called only on the cold error path.
fn span_of_node(ast: &ParseResult, target: NodeId) -> Option<Span> {
    for item in &ast.items {
        if let Some(s) = span_in_item(item, target) {
            return Some(s);
        }
    }
    None
}

fn span_in_item(item: &Item, target: NodeId) -> Option<Span> {
    if item.id() == target {
        return Some(item.span());
    }
    match item {
        Item::FnDef { body, .. } => span_in_expr(body, target),
        Item::Script { stmt, .. } => span_in_stmt(stmt, target),
    }
}

fn span_in_stmt(stmt: &Stmt, target: NodeId) -> Option<Span> {
    if stmt.id() == target {
        return Some(stmt.span());
    }
    match stmt {
        Stmt::Let { init, .. } => span_in_expr(init, target),
        Stmt::Assign { target: t, value, .. } => {
            span_in_expr(t, target).or_else(|| span_in_expr(value, target))
        }
        Stmt::Expr { expr } => span_in_expr(expr, target),
        Stmt::Require(req) => {
            span_in_expr(&req.expr, target)
                .or_else(|| req.message.as_ref().and_then(|m| span_in_expr(m, target)))
                .or_else(|| req.set_fn.as_ref().and_then(|s| span_in_expr(s, target)))
        }
    }
}

fn span_in_expr(expr: &Expr, target: NodeId) -> Option<Span> {
    if expr.id() == target {
        return Some(expr.span());
    }
    match expr {
        Expr::Literal { .. } | Expr::Variable { .. } | Expr::Break { .. } | Expr::Continue { .. } => None,
        Expr::Binary { left, right, .. } => {
            span_in_expr(left, target).or_else(|| span_in_expr(right, target))
        }
        Expr::Unary { expr, .. } => span_in_expr(expr, target),
        Expr::Call { callee, args, .. } => {
            span_in_expr(callee, target).or_else(|| {
                args.iter()
                    .find_map(|a| span_in_expr(&a.value, target))
            })
        }
        Expr::If { cond, then_branch, else_branch, .. } => span_in_expr(cond, target)
            .or_else(|| span_in_expr(then_branch, target))
            .or_else(|| else_branch.as_ref().and_then(|e| span_in_expr(e, target))),
        Expr::Block { stmts, expr, .. } => stmts
            .iter()
            .find_map(|s| span_in_stmt(s, target))
            .or_else(|| expr.as_ref().and_then(|e| span_in_expr(e, target))),
        Expr::Return { expr, .. } => expr.as_ref().and_then(|e| span_in_expr(e, target)),
        Expr::While { cond, body, .. } => {
            span_in_expr(cond, target).or_else(|| span_in_expr(body, target))
        }
        Expr::Loop { body, .. } => span_in_expr(body, target),
        Expr::Closure { body, .. } => span_in_expr(body, target),
        Expr::Shell { parts, .. } => parts.iter().find_map(|p| match p {
            ShellPart::Interpolated(e) => span_in_expr(e, target),
            ShellPart::Literal(_) => None,
        }),
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use ferric_common::{Interner, NodeId, ParseResult, ResolveResult};

    fn empty_resolve() -> ResolveResult {
        ResolveResult::new(
            HashMap::new(),
            HashMap::new(),
            HashMap::new(),
            HashMap::new(),
            vec![],
        )
    }

    #[test]
    fn unify_primitives_succeeds() {
        let mut infer = TypeInfer {
            ast: &ParseResult::new(vec![], vec![]),
            resolve: &empty_resolve(),
            interner: &Interner::new(),
            next_tyvar: 0,
            subst: Substitution::new(),
            env: InferEnv::new(),
            current_fn_ret: None,
            generic_aliases: HashMap::new(),
            node_types: HashMap::new(),
            shell_interp_nodes: vec![],
            errors: vec![],
        };
        infer.unify(&Ty::Int, &Ty::Int, Span::new(0, 0));
        assert!(infer.errors.is_empty());
    }

    #[test]
    fn unify_var_with_concrete_extends_substitution() {
        let mut infer = TypeInfer {
            ast: &ParseResult::new(vec![], vec![]),
            resolve: &empty_resolve(),
            interner: &Interner::new(),
            next_tyvar: 0,
            subst: Substitution::new(),
            env: InferEnv::new(),
            current_fn_ret: None,
            generic_aliases: HashMap::new(),
            node_types: HashMap::new(),
            shell_interp_nodes: vec![],
            errors: vec![],
        };
        let v = infer.fresh_tyvar();
        infer.unify(&v, &Ty::Int, Span::new(0, 0));
        assert!(infer.errors.is_empty());
        assert_eq!(infer.subst.apply(&v), Ty::Int);
    }

    #[test]
    fn occurs_check_rejects_infinite_type() {
        let mut infer = TypeInfer {
            ast: &ParseResult::new(vec![], vec![]),
            resolve: &empty_resolve(),
            interner: &Interner::new(),
            next_tyvar: 0,
            subst: Substitution::new(),
            env: InferEnv::new(),
            current_fn_ret: None,
            generic_aliases: HashMap::new(),
            node_types: HashMap::new(),
            shell_interp_nodes: vec![],
            errors: vec![],
        };
        let v = infer.fresh_tyvar();
        let inner = match v.clone() {
            Ty::Var(tv) => tv,
            _ => unreachable!(),
        };
        let bad = Ty::Fn { params: vec![v.clone()], ret: Box::new(v.clone()) };
        infer.unify(&Ty::Var(inner), &bad, Span::new(0, 0));
        assert!(matches!(infer.errors.first(), Some(TypeError::InfiniteType { .. })));
    }

    #[test]
    fn instantiate_freshens_quantified_vars() {
        let mut infer = TypeInfer {
            ast: &ParseResult::new(vec![], vec![]),
            resolve: &empty_resolve(),
            interner: &Interner::new(),
            next_tyvar: 0,
            subst: Substitution::new(),
            env: InferEnv::new(),
            current_fn_ret: None,
            generic_aliases: HashMap::new(),
            node_types: HashMap::new(),
            shell_interp_nodes: vec![],
            errors: vec![],
        };
        let alpha = TyVar(99);
        let scheme = TypeScheme {
            forall: vec![alpha],
            ty: Ty::Fn { params: vec![Ty::Var(alpha)], ret: Box::new(Ty::Var(alpha)) },
        };
        let ty1 = infer.instantiate(&scheme);
        let ty2 = infer.instantiate(&scheme);
        assert_ne!(ty1, ty2);
    }

    #[test]
    fn empty_program_typechecks() {
        let ast = ParseResult::new(vec![], vec![]);
        let resolve = empty_resolve();
        let interner = Interner::new();
        let result = typecheck(&ast, &resolve, &interner);
        assert!(!result.has_errors());
    }

    #[test]
    fn node_id_unused() {
        // Quiet the unused-import check on NodeId.
        let _ = NodeId::new(0);
    }
}
