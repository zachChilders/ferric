//! String interning for efficient identifier management.

use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use crate::Symbol;

/// String interner for managing identifiers efficiently.
///
/// The interner stores each unique string once and returns a Symbol
/// for fast comparison and lookup. This reduces memory usage and
/// speeds up identifier comparison throughout the compiler.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Interner {
    /// Maps strings to their Symbol identifiers
    map: HashMap<String, Symbol>,
    /// Stores the actual strings in insertion order
    strings: Vec<String>,
}

impl Interner {
    /// Creates a new empty interner.
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
            strings: Vec::new(),
        }
    }

    /// Interns a string and returns its Symbol.
    ///
    /// If the string has been interned before, returns the existing Symbol.
    /// Otherwise, creates a new Symbol and stores the string.
    pub fn intern(&mut self, s: &str) -> Symbol {
        if let Some(&symbol) = self.map.get(s) {
            return symbol;
        }

        let symbol = Symbol::new(self.strings.len() as u32);
        self.strings.push(s.to_string());
        self.map.insert(s.to_string(), symbol);
        symbol
    }

    /// Resolves a Symbol back to its string.
    ///
    /// # Panics
    ///
    /// Panics if the Symbol was not created by this interner.
    pub fn resolve(&self, sym: Symbol) -> &str {
        &self.strings[sym.0 as usize]
    }

    /// Looks up a string without mutating the interner. Returns `None` if the
    /// string has not been interned yet.
    ///
    /// Use this from stages that only have `&Interner` and need to discover
    /// whether a well-known name (e.g. a stdlib native) has already been
    /// registered.
    pub fn lookup(&self, s: &str) -> Option<Symbol> {
        self.map.get(s).copied()
    }
}

impl Default for Interner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_intern_same_string() {
        let mut interner = Interner::new();
        let s1 = interner.intern("hello");
        let s2 = interner.intern("hello");
        assert_eq!(s1, s2);
    }

    #[test]
    fn test_intern_different_strings() {
        let mut interner = Interner::new();
        let s1 = interner.intern("hello");
        let s2 = interner.intern("world");
        assert_ne!(s1, s2);
    }

    #[test]
    fn test_resolve() {
        let mut interner = Interner::new();
        let sym = interner.intern("test");
        assert_eq!(interner.resolve(sym), "test");
    }
}
