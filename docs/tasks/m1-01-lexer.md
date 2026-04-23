# Task: M1 Lexer Implementation

## Objective
Implement the `ferric_lexer` crate with a simple, correct lexer that tokenizes Ferric source code for Milestone 1 features.

## Architecture Context
- The lexer is the first stage in the pipeline
- It must expose exactly one public function: `pub fn lex(...) -> LexResult`
- All internal helpers and data structures must be private
- This implementation may be replaced in future milestones, so keep it simple

## Public Interface (Non-Negotiable)

```rust
// ferric_lexer/src/lib.rs
pub fn lex(source: &str, interner: &mut Interner) -> LexResult;
```

## Feature Requirements

### Tokens to Support (M1)
- **Literals:** integer literals, string literals, booleans (`true`, `false`)
- **Keywords:** `let`, `fn`, `return`, `if`, `else`, `true`, `false`
- **Identifiers:** any valid identifier (start with letter or underscore, continue with alphanumeric or underscore)
- **Operators:** `+`, `-`, `*`, `/`, `%`, `=`, `==`, `!=`, `<`, `>`, `<=`, `>=`, `!`
- **Punctuation:** `(`, `)`, `{`, `}`, `,`, `:`, `->`, `;`
- **Comments:** single-line comments starting with `//` (skip them, don't emit tokens)

### Error Handling (Rule 5 - Every error carries a Span)
Never panic. Accumulate errors in `LexResult.errors` and continue lexing.

Error types to emit:
```rust
LexError::UnexpectedChar { ch, span }
LexError::UnterminatedString { span }
```

### String Interning
All identifiers and string literals must be interned using the provided `Interner`.
Return `Symbol` handles, not raw strings.

## Implementation Notes

### Lexer Structure
```rust
struct Lexer<'a> {
    source: &'a str,
    chars: std::str::Chars<'a>,
    current: Option<char>,
    position: u32,
    interner: &'a mut Interner,
    tokens: Vec<Token>,
    errors: Vec<LexError>,
}

impl<'a> Lexer<'a> {
    fn new(source: &'a str, interner: &'a mut Interner) -> Self;
    fn advance(&mut self) -> Option<char>;
    fn peek(&self) -> Option<char>;
    fn skip_whitespace(&mut self);
    fn skip_comment(&mut self);
    fn lex_number(&mut self) -> Token;
    fn lex_string(&mut self) -> Token;
    fn lex_identifier_or_keyword(&mut self) -> Token;
    fn lex_token(&mut self) -> Option<Token>;
}
```

### Keyword Recognition
Use a simple match or HashMap to distinguish keywords from identifiers:
```rust
fn keyword_or_ident(word: &str, interner: &mut Interner) -> TokenKind {
    match word {
        "let" => TokenKind::Let,
        "fn" => TokenKind::Fn,
        "return" => TokenKind::Return,
        "if" => TokenKind::If,
        "else" => TokenKind::Else,
        "true" => TokenKind::True,
        "false" => TokenKind::False,
        _ => TokenKind::Ident(interner.intern(word)),
    }
}
```

### Number Parsing
For M1, only parse integers. Use `str::parse::<i64>()`.
If parsing fails, emit an error and continue.

### String Parsing
Handle escape sequences: `\n`, `\t`, `\\`, `\"`
Track whether the string was properly terminated.
If unterminated, emit `LexError::UnterminatedString` and continue.

### Span Tracking
Every token must carry an accurate `Span { start, end }`.
Track `position` as you advance through characters.

## Test Cases

Create unit tests for:
1. Empty input produces only Eof
2. Simple arithmetic: `1 + 2`
3. Function definition: `fn foo() { }`
4. Let binding: `let x = 5`
5. String literal: `"hello world"`
6. Unterminated string produces error but continues
7. Unexpected character produces error but continues
8. Comments are skipped
9. Keywords are recognized correctly
10. Multi-character operators: `->`, `==`, `!=`, `<=`, `>=`

## Acceptance Criteria
- [ ] Public API is exactly `pub fn lex(source: &str, interner: &mut Interner) -> LexResult`
- [ ] All M1 token types are lexed correctly
- [ ] Single-line comments are skipped
- [ ] String literals are interned
- [ ] Identifiers are interned
- [ ] Keywords are recognized
- [ ] All errors carry accurate Spans
- [ ] Lexer never panics - all errors are accumulated
- [ ] All unit tests pass
- [ ] No public exports other than `lex` function

## Critical Rules to Enforce

### Rule 2 - Exactly one public entry point
Only `lex()` should be public. All helpers are private.

### Rule 5 - Every error carries a Span
Every `LexError` variant must include a `Span` field.

### Rule 4 - No mutable global state
The `Interner` is passed in and threaded through, not stored in a global.

## Notes for Agent
- Prioritize correctness over performance
- Keep the implementation simple and readable
- Use clear variable names and add comments for non-obvious logic
- This is an MVP - don't over-engineer
- Make sure Span tracking is accurate - this will be critical for error reporting
