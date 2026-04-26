# M7 — Task 3: `ferric_manifest` + `ferric_module`

> **Prerequisite:** Task 1 must be complete before starting this task. Task 2
> (lexer/parser) does not need to be complete — this task operates on the resolved
> import graph, not on parse syntax. However, both tasks must be complete before
> Task 4 begins.

---

## What this task does

Adds two new pipeline stages:

- **`ferric_manifest`** — reads `Ferric.toml` from the workspace root, determines
  script mode vs workspace mode, and produces a `ManifestResult`.
- **`ferric_module`** — walks the import graph, resolves all import paths to source
  files and `DefId`s, validates exports, detects circular imports, and produces a
  `ModuleResult`.

After this task, the interpreter correctly refuses unknown imports, private imports,
circular imports, and cache imports without a manifest — all with span-annotated errors.

---

## New crate: `ferric_manifest`

### Public entry point

```rust
// ferric_manifest/src/lib.rs
pub fn load_manifest(workspace_root: &Path) -> ManifestResult;
```

### Behaviour

1. Look for `Ferric.toml` in `workspace_root`. If absent, return
   `ManifestResult { manifest: None, errors: vec![] }` — this is script mode,
   not an error.

2. If present, parse the TOML. Use the `toml` crate. On parse failure, emit
   `ManifestError::ParseError` and return early.

3. Validate that no file listed under `[submodules]` contains its own `Ferric.toml`.
   If one does, emit `ManifestError::ConflictingManifest { path, span }`.

4. Return `ManifestResult { manifest: Some(parsed), errors }`.

### TOML schema

```toml
[module]
name    = "my-app"      # required
version = "0.1.0"       # required — semver string

[submodules]
include = ["src/db", "src/http"]   # optional — paths relative to workspace root

[dependencies]
ferric-http = "1.2.0"   # optional — name = version constraint
ferric-json = "0.8.3"
```

Unknown keys at any level are ignored with a warning (not an error) — forward
compatibility.

### Cargo.toml dependency

```toml
# ferric_manifest/Cargo.toml
[dependencies]
ferric_common = { path = "../ferric_common" }
toml          = "0.8"
```

---

## New crate: `ferric_module`

### Public entry point

```rust
// ferric_module/src/lib.rs
pub fn resolve_modules(
    ast:      &ParseResult,
    resolve:  &ResolveResult,
    manifest: &ManifestResult,
) -> ModuleResult;
```

### Behaviour

#### Step 1 — collect all import declarations

Walk `ast.items` across all files in the compilation unit. Gather every
`Item::Import(ImportDecl)` into a pending work list.

#### Step 2 — validate path shapes against manifest

For each `ImportDecl`:

- `ImportPath::Relative` — always valid. Resolve to an absolute filesystem path
  relative to the importing file's location.
- `ImportPath::Workspace` — requires `manifest.manifest.is_some()`. If not,
  emit `ModuleError::NoManifest { path, span }`.
  If manifest present, resolve `@/` against `workspace_root`.
- `ImportPath::Cache` — requires `manifest.manifest.is_some()` AND the name must
  appear in `manifest.dependencies`. If not in manifest, emit
  `ModuleError::NoManifest`. If in manifest but not in `.ferric/cache/`, emit
  `ModuleError::CacheMiss { name, span }`.
  If cache hit, resolve against `.ferric/cache/<name>-<version>/`.

#### Step 3 — cycle detection

Build a directed graph: node per file, edge per import. Run a DFS. If a back edge
is found, collect the cycle as an ordered `Vec<String>` of file paths and emit
`ModuleError::CircularImport { cycle, span }` where `span` is the span of the
import declaration that closes the cycle.

Cycle error message format:

```
error: circular import
  --> src/a.fe:1:1
   |
 1 | import { foo } from "./b"
   |         ^^^^^^^^^^^^^^^^^ this import creates a cycle: a → b → a
```

List the full cycle path in the message even if it's longer than two files.

#### Step 4 — validate exports

For each named import `{ X }` from a resolved file, check that `X` is marked
`export` in that file's parsed items. If not, emit
`ModuleError::UnknownExport { name, path, span }`.

For namespace imports (`* as ns`), collect all exported names from the target file.
No error if the target file exports nothing — an empty namespace is valid.

#### Step 5 — build bindings

For each resolved import, produce `ResolvedImport { span, path, bindings }` where
`bindings` maps each local alias to the source file's `DefId` for that item.

For aliased imports (`import { connect as dbConnect }`), the binding key is the
alias (`dbConnect`), not the original name.

#### Step 6 — collect exports for this module

Walk all `Item::Export(ExportDecl)` in the current compilation unit. Build
`ModuleResult::exports: HashMap<Symbol, DefId>`.

---

## Changes to `ferric_resolve`

Two additions to the existing resolver — no structural changes:

**1. Imported name lookup**

When resolving an identifier, check `ModuleResult::imports` for a matching binding
before the local scope stack. Imported names are in scope at file level and do not
shadow local bindings declared after them (imports are hoisted).

**2. Private import error**

If a named import references an item that exists in the target file but is not
exported, emit `ResolveError::PrivateImport { name, path, span }`.

Note: `ModuleResult` must be produced before `ResolveResult` is finalised. In
`main.rs`, `resolve_modules` is called after the initial `resolve` pass. The
resolver is then given a second, lightweight pass to wire in the import bindings.
The exact mechanism (a second entry point on `ferric_resolve`, or passing
`ModuleResult` into a single combined resolve call) is left to the implementor —
the public stage signatures must still be preserved.

---

## `main.rs` changes

```rust
// New call order — additions marked with //+
let manifest      = load_manifest(&workspace_root);              //+
let lex_result    = lex(&source, &mut interner);
let parse_result  = parse(&lex_result);
let resolve_result = resolve(&parse_result);
let module_result  = resolve_modules(                            //+
    &parse_result, &resolve_result, &manifest                    //+
);                                                               //+
let type_result   = typecheck(&parse_result, &resolve_result);
// ... rest unchanged
```

Total delta: **two new calls, two new crate imports.** No existing call sites change.

---

## Error summary

All new errors emitted by this task (declared in Task 1, all carry `Span` — Rule 5):

```rust
// ModuleError:
ModuleError::CircularImport  { cycle: Vec<String>, span: Span }
ModuleError::UnknownExport   { name: Symbol, path: String, span: Span }
ModuleError::NoManifest      { path: String, span: Span }
ModuleError::CacheMiss       { name: String, span: Span }

// ManifestError:
ManifestError::ParseError          { message: String, span: Span }
ManifestError::ConflictingManifest { path: String, span: Span }

// ResolveError:
ResolveError::PrivateImport { name: Symbol, path: String, span: Span }
```

## Diagnostics

Add rendering arms for all new error variants. Representative formats:

```
error: circular import
  --> src/a.fe:1:1
   |
 1 | import { foo } from "./b"
   |         ^^^^^^^^^^^^^^^^^ cycle: a.fe → b.fe → a.fe

error: `connect` is not exported from "./db"
  --> src/main.fe:2:10
   |
 2 | import { connect } from "./db"
   |          ^^^^^^^ not marked `export` in db.fe

error: cache package `ferric-http` not found in .ferric/cache/
  --> src/main.fe:3:1
   |
 3 | import { HttpClient } from "ferric-http"
   |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ run `ferric fetch` to populate the cache

error: `@/` imports require a Ferric.toml manifest
  --> src/main.fe:1:1
   |
 1 | import { Config } from "@/config"
   |         ^^^^^^^^^^^^^^^^^^^^^^^^^ no Ferric.toml found in workspace root
```

---

## Done when

- [ ] `ferric_manifest` crate exists with `load_manifest` as its only public function
- [ ] Script mode (no `Ferric.toml`): `ManifestResult { manifest: None, errors: [] }`
- [ ] Workspace mode: manifest parses correctly including `[submodules]` and `[dependencies]`
- [ ] `ManifestError::ConflictingManifest` fires when a submodule has its own `Ferric.toml`
- [ ] `ferric_module` crate exists with `resolve_modules` as its only public function
- [ ] `ImportPath::Relative` resolves correctly relative to the importing file
- [ ] `ImportPath::Workspace` (`@/`) resolves against workspace root; `NoManifest` if no manifest
- [ ] `ImportPath::Cache` resolves from `.ferric/cache/`; `CacheMiss` if not present
- [ ] Circular imports produce `ModuleError::CircularImport` with the full cycle path
- [ ] `ModuleError::UnknownExport` fires when importing a non-exported item by name
- [ ] Namespace imports (`* as ns`) collect all exported names; empty export is valid
- [ ] `ResolveError::PrivateImport` fires when an item exists but is not exported
- [ ] Import bindings are available to the resolver before type checking runs
- [ ] `main.rs` calls `load_manifest` and `resolve_modules` — two new calls, no existing calls changed
- [ ] All new error variants render correctly through the diagnostics renderer
- [ ] All new error types carry `Span` (Rule 5)
- [ ] All M1–M6 tests still pass
