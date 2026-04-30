# Modules, Imports, Exports, Manifest

Ferric files compose through explicit `import` / `export` declarations. A workspace is rooted at a `Ferric.toml` manifest; without a manifest, the program runs in **script mode** and only file-relative imports work.

For the milestone build plan, see [`../module_system.md`](../module_system.md). This doc is a usage reference.

## Privacy by default

Every top-level item is private to its file unless marked `export`:

```rust
// util.fe
export fn double(n: Int) -> Int { n * 2 }
export struct Point { x: Int, y: Int }

fn private_helper() -> Int { 99 }   // not visible to importers
```

There are **no** default exports. `import X from "./util"` is a parse error. Use the named-import form below.

## Three import path shapes

```rust
import { double, Point } from "./util"          // file-relative
import { connect } from "../db/conn"            // file-relative (parent)
import { config } from "@/config"               // workspace-root-relative
import { get } from "ferric-http"               // cache dependency
```

| Shape | Meaning | Requires |
|---|---|---|
| `"./..."` / `"../..."` | Resolved relative to the importing file. | Nothing — works in script mode. |
| `"@/..."` | Resolved relative to workspace root (next to `Ferric.toml`). | `Ferric.toml` exists. |
| `"ferric-foo"` | Cache dependency loaded from `.ferric/cache/`. | `Ferric.toml` + a dependency entry. |

## Aliases and namespaces

Rename on import:

```rust
import { connect as open_db } from "./db"
```

Whole-module namespace import:

```rust
import * as db from "./db"
db::connect()
```

A namespace import binds a single symbol to a *module value*. Member access is via `::`, the same path operator used for enum variants.

## Manifest — `Ferric.toml`

The manifest is optional. When present, it defines the workspace root (the directory containing it) and any cache dependencies. When absent, the entry file's parent directory is the implicit root and only `./` imports are valid; `@/` and cache imports produce `ModuleError::NoManifest`.

```toml
# Ferric.toml — illustrative
[package]
name = "my-script"
version = "0.1.0"

[dependencies]
ferric-http = "1.2"
```

## Cache layout

Cache dependencies live at:

```
workspace/
├── Ferric.toml
├── .ferric/
│   └── cache/
│       ├── ferric-http-1.2.0/
│       │   ├── Ferric.toml
│       │   └── src/
│       └── ferric-json-0.8.3/
│           ├── Ferric.toml
│           └── src/
└── src/
    └── main.fe
```

The cache is workspace-local. There is no global cache. `.ferric/` should be gitignored.

A `ferric fetch` CLI command for populating the cache is not yet shipped — for now, packages are dropped in manually for testing.

## Type aliases — opaque

`type` declares an **opaque** alias:

```rust
type Url = Str
type UserId = Int
```

Opaque means the type checker treats `Url` and `Str` as distinct — you cannot pass a `Str` where a `Url` is expected (or vice versa) without an explicit `as` cast:

```rust
let raw: Str = "https://example.com"
let url: Url = raw as Url           // explicit wrap
let s:   Str = url as Str           // explicit unwrap
```

At runtime, opaque types **erase** to their inner representation — `as` is a no-op in the VM. The point is purely compile-time discipline.

In M7, all aliases are opaque. The `opaque` keyword is reserved for a possible future "transparent alias" mode.

## Errors

| Error | Condition |
|---|---|
| `ModuleError::NoManifest` | `@/` or cache import in a script-mode program. |
| `ModuleError::FileNotFound` | Relative path doesn't resolve. |
| `ModuleError::CircularImport` | DFS detects a back edge — the message names every file in the cycle. |
| `ModuleError::UnknownExport` | Imported name is not exported by the target file. |
| `ModuleError::DependencyMissing` | Cache import for a name not declared in `[dependencies]`. |

## Pipeline shape

`ferric_manifest::load_manifest` runs once at the start (before resolution), then `ferric_module::resolve_modules` walks imports and stitches every file into the symbol table. The resolver runs **twice**: once with only natives + builtins (so `resolve_modules` has something to consult), then again with imports wired into the global scope.

See `src/main.rs` for the call sites.

## See also

- [`../module_system.md`](../module_system.md) — milestone breakdown.
- [`types.md`](types.md) — opaque type aliases in the type system.
