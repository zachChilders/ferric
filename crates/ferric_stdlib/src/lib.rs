//! # Ferric Standard Library
//!
//! Provides native functions and the NativeRegistry for runtime function lookup.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::rc::Rc;

use ferric_common::{Interner, ShellOutput, Symbol, TypeAnnotation};

// Public modules — each contributes a `register()` entry point.
pub mod bytes;
pub mod crypto;
pub mod encode;
pub mod env;
pub mod fmt;
pub mod http;
pub mod io;
pub mod json;
pub mod list;
pub mod log;
pub mod map;
pub mod math;
pub mod path;
pub mod proc_;
pub mod rand_;
pub mod re;
pub mod serve;
pub mod set;
pub mod sock;
pub mod sort;
pub mod str_;
pub mod sync;
pub mod time_;

// Re-export common repr types so other modules can `use crate::Foo`.
pub use http::{RequestBuilderRepr, ResponseRepr};
pub use json::JsonRepr;
pub use log::{LogLevelRepr, LoggerRepr};
pub use proc_::{CommandRepr, ProcOutputRepr, ProcessRepr};
pub use rand_::RngRepr;
pub use re::MatchRepr;
pub use serve::{RequestRepr as ServeRequestRepr, ResponseRepr as ServeResponseRepr, ServerRepr};
pub use sync::{MutexRepr, OnceRepr, ReceiverRepr, SenderRepr};
pub use time_::{DurationRepr, TimeRepr};

// ============================================================================
// MapKey — hashable + orderable subset of `NativeValue`.
// ============================================================================

/// Hashable + orderable subset of `NativeValue` allowed as map / set key.
/// Float keys are forbidden because of NaN.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MapKey {
    Int(i64),
    Str(String),
    Bool(bool),
}

impl MapKey {
    /// Lifts a `NativeValue` into a `MapKey`, rejecting unsupported variants.
    pub fn from_value(v: &NativeValue) -> Result<Self, String> {
        match v {
            NativeValue::Int(n) => Ok(MapKey::Int(*n)),
            NativeValue::Str(s) => Ok(MapKey::Str(s.clone())),
            NativeValue::Bool(b) => Ok(MapKey::Bool(*b)),
            NativeValue::Float(_) => {
                Err("map/set keys may not be Float (NaN forbids ordering)".to_string())
            }
            other => Err(format!(
                "map/set keys must be Int, Str, or Bool — got {other:?}"
            )),
        }
    }

    /// Lowers a `MapKey` back to a `NativeValue`.
    pub fn into_value(self) -> NativeValue {
        match self {
            MapKey::Int(n) => NativeValue::Int(n),
            MapKey::Str(s) => NativeValue::Str(s),
            MapKey::Bool(b) => NativeValue::Bool(b),
        }
    }

    /// Cheap clone-to-value, used when iterating keys without consuming.
    pub fn to_value(&self) -> NativeValue {
        self.clone().into_value()
    }
}

// ============================================================================
// NativeClosure — a thin wrapper around an Rc<dyn Fn> for higher-order natives.
// ============================================================================

/// Closure wrapper that can be invoked via [`invoke_closure`]. The VM does
/// not yet bridge real Ferric closures into this; for now the stdlib uses
/// `NativeClosure` only for unit testing higher-order functions and as a
/// placeholder until VM closure dispatch lands.
#[derive(Clone)]
pub struct NativeClosure {
    inner: Rc<NativeClosureFn>,
}

type NativeClosureFn = dyn Fn(&[NativeValue]) -> Result<NativeValue, String>;

impl NativeClosure {
    /// Wraps a Rust closure into a `NativeClosure`.
    pub fn new<F>(f: F) -> Self
    where
        F: Fn(&[NativeValue]) -> Result<NativeValue, String> + 'static,
    {
        Self { inner: Rc::new(f) }
    }

    /// Invokes the wrapped closure.
    pub fn call(&self, args: &[NativeValue]) -> Result<NativeValue, String> {
        (self.inner)(args)
    }
}

impl std::fmt::Debug for NativeClosure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<NativeClosure>")
    }
}

impl PartialEq for NativeClosure {
    /// Two closures are equal iff they point at the same `Rc`.
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

/// Invoke a closure-shaped `NativeValue` with the given arguments.
///
/// TODO(stdlib-closure-bridge): the VM should be able to push a real
/// Ferric closure into a `NativeClosure` so that higher-order natives
/// (`list::map`, `sort::with`, `serve::get`, etc.) can call user code.
/// Until that bridge exists, only `NativeValue::Closure` constructed
/// from Rust closures (via `NativeClosure::new`) works.
pub fn invoke_closure(c: &NativeValue, args: &[NativeValue]) -> Result<NativeValue, String> {
    match c {
        NativeValue::Closure(closure) => closure.call(args),
        other => Err(format!(
            "expected closure, got {other:?} — VM closure bridge is not yet wired"
        )),
    }
}

// ============================================================================
// NativeValue
// ============================================================================

/// Type for native function implementations.
///
/// Native functions take a slice of values and return a value or an error message.
/// The VM will convert the error message into a proper RuntimeError with Span.
pub type NativeFn = fn(&[NativeValue]) -> Result<NativeValue, String>;

/// Simplified value type for native function interface.
///
/// This is the boundary between the VM's full `Value` type and the stdlib.
#[derive(Clone)]
pub enum NativeValue {
    // -------- Basic scalars --------
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Unit,

    // -------- Existing extensions --------
    ShellOutput(ShellOutput),

    /// Historical name retained for compatibility.
    Array(Vec<NativeValue>),
    /// Dynamic list of values.
    List(Vec<NativeValue>),

    // -------- New variants for the stdlib build-out --------
    Bytes(Vec<u8>),
    BytesMut(Rc<RefCell<Vec<u8>>>),
    ListMut(Rc<RefCell<Vec<NativeValue>>>),
    Map(BTreeMap<MapKey, NativeValue>),
    MapMut(Rc<RefCell<BTreeMap<MapKey, NativeValue>>>),
    Set(BTreeSet<MapKey>),

    /// Algebraic Option, encoded directly as a `NativeValue` variant rather
    /// than as a tagged tuple. The inner `Box` matches the spec.
    Option(Option<Box<NativeValue>>),
    /// Algebraic Result, encoded directly as a `NativeValue` variant.
    Result(Box<Result<NativeValue, NativeValue>>),
    /// Two-element tuple. Multi-element tuples can be modelled as nested
    /// `Tuple2`s if/when needed.
    Tuple2(Box<NativeValue>, Box<NativeValue>),
    /// N-element tuple — used by some modules (e.g. `rand`) that return
    /// `(value, Rng)` pairs. Many call sites still prefer `Tuple2`.
    Tuple(Vec<NativeValue>),

    // -------- Domain-specific representations --------
    Json(Box<JsonRepr>),
    Time(TimeRepr),
    Duration(DurationRepr),
    Regex(Rc<regex::Regex>),
    Match(MatchRepr),
    Ordering(std::cmp::Ordering),
    Rng(Rc<RefCell<RngRepr>>),

    /// Higher-order callback wrapper (see [`NativeClosure`] and
    /// [`invoke_closure`]).
    Closure(NativeClosure),

    // -------- HTTP --------
    HttpResponse(Rc<ResponseRepr>),
    HttpRequestBuilder(Rc<RefCell<RequestBuilderRepr>>),

    // -------- HTTP server (`serve`) --------
    ServeServer(Rc<RefCell<ServerRepr>>),
    ServeRequest(Rc<ServeRequestRepr>),
    ServeResponse(Rc<RefCell<ServeResponseRepr>>),

    // -------- Sockets --------
    TcpStream(Rc<RefCell<std::net::TcpStream>>),
    TcpListener(Rc<std::net::TcpListener>),
    UdpSocket(Rc<std::net::UdpSocket>),

    // -------- File I/O --------
    FileWriter(Rc<RefCell<std::io::BufWriter<std::fs::File>>>),

    // -------- Process --------
    ProcOutput(Rc<ProcOutputRepr>),
    ProcCommand(Rc<RefCell<CommandRepr>>),
    ProcProcess(Rc<RefCell<ProcessRepr>>),

    // -------- Logging --------
    Logger(Rc<LoggerRepr>),
    LogLevel(LogLevelRepr),

    // -------- Sync --------
    SyncSender(Rc<SenderRepr>),
    SyncReceiver(Rc<ReceiverRepr>),
    SyncMutex(Rc<MutexRepr>),
    SyncOnce(Rc<OnceRepr>),
}

impl std::fmt::Debug for NativeValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NativeValue::Int(v) => f.debug_tuple("Int").field(v).finish(),
            NativeValue::Float(v) => f.debug_tuple("Float").field(v).finish(),
            NativeValue::Bool(v) => f.debug_tuple("Bool").field(v).finish(),
            NativeValue::Str(v) => f.debug_tuple("Str").field(v).finish(),
            NativeValue::Unit => f.write_str("Unit"),
            NativeValue::ShellOutput(v) => f.debug_tuple("ShellOutput").field(v).finish(),
            NativeValue::Array(v) => f.debug_tuple("Array").field(v).finish(),
            NativeValue::List(v) => f.debug_tuple("List").field(v).finish(),
            NativeValue::Bytes(v) => f.debug_tuple("Bytes").field(v).finish(),
            NativeValue::BytesMut(_) => f.write_str("BytesMut(<opaque>)"),
            NativeValue::ListMut(_) => f.write_str("ListMut(<opaque>)"),
            NativeValue::Map(v) => f.debug_tuple("Map").field(v).finish(),
            NativeValue::MapMut(_) => f.write_str("MapMut(<opaque>)"),
            NativeValue::Set(v) => f.debug_tuple("Set").field(v).finish(),
            NativeValue::Option(v) => f.debug_tuple("Option").field(v).finish(),
            NativeValue::Result(v) => f.debug_tuple("Result").field(v).finish(),
            NativeValue::Tuple2(a, b) => f.debug_tuple("Tuple2").field(a).field(b).finish(),
            NativeValue::Tuple(v) => f.debug_tuple("Tuple").field(v).finish(),
            NativeValue::Json(v) => f.debug_tuple("Json").field(v).finish(),
            NativeValue::Time(v) => f.debug_tuple("Time").field(v).finish(),
            NativeValue::Duration(v) => f.debug_tuple("Duration").field(v).finish(),
            NativeValue::Regex(_) => f.write_str("Regex(<opaque>)"),
            NativeValue::Match(v) => f.debug_tuple("Match").field(v).finish(),
            NativeValue::Ordering(v) => f.debug_tuple("Ordering").field(v).finish(),
            NativeValue::Rng(_) => f.write_str("Rng(<opaque>)"),
            NativeValue::Closure(_) => f.write_str("Closure(<opaque>)"),
            NativeValue::HttpResponse(v) => f.debug_tuple("HttpResponse").field(v).finish(),
            NativeValue::HttpRequestBuilder(_) => f.write_str("HttpRequestBuilder(<opaque>)"),
            NativeValue::ServeServer(_) => f.write_str("ServeServer(<opaque>)"),
            NativeValue::ServeRequest(_) => f.write_str("ServeRequest(<opaque>)"),
            NativeValue::ServeResponse(_) => f.write_str("ServeResponse(<opaque>)"),
            NativeValue::TcpStream(_) => f.write_str("TcpStream(<opaque>)"),
            NativeValue::TcpListener(_) => f.write_str("TcpListener(<opaque>)"),
            NativeValue::UdpSocket(_) => f.write_str("UdpSocket(<opaque>)"),
            NativeValue::FileWriter(_) => f.write_str("FileWriter(<opaque>)"),
            NativeValue::ProcOutput(_) => f.write_str("ProcOutput(<opaque>)"),
            NativeValue::ProcCommand(_) => f.write_str("ProcCommand(<opaque>)"),
            NativeValue::ProcProcess(_) => f.write_str("ProcProcess(<opaque>)"),
            NativeValue::Logger(v) => f.debug_tuple("Logger").field(v).finish(),
            NativeValue::LogLevel(v) => f.debug_tuple("LogLevel").field(v).finish(),
            NativeValue::SyncSender(_) => f.write_str("SyncSender(<opaque>)"),
            NativeValue::SyncReceiver(_) => f.write_str("SyncReceiver(<opaque>)"),
            NativeValue::SyncMutex(_) => f.write_str("SyncMutex(<opaque>)"),
            NativeValue::SyncOnce(_) => f.write_str("SyncOnce(<opaque>)"),
        }
    }
}

impl PartialEq for NativeValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (NativeValue::Int(a), NativeValue::Int(b)) => a == b,
            (NativeValue::Float(a), NativeValue::Float(b)) => a == b,
            (NativeValue::Bool(a), NativeValue::Bool(b)) => a == b,
            (NativeValue::Str(a), NativeValue::Str(b)) => a == b,
            (NativeValue::Unit, NativeValue::Unit) => true,
            (NativeValue::ShellOutput(a), NativeValue::ShellOutput(b)) => a == b,
            (NativeValue::Array(a), NativeValue::Array(b)) => a == b,
            (NativeValue::List(a), NativeValue::List(b)) => a == b,
            (NativeValue::Bytes(a), NativeValue::Bytes(b)) => a == b,
            (NativeValue::BytesMut(a), NativeValue::BytesMut(b)) => Rc::ptr_eq(a, b),
            (NativeValue::ListMut(a), NativeValue::ListMut(b)) => Rc::ptr_eq(a, b),
            (NativeValue::Map(a), NativeValue::Map(b)) => a == b,
            (NativeValue::MapMut(a), NativeValue::MapMut(b)) => Rc::ptr_eq(a, b),
            (NativeValue::Set(a), NativeValue::Set(b)) => a == b,
            (NativeValue::Option(a), NativeValue::Option(b)) => a == b,
            (NativeValue::Result(a), NativeValue::Result(b)) => a == b,
            (NativeValue::Tuple2(a1, b1), NativeValue::Tuple2(a2, b2)) => a1 == a2 && b1 == b2,
            (NativeValue::Tuple(a), NativeValue::Tuple(b)) => a == b,
            (NativeValue::Json(a), NativeValue::Json(b)) => a == b,
            (NativeValue::Time(a), NativeValue::Time(b)) => a == b,
            (NativeValue::Duration(a), NativeValue::Duration(b)) => a == b,
            (NativeValue::Regex(a), NativeValue::Regex(b)) => Rc::ptr_eq(a, b),
            (NativeValue::Match(a), NativeValue::Match(b)) => a == b,
            (NativeValue::Ordering(a), NativeValue::Ordering(b)) => a == b,
            (NativeValue::Rng(a), NativeValue::Rng(b)) => Rc::ptr_eq(a, b),
            (NativeValue::Closure(a), NativeValue::Closure(b)) => a == b,
            (NativeValue::HttpResponse(a), NativeValue::HttpResponse(b)) => Rc::ptr_eq(a, b),
            (NativeValue::HttpRequestBuilder(a), NativeValue::HttpRequestBuilder(b)) => {
                Rc::ptr_eq(a, b)
            }
            (NativeValue::ServeServer(a), NativeValue::ServeServer(b)) => Rc::ptr_eq(a, b),
            (NativeValue::ServeRequest(a), NativeValue::ServeRequest(b)) => Rc::ptr_eq(a, b),
            (NativeValue::ServeResponse(a), NativeValue::ServeResponse(b)) => Rc::ptr_eq(a, b),
            (NativeValue::TcpStream(a), NativeValue::TcpStream(b)) => Rc::ptr_eq(a, b),
            (NativeValue::TcpListener(a), NativeValue::TcpListener(b)) => Rc::ptr_eq(a, b),
            (NativeValue::UdpSocket(a), NativeValue::UdpSocket(b)) => Rc::ptr_eq(a, b),
            (NativeValue::FileWriter(a), NativeValue::FileWriter(b)) => Rc::ptr_eq(a, b),
            (NativeValue::ProcOutput(a), NativeValue::ProcOutput(b)) => Rc::ptr_eq(a, b),
            (NativeValue::ProcCommand(a), NativeValue::ProcCommand(b)) => Rc::ptr_eq(a, b),
            (NativeValue::ProcProcess(a), NativeValue::ProcProcess(b)) => Rc::ptr_eq(a, b),
            (NativeValue::Logger(a), NativeValue::Logger(b)) => Rc::ptr_eq(a, b),
            (NativeValue::LogLevel(a), NativeValue::LogLevel(b)) => a == b,
            (NativeValue::SyncSender(a), NativeValue::SyncSender(b)) => Rc::ptr_eq(a, b),
            (NativeValue::SyncReceiver(a), NativeValue::SyncReceiver(b)) => Rc::ptr_eq(a, b),
            (NativeValue::SyncMutex(a), NativeValue::SyncMutex(b)) => Rc::ptr_eq(a, b),
            (NativeValue::SyncOnce(a), NativeValue::SyncOnce(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

/// Registry of native functions available to the VM.
///
/// The VM queries this registry when calling functions by name.
/// If a function is found here, it's executed as a native function.
/// Otherwise, the VM looks for a user-defined function in the AST.
///
/// ASYNC UPGRADE PATH: When async/await is added (post-M3), the stored fn type
/// becomes:
///
/// ```ignore
/// Box<dyn Fn(&[NativeValue]) -> Pin<Box<dyn Future<Output = Result<NativeValue, String>> + Send>> + Send + Sync>
/// ```
///
/// This is a breaking change to `NativeRegistry`'s internal type, but the
/// public stage signature (a `NativeRegistry` passed into `Executor::run`)
/// does not change. All native function registrations will need updating at
/// that point. See `ferric_vm/ASYNC_COMPAT.md`.
#[derive(Clone)]
pub struct NativeRegistry {
    functions: HashMap<Symbol, NativeFn>,
}

impl NativeRegistry {
    /// Creates a new empty native function registry.
    pub fn new() -> Self {
        Self {
            functions: HashMap::new(),
        }
    }

    /// Registers a native function with the given name.
    pub fn register(&mut self, name: Symbol, f: NativeFn) {
        self.functions.insert(name, f);
    }

    /// Looks up a native function by name.
    pub fn get(&self, name: Symbol) -> Option<&NativeFn> {
        self.functions.get(&name)
    }
}

impl Default for NativeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Checks that the argument count matches the expected count.
fn check_arg_count(args: &[NativeValue], expected: usize) -> Result<(), String> {
    if args.len() != expected {
        Err(format!("expected {} argument(s), got {}", expected, args.len()))
    } else {
        Ok(())
    }
}

/// Extracts a string from a NativeValue or returns an error.
fn expect_str(value: &NativeValue) -> Result<&String, String> {
    match value {
        NativeValue::Str(s) => Ok(s),
        _ => Err(format!("expected string, got {value:?}")),
    }
}

/// Extracts an integer from a NativeValue or returns an error.
fn expect_int(value: &NativeValue) -> Result<i64, String> {
    match value {
        NativeValue::Int(n) => Ok(*n),
        _ => Err(format!("expected int, got {value:?}")),
    }
}

/// Extracts a float from a NativeValue or returns an error.
fn expect_float(value: &NativeValue) -> Result<f64, String> {
    match value {
        NativeValue::Float(f) => Ok(*f),
        _ => Err(format!("expected float, got {value:?}")),
    }
}

/// Extracts a boolean from a NativeValue or returns an error.
fn expect_bool(value: &NativeValue) -> Result<bool, String> {
    match value {
        NativeValue::Bool(b) => Ok(*b),
        _ => Err(format!("expected bool, got {value:?}")),
    }
}

// ============================================================================
// Built-in Functions
// ============================================================================

/// Prints a string followed by a newline to stdout.
fn builtin_println(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let s = expect_str(&args[0])?;
    println!("{s}");
    Ok(NativeValue::Unit)
}

/// Prints a string without a newline to stdout.
fn builtin_print(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let s = expect_str(&args[0])?;
    print!("{s}");
    Ok(NativeValue::Unit)
}

/// Converts an integer to its string representation.
fn builtin_int_to_str(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let n = expect_int(&args[0])?;
    Ok(NativeValue::Str(n.to_string()))
}

/// Converts a float to its string representation.
fn builtin_float_to_str(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let f = expect_float(&args[0])?;
    Ok(NativeValue::Str(f.to_string()))
}

/// Converts a boolean to its string representation.
fn builtin_bool_to_str(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let b = expect_bool(&args[0])?;
    Ok(NativeValue::Str(b.to_string()))
}

/// Converts an integer to a float.
fn builtin_int_to_float(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let n = expect_int(&args[0])?;
    Ok(NativeValue::Float(n as f64))
}

/// Returns the captured stdout of a `ShellOutput`.
fn builtin_shell_stdout(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    match &args[0] {
        NativeValue::ShellOutput(out) => Ok(NativeValue::Str(out.stdout.clone())),
        other => Err(format!("expected ShellOutput, got {other:?}")),
    }
}

/// Returns the exit code of a `ShellOutput` as an Int.
fn builtin_shell_exit_code(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    match &args[0] {
        NativeValue::ShellOutput(out) => Ok(NativeValue::Int(out.exit_code as i64)),
        other => Err(format!("expected ShellOutput, got {other:?}")),
    }
}

// `SHELL_EXEC_NATIVE` lives in `ferric_common` so the compiler and the
// stdlib (which cannot depend on each other) cannot drift on the name.
pub use ferric_common::SHELL_EXEC_NATIVE;

/// Runs a shell command synchronously and returns a `Value::ShellOutput`.
fn run_shell_command(cmd: &str) -> ShellOutput {
    #[cfg(any(unix, windows))]
    {
        let output = if cfg!(windows) {
            std::process::Command::new("cmd").arg("/C").arg(cmd).output()
        } else {
            std::process::Command::new("sh").arg("-c").arg(cmd).output()
        };
        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
                let exit_code = out.status.code().unwrap_or(-1);
                ShellOutput { stdout, exit_code }
            }
            Err(_) => ShellOutput { stdout: String::new(), exit_code: 126 },
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = cmd;
        ShellOutput { stdout: String::new(), exit_code: 126 }
    }
}

/// Runs a shell command and returns a `ShellOutput`.
fn builtin_shell_exec(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let cmd = expect_str(&args[0])?;
    Ok(NativeValue::ShellOutput(run_shell_command(cmd)))
}

// ---------------- M6: arrays / strings / math / io ----------------------

fn expect_array(value: &NativeValue) -> Result<&Vec<NativeValue>, String> {
    match value {
        NativeValue::List(elems) | NativeValue::Array(elems) => Ok(elems),
        other => Err(format!("expected array, got {other:?}")),
    }
}

fn builtin_array_len(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let arr = expect_array(&args[0])?;
    Ok(NativeValue::Int(arr.len() as i64))
}

fn builtin_str_len(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let s = expect_str(&args[0])?;
    Ok(NativeValue::Int(s.chars().count() as i64))
}

fn builtin_str_trim(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let s = expect_str(&args[0])?;
    Ok(NativeValue::Str(s.trim().to_string()))
}

fn builtin_str_contains(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let s = expect_str(&args[0])?;
    let sub = expect_str(&args[1])?;
    Ok(NativeValue::Bool(s.contains(sub.as_str())))
}

fn builtin_str_starts_with(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let s = expect_str(&args[0])?;
    let prefix = expect_str(&args[1])?;
    Ok(NativeValue::Bool(s.starts_with(prefix.as_str())))
}

fn builtin_str_parse_int(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let s = expect_str(&args[0])?;
    s.trim()
        .parse::<i64>()
        .map(NativeValue::Int)
        .map_err(|e| format!("parse_int: {e}"))
}

fn builtin_str_split(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let s = expect_str(&args[0])?;
    let sep = expect_str(&args[1])?;
    let parts: Vec<NativeValue> = if sep.is_empty() {
        s.chars()
            .map(|c| NativeValue::Str(c.to_string()))
            .collect()
    } else {
        s.split(sep.as_str())
            .map(|p| NativeValue::Str(p.to_string()))
            .collect()
    };
    Ok(NativeValue::List(parts))
}

fn builtin_abs(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let n = expect_int(&args[0])?;
    Ok(NativeValue::Int(n.abs()))
}

fn builtin_min(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let a = expect_int(&args[0])?;
    let b = expect_int(&args[1])?;
    Ok(NativeValue::Int(a.min(b)))
}

fn builtin_max(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let a = expect_int(&args[0])?;
    let b = expect_int(&args[1])?;
    Ok(NativeValue::Int(a.max(b)))
}

fn builtin_sqrt(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let n = expect_float(&args[0])?;
    Ok(NativeValue::Float(n.sqrt()))
}

fn builtin_pow(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let base = expect_float(&args[0])?;
    let exp = expect_float(&args[1])?;
    Ok(NativeValue::Float(base.powf(exp)))
}

fn builtin_floor(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let n = expect_float(&args[0])?;
    Ok(NativeValue::Int(n.floor() as i64))
}

fn builtin_ceil(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let n = expect_float(&args[0])?;
    Ok(NativeValue::Int(n.ceil() as i64))
}

fn builtin_read_line(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 0)?;
    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .map_err(|e| format!("read_line: {e}"))?;
    while line.ends_with('\n') || line.ends_with('\r') {
        line.pop();
    }
    Ok(NativeValue::Str(line))
}

// ============================================================================
// Built-in Enum Registration
// ============================================================================

/// Returns the table of compiler-provided built-in enums (currently `Option`
/// and `Result`) suitable for [`ferric_resolve::resolve_with_natives_and_builtins`].
#[allow(clippy::type_complexity)]
pub fn builtin_enum_table(
    interner: &mut Interner,
) -> Vec<(Symbol, Vec<(Symbol, Vec<TypeAnnotation>)>)> {
    let option_sym = interner.intern("Option");
    let some_sym = interner.intern("Some");
    let none_sym = interner.intern("None");
    let result_sym = interner.intern("Result");
    let ok_sym = interner.intern("Ok");
    let err_sym = interner.intern("Err");

    vec![
        (
            option_sym,
            vec![
                (some_sym, vec![TypeAnnotation::Infer]),
                (none_sym, vec![]),
            ],
        ),
        (
            result_sym,
            vec![
                (ok_sym, vec![TypeAnnotation::Infer]),
                (err_sym, vec![TypeAnnotation::Infer]),
            ],
        ),
    ]
}

// ============================================================================
// Standard Library Registration
// ============================================================================

/// Registers all standard library functions with the given registry.
///
/// Order matters in two ways:
///   1. The very first symbol interned is `println` so that the parser's
///      sentinel `Symbol::new(0)` (used for positional/anonymous args)
///      resolves stably across builds — many error-message fixtures depend
///      on this.
///   2. Legacy M1–M6 ABIs win on name collision with the new spec modules.
///      We register legacy first to claim Symbol(0), then re-register them
///      after the new modules to ensure they remain the active handlers.
pub fn register_stdlib(registry: &mut NativeRegistry, interner: &mut ferric_common::Interner) {
    // Pass 1: legacy registrations claim early symbols (and are the active
    // handlers until / unless overwritten below).
    register_legacy_builtins(registry, interner);

    // Pass 2: per-module registrations from the stdlib build-out.
    str_::register(registry, interner);
    bytes::register(registry, interner);
    list::register(registry, interner);
    sort::register(registry, interner);
    map::register(registry, interner);
    set::register(registry, interner);
    math::register(registry, interner);
    io::register(registry, interner);
    path::register(registry, interner);
    env::register(registry, interner);
    time_::register(registry, interner);
    rand_::register(registry, interner);
    re::register(registry, interner);
    json::register(registry, interner);
    fmt::register(registry, interner);
    encode::register(registry, interner);
    crypto::register(registry, interner);
    http::register(registry, interner);
    serve::register(registry, interner);
    sock::register(registry, interner);
    proc_::register(registry, interner);
    log::register(registry, interner);
    sync::register(registry, interner);

    // Pass 3: re-register legacy ABIs to win on collision.
    register_legacy_builtins(registry, interner);
}

fn register_legacy_builtins(registry: &mut NativeRegistry, interner: &mut ferric_common::Interner) {
    let println_sym = interner.intern("println");
    registry.register(println_sym, builtin_println);

    let print_sym = interner.intern("print");
    registry.register(print_sym, builtin_print);

    let int_to_str_sym = interner.intern("int_to_str");
    registry.register(int_to_str_sym, builtin_int_to_str);

    // M2 conversion functions
    let float_to_str_sym = interner.intern("float_to_str");
    registry.register(float_to_str_sym, builtin_float_to_str);

    let bool_to_str_sym = interner.intern("bool_to_str");
    registry.register(bool_to_str_sym, builtin_bool_to_str);

    let int_to_float_sym = interner.intern("int_to_float");
    registry.register(int_to_float_sym, builtin_int_to_float);

    // M2.5: shell output accessors
    let shell_stdout_sym = interner.intern("shell_stdout");
    registry.register(shell_stdout_sym, builtin_shell_stdout);

    let shell_exit_code_sym = interner.intern("shell_exit_code");
    registry.register(shell_exit_code_sym, builtin_shell_exit_code);

    // M3: compiler-internal shell runner.
    let shell_exec_sym = interner.intern(SHELL_EXEC_NATIVE);
    registry.register(shell_exec_sym, builtin_shell_exec);

    // M6: arrays / strings / math / io
    registry.register(interner.intern("array_len"),       builtin_array_len);
    registry.register(interner.intern("str_len"),         builtin_str_len);
    registry.register(interner.intern("str_trim"),        builtin_str_trim);
    registry.register(interner.intern("str_contains"),    builtin_str_contains);
    registry.register(interner.intern("str_starts_with"), builtin_str_starts_with);
    registry.register(interner.intern("str_parse_int"),   builtin_str_parse_int);
    registry.register(interner.intern("str_split"),       builtin_str_split);
    registry.register(interner.intern("abs"),             builtin_abs);
    registry.register(interner.intern("min"),             builtin_min);
    registry.register(interner.intern("max"),             builtin_max);
    registry.register(interner.intern("sqrt"),            builtin_sqrt);
    registry.register(interner.intern("pow"),             builtin_pow);
    registry.register(interner.intern("floor"),           builtin_floor);
    registry.register(interner.intern("ceil"),            builtin_ceil);
    registry.register(interner.intern("read_line"),       builtin_read_line);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_sym(n: u32) -> Symbol {
        Symbol::new(n)
    }

    #[test]
    fn test_native_registry_new() {
        let registry = NativeRegistry::new();
        let sym = make_sym(0);
        assert!(registry.get(sym).is_none());
    }

    #[test]
    fn test_native_registry_register() {
        let mut registry = NativeRegistry::new();
        let sym = make_sym(0);

        registry.register(sym, builtin_println);
        assert!(registry.get(sym).is_some());
    }

    #[test]
    fn test_native_registry_get() {
        let mut registry = NativeRegistry::new();
        let sym = make_sym(0);

        registry.register(sym, builtin_println);
        let func = registry.get(sym);
        assert!(func.is_some());
    }

    #[test]
    fn test_native_registry_get_unregistered() {
        let registry = NativeRegistry::new();
        let sym = make_sym(42);
        assert!(registry.get(sym).is_none());
    }

    #[test]
    fn test_register_stdlib() {
        let mut registry = NativeRegistry::new();
        let mut interner = ferric_common::Interner::new();

        register_stdlib(&mut registry, &mut interner);

        let println_sym = interner.intern("println");
        let print_sym = interner.intern("print");
        let int_to_str_sym = interner.intern("int_to_str");

        assert!(registry.get(println_sym).is_some());
        assert!(registry.get(print_sym).is_some());
        assert!(registry.get(int_to_str_sym).is_some());
    }

    #[test]
    fn test_builtin_println_valid() {
        let args = vec![NativeValue::Str("Hello, world!".to_string())];
        let result = builtin_println(&args);

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), NativeValue::Unit);
    }

    #[test]
    fn test_builtin_println_wrong_arg_count() {
        let args = vec![];
        let result = builtin_println(&args);

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("expected 1 argument(s), got 0"));
    }

    #[test]
    fn test_builtin_println_wrong_arg_count_too_many() {
        let args = vec![
            NativeValue::Str("Hello".to_string()),
            NativeValue::Str("World".to_string()),
        ];
        let result = builtin_println(&args);

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("expected 1 argument(s), got 2"));
    }

    #[test]
    fn test_builtin_println_wrong_arg_type() {
        let args = vec![NativeValue::Int(42)];
        let result = builtin_println(&args);

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("expected string"));
    }

    #[test]
    fn test_builtin_print_valid() {
        let args = vec![NativeValue::Str("Hello".to_string())];
        let result = builtin_print(&args);

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), NativeValue::Unit);
    }

    #[test]
    fn test_builtin_print_wrong_arg_count() {
        let args = vec![];
        let result = builtin_print(&args);

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("expected 1 argument(s), got 0"));
    }

    #[test]
    fn test_builtin_print_wrong_arg_type() {
        let args = vec![NativeValue::Bool(true)];
        let result = builtin_print(&args);

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("expected string"));
    }

    #[test]
    fn test_builtin_int_to_str() {
        let args = vec![NativeValue::Int(42)];
        let result = builtin_int_to_str(&args);

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), NativeValue::Str("42".to_string()));
    }

    #[test]
    fn test_builtin_int_to_str_negative() {
        let args = vec![NativeValue::Int(-123)];
        let result = builtin_int_to_str(&args);

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), NativeValue::Str("-123".to_string()));
    }

    #[test]
    fn test_builtin_int_to_str_zero() {
        let args = vec![NativeValue::Int(0)];
        let result = builtin_int_to_str(&args);

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), NativeValue::Str("0".to_string()));
    }

    #[test]
    fn test_builtin_int_to_str_wrong_arg_count() {
        let args = vec![];
        let result = builtin_int_to_str(&args);

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("expected 1 argument(s), got 0"));
    }

    #[test]
    fn test_builtin_int_to_str_wrong_arg_type() {
        let args = vec![NativeValue::Str("not an int".to_string())];
        let result = builtin_int_to_str(&args);

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("expected int"));
    }

    #[test]
    fn test_check_arg_count_correct() {
        let args = vec![NativeValue::Int(1), NativeValue::Int(2)];
        let result = check_arg_count(&args, 2);
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_arg_count_wrong() {
        let args = vec![NativeValue::Int(1)];
        let result = check_arg_count(&args, 2);
        assert!(result.is_err());
    }

    #[test]
    fn test_expect_str_valid() {
        let value = NativeValue::Str("test".to_string());
        let result = expect_str(&value);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "test");
    }

    #[test]
    fn test_expect_str_invalid() {
        let value = NativeValue::Int(42);
        let result = expect_str(&value);
        assert!(result.is_err());
    }

    #[test]
    fn test_expect_int_valid() {
        let value = NativeValue::Int(42);
        let result = expect_int(&value);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
    }

    #[test]
    fn test_expect_int_invalid() {
        let value = NativeValue::Str("not an int".to_string());
        let result = expect_int(&value);
        assert!(result.is_err());
    }

    #[test]
    fn test_native_closure_round_trip() {
        let c = NativeClosure::new(|args: &[NativeValue]| -> Result<NativeValue, String> {
            match &args[0] {
                NativeValue::Int(n) => Ok(NativeValue::Int(n * 2)),
                _ => Err("expected int".into()),
            }
        });
        let v = NativeValue::Closure(c);
        let r = invoke_closure(&v, &[NativeValue::Int(7)]).unwrap();
        assert_eq!(r, NativeValue::Int(14));
    }

    #[test]
    fn test_map_key_from_int() {
        let k = MapKey::from_value(&NativeValue::Int(3)).unwrap();
        assert_eq!(k, MapKey::Int(3));
    }

    #[test]
    fn test_map_key_rejects_float() {
        let r = MapKey::from_value(&NativeValue::Float(1.5));
        assert!(r.is_err());
    }
}
