# Ferric Interpreter — MVP-First Architecture Plan

> Build a working end-to-end interpreter as fast as possible, then iterate.
> Each milestone produces a **runnable, shippable interpreter**. Nothing is thrown away —
> but entire stages **will be replaced** as the interpreter matures. Architecture must
> make replacement safe and mechanical.

---

## Core Philosophy

### MVP-first, iterate aggressively

Each milestone extends the previous one. The pipeline shape is fixed from day one.
Later milestones add features within each stage or wholesale replace a stage's
implementation — without touching the stages around it.

An agent must complete one milestone fully (all targets checked, all tests passing)
before starting the next.

### Stages will be replaced. Plan for it.

This is not a theoretical concern. It will happen:

- The **type checker** starts as a simple recursive checker and is later replaced with a
  full Hindley-Milner inference engine.
- The **VM** starts as a tree-walker, becomes a bytecode interpreter, and may later get
  a JIT backend.
- The **error reporter** starts as a one-liner and is later replaced with a span-annotated
  renderer.

Each replacement must be **surgical** — change one stage, run the test suite, done.
This is only possible if every stage boundary is a clean, versioned interface with no
shared mutable state leaking across it.

---

## The Non-Negotiable Architecture Rules

These rules exist entirely to make stage replacement safe. Violating them creates
coupling that makes replacements expensive and error-prone. They must be enforced
from Milestone 1 and never relaxed.

### Rule 1 — Stages communicate only through their output types

A stage may only read the output struct of the immediately preceding stage.
It may not import internal types from another stage, call internal functions,
or share mutable state.

```rust
// LEGAL
fn typecheck(ast: &ParseResult, resolve: &ResolveResult) -> TypeResult { ... }

// ILLEGAL — type checker reaching into resolver internals
fn typecheck(ast: &ParseResult, resolver: &Resolver) -> TypeResult { ... }
//                                        ^^^^^^^^^^
//           This couples typecheck to Resolver's internal structure.
//           Replacing the resolver now requires changing typecheck too.
```

### Rule 2 — Each stage has exactly one public entry point

Internal helpers, sub-passes, and data structures are private. The only
surface area between stages is the entry function and its input/output types.

```rust
// Each stage exposes exactly this shape. Nothing else is pub at the crate root.

pub fn lex(source: &str) -> LexResult;
pub fn parse(tokens: &LexResult) -> ParseResult;
pub fn resolve(ast: &ParseResult) -> ResolveResult;
pub fn typecheck(ast: &ParseResult, resolve: &ResolveResult) -> TypeResult;
pub fn compile(ast: &ParseResult, resolve: &ResolveResult, types: &TypeResult) -> Program;
pub fn run(program: Program, natives: NativeRegistry) -> Result<Value, RuntimeError>;
```

When a stage is replaced, only these signatures must be preserved.
Everything inside is free to be rewritten from scratch.

### Rule 3 — Output types are defined in a shared `common` crate, owned by nobody

`LexResult`, `ParseResult`, `ResolveResult`, `TypeResult`, `Program` live in a
shared `ferric_common` crate that no stage owns. Every stage depends on `ferric_common`.
No stage depends on another stage.

```
ferric_common   (Span, NodeId, Symbol, DefId, all Result types)
     ↑ ↑ ↑ ↑ ↑ ↑
  [every stage imports from here, never from each other]
```

This means replacing `ferric_typecheck` with `ferric_infer` is:
1. Write the new crate implementing `typecheck(...)`.
2. Swap the dependency in `Cargo.toml`.
3. Done. No other crate changes.

### Rule 4 — No mutable global state

No `lazy_static`, no `thread_local`, no global `Interner` singleton. Every piece
of state that a stage needs is passed in explicitly and returned explicitly.
This makes stages independently testable and replaceable in isolation.

```rust
// ILLEGAL — global interner
lazy_static! { static ref INTERNER: Mutex<Interner> = ...; }

// LEGAL — interner passed through, returned with result
pub fn lex(source: &str, interner: Interner) -> (LexResult, Interner);
```

### Rule 5 — Every error type carries a Span. No exceptions.

Every error across every stage must include the source location that caused it.
Errors without spans are not renderable by a replacement renderer — forcing the
renderer to reach into stage internals to reconstruct location, which violates Rule 1.

This rule costs nothing to follow if done from the start.
It is extremely expensive to retrofit.

### Rule 6 — The VM is behind a trait

The VM is never called directly. It is accessed through an `Executor` trait.
This is the hook that makes replacing the tree-walker with a bytecode VM, or
the bytecode VM with a JIT, a one-line change in the CLI.

```rust
pub trait Executor {
    fn run(&mut self, program: Program, natives: NativeRegistry) -> Result<Value, RuntimeError>;
}

// M1: TreeWalker implements Executor
// M3: BytecodeVM implements Executor
// Future: CraneliftJIT implements Executor
```

### Rule 7 — Value is never constructed directly outside ferric_vm

`Value::Int(5)` must never appear outside of `ferric_vm`. All other crates
construct values through functions: `Value::new_int(5)`. This is the hook
that makes swapping the value representation (e.g., to NaN-boxing) a single-file
change inside `ferric_vm` with zero blast radius.

```rust
// ILLEGAL in any crate other than ferric_vm
let v = Value::Int(5);

// LEGAL everywhere
let v = Value::new_int(5);
```

---

## Project Structure

```
ferric/
├── Cargo.toml                  (workspace)
├── crates/
│   ├── ferric_common/          (Span, NodeId, Symbol, DefId, all stage I/O types)
│   ├── ferric_lexer/           (lex)
│   ├── ferric_parser/          (parse)
│   ├── ferric_resolve/         (resolve)
│   ├── ferric_typecheck/       (typecheck — M1 simple checker, replaced in M3)
│   ├── ferric_compiler/        (compile — added in M3)
│   ├── ferric_vm/              (Executor trait + VM impl — TreeWalker in M1, BytecodeVM from M3)
│   ├── ferric_diagnostics/     (Renderer — one-liner in M1, replaced in M2, replaced again in M6)
│   └── ferric_stdlib/          (NativeRegistry + built-in functions)
└── src/
    └── main.rs                 (CLI — wires stages together, nothing else)
```

`main.rs` is the only place that knows about all stages. It imports each stage's
single public function and calls them in order. When a stage is replaced, `main.rs`
may change an import — that is the **total blast radius**.

---

## Stage I/O Contract Reference

These signatures are fixed for the lifetime of the project.
The internals of each stage are free to be completely replaced at any milestone.

```rust
// ferric_lexer
pub fn lex(source: &str, interner: &mut Interner) -> LexResult;

// ferric_parser
pub fn parse(lex: &LexResult) -> ParseResult;

// ferric_resolve
pub fn resolve(ast: &ParseResult) -> ResolveResult;

// ferric_typecheck (M1–M2) / ferric_infer (M3+)
// Signature identical — only the crate name and internals change
pub fn typecheck(ast: &ParseResult, resolve: &ResolveResult) -> TypeResult;

// ferric_compiler (M3+)
pub fn compile(ast: &ParseResult, resolve: &ResolveResult, types: &TypeResult) -> Program;

// ferric_vm — always accessed through Executor, never directly
pub trait Executor {
    fn run(&mut self, program: Program, natives: NativeRegistry) -> Result<Value, RuntimeError>;
}
```

---

## Common Types (ferric_common)

Defined once. Never redefined inside a stage. All stages import from here.

```rust
pub struct Span    { pub start: u32, pub end: u32 }
pub struct NodeId  (pub u32);
pub struct Symbol  (pub u32);
pub struct DefId   (pub u32);

pub struct Interner {
    map: HashMap<String, Symbol>,
    strings: Vec<String>,
}

// Stage output types — shapes are fixed, contents grow across milestones

pub struct LexResult {
    pub tokens: Vec<Token>,
    pub errors: Vec<LexError>,    // every LexError has a Span — Rule 5
}

pub struct ParseResult {
    pub items:  Vec<Item>,
    pub errors: Vec<ParseError>,  // every ParseError has a Span — Rule 5
}

pub struct ResolveResult {
    pub resolutions: HashMap<NodeId, DefId>,
    pub def_slots:   HashMap<DefId, u32>,
    pub fn_slots:    HashMap<DefId, u32>,
    pub errors:      Vec<ResolveError>,   // every ResolveError has a Span — Rule 5
}

pub struct TypeResult {
    pub node_types: HashMap<NodeId, Ty>,
    pub errors:     Vec<TypeError>,       // every TypeError has a Span — Rule 5
}

pub struct Program {
    pub chunks: Vec<Chunk>,
    pub entry:  u16,
}
```

---

## Milestone 1 — Hello World

**Goal:** Every pipeline stage exists and is wired together end-to-end.
Implementations are deliberately thin — correctness over features.
The architecture rules are established here and must hold for all future milestones.

### What must run

```rust
fn greet(name: Str) -> Str {
    "Hello, " + name
}

let message = greet("world")
println(message)
```

### Stage implementations

**Lexer** — lex string literals, integer literals, booleans, identifiers, keywords
(`let`, `fn`, `return`, `if`, `else`, `true`, `false`), basic operators, and punctuation.
Single-line comments skipped. Accumulate errors, never panic.

**Parser** — parse `let` bindings, function definitions (no generics), function calls,
`if`/`else` expressions, binary and unary expressions, return statements, blocks.
Type annotations are `Named(Symbol)` only: `Int`, `Str`, `Bool`, `Unit`.

**Name Resolution** — scope stack, catch undefined variables and duplicate definitions,
assign slot indices to locals.

**Type Checker** — simple recursive checker. Introduce `Ty::Unknown` as an explicit
escape hatch: any expression the checker doesn't yet understand resolves to `Ty::Unknown`,
which is accepted everywhere without error. This is **intentional technical debt** —
it exists so M1 can ship without a complete type system. `Ty::Unknown` is removed in M3.

```rust
pub enum Ty {
    Int, Float, Bool, Str, Unit,
    Fn { params: Vec<Ty>, ret: Box<Ty> },
    Unknown,    // escape hatch — removed entirely in M3
}
```

**VM** — implement as a **tree-walker** over the AST. No bytecode yet. The tree-walker
implements `Executor`. M3 replaces it wholesale with a bytecode VM, also implementing
`Executor` — the CLI does not change.

```rust
pub struct TreeWalker { env_stack: Vec<HashMap<DefId, Value>> }
impl Executor for TreeWalker { ... }
```

**Diagnostics** — bare minimum: `"error at line N: message"`. A `Renderer` struct
exists but only formats line numbers. M2 replaces its internals completely.
Stages already emit `Span` on all errors (Rule 5), so this replacement requires
zero changes to any stage.

**Stdlib** — register exactly: `println(s: Str)`, `print(s: Str)`, `int_to_str(n: Int) -> Str`.

**CLI** — `ferric <file>` runs all stages in sequence. Any errors: print and exit 1.

### Done when

- [ ] The target program runs and prints correctly
- [ ] Undefined variable produces an error with a line number
- [ ] Wrong argument count produces an error with a line number
- [ ] All stages wired in `main.rs` through their public entry functions only
- [ ] Each stage is a separate crate with exactly one public function
- [ ] `Executor` trait exists in `ferric_vm` and `TreeWalker` implements it
- [ ] All error types across all stages carry `Span`

---

## Milestone 2 — Real Programs

**Goal:** Write non-trivial programs. Adds control flow, recursion, loops, mutable
variables, and human-readable error messages. No stage replacements yet —
only additions within each stage — except diagnostics, which is fully replaced.

### What must run

```rust
fn fibonacci(n: Int) -> Int {
    if n <= 1 { n } else { fibonacci(n - 1) + fibonacci(n - 2) }
}

println(fibonacci(10))

let mut counter = 0
while counter < 5 {
    println(counter)
    counter = counter + 1
}
```

### Additions by stage

**Lexer** — add `Mut`, `While`, `Loop`, `Break`, `Continue`, `FloatLit(f64)`,
`AndAnd`, `OrOr`, `LtEq`, `GtEq`.

**Parser** — add `while`, `loop`, `break`, `continue`, assignment expressions,
float literals, `let mut`.

**Name Resolution** — add `AssignToImmutable`, `BreakOutsideLoop`, `ReturnOutsideFn`
errors. Track mutability on local variable definitions. Track loop depth on scope stack.

**Type Checker** — add `Float`. Check that `if`/`else` branches have matching types.
Check that `while` condition is `Bool`. Check assignment value type matches binding type.

**VM (TreeWalker)** — add evaluation of `while`, `loop`, `break`, `continue`,
assignment, and float values. No structural changes — additions only.

**Stdlib** — add `float_to_str`, `bool_to_str`, `int_to_float`.

### Stage replacement: Diagnostics

Replace the bare line-number renderer with a span-annotated renderer.
The `Renderer` struct signature does not change. All stages already carry `Span`
on every error (Rule 5 from M1), so this replacement requires **zero changes to
any other stage**. This is the architecture rules paying off for the first time.

```
// Before (M1)
error at line 5: undefined variable `x`

// After (M2)
error: undefined variable `x`
  --> main.fe:5:10
   |
 5 |     let y = x + 1
   |             ^ not found in this scope
```

### Done when

- [ ] Fibonacci runs correctly
- [ ] `while` loop with mutable counter works
- [ ] `if` as an expression with a value works
- [ ] Span-annotated errors render correctly for all existing error kinds
- [ ] All errors from all stages route through `Renderer` — no raw `eprintln!` anywhere in stage code
- [ ] Diagnostics replacement required **zero changes** to lexer, parser, resolve, or typecheck

---

## Milestone 3 — Type Safety + Bytecode VM

**Goal:** Close the two biggest quality gaps: eliminate `Ty::Unknown` with real type
inference, and replace the tree-walker with a fast bytecode VM. This milestone
contains **two full stage replacements**. Each replacement is independent — complete
one, verify tests pass, then do the other.

### Replacement A — Type checker → HM inference engine

Replace `ferric_typecheck` with `ferric_infer`. The new crate implements the
**identical public signature**:

```rust
// Unchanged. Internals completely replaced.
pub fn typecheck(ast: &ParseResult, resolve: &ResolveResult) -> TypeResult;
```

The new implementation uses Hindley-Milner type inference (Algorithm J with constraint
accumulation). `Ty::Unknown` is **removed** — the engine must resolve every expression
to a concrete type or emit `TypeError::CannotInfer`.

What the replacement adds:
- Every expression has a fully resolved, concrete `Ty` — no escape hatches
- Generic functions are supported via `TypeScheme` (∀-quantified types)
- Type annotations on `let` and fn params are enforced, not just recorded
- New error kinds: `TypeError::InfiniteType`, `TypeError::CannotInfer`

`main.rs` changes: **one import swap**. No other file changes.

### Replacement B — TreeWalker → Bytecode VM

Replace `TreeWalker` with `BytecodeVM`, both implementing `Executor`.
Add `ferric_compiler` as a new crate with its own public entry point.

```rust
// ferric_compiler — new crate, new stage
pub fn compile(ast: &ParseResult, resolve: &ResolveResult, types: &TypeResult) -> Program;

// ferric_vm — BytecodeVM replaces TreeWalker, same Executor trait
pub struct BytecodeVM { stack: Vec<Value>, call_stack: Vec<Frame>, natives: NativeRegistry }
impl Executor for BytecodeVM { ... }
```

`main.rs` changes: one import swap + one new `compile(...)` call. No other file changes.

Instruction set covering all features built so far:

```rust
pub enum Op {
    // Stack
    LoadConst(u8), LoadSlot(u8), StoreSlot(u8), Pop, Dup,

    // Arithmetic
    AddInt, SubInt, MulInt, DivInt, RemInt,
    AddFloat, SubFloat, MulFloat, DivFloat,
    NegInt, NegFloat,

    // Comparison + logic
    EqInt, LtInt, GtInt, EqFloat, LtFloat, GtFloat,
    EqBool, EqStr, Not, AndBool, OrBool,

    // String
    Concat,

    // Control flow
    Jump(i16), JumpIfFalse(i16), JumpIfTrue(i16), Return,

    // Calls
    Call(u8), CallNative(u8), TailCall(u8),

    // Data
    MakeTuple(u8), MakeClosure(u16, u8), Unit,
}
```

### Done when

- [ ] All M1 + M2 programs still run identically after both replacements
- [ ] `Ty::Unknown` is gone — the type checker rejects all ambiguous programs
- [ ] Generic functions (`fn identity<T>(x: T) -> T`) infer correctly
- [ ] Type mismatch errors include both expected and found types with spans
- [ ] Bytecode VM passes all existing tests
- [ ] Replacing the type checker required changing **only** `main.rs` (one import)
- [ ] Replacing the VM required changing **only** `main.rs` (one import + one call)

---

## Milestone 4 — Algebraic Data Types

**Goal:** Add structs, enums, and pattern matching. This milestone adds to the parser,
resolver, type checker, compiler, and VM — but **replaces nothing**. It also introduces
exhaustiveness checking as a new pipeline stage.

### What must run

```rust
enum Shape {
    Circle(Float),
    Rectangle(Float, Float),
}

fn area(s: Shape) -> Float {
    match s {
        Shape::Circle(r)       => 3.14159 * r * r,
        Shape::Rectangle(w, h) => w * h,
    }
}

struct Point { x: Float, y: Float }
let p = Point { x: 1.0, y: 2.0 }
println(p.x)
```

### New stage: ferric_exhaust

Exhaustiveness checking sits between typecheck and compile. New crate, new public function.

```rust
// ferric_exhaust — new crate
pub fn check_exhaustiveness(ast: &ParseResult, types: &TypeResult) -> ExhaustivenessResult;

pub struct ExhaustivenessResult { pub errors: Vec<ExhaustivenessError> }

pub enum ExhaustivenessError {
    NonExhaustive { missing: Vec<String>, span: Span },
    UnreachableArm { span: Span },
}
```

`main.rs` adds one line. All other stages untouched.

### Additions to common types

```rust
// Add to Ty in ferric_common:
Tuple(Vec<Ty>),
Struct { def_id: DefId, fields: Vec<(Symbol, Ty)> },
Enum   { def_id: DefId, variants: Vec<(Symbol, Vec<Ty>)> },
```

### Additions by stage

**Parser** — add `struct` and `enum` item definitions, struct literal expressions,
field access (`p.x`), `match` expressions, patterns (wildcard, variable binding,
enum variant, struct, tuple, literal).

**Name Resolution** — add struct and enum definitions to the def table. Resolve
field names to field indices. Resolve variant paths. Emit errors for unknown fields
and unknown variants.

**Type Checker** — type-check struct literals (all fields present, correct types),
field access, and match arms (scrutinee type, arm pattern types, arm body types agree).

**Compiler** — add `MakeStruct`, `MakeVariant`, `GetField`, `MatchVariant`,
`UnpackVariant` instructions.

**VM** — add `Value::Struct` and `Value::Variant`. Handle new instructions.

### Done when

- [ ] Enum definition + exhaustive match compiles and runs correctly
- [ ] Non-exhaustive match produces an error naming the missing variant
- [ ] Struct definition + construction + field access works
- [ ] Pattern matching on tuples, literals, and wildcards works
- [ ] Unreachable arms produce a warning
- [ ] New `ferric_exhaust` stage slotted into `main.rs` with **zero changes** to other stages

---

## Milestone 5 — Traits and Generics

**Goal:** User-defined traits and generic functions with trait bounds. This milestone
replaces the type checker again — adding trait constraint solving on top of HM
inference — and introduces a `TraitRegistry` as a new injectable dependency.

### What must run

```rust
trait Describable {
    fn describe(self) -> Str
}

impl Describable for Int {
    fn describe(self) -> Str { "I am an integer: " + int_to_str(self) }
}

fn print_description<T: Describable>(val: T) {
    println(val.describe())
}

print_description(42)
```

### Stage replacement: Type checker + trait solver

Replace `ferric_infer` (from M3) with a new version that also solves trait constraints.
**Public signature is unchanged:**

```rust
// Unchanged. Internals extended to handle trait constraints.
pub fn typecheck(ast: &ParseResult, resolve: &ResolveResult) -> TypeResult;
```

The `TraitRegistry` is a new type in `ferric_common`. It is built by a new
`ferric_traits` crate that runs before typechecking and is passed as an additional
argument — requiring a **one-line signature extension**:

```rust
// Updated signature — only change to the stage contract in this milestone
pub fn typecheck(
    ast: &ParseResult,
    resolve: &ResolveResult,
    registry: &TraitRegistry,   // new
) -> TypeResult;
```

`main.rs` changes: one import swap + one new `build_registry(...)` call +
one new argument passed to `typecheck`. All other stages untouched.

### Done when

- [ ] User-defined traits compile and type-check
- [ ] Generic functions with trait bounds work
- [ ] Calling a method on a type that doesn't implement the trait produces a clear error
- [ ] Built-in trait impls registered at startup (Display for Int, Float, Str, Bool)
- [ ] Replacing the type checker required changing **only** `main.rs`
- [ ] All M1–M4 programs still pass

---

## Milestone 6 — Production Quality

**Goal:** Everything an end-user needs. Closures, arrays, a full stdlib,
`Option`/`Result`, and production-quality multi-span error messages.
No stage replacements — additions only, plus one final diagnostics replacement.

### What must run

```rust
let nums = [1, 2, 3, 4, 5]
let doubled = nums.map(|x| x * 2)

fn safe_divide(a: Int, b: Int) -> Result<Int, Str> {
    if b == 0 { Err("division by zero") } else { Ok(a / b) }
}

match safe_divide(10, 0) {
    Ok(n)  => println(n),
    Err(e) => println("Error: " + e),
}
```

### Additions

**Closures** — capture analysis in the resolver, full closure values in the VM.
`MakeClosure` instruction already exists from M3.

**Arrays** — `[T]` as a built-in generic type. Array literal syntax, indexing,
and `for` loop desugaring to `Iterator::next()`.

**`Option<T>` and `Result<T, E>`** — built-in enums registered in the trait registry
at startup. Pattern-match on them like any other enum.

**Stdlib expansion**:
- `array`: `len`, `push`, `pop`, `map`, `filter`, `fold`, `contains`, `sort`
- `str`: `len`, `split`, `trim`, `contains`, `starts_with`, `parse_int`
- `math`: `abs`, `sqrt`, `pow`, `min`, `max`, `floor`, `ceil`
- `io`: `read_line`

**REPL** — `ferric` with no file argument starts a REPL with persistent env across inputs.

### Stage replacement: Diagnostics (second time)

Replace the span renderer (M2) with a full multi-label renderer supporting primary
span + secondary spans + `note:` / `help:` suggestions. Again — all stages already
carry `Span` on all errors (Rule 5), so this requires **zero changes** to any stage.

```
error[E003]: type mismatch
  --> main.fe:8:18
   |
 6 |     let x: Int = "hello"
   |             --- expected `Int` because of this annotation
 8 |     x + 1.0
   |         ^^^ found `Float`
   |
   = help: remove the type annotation to let the type be inferred
```

### Done when

- [ ] Closures capture variables correctly
- [ ] `for` loop over array works
- [ ] `Option` and `Result` pattern-match correctly
- [ ] Full stdlib available
- [ ] Multi-label error rendering works for all existing error types
- [ ] Diagnostics replacement required **zero changes** to any stage
- [ ] REPL starts and maintains state across inputs
- [ ] All M1–M5 programs still pass

---

## Replacement Log (living document)

Track every stage replacement here as it happens. Each entry confirms the blast
radius was limited to `main.rs` and the test suite still passes.

| Milestone | Stage replaced                   | New implementation       | main.rs delta              |
|-----------|----------------------------------|--------------------------|----------------------------|
| M2        | `ferric_diagnostics`             | Span-annotated renderer  | 0 — renderer injected, not imported by stages |
| M3        | `ferric_typecheck` → `ferric_infer` | HM inference engine   | 1 import swap              |
| M3        | `TreeWalker` → `BytecodeVM`      | Bytecode interpreter     | 1 import swap + 1 new call |
| M5        | `ferric_infer` → trait solver    | HM + trait constraints   | 1 import + 1 new argument  |
| M6        | `ferric_diagnostics`             | Multi-label renderer     | 0                          |

---

## Future Levers (post-M6)

These are possible without breaking any stage boundary. Documented so the initial
architecture does not accidentally preclude them.

**NaN-boxing** — swap `Value`'s internal representation. Because all construction
goes through `Value::new_*()` (Rule 7), this is a single-file change inside `ferric_vm`
with zero blast radius to any other crate.

**Cranelift/LLVM JIT** — add a third `Executor` implementor. `main.rs` picks between
`BytecodeVM` and `JitExecutor` via a flag. All other stages untouched.

**Slot reuse** — liveness analysis in name resolution allows non-overlapping scopes
to share slots. Internal to `ferric_resolve`, invisible to all other stages.

**Inline caches** — method call sites cache their last-seen type tag and resolved
function pointer. Internal to `ferric_vm`, invisible to all other stages.