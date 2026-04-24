//! Bytecode types for the M3 compiler/VM.
//!
//! `Program` (in `results.rs`) holds a `Vec<Chunk>` plus an `entry` index. Each
//! function compiles to its own `Chunk`; top-level script statements compile
//! into the entry chunk. The VM executes `Chunk::code` against `Chunk::constants`.
//!
//! Function references flow through the constant pool (`Constant::Fn`,
//! `Constant::NativeFn`) so call sites compile to `LoadConst` + `Call(argc)`
//! uniformly for user and native callees. The M3 task doc lists `CallNative`
//! and `TailCall` opcodes; both are deferred — `Call` dispatches polymorphically
//! on the popped callable.

use serde::{Deserialize, Serialize};
use crate::{Symbol};

/// A compiled function (or the entry chunk for top-level script code).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Chunk {
    pub code: Vec<Op>,
    pub constants: Vec<Constant>,
    pub name: Symbol,
}

impl Chunk {
    pub fn new(name: Symbol) -> Self {
        Self { code: Vec::new(), constants: Vec::new(), name }
    }
}

/// A constant pool entry.
///
/// `Fn` and `NativeFn` allow a callable to be loaded onto the stack via
/// `LoadConst`, then invoked with `Call(argc)`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Constant {
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
    /// Reference to a user-defined function by chunk index.
    Fn(u16),
    /// Reference to a native function by interned name.
    NativeFn(Symbol),
}

/// Bytecode instruction set (M3).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Op {
    // Stack manipulation
    LoadConst(u8),
    LoadSlot(u8),
    StoreSlot(u8),
    Pop,
    Dup,

    // Integer arithmetic
    AddInt,
    SubInt,
    MulInt,
    DivInt,
    RemInt,
    NegInt,

    // Float arithmetic
    AddFloat,
    SubFloat,
    MulFloat,
    DivFloat,
    NegFloat,

    // Comparison (produce Bool)
    EqInt,
    NeInt,
    LtInt,
    GtInt,
    LeInt,
    GeInt,
    EqFloat,
    NeFloat,
    LtFloat,
    GtFloat,
    LeFloat,
    GeFloat,
    EqBool,
    NeBool,
    EqStr,
    NeStr,

    // Boolean logic
    Not,
    AndBool,
    OrBool,

    // String operations
    Concat,

    // Control flow — offsets are relative to the instruction immediately
    // *after* the jump (i.e. add to `ip` after it has been advanced).
    Jump(i16),
    JumpIfFalse(i16),
    JumpIfTrue(i16),
    Return,

    // Function call. Pops `argc` args + a callable from the stack.
    Call(u8),

    // Push Unit
    Unit,

    // Require-statement failure paths. Each pops a message `Str` from the
    // stack; `RequireFail` raises `RuntimeError::RequireError`, `RequireWarn`
    // eprintlns a warning and continues. An empty message string is treated
    // as "no message supplied" (matches the TreeWalker's `Option<String>`).
    RequireFail,
    RequireWarn,
}
