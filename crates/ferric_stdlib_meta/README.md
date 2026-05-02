# ferric_stdlib_meta

Static metadata for every native function in `ferric_stdlib`. Zero runtime dependencies — exists so the resolver, type checker, and LSP can query function signatures without pulling in stdlib's heavy external crates (regex, crypto, HTTP, etc.).

## What it provides

```rust
pub struct FunctionMeta {
    pub name: &'static str,
    pub params: &'static [&'static str],   // parameter names (for named-arg resolution)
    pub signature: Signature,               // type info
    pub display: &'static str,             // hover string for LSP
}
```

`ALL_MODULES: &[&[FunctionMeta]]` lists every module's metadata in the same registration order as `ferric_stdlib`, so the two crates stay in sync by construction.

## Signature kinds

```rust
pub enum Signature {
    Unknown,                                // no type info; return type stays as free var
    Mono(fn() -> (Vec<Ty>, Ty)),            // concrete, monomorphic signature
    Poly(fn(&mut dyn TyVarGen) -> TypeScheme), // polymorphic (e.g. list::map, set::insert)
}
```

`Poly` receives a `TyVarGen` so it can allocate fresh type variables — the same mechanism `ferric_infer` uses for user-defined generics.

## Built-in types

- `builtin_enums::OPTION_ENUM` — definition of `Option<T>`
- `builtin_enums::RESULT_ENUM` — definition of `Result<T, E>`
- `async_intrinsics::ASYNC_INTRINSICS` — `spawn`, `join`, `block_on`, `await`

These are pre-registered by `ferric_resolve` before user code is analysed.

## Mirrors

Each module in `ferric_stdlib_meta` corresponds to a module in `ferric_stdlib`. When adding a new native function, both places need updating — the metadata table and the runtime implementation.
