//! `textDocument/documentSymbol`.
//!
//! Walks `ParseResult::items` and returns a flat `DocumentSymbol` list. AST
//! items don't carry a separate `name_span`, so `range` and `selection_range`
//! both use the item's full span — the doc explicitly permits this fallback.
//!
//! The spec for this milestone covers `fn` and top-level `let`. We also
//! emit symbols for `struct`, `enum`, `trait`, `impl`, and `type` because
//! the cost is identical and users will expect them in the editor outline.

use ferric_common::{Item, Stmt};
use tower_lsp::lsp_types::{DocumentSymbol, DocumentSymbolResponse, SymbolKind};

use crate::pipeline::PipelineSnapshot;

pub fn symbols(snapshot: &PipelineSnapshot) -> DocumentSymbolResponse {
    let Some(parse) = &snapshot.parse else {
        return DocumentSymbolResponse::Nested(vec![]);
    };

    let li = &snapshot.line_index;
    let mut out = Vec::new();

    for item in &parse.items {
        push_item_symbol(item, snapshot, li, &mut out);
    }

    DocumentSymbolResponse::Nested(out)
}

fn push_item_symbol(
    item: &Item,
    snapshot: &PipelineSnapshot,
    li: &crate::pipeline::LineIndex,
    out: &mut Vec<DocumentSymbol>,
) {
    match item {
        Item::FnDef { name, span, .. } => {
            out.push(make_symbol(
                snapshot.interner.resolve(*name).to_string(),
                SymbolKind::FUNCTION,
                li.range_of(*span),
            ));
        }
        Item::StructDef { name, span, .. } => {
            out.push(make_symbol(
                snapshot.interner.resolve(*name).to_string(),
                SymbolKind::STRUCT,
                li.range_of(*span),
            ));
        }
        Item::EnumDef { name, span, .. } => {
            out.push(make_symbol(
                snapshot.interner.resolve(*name).to_string(),
                SymbolKind::ENUM,
                li.range_of(*span),
            ));
        }
        Item::TraitDef { name, span, .. } => {
            out.push(make_symbol(
                snapshot.interner.resolve(*name).to_string(),
                SymbolKind::INTERFACE,
                li.range_of(*span),
            ));
        }
        Item::ImplBlock { trait_name, type_name, span, .. } => {
            // No single "impl" name in LSP — render `impl Trait for Type`.
            let label = format!(
                "impl {} for {}",
                snapshot.interner.resolve(*trait_name),
                snapshot.interner.resolve(*type_name),
            );
            out.push(make_symbol(label, SymbolKind::NAMESPACE, li.range_of(*span)));
        }
        Item::TypeAlias(decl) => {
            out.push(make_symbol(
                snapshot.interner.resolve(decl.name).to_string(),
                SymbolKind::TYPE_PARAMETER,
                li.range_of(decl.span),
            ));
        }
        Item::Script { stmt: Stmt::Let { name, mutable, span, .. }, .. } => {
            let kind = if *mutable { SymbolKind::VARIABLE } else { SymbolKind::CONSTANT };
            out.push(make_symbol(
                snapshot.interner.resolve(*name).to_string(),
                kind,
                li.range_of(*span),
            ));
        }
        // Top-level expression statements / assignments / requires / for-loops
        // are not meaningful as outline entries.
        Item::Script { .. } => {}
        // Imports/exports are a layer of indirection; the items they reference
        // are emitted directly when they live in this file. Skip them in the
        // outline to avoid duplicate noise. `Item::Export` wraps another item
        // — recurse into its inner so the wrapped definition still appears.
        Item::Export(decl) => {
            push_item_symbol(&decl.item, snapshot, li, out);
        }
        Item::Import(_) => {}
    }
}

fn make_symbol(
    name: String,
    kind: SymbolKind,
    range: tower_lsp::lsp_types::Range,
) -> DocumentSymbol {
    #[allow(deprecated)] // `deprecated` field on DocumentSymbol is itself deprecated.
    DocumentSymbol {
        name,
        detail:          None,
        kind,
        tags:            None,
        deprecated:      None,
        range,
        selection_range: range, // No name_span on AST items — fall back to full span.
        children:        None,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::run_pipeline;

    fn syms(src: &str) -> Vec<DocumentSymbol> {
        let snap = run_pipeline("file:///tmp/t.fe".into(), 1, src.into());
        match symbols(&snap) {
            DocumentSymbolResponse::Nested(v) => v,
            DocumentSymbolResponse::Flat(_) => panic!("handler should return Nested"),
        }
    }

    #[test]
    fn empty_program_has_no_symbols() {
        assert!(syms("").is_empty());
    }

    #[test]
    fn fn_appears_as_function() {
        let s = syms("fn greet(who: Str) -> Unit { println(s: who) }");
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].name, "greet");
        assert_eq!(s[0].kind, SymbolKind::FUNCTION);
    }

    #[test]
    fn immutable_let_is_constant() {
        let s = syms("let x = 1");
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].name, "x");
        assert_eq!(s[0].kind, SymbolKind::CONSTANT);
    }

    #[test]
    fn mut_let_is_variable() {
        let s = syms("let mut counter = 0");
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].kind, SymbolKind::VARIABLE);
    }

    #[test]
    fn structs_and_enums_appear() {
        let src = "struct Point { x: Int, y: Int } enum Color { Red, Green, Blue }";
        let s = syms(src);
        assert!(s.iter().any(|d| d.name == "Point" && d.kind == SymbolKind::STRUCT));
        assert!(s.iter().any(|d| d.name == "Color" && d.kind == SymbolKind::ENUM));
    }

    #[test]
    fn missing_parse_returns_empty_response() {
        // Pipeline always produces a (possibly error-laden) ParseResult — we
        // simulate the pre-parse state by checking the empty-AST behavior
        // through clean source.
        let s = syms("");
        assert!(s.is_empty());
    }

    #[test]
    fn range_and_selection_range_match_when_no_name_span() {
        let s = syms("fn f() -> Unit { }");
        assert_eq!(s[0].range, s[0].selection_range);
    }
}
