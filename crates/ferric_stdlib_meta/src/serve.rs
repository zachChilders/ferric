//! Auto-extracted metadata for the `serve` stdlib module.
//!
//! Names + param names mirror `crates/ferric_stdlib/src/serve.rs`.
//! Type signatures are `Signature::Unknown` unless filled in by hand.

use crate::{FunctionMeta, Signature};

pub const SERVE_FNS: &[FunctionMeta] = &[
    FunctionMeta {
        name: "serve_new",
        params: &[],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "serve_get",
        params: &["s", "path", "handler"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "serve_post",
        params: &["s", "path", "handler"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "serve_put",
        params: &["s", "path", "handler"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "serve_patch",
        params: &["s", "path", "handler"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "serve_delete",
        params: &["s", "path", "handler"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "serve_any",
        params: &["s", "path", "handler"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "serve_use",
        params: &["s", "m"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "serve_use_prefix",
        params: &["s", "prefix", "m"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "serve_mount",
        params: &["s", "prefix", "sub"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "serve_listen",
        params: &["s", "port"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "serve_listen_addr",
        params: &["s", "addr", "port"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "serve_method",
        params: &["r"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "serve_path",
        params: &["r"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "serve_query",
        params: &["r"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "serve_param",
        params: &["r", "name"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "serve_header_get",
        params: &["r", "name"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "serve_body_bytes",
        params: &["r"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "serve_body_str",
        params: &["r"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "serve_body_json",
        params: &["r"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "serve_response",
        params: &["status", "body"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "serve_ok",
        params: &["body"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "serve_ok_str",
        params: &["body"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "serve_ok_json",
        params: &["body"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "serve_created",
        params: &["body"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "serve_no_content",
        params: &[],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "serve_bad_request",
        params: &["message"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "serve_unauthorized",
        params: &["message"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "serve_forbidden",
        params: &["message"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "serve_not_found",
        params: &["message"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "serve_internal_error",
        params: &["message"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "serve_with_header",
        params: &["r", "name", "val"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "serve_with_headers",
        params: &["r", "h"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
];
