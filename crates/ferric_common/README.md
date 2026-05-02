# ferric_common

Shared types used by every stage in the Ferric pipeline. This crate has no logic of its own — it exists so stages can communicate through well-defined output structs without depending on each other.

## What lives here

| Module | Contents |
|---|---|
| `span` | `Span { start, end }` — byte offsets into the source string |
| `identifiers` | `NodeId`, `Symbol`, `DefId` — typed integer handles for AST nodes, interned strings, and variable definitions |
| `interner` | `Interner` — maps `&str` ↔ `Symbol`; passed through the whole pipeline |
| `tokens` | `Token`, `TokenKind`, `ShellTokenPart` — produced by the lexer |
| `ast` | All expression, statement, item, and pattern types |
| `types` | `Ty`, `TyVar`, `TypeScheme`, `TypeAnnotation` — the type language |
| `bytecode` | `Program`, `Chunk`, `Op`, `Constant` — the bytecode schema |
| `errors` | `LexError`, `ParseError`, `ResolveError`, `TypeError`, `ExhaustivenessError` |
| `results` | `LexResult`, `ParseResult`, `ResolveResult`, `TypeResult`, `ExhaustivenessResult`, `AsyncResult` |
| `traits` | `TraitRegistry`, `TraitDef`, `ImplDef`, `MethodSignature` |
| `module` | `ModuleResult`, `ResolvedImport` |

## Architecture contract

All types that cross a stage boundary must be `Send + Sync`. This is enforced at compile time with a const block in `lib.rs` that instantiates every exported type. Adding a non-`Send` type to any of these structs is a compile error.

## Utilities

```rust
// Serialize/deserialize a ParseResult for tooling (AST dump, LSP caching)
let json = ast_to_json(&parse_result);
let restored = ast_from_json(&json);
```

`SHELL_EXEC_NATIVE` is a stable string constant for the native function that runs shell expressions; it's referenced by name at the call site in `ferric_vm` so a rename never silently breaks shell execution.
