//! Output types for each compiler stage.

use std::collections::HashMap;
use crate::{Token, LexError, ParseError, ResolveError, TypeError, NodeId, DefId, Ty, Item};

/// Placeholder for bytecode Chunk type (defined in ferric_vm)
///
/// This will be defined by the VM crate. We use a unit struct here
/// to allow ferric_common to compile independently.
#[derive(Debug, Clone)]
pub struct Chunk;

/// Result of the lexing stage.
///
/// Contains all tokens produced from the source code, along with any
/// lexical errors encountered.
#[derive(Debug, Clone)]
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
#[derive(Debug, Clone)]
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

/// Result of the name resolution stage.
///
/// Maps each identifier reference to its definition and assigns stack slots
/// for variables and functions.
#[derive(Debug, Clone)]
pub struct ResolveResult {
    /// Maps NodeId (identifier usage) to DefId (definition)
    pub resolutions: HashMap<NodeId, DefId>,
    /// Maps each definition to its stack slot for variables
    pub def_slots: HashMap<DefId, u32>,
    /// Maps each function definition to its function index
    pub fn_slots: HashMap<DefId, u32>,
    /// Any errors encountered during resolution
    pub errors: Vec<ResolveError>,
}

impl ResolveResult {
    /// Creates a new ResolveResult.
    pub fn new(
        resolutions: HashMap<NodeId, DefId>,
        def_slots: HashMap<DefId, u32>,
        fn_slots: HashMap<DefId, u32>,
        errors: Vec<ResolveError>,
    ) -> Self {
        Self {
            resolutions,
            def_slots,
            fn_slots,
            errors,
        }
    }

    /// Returns true if there were any errors during resolution.
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }
}

/// Result of the type checking stage.
///
/// Associates each AST node with its inferred or checked type.
#[derive(Debug, Clone)]
pub struct TypeResult {
    /// Maps each NodeId to its type
    pub node_types: HashMap<NodeId, Ty>,
    /// Any errors encountered during type checking
    pub errors: Vec<TypeError>,
}

impl TypeResult {
    /// Creates a new TypeResult.
    pub fn new(node_types: HashMap<NodeId, Ty>, errors: Vec<TypeError>) -> Self {
        Self { node_types, errors }
    }

    /// Returns true if there were any errors during type checking.
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }
}

/// A compiled Ferric program ready for execution.
///
/// Contains all bytecode chunks and identifies the entry point function.
#[derive(Debug, Clone)]
pub struct Program {
    /// All compiled bytecode chunks (one per function)
    pub chunks: Vec<Chunk>,
    /// Index of the entry point function
    pub entry: u16,
}

impl Program {
    /// Creates a new Program.
    pub fn new(chunks: Vec<Chunk>, entry: u16) -> Self {
        Self { chunks, entry }
    }
}
