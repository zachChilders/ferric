# ferric_module

Cross-file import/export resolution. Stage 4 of the pipeline (between the first and second resolver passes).

## Entry point

```rust
pub fn resolve_modules(
    entry_path: &Path,
    workspace_root: &Path,
    ast: &ParseResult,
    resolve: &ResolveResult,
    manifest: &ManifestResult,
    interner: &mut Interner,
) -> ModuleResult
```

## What it does

1. **Walks the import graph** starting from the entry file. Each `import` statement triggers a lex + parse of the target file (if not already cached) and a recursive walk of its own imports.
2. **Detects cycles** — circular imports produce a `ModuleError::Cycle`.
3. **Validates import paths** — relative paths (`./lib`), workspace paths (`@/lib`), and cache paths (`ferric-http`) are resolved against the filesystem and manifest respectively.
4. **Checks export visibility** — importing a symbol that exists in the target file but is not `export`ed produces a `PrivateImport` error surfaced by the second resolver pass.
5. **Allocates synthetic `DefId`s** — imported names get `DefId`s counted down from `u32::MAX` so they never collide with locally allocated IDs.

## ModuleResult

```rust
pub struct ModuleResult {
    pub imports: Vec<ResolvedImport>,      // one per import statement
    pub exports: Vec<String>,              // names exported by the entry file
    pub errors: Vec<ModuleError>,
    pub private_imports: Vec<PrivateImportInfo>,
}
```

`ResolvedImport::bindings: HashMap<Symbol, DefId>` is what `ferric_resolve::resolve_with_imports` merges into the global scope before the main resolution pass runs.
