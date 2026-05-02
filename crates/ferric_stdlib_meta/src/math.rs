//! Auto-extracted metadata for the `math` stdlib module.
//!
//! Names + param names mirror `crates/ferric_stdlib/src/math.rs`.
//! Type signatures are `Signature::Unknown` unless filled in by hand.

use ferric_common::Ty;

use crate::{FunctionMeta, Signature};

fn sig_int_to_int() -> (Vec<Ty>, Ty) {
    (vec![Ty::Int], Ty::Int)
}
fn sig_two_int_to_int() -> (Vec<Ty>, Ty) {
    (vec![Ty::Int, Ty::Int], Ty::Int)
}
fn sig_int_to_float() -> (Vec<Ty>, Ty) {
    (vec![Ty::Int], Ty::Float)
}
fn sig_float_to_float() -> (Vec<Ty>, Ty) {
    (vec![Ty::Float], Ty::Float)
}
fn sig_two_float_to_float() -> (Vec<Ty>, Ty) {
    (vec![Ty::Float, Ty::Float], Ty::Float)
}
fn sig_float_to_int() -> (Vec<Ty>, Ty) {
    (vec![Ty::Float], Ty::Int)
}

pub const MATH_FNS: &[FunctionMeta] = &[
    FunctionMeta {
        name: "math_abs",
        params: &["n"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "math_min",
        params: &["a", "b"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "math_max",
        params: &["a", "b"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "math_clamp",
        params: &["n", "low", "high"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "math_pow",
        params: &["base", "exp"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "math_gcd",
        params: &["a", "b"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "math_lcm",
        params: &["a", "b"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "math_div_floor",
        params: &["a", "b"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "math_div_ceil",
        params: &["a", "b"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "math_rem_euclean",
        params: &["a", "b"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "math_abs_f",
        params: &["n"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "math_min_f",
        params: &["a", "b"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "math_max_f",
        params: &["a", "b"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "math_clamp_f",
        params: &["n", "low", "high"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "math_floor",
        params: &["n"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "math_ceil",
        params: &["n"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "math_round",
        params: &["n"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "math_trunc",
        params: &["n"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "math_fract",
        params: &["n"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "math_sqrt",
        params: &["n"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "math_cbrt",
        params: &["n"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "math_pow_f",
        params: &["base", "exp"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "math_log",
        params: &["n", "base"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "math_ln",
        params: &["n"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "math_log2",
        params: &["n"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "math_log10",
        params: &["n"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "math_exp",
        params: &["n"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "math_sin",
        params: &["n"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "math_cos",
        params: &["n"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "math_tan",
        params: &["n"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "math_asin",
        params: &["n"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "math_acos",
        params: &["n"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "math_atan",
        params: &["n"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "math_atan2",
        params: &["y", "x"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "math_hypot",
        params: &["a", "b"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "math_is_nan",
        params: &["n"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "math_is_finite",
        params: &["n"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "math_is_inf",
        params: &["n"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "math_PI",
        params: &[],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "math_E",
        params: &[],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "math_TAU",
        params: &[],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "math_INF",
        params: &[],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "math_NAN",
        params: &[],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "math_int_to_float",
        params: &["n"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "math_float_to_int",
        params: &["n"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "abs",
        params: &["n"],
        signature: Signature::Mono(sig_int_to_int),
        display: "fn(n: Int) -> Int",
    },
    FunctionMeta {
        name: "min",
        params: &["a", "b"],
        signature: Signature::Mono(sig_two_int_to_int),
        display: "fn(a: Int, b: Int) -> Int",
    },
    FunctionMeta {
        name: "max",
        params: &["a", "b"],
        signature: Signature::Mono(sig_two_int_to_int),
        display: "fn(a: Int, b: Int) -> Int",
    },
    FunctionMeta {
        name: "int_to_float",
        params: &["n"],
        signature: Signature::Mono(sig_int_to_float),
        display: "fn(n: Int) -> Float",
    },
    FunctionMeta {
        name: "sqrt",
        params: &["n"],
        signature: Signature::Mono(sig_float_to_float),
        display: "fn(n: Float) -> Float",
    },
    FunctionMeta {
        name: "pow",
        params: &["base", "exp"],
        signature: Signature::Mono(sig_two_float_to_float),
        display: "fn(base: Float, exp: Float) -> Float",
    },
    FunctionMeta {
        name: "floor",
        params: &["n"],
        signature: Signature::Mono(sig_float_to_int),
        display: "fn(n: Float) -> Int",
    },
    FunctionMeta {
        name: "ceil",
        params: &["n"],
        signature: Signature::Mono(sig_float_to_int),
        display: "fn(n: Float) -> Int",
    },
];
