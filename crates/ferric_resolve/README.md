# ferric_resolve

Name resolution and scope analysis. Stage 5 of the pipeline (after manifest loading and module resolution).

## Entry points

```rust
// Minimal — for tests or tools that don't need stdlib
pub fn resolve(ast: &ParseResult) -> ResolveResult

// Standard — with stdlib native function names pre-registered
pub fn resolve_with_natives(ast: &ParseResult, native_fns: &[(Symbol, Vec<Symbol>)]) -> ResolveResult

// With built-in enum types (Option, Result) pre-registered
pub fn resolve_with_natives_and_builtins(ast, native_fns, builtin_enums) -> ResolveResult

// With imported symbols from ferric_module
pub fn resolve_with_imports(ast, native_fns, module) -> ResolveResult

// Full — used by main.rs
pub fn resolve_with_imports_and_builtins(ast, native_fns, builtin_enums, module) -> ResolveResult
```

## What the resolver produces

`ResolveResult` is a collection of side-tables keyed by `NodeId` or `DefId`:

| Field | Meaning |
|---|---|
| `resolutions` | `NodeId → DefId` — every variable *use* mapped to its definition |
| `def_slots` | `DefId → u32` — stack slot for each local variable |
| `fn_slots` | `DefId → u32` — constant-pool slot for each function |
| `canonical_call_args` | `NodeId → Vec<NamedArg>` — call arguments reordered to definition order |
| `captures` | `NodeId → Vec<(DefId, Symbol)>` — closed-over variables per closure |
| `type_defs` | `Symbol → DefId` — struct/enum/trait names |
| `struct_fields`, `enum_variants` | Metadata for struct construction and pattern matching |
| `defs` | `DefId → DefInfo` — names and source spans (used by LSP) |
| `errors` | All resolution errors with spans |

## Named argument canonicalisation

The resolver reorders call-site arguments to match definition order before the type checker or VM ever see them. This means `fibonacci(n: 10)` and position-based calls are handled in one place, and the rest of the pipeline always sees arguments in declaration order.
