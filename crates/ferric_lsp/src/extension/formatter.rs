//! Formatter extension point. See `mod.rs`.
//!
//! The `is_noop` default lets `capabilities.rs` advertise the formatting
//! capability conditionally — VS Code shows "Format Document" only when a
//! real formatter is wired in.
//!
//! Crate-level `allow(dead_code)` for the same reason as `linter.rs`: the
//! trait surface is intentional scaffolding for the future format milestone.

#![allow(dead_code)]

use ferric_common::ParseResult;

pub trait Formatter: Send + Sync {
    /// Returns the fully-formatted source text, or `None` if formatting was
    /// skipped (e.g. the file has syntax errors that prevent safe formatting).
    fn format(&self, source: &str, ast: &ParseResult) -> Option<String>;

    fn is_noop(&self) -> bool { false }
}

pub struct NoopFormatter;

impl Formatter for NoopFormatter {
    fn format(&self, _source: &str, _ast: &ParseResult) -> Option<String> { None }
    fn is_noop(&self) -> bool { true }
}
