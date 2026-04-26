//! Output types for each compiler stage.

use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use crate::{Token, LexError, ParseError, ResolveError, TypeError, ExhaustivenessError, NodeId, DefId, Ty, Item, NamedArg, Chunk, Symbol, Span};

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
/// along with any parsing errors encountered.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParseResult {
    /// Top-level items (functions, variable declarations, etc.)
    pub items: Vec<Item>,
    /// Any errors encountered during parsing
    pub errors: Vec<ParseError>,
}

impl ParseResult {
    /// Creates a new ParseResult.
    pub fn new(items: Vec<Item>, errors: Vec<ParseError>) -> Self {
        Self { items, errors }
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
    /// Any errors encountered during type checking
    pub errors: Vec<TypeError>,
}

impl TypeResult {
    /// Creates a new TypeResult.
    pub fn new(node_types: HashMap<NodeId, Ty>, errors: Vec<TypeError>) -> Self {
        Self {
            node_types,
            method_dispatch: HashMap::new(),
            errors,
        }
    }

    /// Returns true if there were any errors during type checking.
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }
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
}

impl Program {
    /// Creates a Program from the given chunks and entry index.
    pub fn new(chunks: Vec<Chunk>, entry: u16) -> Self {
        Self { chunks, entry }
    }
}

