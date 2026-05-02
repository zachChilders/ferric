//! Auto-extracted metadata for the `list` stdlib module.
//!
//! Names + param names mirror `crates/ferric_stdlib/src/list.rs`.
//! Type signatures are `Signature::Unknown` unless filled in by hand.

use ferric_common::{Ty, TypeScheme};

use crate::{FunctionMeta, Signature, TyVarGen};

/// `array_len(arr: [T]) -> Int` — generic in the element type. The polymorphic
/// builder mints one fresh `TyVar` per call so each call site gets its own
/// element type.
fn sig_array_len(g: &mut dyn TyVarGen) -> TypeScheme {
    let t = g.fresh();
    TypeScheme {
        forall: vec![t],
        ty: Ty::Fn {
            params: vec![Ty::Array(Box::new(Ty::Var(t)))],
            ret: Box::new(Ty::Int),
        },
    }
}

pub const LIST_FNS: &[FunctionMeta] = &[
    FunctionMeta {
        name: "list_new",
        params: &[],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "list_of",
        params: &["items"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "list_repeat",
        params: &["item", "n"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "list_range",
        params: &["from", "to"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "list_range_inclusive",
        params: &["from", "to"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "list_len",
        params: &["l"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "list_is_empty",
        params: &["l"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "list_get",
        params: &["l", "index"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "list_first",
        params: &["l"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "list_last",
        params: &["l"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "list_slice",
        params: &["l", "from", "to"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "list_contains",
        params: &["l", "item"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "list_find",
        params: &["l", "f"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "list_find_index",
        params: &["l", "f"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "list_index_of",
        params: &["l", "item"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "list_map",
        params: &["l", "f"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "list_flat_map",
        params: &["l", "f"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "list_filter",
        params: &["l", "f"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "list_filter_map",
        params: &["l", "f"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "list_reduce",
        params: &["l", "f"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "list_fold",
        params: &["l", "init", "f"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "list_scan",
        params: &["l", "init", "f"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "list_zip",
        params: &["a", "b"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "list_zip_with",
        params: &["a", "b", "f"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "list_enumerate",
        params: &["l"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "list_flatten",
        params: &["l"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "list_chunk",
        params: &["l", "size"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "list_window",
        params: &["l", "size"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "list_take",
        params: &["l", "n"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "list_drop",
        params: &["l", "n"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "list_take_while",
        params: &["l", "f"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "list_drop_while",
        params: &["l", "f"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "list_partition",
        params: &["l", "f"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "list_unzip",
        params: &["l"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "list_any",
        params: &["l", "f"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "list_all",
        params: &["l", "f"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "list_count",
        params: &["l", "f"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "list_sum",
        params: &["l"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "list_sum_float",
        params: &["l"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "list_min",
        params: &["l"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "list_max",
        params: &["l"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "list_concat",
        params: &["a", "b"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "list_prepend",
        params: &["item", "l"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "list_append",
        params: &["l", "item"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "list_insert",
        params: &["l", "at", "item"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "list_remove",
        params: &["l", "at"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "list_reverse",
        params: &["l"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "list_unique",
        params: &["l"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "list_intersperse",
        params: &["l", "sep"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "list_join_str",
        params: &["l", "sep"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "list_builder",
        params: &[],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "list_push",
        params: &["buf", "item"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "list_pop",
        params: &["buf"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "list_build",
        params: &["buf"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "array_len",
        params: &["arr"],
        signature: Signature::Poly(sig_array_len),
        display: "fn(arr: [T]) -> Int",
    },
];
