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
    /// `Async<T>` — a deferred or resolved async computation. Wraps shared
    /// mutable state so the same `Async` can be observed from multiple
    /// places (the original creator, a `spawn`'s thread, awaiters). The
    /// state machine is `Pending → Ready` for in-process awaits and
    /// `Pending → Running → Ready` for spawned tasks.
    ///
    /// Lazy semantics (M8 Task 5 Option A): the body of an `async { ... }`
    /// or `async fn` does not execute until the value is awaited or
    /// spawned. This is what makes true parallelism possible —
    /// `spawn(task: foo(x))` can move the deferred work onto an OS thread.
    Async(AsyncCell),
    /// `Handle<T>` — opaque reference into the scheduler's `handles` table.
    /// `Op::Await` on a `Handle` consults the scheduler to retrieve the
    /// resolved value (joining a worker thread first if needed).
    Handle(u64),
}

/// Wrapper around the shared, lock-protected state of a `Value::Async`.
/// Newtype exists so `Value` can keep its `PartialEq` derive — equality on
/// `AsyncCell` is *identity* (two cells are equal iff they are the same
/// `Arc`), which is the only meaningful notion for live async values.
#[derive(Debug, Clone)]
pub struct AsyncCell(pub(crate) std::sync::Arc<std::sync::Mutex<AsyncState>>);

impl AsyncCell {
    pub(crate) fn new(state: AsyncState) -> Self {
        AsyncCell(std::sync::Arc::new(std::sync::Mutex::new(state)))
    }

    pub(crate) fn lock(&self) -> std::sync::MutexGuard<'_, AsyncState> {
        self.0.lock().expect("async state poisoned")
    }
}

impl PartialEq for AsyncCell {
    fn eq(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.0, &other.0)
    }
}

/// Internal state machine of a `Value::Async`. Lives inside an
/// [`AsyncCell`] (an `Arc<Mutex<...>>`) so the same async can be observed
/// across threads.
#[derive(Debug)]
pub enum AsyncState {
    /// Body has not yet executed. `chunk_idx` and `captures` together
    /// describe a deferred call: pushing `captures` onto the stack and
    /// entering `chunk_idx` runs the body.
    Pending {
        chunk_idx: u16,
        captures: Vec<Value>,
    },
    /// Body has run to completion and the result is cached.
    Ready(Value),
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

    /// Creates an `Async<T>` value already in `Ready` state. Used by
    /// runtime-only constructors that materialise a value synchronously
    /// (e.g. `sleep`). Rule 7: outside `ferric_vm`, this is the only way
    /// to construct `Value::Async`.
    pub fn new_async_ready(inner: Value) -> Self {
        Value::Async(AsyncCell::new(AsyncState::Ready(inner)))
    }

    /// Creates a `Pending` `Async<T>` capturing a deferred call: when this
    /// value is awaited or spawned, the VM will push `captures` onto the
    /// stack and enter `chunk_idx`. The body has not yet executed.
    pub fn new_async_pending(chunk_idx: u16, captures: Vec<Value>) -> Self {
        Value::Async(AsyncCell::new(AsyncState::Pending {
            chunk_idx,
            captures,
        }))
    }

    /// Creates a `Handle<T>` value referencing scheduler task `id`. Rule 7:
    /// outside `ferric_vm`, this is the only way to construct `Value::Handle`.
    pub fn new_handle(id: u64) -> Self {
        Value::Handle(id)
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
    /// Integer arithmetic produced a value outside `i64`'s representable range.
    IntegerOverflow { op: &'static str, span: Span },
    /// `Op::Await` reached a `Handle` whose task never resolved — the
    /// scheduler made a full pass with no forward progress.
    AsyncDeadlock { span: Span },
}

// Compile-time assertion: `Value` must be `Send` so a future async runtime
// can carry runtime values across `.await` points. If this fails to compile,
// a new `Value` variant has introduced a non-`Send` type — fix the variant
// rather than weakening the bound. See `ASYNC_COMPAT.md`.
const _: fn() = || {
    fn check<T: Send>() {}
    check::<Value>();
};
