//! Type system types.

use serde::{Deserialize, Serialize};
use crate::{DefId, Symbol};

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
    /// Tuple type, e.g. `(Int, Float)`.
    Tuple(Vec<Ty>),
    /// User-defined struct type. Identified by `def_id` (assigned by the
    /// resolver) plus the named field types in declaration order.
    Struct {
        def_id: DefId,
        name: Symbol,
        fields: Vec<(Symbol, Ty)>,
    },
    /// User-defined enum type. Each variant has a name and a list of payload
    /// types (empty for variants with no payload).
    Enum {
        def_id: DefId,
        name: Symbol,
        variants: Vec<(Symbol, Vec<Ty>)>,
    },
    /// Type variable produced by the inferencer. Concrete types are obtained
    /// by applying the inferencer's substitution.
    Var(TyVar),
    /// Homogeneous array type, e.g. `[Int]`.
    Array(Box<Ty>),
    /// `Option<T>` — built-in nullable type.
    Option(Box<Ty>),
    /// `Result<T, E>` — built-in success/error type.
    Result(Box<Ty>, Box<Ty>),
    /// Opaque type alias produced by a `type` declaration. Two `Opaque` types
    /// with different `def_id`s are never equal even when their `inner` types
    /// match — that distinction is what makes `type Url = Str` safer than a
    /// transparent alias. At runtime, opaque types erase to `inner`.
    Opaque {
        def_id: DefId,
        inner: Box<Ty>,
    },
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
            Ty::Tuple(elems) => {
                let parts = elems
                    .iter()
                    .map(|t| t.description())
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("({})", parts)
            }
            Ty::Struct { fields, .. } => {
                let parts = fields
                    .iter()
                    .map(|(_, t)| t.description())
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("struct {{ {} }}", parts)
            }
            Ty::Enum { variants, .. } => {
                let parts = variants
                    .iter()
                    .map(|(_, ts)| {
                        let inner = ts
                            .iter()
                            .map(|t| t.description())
                            .collect::<Vec<_>>()
                            .join(", ");
                        format!("({})", inner)
                    })
                    .collect::<Vec<_>>()
                    .join(" | ");
                format!("enum [ {} ]", parts)
            }
            Ty::Var(v) => format!("?T{}", v.0),
            Ty::Array(elem) => format!("[{}]", elem.description()),
            Ty::Option(inner) => format!("Option<{}>", inner.description()),
            Ty::Result(ok, err) => {
                format!("Result<{}, {}>", ok.description(), err.description())
            }
            Ty::Opaque { def_id, inner } => {
                format!("Opaque#{}({})", def_id.0, inner.description())
            }
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
