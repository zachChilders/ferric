//! # Ferric VM
//!
//! Runtime for compiled Ferric bytecode. The public surface is the
//! [`Executor`] trait — [`BytecodeVM`] is the only implementation.
//!
//! The tree-walking interpreter that lived here through M1/M2 has been
//! replaced wholesale by [`BytecodeVM`] (see `bytecode.rs`). The public
//! signature (the trait, [`Value`], [`RuntimeError`]) is unchanged from the
//! TreeWalker era, which is the point of Rule 6.

use ferric_common::{Interner, Program, Span, Symbol};
use ferric_stdlib::NativeRegistry;

pub mod bytecode;

pub use bytecode::BytecodeVM;

// ============================================================================
// Public API
// ============================================================================

/// Executor trait for running Ferric programs.
///
/// Rule 6: Always use this trait; never depend on a specific implementation
/// directly. The trait's signature is stable across VM replacements.
pub trait Executor {
    /// Executes a program with the given native function registry.
    ///
    /// `interner` is accepted for API stability and is unused by
    /// [`BytecodeVM`] — string literals are baked into the chunk constants
    /// at compile time.
    fn run(
        &mut self,
        program: Program,
        natives: NativeRegistry,
        interner: &Interner,
    ) -> Result<Value, RuntimeError>;
}

/// Runtime value types.
///
/// Rule 7: Never construct `Value` directly outside this crate. Use the
/// constructor functions (`Value::new_int`, etc.).
///
/// INVARIANT: `Value` must remain `Send`. Do not add variants containing
/// `Rc`, `RefCell`, raw pointers, or other non-`Send` types. This is
/// required for the async upgrade path — see `ASYNC_COMPAT.md`.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Unit,
    /// Reference to a user-defined function by chunk index.
    Fn(u16),
    /// Reference to a native (stdlib) function by its interned name.
    NativeFn(Symbol),
    /// Captured output of a shell command.
    ShellOutput(ferric_common::ShellOutput),
    /// Struct value: fields in declaration order.
    Struct(Vec<Value>),
    /// Enum variant value: `(variant_index, payload_fields)`.
    Variant(u16, Vec<Value>),
    /// Tuple value: elements in declaration order.
    Tuple(Vec<Value>),
    /// Homogeneous array value.
    Array(Vec<Value>),
    /// User-defined closure: function chunk plus the values it captured at
    /// the moment it was constructed.
    Closure { fn_idx: u16, captures: Vec<Value> },
}

impl Value {
    /// Creates an integer value.
    pub fn new_int(n: i64) -> Self { Value::Int(n) }

    /// Creates a float value.
    pub fn new_float(f: f64) -> Self { Value::Float(f) }

    /// Creates a boolean value.
    pub fn new_bool(b: bool) -> Self { Value::Bool(b) }

    /// Creates a string value.
    pub fn new_str(s: String) -> Self { Value::Str(s) }

    /// Creates a unit value.
    pub fn new_unit() -> Self { Value::Unit }

    /// Creates a user-function value from a chunk index.
    pub fn new_fn(chunk_idx: u16) -> Self { Value::Fn(chunk_idx) }

    /// Creates a native-function value.
    pub fn new_native_fn(name: Symbol) -> Self { Value::NativeFn(name) }

    /// Creates a `ShellOutput` value.
    pub fn new_shell_output(stdout: String, exit_code: i32) -> Self {
        Value::ShellOutput(ferric_common::ShellOutput { stdout, exit_code })
    }

    /// Creates a struct value with the given fields (declaration order).
    pub fn new_struct(fields: Vec<Value>) -> Self { Value::Struct(fields) }

    /// Creates an enum variant value.
    pub fn new_variant(idx: u16, fields: Vec<Value>) -> Self {
        Value::Variant(idx, fields)
    }

    /// Creates a tuple value.
    pub fn new_tuple(elements: Vec<Value>) -> Self { Value::Tuple(elements) }

    /// Creates an array value.
    pub fn new_array(elements: Vec<Value>) -> Self { Value::Array(elements) }

    /// Creates a closure value referencing chunk `fn_idx` with `captures`
    /// pre-bound into the leading slots of the call frame.
    pub fn new_closure(fn_idx: u16, captures: Vec<Value>) -> Self {
        Value::Closure { fn_idx, captures }
    }
}

/// Runtime errors with source location information.
///
/// Rule 5: All errors must carry a Span. Bytecode errors use
/// `Span::new(0, 0)` as a dummy span — precise span tracking through
/// bytecode is a post-M3 improvement.
#[derive(Debug, Clone, PartialEq)]
pub enum RuntimeError {
    UndefinedVariable { name: Symbol, span: Span },
    UndefinedFunction { name: Symbol, span: Span },
    TypeMismatch { expected: String, found: String, span: Span },
    DivisionByZero { span: Span },
    StackOverflow { span: Span },
    StackUnderflow { span: Span },
    NativeFunctionError { message: String, span: Span },
    InvalidOperation { op: String, span: Span },
    NotCallable { span: Span },
    WrongArgumentCount { expected: usize, found: usize, span: Span },
    /// A require statement with `Error` mode failed.
    RequireError { span: Span, message: Option<String> },
    /// Array index outside `[0, len)`.
    IndexOutOfBounds { index: i64, len: usize, span: Span },
    /// Receiver of an indexing op was not an array.
    NotAnArray { found: String, span: Span },
}

// Compile-time assertion: `Value` must be `Send` so a future async runtime
// can carry runtime values across `.await` points. If this fails to compile,
// a new `Value` variant has introduced a non-`Send` type — fix the variant
// rather than weakening the bound. See `ASYNC_COMPAT.md`.
const _: fn() = || {
    fn check<T: Send>() {}
    check::<Value>();
};
