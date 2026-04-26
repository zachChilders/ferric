# M7 — Module System

> Each task in this milestone produces a fully passing test suite before the next
> begins. Task 1 must be completed first — it settles the shared types in
> `ferric_common` that all other tasks depend on. Tasks 2, 3, and 4 may proceed
> in any order after Task 1 is complete.

---

## Goal

Add a first-class module system to Ferric. After this milestone, programs can be
composed across multiple files, depend on versioned external packages, and enforce
file-local privacy through explicit exports.

---

## Design decisions (settled — do not relitigate)

- All items are **private by default**. Nothing is visible outside its file unless
  marked `export`.
- **No default exports.** `import X from "./file"` is a parse error.
- **Three unambiguous import path shapes:**
  - `"./db"` or `"../db"` — file-relative
  - `"@/db"` — workspace-root-relative (requires manifest)
  - `"ferric-http"` — cache dependency (requires manifest + dependency entry)
- **Type aliases are opaque.** `type Url = Str` creates a type-checker-distinct
  type. Construction and unwrap require explicit `as` casts. At runtime, opaque
  types erase to their inner type — `as` is a no-op in the VM.
- **Circular imports are compile errors.** The module resolver performs a DFS and
  rejects back edges.
- **The manifest (`Ferric.toml`) is optional.** Its absence means script mode —
  only `./` relative imports are valid. Its presence defines a workspace module.
- **The dependency cache is workspace-local.** It lives at `.ferric/cache/` in
  the workspace root. There is no global cache.

---

## New pipeline stages

Two new crates slot into the pipeline between existing stages:

```
lex → parse → resolve → [ferric_manifest] → [ferric_module] → typecheck → compile → run
```

`ferric_manifest` runs before the lexer (it reads `Ferric.toml`, not source files).
`ferric_module` runs after the existing resolver and before the type checker.

`main.rs` delta across the full milestone: **two new calls, two new imports.**
No existing stage signatures change.

---

## Task breakdown

| Task | What it does | Prerequisite |
|------|--------------|--------------|
| [Task 1](m7-01-common-types.md) | Add all new `ferric_common` types | None — do this first |
| [Task 2](m7-02-lexer-parser.md) | Lexer + parser changes for `import`/`export`/`as` | Task 1 |
| [Task 3](m7-03-manifest-resolver.md) | `ferric_manifest` + `ferric_module` stages | Task 1 |
| [Task 4](m7-04-typecheck-vm.md) | Opaque types in type checker + VM module values | Tasks 1, 2, 3 |

---

## Cache layout (reference)

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

`.ferric/` should be gitignored by default. A `ferric fetch` CLI subcommand is
out of scope for this milestone — cache packages may be added manually for testing.

---

## Milestone done when

- [ ] All Task 1–4 checklists are complete
- [ ] All M1–M6 programs still pass unchanged
- [ ] `ferric --dump-ast` output includes `ImportDecl`, `ExportDecl`, `TypeAliasItem`, and `CastExpr` nodes
- [ ] Script mode (no manifest): only `./` imports valid; `@/` and cache imports produce `ModuleError::NoManifest`
- [ ] Workspace mode (manifest present): all three import shapes valid
- [ ] Circular imports produce a clear error naming every file in the cycle
- [ ] Opaque types are enforced by the type checker and erased at runtime
- [ ] All new error types across all stages carry `Span` (Rule 5)
- [ ] All new `ferric_common` types are `Send + Sync` (async compatibility gate)
