# ferric_lsp

Language server for Ferric. Runs as a subprocess and communicates over stdin/stdout using the Language Server Protocol (JSON-RPC).

## Usage

The LSP binary is `ferric-lsp`. IDE extensions (VS Code, Neovim, etc.) launch it automatically. There is no library API — all interaction is through the LSP protocol.

## Features

| LSP Capability | Ferric implementation |
|---|---|
| **Diagnostics** | All errors from lex → parse → resolve → infer surfaced as VS Code squiggles |
| **Hover** | Type of the expression under the cursor; function signature on call sites |
| **Completion** | Local names, imported names, keywords, stdlib functions |
| **Go to definition** | Jump to the `let`, `fn`, or `struct` that defines a symbol |
| **Inlay hints** | Inline type annotations on `let` bindings |
| **Document symbols** | Outline view: all top-level functions and types |

## Implementation

- The server runs the full pipeline (lex → parse → resolve → infer) on every document edit. Results are cached per document in a `DashMap` for concurrent access.
- `ferric_stdlib_meta` is used instead of `ferric_stdlib` so the server starts fast with no heavy runtime dependencies.
- A TextMate grammar for syntax highlighting is generated at build time (`build.rs`) from the keyword list in `ferric_common::keywords`.
- The `extension/` directory contains `NoopFormatter` and `NoopLinter` stubs for future plugin integration.

## Architecture note

The LSP obeys the same stage-boundary rules as the rest of the pipeline: it imports only public entry-point functions from each stage crate, never internal types. This ensures LSP behaviour tracks the language definition automatically as stages evolve.
