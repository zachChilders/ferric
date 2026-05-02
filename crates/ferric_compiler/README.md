# ferric_compiler

Lowers a type-checked AST to bytecode. Stage 10 of the pipeline.

## Entry point

```rust
pub fn compile(
    ast: &ParseResult,
    resolve: &ResolveResult,
    types: &TypeResult,
    interner: &Interner,
) -> Program
```

## Output

`Program` contains a `Vec<Chunk>` (one per function, closure, async body, or impl method) and an `entry: u16` index pointing to the top-level script chunk.

Each `Chunk` holds:
- `name: Symbol` — function name (for stack traces)
- `code: Vec<Op>` — bytecode instructions
- `constants: Vec<Constant>` — literal pool (`Int`, `Float`, `Bool`, `Str`, `Fn`, `NativeFn`)

## Instruction set

The `Op` enum covers ~90 opcodes across these groups:

| Group | Examples |
|---|---|
| Literals | `LoadConst`, `Unit` |
| Variables | `LoadSlot`, `StoreSlot` |
| Arithmetic | `AddInt`, `SubFloat`, `RemInt`, … |
| Comparison | `EqInt`, `LtFloat`, `NeStr`, … |
| Logic | `AndBool`, `OrBool`, `Not` |
| Control flow | `Jump`, `JumpIfFalse`, `Return` |
| Arrays | `MakeArray`, `ArrayGet`, `ArraySet`, `ArrayLen` |
| Structs | `MakeStruct`, `GetField`, `GetTupleField` |
| Enums | `MakeVariant`, `MatchVariant`, `UnpackVariant` |
| Functions | `Call`, `MakeClosure`, `MakeAsync` |
| Shell | `Concat` |
| Require | `RequireFail`, `RequireWarn` |

## Closure and async hoisting

Closure bodies and `async` bodies are lifted into their own chunks during compilation. The enclosing chunk emits a `MakeClosure` or `MakeAsync` instruction that references the hoisted chunk by index and bundles the captured variable values.
