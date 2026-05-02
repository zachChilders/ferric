# ferric_traits

Builds the `TraitRegistry` consumed by `ferric_infer`. Stage 6 of the pipeline.

## Entry point

```rust
pub fn build_registry(
    ast: &ParseResult,
    resolve: &ResolveResult,
    interner: &Interner,
) -> TraitRegistry
```

## What it does

Walks the AST for `TraitDef` and `ImplBlock` items and indexes them into two tables:

- **`traits: HashMap<Symbol, TraitDef>`** — trait name → trait declaration (method signatures, type parameter constraints).
- **`impls: HashMap<(Symbol, ImplTy), ImplDef>`** — `(trait_name, for_type)` → impl declaration (concrete method definitions, `DefId` mappings).

The registry is the only place that knows which concrete types implement which traits. `ferric_infer` queries it during constraint solving to resolve a `T: SomeTrait` bound to a specific `impl` and to look up the `DefId` of the dispatched method.

## Why a separate crate

Separating registry construction from type inference keeps `ferric_infer` focused on the inference algorithm. The registry is also consumed by `ferric_lsp` for completion and hover without pulling in the full inference engine.
