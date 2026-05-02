# ferric_lexer

Converts a Ferric source string into a flat list of tokens. Stage 1 of the pipeline.

## Entry point

```rust
pub fn lex(source: &str, interner: &mut Interner) -> LexResult
```

`LexResult` carries a `Vec<Token>` and a `Vec<LexError>`. Errors are non-fatal — the lexer keeps scanning after an unknown character so the parser can surface more problems in a single pass.

## Two-phase shell handling

Shell expressions (`$ cmd @{expr} ...`) require special treatment: the outer line is tokenized first, collecting `ShellTokenPart` fragments, then any `@{...}` interpolations inside those fragments are recursively re-lexed as normal Ferric expressions. This keeps the main lexer simple while still supporting arbitrary interpolation.

## Token categories

- **Keywords** — `let`, `fn`, `if`, `while`, `match`, `trait`, `impl`, `async`, `await`, `require`, `import`, `export`, and more
- **Identifiers** — interned to `Symbol` on the spot
- **Literals** — integer, float, string (with escape sequences), boolean
- **Operators** — arithmetic, comparison, logical, `->`, `..`
- **Punctuation** — delimiters, commas, colons, semicolons
- **Shell** — `$` lines including `@{expr}` interpolation spans
