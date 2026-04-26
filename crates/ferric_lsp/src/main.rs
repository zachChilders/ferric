//! Ferric Language Server binary.
//!
//! `ferric_lsp` runs as a subprocess of an LSP client (VS Code extension,
//! Neovim, etc.) and communicates over stdio using JSON-RPC. It calls only
//! the public entry points of each pipeline stage — no internal types from
//! any stage crate are imported (LSP Rule 1).

use tower_lsp::{LspService, Server};

mod ast_lookup;
mod capabilities;
mod extension;
mod handlers;
mod pipeline;
mod server;
mod stdlib_names;

use extension::formatter::NoopFormatter;
use extension::linter::NoopLinter;
use server::LspServer;

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    // The single configuration point for the server. Future lint and format
    // milestones add a new crate that implements `Linter`/`Formatter` and
    // swap the noops here — no other LSP code changes (LSP Rule 4).
    let (service, socket) = LspService::new(|client| {
        LspServer::new(client, NoopLinter, NoopFormatter)
    });

    Server::new(stdin, stdout, socket).serve(service).await;
}
