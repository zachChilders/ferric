# Task: M1 Diagnostics Implementation

## Objective
Implement the `ferric_diagnostics` crate with a minimal error renderer. For M1, just print line numbers. This will be completely replaced in M2 with span-annotated rendering.

## Architecture Context
- Diagnostics is the final stage before output
- It renders errors from all previous stages
- This is intentionally minimal for M1 - just "error at line N: message"
- Will be completely replaced in M2, demonstrating architecture rule benefits

## Public Interface (Non-Negotiable)

```rust
// ferric_diagnostics/src/lib.rs

pub struct Renderer {
    source: String,
}

impl Renderer {
    pub fn new(source: String) -> Self;

    pub fn render_lex_error(&self, error: &LexError) -> String;
    pub fn render_parse_error(&self, error: &ParseError) -> String;
    pub fn render_resolve_error(&self, error: &ResolveError) -> String;
    pub fn render_type_error(&self, error: &TypeError) -> String;
    pub fn render_runtime_error(&self, error: &RuntimeError) -> String;
}
```

## Feature Requirements

### M1 Output Format
Very simple for M1:
```
error at line 5: undefined variable `x`
error at line 10: type mismatch: expected Int, found Str
```

### Span to Line Number Conversion
Track newlines in the source to convert `Span` to line numbers:

```rust
impl Renderer {
    fn new(source: String) -> Self {
        // Pre-compute line start positions for O(log n) lookup
        Self { source }
    }

    fn span_to_line(&self, span: Span) -> usize {
        // Count newlines up to span.start
        self.source[..span.start as usize]
            .chars()
            .filter(|&c| c == '\n')
            .count() + 1
    }
}
```

### Error Rendering
For each error type, extract the message and span:

```rust
pub fn render_lex_error(&self, error: &LexError) -> String {
    match error {
        LexError::UnexpectedChar { ch, span } => {
            format!("error at line {}: unexpected character '{}'",
                    self.span_to_line(*span), ch)
        }
        LexError::UnterminatedString { span } => {
            format!("error at line {}: unterminated string literal",
                    self.span_to_line(*span))
        }
    }
}
```

Similar for all other error types.

## Implementation Notes

### Renderer Structure
```rust
pub struct Renderer {
    source: String,
}

impl Renderer {
    pub fn new(source: String) -> Self {
        Self { source }
    }

    fn span_to_line(&self, span: Span) -> usize {
        // Implementation above
    }

    pub fn render_lex_error(&self, error: &LexError) -> String { ... }
    pub fn render_parse_error(&self, error: &ParseError) -> String { ... }
    pub fn render_resolve_error(&self, error: &ResolveError) -> String { ... }
    pub fn render_type_error(&self, error: &TypeError) -> String { ... }
    pub fn render_runtime_error(&self, error: &RuntimeError) -> String { ... }
}
```

### Error Message Guidelines
- Keep messages clear and concise
- Include relevant information (variable name, expected/found types)
- Use consistent formatting
- Don't worry about colors or fancy formatting yet

## Test Cases

Create unit tests for:
1. Single-line source: span at position 5 is line 1
2. Multi-line source: span after first newline is line 2
3. Span at position 0 is line 1
4. LexError renders correctly
5. ParseError renders correctly
6. ResolveError renders correctly
7. TypeError renders correctly
8. All error variants produce output

## Acceptance Criteria
- [ ] `Renderer` struct created with `new()` method
- [ ] All error types from all stages can be rendered
- [ ] Span to line number conversion works correctly
- [ ] Error messages are clear and informative
- [ ] All unit tests pass
- [ ] Only `Renderer` is public

## Critical Rules to Enforce

### Rule 5 - Every error carries a Span
All error types already have Spans (from stage implementations).
The renderer must use these Spans - never reach into stage internals.

This is the payoff: when this crate is replaced in M2, no other stage changes.

## Notes for Agent
- This is intentionally simple - don't over-engineer
- M2 will completely replace this with rich span annotations
- The point is to prove the architecture works
- Focus on correctness of line number calculation
- Document clearly that this will be replaced in M2
- Make sure all error types are handled
