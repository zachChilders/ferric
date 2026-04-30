# Type System Overview

Ferric has a Hindley–Milner inference engine (since M3) extended with trait constraint solving (since M5) and opaque type aliases (since M7). Every expression resolves to a concrete `Ty` — there is no `Unknown` escape hatch.

## Primitive types

| Type | Notes |
|---|---|
| `Int` | 64-bit signed. Arithmetic wraps on overflow. |
| `Float` | 64-bit IEEE 754. |
| `Bool` | `true` / `false`. |
| `Str` | UTF-8 strings. Interning at the lexer level for literals. |
| `Unit` | The single value `()`. The type of statements, empty blocks, and side-effecting expressions. |

Primitives are spelled with a leading capital. They are *not* lexer keywords — they're matched as identifiers and recognised as types by later stages.

## Type-checker types (`Ty`)

In addition to the primitives:

- `Ty::Fn { params, ret }` — function values (`fn(Int, Int) -> Int`).
- `Ty::Tuple(Vec<Ty>)` — fixed-size positional product types.
- `Ty::Array(Box<Ty>)` — homogeneous arrays.
- `Ty::Struct { def_id, fields }` — declared struct types.
- `Ty::Enum { def_id, variants }` — declared enum types.
- `Ty::Option(Box<Ty>)`, `Ty::Result(Box<Ty>, Box<Ty>)` — built-in enums.
- `Ty::Async(Box<Ty>)`, `Ty::Handle(Box<Ty>)`, `Ty::Poll(Box<Ty>)` — async wrappers.
- `Ty::Opaque { def_id, inner }` — opaque type aliases.
- `Ty::Var(u32)` — unification variables, present during inference, all defaulted before final results.

## Type annotations (`TypeAnnotation`)

What appears in source:

```rust
let x: Int = 5
fn f(xs: [Int]) -> Option<Int> { ... }
let result: Result<Int, Str> = Ok(42)
let task: Async<Str> = async { ... }
```

The four shapes:

- `Named(Symbol)` — `Int`, `Str`, `MyStruct`.
- `Array(T)` — `[T]`.
- `Generic { head, args }` — `Option<Int>`, `Result<Int, Str>`, `Async<Str>`.
- `Infer` — used by closure params that omit a type.

The parser doesn't know what `head` resolves to in a `Generic`; later stages dispatch on the head name (`Option`, `Result`, `Async`, `Handle`, custom user generics).

## Inference

Algorithm J style — constraint accumulation + unification:

- Each unannotated expression starts as a fresh `Ty::Var`.
- Operators, calls, and structural positions emit unification constraints.
- After the walk, any remaining variables are defaulted (`Int` for numeric, etc.) or rejected as `TypeError::CannotInfer`.

Generic functions are *not* `forall`-quantified — each fn admits one specialisation per program. See [`traits.md`](traits.md) for the trait-bound case.

## Opaque type aliases

```rust
type Url = Str
let raw: Str = "https://..."
let url: Url = raw as Url       // wrap
let s:   Str = url as Str       // unwrap
```

`Url` is type-distinct from `Str` for the type checker; without an `as` cast, you can't mix them. At runtime, `as` is a no-op — opaque types erase to their inner representation. See [`modules.md`](modules.md).

## Errors

| Error | Condition |
|---|---|
| `TypeError::Mismatch { expected, found }` | Generic unification failure. |
| `TypeError::CannotInfer` | A variable couldn't be defaulted. |
| `TypeError::InfiniteType` | Occurs check failure (`X = X -> Y`). |
| `TypeError::ArmTypeMismatch` | `match` arms disagree. |
| `TypeError::FieldMismatch` | Struct literal vs. declaration. |
| `TypeError::TraitNotImplemented` | Method call against missing impl. |
| `TypeError::AwaitOnNonAsync` / `AwaitOutsideAsync` | Async-context errors. |
| `TypeError::AsyncNotAwaited` | Async value bound to non-async annotation. |
| `TypeError::SpawnNonAsync` / `JoinNonHandle` | Async intrinsic arg-shape errors. |

Every type error carries a `Span` (Rule 5), so the diagnostics renderer can quote the source.

## Pipeline pointers

- Inference is in `crates/ferric_infer/src/lib.rs`. Every walker over `Ty` (`apply`, `occurs_raw`, `free_vars_raw`, `rename_vars`, `default_remaining_vars`, `try_unify`) must learn about every `Ty` variant — adding one is a sweep.
- Trait constraint resolution lives in the same crate, consulting `&TraitRegistry` from `ferric_traits`.
- The type checker writes results into `TypeResult.node_types: HashMap<NodeId, Ty>`. Downstream stages read this map.

## See also

- [`adts.md`](adts.md) — struct/enum/tuple types.
- [`traits.md`](traits.md) — generics, bounds, method dispatch.
- [`async.md`](async.md) — `Async<T>` / `Handle<T>` / `Poll<T>`.
- [`modules.md`](modules.md) — opaque aliases.
