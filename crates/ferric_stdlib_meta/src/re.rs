//! Auto-extracted metadata for the `re` stdlib module.
//!
//! Names + param names mirror `crates/ferric_stdlib/src/re.rs`.
//! Type signatures are `Signature::Unknown` unless filled in by hand.

use crate::{FunctionMeta, Signature};

pub const RE_FNS: &[FunctionMeta] = &[
    FunctionMeta { name: "re_compile", params: &["pattern"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "re_compile_flags", params: &["pattern", "flags"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "re_is_match", params: &["r", "s"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "re_find", params: &["r", "s"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "re_find_all", params: &["r", "s"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "re_match_str", params: &["m"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "re_match_start", params: &["m"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "re_match_end", params: &["m"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "re_capture", params: &["m", "index"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "re_capture_name", params: &["m", "name"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "re_replace", params: &["r", "s", "with"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "re_replace_all", params: &["r", "s", "with"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "re_replace_fn", params: &["r", "s", "f"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "re_split", params: &["r", "s"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "re_split_n", params: &["r", "s", "n"], signature: Signature::Unknown, display: "fn(...)" },
];
