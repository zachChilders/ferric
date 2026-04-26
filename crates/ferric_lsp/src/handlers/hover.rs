//! `textDocument/hover`.
//!
//! Resolves the variable under the cursor to its definition and renders
//! `**name**: Ty` as Markdown. Falls back to `**name**` when the type
//! checker has no entry for that NodeId. Returns `None` for cursors on
//! literals, keywords, or whitespace — `find_ident_at_byte` only matches
//! `Expr::Variable` nodes.
//!
//! Type rendering uses the `Display for Ty` impl from LSP Task 1.

use tower_lsp::lsp_types::{
    Hover, HoverContents, MarkupContent, MarkupKind, Position,
};

use crate::ast_lookup::find_ident_at_byte;
use crate::pipeline::PipelineSnapshot;

pub fn hover(snapshot: &PipelineSnapshot, pos: Position) -> Option<Hover> {
    let parse = snapshot.parse.as_ref()?;
    let byte = snapshot.line_index.byte_offset_of(pos);

    let (use_node_id, span) = find_ident_at_byte(parse, byte)?;

    // Resolve the use to its definition (so we can render the canonical name
    // even if the use site is shadowed or aliased).
    let resolve  = snapshot.resolve.as_ref()?;
    let def_id   = resolve.resolutions.get(&use_node_id).copied()?;
    let def_info = resolve.def(def_id)?;
    let name_str = snapshot.interner.resolve(def_info.name).to_string();

    // Type info is keyed by *use* NodeId (the type checker assigns a type to
    // every expression node, including variable references).
    let type_str = snapshot
        .typecheck
        .as_ref()
        .and_then(|t| t.node_types.get(&use_node_id))
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::run_pipeline;

    fn hover_at(src: &str, byte: u32) -> Option<Hover> {
        let snap = run_pipeline("file:///tmp/t.fe".into(), 1, src.into());
        let pos = snap.line_index.position_of(byte);
        hover(&snap, pos)
    }

    fn body(h: &Hover) -> &str {
        match &h.contents {
            HoverContents::Markup(m) => &m.value,
            _ => panic!("expected Markup hover"),
        }
    }

    #[test]
    fn hover_on_typed_variable_shows_name_and_type() {
        // "let x = 1\nx" — cursor on the trailing `x` (byte 10).
        let h = hover_at("let x = 1\nx", 10).expect("hover should resolve");
        assert_eq!(body(&h), "**x**: Int");
    }

    #[test]
    fn hover_on_literal_returns_none() {
        // Cursor on the literal `42`.
        assert!(hover_at("let x = 42", 8).is_none());
    }

    #[test]
    fn hover_on_keyword_returns_none() {
        // Cursor on the `l` of `let`.
        assert!(hover_at("let x = 1", 0).is_none());
    }

    #[test]
    fn hover_range_covers_the_identifier() {
        let h = hover_at("let x = 1\nx", 10).unwrap();
        let r = h.range.unwrap();
        // The trailing `x` is at line 1, character 0–1.
        assert_eq!(r.start.line, 1);
        assert_eq!(r.start.character, 0);
        assert_eq!(r.end.line, 1);
        assert_eq!(r.end.character, 1);
    }
}
