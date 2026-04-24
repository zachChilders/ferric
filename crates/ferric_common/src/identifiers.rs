//! Unique identifiers used throughout the compiler pipeline.

use serde::{Deserialize, Serialize};

/// Unique identifier for AST nodes.
///
/// Each AST node receives a unique NodeId during parsing, which is used
/// to associate type information, resolution data, and other metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeId(pub u32);

impl NodeId {
    /// Creates a new NodeId from a raw u32 value.
    pub fn new(id: u32) -> Self {
        Self(id)
    }
}

/// Interned string identifier.
///
/// Created by the Interner to avoid string duplication and enable
/// fast equality comparison of identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Symbol(pub u32);

impl Symbol {
    /// Creates a new Symbol from a raw u32 value.
    pub fn new(id: u32) -> Self {
        Self(id)
    }
}

/// Definition identifier.
///
/// Unique identifier for variable, function, and type definitions.
/// Used by the resolver to track which definition each name reference points to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DefId(pub u32);

impl DefId {
    /// Creates a new DefId from a raw u32 value.
    pub fn new(id: u32) -> Self {
        Self(id)
    }
}
