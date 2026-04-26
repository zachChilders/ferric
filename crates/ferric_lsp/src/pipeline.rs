//! Pipeline runner + per-document cache.
//!
//! `PipelineCache` stores one `PipelineSnapshot` per (uri, version), plus a
//! last-good snapshot per stage. `run_pipeline` runs the full Ferric pipeline,
//! wrapping each stage in `catch_unwind` so a stage panic is reported as a
//! single diagnostic rather than crashing the server.
//!
//! ## Scaffolding note
//!
//! Several public items here (`PipelineSnapshot::interner`, `LineIndex`'s
//! methods, `last_good_type`, `uri_url`) are not yet called from inside this
//! crate — they are the public surface that the handler implementations in
//! LSP Tasks 04, 05, and 06 will consume. The crate-level `allow(dead_code)`
//! is removed as those tasks land.

#![allow(dead_code)]

use std::sync::Arc;

use dashmap::DashMap;
use ferric_common::{
    Interner, LexResult, ParseResult, ResolveResult, Span, Symbol, TypeAnnotation, TypeResult,
};
use tower_lsp::lsp_types::{Position, Range, Url};

// ---------------------------------------------------------------------------
// Snapshot
// ---------------------------------------------------------------------------

/// Result of one full pipeline run on one version of a document. Immutable
/// once created. Stored in `PipelineCache` indexed by URI.
pub struct PipelineSnapshot {
    pub uri:       String,
    pub version:   i32,
    pub source:    String,
    /// The interner used during this pipeline run. Each run gets a fresh
    /// interner — symbol IDs are NOT comparable across snapshots.
    pub interner:  Interner,

    /// Each stage result is `Some` when the stage ran to completion, `None`
    /// only when a panic was caught (or when a required preceding result was
    /// absent because of an earlier panic). A stage's own `errors` vector
    /// carries non-fatal stage errors and does **not** prevent later stages
    /// from running.
    pub lex:       Option<LexResult>,
    pub parse:     Option<ParseResult>,
    pub resolve:   Option<ResolveResult>,
    pub typecheck: Option<TypeResult>,

    pub line_index: LineIndex,
}

impl PipelineSnapshot {
    /// Re-parses `self.uri` as a `Url`. Used by the diagnostics publisher.
    pub fn uri_url(&self) -> Url {
        Url::parse(&self.uri).expect("snapshot uri should round-trip as a Url")
    }
}

// ---------------------------------------------------------------------------
// Cache
// ---------------------------------------------------------------------------

pub struct PipelineCache {
    documents: DashMap<String, DocumentState>,
}

struct DocumentState {
    current:           Arc<PipelineSnapshot>,
    last_good_lex:     Option<Arc<PipelineSnapshot>>,
    last_good_parse:   Option<Arc<PipelineSnapshot>>,
    last_good_resolve: Option<Arc<PipelineSnapshot>>,
    last_good_type:    Option<Arc<PipelineSnapshot>>,
}

impl PipelineCache {
    pub fn new() -> Self {
        PipelineCache { documents: DashMap::new() }
    }

    pub fn current(&self, uri: &str) -> Option<Arc<PipelineSnapshot>> {
        self.documents.get(uri).map(|s| Arc::clone(&s.current))
    }

    pub fn last_good_resolve(&self, uri: &str) -> Option<Arc<PipelineSnapshot>> {
        self.documents
            .get(uri)
            .and_then(|s| s.last_good_resolve.as_ref().map(Arc::clone))
    }

    pub fn last_good_type(&self, uri: &str) -> Option<Arc<PipelineSnapshot>> {
        self.documents
            .get(uri)
            .and_then(|s| s.last_good_type.as_ref().map(Arc::clone))
    }

    pub fn remove(&self, uri: &str) {
        self.documents.remove(uri);
    }

    /// Run the pipeline for one (uri, version, source) tuple, store the result
    /// in the cache (replacing the prior current snapshot), and return it.
    /// Last-good snapshots are advanced for any stage that produced a result.
    pub fn run_and_store(
        &self,
        uri: &str,
        version: i32,
        source: &str,
    ) -> Arc<PipelineSnapshot> {
        let snap = Arc::new(run_pipeline(
            uri.to_string(),
            version,
            source.to_string(),
        ));

        let mut entry = self
            .documents
            .entry(uri.to_string())
            .or_insert_with(|| DocumentState {
                current:           Arc::clone(&snap),
                last_good_lex:     None,
                last_good_parse:   None,
                last_good_resolve: None,
                last_good_type:    None,
            });

        entry.current = Arc::clone(&snap);
        if snap.lex.is_some()       { entry.last_good_lex     = Some(Arc::clone(&snap)); }
        if snap.parse.is_some()     { entry.last_good_parse   = Some(Arc::clone(&snap)); }
        if snap.resolve.is_some()   { entry.last_good_resolve = Some(Arc::clone(&snap)); }
        if snap.typecheck.is_some() { entry.last_good_type    = Some(Arc::clone(&snap)); }

        snap
    }
}

impl Default for PipelineCache {
    fn default() -> Self { Self::new() }
}

// ---------------------------------------------------------------------------
// Pipeline runner
// ---------------------------------------------------------------------------

/// Runs the full pipeline. Each stage call is wrapped in `catch_unwind`. A
/// panic in a stage results in `None` for that stage and short-circuits later
/// stages (they cannot run without their input).
///
/// Stages with stage-level errors (entries in `errors: Vec<...>`) DO NOT
/// short-circuit — those are the normal "user has a bug" path and later
/// stages run on the partial result so the LSP can keep providing
/// completions and hover.
pub fn run_pipeline(uri: String, version: i32, source: String) -> PipelineSnapshot {
    let line_index = LineIndex::new(&source);
    let mut interner = Interner::new();

    // Stage 1: lex. The lexer mutates the interner — it must be passed
    // through so later stages can resolve `Symbol` -> `&str`.
    let lex_result = catch_stage(std::panic::AssertUnwindSafe(|| {
        ferric_lexer::lex(&source, &mut interner)
    }));

    // Native function table for resolve. Mirrors `native_fn_table` in
    // `src/main.rs`. Long-term this should live in `ferric_stdlib` so the
    // CLI and the LSP cannot drift.
    let native_fns: Vec<(Symbol, Vec<Symbol>)> = stdlib_native_fn_table(&mut interner);
    let builtin_enums = stdlib_builtin_enum_table(&mut interner);

    // Stage 2: parse. Uses the interner-aware variant for better error
    // messages. The lexer is permissive and emits error tokens that the
    // parser can recover from, so parse runs even when lex has errors.
    let parse_result = match &lex_result {
        Some(lex) => catch_stage(std::panic::AssertUnwindSafe(|| {
            ferric_parser::parse_with_interner(lex, &interner)
        })),
        None => None,
    };

    // Stage 3: resolve.
    let resolve_result = match &parse_result {
        Some(ast) => catch_stage(std::panic::AssertUnwindSafe(|| {
            ferric_resolve::resolve_with_natives_and_builtins(ast, &native_fns, &builtin_enums)
        })),
        None => None,
    };

    // Stage 4: typecheck. M5 inserted `ferric_traits::build_registry` between
    // resolve and infer; the LSP runs it inline so types involving traits
    // resolve correctly.
    let typecheck_result = match (&parse_result, &resolve_result) {
        (Some(ast), Some(res)) => {
            catch_stage(std::panic::AssertUnwindSafe(|| {
                let registry = ferric_traits::build_registry(ast, res, &interner);
                ferric_infer::typecheck(ast, res, &interner, &registry)
            }))
        }
        _ => None,
    };

    PipelineSnapshot {
        uri,
        version,
        source,
        interner,
        lex:       lex_result,
        parse:     parse_result,
        resolve:   resolve_result,
        typecheck: typecheck_result,
        line_index,
    }
}

/// `catch_unwind` wrapper that discards the panic payload. The diagnostics
/// handler (Task 04) detects the resulting `None` and emits a
/// "stage panicked" diagnostic at line 1.
///
/// Stage code holds `&mut Interner` across the boundary, which is not
/// `UnwindSafe` by default; callers wrap their closures in
/// `AssertUnwindSafe`. A panicking stage that has corrupted the interner is
/// already a bug — the LSP's contract is that the *server* survives, not
/// that the stage's state is salvageable.
fn catch_stage<T, F>(f: F) -> Option<T>
where
    F: FnOnce() -> T + std::panic::UnwindSafe,
{
    std::panic::catch_unwind(f).ok()
}

/// Built-in enum table mirroring `ferric_stdlib::builtin_enum_table`. The
/// LSP intentionally does not depend on `ferric_stdlib` (it has no native
/// runtime to register), so this is duplicated. Must be kept in sync.
fn stdlib_builtin_enum_table(
    interner: &mut Interner,
) -> Vec<(Symbol, Vec<(Symbol, Vec<TypeAnnotation>)>)> {
    let option_sym = interner.intern("Option");
    let some_sym = interner.intern("Some");
    let none_sym = interner.intern("None");
    let result_sym = interner.intern("Result");
    let ok_sym = interner.intern("Ok");
    let err_sym = interner.intern("Err");

    vec![
        (
            option_sym,
            vec![
                (some_sym, vec![TypeAnnotation::Infer]),
                (none_sym, vec![]),
            ],
        ),
        (
            result_sym,
            vec![
                (ok_sym, vec![TypeAnnotation::Infer]),
                (err_sym, vec![TypeAnnotation::Infer]),
            ],
        ),
    ]
}

/// Native function table that mirrors `src/main.rs::native_fn_table`. Must be
/// kept in sync with the CLI table — when a new native is added to
/// `ferric_stdlib::register_stdlib`, add it here too. (Long-term: move this
/// table into `ferric_stdlib` so there is one definition.)
fn stdlib_native_fn_table(interner: &mut Interner) -> Vec<(Symbol, Vec<Symbol>)> {
    let entries: &[(&str, &[&str])] = &[
        ("println",         &["s"]),
        ("print",           &["s"]),
        ("int_to_str",      &["n"]),
        ("float_to_str",    &["n"]),
        ("bool_to_str",     &["b"]),
        ("int_to_float",    &["n"]),
        ("shell_stdout",    &["output"]),
        ("shell_exit_code", &["output"]),
        ("array_len",       &["arr"]),
        ("str_len",         &["s"]),
        ("str_trim",        &["s"]),
        ("str_contains",    &["s", "sub"]),
        ("str_starts_with", &["s", "prefix"]),
        ("str_parse_int",   &["s"]),
        ("str_split",       &["s", "sep"]),
        ("abs",             &["n"]),
        ("min",             &["a", "b"]),
        ("max",             &["a", "b"]),
        ("sqrt",            &["n"]),
        ("pow",             &["base", "exp"]),
        ("floor",           &["n"]),
        ("ceil",            &["n"]),
        ("read_line",       &[]),
    ];
    entries
        .iter()
        .map(|(name, params)| {
            let n = interner.intern(name);
            let ps = params.iter().map(|p| interner.intern(p)).collect();
            (n, ps)
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Span <-> LSP position conversion
// ---------------------------------------------------------------------------

/// Pre-computed table of byte offsets where each line begins. Built once per
/// snapshot so span<->position lookups are O(log n) on the line count.
pub struct LineIndex {
    line_starts: Vec<u32>,
}

impl LineIndex {
    pub fn new(source: &str) -> Self {
        let mut line_starts = vec![0u32];
        for (i, b) in source.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push((i + 1) as u32);
            }
        }
        LineIndex { line_starts }
    }

    /// Convert a byte offset to an LSP `Position`. Note: LSP positions are in
    /// UTF-16 code units; this implementation uses UTF-8 byte offsets, which
    /// is correct for ASCII. Multi-byte text is handled correctly for line
    /// breaks but column numbers diverge from the LSP spec for non-ASCII
    /// content. A multi-byte-correct implementation is a future polish.
    pub fn position_of(&self, byte: u32) -> Position {
        let line = match self.line_starts.binary_search(&byte) {
            Ok(idx)  => idx,
            Err(idx) => idx.saturating_sub(1),
        };
        let col = byte.saturating_sub(self.line_starts[line]);
        Position { line: line as u32, character: col }
    }

    pub fn range_of(&self, span: Span) -> Range {
        Range {
            start: self.position_of(span.start),
            end:   self.position_of(span.end),
        }
    }

    pub fn byte_offset_of(&self, pos: Position) -> u32 {
        let line = pos.line as usize;
        let line_start = self
            .line_starts
            .get(line)
            .copied()
            .unwrap_or_else(|| *self.line_starts.last().unwrap_or(&0));
        line_start + pos.character
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_index_round_trip() {
        let src = "let x = 1\nlet y = 2\nlet z = 3";
        let li = LineIndex::new(src);
        assert_eq!(li.position_of(0),  Position { line: 0, character: 0 });
        assert_eq!(li.position_of(10), Position { line: 1, character: 0 });
        assert_eq!(li.position_of(14), Position { line: 1, character: 4 });
        assert_eq!(li.position_of(20), Position { line: 2, character: 0 });
        assert_eq!(li.byte_offset_of(Position { line: 2, character: 4 }), 24);
    }

    #[test]
    fn pipeline_runs_on_clean_source() {
        let snap = run_pipeline(
            "file:///tmp/test.fe".into(),
            1,
            "let x = 1".into(),
        );
        assert!(snap.lex.is_some());
        assert!(snap.parse.is_some());
        assert!(snap.resolve.is_some());
        assert!(snap.typecheck.is_some());
    }

    #[test]
    fn cache_advances_last_good_per_stage() {
        let cache = PipelineCache::new();
        let uri = "file:///tmp/test.fe";

        // First run: clean source — every stage produces a result.
        let _ = cache.run_and_store(uri, 1, "let x = 1");
        assert!(cache.last_good_resolve(uri).is_some());
        assert!(cache.last_good_type(uri).is_some());

        // Subsequent runs replace `current`. The last-good entries advance
        // for every stage that succeeds; here, every stage still runs, so
        // last-good still points at the most recent snapshot.
        let snap2 = cache.run_and_store(uri, 2, "let y = 2");
        assert_eq!(cache.current(uri).unwrap().version, 2);
        assert_eq!(cache.last_good_resolve(uri).unwrap().version, 2);
        let _ = snap2;
    }

    #[test]
    fn close_removes_document_state() {
        let cache = PipelineCache::new();
        let uri = "file:///tmp/test.fe";
        let _ = cache.run_and_store(uri, 1, "let x = 1");
        assert!(cache.current(uri).is_some());
        cache.remove(uri);
        assert!(cache.current(uri).is_none());
    }
}
