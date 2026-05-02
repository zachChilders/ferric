# ferric_async

Async/await lowering pass. Stage 8 of the pipeline, between exhaustiveness checking and compilation.

## Entry point

```rust
pub fn lower_async(ast: &ParseResult, types: &TypeResult) -> AsyncResult
```

## Transformations

**`async fn` → `fn` returning `Async<T>`**  
Each `Item::AsyncFn(decl)` is rewritten to an `Item::Fn` whose return type is `Async<T>` and whose body is wrapped in `AsyncBlock(original_body)`. The compiler then hoists the async body into its own chunk and emits a `MakeAsync` instruction.

**Shell inside async → non-blocking await**  
A `$` shell expression inside an async context would block the worker thread. The lowering pass rewrites it to `await shell_run_async(cmd)` so the shell runs on a separate thread and the async task yields while waiting.

## AsyncResult

```rust
pub struct AsyncResult {
    pub ast: ParseResult,              // rewritten AST
    pub errors: Vec<AsyncLowerError>,
    pub warnings: Vec<AsyncWarning>,
}
```

**Errors**: `InfiniteAsyncRecursion` — an async function that directly calls itself without a base case would create an infinite chain of deferred tasks.

**Warnings**: `BlockingShell` — a `$` expression in a program that uses async syntax but outside any async context. The shell will run synchronously, which may not be intended.
