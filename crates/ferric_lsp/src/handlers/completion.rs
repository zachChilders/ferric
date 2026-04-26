//! `textDocument/completion`.
//!
//! Items emitted, in this order:
//!   1. Every keyword from `ferric_common::keywords::KEYWORDS`.
//!   2. Every stdlib function from `crate::stdlib_names::STDLIB_FUNCTIONS`,
//!      with a signature `detail`.
//!   3. Every name in the snapshot's `ResolveResult::defs` (or the last-good
//!      snapshot's, if the current one has no resolve yet).
//!
//! Type-aware completions (method completions on `.`, field completions on
//! struct values) wait for a future milestone — they require an LSP-side
//! type lookup that's not in scope here.

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

    // 1. Keywords — always available.
    for &kw in KEYWORDS {
        items.push(CompletionItem {
            label: kw.into(),
            kind:  Some(CompletionItemKind::KEYWORD),
            ..Default::default()
        });
    }

    // 2. Stdlib — always available, with signature in `detail`.
    for (name, signature) in STDLIB_FUNCTIONS {
        items.push(CompletionItem {
            label:  (*name).into(),
            kind:   Some(CompletionItemKind::FUNCTION),
            detail: Some((*signature).into()),
            ..Default::default()
        });
    }

    // 3. Resolved names. Prefer the current snapshot; fall back to last-good
    //    when the current snapshot has no resolve (parse failed, stage panic,
    //    etc.). The interner used to render names must match the snapshot
    //    that produced the defs — symbols are not portable across runs.
    let (defs_source, interner) = match snapshot.resolve.as_ref() {
        Some(r) => (Some(r), &snapshot.interner),
        None    => match last_good {
            Some(g) => (g.resolve.as_ref(), &g.interner),
            None    => (None, &snapshot.interner),
        },
    };

    if let Some(resolve) = defs_source {
        for info in resolve.defs.values() {
            // Skip entries whose interner can't resolve them — defensive
            // against last-good drift if the resolver ever changed shape.
            let label = interner.resolve(info.name).to_string();
            if label.is_empty() {
                continue;
            }
            items.push(CompletionItem {
                label,
                // `DefInfo` doesn't yet carry kind detail; map everything to
                // VARIABLE. A future polish refines functions/types.
                kind: Some(CompletionItemKind::VARIABLE),
                ..Default::default()
            });
        }
    }

    CompletionResponse::Array(items)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::run_pipeline;

    fn complete_for(src: &str) -> Vec<CompletionItem> {
        let snap = run_pipeline("file:///tmp/t.fe".into(), 1, src.into());
        match complete(&snap, None, Position { line: 0, character: 0 }) {
            CompletionResponse::Array(v) => v,
            _ => panic!("handler should return Array"),
        }
    }

    #[test]
    fn every_keyword_appears() {
        let items = complete_for("");
        for &kw in KEYWORDS {
            let found = items.iter().any(|i| i.label == kw && i.kind == Some(CompletionItemKind::KEYWORD));
            assert!(found, "keyword `{kw}` missing from completions");
        }
    }

    #[test]
    fn every_stdlib_fn_appears_with_signature() {
        let items = complete_for("");
        for (name, sig) in STDLIB_FUNCTIONS {
            let found = items.iter().any(|i| {
                i.label == *name
                    && i.kind == Some(CompletionItemKind::FUNCTION)
                    && i.detail.as_deref() == Some(*sig)
            });
            assert!(found, "stdlib `{name}` missing or has wrong detail");
        }
    }

    #[test]
    fn user_defined_names_appear() {
        let items = complete_for("fn greet(who: Str) -> Unit { } let answer = 42");
        assert!(items.iter().any(|i| i.label == "greet"), "missing user fn");
        assert!(items.iter().any(|i| i.label == "answer"), "missing user let");
    }

    #[test]
    fn last_good_used_when_current_has_no_resolve() {
        // `last_good` carries the user-defined `greet`; current has none.
        let good = run_pipeline("file:///tmp/t.fe".into(), 1, "fn greet() -> Unit { }".into());
        let mut bad = run_pipeline("file:///tmp/t.fe".into(), 2, "@".into());
        bad.resolve = None;

        let items = match complete(&bad, Some(&good), Position { line: 0, character: 0 }) {
            CompletionResponse::Array(v) => v,
            _ => panic!(),
        };
        assert!(items.iter().any(|i| i.label == "greet"), "fallback to last-good failed");
    }

    #[test]
    fn empty_when_no_resolve_anywhere() {
        // No fallback either — but keywords + stdlib should still appear.
        let mut snap = run_pipeline("file:///tmp/t.fe".into(), 1, "@".into());
        snap.resolve = None;
        let items = match complete(&snap, None, Position { line: 0, character: 0 }) {
            CompletionResponse::Array(v) => v,
            _ => panic!(),
        };
        // Keywords and stdlib still come through; only user defs are absent.
        assert!(!items.is_empty(), "expected keywords + stdlib at minimum");
        assert!(items.iter().any(|i| i.label == "let"));
        assert!(items.iter().any(|i| i.label == "println"));
    }
}
