//! Single source of truth for Ferric keywords, type keywords, and operators.
//!
//! Consumed by the lexer (at runtime, via a parity test) and by
//! `ferric_lsp/build.rs` (at build time, to generate the TextMate grammar).
//! Adding a keyword here automatically updates the TextMate grammar on the
//! next `cargo build`. The lexer's keyword match is checked against this list
//! by `ferric_lexer::tests::keywords_match_common_list`.

/// Reserved words recognised by the lexer as their own `TokenKind` variant.
///
/// Every entry must correspond to a non-`Ident` token produced by the lexer.
/// The lexer test `keywords_match_common_list` enforces this.
pub const KEYWORDS: &[&str] = &[
    "let", "mut", "fn", "return",
    "if", "else", "while", "loop",
    "break", "continue",
    "true", "false",
    "require",
    "struct", "enum", "match",
    "trait", "impl", "for",
    "import", "export", "from",
    "type", "as",
];

/// Built-in primitive type names. These are NOT lexer keywords — they are
/// matched as identifiers and recognised as types by later stages — but the
/// TextMate grammar colours them as type keywords.
pub const TYPE_KEYWORDS: &[&str] = &[
    "Int", "Float", "Bool", "Str", "Unit",
];

/// Operator tokens (arithmetic, comparison, logical, assignment) that the
/// TextMate grammar colours under `keyword.operator.ferric`.
///
/// Punctuation/delimiters (`(`, `)`, `{`, `}`, `,`, `;`, `:`, `.`, `::`, `->`,
/// `=>`) are not in this list — they are handled by VS Code's bracket and
/// punctuation defaults.
pub const OPERATORS: &[&str] = &[
    "+", "-", "*", "/", "%",
    "==", "!=", "<=", ">=", "<", ">",
    "&&", "||", "!",
    "=",
];
