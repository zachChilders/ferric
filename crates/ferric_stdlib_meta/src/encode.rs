//! Auto-extracted metadata for the `encode` stdlib module.
//!
//! Names + param names mirror `crates/ferric_stdlib/src/encode.rs`.
//! Type signatures are `Signature::Unknown` unless filled in by hand.

use crate::{FunctionMeta, Signature};

pub const ENCODE_FNS: &[FunctionMeta] = &[
    FunctionMeta {
        name: "encode_base64",
        params: &["data"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "encode_base64_url",
        params: &["data"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "encode_base64_decode",
        params: &["s"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "encode_base64_url_decode",
        params: &["s"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "encode_hex",
        params: &["data"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "encode_hex_upper",
        params: &["data"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "encode_hex_decode",
        params: &["s"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "encode_url_encode",
        params: &["s"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "encode_url_decode",
        params: &["s"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "encode_url_encode_component",
        params: &["s"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "encode_url_decode_component",
        params: &["s"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "encode_html_escape",
        params: &["s"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "encode_html_unescape",
        params: &["s"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "encode_csv_row",
        params: &["fields"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "encode_csv_parse_row",
        params: &["s"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
];
