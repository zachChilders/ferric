# LSP — Task 4: Diagnostics + Document Symbols

> **Prerequisite:** Task 2 complete. The crate skeleton, `PipelineSnapshot`,
> `LineIndex`, and stub handler modules must already exist. This task replaces
> the bodies of `handlers/diagnostics.rs` and `handlers/document_symbols.rs`.

May run in parallel with tasks 03, 05, 06.

---

## Goal

Implement the two handlers that need only the lex/parse stage outputs and the
`Span` already on every error and AST item. Neither handler reads `TypeResult`
or `ResolveResult`.

- **Diagnostics:** every `LexError`, `ParseError`, `ResolveError`, `TypeError`
  becomes one LSP `Diagnostic`. Severity is `ERROR` for all stage errors
  except `require(warn)` failures, which are `WARNING`.
- **Document symbols:** every top-level `fn` and `let` becomes a flat
  `SymbolInformation` (or a single-level `DocumentSymbol`).

---

## Files

### Replace — `crates/ferric_lsp/src/handlers/diagnostics.rs`

```rust
use ferric_common::{Span, LexError, ParseError, ResolveError, TypeError};
use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString, Range};

use crate::pipeline::{LineIndex, PipelineSnapshot};

pub fn publish(snapshot: &PipelineSnapshot) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let li = &snapshot.line_index;

    if let Some(lex) = &snapshot.lex {
        for err in &lex.errors {
            out.push(diag_from_lex(err, li));
        }
    } else {
        out.push(panic_diag("lex stage panicked"));
        return out;
    }

    if let Some(parse) = &snapshot.parse {
        for err in &parse.errors {
            out.push(diag_from_parse(err, li));
        }
    } else if snapshot.lex.is_some() {
        out.push(panic_diag("parse stage panicked"));
    }

    if let Some(resolve) = &snapshot.resolve {
        for err in &resolve.errors {
            out.push(diag_from_resolve(err, li));
        }
    } else if snapshot.parse.is_some() {
        out.push(panic_diag("resolve stage panicked"));
    }

    if let Some(types) = &snapshot.typecheck {
        for err in &types.errors {
            out.push(diag_from_type(err, li));
        }
    } else if snapshot.resolve.is_some() {
        out.push(panic_diag("typecheck stage panicked"));
    }

    out
}

fn diag(span: Span, severity: DiagnosticSeverity, msg: String, code: &str, li: &LineIndex)
    -> Diagnostic
{
    Diagnostic {
        range:    li.range_of(span),
        severity: Some(severity),
        code:     Some(NumberOrString::String(code.into())),
        source:   Some("ferric".into()),
        message:  msg,
        ..Default::default()
    }
}

fn diag_from_lex(err: &LexError, li: &LineIndex) -> Diagnostic {
    diag(err.span, DiagnosticSeverity::ERROR, err.message().into(), "lex", li)
}

fn diag_from_parse(err: &ParseError, li: &LineIndex) -> Diagnostic {
    diag(err.span, DiagnosticSeverity::ERROR, err.message().into(), "parse", li)
}

fn diag_from_resolve(err: &ResolveError, li: &LineIndex) -> Diagnostic {
    let severity = if err.is_warning() {
        DiagnosticSeverity::WARNING
    } else {
        DiagnosticSeverity::ERROR
    };
    diag(err.span, severity, err.message().into(), "resolve", li)
}

fn diag_from_type(err: &TypeError, li: &LineIndex) -> Diagnostic {
    diag(err.span, DiagnosticSeverity::ERROR, err.message().into(), "type", li)
}

fn panic_diag(msg: &str) -> Diagnostic {
    Diagnostic {
        range: Range {
            start: tower_lsp::lsp_types::Position { line: 0, character: 0 },
            end:   tower_lsp::lsp_types::Position { line: 0, character: 0 },
        },
        severity: Some(DiagnosticSeverity::ERROR),
        code:     Some(NumberOrString::String("internal".into())),
        source:   Some("ferric".into()),
        message:  format!("internal compiler error: {msg}"),
        ..Default::default()
    }
}
```

> **Important:** the methods `err.message()` and `err.is_warning()` are the
> intended API surface but the actual error types in `ferric_common` may use
> `err.msg` (field) or `err.kind` (enum) instead. Match what the existing
> error types provide. The point of this code is: **read `Span` and a string
> from each error; return one LSP `Diagnostic` per error**. Adapt field access
> to whatever the error structs actually expose.
>
> If `ResolveError` does not have an `is_warning()` discriminator yet, add one
> for the `require(warn)` case from M2.5 Task 2 — a warn-mode `require` failure
> should set a flag on the resulting error so the LSP can route it to
> `WARNING`. If the warn flag is not yet wired, route everything to `ERROR`
> for now and leave a `TODO(M2.5-task-2)` comment.

### Replace — `crates/ferric_lsp/src/handlers/document_symbols.rs`

```rust
use ferric_common::{Item, ParseResult};
use tower_lsp::lsp_types::{
    DocumentSymbol, DocumentSymbolResponse, SymbolKind,
};

use crate::pipeline::PipelineSnapshot;

pub fn symbols(snapshot: &PipelineSnapshot) -> DocumentSymbolResponse {
    let Some(parse) = &snapshot.parse else {
        return DocumentSymbolResponse::Nested(vec![]);
    };

    let li = &snapshot.line_index;
    let mut out = Vec::new();

    for item in &parse.items {
        match item {
            Item::Fn(f) => {
                let name = snapshot.interner.resolve(f.name).to_string();
                #[allow(deprecated)]
                out.push(DocumentSymbol {
                    name,
                    detail:          None,
                    kind:            SymbolKind::FUNCTION,
                    tags:            None,
                    deprecated:      None,
                    range:           li.range_of(f.span),
                    selection_range: li.range_of(f.name_span),
                    children:        None,
                });
            }
            Item::Let(b) => {
                let name = snapshot.interner.resolve(b.name).to_string();
                #[allow(deprecated)]
                out.push(DocumentSymbol {
                    name,
                    detail:          None,
                    kind:            if b.mutable { SymbolKind::VARIABLE } else { SymbolKind::CONSTANT },
                    tags:            None,
                    deprecated:      None,
                    range:           li.range_of(b.span),
                    selection_range: li.range_of(b.name_span),
                    children:        None,
                });
            }
            _ => {}
        }
    }

    DocumentSymbolResponse::Nested(out)
}
```

> **Field-name caveat:** `Item::Fn(_).span`, `name_span`, `name`, and `mutable`
> are the field names assumed here. Match the actual `ferric_common` struct
> definitions. If the AST does not have `name_span` on `Item::Fn` and
> `Item::Let`, use `span` for both `range` and `selection_range`.

---

## Done when

**Diagnostics:**
- [ ] Every `LexError`, `ParseError`, `ResolveError`, `TypeError` becomes one
      LSP `Diagnostic`
- [ ] Diagnostic ranges are correct (line/column derived from `Span` via
      `LineIndex`)
- [ ] All stage errors default to `DiagnosticSeverity::ERROR`
- [ ] `require(warn)` failures (M2.5 Task 2) appear as
      `DiagnosticSeverity::WARNING` (or `TODO` left if the warn-flag is not
      yet on `ResolveError`)
- [ ] A stage panic results in exactly one diagnostic at line 1 with message
      "internal compiler error: {stage} stage panicked"
- [ ] No duplicate diagnostics are produced for the same version
- [ ] `source` field is `"ferric"` and `code` field identifies the stage

**Document symbols:**
- [ ] Every top-level `fn` definition appears as `SymbolKind::FUNCTION`
- [ ] Every top-level `let` binding appears as `SymbolKind::CONSTANT` (or
      `VARIABLE` if `mut`)
- [ ] `range` covers the whole item; `selection_range` covers the name
- [ ] Returns `DocumentSymbolResponse::Nested(vec![])` if `snapshot.parse` is
      `None` (does not crash)
- [ ] No resolve or type information is required to produce symbols
