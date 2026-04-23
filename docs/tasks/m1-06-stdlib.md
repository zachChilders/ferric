# Task: M1 Standard Library Implementation

## Objective
Implement the `ferric_stdlib` crate that provides native functions for I/O and type conversions. Registers these functions in a `NativeRegistry` for VM execution.

## Architecture Context
- The stdlib provides built-in functions implemented in Rust
- Functions are registered in `NativeRegistry` and called by the VM
- This crate is independent of all pipeline stages
- It only depends on `ferric_common` for types like `Symbol` and `ferric_vm` for `Value`

## Public Interface (Non-Negotiable)

```rust
// ferric_stdlib/src/lib.rs

pub struct NativeRegistry {
    functions: HashMap<Symbol, NativeFn>,
}

pub type NativeFn = fn(&[Value]) -> Result<Value, String>;

impl NativeRegistry {
    pub fn new() -> Self;
    pub fn register(&mut self, name: Symbol, f: NativeFn);
    pub fn get(&self, name: Symbol) -> Option<&NativeFn>;
}

pub fn register_stdlib(registry: &mut NativeRegistry, interner: &mut Interner);
```

## Feature Requirements

### M1 Built-in Functions

Implement exactly these functions for M1:

1. **`println(s: Str)`**
   - Print a string followed by a newline to stdout
   - Returns `Unit`

2. **`print(s: Str)`**
   - Print a string without a newline to stdout
   - Returns `Unit`

3. **`int_to_str(n: Int) -> Str`**
   - Convert an integer to its string representation
   - Returns a `Value::Str`

### Implementation Strategy

Each native function:
- Takes `&[Value]` as arguments
- Checks argument count and types
- Returns `Result<Value, String>` (error message on failure)
- Uses `Value::new_*()` constructors (Rule 7)

Example implementation:
```rust
fn builtin_println(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err(format!("println expects 1 argument, got {}", args.len()));
    }

    match &args[0] {
        Value::Str(s) => {
            println!("{}", s);
            Ok(Value::new_unit())
        }
        _ => Err(format!("println expects a string, got {:?}", args[0])),
    }
}
```

### Registration

The `register_stdlib` function interns all function names and registers them:

```rust
pub fn register_stdlib(registry: &mut NativeRegistry, interner: &mut Interner) {
    let println_sym = interner.intern("println");
    registry.register(println_sym, builtin_println);

    let print_sym = interner.intern("print");
    registry.register(print_sym, builtin_print);

    let int_to_str_sym = interner.intern("int_to_str");
    registry.register(int_to_str_sym, builtin_int_to_str);
}
```

## Implementation Notes

### NativeRegistry Structure
```rust
pub struct NativeRegistry {
    functions: HashMap<Symbol, NativeFn>,
}

impl NativeRegistry {
    pub fn new() -> Self {
        Self {
            functions: HashMap::new(),
        }
    }

    pub fn register(&mut self, name: Symbol, f: NativeFn) {
        self.functions.insert(name, f);
    }

    pub fn get(&self, name: Symbol) -> Option<&NativeFn> {
        self.functions.get(&name)
    }
}
```

### Argument Validation Helper
```rust
fn check_arg_count(args: &[Value], expected: usize) -> Result<(), String> {
    if args.len() != expected {
        Err(format!("expected {} argument(s), got {}", expected, args.len()))
    } else {
        Ok(())
    }
}

fn expect_str(value: &Value) -> Result<&String, String> {
    match value {
        Value::Str(s) => Ok(s),
        _ => Err(format!("expected string, got {:?}", value)),
    }
}

fn expect_int(value: &Value) -> Result<i64, String> {
    match value {
        Value::Int(n) => Ok(*n),
        _ => Err(format!("expected int, got {:?}", value)),
    }
}
```

### Type Conversion Functions
```rust
fn builtin_int_to_str(args: &[Value]) -> Result<Value, String> {
    check_arg_count(args, 1)?;
    let n = expect_int(&args[0])?;
    Ok(Value::new_str(n.to_string()))
}
```

## Test Cases

Create unit tests for:
1. `NativeRegistry::new()` creates empty registry
2. `register()` adds a function
3. `get()` retrieves a registered function
4. `get()` returns None for unregistered function
5. `register_stdlib()` registers all M1 functions
6. `builtin_println` with valid string argument works
7. `builtin_println` with wrong arg count returns error
8. `builtin_println` with wrong arg type returns error
9. `builtin_int_to_str(42)` returns "42"
10. `builtin_print` works without newline

## Acceptance Criteria
- [ ] `NativeRegistry` implemented with register and get methods
- [ ] All M1 built-in functions implemented correctly
- [ ] `register_stdlib()` function registers all built-ins
- [ ] All functions validate argument count and types
- [ ] All functions use `Value::new_*()` constructors (Rule 7)
- [ ] All functions return descriptive error messages on failure
- [ ] All unit tests pass
- [ ] Only `NativeRegistry`, `NativeFn`, and `register_stdlib` are public

## Critical Rules to Enforce

### Rule 7 - Value is never constructed directly outside ferric_vm
Native functions must use `Value::new_int()`, `Value::new_str()`, etc.
Never use `Value::Int()` or `Value::Str()` directly.

### Rule 4 - No mutable global state
No global registry. It's passed in explicitly.

## Notes for Agent
- Keep function implementations simple and robust
- Validate all inputs - native functions can crash the interpreter
- Use clear error messages - they'll be shown to users
- Remember that Value construction must use the `new_*()` methods
- Make sure `register_stdlib()` is called from main.rs at startup
- Test each function in isolation before integration
