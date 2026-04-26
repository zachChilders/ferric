//! Module-system stage outputs and errors.
//!
//! `ferric_manifest` produces a `ManifestResult` (loading `Ferric.toml`).
//! `ferric_module` produces a `ModuleResult` (resolving the import graph).
//! Both are read by downstream stages through `ferric_common` only — neither
//! crate's internals are visible across the pipeline.

use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use crate::{DefId, ImportPath, Span, Symbol};

/// Result of the `ferric_manifest` stage.
///
/// `manifest` is `None` in script mode (no `Ferric.toml`); the absence of a
/// manifest is not itself an error — it just restricts what import shapes are
/// legal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManifestResult {
    pub manifest: Option<Manifest>,
    pub errors:   Vec<ManifestError>,
}

impl ManifestResult {
    /// Creates a new ManifestResult.
    pub fn new(manifest: Option<Manifest>, errors: Vec<ManifestError>) -> Self {
        Self { manifest, errors }
    }

    /// Returns true if there were any errors during manifest loading.
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }
}

/// Parsed `Ferric.toml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub name:         String,
    pub version:      String,
    pub submodules:   Vec<String>,
    pub dependencies: HashMap<String, String>,
}

/// Errors raised by `ferric_manifest`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ManifestError {
    /// Failed to parse `Ferric.toml` as TOML.
    ParseError {
        message: String,
        span:    Span,
    },
    /// A submodule path contains its own `Ferric.toml`, which would shadow
    /// the workspace manifest.
    ConflictingManifest {
        path: String,
        span: Span,
    },
}

impl ManifestError {
    /// Returns the span associated with this error.
    pub fn span(&self) -> Span {
        match self {
            ManifestError::ParseError { span, .. } => *span,
            ManifestError::ConflictingManifest { span, .. } => *span,
        }
    }

    /// Returns a human-readable description of this error.
    pub fn description(&self) -> String {
        match self {
            ManifestError::ParseError { message, .. } => {
                format!("failed to parse Ferric.toml: {}", message)
            }
            ManifestError::ConflictingManifest { path, .. } => {
                format!("submodule `{}` has its own Ferric.toml", path)
            }
        }
    }
}

/// Result of the `ferric_module` stage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModuleResult {
    /// Names exported by the current compilation unit, mapped to their `DefId`.
    pub exports: HashMap<Symbol, DefId>,
    /// Resolved imports — each entry maps local names introduced by an
    /// `import` declaration to the source `DefId` they refer to.
    pub imports: Vec<ResolvedImport>,
    /// Imports referring to a name that *exists* in the target file but is not
    /// marked `export`. The resolver's wire-in pass turns these into
    /// `ResolveError::PrivateImport` so users get the more precise diagnostic
    /// (vs. the generic "not exported" `UnknownExport`).
    pub private_imports: Vec<PrivateImportInfo>,
    /// Any errors encountered during module resolution.
    pub errors:  Vec<ModuleError>,
}

/// One named import that resolved to a private (non-exported) item in the
/// target file. Carried out of the module stage so the resolver can emit the
/// more specific `ResolveError::PrivateImport`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrivateImportInfo {
    pub name: Symbol,
    pub path: String,
    pub span: Span,
}

impl ModuleResult {
    /// Creates a new ModuleResult.
    pub fn new(
        exports: HashMap<Symbol, DefId>,
        imports: Vec<ResolvedImport>,
        errors:  Vec<ModuleError>,
    ) -> Self {
        Self {
            exports,
            imports,
            private_imports: Vec::new(),
            errors,
        }
    }

    /// Builder-style setter for `private_imports`.
    pub fn with_private_imports(mut self, ps: Vec<PrivateImportInfo>) -> Self {
        self.private_imports = ps;
        self
    }

    /// Returns true if there were any errors during module resolution.
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }
}

/// One resolved `import` declaration. Each binding maps a local name (after
/// applying any `as` alias) to the source file's `DefId` for that item.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResolvedImport {
    pub span:     Span,
    pub path:     ImportPath,
    pub bindings: Vec<(Symbol, DefId)>,
}

/// Errors raised by `ferric_module`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ModuleError {
    /// A cycle exists in the import graph. `cycle` lists every file in the
    /// cycle in order; `span` points at the import that closes it.
    CircularImport {
        cycle: Vec<String>,
        span:  Span,
    },
    /// A named import references an item that does not appear in the target
    /// file's exports.
    UnknownExport {
        name: Symbol,
        path: String,
        span: Span,
    },
    /// A `@/` or cache-name import was used without a `Ferric.toml`.
    NoManifest {
        path: String,
        span: Span,
    },
    /// A cache import named a package that is declared in the manifest but
    /// not present in `.ferric/cache/`.
    CacheMiss {
        name: String,
        span: Span,
    },
    /// `import X from "..."` (no braces, no `*`) — Ferric does not support
    /// default imports.
    DefaultImport {
        span: Span,
    },
}

impl ModuleError {
    /// Returns the span associated with this error.
    pub fn span(&self) -> Span {
        match self {
            ModuleError::CircularImport { span, .. } => *span,
            ModuleError::UnknownExport { span, .. } => *span,
            ModuleError::NoManifest { span, .. } => *span,
            ModuleError::CacheMiss { span, .. } => *span,
            ModuleError::DefaultImport { span } => *span,
        }
    }

    /// Returns a human-readable description of this error.
    pub fn description(&self) -> String {
        match self {
            ModuleError::CircularImport { cycle, .. } => {
                format!("circular import: {}", cycle.join(" → "))
            }
            ModuleError::UnknownExport { path, .. } => {
                format!("imported name is not exported from \"{}\"", path)
            }
            ModuleError::NoManifest { path, .. } => {
                format!("import \"{}\" requires a Ferric.toml manifest", path)
            }
            ModuleError::CacheMiss { name, .. } => {
                format!("cache package `{}` not found in .ferric/cache/", name)
            }
            ModuleError::DefaultImport { .. } => {
                "default imports are not supported in Ferric".to_string()
            }
        }
    }
}
