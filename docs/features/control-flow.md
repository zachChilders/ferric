# Control Flow

Ferric is expression-oriented: `if`, `match`, and `{ ... }` blocks all evaluate to a value.

## `if` / `else`

```rust
let abs = if x < 0 { -x } else { x }
```

- Both branches must produce values of the same type — that's the type of the `if` expression.
- The `else` is optional; when omitted, the `if` produces `Unit` and the `then` branch's tail must also produce `Unit`.
- `else if` chains naturally: `if a { ... } else if b { ... } else { ... }`.

## Blocks

```rust
let y = {
    let a = 1
    let b = 2
    a + b
}
```

A block is a sequence of statements followed by an optional tail expression. The tail expression's value is the block's value. Without a tail expression the block evaluates to `Unit`.

## `while`

```rust
let mut counter = 0
while counter < 5 {
    println(s: int_to_str(n: counter))
    counter = counter + 1
}
```

Condition must be `Bool`. The body is any expression (typically a block). The whole `while` evaluates to `Unit`.

## `loop`

```rust
let result = loop {
    if done() { break }
    work()
}
```

`loop { }` runs forever until `break` exits. Like `while`, it evaluates to `Unit` (M6 does not yet support `break <value>`).

## `break` / `continue`

- `break` exits the enclosing loop.
- `continue` skips to the next iteration.
- Outside a loop, both are `ResolveError::BreakOutsideLoop` / `ContinueOutsideLoop`.

## `return`

`return expr` returns from the enclosing function. `return` without an expression returns `Unit`.

A function's "natural" return is the tail expression of its body — `return` is for early exits.

## `for`

See [`closures-arrays.md`](closures-arrays.md) — `for x in arr { body }` desugars to a counting `while`.

## `match`

See [`adts.md`](adts.md). Match arms must agree on body type; the type checker enforces this.

## Statements vs. expressions

Statements (live in `Stmt`):

- `let` / `let mut` bindings
- `=` assignments to mutable bindings
- `for` loops
- `require`
- A bare expression terminated by a newline or `;`

Everything else is an expression. The trailing position of a block can be either: `let x = { ... a }` uses `a` as the value; `let x = { ... a; }` uses `Unit`.

## Mutability

`let` is immutable. `let mut` is mutable. Assignment to an immutable binding is `ResolveError::AssignToImmutable`. Mutability is tracked in the resolver, not the type system.

## See also

- [`adts.md`](adts.md) — `match` and pattern shapes.
- [`closures-arrays.md`](closures-arrays.md) — `for` loops over arrays.
