//! Auto-extracted metadata for the `log` stdlib module.
//!
//! Names + param names mirror `crates/ferric_stdlib/src/log.rs`.
//! Type signatures are `Signature::Unknown` unless filled in by hand.

use crate::{FunctionMeta, Signature};

pub const LOG_FNS: &[FunctionMeta] = &[
    FunctionMeta { name: "log_debug", params: &["msg"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "log_info", params: &["msg"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "log_warn", params: &["msg"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "log_error", params: &["msg"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "log_debug_fields", params: &["msg", "fields"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "log_info_fields", params: &["msg", "fields"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "log_warn_fields", params: &["msg", "fields"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "log_error_fields", params: &["msg", "fields"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "log_new", params: &["fields"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "log_with", params: &["l", "key", "val"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "log_log_debug", params: &["l", "msg"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "log_log_info", params: &["l", "msg"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "log_log_warn", params: &["l", "msg"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "log_log_error", params: &["l", "msg"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "log_set_level", params: &["level"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "log_get_level", params: &[], signature: Signature::Unknown, display: "fn(...)" },
];
