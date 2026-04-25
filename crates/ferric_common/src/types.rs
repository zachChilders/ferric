//! Type system types.

use serde::{Deserialize, Serialize};

/// A type variable used during Hindley-Milner inference.
///
/// Each fresh variable allocated by the inferencer carries a unique numeric id.
/// After inference completes, every type variable in `node_types` should have
/// been resolved to a concrete type via the substitution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TyVar(pub u32);

impl TyVar {
    /// Creates a new type variable with the given id.
    pub fn new(id: u32) -> Self {
        Self(id)
    }
}

/// Ferric type representation.
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
    /// Type variable produced by the inferencer. Concrete types are obtained
    /// by applying the inferencer's substitution.
    Var(TyVar),
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
            Ty::Var(v) => format!("?T{}", v.0),
        }
    }

    /// Checks if this type is the Unit type.
    pub fn is_unit(&self) -> bool {
        matches!(self, Ty::Unit)
    }
}

/// A polymorphic type scheme: `∀a₁…aₙ. τ`.
///
/// `forall` lists the type variables that are universally quantified;
/// instantiation replaces each of them with a fresh variable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeScheme {
    /// Universally quantified variables.
    pub forall: Vec<TyVar>,
    /// The body of the scheme.
    pub ty: Ty,
}

impl TypeScheme {
    /// A monomorphic scheme — no quantified variables.
    pub fn monomorphic(ty: Ty) -> Self {
        Self { forall: Vec::new(), ty }
    }
}
