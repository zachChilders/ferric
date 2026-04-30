//! Auto-extracted metadata for the `fmt` stdlib module.
//!
//! Names + param names mirror `crates/ferric_stdlib/src/fmt.rs`.
//! Type signatures are `Signature::Unknown` unless filled in by hand.

use ferric_common::Ty;

use crate::{FunctionMeta, Signature};

fn sig_int_to_str() -> (Vec<Ty>, Ty) { (vec![Ty::Int], Ty::Str) }
fn sig_float_to_str() -> (Vec<Ty>, Ty) { (vec![Ty::Float], Ty::Str) }
fn sig_bool_to_str() -> (Vec<Ty>, Ty) { (vec![Ty::Bool], Ty::Str) }

pub const FMT_FNS: &[FunctionMeta] = &[
    FunctionMeta { name: "fmt_format", params: &["template", "args"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "fmt_int", params: &["n", "radix"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "fmt_int_padded", params: &["n", "width", "pad"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "fmt_float", params: &["n", "decimals"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "fmt_float_exp", params: &["n", "decimals"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "fmt_bool", params: &["b"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "fmt_debug", params: &["v"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "int_to_str", params: &["n"], signature: Signature::Mono(sig_int_to_str), display: "fn(n: Int) -> Str" },
    FunctionMeta { name: "float_to_str", params: &["n"], signature: Signature::Mono(sig_float_to_str), display: "fn(n: Float) -> Str" },
    FunctionMeta { name: "bool_to_str", params: &["b"], signature: Signature::Mono(sig_bool_to_str), display: "fn(b: Bool) -> Str" },
];
