//! # Ferric Diagnostics (M1 Implementation)
//!
//! This is the M1 baseline diagnostics renderer. It provides minimal error
//! rendering with just line numbers - no fancy formatting or colors.
//!
//! **IMPORTANT**: This entire implementation will be completely replaced in M2
//! with rich span-annotated rendering. Keep it simple! The goal is to prove
//! the architecture works by showing surgical replacement.
//!
//! Rule 5: All errors carry Spans, so the renderer can work without knowledge
//! of the pipeline internals.

use ferric_common::{ExhaustivenessError, LexError, ParseError, ResolveError, TypeError, Span};

/// Error renderer for Ferric.
///
/// This is the public API for the diagnostics crate. It takes errors from
/// any stage and renders them as human-readable strings.
///
/// For M1, the output format is intentionally minimal:
/// ```text
/// error at line 5: unexpected character '@'
/// ```
pub struct Renderer {
    source: String,
}

impl Renderer {
    /// Creates a new renderer for the given source code.
    ///
    /// The source is stored to enable span-to-line conversion.
    pub fn new(source: String) -> Self {
        Self { source }
    }

    /// Converts a span to a line number.
    ///
    /// Line numbers are 1-indexed. This implementation counts newlines
    /// up to the span's start position.
    fn span_to_line(&self, span: Span) -> usize {
        self.source[..span.start as usize]
            .chars()
            .filter(|&c| c == '\n')
            .count()
            + 1
    }

    /// Renders a lexer error.
    pub fn render_lex_error(&self, error: &LexError) -> String {
        match error {
            LexError::UnexpectedChar { ch, span } => {
                format!(
                    "error at line {}: unexpected character '{}'",
                    self.span_to_line(*span),
                    ch
                )
            }
            LexError::UnterminatedString { span } => {
                format!(
                    "error at line {}: unterminated string literal",
                    self.span_to_line(*span)
                )
            }
            LexError::NestedShellInterp { span } => {
                format!(
                    "error at line {}: nested shell interpolation `@{{` is not allowed",
                    self.span_to_line(*span)
                )
            }
            LexError::UnclosedShellInterp { span } => {
                format!(
                    "error at line {}: unclosed shell interpolation: missing `}}`",
                    self.span_to_line(*span)
                )
            }
        }
    }

    /// Renders a parser error.
    pub fn render_parse_error(&self, error: &ParseError) -> String {
        use ferric_common::ParseError;

        match error {
            ParseError::UnexpectedToken {
                expected,
                found,
                span,
            } => {
                format!(
                    "error at line {}: expected {}, found {}",
                    self.span_to_line(*span),
                    expected,
                    found.description()
                )
            }
            ParseError::ExpectedExpression { found, span } => {
                format!(
                    "error at line {}: expected expression, found {}",
                    self.span_to_line(*span),
                    found.description()
                )
            }
            ParseError::ExpectedStatement { found, span } => {
                format!(
                    "error at line {}: expected statement, found {}",
                    self.span_to_line(*span),
                    found.description()
                )
            }
            ParseError::PositionalArg { span } => {
                format!(
                    "error at line {}: positional argument not allowed; use named syntax (name: value)",
                    self.span_to_line(*span)
                )
            }
            ParseError::InvalidRequireMode { span } => {
                format!(
                    "error at line {}: invalid require mode; expected 'warn'",
                    self.span_to_line(*span)
                )
            }
        }
    }

    /// Renders a resolver error.
    pub fn render_resolve_error(&self, error: &ResolveError) -> String {
        use ferric_common::ResolveError;

        match error {
            ResolveError::UndefinedVariable { name, span } => {
                format!(
                    "error at line {}: undefined variable `{}`",
                    self.span_to_line(*span),
                    name.0 // For M1, just print the raw symbol ID
                )
            }
            ResolveError::DuplicateDefinition {
                name,
                first,
                second,
            } => {
                format!(
                    "error at line {}: duplicate definition of `{}` (first defined at line {})",
                    self.span_to_line(*second),
                    name.0,
                    self.span_to_line(*first)
                )
            }
            ResolveError::AssignToImmutable { name, span } => {
                format!(
                    "error at line {}: assignment to immutable variable `{}`",
                    self.span_to_line(*span),
                    name.0
                )
            }
            ResolveError::BreakOutsideLoop { span } => {
                format!(
                    "error at line {}: break outside of loop",
                    self.span_to_line(*span)
                )
            }
            ResolveError::ContinueOutsideLoop { span } => {
                format!(
                    "error at line {}: continue outside of loop",
                    self.span_to_line(*span)
                )
            }
            ResolveError::ReturnOutsideFn { span } => {
                format!(
                    "error at line {}: return outside of function",
                    self.span_to_line(*span)
                )
            }
            ResolveError::MissingArg { param, call_span } => {
                format!(
                    "error at line {}: missing required argument `{}`",
                    self.span_to_line(*call_span),
                    param.0
                )
            }
            ResolveError::UnknownArg { name, span } => {
                format!(
                    "error at line {}: unknown argument name `{}`",
                    self.span_to_line(*span),
                    name.0
                )
            }
            ResolveError::RequireSetArity { span } => {
                format!(
                    "error at line {}: require set closure must take zero arguments",
                    self.span_to_line(*span)
                )
            }
            ResolveError::UndefinedType { name, span } => {
                format!(
                    "error at line {}: undefined type `{}`",
                    self.span_to_line(*span),
                    name.0
                )
            }
            ResolveError::UnknownField { struct_name, field, span } => {
                format!(
                    "error at line {}: struct `{}` has no field `{}`",
                    self.span_to_line(*span),
                    struct_name.0,
                    field.0
                )
            }
            ResolveError::MissingField { struct_name, field, span } => {
                format!(
                    "error at line {}: missing field `{}` in struct literal of `{}`",
                    self.span_to_line(*span),
                    field.0,
                    struct_name.0
                )
            }
            ResolveError::UnknownVariant { enum_name, variant, span } => {
                format!(
                    "error at line {}: enum `{}` has no variant `{}`",
                    self.span_to_line(*span),
                    enum_name.0,
                    variant.0
                )
            }
            ResolveError::VariantArity {
                enum_name,
                variant,
                expected,
                found,
                span,
            } => {
                format!(
                    "error at line {}: variant `{}::{}` expected {} argument(s), found {}",
                    self.span_to_line(*span),
                    enum_name.0,
                    variant.0,
                    expected,
                    found
                )
            }
        }
    }

    /// Renders a type checker error.
    pub fn render_type_error(&self, error: &TypeError) -> String {
        use ferric_common::TypeError;

        match error {
            TypeError::Mismatch {
                expected,
                found,
                span,
            } => {
                format!(
                    "error at line {}: type mismatch: expected {}, found {}",
                    self.span_to_line(*span),
                    expected.description(),
                    found.description()
                )
            }
            TypeError::IncompatibleTypes {
                operation,
                left,
                right,
                span,
            } => {
                format!(
                    "error at line {}: incompatible types for {}: {} and {}",
                    self.span_to_line(*span),
                    operation,
                    left.description(),
                    right.description()
                )
            }
            TypeError::RequireNonBool { found, span } => {
                format!(
                    "error at line {}: require condition must be Bool, found {}",
                    self.span_to_line(*span),
                    found.description()
                )
            }
            TypeError::RequireMessageNonStr { found, span } => {
                format!(
                    "error at line {}: require message must be Str, found {}",
                    self.span_to_line(*span),
                    found.description()
                )
            }
            TypeError::RequireSetType { found, span } => {
                format!(
                    "error at line {}: require set closure must be Fn() -> Unit, found {}",
                    self.span_to_line(*span),
                    found.description()
                )
            }
            TypeError::ShellInterpType { found, span } => {
                format!(
                    "error at line {}: shell interpolation must be Str or Int, found {}",
                    self.span_to_line(*span),
                    found.description()
                )
            }
            TypeError::InfiniteType { var, ty, span } => {
                format!(
                    "error at line {}: occurs check failed: cannot construct the infinite type ?T{} = {}",
                    self.span_to_line(*span),
                    var.0,
                    ty.description()
                )
            }
            TypeError::CannotInfer { span } => {
                format!(
                    "error at line {}: cannot infer a concrete type for this expression",
                    self.span_to_line(*span)
                )
            }
            TypeError::WrongArgumentCount { expected, found, span } => {
                format!(
                    "error at line {}: expected {} argument(s), found {}",
                    self.span_to_line(*span),
                    expected,
                    found
                )
            }
            TypeError::NotCallable { ty, span } => {
                format!(
                    "error at line {}: cannot call value of type {}",
                    self.span_to_line(*span),
                    ty.description()
                )
            }
            TypeError::NotAStruct { ty, span } => {
                format!(
                    "error at line {}: field access on non-struct type {}",
                    self.span_to_line(*span),
                    ty.description()
                )
            }
            TypeError::NoSuchField { ty, field, span } => {
                format!(
                    "error at line {}: type {} has no field `{}`",
                    self.span_to_line(*span),
                    ty.description(),
                    field.0
                )
            }
            TypeError::FieldTypeMismatch {
                struct_name,
                field,
                expected,
                found,
                span,
            } => {
                format!(
                    "error at line {}: field `{}::{}` expected type {}, found {}",
                    self.span_to_line(*span),
                    struct_name.0,
                    field.0,
                    expected.description(),
                    found.description()
                )
            }
        }
    }

    /// Renders an exhaustiveness checker error.
    pub fn render_exhaustiveness_error(&self, error: &ExhaustivenessError) -> String {
        match error {
            ExhaustivenessError::NonExhaustive { missing, span } => {
                let names: Vec<String> = missing.iter().map(|s| format!("`{}`", s.0)).collect();
                format!(
                    "error at line {}: non-exhaustive match: missing {}",
                    self.span_to_line(*span),
                    names.join(", ")
                )
            }
            ExhaustivenessError::UnreachableArm { span } => {
                format!(
                    "warning at line {}: unreachable match arm",
                    self.span_to_line(*span)
                )
            }
        }
    }

    /// Renders a runtime error.
    ///
    /// Note: RuntimeError is defined in ferric_vm, but for M1 we'll accept
    /// a generic approach. In M2+ this might be more sophisticated.
    pub fn render_runtime_error(&self, error: &ferric_vm::RuntimeError) -> String {
        use ferric_vm::RuntimeError;

        match error {
            RuntimeError::UndefinedVariable { name, span } => {
                format!(
                    "error at line {}: undefined variable `{}`",
                    self.span_to_line(*span),
                    name.0
                )
            }
            RuntimeError::UndefinedFunction { name, span } => {
                format!(
                    "error at line {}: undefined function `{}`",
                    self.span_to_line(*span),
                    name.0
                )
            }
            RuntimeError::TypeMismatch {
                expected,
                found,
                span,
            } => {
                format!(
                    "error at line {}: type mismatch: expected {}, found {}",
                    self.span_to_line(*span),
                    expected,
                    found
                )
            }
            RuntimeError::DivisionByZero { span } => {
                format!(
                    "error at line {}: division by zero",
                    self.span_to_line(*span)
                )
            }
            RuntimeError::StackOverflow { span } => {
                format!(
                    "error at line {}: stack overflow",
                    self.span_to_line(*span)
                )
            }
            RuntimeError::NativeFunctionError { message, span } => {
                format!(
                    "error at line {}: native function error: {}",
                    self.span_to_line(*span),
                    message
                )
            }
            RuntimeError::InvalidOperation { op, span } => {
                format!(
                    "error at line {}: invalid operation: {}",
                    self.span_to_line(*span),
                    op
                )
            }
            RuntimeError::NotCallable { span } => {
                format!(
                    "error at line {}: not a callable value",
                    self.span_to_line(*span)
                )
            }
            RuntimeError::WrongArgumentCount {
                expected,
                found,
                span,
            } => {
                format!(
                    "error at line {}: wrong number of arguments: expected {}, found {}",
                    self.span_to_line(*span),
                    expected,
                    found
                )
            }
            RuntimeError::StackUnderflow { span } => {
                format!(
                    "error at line {}: stack underflow (internal error)",
                    self.span_to_line(*span)
                )
            }
            RuntimeError::RequireError { span, message } => {
                if let Some(msg) = message {
                    format!(
                        "error at line {}: require failed: {}",
                        self.span_to_line(*span),
                        msg
                    )
                } else {
                    format!(
                        "error at line {}: require condition evaluated to false",
                        self.span_to_line(*span)
                    )
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferric_common::{LexError, ParseError, ResolveError, TypeError, Span, Symbol, TokenKind, Ty};

    #[test]
    fn test_span_to_line_single_line() {
        let source = "let x = 5".to_string();
        let renderer = Renderer::new(source);

        // Position 0 is line 1
        let span = Span::new(0, 3);
        assert_eq!(renderer.span_to_line(span), 1);

        // Position 5 is still line 1
        let span = Span::new(5, 6);
        assert_eq!(renderer.span_to_line(span), 1);
    }

    #[test]
    fn test_span_to_line_multi_line() {
        let source = "let x = 5\nlet y = 10\nlet z = 15".to_string();
        let renderer = Renderer::new(source);

        // First line
        let span = Span::new(0, 3);
        assert_eq!(renderer.span_to_line(span), 1);

        // Second line (starts at position 10, after first \n)
        let span = Span::new(10, 13);
        assert_eq!(renderer.span_to_line(span), 2);

        // Third line (starts at position 21, after second \n)
        let span = Span::new(21, 24);
        assert_eq!(renderer.span_to_line(span), 3);
    }

    #[test]
    fn test_span_at_position_zero() {
        let source = "test".to_string();
        let renderer = Renderer::new(source);

        let span = Span::new(0, 0);
        assert_eq!(renderer.span_to_line(span), 1);
    }

    #[test]
    fn test_render_lex_error_unexpected_char() {
        let source = "let x = @".to_string();
        let renderer = Renderer::new(source);

        let error = LexError::UnexpectedChar {
            ch: '@',
            span: Span::new(8, 9),
        };

        let rendered = renderer.render_lex_error(&error);
        assert_eq!(rendered, "error at line 1: unexpected character '@'");
    }

    #[test]
    fn test_render_lex_error_unterminated_string() {
        let source = "let x = \"hello".to_string();
        let renderer = Renderer::new(source);

        let error = LexError::UnterminatedString {
            span: Span::new(8, 14),
        };

        let rendered = renderer.render_lex_error(&error);
        assert_eq!(rendered, "error at line 1: unterminated string literal");
    }

    #[test]
    fn test_render_parse_error() {
        let source = "let x =".to_string();
        let renderer = Renderer::new(source);

        let error = ParseError::UnexpectedToken {
            expected: "expression".to_string(),
            found: TokenKind::Eof,
            span: Span::new(7, 7),
        };

        let rendered = renderer.render_parse_error(&error);
        assert!(rendered.contains("error at line 1"));
        assert!(rendered.contains("expected expression"));
    }

    #[test]
    fn test_render_resolve_error_undefined() {
        let source = "let x = y".to_string();
        let renderer = Renderer::new(source);

        let error = ResolveError::UndefinedVariable {
            name: Symbol::new(42),
            span: Span::new(8, 9),
        };

        let rendered = renderer.render_resolve_error(&error);
        assert_eq!(rendered, "error at line 1: undefined variable `42`");
    }

    #[test]
    fn test_render_resolve_error_duplicate() {
        let source = "let x = 1\nlet x = 2".to_string();
        let renderer = Renderer::new(source);

        let error = ResolveError::DuplicateDefinition {
            name: Symbol::new(5),
            first: Span::new(4, 5),
            second: Span::new(14, 15),
        };

        let rendered = renderer.render_resolve_error(&error);
        assert!(rendered.contains("error at line 2"));
        assert!(rendered.contains("duplicate definition"));
        assert!(rendered.contains("first defined at line 1"));
    }

    #[test]
    fn test_render_type_error_mismatch() {
        let source = "let x: Int = \"hello\"".to_string();
        let renderer = Renderer::new(source);

        let error = TypeError::Mismatch {
            expected: Ty::Int,
            found: Ty::Str,
            span: Span::new(13, 20),
        };

        let rendered = renderer.render_type_error(&error);
        assert_eq!(
            rendered,
            "error at line 1: type mismatch: expected int, found str"
        );
    }

    #[test]
    fn test_all_error_variants_produce_output() {
        let source = "test\ncode\nhere".to_string();
        let renderer = Renderer::new(source);

        // Test that all error types can be rendered
        let lex_err = LexError::UnexpectedChar {
            ch: '@',
            span: Span::new(0, 1),
        };
        assert!(!renderer.render_lex_error(&lex_err).is_empty());

        let parse_err = ParseError::ExpectedExpression {
            found: TokenKind::Eof,
            span: Span::new(0, 1),
        };
        assert!(!renderer.render_parse_error(&parse_err).is_empty());

        let resolve_err = ResolveError::UndefinedVariable {
            name: Symbol::new(1),
            span: Span::new(0, 1),
        };
        assert!(!renderer.render_resolve_error(&resolve_err).is_empty());

        let type_err = TypeError::Mismatch {
            expected: Ty::Int,
            found: Ty::Bool,
            span: Span::new(0, 1),
        };
        assert!(!renderer.render_type_error(&type_err).is_empty());
    }
}
