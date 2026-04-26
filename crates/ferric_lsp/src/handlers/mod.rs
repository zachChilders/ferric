//! LSP request handlers. Each module owns one LSP method.
//!
//! Bodies in this milestone are stubs that return the LSP-defined empty
//! response. Real implementations land in:
//!  - Task 04: `diagnostics`, `document_symbols`
//!  - Task 05: `completion`, `hover`, `goto_def`
//!  - Task 06: `inlay_hints`

pub mod completion;
pub mod diagnostics;
pub mod document_symbols;
pub mod goto_def;
pub mod hover;
pub mod inlay_hints;
