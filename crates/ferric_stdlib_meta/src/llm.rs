//! Metadata for the `llm` stdlib module.
//!
//! Names + param names mirror `crates/ferric_stdlib/src/llm.rs`. Both
//! functions return `Result<_, LlmError>` at the spec level; the
//! `NativeValue::Result` variant collapses to `Value::Unit` at the VM
//! boundary today (`crates/ferric_vm/src/bytecode.rs:1058-1068`), so we
//! surface the inner type until the boundary is fixed.

use ferric_common::Ty;

use crate::{FunctionMeta, Signature};

fn sig_prompt() -> (Vec<Ty>, Ty) {
    (vec![Ty::Str], Ty::Str)
}
fn sig_prompt_schema() -> (Vec<Ty>, Ty) {
    // schema: Json — represented as opaque Unit at the meta layer until
    // Json crosses the VM boundary.
    (vec![Ty::Str, Ty::Unit], Ty::Unit)
}

pub const LLM_FNS: &[FunctionMeta] = &[
    FunctionMeta {
        name: "llm_prompt",
        params: &["text"],
        signature: Signature::Mono(sig_prompt),
        display: "fn(text: Str) -> Str",
    },
    FunctionMeta {
        name: "llm_prompt_schema",
        params: &["text", "schema"],
        signature: Signature::Mono(sig_prompt_schema),
        display: "fn(text: Str, schema: Json) -> Json",
    },
];
