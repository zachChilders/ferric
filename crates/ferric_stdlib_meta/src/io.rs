//! Metadata for the `io` stdlib module.
//!
//! Names + param names mirror `crates/ferric_stdlib/src/io.rs`. Type
//! signatures are filled in by hand. Where a function returns a Ferric
//! `Result<_, IoError>` or `Option<_>` at the spec level, we surface the
//! inner type — the `NativeValue::Result`/`Option` variants collapse to
//! `Value::Unit` at the VM boundary today
//! (`crates/ferric_vm/src/bytecode.rs:1058-1068`), so the LSP-facing
//! return type matches what actually arrives at the call site.

use ferric_common::Ty;

use crate::{FunctionMeta, Signature};

fn s(t: Ty) -> Box<Ty> {
    Box::new(t)
}

fn sig_str_to_unit() -> (Vec<Ty>, Ty) {
    (vec![Ty::Str], Ty::Unit)
}
fn sig_two_str_to_unit() -> (Vec<Ty>, Ty) {
    (vec![Ty::Str, Ty::Str], Ty::Unit)
}
fn sig_str_to_str() -> (Vec<Ty>, Ty) {
    (vec![Ty::Str], Ty::Str)
}
fn sig_str_to_bool() -> (Vec<Ty>, Ty) {
    (vec![Ty::Str], Ty::Bool)
}
fn sig_str_to_int() -> (Vec<Ty>, Ty) {
    (vec![Ty::Str], Ty::Int)
}
fn sig_str_to_arr_str() -> (Vec<Ty>, Ty) {
    (vec![Ty::Str], Ty::Array(s(Ty::Str)))
}
fn sig_str_to_arr_int() -> (Vec<Ty>, Ty) {
    (vec![Ty::Str], Ty::Array(s(Ty::Int)))
}
fn sig_no_args_to_str() -> (Vec<Ty>, Ty) {
    (vec![], Ty::Str)
}
fn sig_writer_str_to_unit() -> (Vec<Ty>, Ty) {
    // FileWriter is opaque at the meta layer; type as Unit for now.
    (vec![Ty::Unit, Ty::Str], Ty::Unit)
}
fn sig_writer_to_unit() -> (Vec<Ty>, Ty) {
    (vec![Ty::Unit], Ty::Unit)
}
fn sig_writer_open() -> (Vec<Ty>, Ty) {
    (vec![Ty::Str], Ty::Unit)
}

pub const IO_FNS: &[FunctionMeta] = &[
    FunctionMeta {
        name: "io_print",
        params: &["s"],
        signature: Signature::Mono(sig_str_to_unit),
        display: "fn(s: Str) -> Unit",
    },
    FunctionMeta {
        name: "io_println",
        params: &["s"],
        signature: Signature::Mono(sig_str_to_unit),
        display: "fn(s: Str) -> Unit",
    },
    FunctionMeta {
        name: "io_eprint",
        params: &["s"],
        signature: Signature::Mono(sig_str_to_unit),
        display: "fn(s: Str) -> Unit",
    },
    FunctionMeta {
        name: "io_eprintln",
        params: &["s"],
        signature: Signature::Mono(sig_str_to_unit),
        display: "fn(s: Str) -> Unit",
    },
    FunctionMeta {
        name: "io_read_line",
        params: &[],
        signature: Signature::Mono(sig_no_args_to_str),
        display: "fn() -> Str",
    },
    FunctionMeta {
        name: "io_read_all",
        params: &[],
        signature: Signature::Mono(sig_no_args_to_str),
        display: "fn() -> Str",
    },
    FunctionMeta {
        name: "io_read_file",
        params: &["path"],
        signature: Signature::Mono(sig_str_to_str),
        display: "fn(path: Str) -> Str",
    },
    FunctionMeta {
        name: "io_write_file",
        params: &["path", "content"],
        signature: Signature::Mono(sig_two_str_to_unit),
        display: "fn(path: Str, content: Str) -> Unit",
    },
    FunctionMeta {
        name: "io_append_file",
        params: &["path", "content"],
        signature: Signature::Mono(sig_two_str_to_unit),
        display: "fn(path: Str, content: Str) -> Unit",
    },
    FunctionMeta {
        name: "io_read_lines",
        params: &["path"],
        signature: Signature::Mono(sig_str_to_arr_str),
        display: "fn(path: Str) -> [Str]",
    },
    FunctionMeta {
        name: "io_read_bytes",
        params: &["path"],
        signature: Signature::Mono(sig_str_to_arr_int),
        display: "fn(path: Str) -> [Int]",
    },
    FunctionMeta {
        name: "io_write_bytes",
        params: &["path", "content"],
        signature: Signature::Mono(|| (vec![Ty::Str, Ty::Array(Box::new(Ty::Int))], Ty::Unit)),
        display: "fn(path: Str, content: [Int]) -> Unit",
    },
    FunctionMeta {
        name: "io_exists",
        params: &["path"],
        signature: Signature::Mono(sig_str_to_bool),
        display: "fn(path: Str) -> Bool",
    },
    FunctionMeta {
        name: "io_is_file",
        params: &["path"],
        signature: Signature::Mono(sig_str_to_bool),
        display: "fn(path: Str) -> Bool",
    },
    FunctionMeta {
        name: "io_is_dir",
        params: &["path"],
        signature: Signature::Mono(sig_str_to_bool),
        display: "fn(path: Str) -> Bool",
    },
    FunctionMeta {
        name: "io_file_size",
        params: &["path"],
        signature: Signature::Mono(sig_str_to_int),
        display: "fn(path: Str) -> Int",
    },
    FunctionMeta {
        name: "io_modified_at",
        params: &["path"],
        signature: Signature::Mono(sig_str_to_int),
        display: "fn(path: Str) -> Int",
    },
    FunctionMeta {
        name: "io_list_dir",
        params: &["path"],
        signature: Signature::Mono(sig_str_to_arr_str),
        display: "fn(path: Str) -> [Str]",
    },
    FunctionMeta {
        name: "io_list_dir_recursive",
        params: &["path"],
        signature: Signature::Mono(sig_str_to_arr_str),
        display: "fn(path: Str) -> [Str]",
    },
    FunctionMeta {
        name: "io_make_dir",
        params: &["path"],
        signature: Signature::Mono(sig_str_to_unit),
        display: "fn(path: Str) -> Unit",
    },
    FunctionMeta {
        name: "io_make_dir_all",
        params: &["path"],
        signature: Signature::Mono(sig_str_to_unit),
        display: "fn(path: Str) -> Unit",
    },
    FunctionMeta {
        name: "io_remove_file",
        params: &["path"],
        signature: Signature::Mono(sig_str_to_unit),
        display: "fn(path: Str) -> Unit",
    },
    FunctionMeta {
        name: "io_remove_dir",
        params: &["path"],
        signature: Signature::Mono(sig_str_to_unit),
        display: "fn(path: Str) -> Unit",
    },
    FunctionMeta {
        name: "io_remove_dir_all",
        params: &["path"],
        signature: Signature::Mono(sig_str_to_unit),
        display: "fn(path: Str) -> Unit",
    },
    FunctionMeta {
        name: "io_rename",
        params: &["from", "to"],
        signature: Signature::Mono(sig_two_str_to_unit),
        display: "fn(from: Str, to: Str) -> Unit",
    },
    FunctionMeta {
        name: "io_copy",
        params: &["from", "to"],
        signature: Signature::Mono(sig_two_str_to_unit),
        display: "fn(from: Str, to: Str) -> Unit",
    },
    FunctionMeta {
        name: "io_file_writer",
        params: &["path"],
        signature: Signature::Mono(sig_writer_open),
        display: "fn(path: Str) -> FileWriter",
    },
    FunctionMeta {
        name: "io_write",
        params: &["w", "s"],
        signature: Signature::Mono(sig_writer_str_to_unit),
        display: "fn(w: FileWriter, s: Str) -> Unit",
    },
    FunctionMeta {
        name: "io_writeln",
        params: &["w", "s"],
        signature: Signature::Mono(sig_writer_str_to_unit),
        display: "fn(w: FileWriter, s: Str) -> Unit",
    },
    FunctionMeta {
        name: "io_flush",
        params: &["w"],
        signature: Signature::Mono(sig_writer_to_unit),
        display: "fn(w: FileWriter) -> Unit",
    },
    FunctionMeta {
        name: "io_close",
        params: &["w"],
        signature: Signature::Mono(sig_writer_to_unit),
        display: "fn(w: FileWriter) -> Unit",
    },
    FunctionMeta {
        name: "println",
        params: &["s"],
        signature: Signature::Mono(sig_str_to_unit),
        display: "fn(s: Str) -> Unit",
    },
    FunctionMeta {
        name: "print",
        params: &["s"],
        signature: Signature::Mono(sig_str_to_unit),
        display: "fn(s: Str) -> Unit",
    },
    FunctionMeta {
        name: "eprint",
        params: &["s"],
        signature: Signature::Mono(sig_str_to_unit),
        display: "fn(s: Str) -> Unit",
    },
    FunctionMeta {
        name: "eprintln",
        params: &["s"],
        signature: Signature::Mono(sig_str_to_unit),
        display: "fn(s: Str) -> Unit",
    },
    FunctionMeta {
        name: "read_line",
        params: &[],
        signature: Signature::Mono(sig_no_args_to_str),
        display: "fn() -> Str",
    },
];
