# Ferric Language Reference

Ferric is a statically-typed, expression-oriented scripting language with first-class shell integration, algebraic effects, async/await (via effects desugaring), generators, and a rich standard library. This document describes the complete language as implemented.

---

## Table of Contents

1. [Syntax Overview](#syntax-overview)
2. [Types](#types)
3. [Variables](#variables)
4. [Operators](#operators)
5. [Control Flow](#control-flow)
6. [Functions](#functions)
7. [Closures](#closures)
8. [Custom Types](#custom-types)
9. [Pattern Matching](#pattern-matching)
10. [Traits](#traits)
11. [Shell Integration](#shell-integration)
12. [Require Assertions](#require-assertions)
13. [Algebraic Effects](#algebraic-effects)
14. [Async/Await](#asyncawait)
15. [Generators](#generators)
16. [Modules](#modules)
17. [Type Aliases](#type-aliases)
18. [Standard Library](#standard-library)

---

## Syntax Overview

Ferric programs are sequences of top-level items (function/struct/enum/trait/impl definitions) and script statements (let bindings and expression statements). Items and script statements may be freely interleaved.

```ferric
fn greet(name: Str) -> Str {
    "Hello, " + name
}

let message = greet(name: "world")
println(s: message)
```

**Comments**: single-line only, `//` to end of line.

**Blocks**: `{ stmt* expr? }` — zero or more statements (each followed by a newline or `;`), optionally ending with a bare expression that is the block's value.

**Expression vs statement**: in Ferric almost everything is an expression. `if`, `while`, `loop`, `match`, and blocks all produce values. A trailing semicolon discards the value and makes the containing block return `Unit`.

---

## Types

### Primitive types

| Type    | Description                    | Literals              |
|---------|--------------------------------|-----------------------|
| `Int`   | 64-bit signed integer          | `0`, `42`, `-5`       |
| `Float` | 64-bit floating-point          | `1.5`, `2.0`, `-3.14` |
| `Bool`  | Boolean                        | `true`, `false`       |
| `Str`   | UTF-8 string                   | `"hello"`             |
| `Unit`  | Empty/void value               | `()`                  |

### Compound types

| Syntax           | Description                            |
|------------------|----------------------------------------|
| `[T]`            | Array of `T` (fixed at creation)       |
| `(T, U)`         | Tuple of types `T` and `U`             |
| `Option<T>`      | Built-in: `Option::Some(T)` or `Option::None` |
| `Result<T, E>`   | Built-in: `Result::Ok(T)` or `Result::Err(E)` |
| `Eff<[E...], T>` | A computation that may perform effects `E...` and returns `T` |
| `Async<T>`       | Sugar for `Eff<[Async], T>` — a deferred async computation |
| `Handle<T>`      | A spawned task handle                  |
| `ShellOutput`    | Result of a `$` shell expression       |
| `Json`           | JSON value (stdlib opaque type)        |

User-defined structs and enums are named types.

### Type inference

Types are inferred using Hindley-Milner inference. Explicit annotations are optional but allowed:

```ferric
let x: Int = 5
let y = 10         // inferred as Int
```

---

## Variables

```ferric
let x = 5              // immutable binding
let mut y = 10         // mutable — can be reassigned
let z: Int = 42        // explicit type annotation
```

Reassignment requires `let mut`:

```ferric
let mut counter = 0
counter = counter + 1
```

Bindings are lexically scoped. `let` in a nested block shadows the outer binding within that block.

---

## Operators

### Arithmetic

`+`, `-`, `*`, `/`, `%` — work on `Int` and `Float`. `+` also concatenates `Str`.

### Comparison

`==`, `!=`, `<`, `<=`, `>`, `>=` — return `Bool`.

### Logical

`&&`, `||` — short-circuit. `!` — logical not (unary).

### Unary

`-expr` — numeric negation. `!expr` — logical negation.

### Precedence (highest to lowest)

| Level | Operators              |
|-------|------------------------|
| 6     | `*`, `/`, `%`          |
| 5     | `+`, `-`               |
| 4     | `<`, `<=`, `>`, `>=`  |
| 3     | `==`, `!=`             |
| 2     | `&&`                   |
| 1     | `\|\|`                 |

Use parentheses to override precedence.

---

## Control Flow

### If / else

`if` is an expression — both branches must have the same type.

```ferric
let abs = if x < 0 { -x } else { x }

if condition {
    do_something()
} else if other {
    do_other()
} else {
    fallback()
}
```

The `else` branch is optional when the `if` is used as a statement (result discarded).

### While

```ferric
let mut i = 0
while i < 5 {
    println(s: int_to_str(n: i))
    i = i + 1
}
```

### Loop (infinite)

```ferric
loop {
    if done { break }
    do_work()
}
```

### For

Iterates over arrays and lists.

```ferric
for x in [1, 2, 3, 4, 5] {
    println(s: int_to_str(n: x))
}

let names = ["alpha", "beta", "gamma"]
for name in names {
    println(s: name)
}
```

### Break and continue

`break` exits the nearest enclosing `while` or `loop`.
`continue` jumps to the next iteration.

### Return

```ferric
return expr    // return a value from the enclosing function
return         // return Unit
```

---

## Functions

### Definition

```ferric
fn name(param: Type, other: Type) -> ReturnType {
    body_expression
}
```

The body is a block; its final expression is the implicit return value.

### Named arguments

**All call sites must use named arguments.** The resolver canonicalises argument order to the definition order, so arguments may appear in any order at the call site.

```ferric
fn add(a: Int, b: Int) -> Int { a + b }

add(a: 3, b: 4)    // definition order
add(b: 10, a: 5)   // out of order — same result
```

### Default parameters

```ferric
fn greet(name: Str, greeting: Str = "Hello") -> Str {
    greeting + ", " + name + "!"
}

greet(name: "world")                     // uses default
greet(name: "Ferric", greeting: "Hey")   // overrides default
```

### Generic functions

```ferric
fn identity<T>(x: T) -> T { x }

fn print_description<T: Describable>(val: T) {
    println(s: val.describe())
}
```

Type parameters can carry trait bounds: `T: TraitName`. Multiple bounds are not yet supported — use a single bound.

### Recursive functions

```ferric
fn fibonacci(n: Int) -> Int {
    if n <= 1 {
        n
    } else {
        fibonacci(n: n - 1) + fibonacci(n: n - 2)
    }
}
```

---

## Closures

```ferric
let double = |x| x * 2
let add = |x, y| x + y

// Explicit param type
let typed = |x: Int| x + 1

// Capture from enclosing scope
let bonus = 100
let add_bonus = |n| n + bonus

// Nested closures
let make_adder = |inc| |x| x + inc
let add10 = make_adder(inc: 10)
println(s: int_to_str(n: add10(x: 32)))
```

Closures capture variables from their enclosing scope. They can be passed to functions and called with named arguments (the closure's parameter names come from its definition).

---

## Custom Types

### Structs

```ferric
struct Point { x: Int, y: Int }

let p = Point { x: 3, y: 4 }
println(s: int_to_str(n: p.x + p.y))
```

Field access uses `.`. Struct literals use `Name { field: value, ... }`.

### Enums

```ferric
enum Shape {
    Circle(Int),
    Square(Int),
    Rectangle(Int, Int),
}

let s = Shape::Circle(5)
let sq = Shape::Square(4)
let r = Shape::Rectangle(3, 5)
```

Variant constructors: `EnumName::VariantName(arg, ...)`. Unit variants (no payload) omit the parens: `Light::Red`.

### Tuples

```ferric
let triple = (10, 20, 30)
```

Tuples are destructured via `match`:

```ferric
match triple {
    (a, b, c) => println(s: int_to_str(n: a + b + c)),
}
```

Tuple field access by index (`.0`, `.1`, etc.) is not yet supported — use `match` to destructure.

---

## Pattern Matching

```ferric
match expr {
    Pattern => body,
    ...
}
```

Patterns:

| Pattern                        | Matches                                 |
|-------------------------------|-----------------------------------------|
| `_`                           | Any value, no binding                   |
| `name`                        | Any value, binds to `name`              |
| `42`, `"x"`, `true`          | Exact literal                           |
| `(a, b, c)`                   | Tuple, binds fields                     |
| `Point { x, y }`             | Struct, binds fields                    |
| `Shape::Circle(r)`           | Enum variant, binds payload             |
| `Option::Some(v)`            | Built-in enum variant                   |
| `Option::None`               | Unit variant                            |

Match is **exhaustive** — the compiler rejects non-exhaustive patterns. Unreachable arms emit a warning.

```ferric
fn area(s: Shape) -> Int {
    match s {
        Shape::Circle(r)        => r * r * 3,
        Shape::Square(side)     => side * side,
        Shape::Rectangle(w, h)  => w * h,
    }
}
```

---

## Traits

```ferric
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

print_description(val: 42)
```

Trait method calls use receiver syntax: `value.method(args)`. The compiler desugars `val.describe()` to a call to the `impl` block's method.

---

## Shell Integration

The `$` prefix runs a shell command via `/bin/sh -c` and produces a `ShellOutput` value.

```ferric
let result = $ echo hello world
println(s: shell_stdout(output: result))
println(s: int_to_str(n: shell_exit_code(output: result)))
```

### Interpolation

`@{expr}` embeds a Ferric expression into the shell command. The expression must be `Int` or `Str`.

```ferric
let name = "world"
let out = $ echo Hello, @{name}!

let a = 3
let b = 4
let r = $ echo @{a + b}
```

### Multiline commands

Backslash-newline continues to the next line:

```ferric
let log = $ echo first \
              second \
              third
```

### Discarding output

```ferric
$ echo discarded > /dev/null
```

### Shell result accessors

| Function                               | Returns                     |
|----------------------------------------|-----------------------------|
| `shell_stdout(output: ShellOutput)`    | `Str` — captured stdout     |
| `shell_exit_code(output: ShellOutput)` | `Int` — process exit code   |

### Async shell

Inside async context, use the `shell_run_async` intrinsic to avoid blocking:

```ferric
async fn fetch_output() -> ShellOutput {
    await shell_run_async(cmd: "curl https://example.com")
}
```

---

## Require Assertions

`require` asserts a condition at runtime. A failed require (in default mode) halts the program with the given message and a diagnostic.

```ferric
require x > 0                             // fails if false
require x > 0, "x must be positive"      // with message
```

### Warn mode

`require(warn)` emits a warning and continues instead of halting.

```ferric
require(warn) x > 0, "x should be positive"
```

### Set (recovery) variant

`set:` provides a closure to establish the condition, which is run once and then rechecked. In warn mode, if the condition is still false after the set block, a warning is emitted and execution continues.

```ferric
let mut x = -1
require x > 0, "x must be positive", set: || { x = 5 }

let mut y = -1
require(warn) y > 0, "y should be positive", set: || { y = -2 }
```

---

## Algebraic Effects

Ferric has a built-in algebraic effects system. Effects are first-class, typed, and support one-shot resumable continuations.

### Effect declarations

An `effect` block declares a named effect and its operations:

```ferric
effect Config {
    Get(key: Str) -> Str,
    Set(key: Str, val: Str) -> Unit,
}

effect Log {
    Write(msg: Str) -> Unit,
}
```

Each operation signature is `OpName(params...) -> ResumeType`. The resume type is the value that `resume` supplies back into the `perform` expression.

### Perform

`perform EffectName::OpName(args...)` suspends the current computation and dispatches to the nearest enclosing handler for that effect:

```ferric
fn read_config(key: Str) -> Str {
    perform Config::Get(key: key)
}
```

The type of a `perform` expression is the operation's resume type.

### Handle / with

`handle { body } with { clauses... }` installs a handler for one or more effect operations:

```ferric
let value = handle {
    read_config(key: "db.host")
} with {
    Config::Get(key) => resume k with lookup(key: key),
    Config::Set(key, val) => {
        store(key: key, val: val)
        resume k with ()
    },
}
```

Each clause matches an operation by name and binds its arguments. `k` is the captured continuation.

### Resume

`resume k with value` resumes the continuation `k` with the given value. The result is the value the `handle` block produces.

Continuations are **one-shot**: resuming the same continuation twice is a runtime error (`RuntimeError::ResumedTwice`).

### Effect-row types

Functions that perform effects carry them in their return type:

```ferric
fn fetch_host() -> Eff<[Config], Str> {
    perform Config::Get(key: "db.host")
}
```

The effect row `[Config]` is inferred automatically — explicit annotation is optional. A function with an empty row `Eff<[], T>` is equivalent to a plain `T` return.

### Full dependency-injection example

```ferric
effect Config {
    Get(key: Str) -> Str,
}

fn app() -> Str {
    let host = perform Config::Get(key: "db.host")
    let port = perform Config::Get(key: "db.port")
    host + ":" + port
}

// Inject a test config
let conn = handle {
    app()
} with {
    Config::Get(key) =>
        if key == "db.host" { resume k with "localhost" }
        else { resume k with "5432" },
}

println(s: conn)   // localhost:5432
```

---

## Async/Await

Async/await is syntactic sugar over the algebraic effects system. `async fn` desugars to a function returning `Eff<[Async], T>`, and `.await` desugars to `perform Async::Suspend`. You never write the effect mechanics by hand — the desugaring is transparent.

### Async functions

`async fn` returns `Async<T>` (i.e. `Eff<[Async], T>`) instead of `T`. The body is not executed until the value is awaited or spawned.

```ferric
async fn fetch(url: Str) -> Str { url }
```

### Await

`.await` (postfix) inside an `async fn` or `async { }` block extracts the `T` from `Async<T>`.

```ferric
async fn pipeline() -> Str {
    let a = await step1()
    let b = await step2(x: a)
    int_to_str(n: b)
}
```

`await` is only legal inside `async fn` bodies or `async { ... }` blocks.

### Async blocks

`async { expr }` lifts a synchronous expression into an `Async<T>` value.

```ferric
let task: Async<Str> = async { await fetch(url: "hello") }
```

### Block_on

Runs an `Async<T>` synchronously from non-async code (e.g., top-level scripts):

```ferric
let result: Str = block_on(task: task)
```

### Spawn and join

`spawn` submits an `Async<T>` to the thread-pool scheduler and returns `Handle<T>`. `join` waits for two handles concurrently and returns a tuple.

```ferric
let h1 = spawn(task: slow_task(n: 1))
let h2 = spawn(task: slow_task(n: 2))

let pair = join(a: h1, b: h2)
match pair {
    (a, b) => println(s: int_to_str(n: a + b))
}
```

### Sleep

```ferric
async fn delayed() -> Int {
    await sleep(ms: 500)
    42
}
```

### Async intrinsics summary

| Function                                          | Signature                              |
|---------------------------------------------------|----------------------------------------|
| `spawn(task: Async<T>) -> Handle<T>`             | Submit task to scheduler               |
| `join(a: Handle<A>, b: Handle<B>) -> (A, B)`     | Wait for two handles                   |
| `sleep(ms: Int) -> Async<Unit>`                  | Non-blocking delay                     |
| `block_on(task: Async<T>) -> T`                  | Drive async from sync context          |
| `shell_run_async(cmd: Str) -> Async<ShellOutput>`| Non-blocking shell execution           |

---

## Generators

Generators are syntactic sugar over the effects system. A `gen fn` desugars to a function returning `Eff<[Gen<T>], Unit>`, and `yield` desugars to `perform Gen::Yield`. Like async/await, the desugaring is fully transparent.

### Generator functions

```ferric
gen fn integers_from(start: Int) {
    let mut n = start
    loop {
        yield n
        n = n + 1
    }
}
```

A `gen fn` implicitly returns `Unit`. The `yield` expression suspends the generator and sends a value to the consumer.

### Consuming a generator

Generators are consumed via `handle ... with` or through stdlib helpers:

```ferric
let gen = integers_from(start: 1)

let first_five = handle {
    gen
} with {
    Gen::Yield(val) => {
        collect(val: val)
        resume k with ()
    },
}
```

---

## Modules

### Export

Mark top-level items as importable from other files:

```ferric
export fn double(n: Int) -> Int { n * 2 }
export struct Point { x: Int, y: Int }
```

Unmarked items are file-private.

### Import

```ferric
import { double, Point } from "./util"
import { connect as db_connect } from "./db"
```

Import paths:

| Form              | Meaning                                 |
|-------------------|-----------------------------------------|
| `"./name"`        | File-relative                           |
| `"../name"`       | Parent-directory-relative               |
| `"@/name"`        | Workspace-root-relative (needs manifest)|
| `"ferric-http"`   | Cache dependency (needs manifest)       |

### Workspace manifest

`Ferric.toml` in the workspace root enables `@/` imports and named dependencies. In its absence, the interpreter runs in script mode using file-relative imports only.

---

## Type Aliases

```ferric
type Url = Str
type UserId = Int
```

Aliases are **opaque**: `Url` and `Str` are distinct types. Use `as` to cast between them:

```ferric
let raw: Str = "https://example.com"
let url: Url = raw as Url
let back: Str = url as Str
```

Casts are compile-time-only — no runtime cost.

---

## Standard Library

All stdlib functions use named arguments. Functions are globally bound (no module prefix needed at the call site unless the prefix is part of the name, e.g. `str_len`, `list_map`).

### Core I/O

| Function                              | Signature                      |
|---------------------------------------|--------------------------------|
| `println(s: Str)`                     | Print with newline to stdout   |
| `print(s: Str)`                       | Print without newline          |
| `eprintln(s: Str)`                    | Print with newline to stderr   |
| `eprint(s: Str)`                      | Print without newline to stderr|
| `read_line() -> Str`                  | Read one line from stdin       |

### Type conversion

| Function                               | Signature                        |
|----------------------------------------|----------------------------------|
| `int_to_str(n: Int) -> Str`            | Integer to decimal string        |
| `float_to_str(n: Float) -> Str`        | Float to string                  |
| `bool_to_str(b: Bool) -> Str`          | Bool to `"true"` / `"false"`     |
| `int_to_float(n: Int) -> Float`        | Widen to float                   |
| `abs(n: Int) -> Int`                   | Absolute value                   |
| `min(a: Int, b: Int) -> Int`           | Minimum of two ints              |
| `max(a: Int, b: Int) -> Int`           | Maximum of two ints              |
| `sqrt(n: Float) -> Float`              | Square root                      |
| `array_len(arr: [T]) -> Int`           | Array length                     |

### Strings (`str_*`)

| Function                                              | Signature                               |
|-------------------------------------------------------|-----------------------------------------|
| `str_len(s: Str) -> Int`                              | Length in bytes                         |
| `str_contains(s: Str, sub: Str) -> Bool`              | Substring test                          |
| `str_starts_with(s: Str, prefix: Str) -> Bool`        |                                         |
| `str_ends_with(s: Str, suffix: Str) -> Bool`          |                                         |
| `str_find(s: Str, needle: Str) -> Option<Int>`        | First match byte offset                 |
| `str_find_all(s: Str, needle: Str) -> [Int]`          | All match offsets                       |
| `str_split(s: Str, sep: Str) -> [Str]`                | Split on separator                      |
| `str_split_once(s: Str, sep: Str) -> Option<(Str,Str)>` | Split at first occurrence             |
| `str_lines(s: Str) -> [Str]`                          | Split on newlines                       |
| `str_chars(s: Str) -> [Str]`                          | Unicode characters                      |
| `str_trim(s: Str) -> Str`                             | Strip leading/trailing whitespace       |
| `str_trim_start(s: Str) -> Str`                       |                                         |
| `str_trim_end(s: Str) -> Str`                         |                                         |
| `str_trim_chars(s: Str, chars: Str) -> Str`           | Strip specific characters               |
| `str_to_upper(s: Str) -> Str`                         |                                         |
| `str_to_lower(s: Str) -> Str`                         |                                         |
| `str_to_title(s: Str) -> Str`                         |                                         |
| `str_replace(s: Str, from: Str, to: Str) -> Str`     | Replace all occurrences                 |
| `str_replace_first(s: Str, from: Str, to: Str) -> Str` | Replace first                         |
| `str_replace_n(s: Str, from: Str, to: Str, n: Int) -> Str` | Replace first N                  |
| `str_join(parts: [Str], sep: Str) -> Str`             | Join array with separator               |
| `str_repeat(s: Str, n: Int) -> Str`                   | Repeat string N times                   |
| `str_pad_start(s: Str, width: Int, pad: Str) -> Str`  | Left-pad to width                       |
| `str_pad_end(s: Str, width: Int, pad: Str) -> Str`    | Right-pad to width                      |
| `str_center(s: Str, width: Int, pad: Str) -> Str`     | Center within width                     |
| `str_slice(s: Str, from: Int, to: Int) -> Str`        | Substring by byte offsets               |
| `str_parse_int(s: Str) -> Int`                        | Parse integer (panics on invalid)       |
| `str_parse_float(s: Str) -> Float`                    | Parse float                             |
| `str_parse_bool(s: Str) -> Bool`                      | Parse bool                              |
| `str_eq_ignore_case(a: Str, b: Str) -> Bool`          | Case-insensitive equality               |
| `str_is_empty(s: Str) -> Bool`                        |                                         |
| `str_is_ascii(s: Str) -> Bool`                        |                                         |
| `str_is_numeric(s: Str) -> Bool`                      | All digit characters                    |
| `str_to_bytes(s: Str) -> Bytes`                       | UTF-8 bytes                             |
| `str_from_bytes(b: Bytes) -> Str`                     | Decode UTF-8 bytes                      |
| `str_from_bytes_lossy(b: Bytes) -> Str`               | Lossy decode                            |

### Lists (`list_*`)

Lists are dynamic arrays. `[T]` literals create fixed arrays; list functions return list values.

| Function                                                   | Description                              |
|------------------------------------------------------------|------------------------------------------|
| `list_new() -> [T]`                                        | Empty list                               |
| `list_of(items: [T]) -> [T]`                               | List from array literal                  |
| `list_range(from: Int, to: Int) -> [Int]`                  | `[from, to)` exclusive                   |
| `list_range_inclusive(from: Int, to: Int) -> [Int]`        | `[from, to]` inclusive                   |
| `list_repeat(val: T, n: Int) -> [T]`                       | N copies of val                          |
| `list_len(l: [T]) -> Int`                                  | Length                                   |
| `list_is_empty(l: [T]) -> Bool`                            |                                          |
| `list_get(l: [T], index: Int) -> Option<T>`                | Safe indexed access                      |
| `list_first(l: [T]) -> Option<T>`                          |                                          |
| `list_last(l: [T]) -> Option<T>`                           |                                          |
| `list_slice(l: [T], from: Int, to: Int) -> [T]`            | Subsequence                              |
| `list_contains(l: [T], item: T) -> Bool`                   |                                          |
| `list_find(l: [T], f: fn(T)->Bool) -> Option<T>`           |                                          |
| `list_find_index(l: [T], f: fn(T)->Bool) -> Option<Int>`   |                                          |
| `list_index_of(l: [T], item: T) -> Option<Int>`            |                                          |
| `list_map(l: [T], f: fn(T)->U) -> [U]`                     |                                          |
| `list_flat_map(l: [T], f: fn(T)->[U]) -> [U]`              |                                          |
| `list_filter(l: [T], f: fn(T)->Bool) -> [T]`               |                                          |
| `list_filter_map(l: [T], f: fn(T)->Option<U>) -> [U]`      |                                          |
| `list_fold(l: [T], init: U, f: fn(U,T)->U) -> U`           | Left fold                                |
| `list_reduce(l: [T], f: fn(T,T)->T) -> Option<T>`          | Reduce (no initial)                      |
| `list_scan(l: [T], init: U, f: fn(U,T)->U) -> [U]`         | Running fold                             |
| `list_any(l: [T], f: fn(T)->Bool) -> Bool`                 |                                          |
| `list_all(l: [T], f: fn(T)->Bool) -> Bool`                 |                                          |
| `list_count(l: [T], f: fn(T)->Bool) -> Int`                |                                          |
| `list_sum(l: [Int]) -> Int`                                |                                          |
| `list_sum_float(l: [Float]) -> Float`                      |                                          |
| `list_min(l: [Int]) -> Option<Int>`                        |                                          |
| `list_max(l: [Int]) -> Option<Int>`                        |                                          |
| `list_concat(a: [T], b: [T]) -> [T]`                       | Concatenate two lists                    |
| `list_prepend(l: [T], item: T) -> [T]`                     | Add at front                             |
| `list_append(l: [T], item: T) -> [T]`                      | Add at back                              |
| `list_push(l: [T], item: T) -> [T]`                        | Alias for append                         |
| `list_pop(l: [T]) -> Option<([T], T)>`                     | Remove last                              |
| `list_insert(l: [T], index: Int, item: T) -> [T]`          | Insert at index                          |
| `list_remove(l: [T], index: Int) -> [T]`                   | Remove at index                          |
| `list_reverse(l: [T]) -> [T]`                              |                                          |
| `list_unique(l: [T]) -> [T]`                               | Deduplicate                              |
| `list_flatten(l: [[T]]) -> [T]`                            |                                          |
| `list_chunk(l: [T], size: Int) -> [[T]]`                   | Split into chunks                        |
| `list_window(l: [T], size: Int) -> [[T]]`                  | Sliding windows                          |
| `list_take(l: [T], n: Int) -> [T]`                         | First N elements                         |
| `list_drop(l: [T], n: Int) -> [T]`                         | Skip first N                             |
| `list_take_while(l: [T], f: fn(T)->Bool) -> [T]`           |                                          |
| `list_drop_while(l: [T], f: fn(T)->Bool) -> [T]`           |                                          |
| `list_partition(l: [T], f: fn(T)->Bool) -> ([T],[T])`      | Split into (yes, no)                     |
| `list_unzip(l: [(A,B)]) -> ([A],[B])`                      |                                          |
| `list_zip(a: [T], b: [U]) -> [(T,U)]`                      |                                          |
| `list_zip_with(a: [T], b: [U], f: fn(T,U)->V) -> [V]`     |                                          |
| `list_enumerate(l: [T]) -> [(Int,T)]`                      | Index-value pairs                        |
| `list_intersperse(l: [T], sep: T) -> [T]`                  | Insert sep between elements              |
| `list_join_str(l: [Str], sep: Str) -> Str`                 | Join strings                             |

### Sorting (`sort_*`)

| Function                                              | Description                            |
|-------------------------------------------------------|----------------------------------------|
| `sort_asc(l: [T]) -> [T]`                             | Sort ascending (Int/Str)               |
| `sort_desc(l: [T]) -> [T]`                            | Sort descending                        |
| `sort_by(l: [T], key: fn(T)->K) -> [T]`              | Sort by key function                   |
| `sort_by_desc(l: [T], key: fn(T)->K) -> [T]`         | Sort by key descending                 |
| `sort_with(l: [T], cmp: fn(T,T)->Ordering) -> [T]`   | Sort with custom comparator            |
| `sort_reverse(l: [T]) -> [T]`                         | Reverse a list                         |
| `sort_top(l: [T], n: Int) -> [T]`                     | Top N elements                         |
| `sort_bottom(l: [T], n: Int) -> [T]`                  | Bottom N elements                      |
| `sort_is_sorted(l: [T]) -> Bool`                      |                                        |
| `sort_cmp_int(a: Int, b: Int) -> Ordering`            | Comparator for ints                    |
| `sort_cmp_str(a: Str, b: Str) -> Ordering`            | Comparator for strings                 |
| `sort_cmp_float(a: Float, b: Float) -> Ordering`      |                                        |
| `sort_cmp_str_natural(a: Str, b: Str) -> Ordering`    | Natural sort (1, 2, 10 order)          |
| `sort_then(first: Ordering, second: Ordering) -> Ordering` | Compose comparators               |
| `sort_less() -> Ordering`                             | Ordering constants                     |
| `sort_equal() -> Ordering`                            |                                        |
| `sort_greater() -> Ordering`                          |                                        |

### Maps (`map_*`)

| Function                                                   | Description                          |
|------------------------------------------------------------|--------------------------------------|
| `map_new() -> Map<K,V>`                                    | Empty map                            |
| `map_get(m: Map<K,V>, key: K) -> Option<V>`               |                                      |
| `map_get_or(m: Map<K,V>, key: K, default: V) -> V`        |                                      |
| `map_insert(m: Map<K,V>, key: K, val: V) -> Map<K,V>`     | Returns new map with entry           |
| `map_set(m: Map<K,V>, key: K, val: V) -> Map<K,V>`        | Alias for insert                     |
| `map_remove(m: Map<K,V>, key: K) -> Map<K,V>`             |                                      |
| `map_contains_key(m: Map<K,V>, key: K) -> Bool`           |                                      |
| `map_keys(m: Map<K,V>) -> [K]`                             |                                      |
| `map_values(m: Map<K,V>) -> [V]`                           |                                      |
| `map_entries(m: Map<K,V>) -> [(K,V)]`                      |                                      |
| `map_len(m: Map<K,V>) -> Int`                              |                                      |
| `map_is_empty(m: Map<K,V>) -> Bool`                        |                                      |
| `map_merge(a: Map<K,V>, b: Map<K,V>) -> Map<K,V>`         | Right wins on conflict               |
| `map_from_list(pairs: [(K,V)]) -> Map<K,V>`               |                                      |
| `map_group_by(l: [T], key: fn(T)->K) -> Map<K,[T]>`       |                                      |
| `map_count_by(l: [T], key: fn(T)->K) -> Map<K,Int>`       |                                      |
| `map_filter(m: Map<K,V>, f: fn(K,V)->Bool) -> Map<K,V>`   |                                      |
| `map_map_values(m: Map<K,V>, f: fn(V)->U) -> Map<K,U>`    |                                      |
| `map_fold(m: Map<K,V>, init: U, f: fn(U,K,V)->U) -> U`    |                                      |

### Sets (`set_*`)

| Function                                              | Description                          |
|-------------------------------------------------------|--------------------------------------|
| `set_new() -> Set<T>`                                 | Empty set                            |
| `set_from_list(l: [T]) -> Set<T>`                    |                                      |
| `set_to_list(s: Set<T>) -> [T]`                      |                                      |
| `set_insert(s: Set<T>, item: T) -> Set<T>`           |                                      |
| `set_remove(s: Set<T>, item: T) -> Set<T>`           |                                      |
| `set_contains(s: Set<T>, item: T) -> Bool`           |                                      |
| `set_len(s: Set<T>) -> Int`                          |                                      |
| `set_is_empty(s: Set<T>) -> Bool`                    |                                      |
| `set_union(a: Set<T>, b: Set<T>) -> Set<T>`          |                                      |
| `set_intersection(a: Set<T>, b: Set<T>) -> Set<T>`   |                                      |
| `set_difference(a: Set<T>, b: Set<T>) -> Set<T>`     |                                      |
| `set_symmetric_difference(a: Set<T>, b: Set<T>) -> Set<T>` |                               |
| `set_is_subset(a: Set<T>, b: Set<T>) -> Bool`        |                                      |
| `set_is_superset(a: Set<T>, b: Set<T>) -> Bool`      |                                      |
| `set_is_disjoint(a: Set<T>, b: Set<T>) -> Bool`      |                                      |
| `set_map(s: Set<T>, f: fn(T)->U) -> Set<U>`          |                                      |
| `set_filter(s: Set<T>, f: fn(T)->Bool) -> Set<T>`    |                                      |

### Math (`math_*`)

| Function                                        | Description                          |
|-------------------------------------------------|--------------------------------------|
| `math_abs(n: Int) -> Int`                       | Absolute value                       |
| `math_abs_f(n: Float) -> Float`                 |                                      |
| `math_min(a: Int, b: Int) -> Int`               |                                      |
| `math_max(a: Int, b: Int) -> Int`               |                                      |
| `math_clamp(n: Int, lo: Int, hi: Int) -> Int`   |                                      |
| `math_pow(base: Int, exp: Int) -> Int`          |                                      |
| `math_gcd(a: Int, b: Int) -> Int`               |                                      |
| `math_lcm(a: Int, b: Int) -> Int`               |                                      |
| `math_floor(n: Float) -> Float`                 |                                      |
| `math_ceil(n: Float) -> Float`                  |                                      |
| `math_round(n: Float) -> Float`                 |                                      |
| `math_sqrt(n: Float) -> Float`                  |                                      |
| `math_pow_f(base: Float, exp: Float) -> Float`  |                                      |
| `math_log(n: Float, base: Float) -> Float`      |                                      |
| `math_ln(n: Float) -> Float`                    |                                      |
| `math_log2(n: Float) -> Float`                  |                                      |
| `math_log10(n: Float) -> Float`                 |                                      |
| `math_sin(n: Float) -> Float`                   |                                      |
| `math_cos(n: Float) -> Float`                   |                                      |
| `math_tan(n: Float) -> Float`                   |                                      |
| `math_atan2(y: Float, x: Float) -> Float`       |                                      |
| `math_is_nan(n: Float) -> Bool`                 |                                      |
| `math_is_finite(n: Float) -> Bool`              |                                      |
| `math_int_to_float(n: Int) -> Float`            |                                      |
| `math_float_to_int(n: Float) -> Int`            | Truncates toward zero                |
| `math_PI() -> Float`                            | 3.14159…                             |
| `math_E() -> Float`                             | 2.71828…                             |
| `math_TAU() -> Float`                           | 6.28318…                             |

### File I/O (`io_*`)

| Function                                                 | Description                        |
|----------------------------------------------------------|------------------------------------|
| `io_print(s: Str) -> Unit`                               |                                    |
| `io_println(s: Str) -> Unit`                             |                                    |
| `io_eprint(s: Str) -> Unit`                              | Stderr                             |
| `io_read_line() -> Str`                                  | Read from stdin                    |
| `io_read_all() -> Str`                                   | Read all of stdin                  |
| `io_read_file(path: Str) -> Str`                         | Read file as string                |
| `io_write_file(path: Str, content: Str) -> Unit`         | Write / overwrite                  |
| `io_append_file(path: Str, content: Str) -> Unit`        | Append                             |
| `io_read_lines(path: Str) -> [Str]`                      | Read lines                         |
| `io_read_bytes(path: Str) -> [Int]`                      | Read raw bytes                     |
| `io_write_bytes(path: Str, content: [Int]) -> Unit`      |                                    |
| `io_exists(path: Str) -> Bool`                           |                                    |
| `io_is_file(path: Str) -> Bool`                          |                                    |
| `io_is_dir(path: Str) -> Bool`                           |                                    |
| `io_file_size(path: Str) -> Int`                         | Bytes                              |
| `io_modified_at(path: Str) -> Int`                       | Unix timestamp                     |
| `io_list_dir(path: Str) -> [Str]`                        | Directory entries                  |
| `io_list_dir_recursive(path: Str) -> [Str]`              |                                    |
| `io_make_dir(path: Str) -> Unit`                         |                                    |
| `io_make_dir_all(path: Str) -> Unit`                     | `mkdir -p`                         |
| `io_remove_file(path: Str) -> Unit`                      |                                    |
| `io_remove_dir(path: Str) -> Unit`                       |                                    |
| `io_remove_dir_all(path: Str) -> Unit`                   | `rm -rf`                           |
| `io_rename(from: Str, to: Str) -> Unit`                  |                                    |
| `io_copy(from: Str, to: Str) -> Unit`                    |                                    |
| `io_file_writer(path: Str) -> FileWriter`                | Streaming writer                   |
| `io_write(w: FileWriter, s: Str) -> Unit`                |                                    |
| `io_writeln(w: FileWriter, s: Str) -> Unit`              |                                    |
| `io_flush(w: FileWriter) -> Unit`                        |                                    |
| `io_close(w: FileWriter) -> Unit`                        |                                    |

### Path (`path_*`)

| Function                                        | Description                          |
|-------------------------------------------------|--------------------------------------|
| `path_join(base: Str, parts: [Str]) -> Str`     | Join path segments                   |
| `path_parent(p: Str) -> Str`                    | Parent directory                     |
| `path_filename(p: Str) -> Str`                  | File name component                  |
| `path_stem(p: Str) -> Str`                      | Name without extension               |
| `path_extension(p: Str) -> Str`                 | Extension without leading `.`        |
| `path_with_extension(p: Str, ext: Str) -> Str`  | Replace extension                    |
| `path_normalize(p: Str) -> Str`                 | Resolve `.` and `..`                 |
| `path_is_absolute(p: Str) -> Bool`              |                                      |
| `path_is_relative(p: Str) -> Bool`              |                                      |
| `path_relative_to(p: Str, base: Str) -> Str`    |                                      |
| `path_components(p: Str) -> [Str]`              | Split into path segments             |

### Environment (`env_*`)

| Function                                          | Description                          |
|---------------------------------------------------|--------------------------------------|
| `env_get(name: Str) -> Option<Str>`              | Get env var                          |
| `env_get_or(name: Str, default: Str) -> Str`     | Get with fallback                    |
| `env_require(name: Str) -> Result<Str, EnvError>`| Get or error                         |
| `env_set(name: Str, val: Str) -> Unit`            |                                      |
| `env_remove(name: Str) -> Unit`                   |                                      |
| `env_all() -> Map<Str, Str>`                      | All env vars                         |
| `env_args() -> [Str]`                             | Command-line arguments               |
| `env_cwd() -> Result<Str, IoError>`               | Current working directory            |
| `env_home_dir() -> Option<Str>`                   |                                      |
| `env_temp_dir() -> Str`                           |                                      |
| `env_exit(code: Int) -> Unit`                     | Exit process                         |

### Dotenv (`dotenv_*`)

| Function                                              | Description                          |
|-------------------------------------------------------|--------------------------------------|
| `dotenv_load() -> Unit`                               | Load `.env` into process env         |
| `dotenv_load_path(path: Str) -> Unit`                 | Load specific file                   |
| `dotenv_load_into_env() -> Unit`                      |                                      |
| `dotenv_parse(s: Str) -> Map<Str, Str>`               | Parse without setting env            |

### Time (`time_*`)

| Function                                           | Description                          |
|----------------------------------------------------|--------------------------------------|
| `time_now() -> Time`                               | Current UTC time                     |
| `time_now_local() -> Time`                         | Current local time                   |
| `time_unix(t: Time) -> Int`                        | Unix timestamp (seconds)             |
| `time_unix_ms(t: Time) -> Int`                     | Milliseconds since epoch             |
| `time_from_unix(secs: Int) -> Time`                |                                      |
| `time_from_unix_ms(ms: Int) -> Time`               |                                      |
| `time_format(t: Time, layout: Str) -> Str`         | strftime-style format                |
| `time_format_iso(t: Time) -> Str`                  | ISO 8601                             |
| `time_format_rfc2822(t: Time) -> Str`              |                                      |
| `time_parse(s: Str, layout: Str) -> Time`          |                                      |
| `time_parse_iso(s: Str) -> Time`                   |                                      |
| `time_year(t: Time) -> Int`                        |                                      |
| `time_month(t: Time) -> Int`                       | 1–12                                 |
| `time_day(t: Time) -> Int`                         | 1–31                                 |
| `time_hour(t: Time) -> Int`                        |                                      |
| `time_minute(t: Time) -> Int`                      |                                      |
| `time_second(t: Time) -> Int`                      |                                      |
| `time_weekday(t: Time) -> Int`                     | 0 = Sunday                           |
| `time_add_secs(t: Time, secs: Int) -> Time`        |                                      |
| `time_add_ms(t: Time, ms: Int) -> Time`            |                                      |
| `time_add_days(t: Time, days: Int) -> Time`        |                                      |
| `time_diff_secs(a: Time, b: Time) -> Int`          | a - b in seconds                     |
| `time_diff_ms(a: Time, b: Time) -> Int`            |                                      |
| `time_secs(n: Int) -> Duration`                    | Duration builder                     |
| `time_ms(n: Int) -> Duration`                      |                                      |
| `time_minutes(n: Int) -> Duration`                 |                                      |
| `time_hours(n: Int) -> Duration`                   |                                      |
| `time_days(n: Int) -> Duration`                    |                                      |
| `time_sleep(d: Duration) -> Unit`                  | Blocking sleep                       |
| `time_monotonic() -> Int`                          | Monotonic clock (ms)                 |

### Random (`rand_*`)

| Function                                        | Description                          |
|-------------------------------------------------|--------------------------------------|
| `rand_int(low: Int, high: Int) -> Int`          | Uniform in `[low, high)`             |
| `rand_float() -> Float`                         | Uniform in `[0, 1)`                  |
| `rand_bool() -> Bool`                           |                                      |
| `rand_pick(l: [T]) -> Option<T>`               | Random element                       |
| `rand_sample(l: [T], n: Int) -> [T]`           | Sample without replacement           |
| `rand_shuffle(l: [T]) -> [T]`                  |                                      |
| `rand_seeded(seed: Int) -> Rng`                | Seeded RNG                           |
| `rand_rng_int(rng: Rng, low: Int, high: Int) -> Int` | From seeded RNG              |
| `rand_rng_float(rng: Rng) -> Float`            |                                      |
| `rand_rng_pick(rng: Rng, l: [T]) -> Option<T>` |                                      |
| `rand_rng_shuffle(rng: Rng, l: [T]) -> [T]`   |                                      |

### Regex (`re_*`)

| Function                                                   | Description                      |
|------------------------------------------------------------|----------------------------------|
| `re_compile(pattern: Str) -> Regex`                        | Compile pattern                  |
| `re_compile_flags(pattern: Str, flags: Str) -> Regex`      | With flags (e.g. `"i"`)          |
| `re_is_match(r: Regex, s: Str) -> Bool`                    |                                  |
| `re_find(r: Regex, s: Str) -> Option<Match>`               | First match                      |
| `re_find_all(r: Regex, s: Str) -> [Match]`                 | All matches                      |
| `re_match_str(m: Match) -> Str`                            | Matched text                     |
| `re_match_start(m: Match) -> Int`                          | Start offset                     |
| `re_match_end(m: Match) -> Int`                            | End offset                       |
| `re_capture(m: Match, index: Int) -> Option<Str>`          | Capture group by index           |
| `re_capture_name(m: Match, name: Str) -> Option<Str>`      | Named capture group              |
| `re_replace(r: Regex, s: Str, repl: Str) -> Str`           | Replace first                    |
| `re_replace_all(r: Regex, s: Str, repl: Str) -> Str`       | Replace all                      |
| `re_replace_fn(r: Regex, s: Str, f: fn(Match)->Str) -> Str`| Replace with function            |
| `re_split(r: Regex, s: Str) -> [Str]`                      |                                  |
| `re_split_n(r: Regex, s: Str, n: Int) -> [Str]`            | At most N parts                  |

### JSON (`json_*`)

| Function                                          | Description                        |
|---------------------------------------------------|------------------------------------|
| `json_parse(s: Str) -> Json`                      | Parse JSON string                  |
| `json_to_str(v: Json) -> Str`                     | Compact JSON string                |
| `json_to_str_pretty(v: Json) -> Str`              | Indented JSON                      |
| `json_null() -> Json`                             | JSON null                          |
| `json_bool(b: Bool) -> Json`                      |                                    |
| `json_int(n: Int) -> Json`                        |                                    |
| `json_float(n: Float) -> Json`                    |                                    |
| `json_str(s: Str) -> Json`                        |                                    |
| `json_array(items: [Json]) -> Json`               |                                    |
| `json_object(fields: [(Str, Json)]) -> Json`      |                                    |
| `json_get(v: Json, key: Str) -> Json`             | Object field access                |
| `json_get_path(v: Json, path: Str) -> Json`       | Dot-path access (`"a.b.c"`)        |
| `json_as_str(v: Json) -> Str`                     | Extract Str (panics if wrong type) |
| `json_as_int(v: Json) -> Int`                     |                                    |
| `json_as_float(v: Json) -> Float`                 |                                    |
| `json_as_bool(v: Json) -> Bool`                   |                                    |
| `json_as_array(v: Json) -> [Json]`                |                                    |
| `json_as_object(v: Json) -> Map<Str, Json>`       |                                    |
| `json_is_null(v: Json) -> Bool`                   |                                    |

### Formatting (`fmt_*`)

| Function                                          | Description                        |
|---------------------------------------------------|------------------------------------|
| `fmt_format(template: Str, args: [Str]) -> Str`   | `{}` placeholders                  |
| `fmt_int(n: Int, radix: Int) -> Str`              | Format int in given base           |
| `fmt_int_padded(n: Int, width: Int) -> Str`       | Zero-padded                        |
| `fmt_float(n: Float, decimals: Int) -> Str`       | Fixed decimal places               |
| `fmt_float_exp(n: Float, decimals: Int) -> Str`   | Scientific notation                |
| `fmt_bool(b: Bool) -> Str`                        |                                    |
| `fmt_debug(v: T) -> Str`                          | Debug representation               |

### Encoding (`encode_*`)

| Function                                              | Description                      |
|-------------------------------------------------------|----------------------------------|
| `encode_base64(data: Bytes) -> Str`                   |                                  |
| `encode_base64_decode(s: Str) -> Bytes`               |                                  |
| `encode_base64_url(data: Bytes) -> Str`               | URL-safe base64                  |
| `encode_base64_url_decode(s: Str) -> Bytes`           |                                  |
| `encode_hex(data: Bytes) -> Str`                      | Lowercase hex                    |
| `encode_hex_upper(data: Bytes) -> Str`                |                                  |
| `encode_hex_decode(s: Str) -> Bytes`                  |                                  |
| `encode_url_encode(s: Str) -> Str`                    | Percent-encode                   |
| `encode_url_decode(s: Str) -> Str`                    |                                  |
| `encode_url_encode_component(s: Str) -> Str`          |                                  |
| `encode_url_decode_component(s: Str) -> Str`          |                                  |
| `encode_html_escape(s: Str) -> Str`                   |                                  |
| `encode_html_unescape(s: Str) -> Str`                 |                                  |
| `encode_csv_row(fields: [Str]) -> Str`                | One CSV row                      |
| `encode_csv_parse_row(s: Str) -> [Str]`               |                                  |

### Crypto (`crypto_*`)

| Function                                              | Description                      |
|-------------------------------------------------------|----------------------------------|
| `crypto_sha256(data: Bytes) -> Bytes`                 |                                  |
| `crypto_sha256_str(s: Str) -> Str`                    | Hex digest of UTF-8 string       |
| `crypto_sha512(data: Bytes) -> Bytes`                 |                                  |
| `crypto_blake3(data: Bytes) -> Bytes`                 |                                  |
| `crypto_hmac_sha256(key: Bytes, data: Bytes) -> Bytes`|                                  |
| `crypto_hmac_sha512(key: Bytes, data: Bytes) -> Bytes`|                                  |
| `crypto_hash_password(password: Str) -> Str`          | bcrypt/argon2                    |
| `crypto_verify_password(password: Str, hash: Str) -> Bool` |                           |
| `crypto_random_bytes(n: Int) -> Bytes`                | Cryptographically random         |
| `crypto_random_int(low: Int, high: Int) -> Int`       |                                  |
| `crypto_eq_constant(a: Bytes, b: Bytes) -> Bool`      | Constant-time compare            |

### HTTP client (`http_*`)

| Function                                                | Description                      |
|---------------------------------------------------------|----------------------------------|
| `http_get(url: Str) -> Response`                        |                                  |
| `http_post(url: Str, body: Bytes) -> Response`          |                                  |
| `http_put(url: Str, body: Bytes) -> Response`           |                                  |
| `http_patch(url: Str, body: Bytes) -> Response`         |                                  |
| `http_delete(url: Str) -> Response`                     |                                  |
| `http_get_json(url: Str) -> Json`                       | GET + parse JSON                 |
| `http_post_json(url: Str, body: Json) -> Response`      | POST JSON body                   |
| `http_request(method: Str, url: Str) -> RequestBuilder`| Request builder                  |
| `http_header(req: RequestBuilder, name: Str, val: Str) -> RequestBuilder` |        |
| `http_bearer(req: RequestBuilder, token: Str) -> RequestBuilder` |              |
| `http_basic_auth(req: RequestBuilder, user: Str, pass: Str) -> RequestBuilder` | |
| `http_body_str(req: RequestBuilder, s: Str) -> RequestBuilder` |                |
| `http_body_json(req: RequestBuilder, v: Json) -> RequestBuilder` |               |
| `http_body_bytes(req: RequestBuilder, b: Bytes) -> RequestBuilder` |             |
| `http_timeout(req: RequestBuilder, ms: Int) -> RequestBuilder` |                 |
| `http_follow_redirects(req: RequestBuilder, follow: Bool) -> RequestBuilder` |   |
| `http_send(req: RequestBuilder) -> Response`            |                          |
| `http_status(r: Response) -> Int`                       | HTTP status code         |
| `http_ok(r: Response) -> Bool`                          | Status in 200–299        |
| `http_body_str_resp(r: Response) -> Str`                | Response body as string  |
| `http_body_json_resp(r: Response) -> Json`              | Response body as JSON    |
| `http_body_bytes_resp(r: Response) -> Bytes`            | Response body as bytes   |
| `http_header_get(r: Response, name: Str) -> Option<Str>`| Response header          |
| `http_headers(r: Response) -> Map<Str, Str>`            |                          |

### HTTP server (`serve_*`)

```ferric
let server = serve_new()
let server = serve_get(s: server, path: "/hello", handler: |req| {
    serve_ok_str(body: "Hello!")
})
serve_listen(s: server, port: 8080)
```

| Function                                                        | Description                     |
|-----------------------------------------------------------------|---------------------------------|
| `serve_new() -> Server`                                         | Create server                   |
| `serve_get(s: Server, path: Str, handler: fn(Request)->Response) -> Server` |           |
| `serve_post(s: Server, path: Str, handler: fn(Request)->Response) -> Server` |          |
| `serve_put(...)`, `serve_patch(...)`, `serve_delete(...)`       |                                 |
| `serve_any(s: Server, path: Str, handler: ...) -> Server`       | Any method                      |
| `serve_use(s: Server, mw: fn(Request,fn(Request)->Response)->Response) -> Server` | Middleware |
| `serve_mount(s: Server, prefix: Str, sub: Server) -> Server`    | Sub-router                      |
| `serve_listen(s: Server, port: Int) -> Unit`                    | Start (blocking)                |
| `serve_listen_addr(s: Server, addr: Str) -> Unit`               |                                 |
| `serve_path(req: Request) -> Str`                               |                                 |
| `serve_method(req: Request) -> Str`                             |                                 |
| `serve_param(req: Request, name: Str) -> Option<Str>`           | URL path parameter              |
| `serve_query(req: Request, name: Str) -> Option<Str>`           | Query string parameter          |
| `serve_header_get(req: Request, name: Str) -> Option<Str>`      |                                 |
| `serve_body_str(req: Request) -> Str`                           |                                 |
| `serve_body_json(req: Request) -> Json`                         |                                 |
| `serve_body_bytes(req: Request) -> Bytes`                       |                                 |
| `serve_ok(status: Int, body: Bytes) -> Response`                |                                 |
| `serve_ok_str(body: Str) -> Response`                           | 200 text/plain                  |
| `serve_ok_json(body: Json) -> Response`                         | 200 application/json            |
| `serve_created(body: Str) -> Response`                          | 201                             |
| `serve_no_content() -> Response`                                | 204                             |
| `serve_bad_request(body: Str) -> Response`                      | 400                             |
| `serve_unauthorized(body: Str) -> Response`                     | 401                             |
| `serve_forbidden(body: Str) -> Response`                        | 403                             |
| `serve_not_found(body: Str) -> Response`                        | 404                             |
| `serve_internal_error(body: Str) -> Response`                   | 500                             |
| `serve_with_header(r: Response, name: Str, val: Str) -> Response` |                               |
| `serve_response(status: Int, body: Str, headers: Map<Str,Str>) -> Response` |               |

### Sockets (`sock_*`)

| Function                                                | Description                      |
|---------------------------------------------------------|----------------------------------|
| `sock_tcp_connect(host: Str, port: Int) -> TcpStream`   |                                  |
| `sock_tcp_listen(port: Int) -> TcpListener`             |                                  |
| `sock_accept(l: TcpListener) -> TcpStream`              | Accept one connection            |
| `sock_read(s: TcpStream, n: Int) -> Bytes`              |                                  |
| `sock_read_exact(s: TcpStream, n: Int) -> Bytes`        |                                  |
| `sock_read_line(s: TcpStream) -> Str`                   |                                  |
| `sock_write(s: TcpStream, b: Bytes) -> Unit`            |                                  |
| `sock_write_str(s: TcpStream, text: Str) -> Unit`       |                                  |
| `sock_close(s: TcpStream) -> Unit`                      |                                  |
| `sock_peer_addr(s: TcpStream) -> Str`                   |                                  |
| `sock_local_addr(s: TcpStream) -> Str`                  |                                  |
| `sock_udp_bind(port: Int) -> UdpSocket`                 |                                  |
| `sock_udp_send(s: UdpSocket, addr: Str, data: Bytes) -> Unit` |                          |
| `sock_udp_recv(s: UdpSocket) -> (Bytes, Str)`           | (data, sender_addr)              |

### Processes (`proc_*`)

| Function                                              | Description                      |
|-------------------------------------------------------|----------------------------------|
| `proc_run(cmd: Str) -> ProcOutput`                    | Run and collect output           |
| `proc_run_shell(cmd: Str) -> ProcOutput`              | Via `/bin/sh -c`                 |
| `proc_stdout(o: ProcOutput) -> Str`                   |                                  |
| `proc_stderr(o: ProcOutput) -> Str`                   |                                  |
| `proc_exit_code(o: ProcOutput) -> Int`                |                                  |
| `proc_ok(o: ProcOutput) -> Bool`                      | Exit code == 0                   |
| `proc_command(cmd: Str) -> ProcCommand`               | Builder                          |
| `proc_arg(c: ProcCommand, arg: Str) -> ProcCommand`   |                                  |
| `proc_args(c: ProcCommand, args: [Str]) -> ProcCommand`|                                 |
| `proc_env_set(c: ProcCommand, key: Str, val: Str) -> ProcCommand` |              |
| `proc_cwd(c: ProcCommand, dir: Str) -> ProcCommand`   |                                  |
| `proc_spawn(c: ProcCommand) -> ProcProcess`           | Non-blocking                     |
| `proc_wait(p: ProcProcess) -> ProcOutput`             |                                  |

### Logging (`log_*`)

| Function                                              | Description                      |
|-------------------------------------------------------|----------------------------------|
| `log_info(msg: Str) -> Unit`                          |                                  |
| `log_warn(msg: Str) -> Unit`                          |                                  |
| `log_error(msg: Str) -> Unit`                         |                                  |
| `log_debug(msg: Str) -> Unit`                         |                                  |
| `log_set_level(level: Str) -> Unit`                   | `"debug"`, `"info"`, `"warn"`, `"error"` |
| `log_get_level() -> Str`                              |                                  |
| `log_new(name: Str) -> Logger`                        | Named logger                     |
| `log_log_info(l: Logger, msg: Str) -> Unit`           |                                  |
| `log_with(l: Logger, key: Str, val: Str) -> Logger`   | Add structured field             |

### Bytes (`bytes_*`)

| Function                                            | Description                      |
|-----------------------------------------------------|----------------------------------|
| `bytes_new() -> Bytes`                              | Empty byte buffer                |
| `bytes_len(b: Bytes) -> Int`                        |                                  |
| `bytes_is_empty(b: Bytes) -> Bool`                  |                                  |
| `bytes_get(b: Bytes, index: Int) -> Option<Int>`    |                                  |
| `bytes_slice(b: Bytes, from: Int, to: Int) -> Bytes`|                                  |
| `bytes_concat(a: Bytes, b: Bytes) -> Bytes`         |                                  |
| `bytes_contains(b: Bytes, sub: Bytes) -> Bool`      |                                  |
| `bytes_to_hex(b: Bytes) -> Str`                     |                                  |
| `bytes_from_hex(s: Str) -> Bytes`                   |                                  |
| `bytes_to_base64(b: Bytes) -> Str`                  |                                  |
| `bytes_from_base64(s: Str) -> Bytes`                |                                  |
| `bytes_to_str(b: Bytes) -> Str`                     | UTF-8 decode                     |
| `bytes_to_list(b: Bytes) -> [Int]`                  |                                  |
| `bytes_from_list(l: [Int]) -> Bytes`                |                                  |
| `bytes_builder() -> BytesBuilder`                   | Mutable builder                  |
| `bytes_write_str(b: BytesBuilder, s: Str) -> Unit`  |                                  |
| `bytes_write_byte(b: BytesBuilder, n: Int) -> Unit` |                                  |
| `bytes_write_u32_be(b: BytesBuilder, n: Int) -> Unit`|                                 |
| `bytes_build(b: BytesBuilder) -> Bytes`             | Finalise builder                 |

### Sync primitives (`sync_*`)

| Function                                              | Description                      |
|-------------------------------------------------------|----------------------------------|
| `sync_channel() -> (Sender<T>, Receiver<T>)`          | MPSC channel                     |
| `sync_send(s: Sender<T>, val: T) -> Unit`             |                                  |
| `sync_recv(r: Receiver<T>) -> T`                      | Blocking receive                 |
| `sync_try_recv(r: Receiver<T>) -> Option<T>`          | Non-blocking                     |
| `sync_mutex(val: T) -> Mutex<T>`                      |                                  |
| `sync_lock(m: Mutex<T>) -> T`                         | Lock and get value               |
| `sync_once() -> Once`                                 |                                  |
| `sync_get(o: Once, f: fn()->T) -> T`                  | Initialize once                  |

### LLM (`llm_*`)

| Function                                              | Description                        |
|-------------------------------------------------------|------------------------------------|
| `llm_prompt(text: Str) -> Result<Str, Str>`           | One-shot text prompt               |
| `llm_prompt_schema(text: Str, schema: Json) -> Result<Json, Str>` | Structured output |

Configuration via environment variables: `OPENROUTER_API_KEY` (required), `OPENROUTER_MODEL` (default `anthropic/claude-sonnet-4-7`), `OPENROUTER_BASE_URL`, `OPENROUTER_TIMEOUT_MS`.

---

## Complete Example Programs

### Hello world

```ferric
fn greet(name: Str) -> Str {
    "Hello, " + name
}

let message = greet(name: "world")
println(s: message)
```

### Fibonacci

```ferric
fn fibonacci(n: Int) -> Int {
    if n <= 1 {
        n
    } else {
        fibonacci(n: n - 1) + fibonacci(n: n - 2)
    }
}

println(s: int_to_str(n: fibonacci(n: 10)))
```

### Option/Result handling

```ferric
fn safe_div(a: Int, b: Int) -> Option<Int> {
    if b == 0 { Option::None } else { Option::Some(a / b) }
}

match safe_div(a: 10, b: 0) {
    Option::Some(v) => println(s: int_to_str(n: v)),
    Option::None    => println(s: "division by zero"),
}
```

### Async parallel tasks

```ferric
async fn slow_task(n: Int) -> Int {
    await sleep(ms: 500)
    n
}

let h1 = spawn(task: slow_task(n: 1))
let h2 = spawn(task: slow_task(n: 2))

let pair = join(a: h1, b: h2)
match pair {
    (a, b) => println(s: int_to_str(n: a + b))
}
```

### HTTP request

```ferric
let resp = http_get(url: "https://api.example.com/data")
if http_ok(r: resp) {
    let body = http_body_str_resp(r: resp)
    println(s: body)
} else {
    println(s: "error: " + int_to_str(n: http_status(r: resp)))
}
```

### Shell integration

```ferric
let name = "world"
let out = $ echo Hello, @{name}!
println(s: shell_stdout(output: out))

let files = $ ls -la
println(s: shell_stdout(output: files))
```
