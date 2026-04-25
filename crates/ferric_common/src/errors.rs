//! Error types for all compiler stages.
//!
//! CRITICAL: Every error type MUST carry a Span field (Rule 5).
//! This enables precise error reporting and future renderer replacement.

use serde::{Deserialize, Serialize};
use crate::{Span, Symbol, TokenKind, Ty, TyVar};

/// Errors that can occur during lexing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// A shell interpolation `@{...}` contained another `@{` inside it.
    NestedShellInterp {
        span: Span,
    },
    /// A shell interpolation `@{...}` had no closing `}` before end-of-line.
    UnclosedShellInterp {
        span: Span,
    },
}

impl LexError {
    /// Returns the span associated with this error.
    pub fn span(&self) -> Span {
        match self {
            LexError::UnexpectedChar { span, .. } => *span,
            LexError::UnterminatedString { span } => *span,
            LexError::NestedShellInterp { span } => *span,
            LexError::UnclosedShellInterp { span } => *span,
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
            LexError::NestedShellInterp { .. } => {
                "nested shell interpolation `@{` is not allowed".to_string()
            }
            LexError::UnclosedShellInterp { .. } => {
                "unclosed shell interpolation: missing `}`".to_string()
            }
        }
    }
}

/// Errors that can occur during parsing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// A positional (unnamed) argument was used at a call site
    PositionalArg {
        /// Location of the offending argument
        span: Span,
    },
    /// An invalid mode was given inside require(...)
    InvalidRequireMode {
        /// Location of the invalid token
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
            ParseError::PositionalArg { span } => *span,
            ParseError::InvalidRequireMode { span } => *span,
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
            ParseError::PositionalArg { .. } => {
                "positional arguments are not allowed; use named arguments (name: value)".to_string()
            }
            ParseError::InvalidRequireMode { .. } => {
                "invalid require mode; expected 'warn'".to_string()
            }
        }
    }
}

/// Errors that can occur during name resolution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// Assignment to an immutable variable
    AssignToImmutable {
        /// The name of the immutable variable
        name: Symbol,
        /// Location of the assignment
        span: Span,
    },
    /// Break used outside of a loop
    BreakOutsideLoop {
        /// Location of the break
        span: Span,
    },
    /// Continue used outside of a loop
    ContinueOutsideLoop {
        /// Location of the continue
        span: Span,
    },
    /// Return used outside of a function
    ReturnOutsideFn {
        /// Location of the return
        span: Span,
    },
    /// A required parameter had no corresponding argument at the call site
    MissingArg {
        /// The parameter that was not supplied
        param: Symbol,
        /// Location of the call
        call_span: Span,
    },
    /// An argument name at a call site does not match any parameter
    UnknownArg {
        /// The unrecognised argument name
        name: Symbol,
        /// Location of the argument
        span: Span,
    },
    /// The set closure in a require statement must take zero arguments
    RequireSetArity {
        /// Location of the closure
        span: Span,
    },
}

impl ResolveError {
    /// Returns the primary span associated with this error.
    pub fn span(&self) -> Span {
        match self {
            ResolveError::UndefinedVariable { span, .. } => *span,
            ResolveError::DuplicateDefinition { second, .. } => *second,
            ResolveError::AssignToImmutable { span, .. } => *span,
            ResolveError::BreakOutsideLoop { span } => *span,
            ResolveError::ContinueOutsideLoop { span } => *span,
            ResolveError::ReturnOutsideFn { span } => *span,
            ResolveError::MissingArg { call_span, .. } => *call_span,
            ResolveError::UnknownArg { span, .. } => *span,
            ResolveError::RequireSetArity { span } => *span,
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
            ResolveError::AssignToImmutable { .. } => {
                "assignment to immutable variable".to_string()
            }
            ResolveError::BreakOutsideLoop { .. } => {
                "break outside of loop".to_string()
            }
            ResolveError::ContinueOutsideLoop { .. } => {
                "continue outside of loop".to_string()
            }
            ResolveError::ReturnOutsideFn { .. } => {
                "return outside of function".to_string()
            }
            ResolveError::MissingArg { .. } => {
                "missing required argument".to_string()
            }
            ResolveError::UnknownArg { .. } => {
                "unknown argument name".to_string()
            }
            ResolveError::RequireSetArity { .. } => {
                "require set closure must take zero arguments".to_string()
            }
        }
    }
}

/// Errors that can occur during type checking.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// The require condition expression is not Bool
    RequireNonBool {
        found: Ty,
        span: Span,
    },
    /// The require message expression is not Str
    RequireMessageNonStr {
        found: Ty,
        span: Span,
    },
    /// The require set closure is not Fn() -> Unit
    RequireSetType {
        found: Ty,
        span: Span,
    },
    /// A shell interpolation `@{expr}` has a type other than Str or Int.
    ShellInterpType {
        found: Ty,
        span: Span,
    },
    /// Unification produced an infinite type (failed occurs check).
    InfiniteType {
        /// The variable that would recursively contain itself.
        var: TyVar,
        /// The type that contains `var`.
        ty: Ty,
        /// Location of the offending unification.
        span: Span,
    },
    /// An expression's type could not be fully resolved to a concrete type
    /// — a type variable remains in the substitution after inference.
    CannotInfer {
        /// Location of the expression whose type is ambiguous.
        span: Span,
    },
    /// A function call had the wrong number of arguments.
    WrongArgumentCount {
        /// Expected number of arguments.
        expected: usize,
        /// Actual number of arguments supplied.
        found: usize,
        /// Location of the call.
        span: Span,
    },
    /// An expression in callee position is not a function.
    NotCallable {
        /// The non-function type that was applied.
        ty: Ty,
        /// Location of the call.
        span: Span,
    },
}

impl TypeError {
    /// Returns the span associated with this error.
    pub fn span(&self) -> Span {
        match self {
            TypeError::Mismatch { span, .. } => *span,
            TypeError::IncompatibleTypes { span, .. } => *span,
            TypeError::RequireNonBool { span, .. } => *span,
            TypeError::RequireMessageNonStr { span, .. } => *span,
            TypeError::RequireSetType { span, .. } => *span,
            TypeError::ShellInterpType { span, .. } => *span,
            TypeError::InfiniteType { span, .. } => *span,
            TypeError::CannotInfer { span } => *span,
            TypeError::WrongArgumentCount { span, .. } => *span,
            TypeError::NotCallable { span, .. } => *span,
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
            TypeError::RequireNonBool { found, .. } => {
                format!("require condition must be Bool, found {}", found.description())
            }
            TypeError::RequireMessageNonStr { found, .. } => {
                format!("require message must be Str, found {}", found.description())
            }
            TypeError::RequireSetType { found, .. } => {
                format!("require set closure must be Fn() -> Unit, found {}", found.description())
            }
            TypeError::ShellInterpType { found, .. } => {
                format!("shell interpolation must be Str or Int, found {}", found.description())
            }
            TypeError::InfiniteType { var, ty, .. } => {
                format!(
                    "occurs check failed: cannot construct the infinite type ?T{} = {}",
                    var.0,
                    ty.description()
                )
            }
            TypeError::CannotInfer { .. } => {
                "cannot infer a concrete type for this expression".to_string()
            }
            TypeError::WrongArgumentCount { expected, found, .. } => {
                format!("expected {} argument(s), found {}", expected, found)
            }
            TypeError::NotCallable { ty, .. } => {
                format!("cannot call value of type {}", ty.description())
            }
        }
    }
}
