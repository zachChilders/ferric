//! Auto-extracted metadata for the `map` stdlib module.
//!
//! Names + param names mirror `crates/ferric_stdlib/src/map.rs`.
//! Type signatures are `Signature::Unknown` unless filled in by hand.

use crate::{FunctionMeta, Signature};

pub const MAP_FNS: &[FunctionMeta] = &[
    FunctionMeta {
        name: "map_new",
        params: &[],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "map_from_list",
        params: &["pairs"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "map_from_lists",
        params: &["keys", "vals"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "map_get",
        params: &["m", "key"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "map_get_or",
        params: &["m", "key", "default"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "map_contains_key",
        params: &["m", "key"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "map_len",
        params: &["m"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "map_is_empty",
        params: &["m"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "map_insert",
        params: &["m", "key", "val"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "map_remove",
        params: &["m", "key"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "map_merge",
        params: &["a", "b"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "map_merge_with",
        params: &["a", "b", "f"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "map_keys",
        params: &["m"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "map_values",
        params: &["m"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "map_entries",
        params: &["m"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "map_map_values",
        params: &["m", "f"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "map_filter",
        params: &["m", "f"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "map_filter_map",
        params: &["m", "f"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "map_fold",
        params: &["m", "init", "f"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "map_group_by",
        params: &["l", "f"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "map_count_by",
        params: &["l", "f"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "map_builder",
        params: &[],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "map_set",
        params: &["buf", "key", "val"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "map_build",
        params: &["buf"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
];
