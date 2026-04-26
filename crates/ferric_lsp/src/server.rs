//! `LspServer` — the `tower-lsp` `LanguageServer` impl. Owns the pipeline
//! cache and the injected `Linter`/`Formatter`. Dispatches LSP requests to
//! the per-method handlers in `crate::handlers`.

use std::sync::Arc;

use tower_lsp::jsonrpc::Result as RpcResult;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

use crate::capabilities::server_capabilities;
use crate::extension::formatter::Formatter;
use crate::extension::linter::Linter;
use crate::handlers;
use crate::pipeline::{PipelineCache, PipelineSnapshot};

pub struct LspServer {
    cache:     Arc<PipelineCache>,
    #[allow(dead_code)] // Linter is read by future lint integration (Task 04+).
    linter:    Arc<dyn Linter>,
    #[allow(dead_code)] // Same for formatter (M-future formatting milestone).
    formatter: Arc<dyn Formatter>,
    client:    Client,
}

impl LspServer {
    /// The single configuration point for the server. Future lint and format
    /// milestones replace one or both noops here without touching anything
    /// else (LSP Rule 4).
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

    /// Run the pipeline on a blocking thread (the stages are synchronous —
    /// pinning them to the async event loop would block all other LSP
    /// traffic during long type-checks). After the run completes, push
    /// fresh diagnostics for the new version.
    async fn run_and_publish(&self, uri: Url, version: i32, source: String) {
        let cache  = Arc::clone(&self.cache);
        let uri_s  = uri.to_string();
        let snapshot = tokio::task::spawn_blocking(move || {
            cache.run_and_store(&uri_s, version, &source)
        })
        .await
        .expect("pipeline join task itself panicked");

        let diags = handlers::diagnostics::publish(&snapshot);
        self.client.publish_diagnostics(uri, diags, Some(version)).await;
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
        // capabilities advertise FULL sync, so the client sends the entire
        // text in a single content-change entry. Take it.
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
        let uri = &params.text_document_position.text_document.uri;
        let Some(snap) = self.snapshot_for(uri) else { return Ok(None); };
        let last_good = self.cache.last_good_resolve(uri.as_str());
        Ok(Some(handlers::completion::complete(
            &snap,
            last_good.as_deref(),
            params.text_document_position.position,
        )))
    }

    async fn hover(&self, params: HoverParams) -> RpcResult<Option<Hover>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let Some(snap) = self.snapshot_for(uri) else { return Ok(None); };
        Ok(handlers::hover::hover(
            &snap,
            params.text_document_position_params.position,
        ))
    }

    async fn goto_definition(&self, params: GotoDefinitionParams)
        -> RpcResult<Option<GotoDefinitionResponse>>
    {
        let uri = params.text_document_position_params.text_document.uri.clone();
        let Some(snap) = self.snapshot_for(&uri) else { return Ok(None); };
        Ok(handlers::goto_def::goto(
            &snap,
            &uri,
            params.text_document_position_params.position,
        ))
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
