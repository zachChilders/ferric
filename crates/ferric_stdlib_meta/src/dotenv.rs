//! Auto-extracted metadata for the `dotenv` stdlib module.
//!
//! Names + param names mirror `crates/ferric_stdlib/src/dotenv.rs`.
//! Type signatures are `Signature::Unknown` unless filled in by hand.

use crate::{FunctionMeta, Signature};

pub const DOTENV_FNS: &[FunctionMeta] = &[
    FunctionMeta {
        name: "dotenv_parse",
        params: &["content"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "dotenv_load",
        params: &[],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "dotenv_load_path",
        params: &["path"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "dotenv_load_into_env",
        params: &[],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "dotenv_load_into_env_path",
        params: &["path", "override_"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
];
