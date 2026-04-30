//! Metadata for the `path` stdlib module.
//!
//! Names + param names mirror `crates/ferric_stdlib/src/path.rs`.

use ferric_common::Ty;

use crate::{FunctionMeta, Signature};

fn sig_str_to_str() -> (Vec<Ty>, Ty) {
    (vec![Ty::Str], Ty::Str)
}
fn sig_str_to_bool() -> (Vec<Ty>, Ty) {
    (vec![Ty::Str], Ty::Bool)
}
fn sig_two_str_to_str() -> (Vec<Ty>, Ty) {
    (vec![Ty::Str, Ty::Str], Ty::Str)
}
fn sig_str_to_arr_str() -> (Vec<Ty>, Ty) {
    (vec![Ty::Str], Ty::Array(Box::new(Ty::Str)))
}
fn sig_join() -> (Vec<Ty>, Ty) {
    // path_join(base: Str, parts: [Str]) -> Str
    (
        vec![Ty::Str, Ty::Array(Box::new(Ty::Str))],
        Ty::Str,
    )
}

pub const PATH_FNS: &[FunctionMeta] = &[
    FunctionMeta {
        name: "path_join",
        params: &["base", "parts"],
        signature: Signature::Mono(sig_join),
        display: "fn(base: Str, parts: [Str]) -> Str",
    },
    FunctionMeta {
        name: "path_parent",
        params: &["p"],
        signature: Signature::Mono(sig_str_to_str),
        display: "fn(p: Str) -> Str",
    },
    FunctionMeta {
        name: "path_filename",
        params: &["p"],
        signature: Signature::Mono(sig_str_to_str),
        display: "fn(p: Str) -> Str",
    },
    FunctionMeta {
        name: "path_stem",
        params: &["p"],
        signature: Signature::Mono(sig_str_to_str),
        display: "fn(p: Str) -> Str",
    },
    FunctionMeta {
        name: "path_extension",
        params: &["p"],
        signature: Signature::Mono(sig_str_to_str),
        display: "fn(p: Str) -> Str",
    },
    FunctionMeta {
        name: "path_with_extension",
        params: &["p", "ext"],
        signature: Signature::Mono(sig_two_str_to_str),
        display: "fn(p: Str, ext: Str) -> Str",
    },
    FunctionMeta {
        name: "path_is_absolute",
        params: &["p"],
        signature: Signature::Mono(sig_str_to_bool),
        display: "fn(p: Str) -> Bool",
    },
    FunctionMeta {
        name: "path_is_relative",
        params: &["p"],
        signature: Signature::Mono(sig_str_to_bool),
        display: "fn(p: Str) -> Bool",
    },
    FunctionMeta {
        name: "path_normalize",
        params: &["p"],
        signature: Signature::Mono(sig_str_to_str),
        display: "fn(p: Str) -> Str",
    },
    FunctionMeta {
        name: "path_relative_to",
        params: &["p", "base"],
        signature: Signature::Mono(sig_two_str_to_str),
        display: "fn(p: Str, base: Str) -> Str",
    },
    FunctionMeta {
        name: "path_components",
        params: &["p"],
        signature: Signature::Mono(sig_str_to_arr_str),
        display: "fn(p: Str) -> [Str]",
    },
];
