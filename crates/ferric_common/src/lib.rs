//! # Ferric Common
//!
//! Shared types and utilities used across all Ferric pipeline stages.
//!
//! This crate is the foundation of the Ferric architecture and is depended upon
//! by all other stages, but never depends on them. This enables surgical stage
//! replacement without cascading changes.

// Re-export all modules
pub use span::*;
pub use identifiers::*;
pub use interner::*;
pub use tokens::*;
pub use types::*;
pub use errors::*;
pub use results::*;

mod span;
mod identifiers;
mod interner;
mod tokens;
mod types;
mod errors;
mod results;
