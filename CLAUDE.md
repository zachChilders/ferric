# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
cargo build                                # build
cargo run -- examples/hello/test.fe        # run a Ferric source file
cargo run                                  # start the REPL
cargo test                                 # run all tests
cargo test -p ferric_parser                # run tests for one crate
cargo check                                # fast type-check without compiling
```

## End-to-end test fixtures live in `examples/`

**This is the canonical place for every Ferric program used as a test.**
Do *not* write `.fe` files to `/tmp` or other ephemeral locations when
iterating on language behaviour — put them under `examples/` so the next
run picks them up.

Each subdirectory is a self-contained fixture:

```text
examples/<name>/
    test.fe         — the source program (always exactly this filename)
    expected.txt    — combined stdout+stderr the program must produce, byte-for-byte
    exit            — the expected integer exit code (e.g. `0\n`)
```

`tests/examples.rs` discovers every directory under `examples/` that has
all three files and runs them through the `ferric` binary, comparing the
captured output against `expected.txt` and the exit code against `exit`.

To add a new test: drop a directory with those three files. To capture
the expected output for a new fixture, run the program once with output
redirected:

```bash
./target/debug/ferric examples/<name>/test.fe > examples/<name>/expected.txt 2>&1
echo $? > examples/<name>/exit
```

A directory missing `expected.txt` is silently skipped — useful only as a
documentation fixture. A directory may contain additional `.fe` files
besides `test.fe` (e.g. modules imported by `test.fe`); only `test.fe` is
the entry point.

**Prefer this pattern over standalone Rust integration tests** for
end-to-end language behaviour. Reserve `tests/*.rs` for the harness
itself or for pure-Rust behaviour that has no Ferric-program form.

## What this is

Ferric is a language interpreter written in Rust. The project is milestone-driven; each milestone extends or replaces stages while keeping the pipeline shape fixed. The current implementation is **M9** (algebraic effects, one-shot continuations, async/await and generators via effects desugaring). Completed milestone specs live in `docs/tasks/complete/`.

## Pipeline

Source code flows through these independent stages, each in its own crate:

```
source → lex() → parse() → resolve() → typecheck() → lower_effects() → Program → vm.run()
```

`main.rs` is the only file that imports from multiple stages. Every other stage only imports from `ferric_common`.

## Non-negotiable architecture rules

These rules exist to make stage replacement safe. Violating them creates coupling that makes later replacements expensive.

1. **Stages communicate only through output types** — a stage may only read the output struct of the immediately preceding stage, never internal types or functions of another stage.
2. **Each stage has exactly one public entry point** — everything else is private.
3. **Output types live in `ferric_common`** — no stage depends on another stage, only on `ferric_common`.
4. **No mutable global state** — no `lazy_static`, no singletons. All state is passed in and returned explicitly.
5. **Every error type carries a `Span`** — no exceptions. Errors without spans cannot be rendered.
6. **The VM is behind the `Executor` trait** — never call `TreeWalker` directly. M3 will swap it for a `BytecodeVM` transparently.
7. **`Value` is never constructed directly outside `ferric_vm`** — use `Value::new_int(5)`, not `Value::Int(5)`. This makes swapping the value representation a single-file change.

## Stage I/O contracts (fixed across milestones)

```rust
pub fn lex(source: &str, interner: &mut Interner) -> LexResult;
pub fn parse(lex: &LexResult) -> ParseResult;
pub fn resolve_with_natives(ast: &ParseResult, native_symbols: &[Symbol]) -> ResolveResult;
pub fn typecheck(ast: &ParseResult, resolve: &ResolveResult, interner: &Interner) -> TypeResult;
pub fn lower_effects(ast: &ParseResult, types: &TypeResult) -> EffectResult;
// Executor trait in ferric_vm — TreeWalker implements it now, BytecodeVM in M3
fn run(&mut self, program: Program, natives: NativeRegistry, interner: &Interner) -> Result<Value, RuntimeError>;
```

## Key types (all in `ferric_common`)

- `Span { start: u32, end: u32 }` — byte offsets into source
- `NodeId(u32)` — unique ID on every AST node; later stages attach metadata by NodeId
- `Symbol(u32)` — interned string handle; resolve to `&str` via `interner.resolve(sym)`
- `DefId(u32)` — identifier for a variable/function definition; used for slot assignment
- `Interner` — passed through the pipeline; `intern()` produces `Symbol`, `resolve()` recovers the string
- `Ty::Unknown` — escape hatch accepted everywhere without error
- `Ty::Eff { effects: Vec<EffectId>, ret: Box<Ty> }` — effect-row return type; inferred automatically by `ferric_infer`

## Ferric language syntax (current)

```ferric
fn fibonacci(n: Int) -> Int {
    if n <= 1 { n } else { fibonacci(n: n - 1) + fibonacci(n: n - 2) }
}

let mut counter = 0
while counter < 5 {
    println(s: int_to_str(n: counter))
    counter = counter + 1
}

// Algebraic effects (M9)
effect Config {
    Get(key: Str) -> Str,
}

let result = handle {
    perform Config::Get(key: "host")
} with {
    Config::Get(key) => resume k with "localhost",
}

// async fn and gen fn desugar to effects automatically
async fn fetch(url: Str) -> Str { url }

gen fn numbers() {
    yield 1
    yield 2
    yield 3
}
```

Stdlib: `println(s: Str)`, `print(s: Str)`, `int_to_str(n: Int)`, `float_to_str(n: Float)`, `bool_to_str(b: Bool)`, `int_to_float(n: Int)`. Full stdlib reference in `language.md`.
