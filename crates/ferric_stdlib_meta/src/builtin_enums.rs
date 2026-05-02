//! Built-in enums (`Option`, `Result`) — the resolver needs these to bind
//! variant constructors. Mirrors `ferric_stdlib::builtin_enum_table` and
//! `ferric_lsp::pipeline::stdlib_builtin_enum_table`.
//!
//! We don't store these as `Ty::Enum` because the resolver uses
//! `TypeAnnotation::Infer` to defer payload typing to inference — the
//! same shape both consumers expect today.

use ferric_common::TypeAnnotation;

/// One built-in enum: `(name, [(variant_name, [payload annotations])])`.
pub struct BuiltinEnum {
    pub name: &'static str,
    pub variants: &'static [BuiltinVariant],
}

pub struct BuiltinVariant {
    pub name: &'static str,
    /// Number of `TypeAnnotation::Infer` payload slots. Stored as a count
    /// because `TypeAnnotation` isn't `'static`-constructible (it carries
    /// `Symbol`s for named types).
    pub payload_arity: usize,
}

pub const BUILTIN_ENUMS: &[BuiltinEnum] = &[
    BuiltinEnum {
        name: "Option",
        variants: &[
            BuiltinVariant {
                name: "Some",
                payload_arity: 1,
            },
            BuiltinVariant {
                name: "None",
                payload_arity: 0,
            },
        ],
    },
    BuiltinEnum {
        name: "Result",
        variants: &[
            BuiltinVariant {
                name: "Ok",
                payload_arity: 1,
            },
            BuiltinVariant {
                name: "Err",
                payload_arity: 1,
            },
        ],
    },
];

/// Convert a payload arity to the corresponding `Vec<TypeAnnotation>` for
/// resolver consumption (every payload starts as `TypeAnnotation::Infer`).
pub fn payload_annotations(arity: usize) -> Vec<TypeAnnotation> {
    (0..arity).map(|_| TypeAnnotation::Infer).collect()
}
