//! # Ferric Diagnostics (M6)
//!
//! Multi-label, rustc-style diagnostic renderer. Replaces the M1 baseline
//! line-prefix renderer wholesale. The public API (six `render_*` methods on
//! `Renderer`) is unchanged — every other stage feeds in errors that already
//! carry `Span` values (Rule 5), so no other crate needed touching for this
//! replacement.
//!
//! Output format:
//!
//! ```text
//! error: type mismatch
//!   --> input:8:18
//!    |
//!  8 |     x + 1.0
//!    |         ^^^ found `Float`
//! ```
//!
//! Multi-label diagnostics add secondary span pointers and trailing
//! `= note:` / `= help:` lines. `RuntimeError` and a few internal errors
//! use a synthetic span (`Span::new(0, 0)`) — the renderer falls back to a
//! header-only block in that case.

use ferric_common::{
    ExhaustivenessError, Interner, LexError, ManifestError, ModuleError, ParseError,
    ResolveError, Span, Symbol, TypeError,
};

/// Public entry point for diagnostics. Keep the surface stable: every other
/// stage consumes `Renderer` through these six methods.
pub struct Renderer<'a> {
    source: String,
    interner: Option<&'a Interner>,
    /// Cached byte offsets of every line start. Computed once at construction.
    line_starts: Vec<usize>,
}

impl<'a> Renderer<'a> {
    /// Convenience constructor that does not carry an interner. Symbol-bearing
    /// errors will fall back to printing the raw symbol id.
    pub fn new(source: String) -> Self {
        let line_starts = compute_line_starts(&source);
        Self { source, interner: None, line_starts }
    }

    /// Constructs a renderer that resolves `Symbol`s through `interner` so
    /// error messages print user-visible names instead of numeric ids.
    pub fn with_interner(source: String, interner: &'a Interner) -> Self {
        let line_starts = compute_line_starts(&source);
        Self { source, interner: Some(interner), line_starts }
    }

    fn name(&self, sym: Symbol) -> String {
        match self.interner {
            Some(i) => i.resolve(sym).to_string(),
            None => sym.0.to_string(),
        }
    }

    pub fn render_lex_error(&self, error: &LexError) -> String {
        match error {
            LexError::UnexpectedChar { ch, span } => self.render(Diag {
                kind: "error",
                message: &format!("unexpected character '{}'", ch),
                primary: Some(Label { span: *span, message: None }),
                secondary: vec![],
                notes: vec![],
                help: None,
            }),
            LexError::UnterminatedString { span } => self.render(Diag {
                kind: "error",
                message: "unterminated string literal",
                primary: Some(Label {
                    span: *span,
                    message: Some("string starts here".to_string()),
                }),
                secondary: vec![],
                notes: vec![],
                help: Some("add a closing `\"`".to_string()),
            }),
            LexError::NestedShellInterp { span } => self.render(Diag {
                kind: "error",
                message: "nested shell interpolation `@{` is not allowed",
                primary: Some(Label { span: *span, message: None }),
                secondary: vec![],
                notes: vec![],
                help: None,
            }),
            LexError::UnclosedShellInterp { span } => self.render(Diag {
                kind: "error",
                message: "unclosed shell interpolation: missing `}`",
                primary: Some(Label { span: *span, message: None }),
                secondary: vec![],
                notes: vec![],
                help: None,
            }),
        }
    }

    pub fn render_parse_error(&self, error: &ParseError) -> String {
        match error {
            ParseError::UnexpectedToken { expected, found, span } => self.render(Diag {
                kind: "error",
                message: &format!(
                    "expected {}, found {}",
                    expected,
                    found.description()
                ),
                primary: Some(Label { span: *span, message: None }),
                secondary: vec![],
                notes: vec![],
                help: None,
            }),
            ParseError::ExpectedExpression { found, span } => self.render(Diag {
                kind: "error",
                message: &format!("expected expression, found {}", found.description()),
                primary: Some(Label { span: *span, message: None }),
                secondary: vec![],
                notes: vec![],
                help: None,
            }),
            ParseError::ExpectedStatement { found, span } => self.render(Diag {
                kind: "error",
                message: &format!("expected statement, found {}", found.description()),
                primary: Some(Label { span: *span, message: None }),
                secondary: vec![],
                notes: vec![],
                help: None,
            }),
            ParseError::PositionalArg { span } => self.render(Diag {
                kind: "error",
                message: "positional arguments are not allowed",
                primary: Some(Label {
                    span: *span,
                    message: Some("use `name: value`".to_string()),
                }),
                secondary: vec![],
                notes: vec![],
                help: Some("Ferric requires named arguments at every call site".to_string()),
            }),
            ParseError::InvalidRequireMode { span, .. } => self.render(Diag {
                kind: "error",
                message: "invalid require mode (expected `error` or `warn`)",
                primary: Some(Label { span: *span, message: None }),
                secondary: vec![],
                notes: vec![],
                help: None,
            }),
            ParseError::LateImport { span } => self.render(Diag {
                kind: "error",
                message: "import declarations must appear before other items",
                primary: Some(Label {
                    span: *span,
                    message: Some("move this import to the top of the file".to_string()),
                }),
                secondary: vec![],
                notes: vec![],
                help: None,
            }),
            ParseError::DefaultImport { span } => self.render(Diag {
                kind: "error",
                message: "default imports are not supported in Ferric",
                primary: Some(Label {
                    span: *span,
                    message: Some("use named imports".to_string()),
                }),
                secondary: vec![],
                notes: vec![],
                help: Some(
                    "rewrite as `import { name } from \"./path\"`".to_string(),
                ),
            }),
            ParseError::InvalidImportPath { span } => self.render(Diag {
                kind: "error",
                message: "invalid import path",
                primary: Some(Label { span: *span, message: None }),
                secondary: vec![],
                notes: vec![],
                help: Some(
                    "expected `./...`, `../...`, `@/...`, or a bare cache name".to_string(),
                ),
            }),
            ParseError::InvalidExportPosition { span } => self.render(Diag {
                kind: "error",
                message: "`export` is only allowed on top-level items",
                primary: Some(Label { span: *span, message: None }),
                secondary: vec![],
                notes: vec![],
                help: None,
            }),
            ParseError::ChainedCast { span } => self.render(Diag {
                kind: "error",
                message: "cannot chain cast expressions",
                primary: Some(Label {
                    span: *span,
                    message: Some("wrap in parentheses: `(x as A) as B`".to_string()),
                }),
                secondary: vec![],
                notes: vec![],
                help: None,
            }),
            ParseError::StrayPipe { span } => self.render(Diag {
                kind: "error",
                message: "unexpected `|`",
                primary: Some(Label {
                    span: *span,
                    message: Some("not a valid expression here".to_string()),
                }),
                secondary: vec![],
                notes: vec![
                    "closures use `|param| body`; Ferric has no bitwise-or operator".to_string(),
                ],
                help: None,
            }),
        }
    }

    pub fn render_resolve_error(&self, error: &ResolveError) -> String {
        match error {
            ResolveError::UndefinedVariable { name, span } => self.render(Diag {
                kind: "error",
                message: &format!("undefined variable `{}`", self.name(*name)),
                primary: Some(Label { span: *span, message: Some("not in scope".to_string()) }),
                secondary: vec![],
                notes: vec![],
                help: None,
            }),
            ResolveError::DuplicateDefinition { name, first, second } => self.render(Diag {
                kind: "error",
                message: &format!("duplicate definition of `{}`", self.name(*name)),
                primary: Some(Label {
                    span: *second,
                    message: Some("redefined here".to_string()),
                }),
                secondary: vec![Label {
                    span: *first,
                    message: Some("first defined here".to_string()),
                }],
                notes: vec![],
                help: None,
            }),
            ResolveError::AssignToImmutable { name, span } => self.render(Diag {
                kind: "error",
                message: &format!("cannot assign to immutable variable `{}`", self.name(*name)),
                primary: Some(Label { span: *span, message: None }),
                secondary: vec![],
                notes: vec![],
                help: Some("declare with `let mut` to allow reassignment".to_string()),
            }),
            ResolveError::BreakOutsideLoop { span } => self.render(Diag {
                kind: "error",
                message: "`break` used outside of a loop",
                primary: Some(Label { span: *span, message: None }),
                secondary: vec![],
                notes: vec![],
                help: None,
            }),
            ResolveError::ContinueOutsideLoop { span } => self.render(Diag {
                kind: "error",
                message: "`continue` used outside of a loop",
                primary: Some(Label { span: *span, message: None }),
                secondary: vec![],
                notes: vec![],
                help: None,
            }),
            ResolveError::ReturnOutsideFn { span } => self.render(Diag {
                kind: "error",
                message: "`return` used outside of a function",
                primary: Some(Label { span: *span, message: None }),
                secondary: vec![],
                notes: vec![],
                help: None,
            }),
            ResolveError::MissingArg { param, call_span } => self.render(Diag {
                kind: "error",
                message: &format!("missing required argument `{}`", self.name(*param)),
                primary: Some(Label { span: *call_span, message: None }),
                secondary: vec![],
                notes: vec![],
                help: None,
            }),
            ResolveError::UnknownArg { name, span } => self.render(Diag {
                kind: "error",
                message: &format!("unknown argument name `{}`", self.name(*name)),
                primary: Some(Label { span: *span, message: None }),
                secondary: vec![],
                notes: vec![],
                help: None,
            }),
            ResolveError::RequireSetArity { span } => self.render(Diag {
                kind: "error",
                message: "the `set:` closure of a require must take zero parameters",
                primary: Some(Label { span: *span, message: None }),
                secondary: vec![],
                notes: vec![],
                help: None,
            }),
            ResolveError::UndefinedType { name, span } => self.render(Diag {
                kind: "error",
                message: &format!("undefined type `{}`", self.name(*name)),
                primary: Some(Label { span: *span, message: None }),
                secondary: vec![],
                notes: vec![],
                help: None,
            }),
            ResolveError::UnknownField { struct_name, field, span } => self.render(Diag {
                kind: "error",
                message: &format!("struct `{}` has no field `{}`", self.name(*struct_name), self.name(*field)),
                primary: Some(Label { span: *span, message: None }),
                secondary: vec![],
                notes: vec![],
                help: None,
            }),
            ResolveError::MissingField { struct_name, field, span } => self.render(Diag {
                kind: "error",
                message: &format!(
                    "missing field `{}` in struct `{}` literal",
                    self.name(*field), self.name(*struct_name)
                ),
                primary: Some(Label { span: *span, message: None }),
                secondary: vec![],
                notes: vec![],
                help: None,
            }),
            ResolveError::UnknownVariant { enum_name, variant, span } => self.render(Diag {
                kind: "error",
                message: &format!(
                    "enum `{}` has no variant `{}`",
                    self.name(*enum_name), self.name(*variant)
                ),
                primary: Some(Label { span: *span, message: None }),
                secondary: vec![],
                notes: vec![],
                help: None,
            }),
            ResolveError::VariantArity {
                enum_name,
                variant,
                expected,
                found,
                span,
            } => self.render(Diag {
                kind: "error",
                message: &format!(
                    "variant `{}::{}` expects {} field(s), got {}",
                    self.name(*enum_name), self.name(*variant), expected, found
                ),
                primary: Some(Label { span: *span, message: None }),
                secondary: vec![],
                notes: vec![],
                help: None,
            }),
            ResolveError::PrivateImport { name, path, span } => self.render(Diag {
                kind: "error",
                message: &format!(
                    "`{}` is not exported from \"{}\"",
                    self.name(*name), path
                ),
                primary: Some(Label {
                    span: *span,
                    message: Some("not marked `export`".to_string()),
                }),
                secondary: vec![],
                notes: vec![],
                help: Some(format!(
                    "add `export` to the definition in \"{}\"",
                    path
                )),
            }),
        }
    }

    /// Renders a manifest-loading error.
    pub fn render_manifest_error(&self, error: &ManifestError) -> String {
        match error {
            ManifestError::ParseError { message, span } => self.render(Diag {
                kind: "error",
                message: &format!("failed to parse Ferric.toml: {}", message),
                primary: Some(Label { span: *span, message: None }),
                secondary: vec![],
                notes: vec![],
                help: None,
            }),
            ManifestError::ConflictingManifest { path, span } => self.render(Diag {
                kind: "error",
                message: &format!("submodule `{}` has its own Ferric.toml", path),
                primary: Some(Label { span: *span, message: None }),
                secondary: vec![],
                notes: vec![],
                help: Some(
                    "remove the nested manifest — submodules share the workspace root's Ferric.toml"
                        .to_string(),
                ),
            }),
        }
    }

    /// Renders a module-resolution error.
    pub fn render_module_error(&self, error: &ModuleError) -> String {
        match error {
            ModuleError::CircularImport { cycle, span } => {
                let chain = cycle.join(" → ");
                self.render(Diag {
                    kind: "error",
                    message: "circular import",
                    primary: Some(Label {
                        span: *span,
                        message: Some(format!("cycle: {}", chain)),
                    }),
                    secondary: vec![],
                    notes: vec![],
                    help: None,
                })
            }
            ModuleError::UnknownExport { name, path, span } => self.render(Diag {
                kind: "error",
                message: &format!(
                    "`{}` is not exported from \"{}\"",
                    self.name(*name),
                    path
                ),
                primary: Some(Label {
                    span: *span,
                    message: Some(format!("not exported in {}", path)),
                }),
                secondary: vec![],
                notes: vec![],
                help: Some(format!(
                    "add `export` to the definition of `{}` in \"{}\"",
                    self.name(*name),
                    path
                )),
            }),
            ModuleError::NoManifest { path, span } => self.render(Diag {
                kind: "error",
                message: &format!("import `{}` requires a Ferric.toml manifest", path),
                primary: Some(Label {
                    span: *span,
                    message: Some("no Ferric.toml found in workspace root".to_string()),
                }),
                secondary: vec![],
                notes: vec![],
                help: Some("run `ferric init` to create a manifest".to_string()),
            }),
            ModuleError::CacheMiss { name, span } => self.render(Diag {
                kind: "error",
                message: &format!(
                    "cache package `{}` not found in .ferric/cache/",
                    name
                ),
                primary: Some(Label {
                    span: *span,
                    message: Some("missing from cache".to_string()),
                }),
                secondary: vec![],
                notes: vec![],
                help: Some("run `ferric fetch` to populate the cache".to_string()),
            }),
            ModuleError::DefaultImport { span } => self.render(Diag {
                kind: "error",
                message: "default imports are not supported in Ferric",
                primary: Some(Label {
                    span: *span,
                    message: Some("use named imports".to_string()),
                }),
                secondary: vec![],
                notes: vec![],
                help: Some(
                    "rewrite as `import { name } from \"./path\"`".to_string(),
                ),
            }),
        }
    }

    pub fn render_type_error(&self, error: &TypeError) -> String {
        match error {
            TypeError::Mismatch { expected, found, span } => self.render(Diag {
                kind: "error",
                message: &format!(
                    "type mismatch: expected {}, found {}",
                    expected.description(),
                    found.description()
                ),
                primary: Some(Label {
                    span: *span,
                    message: Some(format!("found `{}`", found.description())),
                }),
                secondary: vec![],
                notes: vec![],
                help: None,
            }),
            TypeError::IncompatibleTypes { left, right, operation, span } => self.render(Diag {
                kind: "error",
                message: &format!(
                    "operator `{}` does not apply to `{}` and `{}`",
                    operation,
                    left.description(),
                    right.description()
                ),
                primary: Some(Label { span: *span, message: None }),
                secondary: vec![],
                notes: vec![],
                help: None,
            }),
            TypeError::RequireNonBool { found, span } => self.render(Diag {
                kind: "error",
                message: &format!(
                    "require condition must be Bool, found {}",
                    found.description()
                ),
                primary: Some(Label { span: *span, message: None }),
                secondary: vec![],
                notes: vec![],
                help: None,
            }),
            TypeError::RequireMessageNonStr { found, span } => self.render(Diag {
                kind: "error",
                message: &format!(
                    "require message must be Str, found {}",
                    found.description()
                ),
                primary: Some(Label { span: *span, message: None }),
                secondary: vec![],
                notes: vec![],
                help: None,
            }),
            TypeError::RequireSetType { found, span } => self.render(Diag {
                kind: "error",
                message: &format!(
                    "require `set:` closure must have type fn() -> (), found {}",
                    found.description()
                ),
                primary: Some(Label { span: *span, message: None }),
                secondary: vec![],
                notes: vec![],
                help: None,
            }),
            TypeError::ShellInterpType { found, span } => self.render(Diag {
                kind: "error",
                message: &format!(
                    "shell interpolation must be Str or Int, found {}",
                    found.description()
                ),
                primary: Some(Label { span: *span, message: None }),
                secondary: vec![],
                notes: vec![],
                help: None,
            }),
            TypeError::InfiniteType { var, ty, span } => self.render(Diag {
                kind: "error",
                message: &format!(
                    "infinite type: ?T{} occurs in {}",
                    var.0,
                    ty.description()
                ),
                primary: Some(Label { span: *span, message: None }),
                secondary: vec![],
                notes: vec![],
                help: None,
            }),
            TypeError::CannotInfer { span } => self.render(Diag {
                kind: "error",
                message: "type annotations needed: cannot infer type",
                primary: Some(Label { span: *span, message: None }),
                secondary: vec![],
                notes: vec![],
                help: Some("add an explicit type annotation".to_string()),
            }),
            TypeError::WrongArgumentCount { expected, found, span } => self.render(Diag {
                kind: "error",
                message: &format!(
                    "expected {} argument(s), found {}",
                    expected, found
                ),
                primary: Some(Label { span: *span, message: None }),
                secondary: vec![],
                notes: vec![],
                help: None,
            }),
            TypeError::NotCallable { ty, span } => self.render(Diag {
                kind: "error",
                message: &format!(
                    "type `{}` is not callable",
                    ty.description()
                ),
                primary: Some(Label { span: *span, message: None }),
                secondary: vec![],
                notes: vec![],
                help: None,
            }),
            TypeError::NotAStruct { ty, span } => self.render(Diag {
                kind: "error",
                message: &format!(
                    "type `{}` is not a struct",
                    ty.description()
                ),
                primary: Some(Label { span: *span, message: None }),
                secondary: vec![],
                notes: vec![],
                help: None,
            }),
            TypeError::NoSuchField { ty, field, span } => self.render(Diag {
                kind: "error",
                message: &format!(
                    "type `{}` has no field `{}`",
                    ty.description(), self.name(*field)
                ),
                primary: Some(Label { span: *span, message: None }),
                secondary: vec![],
                notes: vec![],
                help: None,
            }),
            TypeError::FieldTypeMismatch {
                struct_name,
                field,
                expected,
                found,
                span,
            } => self.render(Diag {
                kind: "error",
                message: &format!(
                    "field `{}::{}` expects {}, found {}",
                    self.name(*struct_name),
                    self.name(*field),
                    expected.description(),
                    found.description()
                ),
                primary: Some(Label { span: *span, message: None }),
                secondary: vec![],
                notes: vec![],
                help: None,
            }),
            TypeError::NoSuchMethod { ty, method, span } => self.render(Diag {
                kind: "error",
                message: &format!(
                    "type `{}` has no method `{}`",
                    ty.description(),
                    self.name(*method)
                ),
                primary: Some(Label { span: *span, message: None }),
                secondary: vec![],
                notes: vec![],
                help: None,
            }),
            TypeError::TraitBoundNotSatisfied {
                type_param,
                bound,
                ty,
                span,
            } => self.render(Diag {
                kind: "error",
                message: &format!(
                    "the trait bound `{}: {}` is not satisfied (got `{}`)",
                    self.name(*type_param),
                    self.name(*bound),
                    ty.description()
                ),
                primary: Some(Label { span: *span, message: None }),
                secondary: vec![],
                notes: vec![],
                help: None,
            }),
            TypeError::UnknownTrait { name, span } => self.render(Diag {
                kind: "error",
                message: &format!("unknown trait `{}`", self.name(*name)),
                primary: Some(Label { span: *span, message: None }),
                secondary: vec![],
                notes: vec![],
                help: None,
            }),
            TypeError::ImplOfUnknownTrait { trait_name, span } => self.render(Diag {
                kind: "error",
                message: &format!("impl of unknown trait `{}`", self.name(*trait_name)),
                primary: Some(Label { span: *span, message: None }),
                secondary: vec![],
                notes: vec![],
                help: None,
            }),
            TypeError::ImplMethodSignatureMismatch { trait_name, method, span } => self.render(Diag {
                kind: "error",
                message: &format!(
                    "impl method `{}::{}` does not match the trait signature",
                    self.name(*trait_name), self.name(*method)
                ),
                primary: Some(Label { span: *span, message: None }),
                secondary: vec![],
                notes: vec![],
                help: None,
            }),
            TypeError::OpaqueTypeMismatch { expected, found, span } => self.render(Diag {
                kind: "error",
                message: &format!(
                    "type mismatch: expected `{}`, found `{}`",
                    expected.description(),
                    found.description()
                ),
                primary: Some(Label {
                    span: *span,
                    message: Some(format!("found `{}`", found.description())),
                }),
                secondary: vec![],
                notes: vec![],
                help: Some(format!(
                    "use `as {}` to construct or unwrap the opaque type",
                    expected.description()
                )),
            }),
            TypeError::InvalidCast { from, to, span } => self.render(Diag {
                kind: "error",
                message: &format!(
                    "cannot cast `{}` to `{}`",
                    from.description(),
                    to.description()
                ),
                primary: Some(Label { span: *span, message: None }),
                secondary: vec![],
                notes: vec![],
                help: Some(
                    "casts may only wrap or unwrap a single opaque type alias"
                        .to_string(),
                ),
            }),
        }
    }

    pub fn render_exhaustiveness_error(&self, error: &ExhaustivenessError) -> String {
        match error {
            ExhaustivenessError::NonExhaustive { missing, span } => {
                let names: Vec<String> = missing
                    .iter()
                    .map(|s| format!("`{}`", self.name(*s)))
                    .collect();
                self.render(Diag {
                    kind: "error",
                    message: "non-exhaustive match",
                    primary: Some(Label {
                        span: *span,
                        message: Some(format!("missing patterns: {}", names.join(", "))),
                    }),
                    secondary: vec![],
                    notes: vec![],
                    help: Some(
                        "add a wildcard arm `_ => ...` to cover remaining cases".to_string(),
                    ),
                })
            }
            ExhaustivenessError::UnreachableArm { span } => self.render(Diag {
                kind: "warning",
                message: "unreachable match arm",
                primary: Some(Label { span: *span, message: None }),
                secondary: vec![],
                notes: vec![],
                help: None,
            }),
        }
    }

    pub fn render_runtime_error(&self, error: &ferric_vm::RuntimeError) -> String {
        use ferric_vm::RuntimeError;

        // Most runtime errors carry a sentinel span (Span::new(0, 0)) because
        // the bytecode VM does not yet thread spans through every op. The
        // renderer falls back to a header-only block when the span is null.
        match error {
            RuntimeError::UndefinedVariable { name, span } => self.render(Diag {
                kind: "error",
                message: &format!("undefined variable `{}`", self.name(*name)),
                primary: nonzero_label(*span),
                secondary: vec![],
                notes: vec![],
                help: None,
            }),
            RuntimeError::UndefinedFunction { name, span } => self.render(Diag {
                kind: "error",
                message: &format!("undefined function `{}`", self.name(*name)),
                primary: nonzero_label(*span),
                secondary: vec![],
                notes: vec![],
                help: None,
            }),
            RuntimeError::TypeMismatch { expected, found, span } => self.render(Diag {
                kind: "error",
                message: &format!("runtime type mismatch: expected {}, found {}", expected, found),
                primary: nonzero_label(*span),
                secondary: vec![],
                notes: vec![],
                help: None,
            }),
            RuntimeError::DivisionByZero { span } => self.render(Diag {
                kind: "error",
                message: "division by zero",
                primary: nonzero_label(*span),
                secondary: vec![],
                notes: vec![],
                help: None,
            }),
            RuntimeError::StackOverflow { span } => self.render(Diag {
                kind: "error",
                message: "stack overflow",
                primary: nonzero_label(*span),
                secondary: vec![],
                notes: vec![],
                help: None,
            }),
            RuntimeError::StackUnderflow { span } => self.render(Diag {
                kind: "error",
                message: "stack underflow (internal VM error)",
                primary: nonzero_label(*span),
                secondary: vec![],
                notes: vec![],
                help: None,
            }),
            RuntimeError::NativeFunctionError { message, span } => self.render(Diag {
                kind: "error",
                message: &format!("native function error: {}", message),
                primary: nonzero_label(*span),
                secondary: vec![],
                notes: vec![],
                help: None,
            }),
            RuntimeError::InvalidOperation { op, span } => self.render(Diag {
                kind: "error",
                message: &format!("invalid operation: {}", op),
                primary: nonzero_label(*span),
                secondary: vec![],
                notes: vec![],
                help: None,
            }),
            RuntimeError::NotCallable { span } => self.render(Diag {
                kind: "error",
                message: "value is not callable",
                primary: nonzero_label(*span),
                secondary: vec![],
                notes: vec![],
                help: None,
            }),
            RuntimeError::WrongArgumentCount { expected, found, span } => self.render(Diag {
                kind: "error",
                message: &format!("expected {} argument(s), found {}", expected, found),
                primary: nonzero_label(*span),
                secondary: vec![],
                notes: vec![],
                help: None,
            }),
            RuntimeError::RequireError { span, message } => {
                let msg = message.clone().unwrap_or_else(|| {
                    "require condition evaluated to false".to_string()
                });
                self.render(Diag {
                    kind: "error",
                    message: &format!("require failed: {}", msg),
                    primary: nonzero_label(*span),
                    secondary: vec![],
                    notes: vec![],
                    help: None,
                })
            }
            RuntimeError::IndexOutOfBounds { index, len, span } => self.render(Diag {
                kind: "error",
                message: &format!(
                    "array index {} out of bounds (length {})",
                    index, len
                ),
                primary: nonzero_label(*span),
                secondary: vec![],
                notes: vec![],
                help: None,
            }),
            RuntimeError::NotAnArray { found, span } => self.render(Diag {
                kind: "error",
                message: &format!("indexing requires an array, found {}", found),
                primary: nonzero_label(*span),
                secondary: vec![],
                notes: vec![],
                help: None,
            }),
            RuntimeError::IntegerOverflow { op, span } => self.render(Diag {
                kind: "error",
                message: &format!("integer overflow in `{}`", op),
                primary: nonzero_label(*span),
                secondary: vec![],
                notes: vec![],
                help: None,
            }),
        }
    }

    // ---------------------------------------------------------------- Layout

    fn render(&self, diag: Diag) -> String {
        let mut out = String::new();
        out.push_str(diag.kind);
        out.push_str(": ");
        out.push_str(diag.message);

        let primary = match diag.primary {
            Some(p) => p,
            None => return out,
        };

        let (line, col) = self.span_to_line_col(primary.span);
        out.push_str(&format!("\n  --> input:{}:{}\n", line, col));

        let gutter_width = max_gutter_width(&primary, &diag.secondary);
        let blank_gutter = " ".repeat(gutter_width);

        out.push_str(&format!("{} |\n", blank_gutter));
        self.render_label(&mut out, &primary, gutter_width, /*primary*/ true);

        for label in &diag.secondary {
            self.render_label(&mut out, label, gutter_width, false);
        }

        for note in &diag.notes {
            out.push_str(&format!("{} = note: {}\n", blank_gutter, note));
        }
        if let Some(help) = diag.help {
            out.push_str(&format!("{} = help: {}\n", blank_gutter, help));
        }

        // Trim trailing newline for ergonomic eprintln.
        if out.ends_with('\n') {
            out.pop();
        }
        out
    }

    fn render_label(&self, out: &mut String, label: &Label, gutter_width: usize, is_primary: bool) {
        let (line, col) = self.span_to_line_col(label.span);
        let gutter = format!("{:>width$}", line, width = gutter_width);
        let line_text = self.line_text(line);
        out.push_str(&format!("{} | {}\n", gutter, line_text));

        let blank_gutter = " ".repeat(gutter_width);
        let mut underline = " ".repeat(col.saturating_sub(1));
        let span_len = (label.span.end - label.span.start).max(1) as usize;
        // Cap underline length to the rest of the visible source line so we
        // never extend past the line's end.
        let visible = line_text.chars().count();
        let underline_len = span_len.min(visible.saturating_sub(col.saturating_sub(1))).max(1);
        let glyph = if is_primary { '^' } else { '-' };
        underline.extend(std::iter::repeat(glyph).take(underline_len));

        let suffix = label
            .message
            .as_deref()
            .map(|m| format!(" {}", m))
            .unwrap_or_default();
        out.push_str(&format!("{} | {}{}\n", blank_gutter, underline, suffix));
    }

    fn span_to_line_col(&self, span: Span) -> (usize, usize) {
        let pos = span.start as usize;
        // line_starts is sorted; partition_point gives us the line whose start
        // is the largest one <= pos.
        let line_idx = self
            .line_starts
            .partition_point(|&start| start <= pos)
            .saturating_sub(1);
        let line_start = self.line_starts.get(line_idx).copied().unwrap_or(0);
        let col = self.source[line_start..pos].chars().count() + 1;
        (line_idx + 1, col)
    }

    fn line_text(&self, line: usize) -> &str {
        let idx = line.saturating_sub(1);
        let start = self.line_starts.get(idx).copied().unwrap_or(0);
        let end = self
            .line_starts
            .get(idx + 1)
            .copied()
            .unwrap_or(self.source.len());
        let raw = &self.source[start..end];
        // Trim a trailing newline so we don't print an extra blank line.
        raw.trim_end_matches(|c: char| c == '\n' || c == '\r')
    }
}

// ============================================================================
// Internal types
// ============================================================================

struct Diag<'a> {
    kind: &'a str,
    message: &'a str,
    primary: Option<Label>,
    secondary: Vec<Label>,
    notes: Vec<String>,
    help: Option<String>,
}

#[derive(Debug, Clone)]
struct Label {
    span: Span,
    message: Option<String>,
}

fn compute_line_starts(source: &str) -> Vec<usize> {
    let mut starts = vec![0];
    for (i, b) in source.bytes().enumerate() {
        if b == b'\n' {
            starts.push(i + 1);
        }
    }
    starts
}

fn max_gutter_width(primary: &Label, secondary: &[Label]) -> usize {
    // Width is the max line-number digit count across all labels, with a
    // floor of 1 so trivial errors don't crash the layout.
    let max_line = std::iter::once(primary)
        .chain(secondary.iter())
        .map(|l| l.span.start as usize)
        .max()
        .unwrap_or(0);
    let estimated_lines = max_line / 10 + 2;
    estimated_lines.to_string().len().max(1)
}

fn nonzero_label(span: Span) -> Option<Label> {
    if span.start == 0 && span.end == 0 {
        None
    } else {
        Some(Label { span, message: None })
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use ferric_common::{Span, Symbol, TokenKind, Ty};

    #[test]
    fn renders_unexpected_char_with_caret() {
        let source = "let x = @".to_string();
        let renderer = Renderer::new(source);
        let error = LexError::UnexpectedChar { ch: '@', span: Span::new(8, 9) };
        let out = renderer.render_lex_error(&error);
        assert!(out.starts_with("error: unexpected character '@'"));
        assert!(out.contains("--> input:1:9"));
        assert!(out.contains("let x = @"));
        assert!(out.contains("^"));
    }

    #[test]
    fn renders_duplicate_def_with_secondary_label() {
        let source = "let x = 1\nlet x = 2".to_string();
        let renderer = Renderer::new(source);
        let error = ResolveError::DuplicateDefinition {
            name: Symbol::new(5),
            first: Span::new(4, 5),
            second: Span::new(14, 15),
        };
        let out = renderer.render_resolve_error(&error);
        assert!(out.starts_with("error: duplicate definition"));
        assert!(out.contains("--> input:2:5"));
        assert!(out.contains("redefined here"));
        assert!(out.contains("first defined here"));
    }

    #[test]
    fn renders_type_mismatch_help() {
        let source = "let x: Int = \"hi\"".to_string();
        let renderer = Renderer::new(source);
        let error = TypeError::Mismatch {
            expected: Ty::Int,
            found: Ty::Str,
            span: Span::new(13, 17),
        };
        let out = renderer.render_type_error(&error);
        assert!(out.contains("type mismatch"));
        assert!(out.contains("found `str`"));
    }

    #[test]
    fn parse_error_includes_token_description() {
        let source = "let x =".to_string();
        let renderer = Renderer::new(source);
        let error = ParseError::UnexpectedToken {
            expected: "expression".to_string(),
            found: TokenKind::Eof,
            span: Span::new(7, 7),
        };
        let out = renderer.render_parse_error(&error);
        assert!(out.contains("expected expression"));
    }

    #[test]
    fn header_only_for_zero_span_runtime_errors() {
        let source = "println(s: \"x\")".to_string();
        let renderer = Renderer::new(source);
        let err = ferric_vm::RuntimeError::DivisionByZero { span: Span::new(0, 0) };
        let out = renderer.render_runtime_error(&err);
        assert_eq!(out, "error: division by zero");
    }

    #[test]
    fn line_text_round_trip() {
        let renderer = Renderer::new("a\nbb\nccc".to_string());
        assert_eq!(renderer.line_text(1), "a");
        assert_eq!(renderer.line_text(2), "bb");
        assert_eq!(renderer.line_text(3), "ccc");
    }
}
