# ferric_parser

Turns the token stream from `ferric_lexer` into an Abstract Syntax Tree. Stage 2 of the pipeline.

## Entry points

```rust
pub fn parse(lex_result: &LexResult) -> ParseResult
pub fn parse_with_interner(lex_result: &LexResult, interner: &Interner) -> ParseResult
```

`ParseResult` contains a `Vec<Item>` (top-level declarations) and a `Vec<ParseError>`. The parser is error-recovering: on a syntax error it skips tokens until it finds a synchronisation point (typically a statement boundary) and continues, so a single run surfaces many errors at once.

## AST shape

**Items** (top level): `Fn`, `AsyncFn`, `StructDef`, `EnumDef`, `TraitDef`, `ImplBlock`, `Script`, `Import`, `Export`, `TypeAlias`

**Expressions**: `Literal`, `Variable`, `Binary`, `Unary`, `Call`, `If`, `Block`, `Return`, `While`, `Loop`, `Break`, `Continue`, `Closure`, `Shell`, `StructLit`, `FieldAccess`, `Match`, `Tuple`, `VariantCtor`, `MethodCall`, `ArrayLit`, `Index`, `Cast`, `Await`, `AsyncBlock`

**Statements**: `Let`, `Assign`, `Expr`, `Require`, `For`

**Patterns** (in `match`/`let`): `Wildcard`, `Variable`, `Literal`, `Tuple`, `Struct`, `Variant`

Every node is stamped with a `NodeId` — a unique `u32` assigned in parse order. Later stages use these IDs as keys into side-tables (`HashMap<NodeId, Ty>`, `HashMap<NodeId, DefId>`, etc.) without embedding data into the AST itself.
