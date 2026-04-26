# M8 — Async/Await: Overview

> Each task in this milestone produces a fully passing test suite before the next
> begins. Task 1 must be completed first — it settles the shared types in
> `ferric_common` that all other tasks depend on. Tasks 2 and 3 may proceed in
> any order after Task 1. Task 4 requires Tasks 1, 2, and 3. Task 5 requires
> Tasks 1, 2, and 4.

---

## Goal

Add first-class async/await to Ferric. After this milestone, functions can be
declared `async`, their bodies can contain `.await` expressions, and the runtime
executes async call graphs concurrently on a cooperative scheduler embedded in the
VM. Programs can perform non-blocking I/O, sleep, and run independent tasks in
parallel — all without threads.

---

## Design decisions (settled — do not relitigate)

- **Async is a compiler transform, not a VM feature.** The compiler lowers `async fn`
  bodies into explicit state machine structs before bytecode generation. The
  `BytecodeVM` and the `Executor` trait are unchanged. This preserves every existing
  stage boundary.

- **`.await` is suffix syntax.** `fetch(url: u).await` — not a keyword statement,
  not a macro. It is an expression with lower precedence than field access and higher
  precedence than binary operators (same precedence slot as `as` from M7).

- **`async fn` returns `Async<T>`, not `T`.** This is visible in the type system.
  Calling an `async fn` without `.await` is legal and yields an `Async<T>` value.
  Applying `.await` outside an `async` context is a type error.

- **The scheduler is inside `ferric_vm`, injected via `Executor::run`.** The
  `NativeRegistry` fn type changes to return `AsyncVal` (a `Value` or a pending
  future) — this is the breaking change documented in M2.5 Task 4. It is
  contained entirely within `ferric_vm` and `ferric_stdlib`; no other stage sees it.

- **`async` blocks are supported.** `async { expr }` is an expression of type
  `Async<T>` where `T` is the type of `expr`. This allows ad-hoc async values
  without a full function definition.

- **Structured concurrency via `spawn` and `join`.** `spawn(task: async_expr)`
  submits an `Async<T>` to the scheduler and returns a `Handle<T>`. `handle.await`
  waits for it. `join(a: handle_a, b: handle_b)` awaits both and returns a tuple.
  These are stdlib functions, not keywords.

- **`$` shell expressions become async.** The `ShellExpr` AST node (M2.5 Task 3)
  is unchanged. The lowering pass (Task 4 of this milestone) recognises shell
  expressions and emits an awaitable subprocess call instead of a blocking one.
  Existing `$ cmd` syntax continues to work — it is now non-blocking inside async
  contexts and blocking (with a compile warning) outside them.

- **No `Send` requirement on async tasks.** Ferric is single-threaded cooperative
  concurrency. Tasks do not cross thread boundaries. The `Send + Sync` invariants
  from M2.5 Task 4 are maintained — but they are maintained because the types are
  clean, not because the scheduler is multi-threaded.

---

## New pipeline stage

One new crate slots into the pipeline between the type checker and the compiler:

```
lex → parse → resolve → typecheck → [ferric_async] → compile → run
```

`ferric_async` runs after the type checker (it needs type information to distinguish
`Async<T>` from `T` at await sites) and before the compiler (it rewrites the AST
so the compiler sees only ordinary closures and state structs).

`main.rs` delta across the full milestone: **one new call, one new import.**
No existing stage signatures change. The `NativeRegistry` fn type change is
contained within `ferric_vm` and `ferric_stdlib` — `main.rs` does not see it.

---

## Task breakdown

| Task | What it does | Prerequisite |
|------|--------------|--------------|
| [Task 1](m8-01-common-types.md) | Add all new `ferric_common` types | None — do this first |
| [Task 2](m8-02-lexer-parser.md) | Lexer + parser: `async fn`, `.await`, `async` blocks | Task 1 |
| [Task 3](m8-03-typecheck.md) | Type checker: `Async<T>`, await inference, error kinds | Task 1 |
| [Task 4](m8-04-lowering.md) | `ferric_async` lowering pass: state machine transform | Tasks 1, 2, 3 |
| [Task 5](m8-05-vm-stdlib.md) | VM scheduler + `NativeRegistry` upgrade + stdlib | Tasks 1, 2, 4 |

---

## State machine lowering — conceptual overview

This section exists so every task author shares the same mental model of what
Task 4 produces. It is not a specification — Task 4's doc is.

An `async fn` in Ferric is transformed by `ferric_async` into a pair of items:

**1. A state enum** — one variant per `.await` point (plus Start and Done):

```rust
// Source
async fn fetch_and_log(url: Str) -> Str {
    let body = http_get(url: url).await
    println(s: body)
    body
}

// After lowering (conceptual — not valid Ferric syntax, shown for clarity)
enum __FetchAndLogState {
    Start { url: Str },
    AwaitHttpGet { url: Str, __fut: Async<Str> },
    Done,
}
```

**2. A `poll` function** — advances the state machine one step when called by
the scheduler:

```rust
fn __fetch_and_log_poll(state: mut __FetchAndLogState) -> Poll<Str> {
    match state {
        Start { url } => {
            let __fut = http_get(url: url)   // no .await — just the Async<Str>
            *state = AwaitHttpGet { url, __fut }
            Poll::Pending
        }
        AwaitHttpGet { url, __fut } => {
            match __fut.poll() {
                Poll::Ready(body) => {
                    println(s: body)
                    *state = Done
                    Poll::Ready(body)
                }
                Poll::Pending => Poll::Pending
            }
        }
        Done => panic("polled after completion")
    }
}
```

The original `async fn fetch_and_log` is replaced in the AST with a plain `fn`
that constructs and returns an `Async<T>` wrapping the initial state and the poll
function. The compiler and VM see none of this — they compile and run ordinary
Ferric functions.

---

## `main.rs` changes

```rust
// New call order — addition marked with //+
let manifest       = load_manifest(&workspace_root);
let lex_result     = lex(&source, &mut interner);
let parse_result   = parse(&lex_result);
let resolve_result = resolve(&parse_result);
let module_result  = resolve_modules(&parse_result, &resolve_result, &manifest);
let type_result    = typecheck(&parse_result, &resolve_result);
let async_result   = lower_async(&parse_result, &type_result);    //+
let program        = compile(&async_result.ast, &resolve_result, &type_result);
let executor       = BytecodeVM::new(natives);
executor.run(program, natives)
```

`lower_async` returns an `AsyncResult` whose `.ast` field is the rewritten
`ParseResult` — identical in type to the original, with `async fn` items replaced
by their lowered equivalents. Downstream stages (`compile`, the VM) receive the
rewritten AST and require no changes.

Total delta: **one new call, one new import.** No existing call sites change.

---

## Replacement log entry

| Milestone | Stage added        | Blast radius on existing crates                           |
|-----------|--------------------|-----------------------------------------------------------|
| M8        | `ferric_async`     | `main.rs`: +1 call; `ferric_vm`: scheduler + NativeRegistry fn type; `ferric_stdlib`: native fns return `AsyncVal` |

---

## Milestone done when

- [ ] All Task 1–5 checklists are complete
- [ ] All M1–M7 programs still pass unchanged (sync programs are unaffected by the lowering pass)
- [ ] `async fn` with a single `.await` runs correctly end-to-end
- [ ] `async fn` with multiple `.await` points in sequence runs correctly
- [ ] `async fn` called without `.await` returns an `Async<T>` value that can be stored and awaited later
- [ ] `.await` outside an `async` context is a `TypeError::AwaitOutsideAsync` with span
- [ ] `spawn` + `handle.await` runs two async tasks concurrently
- [ ] `join` awaits multiple handles and returns a tuple of results
- [ ] `$ shell` expressions inside `async fn` are non-blocking
- [ ] `$ shell` expressions outside `async fn` emit `AsyncWarning::BlockingShell` and behave as before
- [ ] `ferric --dump-ast` output includes `AsyncFnItem`, `AwaitExpr`, and `AsyncBlockExpr` nodes
- [ ] All new error types across all stages carry `Span` (Rule 5)
- [ ] All new `ferric_common` types are `Send + Sync` (async compatibility gate)
- [ ] `Executor` trait signature is unchanged — M8 passes the existing test that replacing the VM requires only a `main.rs` change
