//! # Ferric Standard Library
//!
//! Provides native functions and the NativeRegistry for runtime function lookup.

use std::collections::HashMap;
use ferric_common::{ShellOutput, Symbol};

// Re-export Value and RuntimeError from ferric_vm
// NOTE: This creates a circular dependency issue - we'll need to move Value here
// or define NativeFn with a generic Result for now. Let's use String for errors.

/// Type for native function implementations.
///
/// Native functions take a slice of values and return a value or an error message.
/// The VM will convert the error message into a proper RuntimeError with Span.
pub type NativeFn = fn(&[NativeValue]) -> Result<NativeValue, String>;

/// Simplified value type for native function interface.
///
/// This is a temporary type to avoid circular dependencies.
/// Native functions work with this type, and the VM converts between
/// this and the full Value type.
#[derive(Debug, Clone, PartialEq)]
pub enum NativeValue {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Unit,
    ShellOutput(ShellOutput),
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
///
/// # Arguments
/// * `s: Str` - The string to print
///
/// # Returns
/// * `Unit`
fn builtin_println(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let s = expect_str(&args[0])?;
    println!("{}", s);
    Ok(NativeValue::Unit)
}

/// Prints a string without a newline to stdout.
///
/// # Arguments
/// * `s: Str` - The string to print
///
/// # Returns
/// * `Unit`
fn builtin_print(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let s = expect_str(&args[0])?;
    print!("{}", s);
    Ok(NativeValue::Unit)
}

/// Converts an integer to its string representation.
///
/// # Arguments
/// * `n: Int` - The integer to convert
///
/// # Returns
/// * `Str` - The string representation of the integer
fn builtin_int_to_str(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let n = expect_int(&args[0])?;
    Ok(NativeValue::Str(n.to_string()))
}

/// Converts a float to its string representation.
///
/// # Arguments
/// * `f: Float` - The float to convert
///
/// # Returns
/// * `Str` - The string representation of the float
fn builtin_float_to_str(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let f = expect_float(&args[0])?;
    Ok(NativeValue::Str(f.to_string()))
}

/// Converts a boolean to its string representation.
///
/// # Arguments
/// * `b: Bool` - The boolean to convert
///
/// # Returns
/// * `Str` - The string representation of the boolean ("true" or "false")
fn builtin_bool_to_str(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let b = expect_bool(&args[0])?;
    Ok(NativeValue::Str(b.to_string()))
}

/// Converts an integer to a float.
///
/// # Arguments
/// * `n: Int` - The integer to convert
///
/// # Returns
/// * `Float` - The integer value as a float
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

// ============================================================================
// Standard Library Registration
// ============================================================================

/// Registers all standard library functions with the given registry.
///
/// This function should be called at startup to populate the native function
/// registry with all built-in functions.
///
/// # Arguments
/// * `registry` - The native function registry to populate
/// * `interner` - The string interner for creating function name symbols
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
        // Test that NativeRegistry::new() creates an empty registry
        let registry = NativeRegistry::new();
        let sym = make_sym(0);
        assert!(registry.get(sym).is_none());
    }

    #[test]
    fn test_native_registry_register() {
        // Test that register() adds a function
        let mut registry = NativeRegistry::new();
        let sym = make_sym(0);

        registry.register(sym, builtin_println);
        assert!(registry.get(sym).is_some());
    }

    #[test]
    fn test_native_registry_get() {
        // Test that get() retrieves a registered function
        let mut registry = NativeRegistry::new();
        let sym = make_sym(0);

        registry.register(sym, builtin_println);
        let func = registry.get(sym);
        assert!(func.is_some());
    }

    #[test]
    fn test_native_registry_get_unregistered() {
        // Test that get() returns None for an unregistered function
        let registry = NativeRegistry::new();
        let sym = make_sym(42);
        assert!(registry.get(sym).is_none());
    }

    #[test]
    fn test_register_stdlib() {
        // Test that register_stdlib() registers all M1 functions
        let mut registry = NativeRegistry::new();
        let mut interner = ferric_common::Interner::new();

        register_stdlib(&mut registry, &mut interner);

        // Check that all three functions are registered
        let println_sym = interner.intern("println");
        let print_sym = interner.intern("print");
        let int_to_str_sym = interner.intern("int_to_str");

        assert!(registry.get(println_sym).is_some());
        assert!(registry.get(print_sym).is_some());
        assert!(registry.get(int_to_str_sym).is_some());
    }

    #[test]
    fn test_builtin_println_valid() {
        // Test that builtin_println with valid string argument works
        let args = vec![NativeValue::Str("Hello, world!".to_string())];
        let result = builtin_println(&args);

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), NativeValue::Unit);
    }

    #[test]
    fn test_builtin_println_wrong_arg_count() {
        // Test that builtin_println with wrong arg count returns error
        let args = vec![];
        let result = builtin_println(&args);

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("expected 1 argument(s), got 0"));
    }

    #[test]
    fn test_builtin_println_wrong_arg_count_too_many() {
        // Test that builtin_println with too many args returns error
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
        // Test that builtin_println with wrong arg type returns error
        let args = vec![NativeValue::Int(42)];
        let result = builtin_println(&args);

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("expected string"));
    }

    #[test]
    fn test_builtin_print_valid() {
        // Test that builtin_print works without newline
        let args = vec![NativeValue::Str("Hello".to_string())];
        let result = builtin_print(&args);

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), NativeValue::Unit);
    }

    #[test]
    fn test_builtin_print_wrong_arg_count() {
        // Test that builtin_print with wrong arg count returns error
        let args = vec![];
        let result = builtin_print(&args);

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("expected 1 argument(s), got 0"));
    }

    #[test]
    fn test_builtin_print_wrong_arg_type() {
        // Test that builtin_print with wrong arg type returns error
        let args = vec![NativeValue::Bool(true)];
        let result = builtin_print(&args);

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("expected string"));
    }

    #[test]
    fn test_builtin_int_to_str() {
        // Test that builtin_int_to_str(42) returns "42"
        let args = vec![NativeValue::Int(42)];
        let result = builtin_int_to_str(&args);

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), NativeValue::Str("42".to_string()));
    }

    #[test]
    fn test_builtin_int_to_str_negative() {
        // Test int_to_str with negative number
        let args = vec![NativeValue::Int(-123)];
        let result = builtin_int_to_str(&args);

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), NativeValue::Str("-123".to_string()));
    }

    #[test]
    fn test_builtin_int_to_str_zero() {
        // Test int_to_str with zero
        let args = vec![NativeValue::Int(0)];
        let result = builtin_int_to_str(&args);

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), NativeValue::Str("0".to_string()));
    }

    #[test]
    fn test_builtin_int_to_str_wrong_arg_count() {
        // Test that int_to_str with wrong arg count returns error
        let args = vec![];
        let result = builtin_int_to_str(&args);

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("expected 1 argument(s), got 0"));
    }

    #[test]
    fn test_builtin_int_to_str_wrong_arg_type() {
        // Test that int_to_str with wrong arg type returns error
        let args = vec![NativeValue::Str("not an int".to_string())];
        let result = builtin_int_to_str(&args);

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("expected int"));
    }

    #[test]
    fn test_check_arg_count_correct() {
        // Test helper function with correct arg count
        let args = vec![NativeValue::Int(1), NativeValue::Int(2)];
        let result = check_arg_count(&args, 2);
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_arg_count_wrong() {
        // Test helper function with wrong arg count
        let args = vec![NativeValue::Int(1)];
        let result = check_arg_count(&args, 2);
        assert!(result.is_err());
    }

    #[test]
    fn test_expect_str_valid() {
        // Test expect_str with valid string
        let value = NativeValue::Str("test".to_string());
        let result = expect_str(&value);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "test");
    }

    #[test]
    fn test_expect_str_invalid() {
        // Test expect_str with invalid type
        let value = NativeValue::Int(42);
        let result = expect_str(&value);
        assert!(result.is_err());
    }

    #[test]
    fn test_expect_int_valid() {
        // Test expect_int with valid integer
        let value = NativeValue::Int(42);
        let result = expect_int(&value);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
    }

    #[test]
    fn test_expect_int_invalid() {
        // Test expect_int with invalid type
        let value = NativeValue::Str("not an int".to_string());
        let result = expect_int(&value);
        assert!(result.is_err());
    }
}
