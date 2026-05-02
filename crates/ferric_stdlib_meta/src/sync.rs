//! Auto-extracted metadata for the `sync` stdlib module.
//!
//! Names + param names mirror `crates/ferric_stdlib/src/sync.rs`.
//! Type signatures are `Signature::Unknown` unless filled in by hand.

use crate::{FunctionMeta, Signature};

pub const SYNC_FNS: &[FunctionMeta] = &[
    FunctionMeta {
        name: "sync_channel",
        params: &["capacity"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "sync_send",
        params: &["s", "val"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "sync_recv",
        params: &["r"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "sync_try_recv",
        params: &["r"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "sync_mutex",
        params: &["val"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "sync_lock",
        params: &["m", "f"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "sync_once",
        params: &["init"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "sync_get",
        params: &["o"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
];
