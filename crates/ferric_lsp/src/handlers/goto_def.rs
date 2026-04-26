//! `textDocument/definition`.
//!
//! Resolves the variable under the cursor to its definition and returns a
//! `Location` pointing at the binder. Returns `None` for:
//!  - cursor on a non-identifier (literal, keyword, whitespace)
//!  - identifiers not in `ResolveResult::resolutions` (parse errors, etc.)
//!  - native / stdlib names (`DefInfo::span` is `None`)
//!
//! Cross-file goto-def has to wait for the module system; for now every
//! `Location` carries the cursor's own document URI.

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
    let (use_node_id, _) = find_ident_at_byte(parse, byte)?;

    let def_id   = resolve.resolutions.get(&use_node_id).copied()?;
    let def_info = resolve.def(def_id)?;

    // Native definitions have no source span — return null per LSP spec.
    let span = def_info.span?;

    Some(GotoDefinitionResponse::Scalar(Location {
        uri:   uri.clone(),
        range: snapshot.line_index.range_of(span),
    }))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::run_pipeline;

    fn goto_at(src: &str, byte: u32) -> Option<GotoDefinitionResponse> {
        let snap = run_pipeline("file:///tmp/t.fe".into(), 1, src.into());
        let uri  = Url::parse("file:///tmp/t.fe").unwrap();
        let pos  = snap.line_index.position_of(byte);
        goto(&snap, &uri, pos)
    }

    fn scalar(r: GotoDefinitionResponse) -> Location {
        match r {
            GotoDefinitionResponse::Scalar(loc) => loc,
            _ => panic!("expected scalar goto response"),
        }
    }

    #[test]
    fn local_variable_navigates_to_let() {
        // "let x = 1\nx" — cursor on trailing `x` should navigate into the
        // `let x = 1` statement on line 0. The AST doesn't track name spans
        // separately, so the navigation target is the full `let` statement
        // span starting at column 0; the editor lands the cursor on that
        // line. The doc permits this fallback.
        let resp = goto_at("let x = 1\nx", 10).expect("goto returns Some");
        let loc  = scalar(resp);
        assert_eq!(loc.range.start.line, 0);
        // Column 0 = start of `let` (start of full statement span).
        assert_eq!(loc.range.start.character, 0);
        // The end column should cover at least up to the binding's `x`.
        assert!(loc.range.end.character >= 5, "end={:?}", loc.range.end);
    }

    #[test]
    fn function_call_navigates_to_fn_definition() {
        // The call site `id(...)` should navigate to the `id` definition.
        let src = "fn id(n: Int) -> Int { n }\nlet y = id(n: 1)";
        // Find cursor byte in `id` of the call (e.g. byte 35 — the `i` of `id`).
        let call_id_byte = src.find("id(n: 1)").unwrap() as u32;
        let resp = goto_at(src, call_id_byte).expect("goto on call");
        let loc  = scalar(resp);
        // Should point at the FnDef span, which covers the whole `fn id(...) { n }`.
        // We just assert it's at line 0 (where the fn starts).
        assert_eq!(loc.range.start.line, 0);
    }

    #[test]
    fn stdlib_name_returns_none() {
        // Cursor on `println` (stdlib, no source span).
        let src   = "println(s: \"hi\")";
        let pbyte = src.find("println").unwrap() as u32;
        assert!(goto_at(src, pbyte).is_none());
    }

    #[test]
    fn cursor_on_literal_returns_none() {
        assert!(goto_at("let x = 42", 8).is_none());
    }

    #[test]
    fn cursor_on_unresolved_name_returns_none_no_panic() {
        // `nonexistent` isn't defined; ResolveResult won't map it.
        let src   = "nonexistent";
        let resp  = goto_at(src, 0);
        assert!(resp.is_none(), "expected None for unresolved, got {resp:?}");
    }

    #[test]
    fn location_uri_matches_the_request_uri() {
        let resp = goto_at("let x = 1\nx", 10).expect("goto returns Some");
        let loc  = scalar(resp);
        assert_eq!(loc.uri.as_str(), "file:///tmp/t.fe");
    }
}
