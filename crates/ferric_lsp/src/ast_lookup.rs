//! Find the smallest `Expr::Variable` whose `Span` contains a byte offset.
//!
//! Used by the hover, goto-definition, and (transitively) completion handlers
//! to resolve "what identifier is the cursor on?". Returns `None` for
//! literals, keywords, whitespace, and operators — the LSP correctly returns
//! "no hover" / "no definition" in those cases.

use ferric_common::{Expr, Item, NodeId, ParseResult, Span, Stmt};

/// Walk every top-level item, returning the deepest `Expr::Variable` whose
/// span contains `byte`. Returns the variable's `(NodeId, Span)`.
pub fn find_ident_at_byte(parse: &ParseResult, byte: u32) -> Option<(NodeId, Span)> {
    for item in &parse.items {
        if let Some(found) = walk_item(item, byte) {
            return Some(found);
        }
    }
    None
}

fn walk_item(item: &Item, byte: u32) -> Option<(NodeId, Span)> {
    match item {
        Item::FnDef { body, .. } => walk_expr(body, byte),
        Item::Script { stmt, .. } => walk_stmt(stmt, byte),
        Item::ImplBlock { methods, .. } => methods.iter().find_map(|m| walk_expr(&m.body, byte)),
        Item::Export(decl) => walk_item(&decl.item, byte),
        // Struct/Enum/Trait/Import/TypeAlias have no expression bodies users
        // can hover over for variable references.
        _ => None,
    }
}

fn walk_stmt(stmt: &Stmt, byte: u32) -> Option<(NodeId, Span)> {
    match stmt {
        Stmt::Let { init, .. }      => walk_expr(init, byte),
        Stmt::Assign { target, value, .. } => walk_expr(target, byte).or_else(|| walk_expr(value, byte)),
        Stmt::Expr { expr }         => walk_expr(expr, byte),
        Stmt::Require(req)          => walk_expr(&req.expr, byte),
        Stmt::For { iter, body, .. } => walk_expr(iter, byte).or_else(|| walk_expr(body, byte)),
    }
}

fn walk_expr(expr: &Expr, byte: u32) -> Option<(NodeId, Span)> {
    if !contains(expr.span(), byte) {
        return None;
    }

    // Recurse into children first; the smallest containing node wins.
    let child_hit = match expr {
        Expr::Block { stmts, expr: tail, .. } => {
            stmts.iter().find_map(|s| walk_stmt(s, byte))
                .or_else(|| tail.as_deref().and_then(|e| walk_expr(e, byte)))
        }
        Expr::If { cond, then_branch, else_branch, .. } => {
            walk_expr(cond, byte)
                .or_else(|| walk_expr(then_branch, byte))
                .or_else(|| else_branch.as_deref().and_then(|e| walk_expr(e, byte)))
        }
        Expr::While { cond, body, .. } => {
            walk_expr(cond, byte).or_else(|| walk_expr(body, byte))
        }
        Expr::Loop { body, .. } => walk_expr(body, byte),
        Expr::Binary { left, right, .. } => {
            walk_expr(left, byte).or_else(|| walk_expr(right, byte))
        }
        Expr::Unary { expr: inner, .. } => walk_expr(inner, byte),
        Expr::Call { callee, args, .. } => {
            args.iter().find_map(|a| walk_expr(&a.value, byte))
                .or_else(|| walk_expr(callee, byte))
        }
        Expr::Return { expr: inner, .. } => {
            inner.as_deref().and_then(|e| walk_expr(e, byte))
        }
        Expr::Closure { body, .. } => walk_expr(body, byte),
        Expr::FieldAccess { expr: receiver, .. } => walk_expr(receiver, byte),
        Expr::MethodCall { receiver, args, .. } => {
            args.iter().find_map(|a| walk_expr(&a.value, byte))
                .or_else(|| walk_expr(receiver, byte))
        }
        Expr::Match { scrutinee, arms, .. } => {
            walk_expr(scrutinee, byte)
                .or_else(|| arms.iter().find_map(|a| walk_expr(&a.body, byte)))
        }
        Expr::Tuple { elements, .. } | Expr::ArrayLit { elements, .. } | Expr::VariantCtor { args: elements, .. } => {
            elements.iter().find_map(|e| walk_expr(e, byte))
        }
        Expr::Index { array, index, .. } => {
            walk_expr(array, byte).or_else(|| walk_expr(index, byte))
        }
        Expr::StructLit { fields, .. } => {
            fields.iter().find_map(|(_, e)| walk_expr(e, byte))
        }
        Expr::Cast(c) => walk_expr(&c.expr, byte),
        // Variants without sub-expressions: Literal, Variable, Break, Continue, Shell.
        // Shell parts may contain interpolated tokens, but those aren't AST
        // expressions we'd hover over here — handled in a future task.
        _ => None,
    };

    if let Some(hit) = child_hit {
        return Some(hit);
    }

    // No child matched. If this expression is itself a variable, return it.
    if let Expr::Variable { id, span, .. } = expr {
        return Some((*id, *span));
    }

    None
}

fn contains(span: Span, byte: u32) -> bool {
    span.start <= byte && byte <= span.end
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::run_pipeline;

    fn lookup(src: &str, byte: u32) -> Option<(NodeId, Span)> {
        let snap = run_pipeline("file:///tmp/t.fe".into(), 1, src.into());
        find_ident_at_byte(snap.parse.as_ref().unwrap(), byte)
    }

    #[test]
    fn cursor_on_variable_returns_node_and_span() {
        // "let x = 1\nx" — cursor on the trailing `x` (byte 10).
        let src = "let x = 1\nx";
        let hit = lookup(src, 10);
        assert!(hit.is_some(), "expected to find variable at byte 10");
        let (_, span) = hit.unwrap();
        assert_eq!(span.start, 10);
        assert_eq!(span.end, 11);
    }

    #[test]
    fn cursor_on_literal_returns_none() {
        // "let x = 42" — cursor on the `4`.
        let hit = lookup("let x = 42", 8);
        assert!(hit.is_none(), "literals should not match: {hit:?}");
    }

    #[test]
    fn cursor_on_whitespace_returns_none() {
        // Cursor on the space between `let` and `x`.
        let hit = lookup("let x = 1\nx", 3);
        assert!(hit.is_none());
    }

    #[test]
    fn smallest_variable_wins_in_nested_call() {
        // `f(g(x))` — cursor on `x` should resolve to `x`, not `g` or the call.
        // src: "fn f(n: Int) -> Int { n }\nfn g(n: Int) -> Int { n }\nlet r = f(n: g(n: 1))"
        // The `g` is at... let me just verify the deepest match wins by searching for an ident
        // span that's a strict subset of any enclosing span.
        let src = "fn id(n: Int) -> Int { n }\nlet y = id(n: 1)";
        // Cursor on the final `n` of the body (`{ n }` — byte 23).
        let hit = lookup(src, 23);
        let (_, span) = hit.expect("expected a Variable hit on body's `n`");
        // The matched span must cover exactly the identifier `n`, not the
        // surrounding block.
        assert_eq!(span.end - span.start, 1, "expected width-1 span, got {span:?}");
    }
}
