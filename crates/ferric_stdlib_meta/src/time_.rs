//! Auto-extracted metadata for the `time` stdlib module.
//!
//! Names + param names mirror `crates/ferric_stdlib/src/time_.rs`.
//! Type signatures are `Signature::Unknown` unless filled in by hand.

use crate::{FunctionMeta, Signature};

pub const TIME_FNS: &[FunctionMeta] = &[
    FunctionMeta { name: "time_now", params: &[], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "time_now_local", params: &[], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "time_monotonic", params: &[], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "time_from_unix", params: &["secs"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "time_from_unix_ms", params: &["ms"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "time_from_unix_ns", params: &["ns"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "time_unix", params: &["t"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "time_unix_ms", params: &["t"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "time_year", params: &["t"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "time_month", params: &["t"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "time_day", params: &["t"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "time_hour", params: &["t"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "time_minute", params: &["t"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "time_second", params: &["t"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "time_weekday", params: &["t"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "time_add_secs", params: &["t", "secs"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "time_add_ms", params: &["t", "ms"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "time_add_days", params: &["t", "days"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "time_diff_secs", params: &["a", "b"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "time_diff_ms", params: &["a", "b"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "time_format", params: &["t", "layout"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "time_format_iso", params: &["t"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "time_format_rfc2822", params: &["t"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "time_parse", params: &["s", "layout"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "time_parse_iso", params: &["s"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "time_secs", params: &["n"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "time_ms", params: &["n"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "time_minutes", params: &["n"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "time_hours", params: &["n"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "time_days", params: &["n"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "time_sleep", params: &["d"], signature: Signature::Unknown, display: "fn(...)" },
];
