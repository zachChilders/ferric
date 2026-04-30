# Traits, Impls, Generics

User-defined traits, `impl` blocks, and generic functions with trait bounds.

## Trait declaration

```rust
trait Describable {
    fn describe(self) -> Str
}
```

Trait methods declare their signature with no body. The first parameter must be `self`; its type is bound to the implementing type at impl time.

## Impl block

```rust
impl Describable for Int {
    fn describe(self) -> Str {
        "I am an integer: " + int_to_str(n: self)
    }
}
```

Each impl method has its own `DefId`, its own `fn_slots` entry, and its own bytecode chunk. The compiler keys method chunks via `method_chunks: HashMap<DefId, u16>`.

A type can implement multiple traits; each impl block targets a single trait + type pair.

## Generic functions with bounds

```rust
fn print_description<T: Describable>(val: T) {
    println(s: val.describe())
}

print_description(val: 42)
```

Bounds are declared via `<T: Trait>` or `<T: TraitA, U: TraitB>`. Multiple bounds on one type parameter use `+`:

```rust
fn show<T: Describable + Comparable>(val: T) { ... }
```

## Method-call syntax

```rust
val.describe()
"hello".len()        // only if a trait/impl provides it
```

Method calls lower to ordinary `Op::Call` with the receiver as the first argument. There is no separate `MethodCall` opcode.

For type-variable receivers (inside a generic body), method dispatch resolves in a **post-pass** after the global type substitution settles. Concrete-receiver impls resolve immediately during inference. The result lands in `types.method_dispatch: HashMap<NodeId, DefId>`, which the compiler reads.

## Built-in trait impls

The trait registry pre-registers `Display` impls for `Int`, `Float`, `Str`, and `Bool` at startup. (M5 — see `crates/ferric_traits/src/lib.rs`.)

## Limitations to keep in mind

- **No quantified generics.** Generic functions are *not* `forall`-quantified; each fn admits one specialisation per program. This is a deliberate M5 limitation. True monomorphisation would require either runtime trait dispatch or a specialisation pass.
- **No higher-kinded traits.** A trait method must reference concrete types or `self`; you can't write `trait Functor<F<_>>`.
- **No associated types or constants** in traits — only methods.
- **No default method bodies** in trait declarations. If you want a default, it has to live in each impl.

## Errors

| Error | Condition |
|---|---|
| `TypeError::TraitMethodMissing` | An impl block omits a trait method. |
| `TypeError::TraitNotImplemented` | A method call on a type that doesn't implement the required trait. |
| `TypeError::AmbiguousMethod` | Two impls in scope satisfy the call site. |

## Pipeline pointers

- `ferric_traits::build_registry` runs after resolution, before typecheck.
- `ferric_infer::typecheck` consumes `&TraitRegistry` (added M5).
- The compiler reads `types.method_dispatch`, **not** the registry — it doesn't need to know about traits qua traits, only the resolved `DefId` for each method call.

## See also

- [`adts.md`](adts.md) — traits work with structs and enums you define.
- [`../architecture.md`](../architecture.md) — milestone M5 covers the trait machinery.
