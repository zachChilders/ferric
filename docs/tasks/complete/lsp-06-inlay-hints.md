# LSP — Task 6: Inlay Hints

> **Prerequisite:** Task 2 complete. Task 1's `Display for Ty` impl is required.

May run in parallel with tasks 03, 04, 05.

---

## Goal

Implement `textDocument/inlayHint`. Hints render inferred types inline as
grayed-out decorations. Two sites emit hints in this milestone:

1. **`let` bindings without annotations** — hint placed after the binding name
2. **Closure parameters** — hint placed after the parameter name

Hints are **suppressed entirely** when `TypeResult` is unavailable. Stale hints
on actively-edited code are confusing — a missing hint is better than a wrong
one. The last-good type result is **not** used here, unlike completion and
hover.

Hints are filtered to the LSP-supplied visible `Range`. Off-screen hints are
not returned.

---

## Files

### Replace — `crates/ferric_lsp/src/handlers/inlay_hints.rs`

```rust
use ferric_common::{Block, Expr, Item, NodeId, ParseResult, Span, Ty, TypeResult};
use tower_lsp::lsp_types::{
    InlayHint, InlayHintKind, InlayHintLabel, Position, Range,
};

use crate::pipeline::{LineIndex, PipelineSnapshot};

pub fn inlay_hints(snapshot: &PipelineSnapshot, range: Range) -> Vec<InlayHint> {
    // Hints require a *current* type result. No fallback to last-good.
    let (Some(parse), Some(types)) = (&snapshot.parse, &snapshot.typecheck)
    else { return vec![]; };

    let mut hints = Vec::new();
    let li = &snapshot.line_index;

    for item in &parse.items {
        collect_for_item(item, types, li, range, &mut hints);
    }

    hints
}

fn collect_for_item(
    item:  &Item,
    types: &TypeResult,
    li:    &LineIndex,
    range: Range,
    hints: &mut Vec<InlayHint>,
) {
    match item {
        Item::Let(b) if b.annotation.is_none() => {
            push_let_hint(b.node_id, b.name_span, types, li, range, hints);
        }
        Item::Fn(f) => {
            // Top-level fn parameters must be annotated, so no hints there.
            // Recurse into the body for nested lets and closures.
            collect_for_block(&f.body, types, li, range, hints);
        }
        _ => {}
    }
}

fn collect_for_block(
    block: &Block,
    types: &TypeResult,
    li:    &LineIndex,
    range: Range,
    hints: &mut Vec<InlayHint>,
) {
    for stmt in &block.stmts {
        collect_for_expr(stmt, types, li, range, hints);
    }
    if let Some(tail) = &block.tail {
        collect_for_expr(tail, types, li, range, hints);
    }
}

fn collect_for_expr(
    expr:  &Expr,
    types: &TypeResult,
    li:    &LineIndex,
    range: Range,
    hints: &mut Vec<InlayHint>,
) {
    match expr {
        Expr::Let(b) if b.annotation.is_none() => {
            push_let_hint(b.node_id, b.name_span, types, li, range, hints);
            collect_for_expr(&b.value, types, li, range, hints);
        }
        Expr::Let(b) => {
            collect_for_expr(&b.value, types, li, range, hints);
        }
        Expr::Closure(c) => {
            for param in &c.params {
                if param.annotation.is_none() {
                    push_param_hint(param.node_id, param.name_span, types, li, range, hints);
                }
            }
            collect_for_expr(&c.body, types, li, range, hints);
        }
        Expr::Block(b) => collect_for_block(b, types, li, range, hints),
        Expr::If(i) => {
            collect_for_expr(&i.cond, types, li, range, hints);
            collect_for_block(&i.then_branch, types, li, range, hints);
            if let Some(e) = &i.else_branch { collect_for_expr(e, types, li, range, hints); }
        }
        Expr::While(w) => {
            collect_for_expr(&w.cond, types, li, range, hints);
            collect_for_block(&w.body, types, li, range, hints);
        }
        Expr::Loop(l) => collect_for_block(&l.body, types, li, range, hints),
        Expr::Binary(b) => {
            collect_for_expr(&b.lhs, types, li, range, hints);
            collect_for_expr(&b.rhs, types, li, range, hints);
        }
        Expr::Unary(u)  => collect_for_expr(&u.operand, types, li, range, hints),
        Expr::Call(c)   => {
            for arg in &c.args { collect_for_expr(&arg.value, types, li, range, hints); }
            collect_for_expr(&c.callee, types, li, range, hints);
        }
        Expr::Assign(a) => {
            collect_for_expr(&a.target, types, li, range, hints);
            collect_for_expr(&a.value,  types, li, range, hints);
        }
        Expr::Return(r) => {
            if let Some(e) = &r.value { collect_for_expr(e, types, li, range, hints); }
        }
        _ => {} // literals, idents, break, continue — no hints needed
    }
}

fn push_let_hint(
    node_id: NodeId,
    name_span: Span,
    types: &TypeResult,
    li: &LineIndex,
    range: Range,
    hints: &mut Vec<InlayHint>,
) {
    let Some(ty) = types.node_types.get(&node_id) else { return; };
    let position = li.position_of(name_span.end);
    if !position_in_range(position, range) { return; }
    hints.push(make_hint(position, ty));
}

fn push_param_hint(
    node_id: NodeId,
    name_span: Span,
    types: &TypeResult,
    li: &LineIndex,
    range: Range,
    hints: &mut Vec<InlayHint>,
) {
    let Some(ty) = types.node_types.get(&node_id) else { return; };
    let position = li.position_of(name_span.end);
    if !position_in_range(position, range) { return; }
    hints.push(make_hint(position, ty));
}

fn make_hint(position: Position, ty: &Ty) -> InlayHint {
    InlayHint {
        position,
        label:           InlayHintLabel::String(format!(": {ty}")),
        kind:            Some(InlayHintKind::TYPE),
        text_edits:      None,
        tooltip:         None,
        padding_left:    Some(false),
        padding_right:   Some(false),
        data:            None,
    }
}

fn position_in_range(p: Position, r: Range) -> bool {
    let after_start = p.line > r.start.line
        || (p.line == r.start.line && p.character >= r.start.character);
    let before_end  = p.line < r.end.line
        || (p.line == r.end.line && p.character <= r.end.character);
    after_start && before_end
}
```

> **Field caveats:**
> - `Item::Let(b)` and `Expr::Let(b)` may be one variant or two — adapt to the
>   actual AST. Many languages put `let` only as a statement, but Ferric
>   currently treats it as both. Check `ferric_common`.
> - `b.annotation: Option<TypeAnnotation>` and `b.name_span: Span` and
>   `b.node_id: NodeId` are the assumed fields. Match what the AST actually
>   exposes — at minimum, the binding must carry the `NodeId` that the type
>   checker keys against.
> - `Expr::Closure` does not exist yet in M2. If closures are not in the AST
>   at the time this task runs, drop the closure arm and the closure-parameter
>   hint case from the implementation. Re-add it when closures land. Update
>   the `Done when` accordingly.
> - `c.params[i].annotation` and `c.params[i].name_span` mirror the `let`
>   binding fields.
>
> The point of this handler is: **for every unannotated binder whose type the
> checker has inferred, push one hint at the end of the binder's name**.

---

## Done when

- [ ] Unannotated `let` bindings show a hint immediately after the binder name
- [ ] `let x: Int = ...` (annotated) shows **no** hint
- [ ] Closure parameters without annotations show a hint after the parameter
      name (omit this requirement if closures don't exist yet)
- [ ] Top-level `fn` parameters never produce hints (always annotated)
- [ ] If `snapshot.typecheck` is `None`, the handler returns `vec![]` — no
      stale hints
- [ ] Hint label is exactly `: {ty}` using `Display for Ty` from Task 1
- [ ] Hints are filtered to the requested LSP `Range` — hints whose position
      is outside the range are not returned
- [ ] Hint `kind` is `InlayHintKind::TYPE`
- [ ] No `padding_left`/`padding_right` (the `:` is part of the label text)
- [ ] Adding a new `Ty` variant without a `Display` arm is a compile error
      (inherited from Task 1's exhaustiveness)
- [ ] Manual VS Code test: open `examples/hello.fe`, observe `: Int` after
      every unannotated `let` binder, observe no hints after annotated ones
