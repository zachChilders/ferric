# Algebraic Data Types + Pattern Matching

Structs, enums, tuples, and `match`. Exhaustiveness is checked at compile time and unreachable arms produce a warning.

## Structs

```rust
struct Point { x: Int, y: Int }

let p = Point { x: 3, y: 4 }
println(s: int_to_str(n: p.x))
```

Fields are order-independent at the literal site but the type checker verifies all required fields are present and well-typed. Field access is `.field_name`.

There are no anonymous record types — every struct has a declared name.

## Tuples

```rust
let pair = (1, "hi")

match pair {
    (n, s) => println(s: int_to_str(n: n) + " " + s),
}
```

Tuples have positional fields. **Tuple field access via `.0` / `.1` is not yet supported** — destructure with `match` to extract elements. (Documented limitation; a known gap inherited from M3.)

Tuple type annotations (`(Int, Str)`) **don't yet parse in `let` bindings** — work around by inferring the type or by destructuring with `match`.

## Enums

```rust
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
```

Variants can carry any number of typed payload fields (zero for unit variants). Construction is `EnumName::Variant(args)`; construction with named fields like `Variant { x: ... }` is not supported.

## Built-in `Option` and `Result`

`Option<T>` and `Result<T, E>` are registered as built-in enums at startup (M6 follow-up). They behave exactly like user enums:

```rust
fn safe_div(a: Int, b: Int) -> Option<Int> {
    if b == 0 { Option::None } else { Option::Some(a / b) }
}

fn parse_pos(s: Str) -> Result<Int, Str> {
    let n = str_parse_int(s: s)
    if n < 0 { Result::Err("negative") } else { Result::Ok(n) }
}

match safe_div(a: 10, b: 2) {
    Option::Some(v) => println(s: int_to_str(n: v)),
    Option::None    => println(s: "none"),
}
```

## Pattern matching

`match` is exhaustive over the scrutinee's type. Supported patterns:

| Pattern | Form |
|---|---|
| Wildcard | `_` |
| Variable binding | `x` |
| Literal | `42`, `"hi"`, `true`, `()` |
| Tuple | `(a, b, c)` |
| Struct | `Point { x, y }` (each field is itself a pattern) |
| Enum variant | `Shape::Circle(r)` / `Color::Red` |

Each arm body can be any expression. The arm types must agree — the `match` expression's type is the common type of all arm bodies.

```rust
fn describe(s: Shape) -> Str {
    match s {
        Shape::Circle(r) if r == 0 => "point",     // guards: not yet supported
        Shape::Circle(_) => "circle",
        Shape::Rectangle(w, h) => "rect",
    }
}
```

> Match guards (`if cond` after a pattern) are **not** currently supported. Work around with nested `match` or `if` inside the arm body.

## Exhaustiveness checking

`crates/ferric_exhaust` runs after typecheck and before lowering. It produces:

- `ExhaustivenessError::NonExhaustive { missing, span }` — fatal; lists at least one missing variant.
- `ExhaustivenessError::UnreachableArm { span }` — warning; arm is shadowed by a previous pattern.

Wildcards count as covering everything below them in the type.

## VM representation

| Surface | `Value` |
|---|---|
| `struct Foo { ... }` | `Value::Struct(Vec<Value>)` — fields in declaration order |
| `Foo::Bar(x, y)` | `Value::Variant(variant_idx: u16, Vec<Value>)` |
| `(a, b, c)` | `Value::Tuple(Vec<Value>)` |

Field access on a struct compiles to `Op::GetField(idx)`; the index is resolved from the field name at compile time using the struct's declaration order.

`match` on a variant compiles to `Op::MatchVariant` + `Op::UnpackVariant` over the discriminant.

## Errors

| Error | Condition |
|---|---|
| `ResolveError::UnknownStruct` / `UnknownVariant` | Name doesn't resolve. |
| `ResolveError::UnknownField` | Field name not on this struct. |
| `TypeError::FieldMismatch` | Field-list mismatch in a struct literal. |
| `TypeError::ArmTypeMismatch` | `match` arms produce different types. |
| `ExhaustivenessError::NonExhaustive` | Missing variant in a `match`. |
| `ExhaustivenessError::UnreachableArm` | Dead arm — warning. |

## See also

- [`closures-arrays.md`](closures-arrays.md) — arrays and closures.
- [`traits.md`](traits.md) — traits operate on the same struct/enum types.
