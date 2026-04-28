# Async / Await

Ferric has lazy `async fn`s, `async { ... }` blocks, prefix `await expr`, and an OS-thread-backed scheduler that gives real wall-clock parallelism for `spawn`-ed tasks.

For the milestone-shaped build plan, see [`../asyncawait.md`](../asyncawait.md). This doc is a usage reference.

## Syntax

```rust
// async function
async fn fetch(url: Str) -> Str { url }

// async block expression — lifts a synchronous block into an Async<T>
let task: Async<Str> = async { await fetch(url: "hi") }

// prefix await — only legal inside an async fn body or async block
async fn pipeline() -> Str {
    let a = await fetch(url: "x")
    let b = await fetch(url: "y")
    a + b
}
```

`await` is **prefix**, not postfix. `expr.await` does not parse. To call a method on an awaited value, parenthesise:

```rust
let len = (await fetch(url: "x")).len()
```

`await` precedence is the same tier as `-` and `!`.

## Types

Two new wrapper types live in the surface language:

- `Async<T>` — a deferred async computation, not yet running on a worker.
- `Handle<T>` — a reference to a task that is running (or has run) on a worker thread, returned by `spawn`.

Both wrap a result type `T`. Calling an `async fn` of return `T` produces `Async<T>`; `spawn(task: ...)` accepts `Async<T>` and returns `Handle<T>`; `await` on either produces `T`.

## Driving async work

| Function | Signature | Notes |
|---|---|---|
| `await expr` | `Async<T> ⟶ T` or `Handle<T> ⟶ T` | Inside async only. |
| `spawn(task: t)` | `Async<T> ⟶ Handle<T>` | Submits to the scheduler; returns immediately. |
| `join(a, b)` | `(Handle<A>, Handle<B>) ⟶ (A, B)` | Waits for both. |
| `sleep(ms: n)` | `Int ⟶ Async<Unit>` | Use with `await sleep(ms: 250)`. |
| `block_on(task: t)` | `Async<T> ⟶ T` | Host-side runner for sync top-level. |
| `shell_run_async(cmd: c)` | `Str ⟶ Async<ShellOutput>` | Lowering of `$` inside async. |

`spawn`, `join`, `sleep`, `block_on`, and `shell_run_async` are not normal natives — they're scheduler-aware and dispatched directly inside `Op::Call`. See `crates/ferric_vm/src/bytecode.rs`.

## Examples

Single async call:

```rust
async fn fetch(url: Str) -> Str { url }

let task: Async<Str> = async { await fetch(url: "hello world") }
let got: Str = block_on(task: task)
println(s: got)
```

Spawn + join (true parallelism):

```rust
async fn slow_task(n: Int) -> Int {
    await sleep(ms: 500)
    n
}

let h1 = spawn(task: slow_task(n: 1))
let h2 = spawn(task: slow_task(n: 2))

let pair = join(a: h1, b: h2)
match pair {
    (a, b) => println(s: int_to_str(n: a + b))
}
```

Both `slow_task`s sleep concurrently — wall clock is ~500 ms, not ~1000 ms. See `examples/m8_async_parallel/`.

## Semantics — lazy evaluation

Calling an `async fn` does **not** run its body. It builds a deferred `Value::Async(AsyncCell)` containing the chunk index and captured arguments. The body runs only when:

- `await` drives the cell to completion in the current thread, or
- `spawn` moves it onto a worker thread, which in turn runs it.

This is what makes `spawn` actually parallel: the work hasn't happened yet at the moment you spawn it, so it can be moved to a worker.

A consequence: side effects in an async body don't fire until the value is awaited or spawned. `let _ = some_async_fn()` evaluates to `Async<T>` but runs no code.

## Where `await` is allowed

- Inside an `async fn` body.
- Inside an `async { ... }` block.
- Nowhere else. The parser fast-paths this for top-level scope; the type checker enforces it everywhere else as `TypeError::AwaitOutsideAsync`.

## Errors

The type checker enforces:

| Error | When |
|---|---|
| `AwaitOutsideAsync` | `await` at top level or inside a sync fn. |
| `AwaitOnNonAsync` | `await x` where `x` isn't `Async<_>` or `Handle<_>`. |
| `SpawnNonAsync` | `spawn(task: x)` where `x` isn't `Async<_>`. |
| `JoinNonHandle` | `join(a, b)` where either isn't `Handle<_>`. |
| `AsyncNotAwaited` | `let x: Int = some_async_fn()` — async value bound with non-async annotation. |
| `InfiniteAsyncRecursion` | `async fn f() -> T { await f() }` — direct self-await. |

## Shell inside async

A `$` expression inside an `async fn` or `async { ... }` is rewritten by the lowering pass to `await shell_run_async(cmd: ...)`. From the user's perspective the syntax is unchanged.

A program that uses `async` *anywhere* but also runs `$` at sync top level emits `AsyncWarning::BlockingShell` — those calls are blocking and may starve the runtime if they're long. In a pure-sync program (no `async` syntax at all), the warning is suppressed.

## Architecture pointers

- `Value::Async(AsyncCell)` wraps `Arc<Mutex<AsyncState>>` with `Pending { chunk_idx, captures } | Ready(Value)`.
- `Value::Handle(u64)` is an opaque task id; the scheduler owns the running thread.
- The VM holds `Arc` references to chunks, the native registry, the interner, and the scheduler so worker threads can construct `BytecodeVM::worker(...)` instances sharing all program state.
- Each `Item::AsyncFn` compiles to **two** chunks: a constructor chunk that just packages args + body chunk index into a `Pending` async, and a body chunk holding the actual code.

## See also

- [`../asyncawait.md`](../asyncawait.md) — milestone breakdown of the async build.
- [`../tasks/complete/m8-*.md`](../tasks/complete/) — task-level decisions.
- [`shell.md`](shell.md) — `$` lowering inside async.
