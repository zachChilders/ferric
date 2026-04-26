# M8 — Task 4: `ferric_async` Lowering Pass

> **Prerequisite:** Tasks 1, 2, and 3 must all be complete before starting this
> task. The lowering pass reads type information from `TypeResult` (Task 3) and
> rewrites AST nodes introduced by the parser (Task 2). Task 5 may begin once
> this task is complete.
>
> See [m8-00-overview.md](m8-00-overview.md) for the milestone-level design
> decisions and the conceptual overview of the state machine transform.

---

## What this task does

Adds `ferric_async` — a new pipeline stage that transforms every `async fn` body
into a pair of ordinary Ferric items: a state enum and a `poll` function. The
output is a `ParseResult` with all `Item::AsyncFn` nodes replaced by their lowered
equivalents. Downstream stages (`ferric_compiler`, `ferric_vm`) see only ordinary
`fn` items and struct-like enums. They require no changes.

---

## New crate: `ferric_async`

### Public entry point

```rust
// ferric_async/src/lib.rs
pub fn lower_async(ast: &ParseResult, types: &TypeResult) -> AsyncResult;
```

`lower_async` returns `AsyncResult { ast, errors, warnings }` where `ast` is a
`ParseResult` — the same type the parser produces, with async items replaced. This
is the key design choice that keeps `ferric_compiler`'s signature unchanged.

### Cargo.toml

```toml
[package]
name    = "ferric-async"
version = "0.1.0"
edition = "2021"

[dependencies]
ferric_common = { path = "../ferric_common" }
```

No dependency on any other stage crate. `ferric_async` reads `ParseResult` and
`TypeResult` from `ferric_common` — both of which it receives as arguments.

---

## Lowering algorithm

The lowering pass makes a single top-level walk of `ast.items`. For each item:

- `Item::AsyncFn(async_fn)` → lower to two items (state enum + poll fn), described below
- `Item::Fn(fn_item)` → walk the body for `async { }` blocks and lower those in place
- All other items → pass through unchanged

### Step 1 — assign suspension points

Walk the `async fn` body depth-first. Every `Expr::Await(AwaitExpr)` is a
**suspension point**. Assign each one a unique `u32` index (0, 1, 2, ...) in
source order. These indices become the state enum variants.

For `Expr::AsyncBlock` nodes nested inside the function body: lower them
recursively as anonymous async fns, then replace the block with a call to the
anonymous fn's constructor. Do not flatten nested async blocks into the parent's
state machine.

### Step 2 — collect captures

For each suspension point, determine which variables are **live across the await**
— that is, declared before the await and used after it. These variables must be
stored in the state enum because execution will resume in a different call to the
poll function.

If a variable is live across an await and is a reference or contains non-`Clone`
data (structural check on `TypeResult`), emit
`AsyncLowerError::CaptureAcrossAwait { name, await_span, capture_span }` and
continue lowering (produce a best-effort output). The resulting code may not
type-check, but the error message is more useful than aborting.

### Step 3 — emit the state enum

For an `async fn` named `foo` with `N` suspension points:

```rust
// Generated — name is mangled to avoid conflicts
enum __FooState {
    __Start {
        // all parameters of foo
        param_a: TypeA,
        param_b: TypeB,
    },
    __Suspend0 {
        // variables live across suspension point 0
        live_var_x: TypeX,
        __fut0: Async<TypeY>,   // the unawaited future at this suspension point
    },
    // ... one variant per suspension point ...
    __SuspendN { ... },
    __Done,
}
```

The enum is emitted as an `Item::Enum` with `pub(crate)` visibility. Its name
is `__` followed by the function name in PascalCase followed by `State`. The
double-underscore prefix marks it as compiler-generated and prevents user code
from naming it.

### Step 4 — emit the poll function

```rust
// Generated — polls the state machine one step
fn __foo_poll(state: mut __FooState) -> Poll<ReturnType> {
    match state {
        __FooState::__Start { param_a, param_b } => {
            // Everything in the fn body up to the first .await
            // The expression being awaited is evaluated but not yet awaited:
            let __fut0 = <expression before first .await>
            *state = __FooState::__Suspend0 { live_var_x, __fut0 }
            Poll::Pending
        }
        __FooState::__Suspend0 { live_var_x, __fut0 } => {
            match __foo_poll(__fut0) {    // poll the inner future
                Poll::Ready(awaited_value) => {
                    // Everything between suspension point 0 and suspension point 1
                    // (or the end of the function if there is no suspension point 1)
                    let __fut1 = <expression before second .await>
                    *state = __FooState::__Suspend1 { ..., __fut1 }
                    Poll::Pending
                }
                Poll::Pending => Poll::Pending
            }
        }
        // ...
        __FooState::__SuspendN { ..., __futN } => {
            match __foo_poll(__futN) {
                Poll::Ready(awaited_value) => {
                    // Remaining body after last .await
                    let result = <trailing expression>
                    *state = __FooState::__Done
                    Poll::Ready(result)
                }
                Poll::Pending => Poll::Pending
            }
        }
        __FooState::__Done => {
            require(warn) false, "polled after completion"
            Poll::Pending
        }
    }
}
```

The poll function is emitted as an `Item::Fn`. Its name is `__` followed by the
function name followed by `_poll`. It takes `state: mut __FooState` and returns
`Poll<ReturnType>`.

### Step 5 — emit the constructor function

The original `async fn foo(param_a: TypeA, param_b: TypeB) -> Str` is replaced
by a plain `fn` that constructs the initial state and wraps it in an `Async<T>`:

```rust
// Replaces the original async fn declaration
fn foo(param_a: TypeA, param_b: TypeB) -> Async<Str> {
    let initial_state = __FooState::__Start { param_a, param_b }
    Async::new(state: initial_state, poll: __foo_poll)
}
```

`Async::new` is a built-in constructor registered in `ferric_stdlib` that takes
an initial state value and a poll function pointer and returns an `Async<T>`. It
is the only way to construct an `Async<T>` value — Rule 7 compliance for the new
value variant.

After this step, `Item::AsyncFn` no longer appears in the output AST. Every
`async fn` is represented as three items: the state enum, the poll function, and
the constructor function — all ordinary `Item::Enum` and `Item::Fn`.

### `async { }` block lowering

`Expr::AsyncBlock(AsyncBlockExpr { block })` is lowered as if it were an anonymous
async fn with no parameters:

1. Assign a unique name: `__anon_async_N` where `N` is a counter.
2. Emit the same three items (state enum, poll fn, constructor fn) as for a named
   async fn.
3. Replace the `AsyncBlockExpr` in the AST with a call to the constructor:
   `__anon_async_N()`.

### `$` shell expression lowering

Shell expressions (`Expr::Shell(ShellExpr)`) are treated differently based on
async context, which `lower_async` determines by tracking depth the same way the
type checker does.

**Inside an async context:** The `ShellExpr` is lowered to an awaitable native
call — `shell_run_async(cmd: <command_str>)` — which is registered in
`ferric_stdlib` (Task 5) and returns `Async<ShellOutput>`. The `.await` is
inserted automatically by the lowering pass; the call site sees `ShellOutput`
as before.

**Outside an async context:** The `ShellExpr` is left as-is (the VM continues
to execute it as a blocking subprocess call). An `AsyncWarning::BlockingShell`
is added to `AsyncResult::warnings` with the span of the shell expression.

### Infinite async recursion detection

If an `async fn foo` directly awaits a call to itself at every code path (i.e.,
there is no non-recursive branch), emit
`AsyncLowerError::InfiniteAsyncRecursion { fn_name, span }`. This is a
best-effort check — only direct self-recursion is detected. Mutual recursion is
not detected here; it surfaces as a runtime stack overflow.

---

## Output invariant

The `AsyncResult::ast` field must be a valid `ParseResult` that satisfies:

1. No `Item::AsyncFn` variants remain anywhere in `ast.items` or nested blocks.
2. No `Expr::Await` variants remain anywhere — they have all been compiled away
   into `match poll(...)` expressions.
3. No `Expr::AsyncBlock` variants remain — they have been replaced by constructor
   calls.
4. All generated names (`__FooState`, `__foo_poll`, `__anon_async_N`, etc.) are
   unique within the compilation unit.

Add assertions for all four invariants as debug-mode checks at the end of
`lower_async`. They are compile-time assertions, not runtime overhead.

---

## `main.rs` changes

```rust
// ferric_async is a new dependency in the workspace Cargo.toml
use ferric_async::lower_async;

// In the pipeline:
let async_result  = lower_async(&parse_result, &type_result);
// Errors and warnings from async_result are rendered before compilation:
if !async_result.errors.is_empty() { render_and_exit(&async_result.errors); }
for w in &async_result.warnings { renderer.warn(w); }
// Downstream stages receive the rewritten AST:
let program = compile(&async_result.ast, &resolve_result, &type_result);
```

Total delta: one new call to `lower_async`, one new import. The `compile` call
site changes its first argument from `&parse_result` to `&async_result.ast` — this
is the only change to an existing call.

---

## Done when

- [ ] `ferric_async` crate exists with `lower_async` as its only public function
- [ ] `AsyncResult` is returned with `ast`, `errors`, and `warnings` fields populated
- [ ] All `Item::AsyncFn` nodes are replaced by state enum + poll fn + constructor fn
- [ ] All `Expr::Await` nodes are replaced by `match poll(...)` expressions
- [ ] All `Expr::AsyncBlock` nodes are replaced by anonymous async fn constructor calls
- [ ] `AsyncLowerError::CaptureAcrossAwait` fires for variables live across a suspension point that cannot be safely moved into the state enum
- [ ] `AsyncLowerError::InfiniteAsyncRecursion` fires for direct self-recursive async fns with no base case
- [ ] Shell expressions inside async contexts are lowered to `shell_run_async(...).await`
- [ ] Shell expressions outside async contexts emit `AsyncWarning::BlockingShell` and are unchanged
- [ ] Output invariant: no `AsyncFn`, `Await`, or `AsyncBlock` nodes remain in `AsyncResult::ast`
- [ ] Generated names are unique within the compilation unit
- [ ] `main.rs` compiles with one new call and one changed argument — no other changes
- [ ] All M1–M7 programs pass through `lower_async` unchanged (no async nodes to transform)
- [ ] All new error types carry `Span` (Rule 5)
