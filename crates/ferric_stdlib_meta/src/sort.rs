//! Auto-extracted metadata for the `sort` stdlib module.
//!
//! Names + param names mirror `crates/ferric_stdlib/src/sort.rs`.
//! Type signatures are `Signature::Unknown` unless filled in by hand.

use crate::{FunctionMeta, Signature};

pub const SORT_FNS: &[FunctionMeta] = &[
    FunctionMeta {
        name: "sort_asc",
        params: &["l"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "sort_desc",
        params: &["l"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "sort_by",
        params: &["l", "key"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "sort_by_desc",
        params: &["l", "key"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "sort_with",
        params: &["l", "cmp"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "sort_cmp_int",
        params: &["a", "b"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "sort_cmp_float",
        params: &["a", "b"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "sort_cmp_str",
        params: &["a", "b"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "sort_cmp_str_natural",
        params: &["a", "b"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "sort_reverse",
        params: &["cmp", "a", "b"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "sort_then",
        params: &["cmp", "tiebreak", "a", "b"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "sort_top",
        params: &["l", "n", "key"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "sort_bottom",
        params: &["l", "n", "key"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "sort_is_sorted",
        params: &["l"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "sort_is_sorted_by",
        params: &["l", "key"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "sort_less",
        params: &[],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "sort_equal",
        params: &[],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "sort_greater",
        params: &[],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
];
