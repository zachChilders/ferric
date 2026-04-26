//! LSP capability declarations sent during `initialize`.

use tower_lsp::lsp_types::*;

pub fn server_capabilities() -> ServerCapabilities {
    ServerCapabilities {
        // FULL sync simplifies the first cut: clients send the entire document
        // text on every change. Switch to INCREMENTAL once the server applies
        // edits itself.
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
