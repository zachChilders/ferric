//! Auto-extracted metadata for the `http` stdlib module.
//!
//! Names + param names mirror `crates/ferric_stdlib/src/http.rs`.
//! Type signatures are `Signature::Unknown` unless filled in by hand.

use crate::{FunctionMeta, Signature};

pub const HTTP_FNS: &[FunctionMeta] = &[
    FunctionMeta {
        name: "http_get",
        params: &["url"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "http_post",
        params: &["url", "body"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "http_put",
        params: &["url", "body"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "http_patch",
        params: &["url", "body"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "http_delete",
        params: &["url"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "http_request",
        params: &["method", "url"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "http_header",
        params: &["req", "name", "val"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "http_headers",
        params: &["req", "h"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "http_body_bytes",
        params: &["req", "b"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "http_body_str",
        params: &["req", "s"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "http_body_json",
        params: &["req", "val"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "http_timeout",
        params: &["req", "ms"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "http_follow_redirects",
        params: &["req", "follow"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "http_basic_auth",
        params: &["req", "user", "pass"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "http_bearer",
        params: &["req", "token"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "http_send",
        params: &["req"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "http_status",
        params: &["r"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "http_ok",
        params: &["r"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "http_header_get",
        params: &["r", "name"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "http_headers_all",
        params: &["r"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "http_body_bytes_resp",
        params: &["r"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "http_body_str_resp",
        params: &["r"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "http_body_json_resp",
        params: &["r"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "http_get_json",
        params: &["url"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
    FunctionMeta {
        name: "http_post_json",
        params: &["url", "body"],
        signature: Signature::Unknown,
        display: "fn(...)",
    },
];
