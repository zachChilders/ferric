# LSP — Task 2: ferric_lsp crate skeleton, pipeline, traits, capabilities

> **Prerequisite:** Task 1 complete. `ferric_common::keywords` and `Display for
> Ty` must exist.

---

## Goal

Create the `ferric_lsp` crate with everything needed for the language server
binary to start, accept LSP connections over stdio, run the pipeline on document
changes, and route requests to handler modules. This task does **not** implement
the handler bodies — those are tasks 04–06. Each handler module ships as a
stub that returns the LSP-defined "no result" value (empty `Vec`, `None`,
`null`).

This task does include:

- Cargo manifest + workspace registration
- `main.rs` (binary entry)
- `server.rs` (`LspServer`, `tower-lsp` `LanguageServer` impl, dispatch)
- `pipeline.rs` (`PipelineSnapshot`, `PipelineCache`, `run_pipeline`,
  `catch_stage`, span-to-position conversion)
- `capabilities.rs` (`server_capabilities`)
- `extension/linter.rs` + `extension/formatter.rs` (traits + Noop impls)
- Stub handler modules (empty bodies) for diagnostics, completion, hover,
  goto-def, document_symbols, inlay_hints

---

## Files

### Create — `crates/ferric_lsp/Cargo.toml`

```toml
[package]
name    = "ferric-lsp"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "ferric-lsp"
path = "src/main.rs"

[dependencies]
ferric_common    = { path = "../ferric_common" }
ferric_lexer     = { path = "../ferric_lexer" }
ferric_parser    = { path = "../ferric_parser" }
ferric_resolve   = { path = "../ferric_resolve" }
ferric_typecheck = { path = "../ferric_typecheck" }

tower-lsp  = "0.20"
tokio      = { version = "1", features = ["full"] }
serde      = { version = "1", features = ["derive"] }
serde_json = "1"
dashmap    = "5"

[build-dependencies]
ferric_common = { path = "../ferric_common" }
serde_json    = "1"
```

### Modify — workspace `Cargo.toml`

Add `crates/ferric_lsp` to `members`.

### Create — `crates/ferric_lsp/src/main.rs`

```rust
use tower_lsp::{LspService, Server};

mod capabilities;
mod extension;
mod handlers;
mod pipeline;
mod server;

use extension::{formatter::NoopFormatter, linter::NoopLinter};
use server::LspServer;

#[tokio::main]
async fn main() {
    let stdin  = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(|client| {
        LspServer::new(client, NoopLinter, NoopFormatter)
    });

    Server::new(stdin, stdout, socket).serve(service).await;
}
```

### Create — `crates/ferric_lsp/src/server.rs`

```rust
use std::sync::Arc;

use tower_lsp::jsonrpc::Result as RpcResult;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

use crate::capabilities::server_capabilities;
use crate::extension::{formatter::Formatter, linter::Linter};
use crate::handlers;
use crate::pipeline::{PipelineCache, PipelineSnapshot};

pub struct LspServer {
    cache:     Arc<PipelineCache>,
    linter:    Arc<dyn Linter>,
    formatter: Arc<dyn Formatter>,
    client:    Client,
}

impl LspServer {
    pub fn new(
        client:    Client,
        linter:    impl Linter + 'static,
        formatter: impl Formatter + 'static,
    ) -> Self {
        LspServer {
            cache:     Arc::new(PipelineCache::new()),
            linter:    Arc::new(linter),
            formatter: Arc::new(formatter),
            client,
        }
    }

    async fn run_and_publish(&self, uri: Url, version: i32, source: String) {
        let cache = Arc::clone(&self.cache);
        let snapshot = tokio::task::spawn_blocking(move || {
            cache.run_and_store(uri.as_str(), version, &source)
        })
        .await
        .expect("pipeline task panicked unexpectedly");

        let diags = handlers::diagnostics::publish(&snapshot);
        self.client.publish_diagnostics(snapshot.uri_url(), diags, Some(version)).await;
    }

    fn snapshot_for(&self, uri: &Url) -> Option<Arc<PipelineSnapshot>> {
        self.cache.current(uri.as_str())
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for LspServer {
    async fn initialize(&self, _params: InitializeParams) -> RpcResult<InitializeResult> {
        Ok(InitializeResult {
            capabilities: server_capabilities(),
            server_info:  Some(ServerInfo {
                name:    "ferric-lsp".into(),
                version: Some(env!("CARGO_PKG_VERSION").into()),
            }),
        })
    }

    async fn shutdown(&self) -> RpcResult<()> { Ok(()) }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let doc = params.text_document;
        self.run_and_publish(doc.uri, doc.version, doc.text).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        // text_document_sync is INCREMENTAL — but tower-lsp gives us the full
        // text in `content_changes[0].text` when we apply changes ourselves.
        // For the first cut we ask the client to send full text by setting
        // sync to FULL in capabilities.rs (see capability note there).
        let uri     = params.text_document.uri;
        let version = params.text_document.version;
        let Some(change) = params.content_changes.into_iter().next() else { return; };
        self.run_and_publish(uri, version, change.text).await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.cache.remove(params.text_document.uri.as_str());
    }

    async fn completion(&self, params: CompletionParams)
        -> RpcResult<Option<CompletionResponse>>
    {
        let uri = params.text_document_position.text_document.uri.clone();
        let Some(snap) = self.snapshot_for(&uri) else { return Ok(None); };
        let last_good = self.cache.last_good_resolve(uri.as_str());
        Ok(Some(handlers::completion::complete(
            &snap, last_good.as_deref(), params.text_document_position.position,
        )))
    }

    async fn hover(&self, params: HoverParams) -> RpcResult<Option<Hover>> {
        let uri = params.text_document_position_params.text_document.uri.clone();
        let Some(snap) = self.snapshot_for(&uri) else { return Ok(None); };
        Ok(handlers::hover::hover(&snap, params.text_document_position_params.position))
    }

    async fn goto_definition(&self, params: GotoDefinitionParams)
        -> RpcResult<Option<GotoDefinitionResponse>>
    {
        let uri = params.text_document_position_params.text_document.uri.clone();
        let Some(snap) = self.snapshot_for(&uri) else { return Ok(None); };
        Ok(handlers::goto_def::goto(&snap, &uri, params.text_document_position_params.position))
    }

    async fn document_symbol(&self, params: DocumentSymbolParams)
        -> RpcResult<Option<DocumentSymbolResponse>>
    {
        let Some(snap) = self.snapshot_for(&params.text_document.uri) else { return Ok(None); };
        Ok(Some(handlers::document_symbols::symbols(&snap)))
    }

    async fn inlay_hint(&self, params: InlayHintParams)
        -> RpcResult<Option<Vec<InlayHint>>>
    {
        let Some(snap) = self.snapshot_for(&params.text_document.uri) else { return Ok(None); };
        Ok(Some(handlers::inlay_hints::inlay_hints(&snap, params.range)))
    }
}
```

### Create — `crates/ferric_lsp/src/pipeline.rs`

```rust
use std::sync::Arc;

use dashmap::DashMap;
use ferric_common::{
    Interner, LexResult, ParseResult, ResolveResult, Span, Symbol, TypeResult,
};
use tower_lsp::lsp_types::{Position, Range, Url};

/// Result of one full pipeline run on one version of a document.
/// Immutable once created.
pub struct PipelineSnapshot {
    pub uri:       String,
    pub version:   i32,
    pub source:    String,
    pub interner:  Interner,

    /// `None` only if the stage panicked (caught by `catch_unwind`) or if a
    /// required preceding result was absent. Stage `errors` vectors carry
    /// non-fatal stage errors and do **not** prevent later stages from running.
    pub lex:       Option<LexResult>,
    pub parse:     Option<ParseResult>,
    pub resolve:   Option<ResolveResult>,
    pub typecheck: Option<TypeResult>,

    pub line_index: LineIndex,
}

impl PipelineSnapshot {
    pub fn uri_url(&self) -> Url { Url::parse(&self.uri).expect("valid uri") }
}

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
    pub fn new() -> Self { PipelineCache { documents: DashMap::new() } }

    pub fn current(&self, uri: &str) -> Option<Arc<PipelineSnapshot>> {
        self.documents.get(uri).map(|s| Arc::clone(&s.current))
    }

    pub fn last_good_resolve(&self, uri: &str) -> Option<Arc<PipelineSnapshot>> {
        self.documents.get(uri).and_then(|s| s.last_good_resolve.as_ref().map(Arc::clone))
    }

    pub fn last_good_type(&self, uri: &str) -> Option<Arc<PipelineSnapshot>> {
        self.documents.get(uri).and_then(|s| s.last_good_type.as_ref().map(Arc::clone))
    }

    pub fn remove(&self, uri: &str) { self.documents.remove(uri); }

    pub fn run_and_store(&self, uri: &str, version: i32, source: &str)
        -> Arc<PipelineSnapshot>
    {
        let snap = Arc::new(run_pipeline(uri.to_string(), version, source.to_string()));

        let mut entry = self.documents.entry(uri.to_string()).or_insert_with(|| {
            DocumentState {
                current:           Arc::clone(&snap),
                last_good_lex:     None,
                last_good_parse:   None,
                last_good_resolve: None,
                last_good_type:    None,
            }
        });

        entry.current = Arc::clone(&snap);
        if snap.lex.is_some()       { entry.last_good_lex     = Some(Arc::clone(&snap)); }
        if snap.parse.is_some()     { entry.last_good_parse   = Some(Arc::clone(&snap)); }
        if snap.resolve.is_some()   { entry.last_good_resolve = Some(Arc::clone(&snap)); }
        if snap.typecheck.is_some() { entry.last_good_type    = Some(Arc::clone(&snap)); }

        snap
    }
}

/// Run the full pipeline. Each stage call is wrapped in `catch_unwind`. A panic
/// in any stage results in `None` for that stage's slot and short-circuits
/// later stages (they cannot run without their input).
pub fn run_pipeline(uri: String, version: i32, source: String) -> PipelineSnapshot {
    let line_index = LineIndex::new(&source);
    let mut interner = Interner::default();

    let lex_result = catch_stage(|| {
        ferric_lexer::lex(&source, &mut interner)
    });

    let parse_result = match &lex_result {
        Some(lex) => catch_stage(|| ferric_parser::parse(lex)),
        None => None,
    };

    // Collect native symbols for resolve. Stdlib names live here.
    let native_symbols: Vec<Symbol> = stdlib_native_symbols(&mut interner);

    let resolve_result = match &parse_result {
        Some(ast) => catch_stage(|| {
            ferric_resolve::resolve_with_natives(ast, &native_symbols)
        }),
        None => None,
    };

    let typecheck_result = match (&parse_result, &resolve_result) {
        (Some(ast), Some(res)) => catch_stage(|| {
            ferric_typecheck::typecheck(ast, res, &interner)
        }),
        _ => None,
    };

    PipelineSnapshot {
        uri, version, source, interner,
        lex:       lex_result,
        parse:     parse_result,
        resolve:   resolve_result,
        typecheck: typecheck_result,
        line_index,
    }
}

fn catch_stage<T, F>(f: F) -> Option<T>
where
    F: FnOnce() -> T + std::panic::UnwindSafe,
{
    std::panic::catch_unwind(f).ok()
}

fn stdlib_native_symbols(interner: &mut Interner) -> Vec<Symbol> {
    [
        "println", "print", "int_to_str", "float_to_str", "bool_to_str",
        "int_to_float",
    ]
    .into_iter()
    .map(|s| interner.intern(s))
    .collect()
}

// ---------------------------------------------------------------------------
// Span ↔ LSP position conversion
// ---------------------------------------------------------------------------

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

    pub fn position_of(&self, byte: u32) -> Position {
        let line = match self.line_starts.binary_search(&byte) {
            Ok(idx)  => idx,
            Err(idx) => idx - 1,
        };
        let col = byte - self.line_starts[line];
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
        let line_start = self.line_starts.get(line).copied().unwrap_or(
            *self.line_starts.last().unwrap_or(&0),
        );
        line_start + pos.character
    }
}
```

### Create — `crates/ferric_lsp/src/capabilities.rs`

```rust
use tower_lsp::lsp_types::*;

pub fn server_capabilities() -> ServerCapabilities {
    ServerCapabilities {
        // FULL sync simplifies the first cut. Switch to INCREMENTAL once the
        // server applies edits itself.
        text_document_sync: Some(TextDocumentSyncCapability::Kind(
            TextDocumentSyncKind::FULL,
        )),
        completion_provider: Some(CompletionOptions {
            trigger_characters: Some(vec![".".into(), ":".into()]),
            ..Default::default()
        }),
        hover_provider:           Some(HoverProviderCapability::Simple(true)),
        definition_provider:      Some(OneOf::Left(true)),
        document_symbol_provider: Some(OneOf::Left(true)),
        inlay_hint_provider:      Some(OneOf::Left(true)),
        ..Default::default()
    }
}
```

### Create — `crates/ferric_lsp/src/extension/mod.rs`

```rust
pub mod formatter;
pub mod linter;
```

### Create — `crates/ferric_lsp/src/extension/linter.rs`

```rust
use ferric_common::{ParseResult, ResolveResult, Span, TypeResult};

pub struct LintDiagnostic {
    pub span:     Span,
    pub message:  String,
    pub severity: LintSeverity,
    pub code:     Option<String>,
}

pub enum LintSeverity { Warning, Error, Info, Hint }

pub trait Linter: Send + Sync {
    /// Called after a successful pipeline run with all available stage outputs.
    fn lint(
        &self,
        ast:     &ParseResult,
        resolve: &ResolveResult,
        types:   &TypeResult,
    ) -> Vec<LintDiagnostic>;
}

pub struct NoopLinter;
impl Linter for NoopLinter {
    fn lint(&self, _: &ParseResult, _: &ResolveResult, _: &TypeResult) -> Vec<LintDiagnostic> {
        vec![]
    }
}
```

### Create — `crates/ferric_lsp/src/extension/formatter.rs`

```rust
use ferric_common::ParseResult;

pub trait Formatter: Send + Sync {
    /// Returns the fully formatted source, or `None` if formatting was skipped
    /// (e.g. file has syntax errors).
    fn format(&self, source: &str, ast: &ParseResult) -> Option<String>;

    fn is_noop(&self) -> bool { false }
}

pub struct NoopFormatter;
impl Formatter for NoopFormatter {
    fn format(&self, _: &str, _: &ParseResult) -> Option<String> { None }
    fn is_noop(&self) -> bool { true }
}
```

### Create — `crates/ferric_lsp/src/handlers/mod.rs`

```rust
pub mod completion;
pub mod diagnostics;
pub mod document_symbols;
pub mod goto_def;
pub mod hover;
pub mod inlay_hints;
```

### Create — stub handler files

Each is a stub that returns the LSP-defined empty value. Bodies are filled in
by tasks 04, 05, 06.

`crates/ferric_lsp/src/handlers/diagnostics.rs`:

```rust
use tower_lsp::lsp_types::Diagnostic;
use crate::pipeline::PipelineSnapshot;

pub fn publish(_snapshot: &PipelineSnapshot) -> Vec<Diagnostic> { vec![] }
```

`crates/ferric_lsp/src/handlers/completion.rs`:

```rust
use tower_lsp::lsp_types::{CompletionResponse, Position};
use crate::pipeline::PipelineSnapshot;

pub fn complete(
    _snapshot:  &PipelineSnapshot,
    _last_good: Option<&PipelineSnapshot>,
    _pos:       Position,
) -> CompletionResponse {
    CompletionResponse::Array(vec![])
}
```

`crates/ferric_lsp/src/handlers/hover.rs`:

```rust
use tower_lsp::lsp_types::{Hover, Position};
use crate::pipeline::PipelineSnapshot;

pub fn hover(_snapshot: &PipelineSnapshot, _pos: Position) -> Option<Hover> { None }
```

`crates/ferric_lsp/src/handlers/goto_def.rs`:

```rust
use tower_lsp::lsp_types::{GotoDefinitionResponse, Position, Url};
use crate::pipeline::PipelineSnapshot;

pub fn goto(
    _snapshot: &PipelineSnapshot,
    _uri:      &Url,
    _pos:      Position,
) -> Option<GotoDefinitionResponse> { None }
```

`crates/ferric_lsp/src/handlers/document_symbols.rs`:

```rust
use tower_lsp::lsp_types::DocumentSymbolResponse;
use crate::pipeline::PipelineSnapshot;

pub fn symbols(_snapshot: &PipelineSnapshot) -> DocumentSymbolResponse {
    DocumentSymbolResponse::Nested(vec![])
}
```

`crates/ferric_lsp/src/handlers/inlay_hints.rs`:

```rust
use tower_lsp::lsp_types::{InlayHint, Range};
use crate::pipeline::PipelineSnapshot;

pub fn inlay_hints(_snapshot: &PipelineSnapshot, _range: Range) -> Vec<InlayHint> {
    vec![]
}
```

### Create — `crates/ferric_lsp/build.rs` (placeholder)

The real implementation is task 03. For task 02, ship a no-op so the crate
builds:

```rust
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
}
```

Task 03 will replace this entire file.

---

## Done when

- [ ] `cargo build -p ferric-lsp` succeeds with no warnings
- [ ] `cargo run -p ferric-lsp` starts the binary and waits for LSP traffic on stdio
- [ ] Sending an `initialize` request returns `server_capabilities()`
- [ ] Opening a document triggers exactly one pipeline run
- [ ] Changing a document version triggers exactly one new pipeline run
- [ ] Closing a document removes its `DocumentState` from the cache
- [ ] Stage panics are caught and result in `None` for that stage — they do
      not crash the server
- [ ] Pipeline runs on `tokio::task::spawn_blocking` — async event loop is
      never blocked by stage code
- [ ] Last-good snapshot is tracked per stage and survives a failing run
- [ ] `LspServer::new(client, NoopLinter, NoopFormatter)` is the only way the
      server is constructed
- [ ] Imports from stage crates are limited to: `ferric_lexer::lex`,
      `ferric_parser::parse`, `ferric_resolve::resolve_with_natives`,
      `ferric_typecheck::typecheck`. No internal types are imported
- [ ] All handler modules compile with stub bodies; full implementations land
      in tasks 04–06
