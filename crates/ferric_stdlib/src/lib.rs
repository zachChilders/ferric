//! # Ferric Standard Library
//!
//! Provides native functions and the NativeRegistry for runtime function lookup.

use std::collections::HashMap;
use ferric_common::Symbol;

// Re-export Value and RuntimeError from ferric_vm
// NOTE: This creates a circular dependency issue - we'll need to move Value here
// or define NativeFn with a generic Result for now. Let's use String for errors.

/// Type for native function implementations.
///
/// Native functions take a slice of values and return a value or an error message.
/// The VM will convert the error message into a proper RuntimeError with Span.
pub type NativeFn = fn(&[NativeValue]) -> Result<NativeValue, String>;

/// Simplified value type for native function interface.
///
/// This is a temporary type to avoid circular dependencies.
/// Native functions work with this type, and the VM converts between
/// this and the full Value type.
#[derive(Debug, Clone, PartialEq)]
pub enum NativeValue {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Unit,
}

/// Registry of native functions available to the VM.
///
/// The VM queries this registry when calling functions by name.
/// If a function is found here, it's executed as a native function.
/// Otherwise, the VM looks for a user-defined function in the AST.
pub struct NativeRegistry {
    functions: HashMap<Symbol, NativeFn>,
}

impl NativeRegistry {
    /// Creates a new empty native function registry.
    pub fn new() -> Self {
        Self {
            functions: HashMap::new(),
        }
    }

    /// Registers a native function with the given name.
    pub fn register(&mut self, name: Symbol, f: NativeFn) {
        self.functions.insert(name, f);
    }

    /// Looks up a native function by name.
    pub fn get(&self, name: Symbol) -> Option<&NativeFn> {
        self.functions.get(&name)
    }
}

impl Default for NativeRegistry {
    fn default() -> Self {
        Self::new()
    }
}
