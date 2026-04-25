//! Token types for the lexer.

use serde::{Deserialize, Serialize};
use crate::{Span, Symbol};

/// A token with its kind and source location.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Token {
    /// The kind of token
    pub kind: TokenKind,
    /// The source location of this token
    pub span: Span,
}

/// A part of a shell command line at the token level.
///
/// The lexer splits a shell line on `@{` / `}` interpolation boundaries and
/// recursively lexes each interpolated segment into a sub-token stream. The
/// parser later converts these sub-token streams into `Expr` nodes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ShellTokenPart {
    /// Literal shell text passed through verbatim.
    Literal(String),
    /// An `@{...}` interpolation — the inner Ferric token stream.
    Interpolated(Vec<Token>),
}

impl Token {
    /// Creates a new token with the given kind and span.
    pub fn new(kind: TokenKind, span: Span) -> Self {
        Self { kind, span }
    }
}

/// The kind of token, including literals, keywords, operators, and punctuation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TokenKind {
    // Literals
    /// Integer literal (e.g., `42`, `-17`)
    IntLit(i64),
    /// Floating-point literal (e.g., `3.14`, `-0.5`)
    FloatLit(f64),
    /// String literal (e.g., `"hello"`)
    StrLit(Symbol),
    /// Boolean true literal
    True,
    /// Boolean false literal
    False,

    // Keywords
    /// `let` keyword for variable declaration
    Let,
    /// `mut` keyword for mutable bindings
    Mut,
    /// `fn` keyword for function definition
    Fn,
    /// `return` keyword
    Return,
    /// `if` keyword
    If,
    /// `else` keyword
    Else,
    /// `while` keyword for while loops
    While,
    /// `loop` keyword for infinite loops
    Loop,
    /// `break` keyword to exit loops
    Break,
    /// `continue` keyword to skip to next iteration
    Continue,
    /// `require` keyword for require statements
    Require,
    /// `struct` keyword for struct definitions
    Struct,
    /// `enum` keyword for enum definitions
    Enum,
    /// `match` keyword for match expressions
    Match,
    /// `trait` keyword for trait definitions
    Trait,
    /// `impl` keyword for impl blocks
    Impl,
    /// `for` keyword for impl blocks (`impl Trait for Type`)
    For,

    // Identifiers and operators
    /// Identifier (variable/function name)
    Ident(Symbol),

    // Arithmetic operators
    /// `+` addition
    Plus,
    /// `-` subtraction
    Minus,
    /// `*` multiplication
    Star,
    /// `/` division
    Slash,
    /// `%` modulo
    Percent,

    // Comparison and equality
    /// `=` assignment
    Eq,
    /// `==` equality comparison
    EqEq,
    /// `!` logical not
    Bang,
    /// `!=` inequality comparison
    BangEq,
    /// `<` less than
    Lt,
    /// `<=` less than or equal
    LtEq,
    /// `>` greater than
    Gt,
    /// `>=` greater than or equal
    GtEq,

    // Logical operators
    /// `&&` logical and
    AndAnd,
    /// `||` logical or
    OrOr,
    /// `|` (single pipe) — used for closure parameter delimiters.
    Pipe,

    // Punctuation
    /// `(` left parenthesis
    LParen,
    /// `)` right parenthesis
    RParen,
    /// `{` left brace
    LBrace,
    /// `}` right brace
    RBrace,
    /// `[` left bracket
    LBracket,
    /// `]` right bracket
    RBracket,
    /// `,` comma
    Comma,
    /// `:` colon
    Colon,
    /// `->` arrow for return types
    Arrow,
    /// `;` semicolon
    Semi,
    /// `.` field access
    Dot,
    /// `::` path separator (used in `Foo::Bar`)
    ColonColon,
    /// `_` wildcard (in patterns)
    Underscore,
    /// `=>` match arm separator
    FatArrow,

    /// A shell command line `$ ...` — composite token produced by shell-line
    /// mode. The parts alternate between literal text and interpolated Ferric
    /// sub-token-streams (from `@{...}`).
    ShellLine(Vec<ShellTokenPart>),

    /// End of file marker
    Eof,
}

impl TokenKind {
    /// Returns a human-readable description of this token kind.
    pub fn description(&self) -> String {
        match self {
            TokenKind::IntLit(n) => format!("integer literal '{}'", n),
            TokenKind::FloatLit(f) => format!("float literal '{}'", f),
            TokenKind::StrLit(_) => "string literal".to_string(),
            TokenKind::True => "keyword 'true'".to_string(),
            TokenKind::False => "keyword 'false'".to_string(),
            TokenKind::Let => "keyword 'let'".to_string(),
            TokenKind::Mut => "keyword 'mut'".to_string(),
            TokenKind::Fn => "keyword 'fn'".to_string(),
            TokenKind::Return => "keyword 'return'".to_string(),
            TokenKind::If => "keyword 'if'".to_string(),
            TokenKind::Else => "keyword 'else'".to_string(),
            TokenKind::While => "keyword 'while'".to_string(),
            TokenKind::Loop => "keyword 'loop'".to_string(),
            TokenKind::Break => "keyword 'break'".to_string(),
            TokenKind::Continue => "keyword 'continue'".to_string(),
            TokenKind::Require => "keyword 'require'".to_string(),
            TokenKind::Struct => "keyword 'struct'".to_string(),
            TokenKind::Enum => "keyword 'enum'".to_string(),
            TokenKind::Match => "keyword 'match'".to_string(),
            TokenKind::Trait => "keyword 'trait'".to_string(),
            TokenKind::Impl => "keyword 'impl'".to_string(),
            TokenKind::For => "keyword 'for'".to_string(),
            TokenKind::Ident(_) => "identifier".to_string(),
            TokenKind::Plus => "'+'".to_string(),
            TokenKind::Minus => "'-'".to_string(),
            TokenKind::Star => "'*'".to_string(),
            TokenKind::Slash => "'/'".to_string(),
            TokenKind::Percent => "'%'".to_string(),
            TokenKind::Eq => "'='".to_string(),
            TokenKind::EqEq => "'=='".to_string(),
            TokenKind::Bang => "'!'".to_string(),
            TokenKind::BangEq => "'!='".to_string(),
            TokenKind::Lt => "'<'".to_string(),
            TokenKind::LtEq => "'<='".to_string(),
            TokenKind::Gt => "'>'".to_string(),
            TokenKind::GtEq => "'>='".to_string(),
            TokenKind::AndAnd => "'&&'".to_string(),
            TokenKind::OrOr => "'||'".to_string(),
            TokenKind::Pipe => "'|'".to_string(),
            TokenKind::LParen => "'('".to_string(),
            TokenKind::RParen => "')'".to_string(),
            TokenKind::LBrace => "'{'".to_string(),
            TokenKind::RBrace => "'}'".to_string(),
            TokenKind::LBracket => "'['".to_string(),
            TokenKind::RBracket => "']'".to_string(),
            TokenKind::Comma => "','".to_string(),
            TokenKind::Colon => "':'".to_string(),
            TokenKind::Arrow => "'->'".to_string(),
            TokenKind::Semi => "';'".to_string(),
            TokenKind::Dot => "'.'".to_string(),
            TokenKind::ColonColon => "'::'".to_string(),
            TokenKind::Underscore => "'_'".to_string(),
            TokenKind::FatArrow => "'=>'".to_string(),
            TokenKind::ShellLine(_) => "shell expression".to_string(),
            TokenKind::Eof => "end of file".to_string(),
        }
    }
}
