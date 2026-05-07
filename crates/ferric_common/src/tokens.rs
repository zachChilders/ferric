//! Token types for the lexer.

use crate::{Span, Symbol};
use serde::{Deserialize, Serialize};

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
    /// `import` keyword for module imports
    Import,
    /// `export` keyword (prefix modifier on exportable items)
    Export,
    /// `from` keyword (used in `import ... from "path"`)
    From,
    /// `type` keyword for type alias definitions
    Type,
    /// `as` keyword for cast expressions and import aliases
    As,
    /// `async` keyword — prefix modifier on `fn` and `{ ... }` blocks
    Async,
    /// `await` keyword — postfix suffix following `.` on an awaitable expr
    Await,
    /// `effect` keyword — declares an algebraic effect at module scope
    Effect,
    /// `perform` keyword — invokes the nearest enclosing handler for an op
    Perform,
    /// `handle` keyword — installs effect handler clauses for a body expr
    Handle,
    /// `with` keyword — separator between handle body and handler clauses
    With,
    /// `resume` keyword — resumes a captured continuation (handler-only)
    Resume,
    /// `gen` keyword — prefix modifier on `fn` introducing a generator
    Gen,
    /// `yield` keyword — generator yield, desugars to `perform Gen::Yield(...)`
    Yield,
    /// `must` keyword. M10 (Task 4). Postfix unwrap: `expr must` panics with a
    /// generic message on `None`/`Err`; `expr must <str>` panics with the given
    /// message. Binds at unary precedence.
    Must,

    // Identifiers and operators
    /// Identifier (variable/function name)
    Ident(Symbol),
    /// Loop label: `'name`. M10 (Task 5). The `'` sigil is followed
    /// immediately (no whitespace) by an identifier. Used in
    /// `'outer: while ...`, `break 'outer`, and `continue 'outer`.
    Label(Symbol),

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
    /// `|>` pipeline operator. M10 (Task 3). Lowest-precedence binary infix:
    /// `lhs |> rhs_call(args...)` desugars to `rhs_call(lhs, args...)` with
    /// the lhs prepended as the implicit first named argument.
    Pipe2,
    /// `?` propagation operator. M10 (Task 4). Postfix; binds tighter than any
    /// binary operator. `expr?` desugars (in the compiler) to a match that
    /// extracts the inner value or early-returns the failure variant.
    Question,

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

    /// Opening of an f-string: the lexer saw `f"`. M10 (Task 2). Followed by
    /// alternating `FStringLit` / `FStringExprStart` / expression tokens /
    /// `FStringExprEnd` until a matching `FStringEnd`.
    FStringStart,
    /// A literal text segment inside an f-string. M10 (Task 2). Already has
    /// `{{` / `}}` escape sequences resolved to `{` / `}`.
    FStringLit(String),
    /// `{` inside an f-string — opens an interpolated expression. The lexer
    /// re-enters normal expression mode until the matching `FStringExprEnd`.
    /// M10 (Task 2).
    FStringExprStart,
    /// `}` closing an interpolated expression inside an f-string. M10 (Task 2).
    FStringExprEnd,
    /// Closing `"` of an f-string. M10 (Task 2).
    FStringEnd,

    /// End of file marker
    Eof,
}

impl TokenKind {
    /// Returns a human-readable description of this token kind.
    pub fn description(&self) -> String {
        match self {
            TokenKind::IntLit(n) => format!("integer literal '{n}'"),
            TokenKind::FloatLit(f) => format!("float literal '{f}'"),
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
            TokenKind::Import => "keyword 'import'".to_string(),
            TokenKind::Export => "keyword 'export'".to_string(),
            TokenKind::From => "keyword 'from'".to_string(),
            TokenKind::Type => "keyword 'type'".to_string(),
            TokenKind::As => "keyword 'as'".to_string(),
            TokenKind::Async => "keyword 'async'".to_string(),
            TokenKind::Await => "keyword 'await'".to_string(),
            TokenKind::Effect => "keyword 'effect'".to_string(),
            TokenKind::Perform => "keyword 'perform'".to_string(),
            TokenKind::Handle => "keyword 'handle'".to_string(),
            TokenKind::With => "keyword 'with'".to_string(),
            TokenKind::Resume => "keyword 'resume'".to_string(),
            TokenKind::Gen => "keyword 'gen'".to_string(),
            TokenKind::Yield => "keyword 'yield'".to_string(),
            TokenKind::Must => "keyword 'must'".to_string(),
            TokenKind::Ident(_) => "identifier".to_string(),
            TokenKind::Label(_) => "loop label".to_string(),
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
            TokenKind::Pipe2 => "'|>'".to_string(),
            TokenKind::Question => "'?'".to_string(),
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
            TokenKind::FStringStart => "f-string start `f\"`".to_string(),
            TokenKind::FStringLit(_) => "f-string literal segment".to_string(),
            TokenKind::FStringExprStart => "f-string `{`".to_string(),
            TokenKind::FStringExprEnd => "f-string `}`".to_string(),
            TokenKind::FStringEnd => "f-string end `\"`".to_string(),
            TokenKind::Eof => "end of file".to_string(),
        }
    }
}
