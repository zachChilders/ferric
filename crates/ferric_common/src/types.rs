//! Type system types.

use serde::{Deserialize, Serialize};

/// Ferric type representation.
///
/// This is the M1 baseline type system with an Unknown escape hatch
/// to allow partial implementation. Unknown will be removed in M3.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Ty {
    /// Integer type
    Int,
    /// Floating-point type
    Float,
    /// Boolean type
    Bool,
    /// String type
    Str,
    /// Unit type (empty tuple, represents no value)
    Unit,
    /// Function type with parameter types and return type
    Fn {
        /// Parameter types
        params: Vec<Ty>,
        /// Return type
        ret: Box<Ty>,
    },
    /// Result of a `$` shell expression (struct with `stdout` and `exit_code`).
    ShellOutput,
    /// Unknown type - escape hatch for M1, will be removed in M3
    Unknown,
}

impl Ty {
    /// Returns a human-readable description of this type.
    pub fn description(&self) -> String {
        match self {
            Ty::Int => "int".to_string(),
            Ty::Float => "float".to_string(),
            Ty::Bool => "bool".to_string(),
            Ty::Str => "str".to_string(),
            Ty::Unit => "()".to_string(),
            Ty::Fn { params, ret } => {
                let params_str = params
                    .iter()
                    .map(|p| p.description())
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("fn({}) -> {}", params_str, ret.description())
            }
            Ty::ShellOutput => "ShellOutput".to_string(),
            Ty::Unknown => "?".to_string(),
        }
    }

    /// Checks if this type is the Unit type.
    pub fn is_unit(&self) -> bool {
        matches!(self, Ty::Unit)
    }

    /// Checks if this type is Unknown.
    pub fn is_unknown(&self) -> bool {
        matches!(self, Ty::Unknown)
    }
}
