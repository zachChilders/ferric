# `require` — Native Validation

`require` asserts an expression is true. If it isn't, the program halts with a diagnostic. With `(warn)` it merely warns and keeps going. With a `set:` closure it can recover and retry — making `require` an idempotent state-enforcement tool.

## Syntax

```
require <cond>
require <cond>, "<message>"
require <cond>, "<message>", set: || { <recovery> }
require(warn) <cond>
require(warn) <cond>, "<message>"
require(warn) <cond>, "<message>", set: || { <recovery> }
```

`<cond>` is any `Bool` expression. `<message>` is any `Str` expression. The `set:` clause is a zero-arg closure.

## Examples

Plain assertion:

```rust
let x = 5
require x > 0
```

With a message:

```rust
let y = 10
require y > 0, "y must be positive"
```

`(warn)` mode — emits a diagnostic but keeps running:

```rust
let mut x = -1
require(warn) x > 0, "x should be positive"
println(s: "execution continued after warn")
```

`set:` recovery — runs the closure if the condition fails, then re-checks:

```rust
let mut x = -1
require x > 0, "x must be positive", set: || { x = 5 }
// x is now 5; the post-set check passes; execution continues.
```

`(warn)` + `set:` — runs the closure, then warns if still false:

```rust
let mut y = -1
require(warn) y > 0, "y should be positive", set: || { y = -2 }
// set ran, condition still false → warning emitted, execution continues.
```

## Semantics in detail

- The condition is evaluated. If true, the require is a no-op.
- If false:
  - If a `set:` closure is present, it runs (and may mutate outer locals — closure capture is by reference for `let mut` bindings).
  - The condition is evaluated again.
  - If still false: in default mode, the program aborts with the message; in `(warn)` mode, a diagnostic is emitted and execution continues.
- If true on the second check, execution continues silently.

This shape is tailored for **idempotent enforcement**: assert that something is the case, and if not, *make* it the case.

## The set: closure

The closure must be zero-arg (`|| { ... }`). It is *inlined at the require site* by the compiler — there are no real closure values, no upvalues. That means:

- It can mutate `let mut` bindings in the surrounding scope.
- It cannot be moved, returned, or stored.
- Recursive closures inside `set:` aren't supported.

This is a deliberate M3 constraint that lets `require` ship without a full first-class closure runtime. Closure-as-value semantics elsewhere (e.g. `let f = |x| x + 1`) come from the M6 closure machinery in `ferric_resolve` + `ferric_compiler`.

## Lowering

`require` lowers to two opcodes — `Op::RequireFail` and `Op::RequireWarn`. The instruction layout is:

```
cond
JumpIfTrue end                      ─┐
(set_fn body, inlined)              │  if set: present
cond                                │
JumpIfTrue end                      │
message                             ┘
RequireFail | RequireWarn
end:
```

See `crates/ferric_compiler/src/lib.rs` for the lowering and `crates/ferric_vm/src/bytecode.rs` for the runtime arm.

## Suggested use cases

- Pre/post conditions on functions.
- Pre-flight checks at the top of a script (`require dir_exists("/etc"), ...`).
- Combined with `$ ...` for host enforcement:

  ```rust
  require shell_exit_code(output: $ test -f /etc/foo.conf) == 0,
          "/etc/foo.conf must exist",
          set: || { $ touch /etc/foo.conf }
  ```

## See also

- [`shell.md`](shell.md) for combining with `$`.
- [`closures-arrays.md`](closures-arrays.md) for first-class closures (the non-inlined kind).
