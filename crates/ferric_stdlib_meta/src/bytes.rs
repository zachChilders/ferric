//! Auto-extracted metadata for the `bytes` stdlib module.
//!
//! Names + param names mirror `crates/ferric_stdlib/src/bytes.rs`.
//! Type signatures are `Signature::Unknown` unless filled in by hand.

use crate::{FunctionMeta, Signature};

pub const BYTES_FNS: &[FunctionMeta] = &[
    FunctionMeta { name: "bytes_new", params: &[], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "bytes_from_hex", params: &["s"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "bytes_from_base64", params: &["s"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "bytes_repeat", params: &["b", "n"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "bytes_len", params: &["b"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "bytes_is_empty", params: &["b"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "bytes_get", params: &["b", "index"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "bytes_slice", params: &["b", "from", "to"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "bytes_contains", params: &["haystack", "needle"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "bytes_find", params: &["haystack", "needle"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "bytes_concat", params: &["a", "b"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "bytes_to_hex", params: &["b"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "bytes_to_base64", params: &["b"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "bytes_to_str", params: &["b"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "bytes_to_list", params: &["b"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "bytes_from_list", params: &["ints"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "bytes_builder", params: &[], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "bytes_write_byte", params: &["buf", "byte"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "bytes_write_bytes", params: &["buf", "b"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "bytes_write_str", params: &["buf", "s"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "bytes_write_u16_le", params: &["buf", "n"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "bytes_write_u16_be", params: &["buf", "n"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "bytes_write_u32_le", params: &["buf", "n"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "bytes_write_u32_be", params: &["buf", "n"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "bytes_write_i64_le", params: &["buf", "n"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "bytes_write_i64_be", params: &["buf", "n"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "bytes_build", params: &["buf"], signature: Signature::Unknown, display: "fn(...)" },
];
