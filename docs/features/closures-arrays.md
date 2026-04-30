# Closures, Arrays, `for` Loops

## Closures

```rust
let add_bonus = |n| n + bonus      // bonus captured from outer scope
let triple = |n| n * 3

println(s: int_to_str(n: add_bonus(n: 5)))
println(s: int_to_str(n: triple(n: 7)))
```

Syntax is `|params| body`. With no parameters, write `|| body`. The body can be any expression — a single expression or a block:

```rust
let f = |x| {
    let doubled = x * 2
    doubled + 1
}
```

Closures are first-class values. They can be returned, stored, passed as arguments, and called like normal functions:

```rust
let make_adder = |inc| |x| x + inc
let add10 = make_adder(inc: 10)
println(s: int_to_str(n: add10(x: 32)))   // 42
```

### Capture analysis

The resolver runs a capture-analysis pass: any identifier referenced inside the closure that resolves to a binding in an enclosing scope is recorded as a captured upvalue. Captures are stored on `ResolveResult::captures` keyed by the closure's `NodeId`.

The compiler emits `Op::MakeClosure(fn_idx, n_captures)` after pushing each captured value onto the stack. At runtime the captured values occupy the leading slots of the closure chunk; ordinary parameters follow.

Top-level functions and stdlib natives are **not** routed through the capture mechanism — they're globally addressable through `Constant::Fn` / `Constant::NativeFn`. The resolver explicitly skips `def_id`s that live in `fn_slots`.

### Calling syntax

Closures use the same named-argument convention as regular functions. Parameters declared without an explicit name (closure-style) take the parameter's position-name (`x`, `n`, etc.) as the named slot:

```rust
let f = |x| x * 2
f(x: 21)
```

### `require` `set:` closures are different

The closures supplied to `require ..., set: || { ... }` are *inlined* by the compiler. They're not first-class values — they can mutate outer locals because they're spliced inline at the require site. See [`require.md`](require.md).

## Arrays

```rust
let xs = [1, 2, 3, 4, 5]
let n = xs[0]
let len = array_len(arr: xs)
```

- Array literals are homogeneous — every element must share an inferred element type.
- The type is `[T]`. Indexing with `[i]` requires an `Int`; out-of-range produces `RuntimeError::IndexOutOfBounds`.
- `array_len` is a stdlib function. Length is not a method on arrays in M6.

VM representation: `Value::Array(Vec<Value>)`.

Opcodes: `Op::MakeArray(n)` builds, `Op::ArrayGet` indexes, `Op::ArrayLen` reads length.

## `for` loops

```rust
let mut total = 0
for x in [1, 2, 3, 4, 5] {
    total = total + x
}
```

`for x in iter { body }` desugars in the compiler to a counting `while` loop over `ArrayGet`. There is no `Iterator` trait in M6 — the iter expression must be an array.

The loop variable is a fresh binding scoped to the body. `break` and `continue` work as expected.

## Stdlib functions over arrays

The stdlib `list::*` and `sort::*` modules cover most array operations. Examples:

- `array_len(arr: xs)` — primitive intrinsic.
- `list::push`, `list::pop`, `list::contains`, `list::join`, ...
- `sort::sort`, `sort::sort_by`, ...

Higher-order array operations (`map`, `filter`, `fold`) that take closures are **not** in the stdlib — natives can't currently call back into closures (calling a Ferric closure from a native would need to plumb the VM through the native call boundary). Work around by writing an explicit `for` loop.

## Errors

| Error | Condition |
|---|---|
| `TypeError::ArrayElementMismatch` | Array literal mixes element types. |
| `TypeError::NotIndexable` | Indexing a non-array. |
| `TypeError::IndexNotInt` | Index expression isn't `Int`. |
| `RuntimeError::IndexOutOfBounds` | `xs[i]` with `i` out of range. |
| `RuntimeError::NotAnArray` | VM-level type error during indexing. |

## See also

- [`control-flow.md`](control-flow.md) — `while` / `loop` / `break` / `continue`.
- [`require.md`](require.md) — `set:` closures (inlined, not first-class).
