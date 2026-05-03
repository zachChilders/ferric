# M9 — Algebraic Effects System: Overview

> **Prerequisite:** M8 (async/await) must be complete and all tests passing before
> starting this milestone. M9 introduces `ferric_effects` alongside `ferric_async`.
> The original plan called for wholesale replacement of `ferric_async`, but the
> implementation chose a more conservative path: `ferric_effects` runs the new
> effects pipeline; `ferric_async` is reduced to a small diagnostic-only pass that
> emits `InfiniteAsyncRecursion` and `BlockingShell` warnings the M8 fixtures
> still depend on. See **Status notes** below for a complete list of deviations.
>
> Task 1 settles all shared types in `ferric_common`. Tasks 2 and 3 may proceed in
> any order after Task 1. Task 4 requires Tasks 1–3. Task 5 requires Tasks 1 and 4.
> Task 6 requires Task 5. Task 7 is final cleanup and requires Task 6.

---

## Goal

Replace the bespoke async state-machine lowering pass (`ferric_async`) with a
general algebraic effects system. After this milestone, `async`/`await`,
generators (`gen fn` / `yield`), and dependency injection are all derived from
one mechanism: **effect declarations, `perform` expressions, and handler blocks**.
No new language features require new compiler machinery — they desugar into effects.

---

## Design decisions (settled — do not relitigate)

- **One-shot continuations only.** A continuation captured by a handler may be
  resumed exactly once. Multi-shot and backtracking effects are not supported.
  This keeps the VM representation simple (a captured continuation is a saved
  stack slice, not a heap-allocated coroutine tree) and rules out effect
  patterns that would surprise users coming from Rust.

- **`ferric_async` is co-resident with `ferric_effects`.** *(Original plan
  called for `ferric_async` to be deleted. Task 7 chose to keep it because
  earlier M9 tasks left `lower_async` load-bearing for two diagnostics
  M8 fixtures depend on: `AsyncLowerError::InfiniteAsyncRecursion` and
  `AsyncWarning::BlockingShell`.)* `ferric_effects` is the primary
  post-typecheck pass and produces `EffectResult`. `lower_async` runs
  alongside it as a tiny diagnostic walk; both crates produce
  `ParseResult`-shaped output that downstream stages consume.

- **`async fn` / `.await` desugar at parse time.** The parser translates:
  - `async fn foo(...) -> T { body }` → `fn foo(...) -> Eff<[Async], T> { body }`
  - `expr.await` → `perform Async::Suspend(expr)`
  into standard AST nodes. After M9 Task 7, `Item::AsyncFn` /
  `AsyncFnItem` and `Expr::Await` / `AwaitExpr` are removed from
  `ferric_common`. `Expr::AsyncBlock` / `AsyncBlockExpr` are **retained**
  — Task 6's type checker uses them as a marker to bump `async_depth` so
  that `await`/`perform Async::Suspend` sites inside an `async { ... }`
  block do not require an explicit `handle ... with { Async::Suspend ... }`
  wrapper.

- **`gen fn` / `yield` desugar the same way.** `gen fn bar(...) -> T { body }`
  becomes `fn bar(...) -> Eff<[Gen<T>], Unit> { body }`, and `yield expr` becomes
  `perform Gen::Yield(expr)`.

- **Effect rows on function types, inferred by the type checker.** A function
  that performs `Async::Suspend` has type `Fn(...) -> Eff<[Async, ...], T>`.
  The type checker infers effect rows using the same HM machinery already in
  place; effect variables unify like type variables. Explicit effect annotations
  are supported but never required.

- **Handler blocks are lexically scoped.** `handle { expr } with { Effect::Op(x) => ... }`
  is an expression. The handler is in scope for the dynamic extent of `expr`.
  Handlers do not escape their lexical scope — this is what makes one-shot
  continuations safe without a borrow checker.

- **Dependency injection falls out naturally.** A `Config::Get` effect with a
  handler that reads from a struct is indistinguishable from a reader monad.
  No dedicated DI mechanism is needed.

- **The VM gains four new opcodes.** `Perform`, `PushHandler`, `PopHandler`,
  `Resume` — all other opcodes are unchanged. The `Executor` trait signature is
  unchanged.

- **Stage boundary is preserved.** `ferric_effects` reads `ParseResult` and
  `TypeResult`; it emits `EffectResult { ast: ParseResult, errors, warnings }`.
  Downstream stages (`ferric_compiler`, `ferric_vm`) see only ordinary `fn` items.

---

## New pipeline position

```
lex → parse → resolve → typecheck → [ferric_async (diagnostics)] → [ferric_effects] → compile → run
```

`ferric_async` is **not** removed from the workspace. It runs as a slim
diagnostic pass *before* `ferric_effects`, emitting two diagnostics the
M8 fixtures depend on (`InfiniteAsyncRecursion`, `BlockingShell`) plus
the in-async shell rewrite. `main.rs` calls both stages — `lower_async`
first (its `AsyncResult.ast` is fed forward), then `lower_effects`.

---

## Syntax overview

### Effect declarations

```rust
// Declare an effect — lives at module scope, not inside fn bodies
effect Async {
    Suspend(future: Async<Str>) -> Str,
}

effect Gen<T> {
    Yield(value: T) -> Unit,
}

effect Config {
    Get(key: Str) -> Str,
}
```

`effect` is a new keyword. Each variant is an operation: `Op(args...) -> resume_type`.
The resume type is what `perform Op(...)` evaluates to at the call site.

### `perform` expressions

```rust
let body = perform Async::Suspend(future: http_get(url: url))
perform Gen::Yield(value: item)
let host = perform Config::Get(key: "db.host")
```

`perform` is a new keyword. It suspends the current computation and invokes the
nearest enclosing handler for that effect. If no handler is in scope, it is a
`EffectError::UnhandledEffect` at runtime (and a type error at compile time for
closed effect rows).

### Handler blocks

```rust
handle {
    let result = fetch_and_log(url: "https://example.com")
    result
} with {
    Async::Suspend(future: f) => {
        // k is the one-shot continuation — resume it with the resolved value
        let value = scheduler_poll(f)
        resume k with value
    }
}
```

`handle { expr } with { Op(args) => handler_body }` is an expression. `k` is the
implicit continuation variable, bound automatically. `resume k with value` invokes
it exactly once.

### `async fn` sugar (parser desugaring)

```rust
// What the programmer writes:
async fn fetch(url: Str) -> Str {
    let body = http_get(url: url).await
    body
}

// What the parser produces (conceptual — not user-visible):
fn fetch(url: Str) -> Eff<[Async], Str> {
    let body = perform Async::Suspend(future: http_get(url: url))
    body
}
```

The parser performs this rewrite. No new AST node types survive into later stages.

### `gen fn` sugar (parser desugaring)

```rust
// What the programmer writes:
gen fn range(start: Int, end: Int) -> Int {
    let mut i = start
    while i < end {
        yield i
        i = i + 1
    }
}

// What the parser produces:
fn range(start: Int, end: Int) -> Eff<[Gen<Int>], Unit> {
    let mut i = start
    while i < end {
        perform Gen::Yield(value: i)
        i = i + 1
    }
}
```

---

## Task breakdown

| Task | What it does | Prerequisite |
|------|--------------|--------------|
| [Task 1](m9-01-common-types.md) | Add `ferric_common` types: effect decls, `perform`, handler blocks, `Eff<R,T>`, `EffectResult` | None — do first |
| [Task 2](m9-02-lexer-parser.md) | Lexer + parser: `effect`, `perform`, `handle...with`, `resume`, `async`/`await`/`gen`/`yield` desugaring | Task 1 |
| [Task 3](m9-03-typecheck.md) | Type checker: effect rows, `Eff<R,T>`, effect inference, handler typing | Task 1 |
| [Task 4](m9-04-lowering.md) | `ferric_effects` lowering pass: continuation capture, handler stack, CPS transform | Tasks 1, 2, 3 |
| [Task 5](m9-05-vm.md) | VM: `Perform`, `PushHandler`, `PopHandler`, `Resume` opcodes; scheduler rewrite | Tasks 1, 4 |
| [Task 6](m9-06-stdlib.md) | Stdlib: built-in `Async`, `Gen`, `Config` handlers; retire `ferric_async` scheduler | Task 5 |
| [Task 7](m9-07-cleanup.md) | Remove `ferric_async`; remove `AsyncFnItem`/`AwaitExpr` from `ferric_common`; update blast-radius log | Task 6 |

---

## `main.rs` changes (as actually shipped)

```rust
use ferric_async::lower_async;       // retained for diagnostics
use ferric_effects::lower_effects;   // new effects pipeline

let async_result = lower_async(&parse_result, &type_result);
// ... render async_result.errors / async_result.warnings ...

let effect_result = lower_effects(&async_result.ast, &type_result, ...);
if !effect_result.errors.is_empty() { render_and_exit(&effect_result.errors); }
for w in &effect_result.warnings { renderer.warn(w); }
let program = compile(&effect_result.ast, &resolve_result, &type_result, ...);
```

Both `lower_async` and `lower_effects` are called. The original plan's
"1 import swap + 1 type rename + 1 argument change" delta does **not**
hold — `main.rs` retains the `lower_async` call alongside the new
`lower_effects` call.

---

## Replacement log entry (as actually shipped)

| Milestone | Stage replaced       | New implementation        | main.rs delta                  |
|-----------|----------------------|---------------------------|--------------------------------|
| M9        | `ferric_async` (partial) | `ferric_effects` co-resident; `ferric_async` retained as diagnostic pass | both stages called             |
| M9        | VM async scheduler   | Effects handler stack (default `Async` handler still uses scheduler under the hood) | 0 — contained in `ferric_vm` |
| M9        | `AsyncVal` native type | plain `Value` return    | 0 — contained in `ferric_stdlib` |

**Pieces retained in place** (load-bearing post-M9):

- `ferric_async` crate, including `lower_async`, `AsyncResult`,
  `AsyncLowerError::InfiniteAsyncRecursion`, and `AsyncWarning::BlockingShell`
- `Expr::AsyncBlock` / `AsyncBlockExpr` (type-checker marker for `async_depth`)
- `Op::MakeAsync`, `Op::Await` (still emitted by other paths and implemented in the VM)
- `Value::Async`, `Value::Handle`, `AsyncCell`, `AsyncState`, the scheduler / `task_seq` / `handles` field on the VM (default `Async` handler runtime state)
- `Ty::Async`, `Ty::Handle`, `Ty::Poll` (still referenced by infer / diagnostics)

**Pieces removed in Task 7:**

- `Item::AsyncFn` variant + `AsyncFnItem` struct
- `Expr::Await` variant + `AwaitExpr` struct

---

## Milestone done when

- [x] All Task 1–7 checklists are complete (with deviations noted in Task 7)
- [x] All M1–M8 programs still pass unchanged — sync programs are unaffected, async programs produce identical output
- [x] `async fn` with a single `.await` runs correctly end-to-end through the effects system
- [x] `async fn` with multiple `.await` points in sequence runs correctly
- [x] `spawn` and `join` work as before — implemented as effect handlers (default `Async` handler defers to the VM scheduler)
- [x] `gen fn` with `yield` produces correct iterator output via `gen::collect`
- [x] `for v in gen_fn() { }` desugars and runs correctly
- [x] A custom `effect` declaration with a `handle...with` block runs correctly
- [x] Dependency injection via a `Config::Get` effect and a test handler works
- [x] `perform` outside any handler emits `EffectError::UnhandledEffect` with span
- [x] `resume k` a second time emits `EffectError::ResumedTwice` with span
- [x] `resume` outside a handler clause emits `ParseError::ResumeOutsideHandler` with span
- [x] Effect row mismatch (wrong effects at call site) emits `TypeError::EffectRowMismatch` with span
- [x] `ferric --dump-ast` shows `PerformExpr`, `HandleExpr`, `ResumeExpr`, `EffectDecl` nodes
- [ ] ~`ferric_async` crate is deleted~ — **not done**: kept as a small diagnostic pass that emits `InfiniteAsyncRecursion` and `BlockingShell`; M9 Task 7 chose conservative cleanup over wholesale deletion.
- [x] `AsyncFnItem`/`AwaitExpr` are gone from `ferric_common` *(but `AsyncBlockExpr` is retained as a type-checker marker — Task 6 uses it to bump `async_depth`).*
- [ ] ~`main.rs` changed by exactly: 1 import swap + 1 type rename + 1 argument change~ — **not done**: `main.rs` calls both `lower_async` and `lower_effects`; the original "1 import swap" delta was abandoned when the diagnostic split was chosen.
- [x] All new error types carry `Span` (Rule 5)
- [x] `Executor` trait signature is unchanged
