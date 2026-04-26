# Ferric

![ferric](./static/ferric.png)

A small, statically-typed scripting language, implemented with a modular interpreter pipeline. The compiler lowers to bytecode that runs on a stack VM.  The type system is a full HM type system that feels familiar to rust, without the overhead of full borrow checking

## Shell First

Ferric has a first class `$` sigil that allows you to drop into a shell and run arbitrary commands.  Results are returned back to ferric and you can continue on from there

`let zero_nines = curl https://mrshu.github.io/github-statuses/`

## Native Validation

The `require` keyword lets you assert arbitrary expressions as pre conditions or post conditions to any block of code.  You can also use it to idempotently enforce state in a script, on a host machine, or wherever!

## First Class Tooling

Ferric ships with package management, a language server, and a full featured std library.

## Forever Versions

The modular architecture allows shipping multiple versions of a pipeline stage, so even when breaking changes happen you can get the old behavior back with a simple flag. 

## Quick taste

```rust
fn fibonacci(n: Int) -> Int {
    if n <= 1 { n } else { fibonacci(n: n - 1) + fibonacci(n: n - 2) }
}

println(s: int_to_str(n: fibonacci(n: 10)))
```

Call sites use named arguments by default — `println(s: ...)`, `fibonacci(n: ...)`. The
resolver canonicalises them to definition order before the type checker or VM see them.

A few more shapes the language supports today:

```rust
// enums + match
enum Shape {
    Circle(Int),
    Rectangle(Int, Int),
}

fn area(s: Shape) -> Int {
    match s {
        Shape::Circle(r) => r * r * 3,
        Shape::Rectangle(w, h) => w * h,
    }
}

// traits + generic bounds
trait Describable {
    fn describe(self) -> Str
}

impl Describable for Int {
    fn describe(self) -> Str {
        "I am an integer: " + int_to_str(n: self)
    }
}

fn print_description<T: Describable>(val: T) {
    println(s: val.describe())
}

// arrays, for-loops, closures with capture
let bonus = 100
let add_bonus = |n| n + bonus

let mut total = 0
for x in [1, 2, 3, 4, 5] {
    total = total + add_bonus(n: x)
}
```


## Running it

```bash
cargo build
cargo run -- examples/hello/hello.fe   # run a Ferric source file
cargo run                              # start the REPL
cargo test                             # run the full test suite
```

The REPL accumulates session source and re-runs the pipeline on each input. `:reset`
clears the session.

## How it's put together

Source flows through six independent stages, each in its own crate:

```
source → lex → parse → resolve → typecheck → compile → vm
```

`src/main.rs` is the only file allowed to import from more than one stage. Every stage
depends only on `ferric_common`, which holds the shared types — `Span`, `NodeId`,
`Symbol`, `DefId`, `Interner`, the bytecode schema.

| Crate | Job |
|---|---|
| `ferric_common` | Shared types and the bytecode schema. No logic. |
| `ferric_lexer` | Source → tokens. |
| `ferric_parser` | Tokens → AST. |
| `ferric_resolve` | Name resolution, slot assignment, closure capture analysis. |
| `ferric_infer` | Hindley-Milner-style type inference. |
| `ferric_traits` | Trait registry and method dispatch resolution. |
| `ferric_exhaust` | Match exhaustiveness and unreachable-arm checks. |
| `ferric_compiler` | Typed AST → bytecode chunks. |
| `ferric_vm` | Stack VM. Implements the `Executor` trait. |
| `ferric_diagnostics` | rustc-style multi-label error rendering. |
| `ferric_stdlib` | Native function registry: `println`, `int_to_str`, `array_len`, shell exec, etc. |

Stage boundaries are enforced by a handful of rules:

- A stage reads only the output struct of the previous stage, never another stage's
  internals.
- Each stage has exactly one public entry point.
- No mutable global state; no singletons.
- Every error type carries a `Span` — diagnostics without spans cannot be rendered.
- The VM lives behind the `Executor` trait, so swapping the runtime is a one-line
  change in `main.rs`. (M3 swapped a tree-walker for a bytecode VM this way.)
- `Value` is constructed only via `Value::new_*` constructors outside of `ferric_vm`,
  so the value representation can change without touching the rest of the project.
