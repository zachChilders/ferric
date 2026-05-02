# ferric_manifest

Reads an optional `Ferric.toml` from the workspace root. Stage 3 of the pipeline.

## Entry point

```rust
pub fn load_manifest(workspace_root: &Path) -> ManifestResult
```

If no `Ferric.toml` exists, `ManifestResult::manifest` is `None` and there are no errors. The pipeline continues in *script mode* — a single-file program with no named modules or declared dependencies.

If `Ferric.toml` is present it is parsed and validated. A `Manifest` includes:

```toml
[package]
name = "my-project"
version = "0.1.0"

[modules]
submodules = ["lib", "utils"]

[dependencies]
ferric-http = "1.0"
```

## ManifestResult

```rust
pub struct ManifestResult {
    pub manifest: Option<Manifest>,
    pub errors: Vec<ManifestError>,
}
```

`ferric_module` reads the manifest to validate `import` paths against the declared submodule list and to resolve workspace-relative (`@/...`) and cache (`ferric-foo`) imports.
