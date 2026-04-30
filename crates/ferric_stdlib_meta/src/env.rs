//! Metadata for the `env` stdlib module.
//!
//! Names mirror `crates/ferric_stdlib/src/env.rs`. Param names come from
//! the doc-comment header at the top of that file (today env's runtime
//! `register()` uses param-less `register`, so this meta restores named-arg
//! support).

use ferric_common::Ty;

use crate::{FunctionMeta, Signature};

fn sig_get_or() -> (Vec<Ty>, Ty) {
    (vec![Ty::Str, Ty::Str], Ty::Str)
}
fn sig_set() -> (Vec<Ty>, Ty) {
    (vec![Ty::Str, Ty::Str], Ty::Unit)
}
fn sig_remove() -> (Vec<Ty>, Ty) {
    (vec![Ty::Str], Ty::Unit)
}
fn sig_temp_dir() -> (Vec<Ty>, Ty) {
    (vec![], Ty::Str)
}
fn sig_args() -> (Vec<Ty>, Ty) {
    (vec![], Ty::Array(Box::new(Ty::Str)))
}
fn sig_exit() -> (Vec<Ty>, Ty) {
    (vec![Ty::Int], Ty::Unit)
}

pub const ENV_FNS: &[FunctionMeta] = &[
    FunctionMeta {
        name: "env_get",
        params: &["name"],
        signature: Signature::Unknown,
        display: "fn(name: Str) -> Option<Str>",
    },
    FunctionMeta {
        name: "env_get_or",
        params: &["name", "default"],
        signature: Signature::Mono(sig_get_or),
        display: "fn(name: Str, default: Str) -> Str",
    },
    FunctionMeta {
        name: "env_require",
        params: &["name"],
        signature: Signature::Unknown,
        display: "fn(name: Str) -> Result<Str, EnvError>",
    },
    FunctionMeta {
        name: "env_set",
        params: &["name", "val"],
        signature: Signature::Mono(sig_set),
        display: "fn(name: Str, val: Str) -> Unit",
    },
    FunctionMeta {
        name: "env_remove",
        params: &["name"],
        signature: Signature::Mono(sig_remove),
        display: "fn(name: Str) -> Unit",
    },
    FunctionMeta {
        name: "env_all",
        params: &[],
        signature: Signature::Unknown,
        display: "fn() -> Map<Str, Str>",
    },
    FunctionMeta {
        name: "env_args",
        params: &[],
        signature: Signature::Mono(sig_args),
        display: "fn() -> [Str]",
    },
    FunctionMeta {
        name: "env_cwd",
        params: &[],
        signature: Signature::Unknown,
        display: "fn() -> Result<Str, IoError>",
    },
    FunctionMeta {
        name: "env_home_dir",
        params: &[],
        signature: Signature::Unknown,
        display: "fn() -> Option<Str>",
    },
    FunctionMeta {
        name: "env_temp_dir",
        params: &[],
        signature: Signature::Mono(sig_temp_dir),
        display: "fn() -> Str",
    },
    FunctionMeta {
        name: "env_exit",
        params: &["code"],
        signature: Signature::Mono(sig_exit),
        display: "fn(code: Int) -> Unit",
    },
];
