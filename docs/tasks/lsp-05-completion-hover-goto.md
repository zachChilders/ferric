# LSP — Task 5: Completion + Hover + Go-to-Definition

> **Prerequisite:** Task 2 complete (skeleton, pipeline, snapshot, line index).
> Task 1's `Display for Ty` impl is required for hover output.

May run in parallel with tasks 03, 04, 06.

---

## Goal

Implement the three navigation handlers that consume `ResolveResult` and
`TypeResult`. All three share a common operation: **find the AST node at a
cursor position**. That utility is added to `pipeline.rs` (or a new
`ast_lookup.rs`) and used by all three.

| Handler     | Reads from                                               |
|-------------|----------------------------------------------------------|
| Completion  | `ResolveResult` (current or last-good), keyword list, stdlib |
| Hover       | `ParseResult` + `ResolveResult` + `TypeResult`           |
| Goto def    | `ParseResult` + `ResolveResult`                          |

When the current snapshot has no `ResolveResult`, completion and hover fall
back to the last-good resolve snapshot. Goto-def returns `null` rather than
guessing from stale state.

---

## Files

### Add — `crates/ferric_lsp/src/ast_lookup.rs`

Shared utility: walk `ParseResult::items` and return the smallest expression
whose `Span` contains a byte offset.

```rust
use ferric_common::{Block, Expr, Item, NodeId, ParseResult, Span};

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
        Item::Fn(f) => walk_block(&f.body, byte),
        Item::Let(b) => walk_expr(&b.value, byte),
        _ => None,
    }
}

fn walk_block(block: &Block, byte: u32) -> Option<(NodeId, Span)> {
    for stmt_expr in &block.stmts {
        if let Some(hit) = walk_expr(stmt_expr, byte) {
            return Some(hit);
        }
    }
    if let Some(tail) = &block.tail {
        return walk_expr(tail, byte);
    }
    None
}

fn walk_expr(expr: &Expr, byte: u32) -> Option<(NodeId, Span)> {
    let span = expr.span();
    if !contains(span, byte) {
        return None;
    }

    // Recurse into children first; the smallest containing node wins.
    let child_hit = match expr {
        Expr::Block(b)        => walk_block(b, byte),
        Expr::If(i)           => walk_expr(&i.cond, byte)
                                    .or_else(|| walk_block(&i.then_branch, byte))
                                    .or_else(|| i.else_branch.as_ref().and_then(|e| walk_expr(e, byte))),
        Expr::While(w)        => walk_expr(&w.cond, byte).or_else(|| walk_block(&w.body, byte)),
        Expr::Loop(l)         => walk_block(&l.body, byte),
        Expr::Binary(b)       => walk_expr(&b.lhs, byte).or_else(|| walk_expr(&b.rhs, byte)),
        Expr::Unary(u)        => walk_expr(&u.operand, byte),
        Expr::Call(c)         => c.args.iter().find_map(|a| walk_expr(&a.value, byte))
                                    .or_else(|| walk_expr(&c.callee, byte)),
        Expr::Assign(a)       => walk_expr(&a.target, byte).or_else(|| walk_expr(&a.value, byte)),
        Expr::Return(r)       => r.value.as_ref().and_then(|e| walk_expr(e, byte)),
        _ => None,
    };

    if let Some(hit) = child_hit {
        return Some(hit);
    }

    // No child matched; if this expr is itself an identifier, return it.
    if let Expr::Ident(id) = expr {
        return Some((id.node_id, id.span));
    }

    None
}

fn contains(span: Span, byte: u32) -> bool {
    span.start <= byte && byte <= span.end
}
```

> **Field caveats:** the exact shape of `Expr` variants depends on the current
> AST. The point of this walk is: **for every variant that contains
> sub-expressions or sub-blocks, recurse**. If a variant exists in the AST
> that this walk does not handle, add an arm. The default arm of `_ => None`
> is intentional — variants without sub-expressions (literals, `break`,
> `continue`) need no recursion.

Add `mod ast_lookup;` to `crates/ferric_lsp/src/main.rs`.

### Add — `crates/ferric_lsp/src/stdlib_names.rs`

Stdlib function names are not in `ResolveResult` (they are injected by the VM).
A static list is the right approach here.

```rust
pub const STDLIB_FUNCTIONS: &[(&str, &str)] = &[
    ("println",      "fn(s: Str) -> Unit"),
    ("print",        "fn(s: Str) -> Unit"),
    ("int_to_str",   "fn(n: Int) -> Str"),
    ("float_to_str", "fn(n: Float) -> Str"),
    ("bool_to_str",  "fn(b: Bool) -> Str"),
    ("int_to_float", "fn(n: Int) -> Float"),
];
```

Add `mod stdlib_names;` to `crates/ferric_lsp/src/main.rs`.

### Replace — `crates/ferric_lsp/src/handlers/completion.rs`

```rust
use ferric_common::keywords::KEYWORDS;
use tower_lsp::lsp_types::{
    CompletionItem, CompletionItemKind, CompletionResponse, Position,
};

use crate::pipeline::PipelineSnapshot;
use crate::stdlib_names::STDLIB_FUNCTIONS;

pub fn complete(
    snapshot:  &PipelineSnapshot,
    last_good: Option<&PipelineSnapshot>,
    _pos:      Position,
) -> CompletionResponse {
    let mut items = Vec::new();

    // Keywords — always available
    for &kw in KEYWORDS {
        items.push(CompletionItem {
            label: kw.into(),
            kind:  Some(CompletionItemKind::KEYWORD),
            ..Default::default()
        });
    }

    // Stdlib — always available
    for (name, signature) in STDLIB_FUNCTIONS {
        items.push(CompletionItem {
            label:  (*name).into(),
            kind:   Some(CompletionItemKind::FUNCTION),
            detail: Some((*signature).into()),
            ..Default::default()
        });
    }

    // Resolved names from the current snapshot, or last-good if absent
    let resolve_source = snapshot.resolve.as_ref()
        .or_else(|| last_good.and_then(|s| s.resolve.as_ref()));
    let interner = if snapshot.resolve.is_some() {
        &snapshot.interner
    } else if let Some(g) = last_good {
        &g.interner
    } else {
        &snapshot.interner
    };

    if let Some(resolve) = resolve_source {
        for def in resolve.definitions() {
            let label = interner.resolve(def.name).to_string();
            items.push(CompletionItem {
                label,
                kind: Some(match def.kind {
                    // Map DefKind variants to LSP kinds. Adapt to actual enum.
                    _ => CompletionItemKind::VARIABLE,
                }),
                ..Default::default()
            });
        }
    }

    CompletionResponse::Array(items)
}
```

> **API caveat:** `resolve.definitions()` and `def.name`/`def.kind` reflect the
> intent — the actual `ResolveResult` shape determines field access. If
> `ResolveResult` exposes only `resolutions: HashMap<NodeId, DefId>` and a
> separate `defs: Vec<Def>`, iterate over the latter. The point: pull every
> defined name and emit a completion item.

### Replace — `crates/ferric_lsp/src/handlers/hover.rs`

```rust
use tower_lsp::lsp_types::{
    Hover, HoverContents, MarkupContent, MarkupKind, Position,
};

use crate::ast_lookup::find_ident_at_byte;
use crate::pipeline::PipelineSnapshot;

pub fn hover(snapshot: &PipelineSnapshot, pos: Position) -> Option<Hover> {
    let parse = snapshot.parse.as_ref()?;
    let byte  = snapshot.line_index.byte_offset_of(pos);

    let (node_id, span) = find_ident_at_byte(parse, byte)?;

    // Resolve the identifier to its definition.
    let resolve  = snapshot.resolve.as_ref()?;
    let def_id   = resolve.resolutions.get(&node_id).copied()?;
    let def      = resolve.def(def_id)?;
    let name_str = snapshot.interner.resolve(def.name);

    // Look up the type. Type info is keyed by NodeId in TypeResult.
    let type_str = snapshot.typecheck.as_ref()
        .and_then(|t| t.node_types.get(&node_id))
        .map(|ty| format!("{ty}"));

    let body = match type_str {
        Some(ty) => format!("**{name_str}**: {ty}"),
        None     => format!("**{name_str}**"),
    };

    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind:  MarkupKind::Markdown,
            value: body,
        }),
        range: Some(snapshot.line_index.range_of(span)),
    })
}
```

> Hovering on a literal or a keyword returns `None` because `find_ident_at_byte`
> only returns nodes that are `Expr::Ident`. That is correct — hovering on `5`
> shows nothing rather than an error.

### Replace — `crates/ferric_lsp/src/handlers/goto_def.rs`

```rust
use ferric_common::DefId;
use tower_lsp::lsp_types::{
    GotoDefinitionResponse, Location, Position, Url,
};

use crate::ast_lookup::find_ident_at_byte;
use crate::pipeline::PipelineSnapshot;

pub fn goto(
    snapshot: &PipelineSnapshot,
    uri:      &Url,
    pos:      Position,
) -> Option<GotoDefinitionResponse> {
    let parse   = snapshot.parse.as_ref()?;
    let resolve = snapshot.resolve.as_ref()?;

    let byte = snapshot.line_index.byte_offset_of(pos);
    let (node_id, _) = find_ident_at_byte(parse, byte)?;

    let def_id: DefId = resolve.resolutions.get(&node_id).copied()?;
    let def           = resolve.def(def_id)?;

    // Stdlib defs have no source location; return null.
    let span = def.span?;

    Some(GotoDefinitionResponse::Scalar(Location {
        uri:   uri.clone(),
        range: snapshot.line_index.range_of(span),
    }))
}
```

> **API caveat:** `resolve.def(def_id) -> Option<&Def>` and `def.span:
> Option<Span>` are the intended shape. Stdlib/native defs have `span = None`;
> user-defined names have `Some(span)`. If `ResolveResult` does not expose a
> `def(_)` accessor today, add one in `ferric_common` (or whichever crate owns
> `ResolveResult`) — the LSP does not poke into private fields. This is a
> single new method on the public type and is in scope for this task.

---

## Done when

**Completion:**
- [ ] All `KEYWORDS` appear with `CompletionItemKind::KEYWORD`
- [ ] All stdlib functions appear with `CompletionItemKind::FUNCTION` and a
      signature `detail`
- [ ] All resolved local + function names appear
- [ ] When the current snapshot has no resolve, completions still include
      keywords + stdlib + last-good resolve names
- [ ] Returns an empty list (not an error) if no resolve is available at all

**Hover:**
- [ ] Hovering an identifier returns `**name**: Ty` formatted as Markdown
- [ ] Hovering an identifier with no type info returns `**name**` only
- [ ] Hovering a literal, keyword, or whitespace returns `None`
- [ ] `range` on the returned `Hover` covers the identifier
- [ ] Uses the `Display for Ty` impl from Task 1 — no separate type formatter

**Goto-def:**
- [ ] Cursor on a local variable navigates to its `let` site
- [ ] Cursor on a function call navigates to its `fn` definition site
- [ ] Cursor on a stdlib name (`println`, etc.) returns `None` (no source)
- [ ] Cursor on whitespace, literals, or out-of-scope names returns `None`
      and does not crash
- [ ] Returns a `Location` with the same `uri` as the cursor's document (no
      cross-file goto until module system M7 is wired)

**Shared:**
- [ ] All three handlers use `find_ident_at_byte` from `ast_lookup.rs`
- [ ] `find_ident_at_byte` returns the **smallest** containing identifier
      (correctness check: nested calls resolve to the inner identifier)
- [ ] No handler reads internal types from any stage crate
