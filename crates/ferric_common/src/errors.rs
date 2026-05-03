//! Error types for all compiler stages.
//!
//! CRITICAL: Every error type MUST carry a Span field (Rule 5).
//! This enables precise error reporting and future renderer replacement.

use crate::{EffectRef, Interner, Span, Symbol, TokenKind, Ty, TyVar};
use serde::{Deserialize, Serialize};

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
    NestedShellInterp { span: Span },
    /// A shell interpolation `@{...}` had no closing `}` before end-of-line.
    UnclosedShellInterp { span: Span },
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
                format!("unexpected character '{ch}'")
            }
            LexError::UnterminatedString { .. } => "unterminated string literal".to_string(),
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
    /// An `import` declaration appeared after a non-import top-level item.
    LateImport { span: Span },
    /// A default-style import: `import X from "..."` (no braces, no `*`).
    DefaultImport { span: Span },
    /// An import path string did not match any of the three supported shapes.
    InvalidImportPath { span: Span },
    /// An `export` modifier appeared somewhere it is not legal (for example,
    /// inside a function body).
    InvalidExportPosition { span: Span },
    /// Two `as` casts chained without parentheses: `x as A as B`.
    ChainedCast { span: Span },
    /// A `|` appeared in expression position not followed by closure-parameter
    /// syntax. The lexer cannot reject `|` directly because `|x|` introduces a
    /// closure, so the rejection lives here as a single clear diagnostic
    /// instead of cascading through the closure-param parser.
    StrayPipe { span: Span },
    /// `await` appeared inside a non-`async` function — caught at parse time
    /// as a fast path. The type checker also catches this (with richer typing
    /// information), but emitting it here gives a clearer diagnostic and lets
    /// downstream stages skip the malformed body.
    AwaitOutsideAsync { span: Span },
    /// `async` was used as a prefix on something other than `fn` or `{`. The
    /// token after `async` was something like `let`, `struct`, or any other
    /// non-function form.
    AsyncOnNonFn { span: Span },
    /// An `effect` declaration appeared inside a function body. Effect
    /// declarations may only appear at module scope.
    EffectDeclInsideFn { span: Span },
    /// `resume` appeared outside of any handler clause body. The parser
    /// statically tracks handler-clause scope so this is caught early.
    ResumeOutsideHandler { span: Span },
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
            ParseError::LateImport { span } => *span,
            ParseError::DefaultImport { span } => *span,
            ParseError::InvalidImportPath { span } => *span,
            ParseError::InvalidExportPosition { span } => *span,
            ParseError::ChainedCast { span } => *span,
            ParseError::StrayPipe { span } => *span,
            ParseError::AwaitOutsideAsync { span } => *span,
            ParseError::AsyncOnNonFn { span } => *span,
            ParseError::EffectDeclInsideFn { span } => *span,
            ParseError::ResumeOutsideHandler { span } => *span,
        }
    }

    /// Returns a human-readable description of this error.
    pub fn description(&self) -> String {
        match self {
            ParseError::UnexpectedToken {
                expected, found, ..
            } => {
                format!("expected {}, found {}", expected, found.description())
            }
            ParseError::ExpectedExpression { found, .. } => {
                format!("expected expression, found {}", found.description())
            }
            ParseError::ExpectedStatement { found, .. } => {
                format!("expected statement, found {}", found.description())
            }
            ParseError::PositionalArg { .. } => {
                "positional arguments are not allowed; use named arguments (name: value)"
                    .to_string()
            }
            ParseError::InvalidRequireMode { .. } => {
                "invalid require mode; expected 'warn'".to_string()
            }
            ParseError::LateImport { .. } => {
                "import declarations must appear before other items".to_string()
            }
            ParseError::DefaultImport { .. } => {
                "default imports are not supported; use named imports `import { X } from \"...\"`"
                    .to_string()
            }
            ParseError::InvalidImportPath { .. } => {
                "invalid import path; expected `./`, `../`, `@/`, or a bare cache name".to_string()
            }
            ParseError::InvalidExportPosition { .. } => {
                "`export` is only allowed on top-level items".to_string()
            }
            ParseError::ChainedCast { .. } => {
                "cannot chain cast expressions; wrap in parentheses".to_string()
            }
            ParseError::StrayPipe { .. } => {
                "unexpected `|`; closures use `|param| body` and Ferric has no bitwise-or operator"
                    .to_string()
            }
            ParseError::AwaitOutsideAsync { .. } => {
                "`await` is only valid inside an `async fn` or `async { ... }` block".to_string()
            }
            ParseError::AsyncOnNonFn { .. } => {
                "`async` must be followed by `fn` or `{`".to_string()
            }
            ParseError::EffectDeclInsideFn { .. } => {
                "`effect` declarations are only allowed at module scope".to_string()
            }
            ParseError::ResumeOutsideHandler { .. } => {
                "`resume` is only legal inside a handler clause body".to_string()
            }
        }
    }
}

/// Non-fatal diagnostics produced by the parser.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ParseWarning {
    /// A handler-clause parameter shadowed the implicit continuation variable
    /// `k`. The clause body's references to `k` will resolve to the parameter,
    /// not the continuation — this is almost always a mistake.
    ShadowsContinuation { span: Span },
}

impl ParseWarning {
    /// Returns the span associated with this warning.
    pub fn span(&self) -> Span {
        match self {
            ParseWarning::ShadowsContinuation { span } => *span,
        }
    }

    /// Returns a human-readable description of this warning.
    pub fn description(&self) -> String {
        match self {
            ParseWarning::ShadowsContinuation { .. } => {
                "handler clause parameter shadows the implicit continuation variable `k`"
                    .to_string()
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
    /// A reference to an undefined struct or enum.
    UndefinedType { name: Symbol, span: Span },
    /// A struct literal mentioned a field name that doesn't belong to the struct.
    UnknownField {
        struct_name: Symbol,
        field: Symbol,
        span: Span,
    },
    /// A struct literal omitted a required field.
    MissingField {
        struct_name: Symbol,
        field: Symbol,
        span: Span,
    },
    /// A reference to an enum variant that doesn't exist.
    UnknownVariant {
        enum_name: Symbol,
        variant: Symbol,
        span: Span,
    },
    /// A variant pattern or constructor had the wrong number of arguments.
    VariantArity {
        enum_name: Symbol,
        variant: Symbol,
        expected: usize,
        found: usize,
        span: Span,
    },
    /// A named import targets an item that exists in the source file but is
    /// not marked `export`.
    PrivateImport {
        name: Symbol,
        path: String,
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
            ResolveError::UndefinedType { span, .. } => *span,
            ResolveError::UnknownField { span, .. } => *span,
            ResolveError::MissingField { span, .. } => *span,
            ResolveError::UnknownVariant { span, .. } => *span,
            ResolveError::VariantArity { span, .. } => *span,
            ResolveError::PrivateImport { span, .. } => *span,
        }
    }

    /// Returns a human-readable description of this error.
    pub fn description(&self) -> String {
        match self {
            ResolveError::UndefinedVariable { .. } => "undefined variable".to_string(),
            ResolveError::DuplicateDefinition { .. } => "duplicate definition".to_string(),
            ResolveError::AssignToImmutable { .. } => {
                "assignment to immutable variable".to_string()
            }
            ResolveError::BreakOutsideLoop { .. } => "break outside of loop".to_string(),
            ResolveError::ContinueOutsideLoop { .. } => "continue outside of loop".to_string(),
            ResolveError::ReturnOutsideFn { .. } => "return outside of function".to_string(),
            ResolveError::MissingArg { .. } => "missing required argument".to_string(),
            ResolveError::UnknownArg { .. } => "unknown argument name".to_string(),
            ResolveError::RequireSetArity { .. } => {
                "require set closure must take zero arguments".to_string()
            }
            ResolveError::UndefinedType { .. } => "undefined type".to_string(),
            ResolveError::UnknownField { .. } => "unknown field".to_string(),
            ResolveError::MissingField { .. } => "missing struct field".to_string(),
            ResolveError::UnknownVariant { .. } => "unknown enum variant".to_string(),
            ResolveError::VariantArity { .. } => {
                "wrong number of arguments to enum variant".to_string()
            }
            ResolveError::PrivateImport { path, .. } => {
                format!("imported name is not exported from \"{path}\"")
            }
        }
    }

    /// Returns a rich, user-facing message that resolves any `Symbol` fields
    /// through `interner`. This is the same one-liner the diagnostics renderer
    /// uses as its top-level message and is what the LSP shows in hovers.
    pub fn message(&self, interner: &Interner) -> String {
        let name = |s: Symbol| interner.resolve(s).to_string();
        match self {
            ResolveError::UndefinedVariable { name: n, .. } => {
                format!("undefined variable `{}`", name(*n))
            }
            ResolveError::DuplicateDefinition { name: n, .. } => {
                format!("duplicate definition of `{}`", name(*n))
            }
            ResolveError::AssignToImmutable { name: n, .. } => {
                format!("cannot assign to immutable variable `{}`", name(*n))
            }
            ResolveError::BreakOutsideLoop { .. } => "`break` used outside of a loop".to_string(),
            ResolveError::ContinueOutsideLoop { .. } => {
                "`continue` used outside of a loop".to_string()
            }
            ResolveError::ReturnOutsideFn { .. } => {
                "`return` used outside of a function".to_string()
            }
            ResolveError::MissingArg { param, .. } => {
                format!("missing required argument `{}`", name(*param))
            }
            ResolveError::UnknownArg { name: n, .. } => {
                format!("unknown argument name `{}`", name(*n))
            }
            ResolveError::RequireSetArity { .. } => {
                "the `set:` closure of a require must take zero parameters".to_string()
            }
            ResolveError::UndefinedType { name: n, .. } => {
                format!("undefined type `{}`", name(*n))
            }
            ResolveError::UnknownField {
                struct_name, field, ..
            } => format!(
                "struct `{}` has no field `{}`",
                name(*struct_name),
                name(*field)
            ),
            ResolveError::MissingField {
                struct_name, field, ..
            } => format!(
                "missing field `{}` in struct `{}` literal",
                name(*field),
                name(*struct_name)
            ),
            ResolveError::UnknownVariant {
                enum_name, variant, ..
            } => format!(
                "enum `{}` has no variant `{}`",
                name(*enum_name),
                name(*variant)
            ),
            ResolveError::VariantArity {
                enum_name,
                variant,
                expected,
                found,
                ..
            } => format!(
                "variant `{}::{}` expects {} field(s), got {}",
                name(*enum_name),
                name(*variant),
                expected,
                found
            ),
            ResolveError::PrivateImport { name: n, path, .. } => {
                format!("`{}` is not exported from \"{}\"", name(*n), path)
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
    RequireNonBool { found: Ty, span: Span },
    /// The require message expression is not Str
    RequireMessageNonStr { found: Ty, span: Span },
    /// The require set closure is not Fn() -> Unit
    RequireSetType { found: Ty, span: Span },
    /// A shell interpolation `@{expr}` has a type other than Str or Int.
    ShellInterpType { found: Ty, span: Span },
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
    /// A field-access expression was applied to a non-struct value.
    NotAStruct { ty: Ty, span: Span },
    /// A struct value did not have the named field.
    NoSuchField { ty: Ty, field: Symbol, span: Span },
    /// A field type didn't match its declared type in a struct literal.
    FieldTypeMismatch {
        struct_name: Symbol,
        field: Symbol,
        expected: Ty,
        found: Ty,
        span: Span,
    },
    /// A method was called on a type that doesn't implement any trait
    /// providing it.
    NoSuchMethod { ty: Ty, method: Symbol, span: Span },
    /// A trait bound on a generic call site was not satisfied: the type
    /// substituted for `type_param` doesn't implement `bound`.
    TraitBoundNotSatisfied {
        type_param: Symbol,
        bound: Symbol,
        ty: Ty,
        span: Span,
    },
    /// A trait was used as a bound but no such trait is defined.
    UnknownTrait { name: Symbol, span: Span },
    /// An impl block referred to a trait that doesn't exist.
    ImplOfUnknownTrait { trait_name: Symbol, span: Span },
    /// An impl block declared a method whose signature doesn't match the
    /// trait's declared signature.
    ImplMethodSignatureMismatch {
        trait_name: Symbol,
        method: Symbol,
        span: Span,
    },
    /// An opaque-typed value was used where its inner type (or a different
    /// opaque type) was expected — `as` was missing.
    OpaqueTypeMismatch { expected: Ty, found: Ty, span: Span },
    /// A cast expression `expr as TypeExpr` was neither a wrap nor an unwrap
    /// of an opaque type alias.
    InvalidCast { from: Ty, to: Ty, span: Span },
    /// `await` was used outside of an `async fn` or `async { ... }` block.
    /// The type checker catches this independently of the parser fast-path so
    /// that lowering can rely on the await-context invariant.
    AwaitOutsideAsync { span: Span },
    /// `await` was applied to a value whose type is not `Async<T>`.
    AwaitOnNonAsync { found: Ty, span: Span },
    /// An `async { ... }` block appeared in a position where its `Async<T>`
    /// result type cannot be used (e.g. a sync function that returns `Unit`
    /// and discards the value).
    AsyncBlockInSync { span: Span },
    /// `spawn(task: ...)` was called with an argument that is not `Async<T>`.
    SpawnNonAsync { found: Ty, span: Span },
    /// An expression of type `Async<T>` was used where the inner `T` was
    /// expected — the user almost always forgot to write `await`. Friendly
    /// alternative to a plain `Mismatch` for this very common case.
    AsyncNotAwaited { found: Ty, expected: Ty, span: Span },
    /// `join(...)` received an argument whose type is not `Handle<T>`.
    JoinNonHandle { found: Ty, span: Span },
    /// A `perform Effect::Op(...)` site appears in a function whose declared
    /// effect row does not contain `Effect`. M9: caught at compile time when
    /// the row is closed (annotated); open rows are constrained instead.
    UnhandledEffect {
        effect: Symbol,
        op: Symbol,
        span: Span,
    },
    /// The inferred effect row of an expression did not unify with the
    /// expected row from its context (e.g. an explicit `Eff<[E1], T>`
    /// annotation that doesn't match the body's actual effects). M9.
    EffectRowMismatch {
        expected: Vec<EffectRef>,
        found: Vec<EffectRef>,
        span: Span,
    },
    /// `resume k with v` had a value whose type does not match the declared
    /// `EffectOp::resume_type` of the handled operation. M9.
    ResumeTypeMismatch {
        expected: Ty,
        found: Ty,
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
            TypeError::NotAStruct { span, .. } => *span,
            TypeError::NoSuchField { span, .. } => *span,
            TypeError::FieldTypeMismatch { span, .. } => *span,
            TypeError::NoSuchMethod { span, .. } => *span,
            TypeError::TraitBoundNotSatisfied { span, .. } => *span,
            TypeError::UnknownTrait { span, .. } => *span,
            TypeError::ImplOfUnknownTrait { span, .. } => *span,
            TypeError::ImplMethodSignatureMismatch { span, .. } => *span,
            TypeError::OpaqueTypeMismatch { span, .. } => *span,
            TypeError::InvalidCast { span, .. } => *span,
            TypeError::AwaitOutsideAsync { span } => *span,
            TypeError::AwaitOnNonAsync { span, .. } => *span,
            TypeError::AsyncBlockInSync { span } => *span,
            TypeError::SpawnNonAsync { span, .. } => *span,
            TypeError::AsyncNotAwaited { span, .. } => *span,
            TypeError::JoinNonHandle { span, .. } => *span,
            TypeError::UnhandledEffect { span, .. } => *span,
            TypeError::EffectRowMismatch { span, .. } => *span,
            TypeError::ResumeTypeMismatch { span, .. } => *span,
        }
    }

    /// Returns a human-readable description of this error.
    pub fn description(&self) -> String {
        match self {
            TypeError::Mismatch {
                expected, found, ..
            } => {
                format!(
                    "type mismatch: expected {}, found {}",
                    expected.description(),
                    found.description()
                )
            }
            TypeError::IncompatibleTypes {
                operation,
                left,
                right,
                ..
            } => {
                format!(
                    "incompatible types for {}: {} and {}",
                    operation,
                    left.description(),
                    right.description()
                )
            }
            TypeError::RequireNonBool { found, .. } => {
                format!(
                    "require condition must be Bool, found {}",
                    found.description()
                )
            }
            TypeError::RequireMessageNonStr { found, .. } => {
                format!("require message must be Str, found {}", found.description())
            }
            TypeError::RequireSetType { found, .. } => {
                format!(
                    "require set closure must be Fn() -> Unit, found {}",
                    found.description()
                )
            }
            TypeError::ShellInterpType { found, .. } => {
                format!(
                    "shell interpolation must be Str or Int, found {}",
                    found.description()
                )
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
            TypeError::WrongArgumentCount {
                expected, found, ..
            } => {
                format!("expected {expected} argument(s), found {found}")
            }
            TypeError::NotCallable { ty, .. } => {
                format!("cannot call value of type {}", ty.description())
            }
            TypeError::NotAStruct { ty, .. } => {
                format!("field access on non-struct type {}", ty.description())
            }
            TypeError::NoSuchField { ty, .. } => {
                format!("type {} has no such field", ty.description())
            }
            TypeError::FieldTypeMismatch {
                expected, found, ..
            } => {
                format!(
                    "field type mismatch: expected {}, found {}",
                    expected.description(),
                    found.description()
                )
            }
            TypeError::NoSuchMethod { ty, .. } => {
                format!("no method found for type {}", ty.description())
            }
            TypeError::TraitBoundNotSatisfied { ty, .. } => {
                format!("trait bound not satisfied for type {}", ty.description())
            }
            TypeError::UnknownTrait { .. } => "unknown trait".to_string(),
            TypeError::ImplOfUnknownTrait { .. } => {
                "impl block targets an undefined trait".to_string()
            }
            TypeError::ImplMethodSignatureMismatch { .. } => {
                "impl method signature does not match trait declaration".to_string()
            }
            TypeError::OpaqueTypeMismatch {
                expected, found, ..
            } => {
                format!(
                    "opaque type mismatch: expected {}, found {}",
                    expected.description(),
                    found.description()
                )
            }
            TypeError::InvalidCast { from, to, .. } => {
                format!("cannot cast {} to {}", from.description(), to.description())
            }
            TypeError::AwaitOutsideAsync { .. } => {
                "`await` is only valid inside an `async fn` or `async { ... }` block".to_string()
            }
            TypeError::AwaitOnNonAsync { found, .. } => {
                format!(
                    "`await` requires an Async<T> operand, found {}",
                    found.description()
                )
            }
            TypeError::AsyncBlockInSync { .. } => {
                "`async { ... }` produces an Async<T> value that cannot be used in this position"
                    .to_string()
            }
            TypeError::SpawnNonAsync { found, .. } => {
                format!(
                    "spawn requires an Async<T> argument, found {}",
                    found.description()
                )
            }
            TypeError::AsyncNotAwaited {
                found, expected, ..
            } => {
                format!(
                    "this expression is {} but {} is expected — did you forget `await`?",
                    found.description(),
                    expected.description()
                )
            }
            TypeError::JoinNonHandle { found, .. } => {
                format!(
                    "join requires Handle<T> arguments, found {}",
                    found.description()
                )
            }
            TypeError::UnhandledEffect { .. } => {
                "perform expression has no enclosing handler".to_string()
            }
            TypeError::EffectRowMismatch { .. } => "effect row mismatch".to_string(),
            TypeError::ResumeTypeMismatch {
                expected, found, ..
            } => format!(
                "resume value type mismatch: expected {}, found {}",
                expected.description(),
                found.description()
            ),
        }
    }

    /// Returns a rich, user-facing message that resolves any `Symbol` fields
    /// through `interner`. Mirrors the top-level message string the diagnostics
    /// renderer produces, and is what the LSP shows in hovers.
    pub fn message(&self, interner: &Interner) -> String {
        let nm = |s: Symbol| interner.resolve(s).to_string();
        match self {
            TypeError::Mismatch {
                expected, found, ..
            } => format!(
                "type mismatch: expected {}, found {}",
                expected.description(),
                found.description()
            ),
            TypeError::IncompatibleTypes {
                operation,
                left,
                right,
                ..
            } => format!(
                "operator `{}` does not apply to `{}` and `{}`",
                operation,
                left.description(),
                right.description()
            ),
            TypeError::RequireNonBool { found, .. } => format!(
                "require condition must be Bool, found {}",
                found.description()
            ),
            TypeError::RequireMessageNonStr { found, .. } => {
                format!("require message must be Str, found {}", found.description())
            }
            TypeError::RequireSetType { found, .. } => format!(
                "require `set:` closure must have type fn() -> (), found {}",
                found.description()
            ),
            TypeError::ShellInterpType { found, .. } => format!(
                "shell interpolation must be Str or Int, found {}",
                found.description()
            ),
            TypeError::InfiniteType { var, ty, .. } => {
                format!("infinite type: ?T{} occurs in {}", var.0, ty.description())
            }
            TypeError::CannotInfer { .. } => {
                "type annotations needed: cannot infer type".to_string()
            }
            TypeError::WrongArgumentCount {
                expected, found, ..
            } => format!("expected {expected} argument(s), found {found}"),
            TypeError::NotCallable { ty, .. } => {
                format!("type `{}` is not callable", ty.description())
            }
            TypeError::NotAStruct { ty, .. } => {
                format!("type `{}` is not a struct", ty.description())
            }
            TypeError::NoSuchField { ty, field, .. } => {
                format!("type `{}` has no field `{}`", ty.description(), nm(*field))
            }
            TypeError::FieldTypeMismatch {
                struct_name,
                field,
                expected,
                found,
                ..
            } => format!(
                "field `{}::{}` expects {}, found {}",
                nm(*struct_name),
                nm(*field),
                expected.description(),
                found.description()
            ),
            TypeError::NoSuchMethod { ty, method, .. } => format!(
                "type `{}` has no method `{}`",
                ty.description(),
                nm(*method)
            ),
            TypeError::TraitBoundNotSatisfied {
                type_param,
                bound,
                ty,
                ..
            } => format!(
                "the trait bound `{}: {}` is not satisfied (got `{}`)",
                nm(*type_param),
                nm(*bound),
                ty.description()
            ),
            TypeError::UnknownTrait { name, .. } => {
                format!("unknown trait `{}`", nm(*name))
            }
            TypeError::ImplOfUnknownTrait { trait_name, .. } => {
                format!("impl of unknown trait `{}`", nm(*trait_name))
            }
            TypeError::ImplMethodSignatureMismatch {
                trait_name, method, ..
            } => format!(
                "impl method `{}::{}` does not match the trait signature",
                nm(*trait_name),
                nm(*method)
            ),
            TypeError::OpaqueTypeMismatch {
                expected, found, ..
            } => format!(
                "type mismatch: expected `{}`, found `{}`",
                expected.description(),
                found.description()
            ),
            TypeError::InvalidCast { from, to, .. } => format!(
                "cannot cast `{}` to `{}`",
                from.description(),
                to.description()
            ),
            TypeError::AwaitOutsideAsync { .. } => {
                "`await` is only valid inside an `async fn` or `async { ... }` block".to_string()
            }
            TypeError::AwaitOnNonAsync { found, .. } => format!(
                "`await` requires an `Async<T>` operand, found `{}`",
                found.description()
            ),
            TypeError::AsyncBlockInSync { .. } => {
                "`async { ... }` produces an `Async<T>` value that cannot be used here".to_string()
            }
            TypeError::SpawnNonAsync { found, .. } => format!(
                "`spawn` requires an `Async<T>` argument, found `{}`",
                found.description()
            ),
            TypeError::AsyncNotAwaited {
                found, expected, ..
            } => format!(
                "this expression is `{}` but `{}` is expected",
                found.description(),
                expected.description()
            ),
            TypeError::JoinNonHandle { found, .. } => format!(
                "`join` requires `Handle<T>` arguments, found `{}`",
                found.description()
            ),
            TypeError::UnhandledEffect { effect, op, .. } => format!(
                "no handler in scope for `{}::{}`",
                nm(*effect),
                nm(*op)
            ),
            TypeError::EffectRowMismatch {
                expected, found, ..
            } => format!(
                "effect row mismatch: expected {} effect(s), found {}",
                expected.len(),
                found.len()
            ),
            TypeError::ResumeTypeMismatch {
                expected, found, ..
            } => format!(
                "`resume` value type mismatch: expected `{}`, found `{}`",
                expected.description(),
                found.description()
            ),
        }
    }
}

/// Errors that can occur in the `ferric_async` lowering pass.
///
/// These complement the type errors caught earlier: the type checker rejects
/// `await` outside an async context and on the wrong types; the lowering
/// pass catches structural problems that are only visible after the full
/// function body is available for state machine construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AsyncLowerError {
    /// A local variable is captured by a state across an `await` point in a
    /// way the lowering pass cannot translate into a state field. The fix is
    /// usually to move the capture inside the awaited subexpression.
    CaptureAcrossAwait {
        name: Symbol,
        await_span: Span,
        capture_span: Span,
    },
    /// An `async fn` calls itself unconditionally, with no awaitable suspension
    /// point along the way — the resulting state machine would loop without
    /// ever yielding to the scheduler.
    InfiniteAsyncRecursion { fn_name: Symbol, span: Span },
}

impl AsyncLowerError {
    /// Returns the primary span associated with this error.
    pub fn span(&self) -> Span {
        match self {
            AsyncLowerError::CaptureAcrossAwait { await_span, .. } => *await_span,
            AsyncLowerError::InfiniteAsyncRecursion { span, .. } => *span,
        }
    }

    /// Returns a human-readable description of this error.
    pub fn description(&self) -> String {
        match self {
            AsyncLowerError::CaptureAcrossAwait { .. } => {
                "value captured across an `await` cannot be lowered into the state machine"
                    .to_string()
            }
            AsyncLowerError::InfiniteAsyncRecursion { .. } => {
                "async function recurses without an `await` — would loop without yielding"
                    .to_string()
            }
        }
    }
}

/// A non-fatal diagnostic emitted during async lowering. Warnings do not halt
/// compilation; the renderer prefixes them with `warning:` instead of
/// `error:`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AsyncWarning {
    pub span: Span,
    pub kind: AsyncWarningKind,
}

/// The categories of warning the async lowering pass can emit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AsyncWarningKind {
    /// A `$ ...` shell expression appears outside an `async` context, where it
    /// will block the thread instead of yielding to the scheduler.
    BlockingShell,
}

impl AsyncWarning {
    /// Returns the span associated with this warning.
    pub fn span(&self) -> Span {
        self.span
    }

    /// Returns a human-readable description of this warning.
    pub fn description(&self) -> String {
        match self.kind {
            AsyncWarningKind::BlockingShell => {
                "`$` shell expression outside an async context — this will block the thread"
                    .to_string()
            }
        }
    }
}

/// Errors discovered by the `ferric_effects` lowering / checking pass.
///
/// All variants carry a `Span` (Rule 5).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EffectError {
    /// A `perform Effect::Op(...)` had no enclosing handler clause for that
    /// `(effect, op)` pair.
    UnhandledEffect {
        effect: Symbol,
        op: Symbol,
        span: Span,
    },
    /// A handler clause's continuation `k` was resumed more than once.
    /// One-shot continuations only.
    ResumedTwice { cont: Symbol, span: Span },
    /// `resume k` appeared somewhere that is not lexically inside a handler
    /// clause body. The parser already rejects most of these; this is the
    /// fallback for cases that survive macro/sugar expansion.
    ResumeOutsideHandler { span: Span },
    /// The effect row of an expression did not match the row required by its
    /// context (e.g. calling a function that performs `Async` from a function
    /// whose declared row does not include `Async`).
    EffectRowMismatch {
        expected: Vec<crate::EffectRef>,
        found: Vec<crate::EffectRef>,
        span: Span,
    },
}

impl EffectError {
    /// Returns the span associated with this error.
    pub fn span(&self) -> Span {
        match self {
            EffectError::UnhandledEffect { span, .. } => *span,
            EffectError::ResumedTwice { span, .. } => *span,
            EffectError::ResumeOutsideHandler { span } => *span,
            EffectError::EffectRowMismatch { span, .. } => *span,
        }
    }

    /// Returns a human-readable description of this error (no interner access).
    pub fn description(&self) -> String {
        match self {
            EffectError::UnhandledEffect { .. } => {
                "perform expression has no enclosing handler".to_string()
            }
            EffectError::ResumedTwice { .. } => {
                "continuation resumed more than once (one-shot only)".to_string()
            }
            EffectError::ResumeOutsideHandler { .. } => {
                "`resume` is only legal inside a handler clause body".to_string()
            }
            EffectError::EffectRowMismatch { .. } => "effect row mismatch".to_string(),
        }
    }
}

/// Non-fatal diagnostic emitted during effect lowering.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EffectWarning {
    /// A handler clause bound a continuation `k` but never resumed it. The
    /// continuation is dropped — that may be intentional (e.g. early return)
    /// but is worth flagging.
    ContinuationDropped { cont: Symbol, span: Span },
}

impl EffectWarning {
    /// Returns the span associated with this warning.
    pub fn span(&self) -> Span {
        match self {
            EffectWarning::ContinuationDropped { span, .. } => *span,
        }
    }

    /// Returns a human-readable description of this warning.
    pub fn description(&self) -> String {
        match self {
            EffectWarning::ContinuationDropped { .. } => {
                "handler bound a continuation but never resumed it".to_string()
            }
        }
    }
}

/// Errors that can occur during exhaustiveness checking.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ExhaustivenessError {
    /// A `match` expression failed to cover every variant of an enum.
    NonExhaustive { missing: Vec<Symbol>, span: Span },
    /// A `match` arm can never be reached because earlier arms already
    /// match every value the scrutinee could take.
    UnreachableArm { span: Span },
}

impl ExhaustivenessError {
    pub fn span(&self) -> Span {
        match self {
            ExhaustivenessError::NonExhaustive { span, .. } => *span,
            ExhaustivenessError::UnreachableArm { span } => *span,
        }
    }

    pub fn description(&self) -> String {
        match self {
            ExhaustivenessError::NonExhaustive { .. } => "non-exhaustive match".to_string(),
            ExhaustivenessError::UnreachableArm { .. } => "unreachable match arm".to_string(),
        }
    }
}
