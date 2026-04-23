//! Lexer for the Ferric programming language.
//!
//! This crate provides a single public entry point: the `lex` function,
//! which converts source code into a stream of tokens.

use ferric_common::{Interner, LexError, LexResult, Span, Token, TokenKind};

/// Lexes the source code into tokens.
///
/// This is the only public API for the lexer. It takes source code and an interner,
/// and returns all tokens along with any lexical errors encountered.
///
/// # Example
///
/// ```
/// use ferric_lexer::lex;
/// use ferric_common::Interner;
///
/// let mut interner = Interner::new();
/// let result = lex("let x = 42;", &mut interner);
/// assert!(!result.has_errors());
/// ```
pub fn lex(source: &str, interner: &mut Interner) -> LexResult {
    let mut lexer = Lexer::new(source, interner);
    lexer.lex_all();
    LexResult::new(lexer.tokens, lexer.errors)
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

    /// Lexes an integer literal.
    fn lex_number(&mut self, start: u32) -> Token {
        let mut num_str = String::new();

        while let Some(ch) = self.peek() {
            if ch.is_ascii_digit() {
                num_str.push(ch);
                self.advance();
            } else {
                break;
            }
        }

        let end = self.position;
        let span = Span::new(start, end);

        // Parse the integer
        match num_str.parse::<i64>() {
            Ok(value) => Token::new(TokenKind::IntLit(value), span),
            Err(_) => {
                // If parsing fails, emit an error and return a 0 token
                self.errors.push(LexError::UnexpectedChar {
                    ch: num_str.chars().next().unwrap_or('0'),
                    span,
                });
                Token::new(TokenKind::IntLit(0), span)
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
            "fn" => TokenKind::Fn,
            "return" => TokenKind::Return,
            "if" => TokenKind::If,
            "else" => TokenKind::Else,
            "true" => TokenKind::True,
            "false" => TokenKind::False,
            _ => {
                // It's an identifier, intern it
                let symbol = self.interner.intern(&ident);
                TokenKind::Ident(symbol)
            }
        };

        Token::new(kind, span)
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
            ',' => TokenKind::Comma,
            ':' => TokenKind::Colon,
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
}
