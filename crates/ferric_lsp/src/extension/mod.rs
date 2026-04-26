//! Injectable extension points the LSP exposes for future milestones.
//!
//! The `Linter` and `Formatter` traits live here. The current implementations
//! are `NoopLinter`/`NoopFormatter`. When the lint/format milestones land,
//! they add a new crate that implements the trait — `LspServer::new` is the
//! single configuration point and only one line in `main.rs` changes.

pub mod formatter;
pub mod linter;
