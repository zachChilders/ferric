# Deferred Issues & Known Gaps

Inventory of known issues, half-wired features, and architectural debt that has been deliberately deferred. The Option/Result enum gap is now resolved — see [`option-result-gap.md`](./option-result-gap.md) for the design write-up.

Each entry below explains *why* it's deferred so the trade-off doesn't have to be re-derived later.

## Span loss across the bytecode boundary

- Runtime errors originating in the VM use sentinel `Span::new(0, 0)` (e.g. `ferric_vm/src/bytecode.rs` `dummy_span()` callers).
- Synthesized resolver/manifest nodes also use dummy spans:
  - `ferric_resolve/src/lib.rs:366, 378, 1000, 1054`
  - `ferric_manifest/src/lib.rs:37, 50, 99`
- Documented strategy in `ferric_diagnostics/src/lib.rs:21, 748, 1075`.

**Why deferred:** the right fix is a per-instruction debug-info table mapping `ip → span`, written by the compiler and consumed by the VM when constructing `RuntimeError`s. That's an additive change to `Program`/`Chunk` plus a fan-out to every `RuntimeError` construction site. It changes the bytecode format and is best done alongside other M3-bytecode revisions.

**Resolution path:** add a parallel `Vec<Span>` per chunk, indexed by instruction offset; replace `dummy_span()` with `self.span_at(ip)`.

## Type aliases (M7 Task 1)

- AST node `TypeAliasItem` and `Ty::Opaque` exist.
- `ferric_infer/src/lib.rs` — `type_aliases` map and `type_alias_resolving` set are gated `#[allow(dead_code)]`.
- `ferric_infer/src/lib.rs` — `TODO(M7)`: once the resolver allocates `DefId`s for type aliases (`type_defs` extension), build `TypeAliasMeta` from `_alias` and insert into `self.type_aliases`.
- Parser does not yet emit imports/exports/type aliases — that's M7 Task 2.

**Why deferred:** wiring this up touches three stages (resolver type-def extension, infer alias-use sites, exhaustiveness for opaque-typed scrutinees) and the surface syntax (cast expressions) is only partially landed. It's a self-contained M7 follow-up rather than a fire-and-forget cleanup.

## `opaque` flag is hardcoded true

- `ferric_common/src/ast.rs` — "In M7, all aliases are opaque — `opaque` is reserved for a future transparent-alias mode and is always `true`."

**Why deferred:** this is a pinned design decision, not a bug. The flag is reserved for a transparent-alias mode that requires its own RFC (cast erasure, equality semantics, `Display` rules). Removing the field would force us to re-add it later.

## Native ↔ closure boundary (`array_map` / `filter` / `fold`)

- M6 spec lists `array_map` / `array_filter` / `array_fold` but they're absent from `ferric_stdlib`.
- Native functions cannot call back into closures — the `NativeFn` signature takes `&[NativeValue]` and returns `Result<NativeValue, String>` with no VM handle.

**Why deferred:** invoking a Ferric closure from a native requires re-entering the VM (new frame, capture environment, recursion guard). The clean version is a `&mut dyn Executor` parameter on `NativeFn`, but that touches the async-readiness contract documented in `ferric_stdlib::NativeRegistry`. Conflating the two is risky; better done in lock-step with the async upgrade.

## LSP linter & formatter

- `ferric_lsp/src/extension/linter.rs` — `#![allow(dead_code)]` at file level. Awaiting LSP Task 04+.
- `ferric_lsp/src/extension/formatter.rs` — `#![allow(dead_code)]` at file level. Awaiting M-future formatting milestone.
- `ferric_lsp/src/server.rs` — `linter` and `formatter` server fields marked `#[allow(dead_code)]`.

**Why deferred:** these are scaffolding for LSP tasks that haven't started. Removing the scaffolding now would lose the wiring, and there is no consumer to hook them into.

## LSP `require(warn)` severity

- `ferric_lsp/src/handlers/diagnostics.rs` — TODO comment: `require(warn)` failures should map to `DiagnosticSeverity::WARNING`.

**Why deferred:** the LSP only runs static analysis. `require(warn)` is a runtime concept — its "failure" only happens when the program is executed. There's no current static analyzer that flags warn-mode requires separately, and there's no runtime → LSP feedback channel. Routing this would require either adding a static-analysis pass that surfaces warn-mode requires as warning diagnostics, or running the program from the LSP and streaming diagnostics back. Both are non-trivial design problems orthogonal to "fix the TODO."

## Wrapping integer arithmetic — *resolved*

The wrapping `+` / `-` / `*` / `/` / `%` / unary `-` ops in `ferric_vm/src/bytecode.rs` were switched to checked variants. Overflow now raises `RuntimeError::IntegerOverflow { op, span }`, rendered by the diagnostics crate.

## `__shell_exec` constant duplicated — *resolved*

`SHELL_EXEC_NATIVE` lives in `ferric_common`. `ferric_compiler` and `ferric_stdlib` both consume it through that single source.

## Pipe token contextual lexing — *resolved*

The lexer still emits `Pipe` for `|` (closures need it), but the parser now rejects stray pipes up front with `ParseError::StrayPipe` instead of cascading through the closure-parameter parser. The diagnostic is a single line that points at the `|` and notes that Ferric has no bitwise-or operator.

## Minor dead-code holdouts — *resolved*

The `_KeepNodeId`, `_KeepItem`, `_KeepNamedArg`, and `_unused_imports` shims are gone. `Param` is now imported directly in `ferric_compiler`; `Item` and `NamedArg` were already used in `results.rs` and don't need preservation aliases.
