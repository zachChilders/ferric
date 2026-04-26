//! # Ferric Exhaustiveness Checker (M4)
//!
//! Walks the AST after type checking and verifies that every `match`
//! expression covers every possible value of its scrutinee — at least, every
//! variant of any enum scrutinee. Also detects arms that are unreachable
//! because an earlier arm already matches everything.
//!
//! Per the architectural rules, this stage reads only `ParseResult` and
//! `TypeResult` from `ferric_common` and produces an `ExhaustivenessResult`.

use std::collections::HashSet;

use ferric_common::{
    ExhaustivenessError, ExhaustivenessResult, Expr, Item, MatchArm, ParseResult, Pattern,
    ShellPart, Stmt, Symbol, Ty, TypeResult,
};

/// Single public entry point for the exhaustiveness stage.
pub fn check_exhaustiveness(ast: &ParseResult, types: &TypeResult) -> ExhaustivenessResult {
    let mut checker = Checker {
        types,
        errors: Vec::new(),
    };
    for item in &ast.items {
        checker.check_item(item);
    }
    ExhaustivenessResult::new(checker.errors)
}

struct Checker<'a> {
    types: &'a TypeResult,
    errors: Vec<ExhaustivenessError>,
}

impl<'a> Checker<'a> {
    fn check_item(&mut self, item: &Item) {
        match item {
            Item::FnDef { body, .. } => self.check_expr(body),
            Item::Script { stmt, .. } => self.check_stmt(stmt),
            Item::StructDef { .. } | Item::EnumDef { .. } | Item::TraitDef { .. } => {}
            Item::ImplBlock { methods, .. } => {
                for m in methods {
                    self.check_expr(&m.body);
                }
            }
            Item::Export(decl) => {
                self.check_item(&decl.item);
            }
            Item::Import(_) | Item::TypeAlias(_) => {}
        }
    }

    fn check_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let { init, .. } => self.check_expr(init),
            Stmt::Assign { target, value, .. } => {
                self.check_expr(target);
                self.check_expr(value);
            }
            Stmt::Expr { expr } => self.check_expr(expr),
            Stmt::Require(req) => {
                self.check_expr(&req.expr);
                if let Some(m) = &req.message {
                    self.check_expr(m);
                }
                if let Some(s) = &req.set_fn {
                    self.check_expr(s);
                }
            }
            Stmt::For { iter, body, .. } => {
                self.check_expr(iter);
                self.check_expr(body);
            }
        }
    }

    fn check_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Literal { .. }
            | Expr::Variable { .. }
            | Expr::Break { .. }
            | Expr::Continue { .. } => {}
            Expr::Binary { left, right, .. } => {
                self.check_expr(left);
                self.check_expr(right);
            }
            Expr::Unary { expr, .. } => self.check_expr(expr),
            Expr::Call { callee, args, .. } => {
                self.check_expr(callee);
                for a in args {
                    self.check_expr(&a.value);
                }
            }
            Expr::If { cond, then_branch, else_branch, .. } => {
                self.check_expr(cond);
                self.check_expr(then_branch);
                if let Some(e) = else_branch {
                    self.check_expr(e);
                }
            }
            Expr::Block { stmts, expr, .. } => {
                for s in stmts {
                    self.check_stmt(s);
                }
                if let Some(e) = expr {
                    self.check_expr(e);
                }
            }
            Expr::Return { expr, .. } => {
                if let Some(e) = expr {
                    self.check_expr(e);
                }
            }
            Expr::While { cond, body, .. } => {
                self.check_expr(cond);
                self.check_expr(body);
            }
            Expr::Loop { body, .. } => self.check_expr(body),
            Expr::Closure { body, .. } => self.check_expr(body),
            Expr::Shell { parts, .. } => {
                for p in parts {
                    if let ShellPart::Interpolated(e) = p {
                        self.check_expr(e);
                    }
                }
            }
            Expr::StructLit { fields, .. } => {
                for (_, fexpr) in fields {
                    self.check_expr(fexpr);
                }
            }
            Expr::FieldAccess { expr, .. } => self.check_expr(expr),
            Expr::Tuple { elements, .. } => {
                for e in elements {
                    self.check_expr(e);
                }
            }
            Expr::VariantCtor { args, .. } => {
                for a in args {
                    self.check_expr(a);
                }
            }
            Expr::Match { scrutinee, arms, span, .. } => {
                self.check_expr(scrutinee);
                for arm in arms {
                    self.check_expr(&arm.body);
                }
                self.check_match(scrutinee, arms, *span);
            }
            Expr::MethodCall { receiver, args, .. } => {
                self.check_expr(receiver);
                for a in args {
                    self.check_expr(&a.value);
                }
            }
            Expr::ArrayLit { elements, .. } => {
                for e in elements {
                    self.check_expr(e);
                }
            }
            Expr::Index { array, index, .. } => {
                self.check_expr(array);
                self.check_expr(index);
            }
            Expr::Cast(c) => self.check_expr(&c.expr),
        }
    }

    fn check_match(
        &mut self,
        scrutinee: &Expr,
        arms: &[MatchArm],
        match_span: ferric_common::Span,
    ) {
        if arms.is_empty() {
            // The type checker likely already complained; nothing useful to add.
            return;
        }

        // Detect unreachable arms first: anything after a wildcard / variable
        // / a single struct or tuple pattern that always succeeds is dead.
        let mut covers_all = false;
        for arm in arms {
            if covers_all {
                self.errors.push(ExhaustivenessError::UnreachableArm {
                    span: arm.span,
                });
                continue;
            }
            if pattern_is_irrefutable(&arm.pattern) {
                covers_all = true;
            }
        }

        // Variant exhaustiveness for enums.
        let scrut_ty = self.types.node_types.get(&scrutinee.id());
        if let Some(Ty::Enum { variants, .. }) = scrut_ty {
            let all: HashSet<Symbol> = variants.iter().map(|(n, _)| *n).collect();
            let mut covered: HashSet<Symbol> = HashSet::new();
            let mut wildcarded = false;
            for arm in arms {
                if pattern_is_irrefutable(&arm.pattern) {
                    wildcarded = true;
                    break;
                }
                if let Pattern::Variant { variant, .. } = &arm.pattern {
                    covered.insert(*variant);
                }
            }
            if !wildcarded {
                let missing: Vec<Symbol> =
                    all.difference(&covered).copied().collect();
                if !missing.is_empty() {
                    self.errors.push(ExhaustivenessError::NonExhaustive {
                        missing,
                        span: match_span,
                    });
                }
            }
        }
    }
}

/// Returns true if a pattern matches every value of its scrutinee type.
/// This is a conservative approximation — it counts only patterns that
/// trivially succeed (wildcard, variable binding, tuples/structs whose every
/// sub-pattern is irrefutable).
fn pattern_is_irrefutable(pat: &Pattern) -> bool {
    match pat {
        Pattern::Wildcard { .. } | Pattern::Variable { .. } => true,
        Pattern::Tuple { patterns, .. } => patterns.iter().all(pattern_is_irrefutable),
        Pattern::Struct { fields, .. } => {
            fields.iter().all(|(_, p)| pattern_is_irrefutable(p))
        }
        Pattern::Literal { .. } | Pattern::Variant { .. } => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferric_common::{ParseResult, TypeResult};

    #[test]
    fn empty_program_has_no_errors() {
        let ast = ParseResult::new(vec![], vec![]);
        let types = TypeResult::new(std::collections::HashMap::new(), vec![]);
        let result = check_exhaustiveness(&ast, &types);
        assert!(result.errors.is_empty());
    }
}
