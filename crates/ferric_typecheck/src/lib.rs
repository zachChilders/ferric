//! # Ferric Type Checker (M1 Implementation)
//!
//! This is the M1 baseline type checker implementation. It uses a simple
//! recursive algorithm with `Ty::Unknown` as an escape hatch for features
//! not yet fully implemented.
//!
//! **IMPORTANT**: This entire implementation will be replaced in M3 with
//! a full Hindley-Milner type inference engine. Keep it simple!

use std::collections::HashMap;
use ferric_common::{
    ParseResult, ResolveResult, TypeResult, Item, Expr, Stmt, Literal,
    BinOp, UnOp, TypeAnnotation, NodeId, DefId, Ty, TypeError, Span, Interner, Symbol,
};

/// Type environment for tracking types of definitions.
///
/// For M1, we maintain both DefId-based and Symbol-based mappings
/// to work around the fact that we don't have a direct mapping from
/// Let statement NodeIds to their DefIds.
#[derive(Debug, Clone)]
struct TypeEnv {
    /// Maps DefId to its type
    def_types: HashMap<DefId, Ty>,
    /// Maps Symbol (name) to DefId for lookup
    /// This is populated as we encounter definitions
    name_to_def: HashMap<Symbol, DefId>,
}

impl TypeEnv {
    /// Creates a new empty type environment.
    fn new() -> Self {
        Self {
            def_types: HashMap::new(),
            name_to_def: HashMap::new(),
        }
    }

    /// Defines a type for a given name and DefId.
    fn define_name(&mut self, name: Symbol, def_id: DefId, ty: Ty) {
        self.name_to_def.insert(name, def_id);
        self.def_types.insert(def_id, ty);
    }

    /// Defines a type for a given DefId (when we already have the DefId).
    #[allow(dead_code)]
    fn define(&mut self, def_id: DefId, ty: Ty) {
        self.def_types.insert(def_id, ty);
    }

    /// Looks up the type of a DefId.
    fn lookup(&self, def_id: DefId) -> Option<&Ty> {
        self.def_types.get(&def_id)
    }

    /// Looks up a DefId by name.
    fn lookup_name(&self, name: Symbol) -> Option<DefId> {
        self.name_to_def.get(&name).copied()
    }
}

/// The type checker.
struct TypeChecker<'a> {
    /// The AST to type-check
    ast: &'a ParseResult,
    /// Resolution information from the resolver
    resolve: &'a ResolveResult,
    /// String interner for looking up type names
    interner: &'a Interner,
    /// Type environment
    env: TypeEnv,
    /// Return type of the current function (for checking return statements)
    current_fn_ret: Option<Ty>,

    // Output
    /// Maps NodeId to its type
    node_types: HashMap<NodeId, Ty>,
    /// Type errors encountered
    errors: Vec<TypeError>,
}

impl<'a> TypeChecker<'a> {
    /// Creates a new type checker.
    fn new(ast: &'a ParseResult, resolve: &'a ResolveResult, interner: &'a Interner) -> Self {
        Self {
            ast,
            resolve,
            interner,
            env: TypeEnv::new(),
            current_fn_ret: None,
            node_types: HashMap::new(),
            errors: Vec::new(),
        }
    }

    /// Type-checks all items in the AST.
    fn check_all(&mut self) {
        // First pass: collect function signatures
        // For M1, we use name-based lookup which is simpler
        for item in &self.ast.items {
            if let Item::FnDef { id, name, params, ret_ty, .. } = item {
                // Build function type
                let param_types: Vec<Ty> = params
                    .iter()
                    .map(|(_, ty)| self.resolve_type_annotation(ty))
                    .collect();
                let ret_type = self.resolve_type_annotation(ret_ty);

                let fn_ty = Ty::Fn {
                    params: param_types,
                    ret: Box::new(ret_type),
                };

                // For M1, we use a synthetic DefId based on the name
                // This is a simplification - M3 will use proper DefId tracking
                let def_id = DefId::new(1000000 + name.0);
                self.env.define_name(*name, def_id, fn_ty.clone());
                self.node_types.insert(*id, fn_ty);
            }
        }

        // Second pass: check function bodies and script items
        for item in &self.ast.items {
            self.check_item(item);
        }
    }

    /// Resolves a type annotation to a concrete type.
    fn resolve_type_annotation(&self, ty: &TypeAnnotation) -> Ty {
        match ty {
            TypeAnnotation::Named(sym) => {
                // Look up the type name in the interner
                let type_name = self.interner.resolve(*sym);
                match type_name {
                    "Int" => Ty::Int,
                    "Str" => Ty::Str,
                    "Bool" => Ty::Bool,
                    "Unit" => Ty::Unit,
                    _ => {
                        // Unknown type - using escape hatch for M1
                        // In M3, this would be a proper error
                        Ty::Unknown
                    }
                }
            }
        }
    }

    /// Type-checks a top-level item.
    fn check_item(&mut self, item: &Item) {
        match item {
            Item::FnDef { id: _, params, ret_ty, body, .. } => {
                // Set up parameter types in the environment
                for (param_name, param_ty) in params.iter() {
                    let param_type = self.resolve_type_annotation(param_ty);
                    // For M1, use synthetic DefId based on parameter name
                    let param_def_id = DefId::new(3000000 + param_name.0);
                    self.env.define_name(*param_name, param_def_id, param_type);
                }

                // Set current function return type
                let ret_type = self.resolve_type_annotation(ret_ty);
                self.current_fn_ret = Some(ret_type.clone());

                // Check the function body
                let body_ty = self.check_expr(body);

                // Verify body type matches return type
                self.unify(&ret_type, &body_ty, body.span());

                // Clear current function context
                self.current_fn_ret = None;
            }
            Item::Script { stmt, .. } => {
                self.check_stmt(stmt);
            }
        }
    }

    /// Type-checks a statement.
    fn check_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let { name, ty, init, id, span } => {
                // Check the initializer
                let init_ty = self.check_expr(init);

                // If there's a type annotation, verify it matches
                let expected_ty = if let Some(annotation) = ty {
                    let annotated_ty = self.resolve_type_annotation(annotation);
                    self.unify(&annotated_ty, &init_ty, *span);
                    annotated_ty
                } else {
                    init_ty.clone()
                };

                // Store the type for this let binding
                // For M1, use synthetic DefId based on name
                let def_id = DefId::new(2000000 + name.0);
                self.env.define_name(*name, def_id, expected_ty.clone());

                self.node_types.insert(*id, expected_ty);
            }
            Stmt::Expr { expr } => {
                self.check_expr(expr);
            }
        }
    }

    /// Type-checks an expression and returns its type.
    fn check_expr(&mut self, expr: &Expr) -> Ty {
        let ty = match expr {
            Expr::Literal { value, id, .. } => {
                let lit_ty = match value {
                    Literal::Int(_) => Ty::Int,
                    Literal::Str(_) => Ty::Str,
                    Literal::Bool(_) => Ty::Bool,
                    Literal::Unit => Ty::Unit,
                };
                self.node_types.insert(*id, lit_ty.clone());
                lit_ty
            }

            Expr::Variable { name, id, span: _ } => {
                // For M1, look up the variable by name
                // First try name-based lookup, then fall back to resolver's DefId mapping
                let ty = if let Some(def_id) = self.env.lookup_name(*name) {
                    // Found in our type environment
                    self.env.lookup(def_id)
                        .cloned()
                        .unwrap_or(Ty::Unknown)
                } else if let Some(def_id) = self.resolve.resolutions.get(id) {
                    // Try the resolver's mapping (for parameters, etc.)
                    self.env.lookup(*def_id)
                        .cloned()
                        .unwrap_or(Ty::Unknown)
                } else {
                    // Not found - use Unknown
                    Ty::Unknown
                };
                self.node_types.insert(*id, ty.clone());
                ty
            }

            Expr::Binary { op, left, right, id, span } => {
                let left_ty = self.check_expr(left);
                let right_ty = self.check_expr(right);

                let result_ty = self.check_binary_op(*op, &left_ty, &right_ty, *span);
                self.node_types.insert(*id, result_ty.clone());
                result_ty
            }

            Expr::Unary { op, expr: inner, id, span } => {
                let inner_ty = self.check_expr(inner);
                let result_ty = self.check_unary_op(*op, &inner_ty, *span);
                self.node_types.insert(*id, result_ty.clone());
                result_ty
            }

            Expr::Call { callee, args, id, span } => {
                let callee_ty = self.check_expr(callee);
                let arg_types: Vec<Ty> = args.iter().map(|arg| self.check_expr(arg)).collect();

                let result_ty = match callee_ty {
                    Ty::Fn { params, ret } => {
                        // Check argument count
                        if params.len() != arg_types.len() {
                            // For M1, we'll be lenient and use Unknown
                            // In M3, this would be a hard error
                            Ty::Unknown
                        } else {
                            // Check argument types match
                            for (param_ty, arg_ty) in params.iter().zip(arg_types.iter()) {
                                self.unify(param_ty, arg_ty, *span);
                            }
                            (*ret).clone()
                        }
                    }
                    Ty::Unknown => Ty::Unknown, // Escape hatch
                    _ => {
                        // Not a callable type - use Unknown for M1
                        Ty::Unknown
                    }
                };

                self.node_types.insert(*id, result_ty.clone());
                result_ty
            }

            Expr::If { cond, then_branch, else_branch, id, span } => {
                let cond_ty = self.check_expr(cond);

                // Condition should be Bool
                self.unify(&Ty::Bool, &cond_ty, cond.span());

                let then_ty = self.check_expr(then_branch);

                let result_ty = if let Some(else_expr) = else_branch {
                    let else_ty = self.check_expr(else_expr);
                    // Both branches should have the same type
                    self.unify(&then_ty, &else_ty, *span);
                    then_ty
                } else {
                    // No else branch - result is Unit
                    Ty::Unit
                };

                self.node_types.insert(*id, result_ty.clone());
                result_ty
            }

            Expr::Block { stmts, expr: final_expr, id, .. } => {
                // Check all statements
                for stmt in stmts {
                    self.check_stmt(stmt);
                }

                // The type is the type of the final expression, or Unit if none
                let block_ty = if let Some(expr) = final_expr {
                    self.check_expr(expr)
                } else {
                    Ty::Unit
                };

                self.node_types.insert(*id, block_ty.clone());
                block_ty
            }

            Expr::Return { expr: ret_expr, id, span } => {
                let ret_ty = if let Some(expr) = ret_expr {
                    self.check_expr(expr)
                } else {
                    Ty::Unit
                };

                // Check against current function's return type
                if let Some(expected_ret) = self.current_fn_ret.clone() {
                    self.unify(&expected_ret, &ret_ty, *span);
                }

                // Return expressions themselves have type Unit (they don't produce a value)
                self.node_types.insert(*id, Ty::Unit);
                Ty::Unit
            }
        };

        ty
    }

    /// Type-checks a binary operation.
    fn check_binary_op(&mut self, op: BinOp, left: &Ty, right: &Ty, span: Span) -> Ty {
        use BinOp::*;

        // Handle Unknown escape hatch
        if left.is_unknown() || right.is_unknown() {
            return Ty::Unknown;
        }

        match op {
            // Arithmetic operations
            Add | Sub | Mul | Div | Rem => {
                match (left, right) {
                    (Ty::Int, Ty::Int) => Ty::Int,
                    (Ty::Float, Ty::Float) => Ty::Float,
                    // String concatenation with +
                    (Ty::Str, Ty::Str) if matches!(op, Add) => Ty::Str,
                    _ => {
                        self.errors.push(TypeError::IncompatibleTypes {
                            operation: format!("{:?}", op),
                            left: left.clone(),
                            right: right.clone(),
                            span,
                        });
                        Ty::Unknown
                    }
                }
            }

            // Comparison operations
            Eq | Ne | Lt | Le | Gt | Ge => {
                // Check operands are the same type and comparable
                if left == right {
                    Ty::Bool
                } else {
                    self.errors.push(TypeError::IncompatibleTypes {
                        operation: format!("{:?}", op),
                        left: left.clone(),
                        right: right.clone(),
                        span,
                    });
                    Ty::Unknown
                }
            }

            // Logical operations
            And | Or => {
                match (left, right) {
                    (Ty::Bool, Ty::Bool) => Ty::Bool,
                    _ => {
                        self.errors.push(TypeError::IncompatibleTypes {
                            operation: format!("{:?}", op),
                            left: left.clone(),
                            right: right.clone(),
                            span,
                        });
                        Ty::Unknown
                    }
                }
            }
        }
    }

    /// Type-checks a unary operation.
    fn check_unary_op(&mut self, op: UnOp, operand: &Ty, _span: Span) -> Ty {
        use UnOp::*;

        // Handle Unknown escape hatch
        if operand.is_unknown() {
            return Ty::Unknown;
        }

        match op {
            Neg => {
                match operand {
                    Ty::Int => Ty::Int,
                    Ty::Float => Ty::Float,
                    _ => Ty::Unknown,
                }
            }
            Not => {
                match operand {
                    Ty::Bool => Ty::Bool,
                    _ => Ty::Unknown,
                }
            }
        }
    }

    /// Attempts to unify two types. Returns the unified type on success,
    /// or Unknown on failure (with an error emitted).
    fn unify(&mut self, expected: &Ty, found: &Ty, span: Span) -> Ty {
        if expected == found {
            expected.clone()
        } else if expected.is_unknown() || found.is_unknown() {
            // Escape hatch - Unknown unifies with anything
            Ty::Unknown
        } else {
            self.errors.push(TypeError::Mismatch {
                expected: expected.clone(),
                found: found.clone(),
                span,
            });
            Ty::Unknown
        }
    }

    /// Consumes the type checker and returns the result.
    fn into_result(self) -> TypeResult {
        TypeResult::new(self.node_types, self.errors)
    }
}

/// Type-checks a parsed AST with resolution information.
///
/// This is the single public entry point for the type checker stage.
///
/// NOTE: For M1, we require access to the interner to resolve type annotations.
/// This is a pragmatic choice to keep the implementation simple. In M3, type
/// resolution will be more sophisticated.
pub fn typecheck(ast: &ParseResult, resolve: &ResolveResult, interner: &Interner) -> TypeResult {
    let mut checker = TypeChecker::new(ast, resolve, interner);
    checker.check_all();
    checker.into_result()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_literal_types() {
        // Test that integer literals have type Int
        let items = vec![];
        let ast = ParseResult::new(items, vec![]);
        let resolve = ResolveResult::new(
            HashMap::new(),
            HashMap::new(),
            HashMap::new(),
            vec![],
        );
        let interner = Interner::new();

        let result = typecheck(&ast, &resolve, &interner);
        assert!(!result.has_errors());
    }

    #[test]
    fn test_unknown_unifies_with_anything() {
        // Test that Ty::Unknown acts as an escape hatch
        let items = vec![];
        let ast = ParseResult::new(items, vec![]);
        let resolve = ResolveResult::new(
            HashMap::new(),
            HashMap::new(),
            HashMap::new(),
            vec![],
        );
        let interner = Interner::new();

        let result = typecheck(&ast, &resolve, &interner);
        assert!(!result.has_errors());
    }
}
