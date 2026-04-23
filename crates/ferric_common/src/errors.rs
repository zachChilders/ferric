//! Error types for all compiler stages.
//!
//! CRITICAL: Every error type MUST carry a Span field (Rule 5).
//! This enables precise error reporting and future renderer replacement.

use crate::{Span, Symbol, TokenKind, Ty};

/// Errors that can occur during lexing.
#[derive(Debug, Clone, PartialEq)]
pub enum LexError {
    /// An unexpected character was encountered
    UnexpectedChar {
        /// The unexpected character
        ch: char,
        /// Location of the error
        span: Span,
    },
    /// A string literal was not terminated before EOF
    UnterminatedString {
        /// Location of the unterminated string
        span: Span,
    },
}

impl LexError {
    /// Returns the span associated with this error.
    pub fn span(&self) -> Span {
        match self {
            LexError::UnexpectedChar { span, .. } => *span,
            LexError::UnterminatedString { span } => *span,
        }
    }

    /// Returns a human-readable description of this error.
    pub fn description(&self) -> String {
        match self {
            LexError::UnexpectedChar { ch, .. } => {
                format!("unexpected character '{}'", ch)
            }
            LexError::UnterminatedString { .. } => {
                "unterminated string literal".to_string()
            }
        }
    }
}

/// Errors that can occur during parsing.
#[derive(Debug, Clone, PartialEq)]
pub enum ParseError {
    /// An unexpected token was found
    UnexpectedToken {
        /// What was expected
        expected: String,
        /// What was actually found
        found: TokenKind,
        /// Location of the unexpected token
        span: Span,
    },
    /// Expected an expression but found something else
    ExpectedExpression {
        /// What was found instead
        found: TokenKind,
        /// Location of the error
        span: Span,
    },
    /// Expected a statement but found something else
    ExpectedStatement {
        /// What was found instead
        found: TokenKind,
        /// Location of the error
        span: Span,
    },
}

impl ParseError {
    /// Returns the span associated with this error.
    pub fn span(&self) -> Span {
        match self {
            ParseError::UnexpectedToken { span, .. } => *span,
            ParseError::ExpectedExpression { span, .. } => *span,
            ParseError::ExpectedStatement { span, .. } => *span,
        }
    }

    /// Returns a human-readable description of this error.
    pub fn description(&self) -> String {
        match self {
            ParseError::UnexpectedToken { expected, found, .. } => {
                format!("expected {}, found {}", expected, found.description())
            }
            ParseError::ExpectedExpression { found, .. } => {
                format!("expected expression, found {}", found.description())
            }
            ParseError::ExpectedStatement { found, .. } => {
                format!("expected statement, found {}", found.description())
            }
        }
    }
}

/// Errors that can occur during name resolution.
#[derive(Debug, Clone, PartialEq)]
pub enum ResolveError {
    /// A variable or function was used but not defined
    UndefinedVariable {
        /// The name that was not found
        name: Symbol,
        /// Location of the undefined reference
        span: Span,
    },
    /// A name was defined multiple times in the same scope
    DuplicateDefinition {
        /// The duplicated name
        name: Symbol,
        /// Location of the first definition
        first: Span,
        /// Location of the duplicate definition
        second: Span,
    },
}

impl ResolveError {
    /// Returns the primary span associated with this error.
    pub fn span(&self) -> Span {
        match self {
            ResolveError::UndefinedVariable { span, .. } => *span,
            ResolveError::DuplicateDefinition { second, .. } => *second,
        }
    }

    /// Returns a human-readable description of this error.
    pub fn description(&self) -> String {
        match self {
            ResolveError::UndefinedVariable { .. } => {
                "undefined variable".to_string()
            }
            ResolveError::DuplicateDefinition { .. } => {
                "duplicate definition".to_string()
            }
        }
    }
}

/// Errors that can occur during type checking.
#[derive(Debug, Clone, PartialEq)]
pub enum TypeError {
    /// A type mismatch occurred
    Mismatch {
        /// The expected type
        expected: Ty,
        /// The type that was found
        found: Ty,
        /// Location of the type mismatch
        span: Span,
    },
    /// An operation was applied to incompatible types
    IncompatibleTypes {
        /// The operation that failed
        operation: String,
        /// The left operand type
        left: Ty,
        /// The right operand type
        right: Ty,
        /// Location of the error
        span: Span,
    },
}

impl TypeError {
    /// Returns the span associated with this error.
    pub fn span(&self) -> Span {
        match self {
            TypeError::Mismatch { span, .. } => *span,
            TypeError::IncompatibleTypes { span, .. } => *span,
        }
    }

    /// Returns a human-readable description of this error.
    pub fn description(&self) -> String {
        match self {
            TypeError::Mismatch { expected, found, .. } => {
                format!(
                    "type mismatch: expected {}, found {}",
                    expected.description(),
                    found.description()
                )
            }
            TypeError::IncompatibleTypes { operation, left, right, .. } => {
                format!(
                    "incompatible types for {}: {} and {}",
                    operation,
                    left.description(),
                    right.description()
                )
            }
        }
    }
}
