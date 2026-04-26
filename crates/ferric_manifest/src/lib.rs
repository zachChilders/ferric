//! # Ferric Manifest Loader
//!
//! Reads `Ferric.toml` from a workspace root and produces a `ManifestResult`.
//! Workspace mode is opt-in: if no `Ferric.toml` is present, the loader returns
//! `manifest: None` and zero errors — script mode is just "no manifest", not a
//! failure.
//!
//! Public API: only `load_manifest` is exposed.

use std::collections::HashMap;
use std::path::Path;

use ferric_common::{Manifest, ManifestError, ManifestResult, Span};

/// Reads `Ferric.toml` from `workspace_root`.
///
/// Behaviour:
/// - No file present → `ManifestResult { manifest: None, errors: [] }` (script mode).
/// - File present and parseable → `ManifestResult { manifest: Some(_), errors }`.
/// - File present but malformed → `ManifestResult { manifest: None, errors: [ParseError] }`.
///
/// `Span` fields on errors are zero-width because the manifest sits outside any
/// `.fe` source file — the renderer downgrades these to header-only diagnostics.
pub fn load_manifest(workspace_root: &Path) -> ManifestResult {
    let toml_path = workspace_root.join("Ferric.toml");
    if !toml_path.exists() {
        return ManifestResult::new(None, Vec::new());
    }

    let contents = match std::fs::read_to_string(&toml_path) {
        Ok(s) => s,
        Err(e) => {
            return ManifestResult::new(
                None,
                vec![ManifestError::ParseError {
                    message: format!("could not read Ferric.toml: {}", e),
                    span: Span::new(0, 0),
                }],
            );
        }
    };

    let value: toml::Value = match contents.parse() {
        Ok(v) => v,
        Err(e) => {
            return ManifestResult::new(
                None,
                vec![ManifestError::ParseError {
                    message: e.message().to_string(),
                    span: Span::new(0, 0),
                }],
            );
        }
    };

    let mut errors: Vec<ManifestError> = Vec::new();

    let module = value.get("module").and_then(|v| v.as_table());
    let name = module
        .and_then(|t| t.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let version = module
        .and_then(|t| t.get("version"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let submodules: Vec<String> = value
        .get("submodules")
        .and_then(|v| v.as_table())
        .and_then(|t| t.get("include"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();

    let dependencies: HashMap<String, String> = value
        .get("dependencies")
        .and_then(|v| v.as_table())
        .map(|t| {
            t.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default();

    // Validate that no submodule contains its own Ferric.toml — that would
    // shadow the workspace manifest and produce confusing scope.
    for sub in &submodules {
        let nested = workspace_root.join(sub).join("Ferric.toml");
        if nested.exists() {
            errors.push(ManifestError::ConflictingManifest {
                path: sub.clone(),
                span: Span::new(0, 0),
            });
        }
    }

    let manifest = Manifest {
        name,
        version,
        submodules,
        dependencies,
    };
    ManifestResult::new(Some(manifest), errors)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn script_mode_when_no_manifest() {
        let dir = tempdir();
        let result = load_manifest(dir.path());
        assert!(result.manifest.is_none());
        assert!(result.errors.is_empty());
    }

    #[test]
    fn parses_minimal_manifest() {
        let dir = tempdir();
        fs::write(
            dir.path().join("Ferric.toml"),
            r#"
[module]
name    = "demo"
version = "0.1.0"
"#,
        )
        .unwrap();
        let result = load_manifest(dir.path());
        assert!(result.errors.is_empty());
        let m = result.manifest.expect("manifest present");
        assert_eq!(m.name, "demo");
        assert_eq!(m.version, "0.1.0");
        assert!(m.submodules.is_empty());
        assert!(m.dependencies.is_empty());
    }

    #[test]
    fn parses_dependencies_and_submodules() {
        let dir = tempdir();
        fs::write(
            dir.path().join("Ferric.toml"),
            r#"
[module]
name    = "app"
version = "0.2.0"

[submodules]
include = ["src/db", "src/http"]

[dependencies]
ferric-http = "1.2.0"
ferric-json = "0.8.3"
"#,
        )
        .unwrap();
        let result = load_manifest(dir.path());
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        let m = result.manifest.expect("manifest present");
        assert_eq!(m.submodules, vec!["src/db".to_string(), "src/http".to_string()]);
        assert_eq!(m.dependencies.get("ferric-http").map(String::as_str), Some("1.2.0"));
        assert_eq!(m.dependencies.get("ferric-json").map(String::as_str), Some("0.8.3"));
    }

    #[test]
    fn parse_error_for_invalid_toml() {
        let dir = tempdir();
        fs::write(dir.path().join("Ferric.toml"), "this is = = invalid").unwrap();
        let result = load_manifest(dir.path());
        assert!(result.manifest.is_none());
        assert!(result.errors.iter().any(|e| matches!(e, ManifestError::ParseError { .. })));
    }

    #[test]
    fn conflicting_submodule_manifest_is_an_error() {
        let dir = tempdir();
        fs::write(
            dir.path().join("Ferric.toml"),
            r#"
[module]
name    = "outer"
version = "0.1.0"

[submodules]
include = ["sub"]
"#,
        )
        .unwrap();
        fs::create_dir_all(dir.path().join("sub")).unwrap();
        fs::write(
            dir.path().join("sub").join("Ferric.toml"),
            r#"[module]
name = "inner"
version = "0.1.0"
"#,
        )
        .unwrap();
        let result = load_manifest(dir.path());
        assert!(result.errors.iter().any(|e| matches!(
            e,
            ManifestError::ConflictingManifest { path, .. } if path == "sub"
        )));
    }

    /// Tiny private tempdir helper — we don't pull in `tempfile` just for two
    /// tests. The directory is removed when the returned guard drops.
    /// Nanos alone collide under parallel tests on coarse-resolution clocks;
    /// pair with an atomic counter so each call always gets a unique directory.
    fn tempdir() -> TempDir {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let mut p = std::env::temp_dir();
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        p.push(format!("ferric-manifest-test-{}-{}-{}", std::process::id(), nonce, n));
        std::fs::create_dir_all(&p).unwrap();
        TempDir { path: p }
    }

    struct TempDir {
        path: std::path::PathBuf,
    }
    impl TempDir {
        fn path(&self) -> &Path {
            &self.path
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}
