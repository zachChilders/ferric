//! `textDocument/publishDiagnostics`.
//!
//! Every `LexError`, `ParseError`, `ResolveError`, and `TypeError` becomes
//! one LSP `Diagnostic`. A stage panic (caught by `catch_unwind` in
//! `pipeline.rs`) results in exactly one diagnostic at line 1 with a
//! synthetic "internal compiler error" message — the LSP keeps running.
//!
//! `ferric_common::errors` exposes `.span()` and `.description()` on every
//! error enum, so this handler stays out of the per-variant detail.

use ferric_common::Span;
use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString, Position, Range};

use crate::pipeline::{LineIndex, PipelineSnapshot};

pub fn publish(snapshot: &PipelineSnapshot) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let li = &snapshot.line_index;

    // Stage 1: lex. A `None` result here means the lexer itself panicked —
    // every later stage was skipped, so emit a single panic diag and exit.
    if let Some(lex) = &snapshot.lex {
        for err in &lex.errors {
            out.push(diag(err.span(), DiagnosticSeverity::ERROR, err.description(), "lex", li));
        }
    } else {
        out.push(panic_diag("lex"));
        return out;
    }

    // Stage 2: parse. Lex errors do NOT block parsing — the lexer emits
    // recovery tokens. So parse runs even with lex errors. A missing parse
    // result here therefore means parse panicked.
    if let Some(parse) = &snapshot.parse {
        for err in &parse.errors {
            out.push(diag(err.span(), DiagnosticSeverity::ERROR, err.description(), "parse", li));
        }
    } else {
        out.push(panic_diag("parse"));
        return out;
    }

    // Stage 3: resolve.
    if let Some(resolve) = &snapshot.resolve {
        for err in &resolve.errors {
            // TODO(M2.5-task-2): `require(warn)` failures should map to
            //   DiagnosticSeverity::WARNING. The current `ResolveError` enum
            //   has no warn-mode discriminator — when M2.5 Task 2 wires one
            //   in (e.g. `ResolveError::RequireWarn { … }` or a `mode` field
            //   on a generic `Require*` variant), branch here.
            out.push(diag(err.span(), DiagnosticSeverity::ERROR, err.description(), "resolve", li));
        }
    } else {
        out.push(panic_diag("resolve"));
        return out;
    }

    // Stage 4: typecheck.
    if let Some(types) = &snapshot.typecheck {
        for err in &types.errors {
            out.push(diag(err.span(), DiagnosticSeverity::ERROR, err.description(), "type", li));
        }
    } else {
        out.push(panic_diag("typecheck"));
    }

    out
}

fn diag(
    span:     Span,
    severity: DiagnosticSeverity,
    msg:      String,
    code:     &str,
    li:       &LineIndex,
) -> Diagnostic {
    Diagnostic {
        range:    li.range_of(span),
        severity: Some(severity),
        code:     Some(NumberOrString::String(code.into())),
        source:   Some("ferric".into()),
        message:  msg,
        ..Default::default()
    }
}

fn panic_diag(stage: &str) -> Diagnostic {
    Diagnostic {
        range: Range {
            start: Position { line: 0, character: 0 },
            end:   Position { line: 0, character: 0 },
        },
        severity: Some(DiagnosticSeverity::ERROR),
        code:     Some(NumberOrString::String("internal".into())),
        source:   Some("ferric".into()),
        message:  format!("internal compiler error: {stage} stage panicked"),
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::run_pipeline;

    fn run(src: &str) -> Vec<Diagnostic> {
        let snap = run_pipeline("file:///tmp/t.fe".into(), 1, src.into());
        publish(&snap)
    }

    #[test]
    fn clean_program_has_no_diagnostics() {
        let diags = run("fn main() -> Unit { }");
        assert!(diags.is_empty(), "unexpected diags: {diags:?}");
    }

    #[test]
    fn unterminated_string_yields_one_diag() {
        let diags = run("let x = \"oops");
        assert_eq!(diags.len(), 1);
        let d = &diags[0];
        assert_eq!(d.severity, Some(DiagnosticSeverity::ERROR));
        assert_eq!(d.source.as_deref(), Some("ferric"));
        assert_eq!(d.code, Some(NumberOrString::String("lex".into())));
    }

    #[test]
    fn parse_error_uses_parse_code() {
        // Positional args are forbidden by M2.5; this trips a ParseError.
        let diags = run("fn f(x: Int) -> Int { x } let r = f(5)");
        let parse_diag = diags.iter().find(|d| d.code == Some(NumberOrString::String("parse".into())));
        assert!(parse_diag.is_some(), "expected a parse-stage diagnostic in {diags:?}");
    }

    #[test]
    fn diagnostic_range_lands_on_the_right_line() {
        // The `\n` puts the lex error on line 1 (0-indexed).
        let diags = run("let x = 1\nlet y = \"oops");
        let d = diags.first().expect("at least one diag");
        assert_eq!(d.range.start.line, 1, "diag range: {:?}", d.range);
    }

    #[test]
    fn no_duplicate_diagnostics_for_same_error() {
        // Same source twice → diagnostics must be identical, not doubled.
        let diags1 = run("@invalid");
        let diags2 = run("@invalid");
        assert_eq!(diags1.len(), diags2.len());
        assert!(!diags1.is_empty());
    }
}
