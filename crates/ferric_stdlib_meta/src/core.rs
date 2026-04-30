//! Compiler-internal natives that don't have a natural per-module home.
//! Mirrors the registrations directly under `register_stdlib` in
//! `crates/ferric_stdlib/src/lib.rs` (currently `shell_stdout` and
//! `shell_exit_code`). `__shell_exec` is excluded — it's emitted by the
//! compiler rather than called from Ferric source, so the resolver doesn't
//! need it.

use ferric_common::Ty;

use crate::{FunctionMeta, Signature};

fn sig_shell_stdout() -> (Vec<Ty>, Ty) {
    (vec![Ty::ShellOutput], Ty::Str)
}
fn sig_shell_exit_code() -> (Vec<Ty>, Ty) {
    (vec![Ty::ShellOutput], Ty::Int)
}

pub const CORE_FNS: &[FunctionMeta] = &[
    FunctionMeta {
        name: "shell_stdout",
        params: &["output"],
        signature: Signature::Mono(sig_shell_stdout),
        display: "fn(output: ShellOutput) -> Str",
    },
    FunctionMeta {
        name: "shell_exit_code",
        params: &["output"],
        signature: Signature::Mono(sig_shell_exit_code),
        display: "fn(output: ShellOutput) -> Int",
    },
];
