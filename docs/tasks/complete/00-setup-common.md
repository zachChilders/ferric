# Task: Project Setup and Common Types

## Objective
Set up the Ferric workspace structure and implement the `ferric_common` crate with all shared types that will be used across all pipeline stages.

## Architecture Context
- All stages depend on `ferric_common` but never on each other
- Common types are defined once and never redefined inside stages
- This is the foundation that enables surgical stage replacement

## Deliverables

### 1. Workspace Structure
Create the following directory structure:
```
ferric/
├── Cargo.toml                  (workspace)
├── crates/
│   ├── ferric_common/
│   ├── ferric_lexer/
│   ├── ferric_parser/
│   ├── ferric_resolve/
│   ├── ferric_typecheck/
│   ├── ferric_vm/
│   ├── ferric_diagnostics/
│   └── ferric_stdlib/
└── src/
    └── main.rs
```

### 2. ferric_common Implementation

**Required types:**

```rust
// Location tracking
pub struct Span {
    pub start: u32,
    pub end: u32,
}

// Unique identifiers
pub struct NodeId(pub u32);
pub struct Symbol(pub u32);
pub struct DefId(pub u32);

// String interning
pub struct Interner {
    map: HashMap<String, Symbol>,
    strings: Vec<String>,
}

impl Interner {
    pub fn new() -> Self;
    pub fn intern(&mut self, s: &str) -> Symbol;
    pub fn resolve(&self, sym: Symbol) -> &str;
}

// Stage output types (start with minimal fields, will grow)
pub struct LexResult {
    pub tokens: Vec<Token>,
    pub errors: Vec<LexError>,
}

pub struct ParseResult {
    pub items: Vec<Item>,
    pub errors: Vec<ParseError>,
}

pub struct ResolveResult {
    pub resolutions: HashMap<NodeId, DefId>,
    pub def_slots: HashMap<DefId, u32>,
    pub fn_slots: HashMap<DefId, u32>,
    pub errors: Vec<ResolveError>,
}

pub struct TypeResult {
    pub node_types: HashMap<NodeId, Ty>,
    pub errors: Vec<TypeError>,
}

pub struct Program {
    pub chunks: Vec<Chunk>,
    pub entry: u16,
}
```

**Type system types (M1 baseline):**
```rust
pub enum Ty {
    Int,
    Float,
    Bool,
    Str,
    Unit,
    Fn { params: Vec<Ty>, ret: Box<Ty> },
    Unknown,  // escape hatch for M1, removed in M3
}
```

**Token types:**
```rust
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

pub enum TokenKind {
    // Literals
    IntLit(i64),
    FloatLit(f64),
    StrLit(Symbol),
    True, False,

    // Keywords
    Let, Mut, Fn, Return, If, Else, While, Loop, Break, Continue,

    // Identifiers and operators
    Ident(Symbol),
    Plus, Minus, Star, Slash, Percent,
    Eq, EqEq, Bang, BangEq,
    Lt, LtEq, Gt, GtEq,
    AndAnd, OrOr,

    // Punctuation
    LParen, RParen, LBrace, RBrace, LBracket, RBracket,
    Comma, Colon, Arrow, Semi,

    Eof,
}
```

**Error types (must all carry Span - Rule 5):**
```rust
pub enum LexError {
    UnexpectedChar { ch: char, span: Span },
    UnterminatedString { span: Span },
}

pub enum ParseError {
    UnexpectedToken { expected: String, found: TokenKind, span: Span },
    // ... more variants as needed
}

pub enum ResolveError {
    UndefinedVariable { name: Symbol, span: Span },
    DuplicateDefinition { name: Symbol, first: Span, second: Span },
    // ... more variants as needed
}

pub enum TypeError {
    Mismatch { expected: Ty, found: Ty, span: Span },
    // ... more variants as needed
}
```

## Critical Rules to Enforce

### Rule 5 - Every error carries a Span
Every error type across every stage MUST include a Span. This is non-negotiable and must be enforced from the start. This makes future renderer replacements zero-cost.

### Rule 3 - Common crate is owned by nobody
`ferric_common` must not depend on any other ferric crate. It is a pure dependency that all stages import from.

## Acceptance Criteria
- [ ] Workspace Cargo.toml configured with all crate members
- [ ] ferric_common compiles independently
- [ ] All fundamental types (Span, NodeId, Symbol, DefId, Interner) implemented
- [ ] All stage output types (LexResult, ParseResult, etc.) defined
- [ ] All error types carry Span fields
- [ ] Token and TokenKind enums complete for M1 features
- [ ] Basic Ty enum with Unknown escape hatch
- [ ] No dependencies on other ferric crates

## Notes for Agent
- Keep implementations simple and focused
- Add comprehensive doc comments explaining the purpose of each type
- Use derive macros for Debug, Clone, Copy where appropriate
- Don't over-engineer - this is an MVP foundation
