//! Abstract Syntax Tree types for Ferric.
//!
//! These types represent the parsed structure of Ferric source code.
//! Every node carries a NodeId for later stages to attach metadata,
//! and a Span for error reporting.

use crate::{NodeId, Span, Symbol};
use serde::{Deserialize, Serialize};

/// A named argument at a call site.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NamedArg {
    pub span: Span,
    pub name: Symbol,
    pub value: Box<Expr>,
    /// `true` when the parser inferred the name from a bare identifier
    /// (M10 implicit named args). `false` for explicit `name: value` args.
    /// Informational only — downstream stages do not branch on it.
    #[serde(default)]
    pub implicit: bool,
}

/// A part of a shell command line in the AST.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ShellPart {
    /// Literal shell text passed through verbatim.
    Literal(String),
    /// The parsed Ferric expression inside `@{...}`.
    Interpolated(Box<Expr>),
}

/// Captured result of running a shell command.
///
/// This is a value type carrying a command's stdout and exit code. Callers
/// must access the fields explicitly — swallowing the exit code is intentionally
/// not the default.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShellOutput {
    pub stdout: String,
    pub exit_code: i32,
}

// Compile-time assertion: shell types are `Send + Sync` so the async milestone
// can lower `$` expressions to awaitable operations without re-shaping the AST.
const _: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ShellOutput>();
    assert_send_sync::<ShellPart>();
};

/// A function parameter definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Param {
    pub span: Span,
    pub name: Symbol,
    pub ty: TypeAnnotation,
    pub default: Option<Box<Expr>>,
}

/// A generic type parameter, e.g. `T` or `T: Describable`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeParam {
    pub name: Symbol,
    pub bounds: Vec<Symbol>,
    pub span: Span,
}

/// One method signature in a `trait` definition. Trait methods do not have
/// bodies — only a name, declared parameters (the first being `self`), and
/// a return type annotation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TraitMethod {
    pub id: NodeId,
    pub name: Symbol,
    pub params: Vec<Param>,
    pub ret_ty: TypeAnnotation,
    pub span: Span,
}

/// Mode for a require statement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RequireMode {
    /// Default — halts on failure
    Error,
    /// Emits a diagnostic warning and continues
    Warn,
}

/// A require statement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RequireStmt {
    pub span: Span,
    pub mode: RequireMode,
    pub expr: Box<Expr>,
    pub message: Option<Box<Expr>>,
    pub set_fn: Option<Box<Expr>>,
}

/// An `import` declaration at the top of a Ferric file.
///
/// The parser classifies the path string into one of three shapes; the module
/// resolver later turns the path into an actual filesystem location.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportDecl {
    pub id: NodeId,
    pub span: Span,
    pub path: ImportPath,
    pub items: ImportItems,
}

/// The shape of an import path string.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ImportPath {
    /// File-relative path: `"./db"` or `"../util"`.
    Relative(String),
    /// Workspace-root-relative path: `"@/config"`. Requires a manifest.
    Workspace(String),
    /// Cache dependency name: `"ferric-http"`. Requires manifest + dependency.
    Cache(String),
}

/// The items being imported.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ImportItems {
    /// Named imports: `{ connect, disconnect as d }`.
    Named(Vec<ImportItem>),
    /// Namespace import: `* as db` — binds the symbol to a module value.
    Namespace(Symbol),
}

/// One entry in a named import list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportItem {
    pub span: Span,
    pub name: Symbol,
    pub alias: Option<Symbol>,
}

/// An `export` modifier wrapping a top-level item.
///
/// `export` is a parser-level decorator; the inner item is parsed exactly as
/// it would be without `export`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExportDecl {
    pub id: NodeId,
    pub span: Span,
    pub item: Box<Item>,
}

/// A `type` alias item: `type Url = Str` or `type Result<T> = StdResult<T, AppError>`.
///
/// In M7, all aliases are opaque — `opaque` is reserved for a future
/// transparent-alias mode and is always `true`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TypeAliasItem {
    pub id: NodeId,
    pub span: Span,
    pub name: Symbol,
    pub params: Vec<Symbol>,
    pub ty: TypeAnnotation,
    pub opaque: bool,
}

/// A `expr as TypeExpr` cast expression.
///
/// At type-check time, casts wrap or unwrap an opaque type. At runtime, they
/// erase to a no-op — there is no `Value` variant for opaque types.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CastExpr {
    pub id: NodeId,
    pub span: Span,
    pub expr: Box<Expr>,
    pub target: TypeAnnotation,
}

/// A plain (synchronous) function definition.
///
/// `async fn` once had its own `Item::AsyncFn(AsyncFnItem)` variant, but as of
/// M9 the parser desugars `async fn foo() -> T` into an ordinary `Item::Fn`
/// whose return type is `Eff<[Async], T>`. Both `AsyncFnItem` and the
/// corresponding `Item::AsyncFn` variant were removed in M9 Task 7.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FnItem {
    /// Unique identifier for this node
    pub id: NodeId,
    /// Function name
    pub name: Symbol,
    /// Generic type parameters (`<T, U: Bound>`). Empty for non-generic fns.
    pub type_params: Vec<TypeParam>,
    /// Parameters
    pub params: Vec<Param>,
    /// Return type
    pub ret_ty: TypeAnnotation,
    /// Function body
    pub body: Expr,
    /// Source location
    pub span: Span,
}

/// Top-level item in a Ferric program.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Item {
    /// Function definition. As of M9, `async fn` is desugared by the parser
    /// to `Item::Fn` with an `Eff<[Async], T>` return type — there is no
    /// dedicated `AsyncFn` variant.
    Fn(FnItem),
    /// Struct definition
    StructDef {
        id: NodeId,
        name: Symbol,
        fields: Vec<(Symbol, TypeAnnotation)>,
        span: Span,
    },
    /// Enum definition
    EnumDef {
        id: NodeId,
        name: Symbol,
        variants: Vec<(Symbol, Vec<TypeAnnotation>)>,
        span: Span,
    },
    /// Trait definition: a collection of method signatures.
    TraitDef {
        id: NodeId,
        name: Symbol,
        /// Generic type parameters declared on the trait itself (e.g. `T`
        /// in `trait To<T>`). Empty for non-generic traits like `Describable`.
        type_params: Vec<TypeParam>,
        methods: Vec<TraitMethod>,
        span: Span,
    },
    /// Impl block: a trait implementation for a specific type.
    /// Each impl method is a fully-defined function with a body; downstream
    /// stages treat each as if it were a top-level `FnDef` for naming and
    /// slot-allocation purposes.
    ImplBlock {
        id: NodeId,
        trait_name: Symbol,
        /// Concrete type arguments supplied to the trait at the impl site
        /// (e.g. `[Str]` in `impl To<Str> for ShellOutput`). Empty for
        /// non-generic traits.
        trait_args: Vec<TypeAnnotation>,
        type_name: Symbol,
        methods: Vec<ImplMethod>,
        span: Span,
    },
    /// Top-level script statement (let binding or expression)
    /// These are executed in order when the program runs
    Script {
        /// The statement
        stmt: Stmt,
        /// Unique identifier for this node
        id: NodeId,
        /// Source location
        span: Span,
    },
    /// An `import` declaration. Must appear at the top of a file before any
    /// non-import items.
    Import(ImportDecl),
    /// An `export` modifier wrapping another top-level item.
    Export(ExportDecl),
    /// A `type` alias definition.
    TypeAlias(TypeAliasItem),
    /// An `effect` declaration. The effect name and its operations live at
    /// module scope; downstream stages turn `perform` calls into handler
    /// dispatches against this declaration.
    EffectDecl(EffectDecl),
}

/// A function inside an `impl` block. Same shape as a top-level FnDef, but
/// scoped to the impl. The `id` uniquely names this method across the AST.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImplMethod {
    pub id: NodeId,
    pub name: Symbol,
    pub params: Vec<Param>,
    pub ret_ty: TypeAnnotation,
    pub body: Expr,
    pub span: Span,
}

impl Item {
    /// Returns the NodeId of this item.
    pub fn id(&self) -> NodeId {
        match self {
            Item::Fn(item) => item.id,
            Item::StructDef { id, .. } => *id,
            Item::EnumDef { id, .. } => *id,
            Item::TraitDef { id, .. } => *id,
            Item::ImplBlock { id, .. } => *id,
            Item::Script { id, .. } => *id,
            Item::Import(decl) => decl.id,
            Item::Export(decl) => decl.id,
            Item::TypeAlias(decl) => decl.id,
            // Effect declarations have no `NodeId` of their own — the parser
            // doesn't attach metadata at this granularity yet. Use the
            // span-derived sentinel `NodeId::new(0)` so callers that only need
            // an Item-keyed identity still work; resolver/typecheck never read
            // this id for effect decls.
            Item::EffectDecl(_) => NodeId::new(0),
        }
    }

    /// Returns the Span of this item.
    pub fn span(&self) -> Span {
        match self {
            Item::Fn(item) => item.span,
            Item::StructDef { span, .. } => *span,
            Item::EnumDef { span, .. } => *span,
            Item::TraitDef { span, .. } => *span,
            Item::ImplBlock { span, .. } => *span,
            Item::Script { span, .. } => *span,
            Item::Import(decl) => decl.span,
            Item::Export(decl) => decl.span,
            Item::TypeAlias(decl) => decl.span,
            Item::EffectDecl(decl) => decl.span,
        }
    }
}

/// An `async { ... }` block expression. Lifts a synchronous block into an
/// `Async<T>` value where `T` is the type of the block's tail expression.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AsyncBlockExpr {
    pub id: NodeId,
    pub span: Span,
    /// The block being lifted. The parser guarantees this is `Expr::Block`.
    pub block: Box<Expr>,
}

/// One operation in an `effect` declaration.
///
/// `Op(name: ParamTy, ...) -> ResumeTy` — the parameters describe what the
/// `perform` site supplies, and `resume_type` is what `perform Op(...)`
/// evaluates to (i.e. the type the handler `resume k with v` passes back).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EffectOp {
    pub span: Span,
    pub name: Symbol,
    pub params: Vec<(Symbol, crate::Ty)>,
    /// What `perform Op(...)` evaluates to.
    pub resume_type: crate::Ty,
}

/// An `effect` declaration at module scope.
///
/// ```text
/// effect Async {
///     Suspend(future: Async<Str>) -> Str,
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EffectDecl {
    pub span: Span,
    pub name: Symbol,
    /// Generic type parameters, e.g. `T` in `effect Gen<T>`.
    pub type_params: Vec<Symbol>,
    pub ops: Vec<EffectOp>,
}

/// A `perform Effect::Op(name: value, ...)` expression.
///
/// Evaluates by suspending the current computation and invoking the nearest
/// enclosing handler clause for `effect::op`. The result of the expression is
/// whatever the handler resumes with.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PerformExpr {
    pub id: NodeId,
    pub span: Span,
    pub effect: Symbol,
    pub op: Symbol,
    /// Named arguments, matching the corresponding `EffectOp::params`.
    pub args: Vec<(Symbol, Expr)>,
}

/// One clause inside a `handle ... with { ... }` block, matching a single
/// `Effect::Op` and binding a continuation variable that can be resumed
/// exactly once.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HandlerClause {
    pub span: Span,
    pub effect: Symbol,
    pub op: Symbol,
    /// Names bound to the operation's argument values (one per `EffectOp::params`).
    pub params: Vec<Symbol>,
    /// The continuation variable name (defaults to `k`).
    pub cont: Symbol,
    pub handler: Box<Expr>,
}

/// A `handle { body } with { Op(...) => clause, ... }` expression.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HandleExpr {
    pub id: NodeId,
    pub span: Span,
    pub body: Box<Expr>,
    pub clauses: Vec<HandlerClause>,
}

/// A `resume k with value` expression. Legal only inside a `HandlerClause`'s
/// handler body. Resumes the captured continuation `k` with `value` exactly
/// once; resuming twice is an error caught by the effects pass.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResumeExpr {
    pub id: NodeId,
    pub span: Span,
    /// Must refer to the enclosing `HandlerClause::cont`.
    pub cont: Symbol,
    pub value: Box<Expr>,
}

/// A loop label, e.g. `'outer` in `'outer: while cond { ... }`.
///
/// M10 (Task 5). Carries the parsed identifier (without the leading tick) so
/// the resolver can match `break 'outer` / `continue 'outer` against the label
/// stack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Label {
    pub span: Span,
    /// The identifier after the `'` sigil.
    pub name: Symbol,
}

/// One segment of a parsed f-string literal. M10 (Task 2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FStringSegment {
    pub span: Span,
    pub kind: FStringSegmentKind,
}

/// What an `FStringSegment` carries: a literal piece of text, or an embedded
/// expression to be interpolated.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FStringSegmentKind {
    /// Literal text between `{...}` interpolations. The lexer handles `{{`/`}}`
    /// escapes during emission so this is the post-escape string.
    Lit(String),
    /// `{expr}` — any Ferric expression. `to_str` coercion is applied later by
    /// the type checker for non-`Str` operands.
    Expr(Box<Expr>),
}

/// `f"..."` expression — a sequence of literal and interpolated segments
/// concatenated into a single `Str` value. M10 (Task 2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FStringExpr {
    pub id: NodeId,
    pub span: Span,
    pub segments: Vec<FStringSegment>,
}

/// `lhs |> rhs` pipeline expression. The resolver desugars this to a
/// `CallExpr` where `lhs` is prepended as an implicit first named argument
/// (M10 Task 3); after the resolver, this variant no longer exists.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PipelineExpr {
    pub span: Span,
    pub lhs: Box<Expr>,
    /// Must resolve to a `CallExpr` after desugaring.
    pub rhs: Box<Expr>,
}

/// `expr?` propagation expression. The operand must be `Option<T>` or
/// `Result<T, E>`; on `None` / `Err(_)` the enclosing function returns early.
/// M10 (Task 4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PropagateExpr {
    pub span: Span,
    pub operand: Box<Expr>,
}

/// `expr must` and `expr must <message>` unwrap expression. The operand must
/// be `Option<T>` or `Result<T, E>`; on absence the VM panics with the given
/// message (or a generic message when none is provided). M10 (Task 4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MustExpr {
    pub span: Span,
    pub operand: Box<Expr>,
    /// Optional `Str` panic message. `None` produces a generic "unwrap failed".
    pub message: Option<Box<Expr>>,
}

/// The left-hand-side pattern of a `let` binding. M10 (Task 5).
///
/// Existing `let x = ...` becomes `LetPattern::Ident(x)`. Tuple and struct
/// destructuring are added in Task 5; only irrefutable patterns are allowed
/// in `let` (no enum patterns — those still require `match`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LetPattern {
    /// `let x = ...` — single-name binding (the existing form).
    Ident(Symbol),
    /// `let (a, b, c) = ...` — tuple destructuring.
    Tuple(Vec<Symbol>),
    /// `let Point { x, y } = ...` — struct destructuring with bare-name
    /// shorthand fields only (no `field: alias` renaming in this milestone).
    Struct { name: Symbol, fields: Vec<Symbol> },
}

/// `receiver.method(args...)` — a method call. M10 retroactive (Task 6 Part B).
///
/// Currently the AST encodes method calls inline as `Expr::MethodCall { ... }`
/// using a struct-style enum variant. This standalone struct is provided for
/// spec parity and may be used by Task 6 implementations and external tooling
/// that prefer a single value type. The two forms are interchangeable: the
/// fields match exactly except that this struct omits the `id`, which is
/// produced fresh at construction time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MethodCallExpr {
    pub span: Span,
    pub receiver: Box<Expr>,
    pub method: Symbol,
    pub args: Vec<NamedArg>,
}

/// Expression in Ferric.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Expr {
    /// Literal value (integer, string, boolean, unit)
    Literal {
        value: Literal,
        id: NodeId,
        span: Span,
    },
    /// Variable reference
    Variable {
        name: Symbol,
        id: NodeId,
        span: Span,
    },
    /// Binary operation (e.g., `a + b`)
    Binary {
        op: BinOp,
        left: Box<Expr>,
        right: Box<Expr>,
        id: NodeId,
        span: Span,
    },
    /// Unary operation (e.g., `-x`, `!b`)
    Unary {
        op: UnOp,
        expr: Box<Expr>,
        id: NodeId,
        span: Span,
    },
    /// Function call — args are named and in any order at parse time;
    /// resolver canonicalises to definition order before typecheck/VM see it.
    Call {
        callee: Box<Expr>,
        args: Vec<NamedArg>,
        id: NodeId,
        span: Span,
    },
    /// If expression with optional else branch
    If {
        cond: Box<Expr>,
        then_branch: Box<Expr>,
        else_branch: Option<Box<Expr>>,
        id: NodeId,
        span: Span,
    },
    /// Block expression: `{ stmt*; expr? }`
    Block {
        stmts: Vec<Stmt>,
        expr: Option<Box<Expr>>,
        id: NodeId,
        span: Span,
    },
    /// Return expression
    Return {
        expr: Option<Box<Expr>>,
        id: NodeId,
        span: Span,
    },
    /// While loop: `while cond { body }`. `label` is `Some` for labeled loops
    /// like `'outer: while ...` (M10 Task 5).
    While {
        cond: Box<Expr>,
        body: Box<Expr>,
        /// M10 Task 5 — `Some` for labeled loops, `None` otherwise.
        #[serde(default)]
        label: Option<Label>,
        id: NodeId,
        span: Span,
    },
    /// Infinite loop: `loop { body }`. `label` is `Some` for labeled loops
    /// (M10 Task 5).
    Loop {
        body: Box<Expr>,
        /// M10 Task 5 — `Some` for labeled loops, `None` otherwise.
        #[serde(default)]
        label: Option<Label>,
        id: NodeId,
        span: Span,
    },
    /// Break expression. `label` targets a specific labeled enclosing loop
    /// when `Some`; `None` means break the nearest enclosing loop (M10 Task 5).
    Break {
        #[serde(default)]
        label: Option<Label>,
        id: NodeId,
        span: Span,
    },
    /// Continue expression. `label` targets a specific labeled enclosing loop
    /// when `Some`; `None` means continue the nearest enclosing loop (M10 Task 5).
    Continue {
        #[serde(default)]
        label: Option<Label>,
        id: NodeId,
        span: Span,
    },
    /// Closure expression: `|| { body }`
    Closure {
        params: Vec<Param>,
        body: Box<Expr>,
        id: NodeId,
        span: Span,
    },
    /// Shell command expression: `$ cmd @{var} ...`
    Shell {
        parts: Vec<ShellPart>,
        id: NodeId,
        span: Span,
    },
    /// Struct literal: `Point { x: 1.0, y: 2.0 }`
    StructLit {
        name: Symbol,
        fields: Vec<(Symbol, Expr)>,
        id: NodeId,
        span: Span,
    },
    /// Field access: `p.x`
    FieldAccess {
        expr: Box<Expr>,
        field: Symbol,
        id: NodeId,
        span: Span,
    },
    /// Match expression: `match x { ... }`
    Match {
        scrutinee: Box<Expr>,
        arms: Vec<MatchArm>,
        id: NodeId,
        span: Span,
    },
    /// Tuple expression: `(1, 2, 3)`
    Tuple {
        elements: Vec<Expr>,
        id: NodeId,
        span: Span,
    },
    /// Enum variant constructor: `Shape::Circle(r)` or `Color::Red`.
    VariantCtor {
        enum_name: Symbol,
        variant: Symbol,
        args: Vec<Expr>,
        id: NodeId,
        span: Span,
    },
    /// Array literal: `[1, 2, 3]`. All elements share an inferred element type.
    ArrayLit {
        elements: Vec<Expr>,
        id: NodeId,
        span: Span,
    },
    /// Indexing: `arr[i]`. The receiver must be an array; the index must be
    /// an `Int`.
    Index {
        array: Box<Expr>,
        index: Box<Expr>,
        id: NodeId,
        span: Span,
    },
    /// Method call: `receiver.method(args)`. Lowered by the type checker /
    /// compiler to a regular function call where the receiver is the first
    /// argument.
    MethodCall {
        receiver: Box<Expr>,
        method: Symbol,
        args: Vec<NamedArg>,
        id: NodeId,
        span: Span,
    },
    /// Cast expression: `expr as TypeExpr`. At runtime, this is a no-op;
    /// at type-check time, it wraps or unwraps an opaque type alias.
    Cast(CastExpr),
    /// `async { ... }` — lifts a block into an `Async<T>` value.
    ///
    /// The user-facing `await expr` syntax is desugared at parse time to
    /// `perform Async::Suspend(future: expr)`, so there is no longer an
    /// `Expr::Await` variant. `Expr::AsyncBlock` is retained because the
    /// type checker uses it as a marker to bump `async_depth`.
    AsyncBlock(AsyncBlockExpr),
    /// `perform Effect::Op(name: value, ...)` — invokes the nearest enclosing
    /// handler clause for the named effect operation.
    Perform(PerformExpr),
    /// `handle { body } with { Op(...) => ... }` — installs handler clauses
    /// for the dynamic extent of `body`.
    Handle(HandleExpr),
    /// `resume k with value` — resumes the continuation captured by an
    /// enclosing `HandlerClause`. Legal only inside a handler clause body.
    Resume(ResumeExpr),
    /// `f"..."` interpolated string. M10 (Task 2). The parser produces this
    /// directly; the type checker assigns `Ty::Str`.
    FString(FStringExpr),
    /// `lhs |> rhs` pipeline. M10 (Task 3). The resolver desugars this to a
    /// `CallExpr` before downstream stages see it.
    Pipeline(PipelineExpr),
    /// `expr?` propagation. M10 (Task 4). Operand must be `Option<T>` or
    /// `Result<T, E>`.
    Propagate(PropagateExpr),
    /// `expr must <msg>?` unwrap. M10 (Task 4). Operand must be `Option<T>` or
    /// `Result<T, E>`; the VM panics with `message` on absence.
    Must(MustExpr),
}

/// A single arm of a `match` expression.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub body: Expr,
    pub span: Span,
}

/// Pattern in a `match` arm or destructuring let (M4: only match arms).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Pattern {
    /// `_` — matches any value, no binding.
    Wildcard { span: Span },
    /// Binding pattern: a fresh name that captures the matched value.
    Variable {
        name: Symbol,
        id: NodeId,
        span: Span,
    },
    /// Literal pattern: matches if the scrutinee is structurally equal.
    Literal { value: Literal, span: Span },
    /// Tuple destructuring: `(a, b, c)`.
    Tuple { patterns: Vec<Pattern>, span: Span },
    /// Struct destructuring: `Point { x, y }`. Each field pattern may be a
    /// shorthand variable binding (when the parser fills in `Variable { name }`)
    /// or an explicit nested pattern.
    Struct {
        name: Symbol,
        fields: Vec<(Symbol, Pattern)>,
        span: Span,
    },
    /// Enum-variant destructuring: `Shape::Circle(r)`.
    Variant {
        enum_name: Symbol,
        variant: Symbol,
        patterns: Vec<Pattern>,
        span: Span,
    },
}

impl Pattern {
    pub fn span(&self) -> Span {
        match self {
            Pattern::Wildcard { span }
            | Pattern::Variable { span, .. }
            | Pattern::Literal { span, .. }
            | Pattern::Tuple { span, .. }
            | Pattern::Struct { span, .. }
            | Pattern::Variant { span, .. } => *span,
        }
    }
}

impl Expr {
    /// Returns the NodeId of this expression.
    pub fn id(&self) -> NodeId {
        match self {
            Expr::Literal { id, .. } => *id,
            Expr::Variable { id, .. } => *id,
            Expr::Binary { id, .. } => *id,
            Expr::Unary { id, .. } => *id,
            Expr::Call { id, .. } => *id,
            Expr::If { id, .. } => *id,
            Expr::Block { id, .. } => *id,
            Expr::Return { id, .. } => *id,
            Expr::While { id, .. } => *id,
            Expr::Loop { id, .. } => *id,
            Expr::Break { id, .. } => *id,
            Expr::Continue { id, .. } => *id,
            Expr::Closure { id, .. } => *id,
            Expr::Shell { id, .. } => *id,
            Expr::StructLit { id, .. } => *id,
            Expr::FieldAccess { id, .. } => *id,
            Expr::Match { id, .. } => *id,
            Expr::Tuple { id, .. } => *id,
            Expr::VariantCtor { id, .. } => *id,
            Expr::MethodCall { id, .. } => *id,
            Expr::ArrayLit { id, .. } => *id,
            Expr::Index { id, .. } => *id,
            Expr::Cast(c) => c.id,
            Expr::AsyncBlock(b) => b.id,
            Expr::Perform(p) => p.id,
            Expr::Handle(h) => h.id,
            Expr::Resume(r) => r.id,
            Expr::FString(e) => e.id,
            // M10 stubs: these variants aren't emitted by the parser yet
            // (Tasks 3-4 wire them up). They carry no `id` field of their
            // own yet — Task 1 was data-structure scaffolding only — so
            // callers that hit them before downstream tasks land would
            // have a bug; surface that with a sentinel `NodeId::new(0)`.
            Expr::Pipeline(_) | Expr::Propagate(_) | Expr::Must(_) => NodeId::new(0),
        }
    }

    /// Returns the Span of this expression.
    pub fn span(&self) -> Span {
        match self {
            Expr::Literal { span, .. } => *span,
            Expr::Variable { span, .. } => *span,
            Expr::Binary { span, .. } => *span,
            Expr::Unary { span, .. } => *span,
            Expr::Call { span, .. } => *span,
            Expr::If { span, .. } => *span,
            Expr::Block { span, .. } => *span,
            Expr::Return { span, .. } => *span,
            Expr::While { span, .. } => *span,
            Expr::Loop { span, .. } => *span,
            Expr::Break { span, .. } => *span,
            Expr::Continue { span, .. } => *span,
            Expr::Closure { span, .. } => *span,
            Expr::Shell { span, .. } => *span,
            Expr::StructLit { span, .. } => *span,
            Expr::FieldAccess { span, .. } => *span,
            Expr::Match { span, .. } => *span,
            Expr::Tuple { span, .. } => *span,
            Expr::VariantCtor { span, .. } => *span,
            Expr::MethodCall { span, .. } => *span,
            Expr::ArrayLit { span, .. } => *span,
            Expr::Index { span, .. } => *span,
            Expr::Cast(c) => c.span,
            Expr::AsyncBlock(b) => b.span,
            Expr::Perform(p) => p.span,
            Expr::Handle(h) => h.span,
            Expr::Resume(r) => r.span,
            Expr::FString(e) => e.span,
            Expr::Pipeline(e) => e.span,
            Expr::Propagate(e) => e.span,
            Expr::Must(e) => e.span,
        }
    }
}

/// Statement in Ferric.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Stmt {
    /// Let binding: `let x: Type = expr` or `let mut x: Type = expr`.
    ///
    /// As of M10 (Task 5) the LHS is a `LetPattern` rather than a single name
    /// — existing single-name bindings carry `LetPattern::Ident(name)`, and
    /// tuple/struct destructuring is supported via the other variants.
    Let {
        /// LHS pattern. Single-name bindings use `LetPattern::Ident`.
        pattern: LetPattern,
        mutable: bool,
        ty: Option<TypeAnnotation>,
        init: Expr,
        id: NodeId,
        span: Span,
    },
    /// Assignment statement: `x = expr`
    Assign {
        target: Expr,
        value: Expr,
        id: NodeId,
        span: Span,
    },
    /// Expression statement
    Expr { expr: Expr },
    /// Require statement
    Require(RequireStmt),
    /// `for x in expr { body }` loop. `label` is `Some` for labeled `for`
    /// loops like `'outer: for ...` (M10 Task 5).
    For {
        var: Symbol,
        var_id: NodeId,
        iter: Expr,
        body: Expr,
        /// M10 Task 5 — `Some` for labeled loops, `None` otherwise.
        #[serde(default)]
        label: Option<Label>,
        id: NodeId,
        span: Span,
    },
}

impl Stmt {
    /// Returns the NodeId of this statement.
    pub fn id(&self) -> NodeId {
        match self {
            Stmt::Let { id, .. } => *id,
            Stmt::Assign { id, .. } => *id,
            Stmt::Expr { expr } => expr.id(),
            Stmt::Require(req) => req.expr.id(),
            Stmt::For { id, .. } => *id,
        }
    }

    /// Returns the Span of this statement.
    pub fn span(&self) -> Span {
        match self {
            Stmt::Let { span, .. } => *span,
            Stmt::Assign { span, .. } => *span,
            Stmt::Expr { expr } => expr.span(),
            Stmt::Require(req) => req.span,
            Stmt::For { span, .. } => *span,
        }
    }
}

/// Binary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BinOp {
    // Arithmetic
    Add,
    Sub,
    Mul,
    Div,
    Rem,

    // Comparison
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,

    // Logical
    And,
    Or,
}

impl BinOp {
    /// Returns the precedence level of this operator (higher = tighter binding).
    pub fn precedence(&self) -> u8 {
        match self {
            BinOp::Or => 1,
            BinOp::And => 2,
            BinOp::Eq | BinOp::Ne => 3,
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => 4,
            BinOp::Add | BinOp::Sub => 5,
            BinOp::Mul | BinOp::Div | BinOp::Rem => 6,
        }
    }
}

/// Unary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnOp {
    /// Numeric negation (`-`)
    Neg,
    /// Logical negation (`!`)
    Not,
}

/// Literal values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Literal {
    /// Integer literal
    Int(i64),
    /// Floating-point literal
    Float(f64),
    /// String literal (interned)
    Str(Symbol),
    /// Boolean literal
    Bool(bool),
    /// Unit literal `()`
    Unit,
}

/// One reference to an effect in an effect-row type annotation. Carries the
/// effect's name and any type arguments (e.g. `Gen<Int>`). Mirrors the
/// runtime `EffectRef` but in TypeAnnotation form so it survives parsing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectAnnotation {
    pub name: Symbol,
    pub args: Vec<TypeAnnotation>,
}

/// Type annotation in source code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TypeAnnotation {
    /// Named type (e.g., `Int`, `Bool`, `Str`, `Unit`)
    Named(Symbol),
    /// Array type, e.g. `[Int]`.
    Array(Box<TypeAnnotation>),
    /// Generic type application, e.g. `Option<Int>` or `Result<Int, Str>`.
    /// The parser produces this without knowing what `head` resolves to —
    /// later stages dispatch on the head's name.
    Generic {
        head: Symbol,
        args: Vec<TypeAnnotation>,
    },
    /// `Eff<[E1, E2, ...], T>` — an effect-row return type. The parser
    /// produces this as the desugared return-type of `async fn`/`gen fn`
    /// and as the explicit user-written form `Eff<[..], T>`.
    Eff {
        row: Vec<EffectAnnotation>,
        result: Box<TypeAnnotation>,
    },
    /// Placeholder for an annotation that should be inferred (used by closure
    /// params that omit an explicit type).
    Infer,
}
