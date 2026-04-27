# M8 — Task 1: Common Types

> **Do this task first.** It adds all new types to `ferric_common` that every
> other M8 task depends on. Tasks 2, 3, 4, and 5 may not begin until this task is
> complete and the codebase compiles cleanly.
>
> See [m8-00-overview.md](m8-00-overview.md) for the milestone-level design
> decisions and pipeline shape.

---

## What this task does

Adds the AST nodes, stage output types, `Ty` variants, and error variants that
async/await requires. No behaviour changes — this task only extends data structures.
Nothing is wired up yet; that happens in Tasks 2–5.

---

## ferric_common — additions required

### 1. `AsyncFnItem`

`async fn` is a modifier on an ordinary function definition. The parser wraps the
inner `FnItem` in `AsyncFnItem` rather than adding a boolean flag to `FnItem` —
this keeps the two forms structurally distinct and makes exhaustive matching
straightforward.

```rust
pub struct AsyncFnItem {
    pub span: Span,
    pub item: Box<FnItem>,   // the inner fn — params, return type, body unchanged
}
```

Add `Item::AsyncFn(AsyncFnItem)` to the `Item` enum alongside `Item::Fn`.

### 2. `AwaitExpr`

`.await` is a postfix expression. It wraps any expression — not just identifiers.

```rust
pub struct AwaitExpr {
    pub span:    Span,
    pub operand: Box<Expr>,   // the Async<T> value being awaited
}
```

Add `Expr::Await(AwaitExpr)` to the `Expr` enum.

### 3. `AsyncBlockExpr`

`async { ... }` is an expression that lifts a synchronous block into an `Async<T>`.

```rust
pub struct AsyncBlockExpr {
    pub span:  Span,
    pub block: Box<Expr>,   // must be a Block expression
}
```

Add `Expr::AsyncBlock(AsyncBlockExpr)` to the `Expr` enum.

### 4. New `Ty` variants

```rust
// Add to the Ty enum in ferric_common:
Ty::Async(Box<Ty>),           // Async<T> — the return type of an async fn
Ty::Handle(Box<Ty>),          // Handle<T> — returned by spawn(), awaitable
```

`Ty::Async(Box<Ty::Int>)` is the type of `async fn foo() -> Int { ... }` when
called. `Ty::Handle(Box<Ty::Str>)` is the type returned by
`spawn(task: some_async_str_fn())`.

### 5. `AsyncResult`

New stage output type returned by `ferric_async`. Follows the same shape as all
other stage output types.

```rust
pub struct AsyncResult {
    pub ast:    ParseResult,          // rewritten AST — async fns replaced by lowered equivalents
    pub errors: Vec<AsyncLowerError>,
}

pub enum AsyncLowerError {
    // Lowering-time errors — these complement the type errors caught earlier.
    // The type checker catches await-outside-async; the lowering pass catches
    // structural problems that are only visible after the full function body
    // is available for state machine construction.
    CaptureAcrossAwait { name: Symbol, await_span: Span, capture_span: Span },
    InfiniteAsyncRecursion { fn_name: Symbol, span: Span },
}
```

All `AsyncLowerError` variants carry `Span` (Rule 5).

`AsyncResult::ast` is a full `ParseResult` — same type as the parser's output.
This is the key design choice: downstream stages (`ferric_compiler`) accept a
`&ParseResult`, so the lowered AST slots in without any signature change.

### 6. `Poll` — built-in enum for scheduler protocol

`Poll` is a built-in enum registered at VM startup, analogous to `Option` and
`Result`. It is defined here so the type checker and lowering pass can reference it
by type without depending on VM internals.

```rust
// Registered in ferric_common as a well-known DefId constant, like Option and Result.
// Variants:
//   Poll::Ready(T)
//   Poll::Pending
```

Add `Ty::Poll(Box<Ty>)` to the `Ty` enum:

```rust
Ty::Poll(Box<Ty>),   // Poll<T>
```

### 7. New error variants

```rust
// Add to TypeError:
TypeError::AwaitOutsideAsync   { span: Span }
TypeError::AwaitOnNonAsync     { found: Ty, span: Span }   // .await on a non-Async<T> value
TypeError::AsyncBlockInSync    { span: Span }               // async {} where Async<T> can't be used
TypeError::SpawnNonAsync       { found: Ty, span: Span }   // spawn() given a non-Async<T>

// Add to ParseError:
ParseError::AwaitOutsideAsync  { span: Span }   // .await in a non-async fn — caught at parse time
                                                 // as a fast-path; the type checker also catches it
```

All variants carry `Span` (Rule 5).

### 8. `AsyncWarning`

Warnings are distinct from errors — they do not halt compilation. This is the
first use of a formal warning type in `ferric_common`; the renderer (M2) already
supports the `warning:` prefix, so no diagnostics change is needed.

```rust
pub struct AsyncWarning {
    pub span: Span,
    pub kind: AsyncWarningKind,
}

pub enum AsyncWarningKind {
    BlockingShell,   // $ expr outside an async context — will block the thread
}
```

`AsyncWarning` carries a `Span` (Rule 5). Add `AsyncResult::warnings: Vec<AsyncWarning>`.

### 9. `Display` additions for new `Ty` variants

The `Ty::Display` impl in `ferric_common` (added in the LSP milestone) must cover
the new variants. Add arms now so the build fails immediately if a future variant
is added without a display form:

```rust
// Add to impl Display for Ty:
Ty::Async(inner)  => write!(f, "Async<{inner}>"),
Ty::Handle(inner) => write!(f, "Handle<{inner}>"),
Ty::Poll(inner)   => write!(f, "Poll<{inner}>"),
```

### 10. Serialisation and async gate

All new types must:
- Derive `Serialize + Deserialize` (consistent with M2.5 Task 4)
- Derive `Debug + Clone + PartialEq`
- Be `Send + Sync` — add them to the compile-time assertion in `ferric_common/src/lib.rs`:

```rust
fn _assert_send_sync() {
    fn check<T: Send + Sync>() {}
    // ... existing checks ...
    check::<AsyncFnItem>();
    check::<AwaitExpr>();
    check::<AsyncBlockExpr>();
    check::<AsyncResult>();
    check::<AsyncWarning>();
}
```

---

## Done when

- [ ] `AsyncFnItem`, `AwaitExpr`, `AsyncBlockExpr` exist in `ferric_common`
- [ ] `Item::AsyncFn` variant exists
- [ ] `Expr::Await` and `Expr::AsyncBlock` variants exist
- [ ] `Ty::Async`, `Ty::Handle`, and `Ty::Poll` variants exist
- [ ] `AsyncResult` exists with `ast`, `errors`, and `warnings` fields
- [ ] `AsyncLowerError` enum exists with all variants
- [ ] `AsyncWarning` and `AsyncWarningKind` exist
- [ ] All new `TypeError` and `ParseError` variants exist
- [ ] `Ty::Display` covers `Async`, `Handle`, and `Poll` — missing arm is a compile error
- [ ] All new types derive `Serialize + Deserialize + Debug + Clone + PartialEq`
- [ ] All new types are covered by the `Send + Sync` compile-time assertion
- [ ] `ferric_vm/ASYNC_COMPAT.md` is updated: mark the NativeRegistry constraint as "resolved in M8 Task 5"
- [ ] Codebase compiles cleanly with no warnings
- [ ] All M1–M7 tests still pass (no behaviour change — data structures only)
