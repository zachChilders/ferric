//! Auto-extracted metadata for the `proc` stdlib module.
//!
//! Names + param names mirror `crates/ferric_stdlib/src/proc_.rs`.
//! Type signatures are `Signature::Unknown` unless filled in by hand.

use crate::{FunctionMeta, Signature};

pub const PROC_FNS: &[FunctionMeta] = &[
    FunctionMeta {
        name: "proc_run",
        params: &["cmd"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "proc_run_shell",
        params: &["cmd"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "proc_stdout",
        params: &["o"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "proc_stderr",
        params: &["o"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "proc_exit_code",
        params: &["o"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "proc_ok",
        params: &["o"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "proc_command",
        params: &["program"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "proc_arg",
        params: &["c", "a"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "proc_args",
        params: &["c", "a"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "proc_env_set",
        params: &["c", "key", "val"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "proc_env_clear",
        params: &["c"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "proc_cwd",
        params: &["c", "path"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "proc_stdin_bytes",
        params: &["c", "data"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "proc_stdin_str",
        params: &["c", "data"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "proc_timeout",
        params: &["c", "ms"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "proc_spawn",
        params: &["c"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "proc_spawn_streaming",
        params: &["c"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "proc_write_stdin",
        params: &["p", "data"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "proc_read_stdout_line",
        params: &["p"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "proc_wait",
        params: &["p"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "proc_kill",
        params: &["p"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
];
