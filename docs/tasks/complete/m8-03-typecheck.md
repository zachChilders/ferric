# M8 — Task 3: Type Checker

> **Prerequisite:** Task 1 ([m8-01-common-types.md](m8-01-common-types.md)) must
> be complete before starting this task. Task 2 (lexer/parser) does not need to
> be complete — the type checker additions here depend only on the new
> `ferric_common` types, not on the parser implementation. However, both Tasks
> 2 and 3 must be complete before Task 4 begins.
>
> See [m8-00-overview.md](m8-00-overview.md) for the milestone-level design
> decisions.

---

## What this task does

Extends the type checker to understand `Async<T>`, `Handle<T>`, `.await`
expressions, `async` blocks, and the rules governing where each may legally appear.
After this task, all async type errors are caught before any lowering or compilation
occurs.

---

## Type checking rules

### `async fn` return type

An `async fn` with a declared return type of `T` has an inferred return type of
`Async<T>` at call sites. The declared return annotation continues to refer to `T`
— the `Async<>` wrapper is applied by the type checker, not written by the user.

```rust
async fn fetch(url: Str) -> Str { ... }
// Call site type: Async<Str>
// Body return type (what `return` and the trailing expression must produce): Str

let a: Async<Str> = fetch(url: "example.com")   // legal — Async<Str>
let b: Str        = fetch(url: "example.com")   // TypeError::AsyncNotAwaited
```

**New error variant** (declared in Task 1, emitted here):

```rust
TypeError::AsyncNotAwaited { found: Ty, expected: Ty, span: Span }
// "this expression is `Async<Str>` but `Str` is expected — did you forget `.await`?"
```

Carries `Span` (Rule 5).

### `.await` expressions

`expr.await` is valid if and only if:
1. `expr` has type `Async<T>` or `Handle<T>` — if not, emit `TypeError::AwaitOnNonAsync`.
2. The `.await` expression appears inside an `async fn` body or an `async { }` block
   — if not, emit `TypeError::AwaitOutsideAsync`.

The type of a valid `expr.await` is the inner type `T`.

```rust
// LEGAL — inside async fn, awaiting Async<Str>
async fn go() -> Str {
    fetch(url: "example.com").await   // type: Str
}

// TypeError::AwaitOnNonAsync — not an Async<T>
async fn bad1() {
    let x: Int = 5
    x.await
}

// TypeError::AwaitOutsideAsync — not in an async context
fn bad2() -> Str {
    fetch(url: "example.com").await
}
```

### Tracking async context

The type checker maintains an `async_depth: usize` counter, incremented on entry
to each `async fn` body or `async { }` block and decremented on exit. A `.await`
expression is in an async context if and only if `async_depth > 0`.

This is an internal concern of `ferric_typecheck` — it does not appear in any
public stage output type.

### `async { }` blocks

`async { expr }` has type `Async<T>` where `T` is the type of `expr`. The body
is type-checked with `async_depth` incremented, so `.await` is valid inside it.

```rust
let task: Async<Int> = async { 1 + 2 }         // Async<Int>
let task2: Async<Str> = async {
    fetch(url: "a").await + fetch(url: "b").await   // Async<Str>
}
```

An `async { }` block with no trailing expression has type `Async<Unit>`.

### `spawn` and `Handle<T>`

`spawn` is a stdlib function with the signature:

```rust
fn spawn(task: Async<T>) -> Handle<T>
```

It is generic in `T`. The type checker resolves `T` from the argument type using
the existing HM inference machinery — no special casing needed beyond registering
the signature.

`Handle<T>.await` is valid by the same rule as `Async<T>.await`. Its type is `T`.

```rust
let h: Handle<Str> = spawn(task: fetch(url: "a"))
let result: Str    = h.await
```

### `join` — tuple result

`join` is a stdlib function. Because it takes a variable number of `Handle` arguments
of potentially different types, it is handled as a built-in with special type
checker support (like tuples), not as a generic function:

```rust
// Two-handle form:
fn join(a: Handle<A>, b: Handle<B>) -> (A, B)

// Three-handle form:
fn join(a: Handle<A>, b: Handle<B>, c: Handle<C>) -> (A, B, C)
```

Register both forms as separate overloads in the type checker's built-in table.
Up to four handles is sufficient for this milestone. The `join` function is resolved
by arity at call sites.

If `join` is called with non-`Handle<T>` arguments, emit `TypeError::JoinNonHandle { found: Ty, span: Span }`.

```rust
// Add to TypeError (declared in this task — not in Task 1):
TypeError::JoinNonHandle    { found: Ty, span: Span }
TypeError::AsyncNotAwaited  { found: Ty, expected: Ty, span: Span }
```

Both carry `Span` (Rule 5).

### `$` shell expressions in async context

Shell expressions (`ShellExpr`) have type `ShellOutput` regardless of async
context — this is unchanged from M2.5. The async context affects only the
**lowering** (Task 4), not the type. The type checker records whether each
`ShellExpr` is inside an async context by annotating `TypeResult::node_types`
with a special marker type `Ty::ShellOutput` — no change — but Task 4 reads
`TypeResult` to determine the async depth at each shell expression site.

Shell expressions outside any async context generate `AsyncWarning::BlockingShell`
in `AsyncResult::warnings` (produced by the lowering pass in Task 4, not here).
The type checker does not emit warnings — it only records context.

### Type annotation for `Async<T>` and `Handle<T>`

Users may write explicit type annotations using the type syntax accepted by the
parser (Task 2):

```rust
let task: Async<Int>   = async { 42 }
let h:    Handle<Str>  = spawn(task: fetch(url: u))
```

The type checker resolves `TypeExpr::Generic("Async", [T])` to `Ty::Async(Box<T>)`
and similarly for `Handle`.

---

## Error summary

All new `TypeError` variants (all carry `Span` — Rule 5):

```rust
TypeError::AwaitOutsideAsync   { span: Span }              // declared in Task 1
TypeError::AwaitOnNonAsync     { found: Ty, span: Span }   // declared in Task 1
TypeError::AsyncNotAwaited     { found: Ty, expected: Ty, span: Span }   // declared here
TypeError::SpawnNonAsync       { found: Ty, span: Span }   // declared in Task 1
TypeError::JoinNonHandle       { found: Ty, span: Span }   // declared here
```

Note: `AwaitOutsideAsync`, `AwaitOnNonAsync`, and `SpawnNonAsync` were declared
in Task 1. `AsyncNotAwaited` and `JoinNonHandle` are declared as part of this
task and must be added to `ferric_common`.

---

## Diagnostics

Add rendering arms for all new `TypeError` variants:

```
error: `.await` used outside of an `async` context
  --> src/main.fe:3:19
   |
 3 |     fetch(url: u).await
   |                   ^^^^^ this function is not `async`
   |
   = help: declare the enclosing function as `async fn` to use `.await`

error: `.await` applied to a non-async value
  --> src/main.fe:5:11
   |
 5 |     let x = 42.await
   |             ^^^^^^^^ expected `Async<_>`, found `Int`

error: this expression is `Async<Str>` but `Str` is expected
  --> src/main.fe:7:13
   |
 7 |     let s: Str = fetch(url: u)
   |                  ^^^^^^^^^^^^^ this is `Async<Str>`; add `.await` to resolve it
   |
   = help: change to `fetch(url: u).await`
```

---

## Done when

- [ ] `async fn foo() -> T` has call-site type `Async<T>`
- [ ] The body of `async fn` type-checks `return` and trailing expressions against `T`, not `Async<T>`
- [ ] `expr.await` where `expr: Async<T>` has type `T`
- [ ] `expr.await` where `expr: Handle<T>` has type `T`
- [ ] `TypeError::AwaitOnNonAsync` fires when `.await` is applied to a non-`Async`/non-`Handle` type
- [ ] `TypeError::AwaitOutsideAsync` fires when `.await` appears outside any async context
- [ ] `async { expr }` has type `Async<T>` where `T` is the type of `expr`
- [ ] `async { }` (empty) has type `Async<Unit>`
- [ ] `.await` is valid inside `async { }` blocks
- [ ] `spawn(task: expr)` where `expr: Async<T>` returns `Handle<T>`
- [ ] `TypeError::SpawnNonAsync` fires when `spawn` receives a non-`Async<T>`
- [ ] `join(a: h1, b: h2)` where `h1: Handle<A>`, `h2: Handle<B>` returns `(A, B)`
- [ ] `TypeError::JoinNonHandle` fires when `join` receives a non-`Handle<T>`
- [ ] `TypeError::AsyncNotAwaited` fires when an `Async<T>` is used where `T` is expected
- [ ] `Async<T>` and `Handle<T>` are valid type annotations in `let` and `fn` signatures
- [ ] All new `TypeError` variants render correctly through the diagnostics renderer
- [ ] All new error types carry `Span` (Rule 5)
- [ ] All M1–M7 tests still pass
