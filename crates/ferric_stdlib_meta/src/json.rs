//! Metadata for the `json` stdlib module.
//!
//! Names + param names mirror `crates/ferric_stdlib/src/json.rs`. Many of
//! these surface a `Json` opaque or `Result<Json, _>` at the spec level;
//! `NativeValue::Json`/`Result` collapse to `Value::Unit` at the VM
//! boundary today, so the LSP-facing return type is `Unit` for those —
//! revisit once that boundary is fixed.

use ferric_common::Ty;

use crate::{FunctionMeta, Signature};

fn sig_v_to_str() -> (Vec<Ty>, Ty) {
    (vec![Ty::Unit], Ty::Str)
}
fn sig_str_to_v() -> (Vec<Ty>, Ty) {
    (vec![Ty::Str], Ty::Unit)
}
fn sig_v_to_v() -> (Vec<Ty>, Ty) {
    (vec![Ty::Unit], Ty::Unit)
}
fn sig_v_to_bool() -> (Vec<Ty>, Ty) {
    (vec![Ty::Unit], Ty::Bool)
}
fn sig_v_key_to_v() -> (Vec<Ty>, Ty) {
    (vec![Ty::Unit, Ty::Str], Ty::Unit)
}
fn sig_no_args_to_v() -> (Vec<Ty>, Ty) {
    (vec![], Ty::Unit)
}
fn sig_bool_to_v() -> (Vec<Ty>, Ty) {
    (vec![Ty::Bool], Ty::Unit)
}
fn sig_int_to_v() -> (Vec<Ty>, Ty) {
    (vec![Ty::Int], Ty::Unit)
}
fn sig_float_to_v() -> (Vec<Ty>, Ty) {
    (vec![Ty::Float], Ty::Unit)
}
fn sig_str_to_v_constr() -> (Vec<Ty>, Ty) {
    (vec![Ty::Str], Ty::Unit)
}
fn sig_arr_to_v() -> (Vec<Ty>, Ty) {
    (vec![Ty::Array(Box::new(Ty::Unit))], Ty::Unit)
}

pub const JSON_FNS: &[FunctionMeta] = &[
    FunctionMeta {
        name: "json_to_str",
        params: &["v"],
        signature: Signature::Mono(sig_v_to_str),
        display: "fn(v: Json) -> Str",
    },
    FunctionMeta {
        name: "json_to_str_pretty",
        params: &["v"],
        signature: Signature::Mono(sig_v_to_str),
        display: "fn(v: Json) -> Str",
    },
    FunctionMeta {
        name: "json_parse",
        params: &["s"],
        signature: Signature::Mono(sig_str_to_v),
        display: "fn(s: Str) -> Json",
    },
    FunctionMeta {
        name: "json_as_bool",
        params: &["v"],
        signature: Signature::Mono(sig_v_to_bool),
        display: "fn(v: Json) -> Bool",
    },
    FunctionMeta {
        name: "json_as_int",
        params: &["v"],
        signature: Signature::Mono(|| (vec![Ty::Unit], Ty::Int)),
        display: "fn(v: Json) -> Int",
    },
    FunctionMeta {
        name: "json_as_float",
        params: &["v"],
        signature: Signature::Mono(|| (vec![Ty::Unit], Ty::Float)),
        display: "fn(v: Json) -> Float",
    },
    FunctionMeta {
        name: "json_as_str",
        params: &["v"],
        signature: Signature::Mono(|| (vec![Ty::Unit], Ty::Str)),
        display: "fn(v: Json) -> Str",
    },
    FunctionMeta {
        name: "json_as_array",
        params: &["v"],
        signature: Signature::Mono(|| (vec![Ty::Unit], Ty::Array(Box::new(Ty::Unit)))),
        display: "fn(v: Json) -> [Json]",
    },
    FunctionMeta {
        name: "json_as_object",
        params: &["v"],
        signature: Signature::Mono(sig_v_to_v),
        display: "fn(v: Json) -> Map<Str, Json>",
    },
    FunctionMeta {
        name: "json_is_null",
        params: &["v"],
        signature: Signature::Mono(sig_v_to_bool),
        display: "fn(v: Json) -> Bool",
    },
    FunctionMeta {
        name: "json_get",
        params: &["v", "key"],
        signature: Signature::Mono(sig_v_key_to_v),
        display: "fn(v: Json, key: Str) -> Json",
    },
    FunctionMeta {
        name: "json_get_path",
        params: &["v", "path"],
        signature: Signature::Mono(sig_v_key_to_v),
        display: "fn(v: Json, path: Str) -> Json",
    },
    FunctionMeta {
        name: "json_null",
        params: &[],
        signature: Signature::Mono(sig_no_args_to_v),
        display: "fn() -> Json",
    },
    FunctionMeta {
        name: "json_bool",
        params: &["b"],
        signature: Signature::Mono(sig_bool_to_v),
        display: "fn(b: Bool) -> Json",
    },
    FunctionMeta {
        name: "json_int",
        params: &["n"],
        signature: Signature::Mono(sig_int_to_v),
        display: "fn(n: Int) -> Json",
    },
    FunctionMeta {
        name: "json_float",
        params: &["n"],
        signature: Signature::Mono(sig_float_to_v),
        display: "fn(n: Float) -> Json",
    },
    FunctionMeta {
        name: "json_str",
        params: &["s"],
        signature: Signature::Mono(sig_str_to_v_constr),
        display: "fn(s: Str) -> Json",
    },
    FunctionMeta {
        name: "json_array",
        params: &["items"],
        signature: Signature::Mono(sig_arr_to_v),
        display: "fn(items: [Json]) -> Json",
    },
    FunctionMeta {
        name: "json_object",
        params: &["fields"],
        signature: Signature::Unknown,
        display: "fn(fields: [(Str, Json)]) -> Json",
    },
];
