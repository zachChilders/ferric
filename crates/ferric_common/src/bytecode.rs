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

use crate::Symbol;
use serde::{Deserialize, Serialize};

/// A compiled function (or the entry chunk for top-level script code).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Chunk {
    pub code: Vec<Op>,
    pub constants: Vec<Constant>,
    pub name: Symbol,
}

impl Chunk {
    pub fn new(name: Symbol) -> Self {
        Self {
            code: Vec::new(),
            constants: Vec::new(),
            name,
        }
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

    // ---------------- M4: structs / enums / tuples / patterns ----------------
    /// Pop `n` values from the stack and push a struct value with those fields
    /// (declaration order, leftmost-first).
    MakeStruct(u8),
    /// Pop a struct, push the value of field at index `idx`.
    GetField(u8),
    /// Pop `n` values from the stack and push an enum variant value with the
    /// given variant index and field count.
    MakeVariant(u16, u8),
    /// Pop the top of the stack: if it's an enum variant with the given
    /// index, push `true`; otherwise push `false`.
    MatchVariant(u16),
    /// Pop a variant from the stack and push each of its fields onto the stack
    /// (leftmost-first; rightmost ends up on top).
    UnpackVariant,
    /// Pop `n` values, push a tuple containing them (leftmost-first).
    MakeTuple(u8),
    /// Pop a tuple, push its element at the given index.
    GetTupleField(u8),

    // ---------------- M6: arrays / closures ----------------------------------
    /// Pop `n` values from the stack and push an array value with those
    /// elements (leftmost-first).
    MakeArray(u8),
    /// Pop an index (Int) and an array, push the element at that index.
    /// Raises `IndexOutOfBounds` at runtime if the index is invalid.
    ArrayGet,
    /// Pop an array, push its length as an Int.
    ArrayLen,
    /// Build a closure: pop `capture_count` values from the stack (in the
    /// reverse order they were pushed), then push a closure value referencing
    /// the chunk at `fn_idx` with those captures.
    MakeClosure(u16, u8),

    // ---------------- M8: async / await --------------------------------------
    /// Build a deferred async value: pop `n_captures` values from the stack,
    /// then push `Value::Async` wrapping a Pending state that captures the
    /// chunk index and the captures vector. The body chunk is **not run
    /// here** — execution is deferred until the value is awaited or
    /// spawned. Mirrors `MakeClosure`'s shape: captures occupy the leading
    /// slots of the deferred call frame.
    ///
    /// Lazy semantics (M8 Task 5 Option A) is what makes true concurrency
    /// possible: `spawn(task: foo(1))` can move the deferred work onto a
    /// thread because the body of `foo` has not yet executed.
    MakeAsync(u16, u8),
    /// Pop a value, push the awaited inner value. For a Pending
    /// `Value::Async`, runs the body chunk synchronously (in a nested
    /// dispatch loop) and caches the result. For a Ready async, pushes the
    /// cached value. For a `Value::Handle` referencing a spawned task,
    /// joins the worker thread (blocks if not yet done) and pushes the
    /// resolved value. For any other type, raises `RuntimeError`.
    Await,

    // ---------------- M9: algebraic effects ----------------------------------
    /// Invoke an effect operation. Pops `arg_count` arguments + the
    /// (effect_tag, op_tag) pair from the stack; transfers control to the
    /// nearest enclosing handler clause for `(effect_tag, op_tag)`. The
    /// handler resumes by pushing a value and executing `Resume`.
    ///
    /// Encoded as a triple: `Perform(effect_tag, op_tag, arg_count)`.
    Perform(u16, u8, u8),
    /// Push a handler frame onto the handler stack.
    ///
    /// Operands:
    ///   - `table_idx`: index into `Program::handler_tables`. The indexed
    ///     table lists `(effect_tag, op_tag, clause_chunk_idx)` entries.
    ///   - `body_end_offset`: distance (in instructions) from the
    ///     instruction *after* this `PushHandler` to the instruction
    ///     immediately after the matching `PopHandler`. Used by
    ///     `Op::Perform` when a handler clause runs without ever
    ///     resuming: in that case the body's bytecode after the perform
    ///     site is skipped, and the body's containing frame jumps to
    ///     this position to flow the clause's return value back through
    ///     the enclosing computation.
    PushHandler(u16, u16),
    /// Pop a handler frame from the handler stack.
    PopHandler,
    /// Resume a captured continuation exactly once. Pops a value and a
    /// continuation handle from the stack; transfers control back to the
    /// suspended computation.
    Resume,
}
