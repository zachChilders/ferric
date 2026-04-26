//! Linter extension point. See `mod.rs`.
//!
//! Crate-level `allow(dead_code)` because the trait surface is intentionally
//! unused in this milestone — `NoopLinter` is the only implementor and the
//! diagnostics handler does not yet invoke `lint`. The future lint milestone
//! removes this allow.

#![allow(dead_code)]

use ferric_common::{ParseResult, ResolveResult, Span, TypeResult};

#[derive(Debug, Clone)]
pub struct LintDiagnostic {
    pub span:     Span,
    pub message:  String,
    pub severity: LintSeverity,
    /// Optional rule code — e.g. "F0042". Lets clients link to docs.
    pub code:     Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LintSeverity { Warning, Error, Info, Hint }

/// Implementations are called after every fully-successful pipeline run with
/// every stage's output available.
pub trait Linter: Send + Sync {
    fn lint(
        &self,
        ast:     &ParseResult,
        resolve: &ResolveResult,
        types:   &TypeResult,
    ) -> Vec<LintDiagnostic>;
}

pub struct NoopLinter;

impl Linter for NoopLinter {
    fn lint(
        &self,
        _ast:     &ParseResult,
        _resolve: &ResolveResult,
        _types:   &TypeResult,
    ) -> Vec<LintDiagnostic> {
        vec![]
    }
}
