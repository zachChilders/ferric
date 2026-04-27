# M8 — Task 2: Lexer + Parser

> **Prerequisite:** Task 1 ([m8-01-common-types.md](m8-01-common-types.md)) must
> be complete and the codebase compiling before starting this task. Tasks 2 and
> 3 are independent of each other and may run concurrently if two agents are
> available.
>
> See [m8-00-overview.md](m8-00-overview.md) for the milestone-level design
> decisions.

---

## What this task does

Adds lexer tokens and parser rules for `async fn` declarations, `.await` postfix
expressions, and `async { }` block expressions. After this task, the parser
produces correct AST nodes for all async syntax — but nothing is type-checked
or lowered yet.

---

## Lexer

### New reserved keyword tokens

```rust
// Add to token enum:
Async,   // `async`
Await,   // `await`
```

Both are reserved keywords, not contextual. `async` and `await` are common enough
in Ferric programs that reserving them is correct — and their meaning is
unambiguous at every position they can legally appear.

### No new lexer error variants

All lexer-level errors for this feature are caught in the parser.

---

## Parser

### `async fn` declarations

`async` is a prefix modifier on a `fn` item at the top level or inside a block.
The parser recognises `async fn` as a unit and wraps the parsed `FnItem` in
`AsyncFnItem`.

```rust
// LEGAL — top level
async fn fetch(url: Str) -> Str { ... }

// LEGAL — nested inside a function body (returns a local async fn value)
fn setup() {
    async fn helper(x: Int) -> Int { ... }
}

// ILLEGAL — async on non-fn items
async let x = 5          // ParseError::AsyncOnNonFn
async struct Foo { }      // ParseError::AsyncOnNonFn
```

**New error variant:**

```rust
// Add to ParseError:
ParseError::AsyncOnNonFn { span: Span }   // `async` keyword not followed by `fn`
```

Carries `Span` (Rule 5). Accumulate and continue — do not abort the parse.

### `.await` postfix expressions

`.await` is parsed as a postfix suffix on any expression, at the same precedence
level as field access (`.field`) and method calls (`.method(...)`). This means it
chains naturally:

```rust
fetch(url: u).await               // AwaitExpr wrapping a CallExpr
fetch(url: u).await.len()         // field/method call on the awaited result
nested_async().await.await        // legal syntax — type checker rejects if wrong
```

The parser does **not** reject `.await` in non-async functions. That is the type
checker's job (Task 3). The parser emits `ParseError::AwaitOutsideAsync` only as
a fast-path heuristic when it can determine with certainty that no enclosing
`async` scope exists — specifically, when `.await` appears at the top level of
a file, outside any function. Inside a function body the parser cannot know
whether an enclosing async context will exist after macro expansion or future
language additions, so it defers to the type checker.

```rust
// ParseError::AwaitOutsideAsync fired by parser:
let x = foo().await     // at file top level — no function scope possible

// TypeError::AwaitOutsideAsync fired by type checker (Task 3):
fn sync_fn() -> Int {
    bar().await         // inside a non-async fn
}
```

**Grammar:**

```
postfix_expr ::= primary_expr postfix_suffix*
postfix_suffix ::=
    "." ident                    // field access
  | "." ident "(" named_args ")" // method call
  | "." "await"                  // await — note: `await` is a keyword, not an ident
  | "[" expr "]"                 // index
```

`await` is a keyword token, so `expr.await` does not conflict with `expr.field_name`.
The tokeniser produces `Dot` followed by `Await` — the parser matches this two-token
sequence specifically.

### `async { }` block expressions

`async` followed by a `{` (with optional whitespace) begins an async block
expression. This is an expression, not a statement, and may appear anywhere an
expression is valid.

```rust
let task: Async<Int> = async { expensive_computation() }
let result = async { fetch(url: u).await }.await
```

**Grammar:**

```
async_block_expr ::= "async" block_expr
```

`block_expr` is the existing block production (zero or more statements followed
by an optional trailing expression). The async block's type is `Async<T>` where
`T` is the type of the trailing expression, or `Async<Unit>` if there is none.

The parser does not need to distinguish `async fn` from `async {` at the token
level — it peeks one token ahead after `Async`:

- Next token is `Fn` → parse as `AsyncFnItem`
- Next token is `LBrace` → parse as `AsyncBlockExpr`
- Anything else → `ParseError::AsyncOnNonFn`

### `Async<T>` and `Handle<T>` type expressions

The parser must accept `Async<T>` and `Handle<T>` as type expressions wherever
a `TypeExpr` is valid (let bindings, function params, return types). This is
required by Task 3 (the type checker resolves `TypeExpr::Generic("Async", [T])`
to `Ty::Async(Box<T>)`).

```rust
let task: Async<Int>   = async { 42 }
let h:    Handle<Str>  = spawn(task: fetch(url: u))
fn run(t: Async<Bool>) -> Async<Unit> { ... }
```

### Precedence table update

The `.await` suffix shares the field-access precedence tier. The updated table
(showing only the relevant rows, highest to lowest):

| Precedence | Operators / suffixes                                    |
|------------|---------------------------------------------------------|
| Postfix    | `.field`, `.method(...)`, `.await`, `[index]`           |
| Prefix     | `-`, `!`                                                |
| Cast       | `as`                                                    |
| Mul        | `*`, `/`, `%`                                           |
| Add        | `+`, `-`                                                |
| Compare    | `==`, `!=`, `<`, `>`, `<=`, `>=`                        |
| And        | `&&`                                                    |
| Or         | `\|\|`                                                  |
| Assign     | `=`                                                     |

---

## Diagnostics

Add rendering arms for the new `ParseError` variants:

```
error: `async` must be followed by `fn` or `{`
  --> src/main.fe:3:1
   |
 3 | async let x = 5
   | ^^^^^ `async let` is not valid — did you mean `async fn` or `async { ... }`?

error: `.await` used outside of any function
  --> src/main.fe:1:14
   |
 1 | let x = foo().await
   |              ^^^^^^ `.await` can only appear inside an `async fn` or `async` block
```

---

## `--dump-ast` verification

After this task, `ferric --dump-ast` on a file containing async syntax must
include `AsyncFnItem`, `AwaitExpr`, and `AsyncBlockExpr` nodes in the JSON output.
This verifies that the serialisation from Task 1 is correctly wired through the
parser.

---

## Done when

- [ ] `Async` and `Await` are reserved keyword tokens in the lexer
- [ ] `async fn name(...)` parses into `Item::AsyncFn(AsyncFnItem)` at top level and in blocks
- [ ] `ParseError::AsyncOnNonFn` fires for `async` not followed by `fn` or `{`
- [ ] `expr.await` parses into `Expr::Await(AwaitExpr)` at the correct precedence level
- [ ] `expr.await.field` and `expr.await.method()` parse correctly (postfix chains)
- [ ] `expr.await.await` parses without error (type checker rejects it if wrong)
- [ ] `ParseError::AwaitOutsideAsync` fires for `.await` at file top level only
- [ ] `async { expr }` parses into `Expr::AsyncBlock(AsyncBlockExpr)`
- [ ] `async { }` (empty body) parses into `AsyncBlockExpr` with `Async<Unit>` type (checked in Task 3)
- [ ] The parser correctly distinguishes `async fn` from `async {` by one-token lookahead
- [ ] `Async<T>` and `Handle<T>` parse as valid `TypeExpr` nodes
- [ ] All new `ParseError` variants render correctly through the diagnostics renderer
- [ ] `ferric --dump-ast` output includes `AsyncFnItem`, `AwaitExpr`, and `AsyncBlockExpr` nodes
- [ ] All new error types carry `Span` (Rule 5)
- [ ] All M1–M7 tests still pass
