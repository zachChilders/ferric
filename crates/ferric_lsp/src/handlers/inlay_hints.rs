//! `textDocument/inlayHint`.
//!
//! Renders inferred-type hints inline at three sites:
//!
//!   1. Unannotated `let` bindings — `let x = 1` shows `: Int`
//!   2. Closure parameters with no explicit type — `|x| x * 2` shows `: Int`
//!   3. Implicitly coerced call arguments — `f(x: 5)` where `f` takes
//!      `Float` shows `as Float` immediately after the argument.
//!
//! Hints require a *current* `TypeResult`. If the type checker couldn't run
//! (parse errors, panic), no hints are returned — stale hints on actively
//! edited code are worse than no hints.
//!
//! Hints are filtered to the LSP-supplied visible range; off-screen hints
//! are dropped.
//!
//! AST shape notes:
//!   - Top-level `let` is wrapped in `Item::Script { stmt: Stmt::Let { .. } }`.
//!   - Nested `let` lives inside `Stmt::Let` within an `Expr::Block`.
//!   - Closures are `Expr::Closure { params: Vec<Param>, body: Box<Expr>, id, .. }`.
//!   - `Param.ty` is non-optional `TypeAnnotation`; `TypeAnnotation::Infer`
//!     marks closure params with no explicit annotation.
//!   - Function-definition bodies are `Expr` (not a separate `Block` type).
//!   - The AST has no separate name-span for binders, so the hint position
//!     is computed by scanning forward over the source text.

use ferric_common::{
    Expr, Item, NamedArg, NodeId, Param, ResolveResult, Span, Stmt, Ty, TypeAnnotation, TypeResult,
};
use tower_lsp::lsp_types::{InlayHint, InlayHintKind, InlayHintLabel, Position, Range};

use crate::pipeline::{LineIndex, PipelineSnapshot};

pub fn inlay_hints(snapshot: &PipelineSnapshot, range: Range) -> Vec<InlayHint> {
    // Current type result is required. No fallback to last-good — stale hints
    // are confusing on actively-edited code.
    let (Some(parse), Some(types)) = (&snapshot.parse, &snapshot.typecheck) else {
        return vec![];
    };

    let mut hints = Vec::new();
    let cx = Ctx {
        source: &snapshot.source,
        types,
        // `resolve` is optional: coercion hints only fire when the resolver
        // produced canonical-arg metadata. The other hint sites do not need
        // it.
        resolve: snapshot.resolve.as_ref(),
        li: &snapshot.line_index,
        range,
    };

    for item in &parse.items {
        collect_for_item(item, &cx, &mut hints);
    }

    hints
}

struct Ctx<'a> {
    source: &'a str,
    types: &'a TypeResult,
    resolve: Option<&'a ResolveResult>,
    li: &'a LineIndex,
    range: Range,
}

// ---------------------------------------------------------------------------
// Walkers
// ---------------------------------------------------------------------------

fn collect_for_item(item: &Item, cx: &Ctx, hints: &mut Vec<InlayHint>) {
    match item {
        Item::Fn(item) => {
            // Top-level fn parameters always have explicit annotations
            // (the parser requires it), so they never produce hints.
            // Recurse into the body for nested lets and closures.
            collect_for_expr(&item.body, cx, hints);
        }
        Item::Script { stmt, .. } => collect_for_stmt(stmt, cx, hints),
        Item::ImplBlock { methods, .. } => {
            for m in methods {
                collect_for_expr(&m.body, cx, hints);
            }
        }
        Item::Export(decl) => collect_for_item(&decl.item, cx, hints),
        // Struct/Enum/Trait/Import/TypeAlias contain no expressions that can
        // produce inferred-type hints.
        _ => {}
    }
}

fn collect_for_stmt(stmt: &Stmt, cx: &Ctx, hints: &mut Vec<InlayHint>) {
    match stmt {
        Stmt::Let {
            ty, init, id, span, ..
        } => {
            // Hint only when there is no explicit annotation.
            if ty.is_none() {
                push_let_hint(*id, *span, cx, hints);
            }
            collect_for_expr(init, cx, hints);
        }
        Stmt::Assign { target, value, .. } => {
            collect_for_expr(target, cx, hints);
            collect_for_expr(value, cx, hints);
        }
        Stmt::Expr { expr } => collect_for_expr(expr, cx, hints),
        Stmt::Require(req) => collect_for_expr(&req.expr, cx, hints),
        Stmt::For { iter, body, .. } => {
            collect_for_expr(iter, cx, hints);
            collect_for_expr(body, cx, hints);
        }
    }
}

fn collect_for_expr(expr: &Expr, cx: &Ctx, hints: &mut Vec<InlayHint>) {
    match expr {
        Expr::Block {
            stmts, expr: tail, ..
        } => {
            for s in stmts {
                collect_for_stmt(s, cx, hints);
            }
            if let Some(t) = tail {
                collect_for_expr(t, cx, hints);
            }
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
            ..
        } => {
            collect_for_expr(cond, cx, hints);
            collect_for_expr(then_branch, cx, hints);
            if let Some(e) = else_branch {
                collect_for_expr(e, cx, hints);
            }
        }
        Expr::While { cond, body, .. } => {
            collect_for_expr(cond, cx, hints);
            collect_for_expr(body, cx, hints);
        }
        Expr::Loop { body, .. } => collect_for_expr(body, cx, hints),
        Expr::Binary { left, right, .. } => {
            collect_for_expr(left, cx, hints);
            collect_for_expr(right, cx, hints);
        }
        Expr::Unary { expr: inner, .. } => collect_for_expr(inner, cx, hints),
        Expr::Call {
            callee,
            args,
            id: call_id,
            ..
        } => {
            for a in args {
                collect_for_expr(&a.value, cx, hints);
            }
            collect_for_expr(callee, cx, hints);
            push_coercion_hints(*call_id, callee, args, cx, hints);
        }
        Expr::Return { expr: Some(e), .. } => collect_for_expr(e, cx, hints),
        Expr::Closure {
            params, body, id, ..
        } => {
            push_closure_param_hints(*id, params, cx, hints);
            collect_for_expr(body, cx, hints);
        }
        Expr::FieldAccess { expr: receiver, .. } => collect_for_expr(receiver, cx, hints),
        Expr::MethodCall { receiver, args, .. } => {
            collect_for_expr(receiver, cx, hints);
            for a in args {
                collect_for_expr(&a.value, cx, hints);
            }
        }
        Expr::Match {
            scrutinee, arms, ..
        } => {
            collect_for_expr(scrutinee, cx, hints);
            for arm in arms {
                collect_for_expr(&arm.body, cx, hints);
            }
        }
        Expr::Tuple { elements, .. }
        | Expr::ArrayLit { elements, .. }
        | Expr::VariantCtor { args: elements, .. } => {
            for e in elements {
                collect_for_expr(e, cx, hints);
            }
        }
        Expr::Index { array, index, .. } => {
            collect_for_expr(array, cx, hints);
            collect_for_expr(index, cx, hints);
        }
        Expr::StructLit { fields, .. } => {
            for (_, e) in fields {
                collect_for_expr(e, cx, hints);
            }
        }
        Expr::Cast(c) => collect_for_expr(&c.expr, cx, hints),
        // Leaves: Literal, Variable, Break, Continue, Shell — no hints.
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Hint emitters
// ---------------------------------------------------------------------------

fn push_let_hint(stmt_id: NodeId, stmt_span: Span, cx: &Ctx, hints: &mut Vec<InlayHint>) {
    let Some(ty) = cx.types.node_types.get(&stmt_id) else {
        return;
    };
    let Some(name_end) = let_name_end(cx.source, stmt_span) else {
        return;
    };
    let position = cx.li.position_of(name_end);
    if !position_in_range(position, cx.range) {
        return;
    }
    hints.push(make_hint(position, ty));
}

/// Closure params have no NodeId, so we cannot look them up directly in
/// `node_types`. Instead, look up the *closure's* type — which is recorded
/// as `Ty::Fn { params, ret }` — and zip positionally with the AST param
/// list. This is robust against the inferencer's substitution: the type
/// stored at `node_types[closure.id]` is post-substitution.
fn push_closure_param_hints(
    closure_id: NodeId,
    params: &[Param],
    cx: &Ctx,
    hints: &mut Vec<InlayHint>,
) {
    let Some(closure_ty) = cx.types.node_types.get(&closure_id) else {
        return;
    };
    let Ty::Fn {
        params: param_tys, ..
    } = closure_ty
    else {
        // Closure type didn't resolve to an Fn (still a free var, etc.).
        // Skip rather than guess.
        return;
    };
    if param_tys.len() != params.len() {
        return;
    } // defensive

    for (param, ty) in params.iter().zip(param_tys.iter()) {
        if !matches!(param.ty, TypeAnnotation::Infer) {
            continue; // user wrote an explicit annotation already
        }
        let Some(name_end) = param_name_end(cx.source, param.span) else {
            continue;
        };
        let position = cx.li.position_of(name_end);
        if !position_in_range(position, cx.range) {
            continue;
        }
        hints.push(make_hint(position, ty));
    }
}

fn make_hint(position: Position, ty: &Ty) -> InlayHint {
    InlayHint {
        position,
        label: InlayHintLabel::String(format!(": {ty}")),
        kind: Some(InlayHintKind::TYPE),
        text_edits: None,
        tooltip: None,
        padding_left: Some(false),
        padding_right: Some(false),
        data: None,
    }
}

/// Inlay hint for an implicit `To<T>` coercion at a call argument position.
/// Renders ` as T` immediately after the argument expression so the editor
/// surfaces what the type checker silently inserted.
fn make_coercion_hint(position: Position, ty: &Ty) -> InlayHint {
    InlayHint {
        position,
        label: InlayHintLabel::String(format!(" as {ty}")),
        // The hint reflects the *target* type the value is converted to, so
        // it shares the same kind as the inferred-type hints.
        kind: Some(InlayHintKind::TYPE),
        text_edits: None,
        tooltip: None,
        padding_left: Some(false),
        padding_right: Some(false),
        data: None,
    }
}

/// Walks the arguments of a call and emits an `as T` hint for every argument
/// the type checker recorded in `coercions`. The target `T` is read off the
/// callee's `Ty::Fn { params, .. }` at the same canonical position the
/// resolver assigned.
///
/// Implementation detail: the canonical-arg list (definition order, defaults
/// inserted) is what the type checker indexed against `params`, so coercions
/// are keyed on canonical-arg `NodeId`s. A canonical arg cloned from a
/// source arg keeps its original `NodeId`, so we can iterate the resolver's
/// canonical list and the source list both — the source list is what we
/// need to compute the hint *position* (defaults aren't visible in source).
fn push_coercion_hints(
    call_id: NodeId,
    callee: &Expr,
    args: &[NamedArg],
    cx: &Ctx,
    hints: &mut Vec<InlayHint>,
) {
    if cx.types.coercions.is_empty() {
        return;
    }
    let Some(resolve) = cx.resolve else {
        return;
    };
    let Some(callee_ty) = cx.types.node_types.get(&callee.id()) else {
        return;
    };
    let Ty::Fn { params, .. } = callee_ty else {
        return;
    };
    let Some(canonical) = resolve.canonical_call_args.get(&call_id) else {
        return;
    };
    if canonical.len() != params.len() {
        return; // shape mismatch — skip rather than guess
    }

    for (canon_idx, canon_arg) in canonical.iter().enumerate() {
        let arg_id = canon_arg.value.id();
        if !cx.types.coercions.contains_key(&arg_id) {
            continue;
        }
        // The argument may have been written explicitly or filled in from a
        // default — only show a hint when the user actually wrote it,
        // otherwise the hint floats on the call-site span (the default's
        // span is the call's own span by construction).
        let Some(source_arg) = args.iter().find(|a| a.name == canon_arg.name) else {
            continue;
        };
        let position = cx.li.position_of(source_arg.value.span().end);
        if !position_in_range(position, cx.range) {
            continue;
        }
        hints.push(make_coercion_hint(position, &params[canon_idx]));
    }
}

// ---------------------------------------------------------------------------
// Source-text scanning to find name-end positions.
// The AST has no separate name-span for binders. Both helpers parse a
// known prefix (`let` for stmts; nothing for params) then consume the
// identifier characters. The lexer/parser have already validated the shape,
// so we can trust the prefix is exactly as expected.
// ---------------------------------------------------------------------------

/// For `let x = …` or `let mut counter = …` returns the byte offset
/// immediately after the binder name. Returns `None` if the source slice
/// doesn't start with `let` (defensive — shouldn't happen).
fn let_name_end(source: &str, stmt_span: Span) -> Option<u32> {
    let start = stmt_span.start as usize;
    let s = source.get(start..)?;
    let s = s.strip_prefix("let")?;
    let s = trim_left_ws(s);
    let s = match s.strip_prefix("mut") {
        // `mut` must be followed by whitespace to count as the modifier
        // (otherwise it's part of an identifier like `mutable`).
        Some(rest) if rest.starts_with(|c: char| c.is_whitespace()) => trim_left_ws(rest),
        _ => s,
    };
    let consumed_before_name = (source.len() - start) - s.len();
    let name_byte_len = ident_byte_len(s);
    if name_byte_len == 0 {
        return None;
    }
    Some((start + consumed_before_name + name_byte_len) as u32)
}

/// For a closure parameter with span starting at the binder name, returns
/// the byte offset immediately after the name.
fn param_name_end(source: &str, param_span: Span) -> Option<u32> {
    let start = param_span.start as usize;
    let s = source.get(start..)?;
    let name_byte_len = ident_byte_len(s);
    if name_byte_len == 0 {
        return None;
    }
    Some((start + name_byte_len) as u32)
}

fn trim_left_ws(s: &str) -> &str {
    s.trim_start_matches(|c: char| c.is_whitespace())
}

fn ident_byte_len(s: &str) -> usize {
    s.chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .map(|c| c.len_utf8())
        .sum()
}

fn position_in_range(p: Position, r: Range) -> bool {
    let after_start =
        p.line > r.start.line || (p.line == r.start.line && p.character >= r.start.character);
    let before_end =
        p.line < r.end.line || (p.line == r.end.line && p.character <= r.end.character);
    after_start && before_end
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::run_pipeline;

    fn full_range() -> Range {
        Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: Position {
                line: u32::MAX,
                character: u32::MAX,
            },
        }
    }

    fn hints_for(src: &str) -> Vec<InlayHint> {
        let snap = run_pipeline("file:///tmp/t.fe".into(), 1, src.into());
        inlay_hints(&snap, full_range())
    }

    fn label(h: &InlayHint) -> &str {
        match &h.label {
            InlayHintLabel::String(s) => s,
            _ => panic!("expected String label"),
        }
    }

    #[test]
    fn unannotated_let_emits_hint() {
        let h = hints_for("let x = 1");
        assert_eq!(h.len(), 1);
        assert_eq!(label(&h[0]), ": Int");
        // Position should land immediately after `x` (line 0, char 5).
        assert_eq!(
            h[0].position,
            Position {
                line: 0,
                character: 5
            }
        );
        assert_eq!(h[0].kind, Some(InlayHintKind::TYPE));
        assert_eq!(h[0].padding_left, Some(false));
        assert_eq!(h[0].padding_right, Some(false));
    }

    #[test]
    fn annotated_let_emits_no_hint() {
        let h = hints_for("let x: Int = 1");
        assert!(h.is_empty(), "expected no hints, got {h:?}");
    }

    #[test]
    fn mut_let_emits_hint_after_name() {
        let h = hints_for("let mut counter = 0");
        assert_eq!(h.len(), 1);
        assert_eq!(label(&h[0]), ": Int");
        // "let mut counter" → `counter` ends at byte 15.
        assert_eq!(
            h[0].position,
            Position {
                line: 0,
                character: 15
            }
        );
    }

    #[test]
    fn nested_let_inside_fn_emits_hint() {
        let h = hints_for("fn main() -> Unit { let pi = 3.14 }");
        let pi_hint = h.iter().find(|h| label(h) == ": Float");
        assert!(
            pi_hint.is_some(),
            "expected `: Float` hint inside fn body, got {h:?}"
        );
    }

    #[test]
    fn fn_params_never_emit_hints() {
        // Annotated fn params; no nested unannotated lets.
        let h = hints_for("fn f(n: Int) -> Int { n }");
        assert!(h.is_empty(), "expected no hints, got {h:?}");
    }

    #[test]
    fn no_hints_when_typecheck_unavailable() {
        let mut snap = run_pipeline("file:///tmp/t.fe".into(), 1, "let x = 1".into());
        snap.typecheck = None;
        let h = inlay_hints(&snap, full_range());
        assert!(h.is_empty(), "stale hints leaked: {h:?}");
    }

    #[test]
    fn hints_outside_range_are_filtered() {
        // Hint would land at line 0; ask for line 5 only.
        let snap = run_pipeline("file:///tmp/t.fe".into(), 1, "let x = 1".into());
        let off_screen = Range {
            start: Position {
                line: 5,
                character: 0,
            },
            end: Position {
                line: 9,
                character: 0,
            },
        };
        let h = inlay_hints(&snap, off_screen);
        assert!(h.is_empty(), "hint outside range was returned: {h:?}");
    }

    #[test]
    fn label_format_matches_display_for_ty() {
        let h = hints_for("let s = \"hi\"");
        assert_eq!(label(&h[0]), ": Str");
    }

    #[test]
    fn name_end_helper_handles_let() {
        assert_eq!(let_name_end("let x = 1", Span::new(0, 9)), Some(5));
        assert_eq!(
            let_name_end("let mut counter=0", Span::new(0, 17)),
            Some(15)
        );
        // Identifier with underscores and digits.
        assert_eq!(let_name_end("let abc_42 = 1", Span::new(0, 14)), Some(10));
    }

    #[test]
    fn name_end_helper_does_not_eat_mutable_prefix_into_mut_keyword() {
        // `let mutable = 5` — `mut` is NOT followed by whitespace, so the
        // identifier is `mutable`, not `mut` + `able`.
        assert_eq!(let_name_end("let mutable = 5", Span::new(0, 15)), Some(11));
    }

    #[test]
    fn closure_unannotated_param_emits_hint() {
        // `|x| x + 1` — the closure type infers as `fn(Int) -> Int` once
        // applied with an Int argument. We force concrete inference by
        // calling it.
        let h = hints_for("let r = |x| x + 1\nlet n = r(x: 5)");
        // Expect at least three hints overall:
        //   - `: Int` after the closure param `x`
        //   - `: fn(Int) -> Int` (or similar) after `let r`
        //   - `: Int` after `let n`
        let param_hint = h.iter().find(|h| {
            // `x` of the closure starts at byte 9, ends at byte 10 — line 0, char 10.
            h.position
                == Position {
                    line: 0,
                    character: 10,
                }
                && label(h) == ": Int"
        });
        assert!(
            param_hint.is_some(),
            "expected `: Int` hint after closure param, got {h:?}",
        );
    }

    #[test]
    fn closure_annotated_param_emits_no_hint() {
        // `|x: Int| x + 1` — explicit annotation suppresses the param hint.
        let h = hints_for("let r = |x: Int| x + 1\nlet n = r(x: 5)");
        // The let `r` and let `n` may still have hints, but no hint should
        // appear *inside* the closure param (positions 9..15).
        let param_hint = h.iter().find(|h| {
            h.position.line == 0 && h.position.character >= 9 && h.position.character <= 15
        });
        assert!(
            param_hint.is_none(),
            "annotated closure param produced a hint: {param_hint:?}",
        );
    }

    #[test]
    fn coerced_int_to_float_arg_emits_as_float_hint() {
        // The integer literal `1` is implicitly coerced to `Float` because
        // `take_float`'s `x` param is `Float`. The inlay hint should land
        // immediately after the `1` and read ` as Float`.
        let src = "fn take_float(x: Float) -> Float { x }\nlet r = take_float(x: 1)";
        let h = hints_for(src);

        let coercion_hint = h.iter().find(|h| label(h) == " as Float");
        assert!(
            coercion_hint.is_some(),
            "expected ` as Float` coercion hint, got {h:?}",
        );

        // Source layout: the line `let r = take_float(x: 1)` is line 1 and
        // the `1` literal sits at column 22; its span end (exclusive) is
        // column 23, where the hint should land.
        let hint = coercion_hint.unwrap();
        assert_eq!(
            hint.position,
            Position {
                line: 1,
                character: 23,
            },
            "coercion hint at unexpected position: {hint:?}",
        );
        assert_eq!(hint.kind, Some(InlayHintKind::TYPE));
    }

    #[test]
    fn matching_arg_type_emits_no_coercion_hint() {
        // `1.0` already has type `Float`; no coercion is recorded, so no
        // ` as ...` hint should appear.
        let src = "fn take_float(x: Float) -> Float { x }\nlet r = take_float(x: 1.0)";
        let h = hints_for(src);
        let coercion_hint = h.iter().find(|h| label(h).starts_with(" as "));
        assert!(
            coercion_hint.is_none(),
            "did not expect a coercion hint, got {coercion_hint:?}",
        );
    }
}
