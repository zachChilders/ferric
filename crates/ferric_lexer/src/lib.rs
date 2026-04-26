//! Lexer for the Ferric programming language.
//!
//! This crate provides a single public entry point: the `lex` function,
//! which converts source code into a stream of tokens.

use ferric_common::{
    Interner, LexError, LexResult, ShellTokenPart, Span, Token, TokenKind,
};

/// Raw intermediate shell part used during phase-1 lexing. Phase 2 converts
/// `Interpolated` raw parts into `ShellTokenPart::Interpolated(Vec<Token>)`.
enum RawShellPart {
    Literal(String),
    Interpolated {
        /// Source text inside the `@{...}` (not including the delimiters).
        content: String,
        /// Byte offset of `content` within the original source.
        start_offset: u32,
    },
}

/// Outcome of scanning a single `@{...}` interpolation.
enum ShellInterpResult {
    /// Successful collection — content is the inner source.
    Ok(String),
    /// A nested `@{` was encountered and recovered from. The outer segment
    /// is consumed up to its matching `}` but no part should be emitted.
    Recovered,
    /// The interpolation ran past end-of-line without a closing `}`.
    Unclosed,
}

/// Lexes the source code into tokens.
///
/// This is the only public API for the lexer. It takes source code and an interner,
/// and returns all tokens along with any lexical errors encountered.
pub fn lex(source: &str, interner: &mut Interner) -> LexResult {
    // Phase 1: main lex. Collect normal tokens plus raw shell-line parts.
    let (mut tokens, mut errors, raw_shell_parts) = {
        let mut lexer = Lexer::new(source, interner);
        lexer.lex_all();
        (lexer.tokens, lexer.errors, lexer.raw_shell_parts)
    };

    // Phase 2: for each recorded shell-line placeholder, recursively lex
    // the `@{...}` interpolations and splice them into the final token.
    for (tok_idx, raw_parts) in raw_shell_parts {
        let mut cooked_parts: Vec<ShellTokenPart> = Vec::new();
        for part in raw_parts {
            match part {
                RawShellPart::Literal(s) => cooked_parts.push(ShellTokenPart::Literal(s)),
                RawShellPart::Interpolated { content, start_offset } => {
                    let sub_result = lex(&content, interner);
                    for err in sub_result.errors {
                        errors.push(offset_lex_error(err, start_offset));
                    }
                    let sub_tokens: Vec<Token> = sub_result
                        .tokens
                        .into_iter()
                        .filter(|t| !matches!(t.kind, TokenKind::Eof))
                        .map(|t| offset_token(t, start_offset))
                        .collect();
                    cooked_parts.push(ShellTokenPart::Interpolated(sub_tokens));
                }
            }
        }
        tokens[tok_idx].kind = TokenKind::ShellLine(cooked_parts);
    }

    LexResult::new(tokens, errors)
}

/// Adjusts all span offsets in a token (and any nested sub-tokens) by `offset`.
fn offset_token(tok: Token, offset: u32) -> Token {
    let Token { kind, span } = tok;
    let kind = match kind {
        TokenKind::ShellLine(parts) => {
            let parts = parts
                .into_iter()
                .map(|part| match part {
                    ShellTokenPart::Literal(s) => ShellTokenPart::Literal(s),
                    ShellTokenPart::Interpolated(toks) => ShellTokenPart::Interpolated(
                        toks.into_iter().map(|t| offset_token(t, offset)).collect(),
                    ),
                })
                .collect();
            TokenKind::ShellLine(parts)
        }
        other => other,
    };
    Token {
        kind,
        span: Span::new(span.start + offset, span.end + offset),
    }
}

/// Adjusts the span on a `LexError` by `offset`.
fn offset_lex_error(err: LexError, offset: u32) -> LexError {
    let shift = |s: Span| Span::new(s.start + offset, s.end + offset);
    match err {
        LexError::UnexpectedChar { ch, span } => LexError::UnexpectedChar { ch, span: shift(span) },
        LexError::UnterminatedString { span } => LexError::UnterminatedString { span: shift(span) },
        LexError::NestedShellInterp { span } => LexError::NestedShellInterp { span: shift(span) },
        LexError::UnclosedShellInterp { span } => LexError::UnclosedShellInterp { span: shift(span) },
    }
}

/// Internal lexer implementation.
///
/// All members are private - only the public `lex` function is exposed.
struct Lexer<'a> {
    /// Character iterator
    chars: std::iter::Peekable<std::str::Chars<'a>>,
    /// Current byte position in source
    position: u32,
    /// String interner for identifiers and string literals
    interner: &'a mut Interner,
    /// Accumulated tokens
    tokens: Vec<Token>,
    /// Accumulated errors
    errors: Vec<LexError>,
    /// Side-channel: `(token_index, raw_parts)` for shell-line placeholders
    /// awaiting sub-lexing of their `@{...}` interpolations in phase 2.
    raw_shell_parts: Vec<(usize, Vec<RawShellPart>)>,
}

impl<'a> Lexer<'a> {
    /// Creates a new lexer for the given source code.
    fn new(source: &'a str, interner: &'a mut Interner) -> Self {
        Self {
            chars: source.chars().peekable(),
            position: 0,
            interner,
            tokens: Vec::new(),
            errors: Vec::new(),
            raw_shell_parts: Vec::new(),
        }
    }

    /// Advances to the next character and returns it.
    fn advance(&mut self) -> Option<char> {
        let ch = self.chars.next()?;
        self.position += ch.len_utf8() as u32;
        Some(ch)
    }

    /// Peeks at the next character without consuming it.
    fn peek(&mut self) -> Option<char> {
        self.chars.peek().copied()
    }

    /// Skips whitespace characters.
    fn skip_whitespace(&mut self) {
        while let Some(ch) = self.peek() {
            if ch.is_whitespace() {
                self.advance();
            } else {
                break;
            }
        }
    }

    /// Skips a single-line comment (from `//` to end of line).
    fn skip_comment(&mut self) {
        // Skip the leading '//'
        self.advance();
        self.advance();

        // Skip until newline or EOF
        while let Some(ch) = self.peek() {
            if ch == '\n' {
                break;
            }
            self.advance();
        }
    }

    /// Lexes a number literal (integer or float).
    fn lex_number(&mut self, start: u32) -> Token {
        let mut num_str = String::new();
        let mut is_float = false;

        // Lex the integer part
        while let Some(ch) = self.peek() {
            if ch.is_ascii_digit() {
                num_str.push(ch);
                self.advance();
            } else {
                break;
            }
        }

        // Check for decimal point
        if self.peek() == Some('.') {
            // Look ahead to see if there's a digit after the '.'
            let mut chars_clone = self.chars.clone();
            chars_clone.next(); // Skip the '.'
            if let Some(next_ch) = chars_clone.peek() {
                if next_ch.is_ascii_digit() {
                    // It's a float
                    is_float = true;
                    num_str.push('.');
                    self.advance();

                    // Lex the fractional part
                    while let Some(ch) = self.peek() {
                        if ch.is_ascii_digit() {
                            num_str.push(ch);
                            self.advance();
                        } else {
                            break;
                        }
                    }
                }
            }
        }

        let end = self.position;
        let span = Span::new(start, end);

        // Parse as float or integer
        if is_float {
            match num_str.parse::<f64>() {
                Ok(value) => Token::new(TokenKind::FloatLit(value), span),
                Err(_) => {
                    self.errors.push(LexError::UnexpectedChar {
                        ch: num_str.chars().next().unwrap_or('0'),
                        span,
                    });
                    Token::new(TokenKind::FloatLit(0.0), span)
                }
            }
        } else {
            match num_str.parse::<i64>() {
                Ok(value) => Token::new(TokenKind::IntLit(value), span),
                Err(_) => {
                    self.errors.push(LexError::UnexpectedChar {
                        ch: num_str.chars().next().unwrap_or('0'),
                        span,
                    });
                    Token::new(TokenKind::IntLit(0), span)
                }
            }
        }
    }

    /// Lexes a string literal with escape sequence support.
    fn lex_string(&mut self, start: u32) -> Token {
        // Skip opening quote
        self.advance();

        let mut string_content = String::new();
        let mut terminated = false;

        while let Some(ch) = self.peek() {
            if ch == '"' {
                // Found closing quote
                self.advance();
                terminated = true;
                break;
            } else if ch == '\\' {
                // Handle escape sequence
                self.advance();
                if let Some(escaped) = self.peek() {
                    let escaped_char = match escaped {
                        'n' => '\n',
                        't' => '\t',
                        '\\' => '\\',
                        '"' => '"',
                        _ => escaped, // For now, just use the character as-is
                    };
                    string_content.push(escaped_char);
                    self.advance();
                } else {
                    // EOF after backslash
                    break;
                }
            } else if ch == '\n' {
                // Newline in string literal (unterminated)
                break;
            } else {
                string_content.push(ch);
                self.advance();
            }
        }

        let end = self.position;
        let span = Span::new(start, end);

        if !terminated {
            self.errors.push(LexError::UnterminatedString { span });
        }

        let symbol = self.interner.intern(&string_content);
        Token::new(TokenKind::StrLit(symbol), span)
    }

    /// Lexes an identifier or keyword.
    fn lex_identifier_or_keyword(&mut self, start: u32) -> Token {
        let mut ident = String::new();

        while let Some(ch) = self.peek() {
            if ch.is_alphanumeric() || ch == '_' {
                ident.push(ch);
                self.advance();
            } else {
                break;
            }
        }

        let end = self.position;
        let span = Span::new(start, end);

        // Check if it's a keyword
        let kind = match ident.as_str() {
            "let" => TokenKind::Let,
            "mut" => TokenKind::Mut,
            "fn" => TokenKind::Fn,
            "return" => TokenKind::Return,
            "if" => TokenKind::If,
            "else" => TokenKind::Else,
            "while" => TokenKind::While,
            "loop" => TokenKind::Loop,
            "break" => TokenKind::Break,
            "continue" => TokenKind::Continue,
            "require" => TokenKind::Require,
            "struct" => TokenKind::Struct,
            "enum" => TokenKind::Enum,
            "match" => TokenKind::Match,
            "trait" => TokenKind::Trait,
            "impl" => TokenKind::Impl,
            "for" => TokenKind::For,
            "import" => TokenKind::Import,
            "export" => TokenKind::Export,
            "from" => TokenKind::From,
            "type" => TokenKind::Type,
            "as" => TokenKind::As,
            "true" => TokenKind::True,
            "false" => TokenKind::False,
            "_" => TokenKind::Underscore,
            _ => {
                // It's an identifier, intern it
                let symbol = self.interner.intern(&ident);
                TokenKind::Ident(symbol)
            }
        };

        Token::new(kind, span)
    }

    /// Lexes a `$` shell expression, emitting a placeholder `ShellLine` token
    /// and recording the raw parts for phase-2 sub-lexing.
    fn lex_shell_line(&mut self, start: u32) -> Token {
        // Caller has already consumed the `$`.
        let mut parts: Vec<RawShellPart> = Vec::new();
        let mut current_literal = String::new();

        loop {
            match self.peek() {
                None => break,       // EOF
                Some('\n') => break, // End of line (no continuation)
                Some('\\') => {
                    // Check for `\` line continuation: `\` followed by optional
                    // whitespace and then `\n`.
                    let mut look = self.chars.clone();
                    look.next(); // skip the `\`
                    let mut ws_count = 0usize;
                    let mut is_continuation = false;
                    loop {
                        match look.peek().copied() {
                            Some(' ') | Some('\t') => {
                                look.next();
                                ws_count += 1;
                            }
                            Some('\n') => {
                                is_continuation = true;
                                break;
                            }
                            _ => break,
                        }
                    }
                    if is_continuation {
                        // Consume `\`, any trailing ws, and the `\n`.
                        self.advance(); // `\`
                        for _ in 0..ws_count {
                            self.advance();
                        }
                        self.advance(); // `\n`
                        // Continue lexing the next physical line as part of
                        // the same logical shell line. Leading whitespace is
                        // preserved in the emitted literal.
                    } else {
                        // Literal backslash — pass through to the shell.
                        current_literal.push('\\');
                        self.advance();
                    }
                }
                Some('@') => {
                    // Check for `@{` interpolation marker.
                    let mut look = self.chars.clone();
                    look.next(); // skip `@`
                    if look.peek() == Some(&'{') {
                        let interp_start = self.position;
                        self.advance(); // `@`
                        self.advance(); // `{`
                        let content_start = self.position;
                        match self.collect_shell_interp_content(interp_start) {
                            ShellInterpResult::Ok(content) => {
                                if !current_literal.is_empty() {
                                    parts.push(RawShellPart::Literal(std::mem::take(
                                        &mut current_literal,
                                    )));
                                }
                                parts.push(RawShellPart::Interpolated {
                                    content,
                                    start_offset: content_start,
                                });
                            }
                            ShellInterpResult::Recovered => {
                                // Nested `@{` error already pushed; outer
                                // segment was consumed up to its matching `}`.
                                // Do not emit a part — suppresses cascading
                                // parse/type errors for the recovered span.
                            }
                            ShellInterpResult::Unclosed => {
                                // UnclosedShellInterp already pushed; line is
                                // effectively over.
                                break;
                            }
                        }
                    } else {
                        current_literal.push('@');
                        self.advance();
                    }
                }
                Some(ch) => {
                    current_literal.push(ch);
                    self.advance();
                }
            }
        }

        // Flush any trailing literal.
        if !current_literal.is_empty() {
            parts.push(RawShellPart::Literal(current_literal));
        }

        // Ensure at least one part is present (an empty command is still valid).
        if parts.is_empty() {
            parts.push(RawShellPart::Literal(String::new()));
        }

        let end = self.position;
        let span = Span::new(start, end);

        // Record the raw parts for phase-2 processing and emit a placeholder.
        let token_idx = self.tokens.len();
        self.raw_shell_parts.push((token_idx, parts));
        Token::new(TokenKind::ShellLine(Vec::new()), span)
    }

    /// Collects the raw source text of an `@{...}` interpolation, ending at
    /// the matching `}`. The caller must have consumed `@{` already.
    ///
    /// `interp_start` is the byte offset of the `@` character (used for the
    /// error span).
    fn collect_shell_interp_content(&mut self, interp_start: u32) -> ShellInterpResult {
        let mut content = String::new();
        let mut depth: i32 = 1;
        let mut nested_seen = false;

        loop {
            match self.peek() {
                None | Some('\n') => {
                    let span = Span::new(interp_start, self.position);
                    self.errors.push(LexError::UnclosedShellInterp { span });
                    return ShellInterpResult::Unclosed;
                }
                Some('{') => {
                    depth += 1;
                    content.push('{');
                    self.advance();
                }
                Some('}') => {
                    depth -= 1;
                    if depth == 0 {
                        self.advance(); // consume closing `}`
                        return if nested_seen {
                            ShellInterpResult::Recovered
                        } else {
                            ShellInterpResult::Ok(content)
                        };
                    }
                    content.push('}');
                    self.advance();
                }
                Some('@') => {
                    // Check for nested `@{` — an error.
                    let mut look = self.chars.clone();
                    look.next();
                    if look.peek() == Some(&'{') {
                        let nested_start = self.position;
                        self.advance(); // `@`
                        self.advance(); // `{`
                        let nested_span = Span::new(nested_start, self.position);
                        self.errors.push(LexError::NestedShellInterp { span: nested_span });
                        nested_seen = true;
                        // Recovery: skip the entire nested `@{...}` segment.
                        let mut inner_depth: i32 = 1;
                        while inner_depth > 0 {
                            match self.peek() {
                                None | Some('\n') => {
                                    let span = Span::new(interp_start, self.position);
                                    self.errors.push(LexError::UnclosedShellInterp { span });
                                    return ShellInterpResult::Unclosed;
                                }
                                Some('{') => {
                                    inner_depth += 1;
                                    self.advance();
                                }
                                Some('}') => {
                                    inner_depth -= 1;
                                    self.advance();
                                }
                                Some(_) => {
                                    self.advance();
                                }
                            }
                        }
                    } else {
                        content.push('@');
                        self.advance();
                    }
                }
                Some(ch) => {
                    content.push(ch);
                    self.advance();
                }
            }
        }
    }

    /// Lexes a single token.
    fn lex_token(&mut self) -> Option<Token> {
        self.skip_whitespace();

        let start = self.position;
        let ch = self.peek()?;

        // Handle comments
        if ch == '/' && self.chars.clone().nth(1) == Some('/') {
            self.skip_comment();
            return self.lex_token(); // Recursively get next token
        }

        // Handle numbers
        if ch.is_ascii_digit() {
            return Some(self.lex_number(start));
        }

        // Handle strings
        if ch == '"' {
            return Some(self.lex_string(start));
        }

        // Handle identifiers and keywords
        if ch.is_alphabetic() || ch == '_' {
            return Some(self.lex_identifier_or_keyword(start));
        }

        // Handle `$` shell expressions
        if ch == '$' {
            self.advance(); // consume `$`
            return Some(self.lex_shell_line(start));
        }

        // Handle operators and punctuation
        self.advance();

        let kind = match ch {
            '+' => TokenKind::Plus,
            '*' => TokenKind::Star,
            '/' => TokenKind::Slash,
            '%' => TokenKind::Percent,
            '(' => TokenKind::LParen,
            ')' => TokenKind::RParen,
            '{' => TokenKind::LBrace,
            '}' => TokenKind::RBrace,
            '[' => TokenKind::LBracket,
            ']' => TokenKind::RBracket,
            ',' => TokenKind::Comma,
            '.' => TokenKind::Dot,
            ':' => {
                if self.peek() == Some(':') {
                    self.advance();
                    TokenKind::ColonColon
                } else {
                    TokenKind::Colon
                }
            }
            ';' => TokenKind::Semi,

            // Multi-character operators
            '-' => {
                if self.peek() == Some('>') {
                    self.advance();
                    TokenKind::Arrow
                } else {
                    TokenKind::Minus
                }
            }
            '=' => {
                if self.peek() == Some('=') {
                    self.advance();
                    TokenKind::EqEq
                } else if self.peek() == Some('>') {
                    self.advance();
                    TokenKind::FatArrow
                } else {
                    TokenKind::Eq
                }
            }
            '!' => {
                if self.peek() == Some('=') {
                    self.advance();
                    TokenKind::BangEq
                } else {
                    TokenKind::Bang
                }
            }
            '<' => {
                if self.peek() == Some('=') {
                    self.advance();
                    TokenKind::LtEq
                } else {
                    TokenKind::Lt
                }
            }
            '>' => {
                if self.peek() == Some('=') {
                    self.advance();
                    TokenKind::GtEq
                } else {
                    TokenKind::Gt
                }
            }
            '&' => {
                if self.peek() == Some('&') {
                    self.advance();
                    TokenKind::AndAnd
                } else {
                    // Single & is not supported in M1
                    let end = self.position;
                    let span = Span::new(start, end);
                    self.errors.push(LexError::UnexpectedChar { ch, span });
                    return self.lex_token();
                }
            }
            '|' => {
                if self.peek() == Some('|') {
                    self.advance();
                    TokenKind::OrOr
                } else {
                    TokenKind::Pipe
                }
            }

            _ => {
                // Unexpected character
                let end = self.position;
                let span = Span::new(start, end);
                self.errors.push(LexError::UnexpectedChar { ch, span });
                // Return a placeholder token to continue
                return self.lex_token();
            }
        };

        let end = self.position;
        let span = Span::new(start, end);
        Some(Token::new(kind, span))
    }

    /// Lexes all tokens from the source code.
    fn lex_all(&mut self) {
        while self.peek().is_some() {
            if let Some(token) = self.lex_token() {
                self.tokens.push(token);
            }
        }

        // Add EOF token
        let eof_span = Span::new(self.position, self.position);
        self.tokens.push(Token::new(TokenKind::Eof, eof_span));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_input() {
        let mut interner = Interner::new();
        let result = lex("", &mut interner);
        assert_eq!(result.tokens.len(), 1); // Only EOF
        assert!(matches!(result.tokens[0].kind, TokenKind::Eof));
        assert!(!result.has_errors());
    }

    #[test]
    fn test_simple_arithmetic() {
        let mut interner = Interner::new();
        let result = lex("1 + 2", &mut interner);
        assert!(!result.has_errors());
        assert_eq!(result.tokens.len(), 4); // 1, +, 2, EOF
        assert!(matches!(result.tokens[0].kind, TokenKind::IntLit(1)));
        assert!(matches!(result.tokens[1].kind, TokenKind::Plus));
        assert!(matches!(result.tokens[2].kind, TokenKind::IntLit(2)));
        assert!(matches!(result.tokens[3].kind, TokenKind::Eof));
    }

    #[test]
    fn test_function_definition() {
        let mut interner = Interner::new();
        let result = lex("fn foo() { }", &mut interner);
        assert!(!result.has_errors());
        assert!(matches!(result.tokens[0].kind, TokenKind::Fn));
        assert!(matches!(result.tokens[1].kind, TokenKind::Ident(_)));
        assert!(matches!(result.tokens[2].kind, TokenKind::LParen));
        assert!(matches!(result.tokens[3].kind, TokenKind::RParen));
        assert!(matches!(result.tokens[4].kind, TokenKind::LBrace));
        assert!(matches!(result.tokens[5].kind, TokenKind::RBrace));
    }

    #[test]
    fn test_let_binding() {
        let mut interner = Interner::new();
        let result = lex("let x = 5", &mut interner);
        assert!(!result.has_errors());
        assert!(matches!(result.tokens[0].kind, TokenKind::Let));
        assert!(matches!(result.tokens[1].kind, TokenKind::Ident(_)));
        assert!(matches!(result.tokens[2].kind, TokenKind::Eq));
        assert!(matches!(result.tokens[3].kind, TokenKind::IntLit(5)));
    }

    #[test]
    fn test_string_literal() {
        let mut interner = Interner::new();
        let result = lex(r#""hello world""#, &mut interner);
        assert!(!result.has_errors());
        assert!(matches!(result.tokens[0].kind, TokenKind::StrLit(_)));
        if let TokenKind::StrLit(sym) = result.tokens[0].kind {
            assert_eq!(interner.resolve(sym), "hello world");
        }
    }

    #[test]
    fn test_unterminated_string() {
        let mut interner = Interner::new();
        let result = lex(r#""hello"#, &mut interner);
        assert!(result.has_errors());
        assert_eq!(result.errors.len(), 1);
        assert!(matches!(result.errors[0], LexError::UnterminatedString { .. }));
    }

    #[test]
    fn test_unexpected_character() {
        let mut interner = Interner::new();
        let result = lex("@", &mut interner);
        assert!(result.has_errors());
        assert_eq!(result.errors.len(), 1);
        assert!(matches!(result.errors[0], LexError::UnexpectedChar { ch: '@', .. }));
    }

    #[test]
    fn test_comments_are_skipped() {
        let mut interner = Interner::new();
        let result = lex("// this is a comment\nlet x = 5", &mut interner);
        assert!(!result.has_errors());
        assert!(matches!(result.tokens[0].kind, TokenKind::Let));
    }

    #[test]
    fn test_keywords_recognized() {
        let mut interner = Interner::new();
        let result = lex("let fn return if else true false", &mut interner);
        assert!(!result.has_errors());
        assert!(matches!(result.tokens[0].kind, TokenKind::Let));
        assert!(matches!(result.tokens[1].kind, TokenKind::Fn));
        assert!(matches!(result.tokens[2].kind, TokenKind::Return));
        assert!(matches!(result.tokens[3].kind, TokenKind::If));
        assert!(matches!(result.tokens[4].kind, TokenKind::Else));
        assert!(matches!(result.tokens[5].kind, TokenKind::True));
        assert!(matches!(result.tokens[6].kind, TokenKind::False));
    }

    #[test]
    fn test_multi_char_operators() {
        let mut interner = Interner::new();
        let result = lex("-> == != <= >=", &mut interner);
        assert!(!result.has_errors());
        assert!(matches!(result.tokens[0].kind, TokenKind::Arrow));
        assert!(matches!(result.tokens[1].kind, TokenKind::EqEq));
        assert!(matches!(result.tokens[2].kind, TokenKind::BangEq));
        assert!(matches!(result.tokens[3].kind, TokenKind::LtEq));
        assert!(matches!(result.tokens[4].kind, TokenKind::GtEq));
    }

    #[test]
    fn test_string_escapes() {
        let mut interner = Interner::new();
        let result = lex(r#""hello\nworld\t\"escaped\"""#, &mut interner);
        assert!(!result.has_errors());
        if let TokenKind::StrLit(sym) = result.tokens[0].kind {
            assert_eq!(interner.resolve(sym), "hello\nworld\t\"escaped\"");
        }
    }

    #[test]
    fn test_span_tracking() {
        let mut interner = Interner::new();
        let result = lex("let x", &mut interner);
        assert!(!result.has_errors());

        // "let" should span bytes 0-3
        assert_eq!(result.tokens[0].span.start, 0);
        assert_eq!(result.tokens[0].span.end, 3);

        // "x" should span bytes 4-5
        assert_eq!(result.tokens[1].span.start, 4);
        assert_eq!(result.tokens[1].span.end, 5);
    }

    #[test]
    fn test_multiple_errors_accumulated() {
        let mut interner = Interner::new();
        let result = lex("@ let # x", &mut interner);
        assert!(result.has_errors());
        assert_eq!(result.errors.len(), 2); // @ and #
    }

    // ============================================================
    // Shell expression tests
    // ============================================================

    #[test]
    fn test_shell_simple() {
        let mut interner = Interner::new();
        let result = lex("$ git rev-parse HEAD", &mut interner);
        assert!(!result.has_errors());
        match &result.tokens[0].kind {
            TokenKind::ShellLine(parts) => {
                assert_eq!(parts.len(), 1);
                match &parts[0] {
                    ShellTokenPart::Literal(s) => assert_eq!(s, " git rev-parse HEAD"),
                    _ => panic!("expected literal"),
                }
            }
            _ => panic!("expected ShellLine"),
        }
    }

    #[test]
    fn test_shell_with_interpolation() {
        let mut interner = Interner::new();
        let result = lex("$ cat @{filename} | wc -l", &mut interner);
        assert!(!result.has_errors(), "errors: {:?}", result.errors);
        match &result.tokens[0].kind {
            TokenKind::ShellLine(parts) => {
                assert_eq!(parts.len(), 3);
                assert!(matches!(&parts[0], ShellTokenPart::Literal(s) if s == " cat "));
                match &parts[1] {
                    ShellTokenPart::Interpolated(toks) => {
                        assert_eq!(toks.len(), 1);
                        assert!(matches!(toks[0].kind, TokenKind::Ident(_)));
                    }
                    _ => panic!("expected interpolated"),
                }
                assert!(matches!(&parts[2], ShellTokenPart::Literal(s) if s == " | wc -l"));
            }
            _ => panic!("expected ShellLine"),
        }
    }

    #[test]
    fn test_shell_literal_dollar_and_braces() {
        // `$ awk '{print $1}' @{dir}/file.txt`
        let mut interner = Interner::new();
        let result = lex("$ awk '{print $1}' @{dir}/file.txt", &mut interner);
        assert!(!result.has_errors(), "errors: {:?}", result.errors);
        match &result.tokens[0].kind {
            TokenKind::ShellLine(parts) => {
                assert!(parts.len() >= 3);
                if let ShellTokenPart::Literal(s) = &parts[0] {
                    // The `{print $1}` should pass through verbatim.
                    assert!(s.contains("{print $1}"));
                } else {
                    panic!("expected literal first");
                }
            }
            _ => panic!("expected ShellLine"),
        }
    }

    #[test]
    fn test_shell_continuation() {
        let mut interner = Interner::new();
        let src = "$ git log \\\n    --oneline\nlet x = 5";
        let result = lex(src, &mut interner);
        assert!(!result.has_errors(), "errors: {:?}", result.errors);
        // First token should be ShellLine, second should be Let
        assert!(matches!(result.tokens[0].kind, TokenKind::ShellLine(_)));
        assert!(matches!(result.tokens[1].kind, TokenKind::Let));
    }

    #[test]
    fn test_shell_nested_interpolation_errors() {
        let mut interner = Interner::new();
        let result = lex("$ echo @{@{x}}", &mut interner);
        assert!(result.has_errors());
        assert!(result
            .errors
            .iter()
            .any(|e| matches!(e, LexError::NestedShellInterp { .. })));
    }

    #[test]
    fn test_shell_unclosed_interpolation_errors() {
        let mut interner = Interner::new();
        let result = lex("$ echo @{x\nlet y = 1", &mut interner);
        assert!(result.has_errors());
        assert!(result
            .errors
            .iter()
            .any(|e| matches!(e, LexError::UnclosedShellInterp { .. })));
    }
}
