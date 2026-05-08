//! Auto-extracted metadata for the `str` stdlib module.
//!
//! Names + param names mirror `crates/ferric_stdlib/src/str_.rs`.
//! Type signatures are `Signature::Unknown` unless filled in by hand.

use ferric_common::Ty;

use crate::{FunctionMeta, Signature};

fn sig_str_len() -> (Vec<Ty>, Ty) {
    (vec![Ty::Str], Ty::Int)
}
fn sig_str_trim() -> (Vec<Ty>, Ty) {
    (vec![Ty::Str], Ty::Str)
}
fn sig_str_contains() -> (Vec<Ty>, Ty) {
    (vec![Ty::Str, Ty::Str], Ty::Bool)
}
fn sig_str_starts_with() -> (Vec<Ty>, Ty) {
    (vec![Ty::Str, Ty::Str], Ty::Bool)
}
fn sig_str_parse_int() -> (Vec<Ty>, Ty) {
    (vec![Ty::Str], Ty::Int)
}
fn sig_str_split() -> (Vec<Ty>, Ty) {
    (vec![Ty::Str, Ty::Str], Ty::Array(Box::new(Ty::Str)))
}
fn sig_str_to_upper() -> (Vec<Ty>, Ty) {
    (vec![Ty::Str], Ty::Str)
}
fn sig_str_to_lower() -> (Vec<Ty>, Ty) {
    (vec![Ty::Str], Ty::Str)
}
fn sig_str_ends_with() -> (Vec<Ty>, Ty) {
    (vec![Ty::Str, Ty::Str], Ty::Bool)
}
fn sig_str_is_empty() -> (Vec<Ty>, Ty) {
    (vec![Ty::Str], Ty::Bool)
}
fn sig_str_replace() -> (Vec<Ty>, Ty) {
    (vec![Ty::Str, Ty::Str, Ty::Str], Ty::Str)
}
fn sig_str_lines() -> (Vec<Ty>, Ty) {
    (vec![Ty::Str], Ty::Array(Box::new(Ty::Str)))
}
fn sig_str_chars() -> (Vec<Ty>, Ty) {
    (vec![Ty::Str], Ty::Array(Box::new(Ty::Str)))
}
fn sig_str_slice() -> (Vec<Ty>, Ty) {
    (vec![Ty::Str, Ty::Int, Ty::Int], Ty::Str)
}
fn sig_str_parse_float() -> (Vec<Ty>, Ty) {
    (vec![Ty::Str], Ty::Float)
}

pub const STR_FNS: &[FunctionMeta] = &[
    FunctionMeta {
        name: "str_len",
        params: &["s"],
        signature: Signature::Mono(sig_str_len),
        display: "fn(s: Str) -> Int",
    },
    FunctionMeta {
        name: "str_slice",
        params: &["s", "from", "to"],
        signature: Signature::Mono(sig_str_slice),
        display: "fn(s: Str, from: Int, to: Int) -> Str",
    },
    FunctionMeta {
        name: "str_contains",
        params: &["s", "sub"],
        signature: Signature::Mono(sig_str_contains),
        display: "fn(s: Str, sub: Str) -> Bool",
    },
    FunctionMeta {
        name: "str_starts_with",
        params: &["s", "prefix"],
        signature: Signature::Mono(sig_str_starts_with),
        display: "fn(s: Str, prefix: Str) -> Bool",
    },
    FunctionMeta {
        name: "str_ends_with",
        params: &["s", "suffix"],
        signature: Signature::Mono(sig_str_ends_with),
        display: "fn(s: Str, suffix: Str) -> Bool",
    },
    FunctionMeta {
        name: "str_find",
        params: &["s", "needle"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "str_find_all",
        params: &["s", "needle"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "str_split",
        params: &["s", "sep"],
        signature: Signature::Mono(sig_str_split),
        display: "fn(s: Str, sep: Str) -> [Str]",
    },
    FunctionMeta {
        name: "str_split_once",
        params: &["s", "on"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "str_lines",
        params: &["s"],
        signature: Signature::Mono(sig_str_lines),
        display: "fn(s: Str) -> [Str]",
    },
    FunctionMeta {
        name: "str_chars",
        params: &["s"],
        signature: Signature::Mono(sig_str_chars),
        display: "fn(s: Str) -> [Str]",
    },
    FunctionMeta {
        name: "str_trim",
        params: &["s"],
        signature: Signature::Mono(sig_str_trim),
        display: "fn(s: Str) -> Str",
    },
    FunctionMeta {
        name: "str_trim_start",
        params: &["s"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "str_trim_end",
        params: &["s"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "str_trim_chars",
        params: &["s", "chars"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "str_to_upper",
        params: &["s"],
        signature: Signature::Mono(sig_str_to_upper),
        display: "fn(s: Str) -> Str",
    },
    FunctionMeta {
        name: "str_to_lower",
        params: &["s"],
        signature: Signature::Mono(sig_str_to_lower),
        display: "fn(s: Str) -> Str",
    },
    FunctionMeta {
        name: "str_to_title",
        params: &["s"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "str_pad_start",
        params: &["s", "to", "with"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "str_pad_end",
        params: &["s", "to", "with"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "str_center",
        params: &["s", "to", "with"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "str_replace",
        params: &["s", "from", "to"],
        signature: Signature::Mono(sig_str_replace),
        display: "fn(s: Str, from: Str, to: Str) -> Str",
    },
    FunctionMeta {
        name: "str_replace_first",
        params: &["s", "from", "to"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "str_replace_n",
        params: &["s", "from", "to", "n"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "str_join",
        params: &["parts", "sep"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "str_repeat",
        params: &["s", "n"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "str_parse_int",
        params: &["s"],
        signature: Signature::Mono(sig_str_parse_int),
        display: "fn(s: Str) -> Int",
    },
    FunctionMeta {
        name: "str_parse_float",
        params: &["s"],
        signature: Signature::Mono(sig_str_parse_float),
        display: "fn(s: Str) -> Float",
    },
    FunctionMeta {
        name: "str_parse_bool",
        params: &["s"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "str_eq_ignore_case",
        params: &["a", "b"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "str_to_bytes",
        params: &["s"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "str_from_bytes",
        params: &["b"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "str_from_bytes_lossy",
        params: &["b"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "str_is_empty",
        params: &["s"],
        signature: Signature::Mono(sig_str_is_empty),
        display: "fn(s: Str) -> Bool",
    },
    FunctionMeta {
        name: "str_is_ascii",
        params: &["s"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "str_is_numeric",
        params: &["s"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
];
