# Ferric

![ferric](./static/ferric.png)

A small, statically-typed scripting language with a modular interpreter pipeline. Source is lowered to bytecode that runs on a stack VM. The type system is Hindley–Milner with trait-based bounded generics — familiar to Rust without the overhead of borrow checking. Effects are first-class: `async`/`await` and generators desugar into the algebraic effects system rather than bespoke state machines.

## Shell First

Ferric has a first-class `$` sigil that drops into a shell and runs arbitrary commands. The result is a `ShellOutput` value with `stdout` and `exit_code`, returned back into Ferric so the program can keep going.

```rust
let nines = $ curl https://mrshu.github.io/github-statuses/
println(s: shell_stdout(output: nines))
```

Interpolation uses `@{expr}` so Ferric variables compose into the command without string-concatenation gymnastics.

## Native Validation

The `require` keyword asserts arbitrary expressions as pre- or post-conditions on any block of code. It can also idempotently enforce state on a script, a host machine, or anywhere else — `require <cond>, "<msg>", || { <set> }` runs the set block if the condition fails, then re-checks it.

## Named Arguments

Call sites use named arguments by definition order. `println(s: ...)`, `fibonacci(n: ...)`. The resolver canonicalises arguments to definition order before the type checker or VM ever see them, so you can't drift.

## First-Class Tooling

Ferric ships with a workspace manifest (`Ferric.toml`), a module system, a language server (`ferric_lsp`) with a TextMate grammar generated from a single keyword list, and a stdlib that covers strings, collections, IO, time, randomness, regex, JSON, HTTP client/server, sockets, crypto, encoding, processes, logging, and more.

## Algebraic Effects

`effect` declares a named set of operations; `perform` suspends the current computation; `handle ... with` installs a handler that intercepts performs and `resume`s the continuation with a result. Continuations are one-shot. `async fn` and `gen fn` are syntactic sugar — the parser desugars them to `Eff<[Async], T>` and `Eff<[Gen<T>], Unit]` before any other stage sees them.

```rust
effect Config {
    Get(key: Str) -> Str,
}

let host = handle {
    perform Config::Get(key: "db.host")
} with {
    Config::Get(key) => resume k with "localhost",
}
```

## Async/Await with Real Parallelism

`async fn`, `async { ... }`, and postfix `.await` are first class (desugared to `Eff<[Async], T>` at parse time). `spawn` moves a deferred async value onto a worker thread; `join` waits on two handles; `block_on` runs an `Async<T>` from sync code. The wall-clock parallelism test in `examples/m8_async_parallel/` proves two 250 ms `sleep` tasks finish in well under 500 ms.

## Forever Versions

Stages are independently replaceable behind fixed I/O contracts, so older stage implementations can ship alongside newer ones — picking up old behaviour later is a flag, not a fork.

## Quick taste

```rust
fn fibonacci(n: Int) -> Int {
    if n <= 1 { n } else { fibonacci(n: n - 1) + fibonacci(n: n - 2) }
}

println(s: int_to_str(n: fibonacci(n: 10)))
```

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

// async + spawn + join
async fn fetch_one(id: Int) -> Int { id * 2 }

async fn main() -> (Int, Int) {
    let a = spawn(task: fetch_one(id: 1))
    let b = spawn(task: fetch_one(id: 2))
    join(a: a, b: b)
}

let (x, y) = block_on(task: main())
```

## Running it

```bash
cargo build
cargo run -- examples/hello/test.fe        # run a Ferric source file
cargo run                                  # start the REPL
cargo test                                 # run the full test suite
cargo run -- --dump-ast examples/hello/test.fe   # parse only and print the AST as JSON
```

The REPL accumulates session source and re-runs the pipeline on each input. `:reset` clears the session.

End-to-end fixtures live under `examples/<name>/` — each directory pairs a `test.fe` source file with `expected.txt` (combined stdout+stderr) and `exit` (expected exit code). `tests/examples.rs` runs every fixture through the `ferric` binary.

## How it's put together

Source flows through a fixed sequence of independent stages, each in its own crate. Every stage depends only on `ferric_common`; only `src/main.rs` is allowed to import from more than one stage.

```
source
  → lex
  → parse
  → load_manifest        (workspace's Ferric.toml, optional → script mode)
  → resolve              (initial: natives + builtins)
  → resolve_modules      (cross-file imports / exports / cache deps)
  → resolve              (re-run with imports wired into global scope)
  → build_registry       (traits + impl blocks)
  → typecheck            (HM inference + trait constraint solving)
  → check_exhaustiveness (match coverage + unreachable-arm warnings)
  → lower_async          (diagnostic-only backwards-compat pass)
  → lower_effects        (algebraic effects analysis; async/await and gen already desugared by parser)
  → compile              (typed AST → bytecode chunks)
  → vm.run               (Executor — currently BytecodeVM)
```

| Crate | Job |
|---|---|
| `ferric_common` | Shared types: `Span`, `NodeId`, `Symbol`, `DefId`, AST, bytecode schema, errors, `Interner`. No logic. |
| `ferric_lexer` | Source → tokens. |
| `ferric_parser` | Tokens → AST. |
| `ferric_manifest` | Reads optional `Ferric.toml`; reports script mode if absent. |
| `ferric_resolve` | Name resolution, slot assignment, mutability + loop-depth tracking, closure capture analysis. |
| `ferric_module` | Cross-file import/export resolution; relative, workspace (`@/...`), and cache (`ferric-foo`) imports; circular-import detection. |
| `ferric_traits` | Builds the `TraitRegistry` consumed by the type checker. |
| `ferric_infer` | Hindley–Milner inference + trait constraint solving + method dispatch. |
| `ferric_exhaust` | Match exhaustiveness and unreachable-arm checks. |
| `ferric_async` | Retained as a diagnostic-only pass for backwards compatibility; async/await desugaring now happens in the parser. |
| `ferric_effects` | Algebraic effects lowering: assigns effect/op tags, validates handler clauses, performs static analysis. Entry point: `lower_effects(ast, types) -> EffectResult`. |
| `ferric_compiler` | Typed AST → bytecode chunks (one per fn / closure / async body / impl method). |
| `ferric_vm` | Stack VM behind the `Executor` trait. Holds the scheduler for `spawn`/`join`/`await`. |
| `ferric_diagnostics` | Multi-label rustc-style error rendering. |
| `ferric_stdlib` | Native registry: shell + 25 stdlib modules (str, bytes, list, sort, map, set, math, io, path, env, time, rand, re, json, fmt, encode, crypto, http, serve, sock, proc, log, sync, dotenv, llm). |
| `ferric_lsp` | Language server + TextMate grammar generated at build time from `ferric_common::keywords`. |

Stage boundaries are enforced by a handful of rules:

- A stage reads only the output struct of the previous stage, never another stage's internals.
- Each stage has exactly one public entry point.
- No mutable global state; no singletons.
- Every error type carries a `Span` — diagnostics without spans cannot be rendered.
- The VM lives behind the `Executor` trait so the runtime can be swapped in `main.rs` alone. (M3 swapped a tree-walker for a bytecode VM this way.)
- `Value` is constructed only via `Value::new_*` constructors outside `ferric_vm`, so the value representation can change without touching the rest of the project.

For the architectural rationale and full milestone history, see [`docs/architecture.md`](docs/architecture.md). For per-feature deep-dives, see [`docs/features/`](docs/features/). For "what do I touch to add new syntax?", see [`docs/adding-syntax.md`](docs/adding-syntax.md).
