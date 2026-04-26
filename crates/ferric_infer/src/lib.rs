//! # Ferric Type Inference (M3)
//!
//! Replaces `ferric_typecheck` with a Hindley-Milner inference engine using
//! Algorithm J — fresh type variables, unification with the occurs check,
//! and let-generalisation. Public surface is identical to the previous stage:
//! a single `typecheck` entry point taking `(ast, resolve, interner)` and
//! returning a `TypeResult`.

use std::collections::{HashMap, HashSet};

use ferric_common::{
    BinOp, DefId, Expr, ImplMethod, Interner, Item, Literal, MatchArm, NamedArg, NodeId,
    Param, ParseResult, Pattern, RequireStmt, ResolveResult, ShellPart, Span, Stmt, Symbol,
    TraitRegistry, Ty, TyVar, TypeAnnotation, TypeError, TypeResult, TypeScheme,
    UnOp,
};

/// Type-checks (and infers) a parsed AST with resolution information.
///
/// This is the single public entry point for the inference stage. It mirrors
/// the M1 `ferric_typecheck::typecheck` signature so swapping the crate is a
/// one-line change in `main.rs`.
///
/// `registry` carries the program's traits and impls. It is consulted to
/// dispatch method calls and to verify trait bounds on generic functions.
pub fn typecheck(
    ast: &ParseResult,
    resolve: &ResolveResult,
    interner: &Interner,
    registry: &TraitRegistry,
) -> TypeResult {
    let mut infer = TypeInfer::new(ast, resolve, interner, registry);
    infer.infer_program();
    infer.finish()
}

/// Backwards-compatible wrapper used by tests and crates that don't yet
/// thread a registry through. Equivalent to passing an empty registry.
pub fn typecheck_no_traits(
    ast: &ParseResult,
    resolve: &ResolveResult,
    interner: &Interner,
) -> TypeResult {
    let registry = TraitRegistry::new();
    typecheck(ast, resolve, interner, &registry)
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
            Ty::Tuple(elems) => Ty::Tuple(elems.iter().map(|t| self.apply(t)).collect()),
            Ty::Struct { def_id, name, fields } => Ty::Struct {
                def_id: *def_id,
                name: *name,
                fields: fields.iter().map(|(n, t)| (*n, self.apply(t))).collect(),
            },
            Ty::Enum { def_id, name, variants } => Ty::Enum {
                def_id: *def_id,
                name: *name,
                variants: variants
                    .iter()
                    .map(|(n, ts)| (*n, ts.iter().map(|t| self.apply(t)).collect()))
                    .collect(),
            },
            Ty::Array(inner) => Ty::Array(Box::new(self.apply(inner))),
            Ty::Option(inner) => Ty::Option(Box::new(self.apply(inner))),
            Ty::Result(ok, err) => Ty::Result(
                Box::new(self.apply(ok)),
                Box::new(self.apply(err)),
            ),
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
        Ty::Tuple(elems) => elems.iter().any(|t| occurs_raw(var, t)),
        Ty::Struct { fields, .. } => fields.iter().any(|(_, t)| occurs_raw(var, t)),
        Ty::Enum { variants, .. } => variants
            .iter()
            .any(|(_, ts)| ts.iter().any(|t| occurs_raw(var, t))),
        Ty::Array(inner) | Ty::Option(inner) => occurs_raw(var, inner),
        Ty::Result(ok, err) => occurs_raw(var, ok) || occurs_raw(var, err),
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
        Ty::Tuple(elems) => {
            for t in elems {
                free_vars_raw(t, out);
            }
        }
        Ty::Struct { fields, .. } => {
            for (_, t) in fields {
                free_vars_raw(t, out);
            }
        }
        Ty::Enum { variants, .. } => {
            for (_, ts) in variants {
                for t in ts {
                    free_vars_raw(t, out);
                }
            }
        }
        Ty::Array(inner) | Ty::Option(inner) => free_vars_raw(inner, out),
        Ty::Result(ok, err) => {
            free_vars_raw(ok, out);
            free_vars_raw(err, out);
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
// Type-alias registry (M7)
// ============================================================================

/// Per-alias metadata: the parameter list (generic alias names) and the raw
/// inner annotation. The inner is resolved lazily at each use site so that
/// alias-to-alias references don't depend on declaration order.
///
/// **Scaffolding (M7):** populated by `register_type_aliases` once the
/// resolver allocates `DefId`s for type-alias items. Currently no fields are
/// read — the `dead_code` allow is removed when the M7 follow-up wires up the
/// alias-use sites.
#[allow(dead_code)]
#[derive(Debug, Clone)]
struct TypeAliasMeta {
    def_id: DefId,
    /// Generic parameter names declared on the alias (`type Foo<T> = ...`).
    /// Empty for non-generic aliases.
    param_syms: Vec<Symbol>,
    /// Raw inner annotation; resolved fresh at each use site.
    inner_ann: TypeAnnotation,
    opaque: bool,
}

// ============================================================================
// Inferencer
// ============================================================================

struct TypeInfer<'a> {
    ast: &'a ParseResult,
    resolve: &'a ResolveResult,
    interner: &'a Interner,
    registry: &'a TraitRegistry,

    next_tyvar: u32,
    subst: Substitution,
    env: InferEnv,

    /// Type-alias registry. Populated in the pre-pass before any function
    /// body is checked so forward references and out-of-order declarations
    /// resolve consistently.
    ///
    /// Scaffolding (M7): populated by `register_type_aliases`, not yet read
    /// at use sites. Drop the `allow` when the consumer lands.
    #[allow(dead_code)]
    type_aliases: HashMap<Symbol, TypeAliasMeta>,
    /// In-progress aliases for cycle detection during lazy inner resolution.
    #[allow(dead_code)]
    type_alias_resolving: HashSet<Symbol>,

    /// Within the body of the current function, the expected return type.
    /// `None` at the top level (script statements).
    current_fn_ret: Option<Ty>,

    /// Stable mapping from a "generic" type-name symbol (e.g. `T`) to a
    /// fresh type variable, scoped to the function being processed.
    generic_aliases: HashMap<Symbol, TyVar>,

    /// Trait bounds attached to the type variables in scope (one entry per
    /// generic parameter that has any bounds). Read by `MethodCall` to find
    /// a callable trait method when the receiver is a still-unbound type
    /// variable.
    bound_constraints: HashMap<TyVar, Vec<Symbol>>,

    /// Output: every node's resolved type.
    node_types: HashMap<NodeId, Ty>,
    /// Output: each method-call NodeId → resolved impl method DefId.
    method_dispatch: HashMap<NodeId, DefId>,
    /// Pending method-call resolutions: receiver NodeId, method name,
    /// the call's MethodCall NodeId, and source span. Resolved post-pass
    /// after the substitution settles.
    pending_methods: Vec<(NodeId, Symbol, NodeId, Span)>,
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
        registry: &'a TraitRegistry,
    ) -> Self {
        Self {
            ast,
            resolve,
            interner,
            registry,
            next_tyvar: 0,
            subst: Substitution::new(),
            env: InferEnv::new(),
            current_fn_ret: None,
            generic_aliases: HashMap::new(),
            bound_constraints: HashMap::new(),
            type_aliases: HashMap::new(),
            type_alias_resolving: HashSet::new(),
            node_types: HashMap::new(),
            method_dispatch: HashMap::new(),
            pending_methods: Vec::new(),
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

    /// Walk top-level `Item::TypeAlias` declarations and populate
    /// `self.type_aliases`. The map is consulted lazily at each alias use
    /// site so forward references and out-of-order declarations behave
    /// consistently.
    ///
    /// **State note (M7 scaffolding):** the resolver does not yet allocate
    /// a `DefId` for `Item::TypeAlias` — see `Item::TypeAlias(_) => {}` in
    /// `ferric_resolve`. Until that lands, this method walks the AST but
    /// does not insert anything (synthesizing a fake `DefId` would collide
    /// with real ones the resolver allocates). The call site in
    /// `infer_program` and the struct fields are kept so the M7 follow-up
    /// is a single-method change.
    fn register_type_aliases(&mut self) {
        for item in &self.ast.items {
            if let Item::TypeAlias(_alias) = item {
                // TODO(M7): once the resolver allocates `DefId`s for type
                //   aliases (`type_defs` extension), build `TypeAliasMeta`
                //   from `_alias` and insert into `self.type_aliases`.
            }
        }
    }

    fn infer_program(&mut self) {
        // 1. Register native function signatures so calls to stdlib like
        //    `println(s: ...)` can be checked.
        self.register_native_signatures();

        // 2. Register every `type` alias declared in the program. Aliases are
        //    stored as raw `TypeAnnotation`s and resolved lazily at each use
        //    site, so forward and out-of-order references behave consistently.
        self.register_type_aliases();

        // 3. Pre-pass: collect every user-defined function's signature so
        //    forward references and recursion type-check correctly.
        // `Export` is treated as transparent — its inner item participates
        // in the pre-pass exactly like a top-level item.
        let mut fn_aliases: Vec<HashMap<Symbol, TyVar>> = Vec::new();
        for item in &self.ast.items {
            let item = unwrap_export(item);
            if let Item::FnDef { name, type_params, params, ret_ty, .. } = item {
                let mut aliases = HashMap::new();
                // Pre-seed aliases for declared generic parameters so they
                // resolve to a stable TyVar across param/ret types — and so
                // any trait bounds can be attached to that TyVar.
                for tp in type_params {
                    let var = self.fresh_var_only();
                    aliases.insert(tp.name, var);
                    if !tp.bounds.is_empty() {
                        self.bound_constraints
                            .entry(var)
                            .or_default()
                            .extend(tp.bounds.iter().copied());
                    }
                }
                let param_tys: Vec<Ty> = params
                    .iter()
                    .map(|p| self.resolve_type_annotation(&p.ty, &mut aliases))
                    .collect();
                let ret_ty_resolved = self.resolve_type_annotation(ret_ty, &mut aliases);
                let fn_ty = Ty::Fn {
                    params: param_tys,
                    ret: Box::new(ret_ty_resolved),
                };
                // Generic functions: do NOT quantify the type parameters.
                // This sidesteps freshening at call sites so the function's
                // body and call site share the same TyVar, which lets
                // method-call dispatch and bound checking resolve through
                // the global substitution. The trade-off is that a generic
                // function can only be called with one concrete type per
                // type parameter — adequate for M5's one-specialization
                // model.
                let forall = if type_params.is_empty() {
                    aliases.values().copied().collect()
                } else {
                    Vec::new()
                };
                let scheme = TypeScheme { forall, ty: fn_ty };
                self.env.define(*name, scheme);
                fn_aliases.push(aliases);
            }
        }

        // 4. Walk items: check each function body and each script statement.
        // Again, `Export` is unwrapped — its inner item is processed normally.
        let mut fn_idx = 0;
        for item in &self.ast.items {
            let item = unwrap_export(item);
            match item {
                Item::FnDef { id, params, ret_ty, body, .. } => {
                    let aliases = fn_aliases[fn_idx].clone();
                    fn_idx += 1;
                    self.check_fn_def(*id, params, ret_ty, body, aliases);
                }
                Item::StructDef { .. } | Item::EnumDef { .. } | Item::TraitDef { .. } => {
                    // Type-only definitions; bodies (if any) are signatures
                    // only. Trait method signatures are picked up via the
                    // registry, not type-checked here.
                }
                Item::ImplBlock { trait_name, type_name, methods, span, .. } => {
                    // Verify the trait exists. If not, emit an error and skip
                    // the body checks — downstream consumers ignore unknown
                    // impls.
                    if !self.registry.traits.contains_key(trait_name) {
                        self.errors.push(TypeError::ImplOfUnknownTrait {
                            trait_name: *trait_name,
                            span: *span,
                        });
                    }
                    let for_ty = self.resolve_for_type(*type_name);
                    for m in methods {
                        self.check_impl_method(m, &for_ty);
                    }
                }
                Item::Script { stmt, .. } => {
                    self.check_stmt(stmt);
                }
                // M7 Task 4: type aliases are registered in the pre-pass; no
                // body to walk here. Imports surface their bindings via the
                // resolver. `Export` is a transparent wrapper — its inner item
                // is processed in a second walk below.
                Item::Import(_) | Item::TypeAlias(_) | Item::Export(_) => {}
            }
        }
    }

    /// Maps the impl's "for type" name (e.g. `Int`) to a `Ty`. Used to bind
    /// the `self` parameter to the concrete impl receiver type.
    fn resolve_for_type(&mut self, name: Symbol) -> Ty {
        let s = self.interner.resolve(name);
        match s {
            "Int" => Ty::Int,
            "Float" => Ty::Float,
            "Bool" => Ty::Bool,
            "Str" => Ty::Str,
            "Unit" | "" => Ty::Unit,
            "ShellOutput" => Ty::ShellOutput,
            _ => self.lookup_user_type(name).unwrap_or(Ty::Unit),
        }
    }

    /// Type-checks a single impl block method. Treats the method like a
    /// regular function definition but does not generalise (impl methods
    /// are monomorphic in their receiver type — there are no per-method
    /// type parameters in this milestone). The `self` parameter (whether
    /// it carries an explicit `Self` annotation or just the bare name
    /// `self`) is bound to `for_type`.
    fn check_impl_method(&mut self, m: &ImplMethod, for_type: &Ty) {
        let mut aliases: HashMap<Symbol, TyVar> = HashMap::new();
        // Resolve params, but swap the `self` parameter's annotation with
        // the impl's concrete `for_type`.
        let self_sym = self.interner.lookup("self");
        let self_type_name = self.interner.lookup("Self");
        let param_tys: Vec<Ty> = m
            .params
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let is_self = i == 0
                    && (Some(p.name) == self_sym
                        || matches!(&p.ty, TypeAnnotation::Named(sym) if Some(*sym) == self_type_name)
                        || matches!(&p.ty, TypeAnnotation::Named(sym) if Some(*sym) == self_sym));
                if is_self {
                    for_type.clone()
                } else {
                    self.resolve_type_annotation(&p.ty, &mut aliases)
                }
            })
            .collect();
        let ret_ty_resolved = self.resolve_type_annotation(&m.ret_ty, &mut aliases);

        for (param, declared) in m.params.iter().zip(param_tys.iter()) {
            if let Some(default) = &param.default {
                let default_ty = self.infer_expr(default);
                self.unify(declared, &default_ty, default.span());
            }
        }

        self.env.push_scope();
        for p in &param_tys {
            let mut fv = HashSet::new();
            free_vars_in(p, &self.subst, &mut fv);
            self.env.pin_monomorphic_vars(fv);
        }
        for (param, declared) in m.params.iter().zip(param_tys.iter()) {
            self.env
                .define(param.name, TypeScheme::monomorphic(declared.clone()));
        }

        let prev_ret = self.current_fn_ret.take();
        self.current_fn_ret = Some(ret_ty_resolved.clone());
        let prev_aliases = std::mem::replace(&mut self.generic_aliases, aliases);

        let body_ty = self.infer_expr(&m.body);
        self.unify(&ret_ty_resolved, &body_ty, m.body.span());

        self.generic_aliases = prev_aliases;
        self.current_fn_ret = prev_ret;
        self.env.pop_scope();

        let fn_ty = Ty::Fn {
            params: param_tys,
            ret: Box::new(ret_ty_resolved),
        };
        self.node_types.insert(m.id, fn_ty);
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
            // M6: string / math / io
            ("str_len", &[Ty::Str], Ty::Int),
            ("str_trim", &[Ty::Str], Ty::Str),
            ("str_contains", &[Ty::Str, Ty::Str], Ty::Bool),
            ("str_starts_with", &[Ty::Str, Ty::Str], Ty::Bool),
            ("str_parse_int", &[Ty::Str], Ty::Int),
            ("str_split", &[Ty::Str, Ty::Str], Ty::Array(Box::new(Ty::Str))),
            ("abs", &[Ty::Int], Ty::Int),
            ("min", &[Ty::Int, Ty::Int], Ty::Int),
            ("max", &[Ty::Int, Ty::Int], Ty::Int),
            ("sqrt", &[Ty::Float], Ty::Float),
            ("pow", &[Ty::Float, Ty::Float], Ty::Float),
            ("floor", &[Ty::Float], Ty::Int),
            ("ceil", &[Ty::Float], Ty::Int),
            ("read_line", &[], Ty::Str),
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

        // Polymorphic natives: `array_len(arr: [T]) -> Int` is generic in T.
        // Build a type scheme with one quantified TyVar so each call site
        // instantiates a fresh element type.
        if let Some(sym) = self.interner.lookup("array_len") {
            let elem_var = TyVar(self.next_tyvar);
            self.next_tyvar += 1;
            let fn_ty = Ty::Fn {
                params: vec![Ty::Array(Box::new(Ty::Var(elem_var)))],
                ret: Box::new(Ty::Int),
            };
            self.env
                .define(sym, TypeScheme { forall: vec![elem_var], ty: fn_ty });
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
                        // Look up a user-defined struct or enum.
                        if let Some(ty) = self.lookup_user_type(*sym) {
                            return ty;
                        }
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
            TypeAnnotation::Array(inner) => {
                Ty::Array(Box::new(self.resolve_type_annotation(inner, aliases)))
            }
            TypeAnnotation::Generic { head, args } => {
                let name = self.interner.resolve(*head);
                match (name, args.as_slice()) {
                    ("Option", [inner]) => Ty::Option(Box::new(
                        self.resolve_type_annotation(inner, aliases),
                    )),
                    ("Result", [ok, err]) => Ty::Result(
                        Box::new(self.resolve_type_annotation(ok, aliases)),
                        Box::new(self.resolve_type_annotation(err, aliases)),
                    ),
                    _ => self.fresh_tyvar(),
                }
            }
            TypeAnnotation::Infer => self.fresh_tyvar(),
        }
    }

    /// Constructs a `Ty::Struct` or `Ty::Enum` for a user-defined type symbol,
    /// or `None` if the symbol does not name a known user-defined type.
    fn lookup_user_type(&mut self, name: Symbol) -> Option<Ty> {
        let def_id = *self.resolve.type_defs.get(&name)?;
        if let Some(fields) = self.resolve.struct_fields.get(&def_id).cloned() {
            let mut tmp = HashMap::new();
            let resolved: Vec<(Symbol, Ty)> = fields
                .iter()
                .map(|(n, ann)| (*n, self.resolve_type_annotation(ann, &mut tmp)))
                .collect();
            return Some(Ty::Struct {
                def_id,
                name,
                fields: resolved,
            });
        }
        if let Some(variants) = self.resolve.enum_variants.get(&def_id).cloned() {
            let mut tmp = HashMap::new();
            let resolved: Vec<(Symbol, Vec<Ty>)> = variants
                .iter()
                .map(|(vname, payload)| {
                    let tys = payload
                        .iter()
                        .map(|ann| self.resolve_type_annotation(ann, &mut tmp))
                        .collect();
                    (*vname, tys)
                })
                .collect();
            return Some(Ty::Enum {
                def_id,
                name,
                variants: resolved,
            });
        }
        None
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
            Stmt::For { var, var_id, iter, body, span, .. } => {
                let iter_ty = self.infer_expr(iter);
                let elem_ty = self.fresh_tyvar();
                self.unify(&iter_ty, &Ty::Array(Box::new(elem_ty.clone())), *span);

                let resolved_elem = self.subst.apply(&elem_ty);
                self.node_types.insert(*var_id, resolved_elem.clone());

                self.env.push_scope();
                self.env
                    .define(*var, TypeScheme::monomorphic(resolved_elem));
                let _ = self.infer_expr(body);
                self.env.pop_scope();
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

                // If the callee is a direct reference to a generic function,
                // verify that any concrete type substituted for a bounded
                // type parameter implements every required trait. Run this
                // check after unification so the substitution is up to date.
                if let Expr::Variable { name, .. } = callee.as_ref() {
                    self.check_call_bounds(*name, *span);
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

            Expr::StructLit { name, fields, id, span } => {
                let struct_ty = self
                    .lookup_user_type(*name)
                    .filter(|t| matches!(t, Ty::Struct { .. }))
                    .unwrap_or_else(|| {
                        // Resolver should have already reported UndefinedType.
                        // Record a fresh var so cascading errors are minimal.
                        self.fresh_tyvar()
                    });

                if let Ty::Struct { fields: declared, name: sname, .. } = &struct_ty {
                    let declared = declared.clone();
                    let sname = *sname;
                    for (fname, fexpr) in fields {
                        let expr_ty = self.infer_expr(fexpr);
                        if let Some((_, decl_ty)) = declared.iter().find(|(n, _)| *n == *fname)
                        {
                            if self.try_unify(decl_ty, &expr_ty).is_err() {
                                let expected = self.subst.apply(decl_ty);
                                let found = self.subst.apply(&expr_ty);
                                self.errors.push(TypeError::FieldTypeMismatch {
                                    struct_name: sname,
                                    field: *fname,
                                    expected,
                                    found,
                                    span: fexpr.span(),
                                });
                            }
                        }
                    }
                } else {
                    // Still walk the field expressions so any nested errors are reported.
                    for (_, fexpr) in fields {
                        self.infer_expr(fexpr);
                    }
                    let _ = span;
                }

                self.node_types.insert(*id, struct_ty.clone());
                struct_ty
            }

            Expr::FieldAccess { expr, field, id, span } => {
                let recv_ty = self.infer_expr(expr);
                let resolved = self.subst.apply(&recv_ty);
                let result_ty = match &resolved {
                    Ty::Struct { fields, .. } => {
                        match fields.iter().find(|(n, _)| *n == *field) {
                            Some((_, t)) => t.clone(),
                            None => {
                                self.errors.push(TypeError::NoSuchField {
                                    ty: resolved.clone(),
                                    field: *field,
                                    span: *span,
                                });
                                self.fresh_tyvar()
                            }
                        }
                    }
                    Ty::Var(_) => {
                        // Could not narrow yet; emit CannotInfer at the end.
                        self.fresh_tyvar()
                    }
                    other => {
                        self.errors.push(TypeError::NotAStruct {
                            ty: other.clone(),
                            span: *span,
                        });
                        self.fresh_tyvar()
                    }
                };
                self.node_types.insert(*id, result_ty.clone());
                result_ty
            }

            Expr::Match { scrutinee, arms, id, span } => {
                let scrutinee_ty = self.infer_expr(scrutinee);

                let result_ty = self.fresh_tyvar();
                if arms.is_empty() {
                    self.node_types.insert(*id, result_ty.clone());
                    return result_ty;
                }
                for arm in arms {
                    self.check_match_arm(arm, &scrutinee_ty, &result_ty);
                }
                let _ = span;
                self.node_types.insert(*id, result_ty.clone());
                result_ty
            }

            Expr::Tuple { elements, id, .. } => {
                let elem_tys: Vec<Ty> = elements.iter().map(|e| self.infer_expr(e)).collect();
                let tuple_ty = Ty::Tuple(elem_tys);
                self.node_types.insert(*id, tuple_ty.clone());
                tuple_ty
            }

            Expr::MethodCall { receiver, method, args, id, span } => {
                let receiver_ty = self.infer_expr(receiver);

                // Type-check arg expressions up front so any errors land
                // even if dispatch resolution fails.
                let arg_tys: Vec<Ty> = args.iter().map(|a| self.infer_expr(&a.value)).collect();

                // Try to resolve dispatch immediately if the receiver type
                // is concrete; otherwise defer to the post-pass.
                let resolved = self.subst.apply(&receiver_ty);
                let mut method_sig: Option<(Vec<Ty>, Ty)> = None;

                // 1) Concrete receiver: look up an impl directly.
                if ferric_common::ImplTy::from_ty(&resolved).is_some() {
                    if let Some((trait_name, def_id)) =
                        self.registry.find_method(&resolved, *method)
                    {
                        self.method_dispatch.insert(*id, def_id);
                        if let Some(trait_def) = self.registry.traits.get(&trait_name) {
                            if let Some(sig) = trait_def.methods.get(method) {
                                method_sig = Some((sig.params.clone(), sig.ret.clone()));
                            }
                        }
                    }
                }

                // 2) Receiver is a type variable bound by a trait — use the
                //    trait's method signature for type-checking; dispatch is
                //    resolved monomorphically at compile time.
                if method_sig.is_none() {
                    if let Ty::Var(v) = &resolved {
                        if let Some(bounds) = self.bound_constraints.get(v).cloned() {
                            for bound in &bounds {
                                if let Some(trait_def) = self.registry.traits.get(bound) {
                                    if let Some(sig) = trait_def.methods.get(method) {
                                        method_sig = Some((sig.params.clone(), sig.ret.clone()));
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }

                // Always schedule a post-pass dispatch attempt for
                // method_dispatch resolution. The post-pass also reports
                // NoSuchMethod if the receiver type stays unbound.
                if !self.method_dispatch.contains_key(id) {
                    self.pending_methods
                        .push((receiver.id(), *method, *id, *span));
                }

                let ret_ty = if let Some((sig_params, sig_ret)) = method_sig {
                    // sig_params[0] is the trait-method's `self`; we don't
                    // unify it (the trait registry uses a sentinel TyVar(0)
                    // which would collide with the inferer's allocations).
                    // Match argument count and unify the remaining params.
                    let rest = if sig_params.is_empty() {
                        &[][..]
                    } else {
                        &sig_params[1..]
                    };
                    if rest.len() == arg_tys.len() {
                        for (sig_t, arg_t) in rest.iter().zip(arg_tys.iter()) {
                            self.unify(sig_t, arg_t, *span);
                        }
                    } else {
                        self.errors.push(TypeError::WrongArgumentCount {
                            expected: rest.len(),
                            found: arg_tys.len(),
                            span: *span,
                        });
                    }
                    sig_ret
                } else {
                    // No signature available. The post-pass will report
                    // NoSuchMethod if the receiver remains unbound.
                    self.fresh_tyvar()
                };

                self.node_types.insert(*id, ret_ty.clone());
                ret_ty
            }

            Expr::VariantCtor { enum_name, variant, args, id, span } => {
                let enum_ty = self
                    .lookup_user_type(*enum_name)
                    .filter(|t| matches!(t, Ty::Enum { .. }))
                    .unwrap_or_else(|| self.fresh_tyvar());
                if let Ty::Enum { variants, .. } = &enum_ty {
                    let variants = variants.clone();
                    if let Some((_, payload_tys)) = variants.iter().find(|(n, _)| *n == *variant) {
                        if payload_tys.len() == args.len() {
                            for (arg, expected) in args.iter().zip(payload_tys.iter()) {
                                let arg_ty = self.infer_expr(arg);
                                self.unify(expected, &arg_ty, arg.span());
                            }
                        } else {
                            for arg in args {
                                self.infer_expr(arg);
                            }
                        }
                    } else {
                        for arg in args {
                            self.infer_expr(arg);
                        }
                    }
                } else {
                    for arg in args {
                        self.infer_expr(arg);
                    }
                }
                let _ = span;
                self.node_types.insert(*id, enum_ty.clone());
                enum_ty
            }

            Expr::ArrayLit { elements, id, span } => {
                let elem_ty = if let Some(first) = elements.first() {
                    let first_ty = self.infer_expr(first);
                    for e in &elements[1..] {
                        let t = self.infer_expr(e);
                        self.unify(&first_ty, &t, e.span());
                    }
                    first_ty
                } else {
                    self.fresh_tyvar()
                };
                let array_ty = Ty::Array(Box::new(elem_ty));
                self.node_types.insert(*id, array_ty.clone());
                let _ = span;
                array_ty
            }

            Expr::Index { array, index, id, span } => {
                let array_ty = self.infer_expr(array);
                let index_ty = self.infer_expr(index);
                self.unify(&index_ty, &Ty::Int, index.span());

                let elem_ty = self.fresh_tyvar();
                self.unify(&array_ty, &Ty::Array(Box::new(elem_ty.clone())), *span);
                let resolved = self.subst.apply(&elem_ty);
                self.node_types.insert(*id, resolved.clone());
                resolved
            }
            // Cast expressions are wired into the type checker in M7 Task 4.
            // For now, treat them as identity: the result type is the inner
            // expression's type. Real wrap/unwrap logic against `Ty::Opaque`
            // arrives with the type-alias registry.
            Expr::Cast(c) => {
                let inner = self.infer_expr(&c.expr);
                self.node_types.insert(c.id, inner.clone());
                inner
            }
        };

        ty
    }

    /// Type-checks a single match arm: pattern must match scrutinee, body
    /// type must unify with the overall match result type.
    fn check_match_arm(&mut self, arm: &MatchArm, scrutinee_ty: &Ty, result_ty: &Ty) {
        self.env.push_scope();
        self.check_pattern(&arm.pattern, scrutinee_ty);
        let body_ty = self.infer_expr(&arm.body);
        self.unify(result_ty, &body_ty, arm.body.span());
        self.env.pop_scope();
    }

    /// Type-checks a pattern against the type of the scrutinee. Each Variable
    /// pattern adds a binding to the current scope.
    fn check_pattern(&mut self, pattern: &Pattern, scrutinee_ty: &Ty) {
        match pattern {
            Pattern::Wildcard { .. } => {}
            Pattern::Variable { name, .. } => {
                self.env
                    .define(*name, TypeScheme::monomorphic(scrutinee_ty.clone()));
            }
            Pattern::Literal { value, span } => {
                let lit_ty = match value {
                    Literal::Int(_) => Ty::Int,
                    Literal::Float(_) => Ty::Float,
                    Literal::Str(_) => Ty::Str,
                    Literal::Bool(_) => Ty::Bool,
                    Literal::Unit => Ty::Unit,
                };
                self.unify(scrutinee_ty, &lit_ty, *span);
            }
            Pattern::Tuple { patterns, span } => {
                let elem_tys: Vec<Ty> =
                    (0..patterns.len()).map(|_| self.fresh_tyvar()).collect();
                let tuple_ty = Ty::Tuple(elem_tys.clone());
                self.unify(scrutinee_ty, &tuple_ty, *span);
                for (sub, ty) in patterns.iter().zip(elem_tys.iter()) {
                    self.check_pattern(sub, ty);
                }
            }
            Pattern::Struct { name, fields, span } => {
                let struct_ty = self
                    .lookup_user_type(*name)
                    .filter(|t| matches!(t, Ty::Struct { .. }))
                    .unwrap_or_else(|| self.fresh_tyvar());
                self.unify(scrutinee_ty, &struct_ty, *span);
                if let Ty::Struct { fields: declared, .. } = &struct_ty {
                    let declared = declared.clone();
                    for (fname, fpat) in fields {
                        let field_ty = declared
                            .iter()
                            .find(|(n, _)| *n == *fname)
                            .map(|(_, t)| t.clone())
                            .unwrap_or_else(|| self.fresh_tyvar());
                        self.check_pattern(fpat, &field_ty);
                    }
                } else {
                    for (_, fpat) in fields {
                        let any = self.fresh_tyvar();
                        self.check_pattern(fpat, &any);
                    }
                }
            }
            Pattern::Variant {
                enum_name,
                variant,
                patterns,
                span,
            } => {
                let enum_ty = self
                    .lookup_user_type(*enum_name)
                    .filter(|t| matches!(t, Ty::Enum { .. }))
                    .unwrap_or_else(|| self.fresh_tyvar());
                self.unify(scrutinee_ty, &enum_ty, *span);
                if let Ty::Enum { variants, .. } = &enum_ty {
                    let variants = variants.clone();
                    if let Some((_, payload)) = variants.iter().find(|(n, _)| *n == *variant) {
                        if payload.len() == patterns.len() {
                            for (sub, ty) in patterns.iter().zip(payload.iter()) {
                                self.check_pattern(sub, ty);
                            }
                            return;
                        }
                    }
                }
                // Fallback when the variant is unknown or arity mismatches:
                // still walk subpatterns so binding sites are introduced.
                for sub in patterns {
                    let any = self.fresh_tyvar();
                    self.check_pattern(sub, &any);
                }
            }
        }
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

            (Ty::Tuple(a), Ty::Tuple(b)) => {
                if a.len() != b.len() {
                    return Err(());
                }
                for (t1, t2) in a.iter().zip(b.iter()) {
                    self.try_unify(t1, t2)?;
                }
                Ok(())
            }

            (Ty::Struct { def_id: d1, .. }, Ty::Struct { def_id: d2, .. }) => {
                if d1 == d2 {
                    Ok(())
                } else {
                    Err(())
                }
            }

            (Ty::Enum { def_id: d1, .. }, Ty::Enum { def_id: d2, .. }) => {
                if d1 == d2 {
                    Ok(())
                } else {
                    Err(())
                }
            }

            (Ty::Array(a), Ty::Array(b)) => self.try_unify(&a, &b),
            (Ty::Option(a), Ty::Option(b)) => self.try_unify(&a, &b),
            (Ty::Result(a_ok, a_err), Ty::Result(b_ok, b_err)) => {
                self.try_unify(&a_ok, &b_ok)?;
                self.try_unify(&a_err, &b_err)
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

    /// At a Call site, verify that any concrete type substituted for a
    /// generic parameter satisfies its declared trait bounds. Walks the
    /// AST to find the function definition by name.
    fn check_call_bounds(&mut self, fn_name: Symbol, span: Span) {
        let bounds_to_check: Vec<(Symbol, Vec<Symbol>)> = self
            .ast
            .items
            .iter()
            .find_map(|item| match item {
                Item::FnDef { name, type_params, .. } if *name == fn_name => Some(
                    type_params
                        .iter()
                        .filter(|tp| !tp.bounds.is_empty())
                        .map(|tp| (tp.name, tp.bounds.clone()))
                        .collect(),
                ),
                _ => None,
            })
            .unwrap_or_default();

        if bounds_to_check.is_empty() {
            return;
        }

        // The pre-pass stored the function's aliases in `fn_aliases` but we
        // don't keep that around. Instead, look up the scheme to recover
        // each generic param's TyVar position. Since the scheme stores the
        // function type with TyVars, we walk it.
        let scheme = match self.env.lookup(fn_name).cloned() {
            Some(s) => s,
            None => return,
        };
        let fn_ty = scheme.ty;
        let param_tys = match &fn_ty {
            Ty::Fn { params, .. } => params.clone(),
            _ => return,
        };

        // Map each generic-param Symbol to its TyVar by re-resolving the
        // declared param types of the FnDef. We rely on the original type
        // annotations being identifiers like "T".
        let fn_def = self.ast.items.iter().find_map(|item| match item {
            Item::FnDef { name, params, .. } if *name == fn_name => Some(params.clone()),
            _ => None,
        });
        let formal_params = match fn_def {
            Some(p) => p,
            None => return,
        };

        let mut name_to_var: HashMap<Symbol, TyVar> = HashMap::new();
        for (formal, ty) in formal_params.iter().zip(param_tys.iter()) {
            // Generic-bound checking only meaningful for `Named` annotations
            // (the parameter is the generic name, e.g. `T`). Other forms
            // (Array, Option, Result, Infer) don't introduce new generics.
            let sym = match &formal.ty {
                TypeAnnotation::Named(sym) => *sym,
                _ => continue,
            };
            if let Ty::Var(v) = ty {
                name_to_var.insert(sym, *v);
            }
        }

        for (param_name, bounds) in bounds_to_check {
            let v = match name_to_var.get(&param_name) {
                Some(v) => *v,
                None => continue,
            };
            let resolved = self.subst.apply(&Ty::Var(v));
            // Skip if not yet concrete — the post-pass will catch unresolved
            // type vars elsewhere.
            if matches!(&resolved, Ty::Var(_)) {
                continue;
            }
            for bound in &bounds {
                if !self.registry.has_impl(*bound, &resolved) {
                    self.errors.push(TypeError::TraitBoundNotSatisfied {
                        type_param: param_name,
                        bound: *bound,
                        ty: resolved.clone(),
                        span,
                    });
                }
            }
        }
    }

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

        // Late-bound method dispatch. For any method call whose receiver
        // type wasn't concrete during inference, retry resolution now that
        // the substitution has settled and defaults have been applied.
        for (recv_id, method, call_id, span) in &self.pending_methods {
            if self.method_dispatch.contains_key(call_id) {
                continue;
            }
            let recv_ty = resolved
                .get(recv_id)
                .cloned()
                .unwrap_or(Ty::Unit);
            if let Some((_, def_id)) = self.registry.find_method(&recv_ty, *method) {
                self.method_dispatch.insert(*call_id, def_id);
            } else if !matches!(recv_ty, Ty::Var(_)) {
                self.errors.push(TypeError::NoSuchMethod {
                    ty: recv_ty,
                    method: *method,
                    span: *span,
                });
            }
        }

        let mut result = TypeResult::new(resolved, self.errors);
        result.method_dispatch = self.method_dispatch;
        result
    }
}

/// Returns the inner item of an `Item::Export(...)`, or `item` itself when
/// it is not an export. The type checker treats `export` as transparent —
/// the export modifier is purely a module-system concern (Task 3).
fn unwrap_export(item: &Item) -> &Item {
    match item {
        Item::Export(decl) => decl.item.as_ref(),
        other => other,
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
        Ty::Tuple(elems) => Ty::Tuple(elems.iter().map(|t| rename_vars(t, mapping)).collect()),
        Ty::Struct { def_id, name, fields } => Ty::Struct {
            def_id: *def_id,
            name: *name,
            fields: fields
                .iter()
                .map(|(n, t)| (*n, rename_vars(t, mapping)))
                .collect(),
        },
        Ty::Enum { def_id, name, variants } => Ty::Enum {
            def_id: *def_id,
            name: *name,
            variants: variants
                .iter()
                .map(|(n, ts)| (*n, ts.iter().map(|t| rename_vars(t, mapping)).collect()))
                .collect(),
        },
        Ty::Array(inner) => Ty::Array(Box::new(rename_vars(inner, mapping))),
        Ty::Option(inner) => Ty::Option(Box::new(rename_vars(inner, mapping))),
        Ty::Result(ok, err) => Ty::Result(
            Box::new(rename_vars(ok, mapping)),
            Box::new(rename_vars(err, mapping)),
        ),
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
        Ty::Tuple(elems) => Ty::Tuple(elems.iter().map(default_remaining_vars).collect()),
        Ty::Struct { def_id, name, fields } => Ty::Struct {
            def_id: *def_id,
            name: *name,
            fields: fields
                .iter()
                .map(|(n, t)| (*n, default_remaining_vars(t)))
                .collect(),
        },
        Ty::Enum { def_id, name, variants } => Ty::Enum {
            def_id: *def_id,
            name: *name,
            variants: variants
                .iter()
                .map(|(n, ts)| (*n, ts.iter().map(default_remaining_vars).collect()))
                .collect(),
        },
        Ty::Array(inner) => Ty::Array(Box::new(default_remaining_vars(inner))),
        Ty::Option(inner) => Ty::Option(Box::new(default_remaining_vars(inner))),
        Ty::Result(ok, err) => Ty::Result(
            Box::new(default_remaining_vars(ok)),
            Box::new(default_remaining_vars(err)),
        ),
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
        Item::ImplBlock { methods, .. } => methods.iter().find_map(|m| {
            if m.id == target {
                return Some(m.span);
            }
            span_in_expr(&m.body, target)
        }),
        Item::StructDef { .. } | Item::EnumDef { .. } | Item::TraitDef { .. } => None,
        Item::Export(decl) => span_in_item(&decl.item, target),
        Item::Import(_) | Item::TypeAlias(_) => None,
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
        Stmt::For { iter, body, .. } => {
            span_in_expr(iter, target).or_else(|| span_in_expr(body, target))
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
        Expr::StructLit { fields, .. } => {
            fields.iter().find_map(|(_, e)| span_in_expr(e, target))
        }
        Expr::FieldAccess { expr, .. } => span_in_expr(expr, target),
        Expr::Match { scrutinee, arms, .. } => span_in_expr(scrutinee, target).or_else(|| {
            arms.iter().find_map(|arm| span_in_expr(&arm.body, target))
        }),
        Expr::Tuple { elements, .. } => elements.iter().find_map(|e| span_in_expr(e, target)),
        Expr::VariantCtor { args, .. } => args.iter().find_map(|e| span_in_expr(e, target)),
        Expr::MethodCall { receiver, args, .. } => span_in_expr(receiver, target)
            .or_else(|| args.iter().find_map(|a| span_in_expr(&a.value, target))),
        Expr::ArrayLit { elements, .. } => {
            elements.iter().find_map(|e| span_in_expr(e, target))
        }
        Expr::Index { array, index, .. } => {
            span_in_expr(array, target).or_else(|| span_in_expr(index, target))
        }
        Expr::Cast(c) => span_in_expr(&c.expr, target),
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use ferric_common::{Interner, NodeId, ParseResult, ResolveResult, TraitRegistry};

    fn empty_resolve() -> ResolveResult {
        ResolveResult::new(
            HashMap::new(),
            HashMap::new(),
            HashMap::new(),
            HashMap::new(),
            vec![],
        )
    }

    fn empty_registry() -> TraitRegistry {
        TraitRegistry::new()
    }

    #[test]
    fn unify_primitives_succeeds() {
        let registry = empty_registry();
        let mut infer = TypeInfer {
            ast: &ParseResult::new(vec![], vec![]),
            resolve: &empty_resolve(),
            interner: &Interner::new(),
            registry: &registry,
            next_tyvar: 0,
            subst: Substitution::new(),
            env: InferEnv::new(),
            current_fn_ret: None,
            generic_aliases: HashMap::new(),
            bound_constraints: HashMap::new(),
            type_aliases: HashMap::new(),
            type_alias_resolving: HashSet::new(),
            node_types: HashMap::new(),
            method_dispatch: HashMap::new(),
            pending_methods: Vec::new(),
            shell_interp_nodes: vec![],
            errors: vec![],
        };
        infer.unify(&Ty::Int, &Ty::Int, Span::new(0, 0));
        assert!(infer.errors.is_empty());
    }

    #[test]
    fn unify_var_with_concrete_extends_substitution() {
        let registry = empty_registry();
        let mut infer = TypeInfer {
            ast: &ParseResult::new(vec![], vec![]),
            resolve: &empty_resolve(),
            interner: &Interner::new(),
            registry: &registry,
            next_tyvar: 0,
            subst: Substitution::new(),
            env: InferEnv::new(),
            current_fn_ret: None,
            generic_aliases: HashMap::new(),
            bound_constraints: HashMap::new(),
            type_aliases: HashMap::new(),
            type_alias_resolving: HashSet::new(),
            node_types: HashMap::new(),
            method_dispatch: HashMap::new(),
            pending_methods: Vec::new(),
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
        let registry = empty_registry();
        let mut infer = TypeInfer {
            ast: &ParseResult::new(vec![], vec![]),
            resolve: &empty_resolve(),
            interner: &Interner::new(),
            registry: &registry,
            next_tyvar: 0,
            subst: Substitution::new(),
            env: InferEnv::new(),
            current_fn_ret: None,
            generic_aliases: HashMap::new(),
            bound_constraints: HashMap::new(),
            type_aliases: HashMap::new(),
            type_alias_resolving: HashSet::new(),
            node_types: HashMap::new(),
            method_dispatch: HashMap::new(),
            pending_methods: Vec::new(),
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
        let registry = empty_registry();
        let mut infer = TypeInfer {
            ast: &ParseResult::new(vec![], vec![]),
            resolve: &empty_resolve(),
            interner: &Interner::new(),
            registry: &registry,
            next_tyvar: 0,
            subst: Substitution::new(),
            env: InferEnv::new(),
            current_fn_ret: None,
            generic_aliases: HashMap::new(),
            bound_constraints: HashMap::new(),
            type_aliases: HashMap::new(),
            type_alias_resolving: HashSet::new(),
            node_types: HashMap::new(),
            method_dispatch: HashMap::new(),
            pending_methods: Vec::new(),
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
        let registry = empty_registry();
        let result = typecheck(&ast, &resolve, &interner, &registry);
        assert!(!result.has_errors());
    }

    #[test]
    fn node_id_unused() {
        // Quiet the unused-import check on NodeId.
        let _ = NodeId::new(0);
    }
}
