//! Auto-extracted metadata for the `rand` stdlib module.
//!
//! Names + param names mirror `crates/ferric_stdlib/src/rand_.rs`.
//! Type signatures are `Signature::Unknown` unless filled in by hand.

use crate::{FunctionMeta, Signature};

pub const RAND_FNS: &[FunctionMeta] = &[
    FunctionMeta { name: "rand_int", params: &["low", "high"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "rand_float", params: &[], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "rand_bool", params: &[], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "rand_pick", params: &["l"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "rand_shuffle", params: &["l"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "rand_sample", params: &["l", "n"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "rand_seeded", params: &["seed"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "rand_rng_int", params: &["r", "low", "high"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "rand_rng_float", params: &["r"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "rand_rng_pick", params: &["r", "l"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "rand_rng_shuffle", params: &["r", "l"], signature: Signature::Unknown, display: "fn(...)" },
];
