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

/// User-facing rendering. Matches the language's surface syntax (Title Case
/// primitives, `fn(P) -> R`, `[T]`, `Option<T>`, `Result<T, E>`, …).
///
/// Named types (`Struct`, `Enum`, `Opaque`) carry a `Symbol` for their name,
/// but `Display` has no interner. They render with a `def_id` placeholder so
/// the output is unique but not pretty. Callers that need the user-facing
/// name (e.g. the LSP hover handler) must format with interner access
/// instead — `Display` is the fallback for non-named contexts.
///
/// **Exhaustiveness:** the match has no wildcard arm. Adding a new `Ty`
/// variant must add a `Display` arm or it will not compile.
impl std::fmt::Display for Ty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Ty::Int   => write!(f, "Int"),
            Ty::Float => write!(f, "Float"),
            Ty::Bool  => write!(f, "Bool"),
            Ty::Str   => write!(f, "Str"),
            Ty::Unit  => write!(f, "Unit"),
            Ty::Fn { params, ret } => {
                write!(f, "fn(")?;
                for (i, p) in params.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{p}")?;
                }
                write!(f, ") -> {ret}")
            }
            Ty::ShellOutput => write!(f, "ShellOutput"),
            Ty::Tuple(elems) => {
                write!(f, "(")?;
                for (i, e) in elems.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{e}")?;
                }
                if elems.len() == 1 { write!(f, ",")?; }
                write!(f, ")")
            }
            Ty::Struct { def_id, .. } => write!(f, "<struct#{}>", def_id.0),
            Ty::Enum   { def_id, .. } => write!(f, "<enum#{}>",   def_id.0),
            Ty::Var(v)            => write!(f, "?T{}", v.0),
            Ty::Array(elem)       => write!(f, "[{elem}]"),
            Ty::Option(inner)     => write!(f, "Option<{inner}>"),
            Ty::Result(ok, err)   => write!(f, "Result<{ok}, {err}>"),
            Ty::Opaque { inner, .. } => write!(f, "{inner}"),
        }
    }
}

#[cfg(test)]
mod display_tests {
    use super::*;

    #[test]
    fn primitives_use_title_case() {
        assert_eq!(format!("{}", Ty::Int),   "Int");
        assert_eq!(format!("{}", Ty::Float), "Float");
        assert_eq!(format!("{}", Ty::Bool),  "Bool");
        assert_eq!(format!("{}", Ty::Str),   "Str");
        assert_eq!(format!("{}", Ty::Unit),  "Unit");
    }

    #[test]
    fn function_type_round_trips() {
        let ty = Ty::Fn {
            params: vec![Ty::Int, Ty::Bool],
            ret:    Box::new(Ty::Str),
        };
        assert_eq!(format!("{ty}"), "fn(Int, Bool) -> Str");
    }

    #[test]
    fn nullary_function() {
        let ty = Ty::Fn { params: vec![], ret: Box::new(Ty::Unit) };
        assert_eq!(format!("{ty}"), "fn() -> Unit");
    }

    #[test]
    fn collections_and_options() {
        assert_eq!(format!("{}", Ty::Array(Box::new(Ty::Int))),  "[Int]");
        assert_eq!(format!("{}", Ty::Option(Box::new(Ty::Str))), "Option<Str>");
        assert_eq!(
            format!("{}", Ty::Result(Box::new(Ty::Int), Box::new(Ty::Str))),
            "Result<Int, Str>",
        );
    }

    #[test]
    fn tuple_disambiguates_singleton() {
        assert_eq!(format!("{}", Ty::Tuple(vec![Ty::Int, Ty::Bool])), "(Int, Bool)");
        // Singleton tuples get a trailing comma so they are distinct from
        // parenthesised types.
        assert_eq!(format!("{}", Ty::Tuple(vec![Ty::Int])), "(Int,)");
        assert_eq!(format!("{}", Ty::Tuple(vec![])), "()");
    }

    #[test]
    fn type_var_uses_question_prefix() {
        assert_eq!(format!("{}", Ty::Var(TyVar::new(7))), "?T7");
    }

    #[test]
    fn opaque_erases_to_inner() {
        let ty = Ty::Opaque { def_id: DefId::new(3), inner: Box::new(Ty::Str) };
        assert_eq!(format!("{ty}"), "Str");
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
