//! Output types for each compiler stage.

use crate::{
    AsyncLowerError, AsyncWarning, Chunk, DefId, EffectError, EffectWarning, ExhaustivenessError,
    Item, LexError, NamedArg, NodeId, ParseError, ParseWarning, ResolveError, Span, Symbol, Token,
    Ty, TypeError,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Result of the lexing stage.
///
/// Contains all tokens produced from the source code, along with any
/// lexical errors encountered.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LexResult {
    /// All tokens produced by the lexer
    pub tokens: Vec<Token>,
    /// Any errors encountered during lexing
    pub errors: Vec<LexError>,
}

impl LexResult {
    /// Creates a new LexResult.
    pub fn new(tokens: Vec<Token>, errors: Vec<LexError>) -> Self {
        Self { tokens, errors }
    }

    /// Returns true if there were any errors during lexing.
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }
}

/// Result of the parsing stage.
///
/// Contains the abstract syntax tree (as a collection of top-level items)
/// along with any parsing errors and warnings encountered.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParseResult {
    /// Top-level items (functions, variable declarations, etc.)
    pub items: Vec<Item>,
    /// Any errors encountered during parsing
    pub errors: Vec<ParseError>,
    /// Non-fatal diagnostics emitted during parsing. Defaults to empty for
    /// parsers/tests that don't emit warnings.
    #[serde(default)]
    pub warnings: Vec<ParseWarning>,
}

impl ParseResult {
    /// Creates a new ParseResult with no warnings.
    pub fn new(items: Vec<Item>, errors: Vec<ParseError>) -> Self {
        Self {
            items,
            errors,
            warnings: Vec::new(),
        }
    }

    /// Creates a new ParseResult including parse warnings.
    pub fn with_warnings(
        items: Vec<Item>,
        errors: Vec<ParseError>,
        warnings: Vec<ParseWarning>,
    ) -> Self {
        Self {
            items,
            errors,
            warnings,
        }
    }

    /// Returns true if there were any errors during parsing.
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }
}

/// Per-definition metadata: name and source location.
///
/// Built by the resolver and consumed by tooling (LSP hover, completion,
/// goto-def). `span` is `None` for native definitions registered by the
/// runtime — they have no source location.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DefInfo {
    pub name: Symbol,
    pub span: Option<Span>,
}

/// Result of the name resolution stage.
///
/// Maps each identifier reference to its definition and assigns stack slots
/// for variables and functions. Also carries canonicalized call argument lists
/// (in parameter-definition order) so downstream stages need no named-param logic.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResolveResult {
    /// Maps NodeId (identifier usage) to DefId (definition)
    pub resolutions: HashMap<NodeId, DefId>,
    /// Maps each definition to its stack slot for variables
    pub def_slots: HashMap<DefId, u32>,
    /// Maps each function definition to its function index
    pub fn_slots: HashMap<DefId, u32>,
    /// Maps each Call NodeId to its args in parameter-definition order (with defaults inserted).
    /// Downstream stages use this instead of CallExpr::args directly.
    pub canonical_call_args: HashMap<NodeId, Vec<NamedArg>>,
    /// Map from struct/enum name → DefId. Filled during a pre-pass over
    /// top-level items.
    pub type_defs: HashMap<Symbol, DefId>,
    /// Map from struct DefId → ordered list of (field_name, declared_type).
    /// Used by the inferencer to type-check field accesses and struct literals,
    /// and by the compiler to compute field indices.
    pub struct_fields: HashMap<DefId, Vec<(Symbol, crate::TypeAnnotation)>>,
    /// Map from enum DefId → ordered list of variants.
    pub enum_variants: HashMap<DefId, Vec<(Symbol, Vec<crate::TypeAnnotation>)>>,
    /// Map from each `type` alias name to its raw inner annotation. The
    /// inferencer uses this to construct `Ty::Alias(name, inner)` at every
    /// alias use site. M10 (Task 6 Part C). Aliases are opaque-only — the
    /// alias and inner type do not unify directly; `as` casts bridge them.
    #[serde(default)]
    pub type_aliases: HashMap<Symbol, crate::TypeAnnotation>,
    /// Map from each impl-method NodeId to the DefId allocated for it.
    pub method_def_ids: HashMap<NodeId, DefId>,
    /// Map from each impl-method NodeId to its declared parameter list.
    /// Stored separately from `fn_params` (which is keyed by Symbol) because
    /// methods share names across types.
    pub method_params: HashMap<NodeId, Vec<crate::Param>>,
    /// Map from each `Expr::Closure` NodeId to the ordered list of
    /// captured (DefId, Symbol) pairs. The Symbol is the captured variable's
    /// source name — the compiler binds it to a local slot inside the closure
    /// chunk so the body can reference it by name.
    pub captures: HashMap<NodeId, Vec<(DefId, Symbol)>>,
    /// Per-DefId metadata: source name + (optional) source span. Populated by
    /// the resolver at every DefId-allocation site; used by tooling for hover,
    /// completion, and goto-definition.
    pub defs: HashMap<DefId, DefInfo>,
    /// Any errors encountered during resolution
    pub errors: Vec<ResolveError>,
}

impl ResolveResult {
    /// Creates a new ResolveResult.
    pub fn new(
        resolutions: HashMap<NodeId, DefId>,
        def_slots: HashMap<DefId, u32>,
        fn_slots: HashMap<DefId, u32>,
        canonical_call_args: HashMap<NodeId, Vec<NamedArg>>,
        errors: Vec<ResolveError>,
    ) -> Self {
        Self {
            resolutions,
            def_slots,
            fn_slots,
            canonical_call_args,
            type_defs: HashMap::new(),
            struct_fields: HashMap::new(),
            enum_variants: HashMap::new(),
            type_aliases: HashMap::new(),
            method_def_ids: HashMap::new(),
            method_params: HashMap::new(),
            captures: HashMap::new(),
            defs: HashMap::new(),
            errors,
        }
    }

    /// Returns true if there were any errors during resolution.
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    /// Look up the metadata for a `DefId`. Returns `None` if the resolver
    /// did not record the definition (e.g. when reading an older
    /// `ResolveResult` round-tripped through JSON before this field existed).
    pub fn def(&self, id: DefId) -> Option<&DefInfo> {
        self.defs.get(&id)
    }
}

/// Result of the exhaustiveness checking stage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExhaustivenessResult {
    /// Errors and warnings discovered during exhaustiveness checking.
    pub errors: Vec<ExhaustivenessError>,
}

impl ExhaustivenessResult {
    pub fn new(errors: Vec<ExhaustivenessError>) -> Self {
        Self { errors }
    }

    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }
}

/// Result of the type checking stage.
///
/// Associates each AST node with its inferred or checked type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TypeResult {
    /// Maps each NodeId to its type
    pub node_types: HashMap<NodeId, Ty>,
    /// For each `MethodCall` NodeId, the `DefId` of the impl method to invoke.
    /// The compiler reads this to lower a method call to a regular function call.
    pub method_dispatch: HashMap<NodeId, DefId>,
    /// For each `MethodCall` NodeId resolved to a stdlib method, the `Symbol`
    /// of the corresponding stdlib function name (e.g. `str_len` for
    /// `s.len()`). The compiler reads this to lower the method call into a
    /// regular call against the named native. Distinct from `method_dispatch`,
    /// which carries user-defined impl-method dispatches. M10 (Task 6 Part B).
    #[serde(default)]
    pub stdlib_method_dispatch: HashMap<NodeId, Symbol>,
    /// Any errors encountered during type checking
    pub errors: Vec<TypeError>,
    /// Resolve-stage errors that the inferencer detected because they require
    /// type information to diagnose. M10 Task 4 uses this for
    /// `ResolveError::PropagateNonFallible` / `ResolveError::MustNonFallible`.
    /// Main wires these into the final error report alongside the resolver's
    /// own errors.
    #[serde(default)]
    pub resolve_errors: Vec<ResolveError>,
}

impl TypeResult {
    /// Creates a new TypeResult.
    pub fn new(node_types: HashMap<NodeId, Ty>, errors: Vec<TypeError>) -> Self {
        Self {
            node_types,
            method_dispatch: HashMap::new(),
            stdlib_method_dispatch: HashMap::new(),
            errors,
            resolve_errors: Vec::new(),
        }
    }

    /// Returns true if there were any errors during type checking, including
    /// `resolve_errors` discovered by the type-checker (M10 Task 4).
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty() || !self.resolve_errors.is_empty()
    }
}

/// Result of the `ferric_async` lowering stage.
///
/// Slots in between the type checker and the compiler. The `ast` field is a
/// rewritten `ParseResult` — same type as the parser's output — with
/// `async fn` items and `.await` / `async { ... }` expressions replaced by
/// their lowered (state machine + poll fn) equivalents. Downstream stages
/// (`ferric_compiler`, the VM) accept this `ParseResult` unchanged, so the
/// new stage is additive on the existing pipeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AsyncResult {
    /// The rewritten AST. For programs with no `async fn` / `.await` use, this
    /// is structurally equivalent to the input `ParseResult`.
    pub ast: ParseResult,
    /// Errors discovered during lowering.
    pub errors: Vec<AsyncLowerError>,
    /// Non-fatal diagnostics (e.g. blocking-shell warnings).
    pub warnings: Vec<AsyncWarning>,
}

impl AsyncResult {
    /// Creates a new AsyncResult.
    pub fn new(
        ast: ParseResult,
        errors: Vec<AsyncLowerError>,
        warnings: Vec<AsyncWarning>,
    ) -> Self {
        Self {
            ast,
            errors,
            warnings,
        }
    }

    /// Returns true if any lowering errors were recorded.
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }
}

/// Numeric tag tables assigned by the effects lowering pass.
///
/// The compiler emits `Op::Perform(effect_tag, op_tag, arg_count)` and the
/// VM's handler stack dispatches by these tags. The lowering pass assigns
/// stable tags up front (built-ins first, then user-declared effects) and
/// publishes them here so the compiler can do the lookup without re-walking
/// the AST.
///
/// Built-in tags: `Async = 0`, `Gen = 1`. User-declared effects get tags
/// starting at 2. Within an effect, ops are tagged in declaration order
/// (also: built-in ops `Async::Suspend = 0`, `Async::Spawn = 1`,
/// `Async::Join = 2`, `Gen::Yield = 0`).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct EffectTags {
    /// `effect_name -> u16 tag`.
    pub effect_tags: HashMap<Symbol, u16>,
    /// `(effect_name, op_name) -> u8 tag`.
    pub op_tags: HashMap<(Symbol, Symbol), u8>,
}

impl EffectTags {
    /// Looks up the tag pair for a `(effect, op)`. Returns `None` if the
    /// effect or op isn't registered.
    pub fn lookup(&self, effect: Symbol, op: Symbol) -> Option<(u16, u8)> {
        let e = *self.effect_tags.get(&effect)?;
        let o = *self.op_tags.get(&(effect, op))?;
        Some((e, o))
    }
}

/// Result of the `ferric_effects` lowering / checking stage.
///
/// Slots in between the type checker and the compiler — same pipeline position
/// as `AsyncResult`, which it replaces in M9. The `ast` field is a rewritten
/// `ParseResult` (same type as the parser's output) with `effect`/`perform`/
/// `handle`/`resume` desugared to the bytecode primitives the compiler emits.
/// Downstream stages accept the rewritten `ParseResult` unchanged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EffectResult {
    /// The rewritten AST. For programs with no effect machinery, this is
    /// structurally equivalent to the input `ParseResult`.
    pub ast: ParseResult,
    /// Effect / op tag table the compiler reads to lower
    /// `Expr::Perform`/`Handle`/`Resume` to opcodes.
    #[serde(default)]
    pub tags: EffectTags,
    /// Errors discovered during effect checking / lowering.
    pub errors: Vec<EffectError>,
    /// Non-fatal diagnostics.
    pub warnings: Vec<EffectWarning>,
}

impl EffectResult {
    /// Creates a new `EffectResult`.
    pub fn new(ast: ParseResult, errors: Vec<EffectError>, warnings: Vec<EffectWarning>) -> Self {
        Self {
            ast,
            tags: EffectTags::default(),
            errors,
            warnings,
        }
    }

    /// Creates a new `EffectResult` with an explicit tag table.
    pub fn with_tags(
        ast: ParseResult,
        tags: EffectTags,
        errors: Vec<EffectError>,
        warnings: Vec<EffectWarning>,
    ) -> Self {
        Self {
            ast,
            tags,
            errors,
            warnings,
        }
    }

    /// Returns true if any effect errors were recorded.
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }
}

/// A single entry in a [`HandlerTable`]. Maps a `(effect_tag, op_tag)` pair
/// to the chunk index that holds the compiled handler clause body. The VM
/// reads this when an `Op::Perform` walks the handler stack.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HandlerEntry {
    /// Effect tag from [`EffectTags::effect_tags`].
    pub effect_tag: u16,
    /// Op tag from [`EffectTags::op_tags`].
    pub op_tag: u8,
    /// Chunk index of the compiled handler clause body. The clause's
    /// leading slots are bound to the operation's argument values
    /// (declaration order), followed by the continuation as the last
    /// leading slot.
    pub clause_chunk_idx: u16,
}

/// A list of handler clauses associated with one `handle ... with { ... }`
/// expression. Indexed from `Op::PushHandler(handler_table_idx)`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct HandlerTable {
    /// Clauses in source order. Lookup is linear; tables are small (typically
    /// 1–3 clauses).
    pub entries: Vec<HandlerEntry>,
}

/// A compiled Ferric program ready for execution.
///
/// A `Program` is pure bytecode: a list of `Chunk`s (one per user function,
/// plus the entry chunk for top-level script code) and an `entry` index.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Program {
    /// Bytecode chunks (one per function, plus the entry chunk for top-level code).
    pub chunks: Vec<Chunk>,
    /// Index into `chunks` of the entry point.
    pub entry: u16,
    /// Handler-clause tables, one per `handle ... with { ... }` expression
    /// the compiler encountered. `Op::PushHandler(idx)` references an entry
    /// in this list. Empty for programs that have no handler blocks.
    #[serde(default)]
    pub handler_tables: Vec<HandlerTable>,
    /// Effect/op tag table assigned by the effects pass. Forwarded into the
    /// VM so it can recognise built-in effects (e.g. `Async::Suspend`) for
    /// special-case handling. Empty for programs with no effects.
    #[serde(default)]
    pub effect_tags: EffectTags,
}

impl Program {
    /// Creates a Program from the given chunks and entry index. No handler
    /// tables are registered; programs that compile `handle` expressions
    /// must build the program with [`Program::with_handler_tables`].
    pub fn new(chunks: Vec<Chunk>, entry: u16) -> Self {
        Self {
            chunks,
            entry,
            handler_tables: Vec::new(),
            effect_tags: EffectTags::default(),
        }
    }

    /// Creates a Program with explicit handler tables.
    pub fn with_handler_tables(
        chunks: Vec<Chunk>,
        entry: u16,
        handler_tables: Vec<HandlerTable>,
    ) -> Self {
        Self {
            chunks,
            entry,
            handler_tables,
            effect_tags: EffectTags::default(),
        }
    }

    /// Creates a Program with explicit handler tables and effect tag table.
    pub fn with_effects(
        chunks: Vec<Chunk>,
        entry: u16,
        handler_tables: Vec<HandlerTable>,
        effect_tags: EffectTags,
    ) -> Self {
        Self {
            chunks,
            entry,
            handler_tables,
            effect_tags,
        }
    }
}
