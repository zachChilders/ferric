# LSP — Task 1: ferric_common additions

> **Do this task first.** It changes the public surface of `ferric_common` (a new
> module + a `Display` impl) and refactors `ferric_lexer` internals. Every other
> LSP task depends on the `keywords` module and the `Display for Ty` impl.

---

## Goal

Add a single source of truth for keywords/operators in `ferric_common` so both
the lexer (at runtime) and `build.rs` (at build time) consume the same data.
Add a `Display` impl for `Ty` so the LSP can render types in hover and inlay
hints. Refactor the lexer's internal keyword matcher to read from the new module
instead of duplicating string literals.

The lexer's public signature
`pub fn lex(source: &str, interner: &mut Interner) -> LexResult` is unchanged.

---

## Files

### Create — `crates/ferric_common/src/keywords.rs`

```rust
//! Single source of truth for Ferric keywords, type keywords, and operators.
//!
//! Consumed by the lexer (at runtime) and by `ferric_lsp/build.rs` (at build
//! time). Adding a keyword here automatically updates the TextMate grammar on
//! the next `cargo build`.

pub const KEYWORDS: &[&str] = &[
    "let", "mut", "fn", "return",
    "if", "else", "while", "loop",
    "break", "continue", "true", "false",
    "require",
];

pub const TYPE_KEYWORDS: &[&str] = &[
    "Int", "Float", "Bool", "Str", "Unit",
];

pub const OPERATORS: &[&str] = &[
    "+", "-", "*", "/", "%",
    "==", "!=", "<", ">", "<=", ">=",
    "&&", "||", "!",
    "=",
];
```

### Modify — `crates/ferric_common/src/lib.rs`

Add `pub mod keywords;` next to the other `pub mod` declarations.

### Modify — `crates/ferric_common/src/types.rs` (or wherever `Ty` is defined)

Add the following `Display` impl alongside the `Ty` definition. Match on **every**
current variant explicitly — the goal is that adding a new `Ty` variant in a
future milestone fails to compile until a `Display` arm is added.

```rust
impl std::fmt::Display for Ty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Ty::Int           => write!(f, "Int"),
            Ty::Float         => write!(f, "Float"),
            Ty::Bool          => write!(f, "Bool"),
            Ty::Str           => write!(f, "Str"),
            Ty::Unit          => write!(f, "Unit"),
            Ty::Fn { params, ret } => {
                write!(f, "fn(")?;
                for (i, p) in params.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{p}")?;
                }
                write!(f, ") -> {ret}")
            }
            Ty::Unknown => write!(f, "_"),
        }
    }
}
```

If `Ty` has additional variants in the current codebase (e.g. M2.5 added more),
add an arm for each. Do **not** use a wildcard `_ =>` arm — exhaustiveness is
the safety mechanism.

### Modify — `crates/ferric_lexer/` (internal only)

Find the place where the lexer matches identifier text against keyword strings.
Replace any duplicated keyword string array with `ferric_common::keywords::KEYWORDS`.

```rust
use ferric_common::keywords::KEYWORDS;

fn classify_ident(text: &str) -> TokenKind {
    if KEYWORDS.contains(&text) {
        TokenKind::Keyword(text.to_string())  // or however the lexer represents it today
    } else {
        TokenKind::Ident
    }
}
```

The exact API depends on the current lexer implementation — match the existing
internal style. The only requirement is that the keyword **string list** comes
from `ferric_common::keywords::KEYWORDS` rather than a literal in the lexer.

---

## Done when

- [ ] `crates/ferric_common/src/keywords.rs` exists with `KEYWORDS`,
      `TYPE_KEYWORDS`, `OPERATORS` as `pub const &[&str]` slices
- [ ] `keywords` is re-exported as `pub mod keywords;` in `ferric_common/src/lib.rs`
- [ ] `impl std::fmt::Display for Ty` exists, has an arm for every current
      variant, uses no wildcard arm
- [ ] `format!("{}", Ty::Int)` returns `"Int"`; round-trip works for every variant
- [ ] `ferric_lexer` references `ferric_common::keywords::KEYWORDS` instead of
      duplicating keyword strings
- [ ] `ferric_lexer`'s public signature is unchanged
- [ ] `cargo test` passes for all crates
- [ ] Adding a fictional new variant to `Ty` (then reverting) produces a
      compile error pointing at the `Display` impl — confirming exhaustiveness
