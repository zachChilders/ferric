# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
cargo build                          # build
cargo run -- examples/hello.fe       # run a Ferric source file
cargo run                            # start the REPL
cargo test                           # run all tests
cargo test -p ferric_parser          # run tests for one crate
cargo check                          # fast type-check without compiling
```

## What this is

Ferric is a language interpreter written in Rust. The project is milestone-driven; each milestone extends or replaces stages while keeping the pipeline shape fixed. The current implementation is **M2** (while loops, mutable vars, floats, span-annotated errors). **M2.5** tasks are in `docs/tasks/m2.5-*.md`.

## Pipeline

Source code flows through six independent stages, each in its own crate:

```
source → lex() → parse() → resolve() → typecheck() → Program → vm.run()
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
// Executor trait in ferric_vm — TreeWalker implements it now, BytecodeVM in M3
fn run(&mut self, program: Program, natives: NativeRegistry, interner: &Interner) -> Result<Value, RuntimeError>;
```

## Key types (all in `ferric_common`)

- `Span { start: u32, end: u32 }` — byte offsets into source
- `NodeId(u32)` — unique ID on every AST node; later stages attach metadata by NodeId
- `Symbol(u32)` — interned string handle; resolve to `&str` via `interner.resolve(sym)`
- `DefId(u32)` — identifier for a variable/function definition; used for slot assignment
- `Interner` — passed through the pipeline; `intern()` produces `Symbol`, `resolve()` recovers the string
- `Ty::Unknown` — escape hatch accepted everywhere without error; intentionally removed in M3

## Upcoming work (M2.5)

Four tasks planned; **Task 1 (named parameters) must complete first** as it modifies `CallExpr` in `ferric_common`, which all stages read:

1. `m2.5-01-named-params.md` — make named parameters mandatory at all call sites; resolver canonicalises to definition order before typecheck/VM see it
2. `m2.5-02-require.md` — module/require system
3. `m2.5-03-shell.md` — shell integration
4. `m2.5-04-async-prep.md` — async preparation

## Ferric language syntax (current)

```rust
fn fibonacci(n: Int) -> Int {
    if n <= 1 { n } else { fibonacci(n - 1) + fibonacci(n - 2) }
}

let mut counter = 0
while counter < 5 {
    println(int_to_str(counter))
    counter = counter + 1
}
```

Stdlib: `println(s: Str)`, `print(s: Str)`, `int_to_str(n: Int)`, `float_to_str(n: Float)`, `bool_to_str(b: Bool)`, `int_to_float(n: Int)`.
