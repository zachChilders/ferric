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
                "map/set keys must be Int, Str, or Bool — got {:?}",
                other
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
    inner: Rc<dyn Fn(&[NativeValue]) -> Result<NativeValue, String>>,
}

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
            "expected closure, got {:?} — VM closure bridge is not yet wired",
            other
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
#[derive(Debug, Clone, PartialEq)]
pub enum NativeValue {
    // -------- Basic scalars --------
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Unit,

    // -------- Existing extensions --------
    ShellOutput(ShellOutput),

    /// Dynamic list of values. Was historically named `Array`; renamed to
    /// `List` to match the stdlib spec terminology. Callers may still use
    /// the `Array` variant name via the deprecated alias below.
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

#[allow(non_upper_case_globals)]
impl NativeValue {
    /// Backwards-compat constructor for callers that still spell the old
    /// `Array` variant. Prefer `NativeValue::List(...)` directly.
    #[allow(non_snake_case)]
    pub fn Array(elems: Vec<NativeValue>) -> NativeValue {
        NativeValue::List(elems)
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
        _ => Err(format!("expected string, got {:?}", value)),
    }
}

/// Extracts an integer from a NativeValue or returns an error.
fn expect_int(value: &NativeValue) -> Result<i64, String> {
    match value {
        NativeValue::Int(n) => Ok(*n),
        _ => Err(format!("expected int, got {:?}", value)),
    }
}

/// Extracts a float from a NativeValue or returns an error.
fn expect_float(value: &NativeValue) -> Result<f64, String> {
    match value {
        NativeValue::Float(f) => Ok(*f),
        _ => Err(format!("expected float, got {:?}", value)),
    }
}

/// Extracts a boolean from a NativeValue or returns an error.
fn expect_bool(value: &NativeValue) -> Result<bool, String> {
    match value {
        NativeValue::Bool(b) => Ok(*b),
        _ => Err(format!("expected bool, got {:?}", value)),
    }
}

// ============================================================================
// Built-in Functions
// ============================================================================

/// Prints a string followed by a newline to stdout.
fn builtin_println(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let s = expect_str(&args[0])?;
    println!("{}", s);
    Ok(NativeValue::Unit)
}

/// Prints a string without a newline to stdout.
fn builtin_print(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let s = expect_str(&args[0])?;
    print!("{}", s);
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
        other => Err(format!("expected ShellOutput, got {:?}", other)),
    }
}

/// Returns the exit code of a `ShellOutput` as an Int.
fn builtin_shell_exit_code(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    match &args[0] {
        NativeValue::ShellOutput(out) => Ok(NativeValue::Int(out.exit_code as i64)),
        other => Err(format!("expected ShellOutput, got {:?}", other)),
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
        NativeValue::List(elems) => Ok(elems),
        other => Err(format!("expected array, got {:?}", other)),
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
        .map_err(|e| format!("parse_int: {}", e))
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
        .map_err(|e| format!("read_line: {}", e))?;
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
pub fn register_stdlib(registry: &mut NativeRegistry, interner: &mut ferric_common::Interner) {
    // M1 functions
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

    // Per-module registrations from the stdlib build-out.
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
