//! Bytecode VM. Implements [`crate::Executor`].
//!
//! Classic fetch–decode–execute loop over `Program.chunks`. Each call frame
//! owns its own `slots` vector (local variables indexed by `u8`). The value
//! stack is shared across frames; a caller pushes args then a callable, and
//! `Op::Call(argc)` pops them into a new frame.
//!
//! ## Frame management
//!
//! `CallFrame` lives on a heap-allocated `Vec<CallFrame>` rather than the
//! Rust call stack. This is deliberate — it lets a future async runtime
//! suspend/resume Ferric-level function calls without fighting the borrow
//! checker or blocking the executor. See `ASYNC_COMPAT.md`.

use ferric_common::{Chunk, Constant, Interner, Op, Program, Span};
use ferric_stdlib::{NativeRegistry, NativeValue};

use crate::{Executor, RuntimeError, Value};

/// Call frame for a single Ferric function invocation.
struct CallFrame {
    /// Index into `Program.chunks` of the code we're executing.
    chunk_idx: u16,
    /// Instruction pointer into `chunks[chunk_idx].code`.
    ip: usize,
    /// Local slots, indexed by the `u8` operand of `LoadSlot`/`StoreSlot`.
    /// Grown on demand by `StoreSlot` (see the opcode handler).
    slots: Vec<Value>,
}

/// Bytecode interpreter.
pub struct BytecodeVM {
    stack: Vec<Value>,
    call_stack: Vec<CallFrame>,
    natives: NativeRegistry,
}

impl BytecodeVM {
    /// Creates a new bytecode VM. The VM is reusable across calls to
    /// [`Executor::run`]; state is cleared at the start of each run.
    pub fn new() -> Self {
        Self {
            stack: Vec::new(),
            call_stack: Vec::new(),
            natives: NativeRegistry::new(),
        }
    }
}

impl Default for BytecodeVM {
    fn default() -> Self {
        Self::new()
    }
}

impl Executor for BytecodeVM {
    fn run(
        &mut self,
        program: Program,
        natives: NativeRegistry,
        _interner: &Interner,
    ) -> Result<Value, RuntimeError> {
        self.stack.clear();
        self.call_stack.clear();
        self.natives = natives;

        // Entry frame for the top-level script chunk.
        self.call_stack.push(CallFrame {
            chunk_idx: program.entry,
            ip: 0,
            slots: Vec::new(),
        });

        let chunks = program.chunks;

        // Main interpreter loop. Each iteration fetches one instruction from
        // the current frame, advances `ip`, and dispatches.
        loop {
            if self.call_stack.is_empty() {
                break;
            }

            let (chunk_idx, ip) = {
                let frame = self.call_stack.last().unwrap();
                (frame.chunk_idx, frame.ip)
            };
            let chunk: &Chunk = &chunks[chunk_idx as usize];

            // Implicit return if we fall off the end of a chunk.
            if ip >= chunk.code.len() {
                self.call_stack.pop();
                continue;
            }

            let op = chunk.code[ip];
            self.call_stack.last_mut().unwrap().ip = ip + 1;

            match op {
                // ---------------- Stack / slots --------------------------
                Op::LoadConst(idx) => {
                    let v = constant_to_value(&chunk.constants[idx as usize]);
                    self.stack.push(v);
                }
                Op::LoadSlot(slot) => {
                    let frame = self.call_stack.last().unwrap();
                    let v = frame
                        .slots
                        .get(slot as usize)
                        .cloned()
                        .unwrap_or(Value::Unit);
                    self.stack.push(v);
                }
                Op::StoreSlot(slot) => {
                    let v = self.pop()?;
                    let frame = self.call_stack.last_mut().unwrap();
                    if (slot as usize) >= frame.slots.len() {
                        frame.slots.resize(slot as usize + 1, Value::Unit);
                    }
                    frame.slots[slot as usize] = v;
                }
                Op::Pop => {
                    self.pop()?;
                }
                Op::Dup => {
                    let top = self.stack.last().cloned().ok_or_else(underflow)?;
                    self.stack.push(top);
                }

                // ---------------- Integer arithmetic ---------------------
                Op::AddInt => {
                    let b = self.pop_int()?;
                    let a = self.pop_int()?;
                    self.stack.push(Value::new_int(a.wrapping_add(b)));
                }
                Op::SubInt => {
                    let b = self.pop_int()?;
                    let a = self.pop_int()?;
                    self.stack.push(Value::new_int(a.wrapping_sub(b)));
                }
                Op::MulInt => {
                    let b = self.pop_int()?;
                    let a = self.pop_int()?;
                    self.stack.push(Value::new_int(a.wrapping_mul(b)));
                }
                Op::DivInt => {
                    let b = self.pop_int()?;
                    let a = self.pop_int()?;
                    if b == 0 {
                        return Err(RuntimeError::DivisionByZero { span: dummy_span() });
                    }
                    self.stack.push(Value::new_int(a / b));
                }
                Op::RemInt => {
                    let b = self.pop_int()?;
                    let a = self.pop_int()?;
                    if b == 0 {
                        return Err(RuntimeError::DivisionByZero { span: dummy_span() });
                    }
                    self.stack.push(Value::new_int(a % b));
                }
                Op::NegInt => {
                    let a = self.pop_int()?;
                    self.stack.push(Value::new_int(a.wrapping_neg()));
                }

                // ---------------- Float arithmetic -----------------------
                Op::AddFloat => {
                    let b = self.pop_float()?;
                    let a = self.pop_float()?;
                    self.stack.push(Value::new_float(a + b));
                }
                Op::SubFloat => {
                    let b = self.pop_float()?;
                    let a = self.pop_float()?;
                    self.stack.push(Value::new_float(a - b));
                }
                Op::MulFloat => {
                    let b = self.pop_float()?;
                    let a = self.pop_float()?;
                    self.stack.push(Value::new_float(a * b));
                }
                Op::DivFloat => {
                    let b = self.pop_float()?;
                    let a = self.pop_float()?;
                    if b == 0.0 {
                        return Err(RuntimeError::DivisionByZero { span: dummy_span() });
                    }
                    self.stack.push(Value::new_float(a / b));
                }
                Op::NegFloat => {
                    let a = self.pop_float()?;
                    self.stack.push(Value::new_float(-a));
                }

                // ---------------- Comparisons ----------------------------
                Op::EqInt => { let b = self.pop_int()?;   let a = self.pop_int()?;   self.stack.push(Value::new_bool(a == b)); }
                Op::NeInt => { let b = self.pop_int()?;   let a = self.pop_int()?;   self.stack.push(Value::new_bool(a != b)); }
                Op::LtInt => { let b = self.pop_int()?;   let a = self.pop_int()?;   self.stack.push(Value::new_bool(a <  b)); }
                Op::GtInt => { let b = self.pop_int()?;   let a = self.pop_int()?;   self.stack.push(Value::new_bool(a >  b)); }
                Op::LeInt => { let b = self.pop_int()?;   let a = self.pop_int()?;   self.stack.push(Value::new_bool(a <= b)); }
                Op::GeInt => { let b = self.pop_int()?;   let a = self.pop_int()?;   self.stack.push(Value::new_bool(a >= b)); }
                Op::EqFloat => { let b = self.pop_float()?; let a = self.pop_float()?; self.stack.push(Value::new_bool(a == b)); }
                Op::NeFloat => { let b = self.pop_float()?; let a = self.pop_float()?; self.stack.push(Value::new_bool(a != b)); }
                Op::LtFloat => { let b = self.pop_float()?; let a = self.pop_float()?; self.stack.push(Value::new_bool(a <  b)); }
                Op::GtFloat => { let b = self.pop_float()?; let a = self.pop_float()?; self.stack.push(Value::new_bool(a >  b)); }
                Op::LeFloat => { let b = self.pop_float()?; let a = self.pop_float()?; self.stack.push(Value::new_bool(a <= b)); }
                Op::GeFloat => { let b = self.pop_float()?; let a = self.pop_float()?; self.stack.push(Value::new_bool(a >= b)); }
                Op::EqBool => { let b = self.pop_bool()?; let a = self.pop_bool()?; self.stack.push(Value::new_bool(a == b)); }
                Op::NeBool => { let b = self.pop_bool()?; let a = self.pop_bool()?; self.stack.push(Value::new_bool(a != b)); }
                Op::EqStr  => { let b = self.pop_str()?;  let a = self.pop_str()?;  self.stack.push(Value::new_bool(a == b)); }
                Op::NeStr  => { let b = self.pop_str()?;  let a = self.pop_str()?;  self.stack.push(Value::new_bool(a != b)); }

                // ---------------- Boolean logic --------------------------
                Op::Not => {
                    let a = self.pop_bool()?;
                    self.stack.push(Value::new_bool(!a));
                }
                Op::AndBool => {
                    let b = self.pop_bool()?;
                    let a = self.pop_bool()?;
                    self.stack.push(Value::new_bool(a && b));
                }
                Op::OrBool => {
                    let b = self.pop_bool()?;
                    let a = self.pop_bool()?;
                    self.stack.push(Value::new_bool(a || b));
                }

                // ---------------- Strings --------------------------------
                Op::Concat => {
                    let b = self.pop_str()?;
                    let a = self.pop_str()?;
                    let mut out = String::with_capacity(a.len() + b.len());
                    out.push_str(&a);
                    out.push_str(&b);
                    self.stack.push(Value::new_str(out));
                }

                // ---------------- Control flow ---------------------------
                // Jump offsets are relative to the instruction immediately
                // after the jump — i.e. the `ip` we already advanced to.
                Op::Jump(off) => {
                    let frame = self.call_stack.last_mut().unwrap();
                    frame.ip = apply_offset(frame.ip, off);
                }
                Op::JumpIfFalse(off) => {
                    let cond = self.pop_bool()?;
                    if !cond {
                        let frame = self.call_stack.last_mut().unwrap();
                        frame.ip = apply_offset(frame.ip, off);
                    }
                }
                Op::JumpIfTrue(off) => {
                    let cond = self.pop_bool()?;
                    if cond {
                        let frame = self.call_stack.last_mut().unwrap();
                        frame.ip = apply_offset(frame.ip, off);
                    }
                }
                Op::Return => {
                    self.call_stack.pop();
                }

                // ---------------- Calls ----------------------------------
                Op::Call(argc) => {
                    let callee = self.pop()?;
                    let argc = argc as usize;
                    if self.stack.len() < argc {
                        return Err(underflow());
                    }
                    let args_start = self.stack.len() - argc;
                    let args: Vec<Value> = self.stack.drain(args_start..).collect();

                    match callee {
                        Value::Fn(chunk_idx) => {
                            self.call_stack.push(CallFrame {
                                chunk_idx,
                                ip: 0,
                                slots: args,
                            });
                        }
                        Value::NativeFn(sym) => {
                            let native = self.natives.get(sym).copied().ok_or_else(|| {
                                RuntimeError::UndefinedFunction {
                                    name: sym,
                                    span: dummy_span(),
                                }
                            })?;
                            let native_args: Vec<NativeValue> =
                                args.iter().map(value_to_native).collect();
                            match native(&native_args) {
                                Ok(result) => self.stack.push(native_to_value(result)),
                                Err(message) => {
                                    return Err(RuntimeError::NativeFunctionError {
                                        message,
                                        span: dummy_span(),
                                    });
                                }
                            }
                        }
                        _ => {
                            return Err(RuntimeError::NotCallable { span: dummy_span() });
                        }
                    }
                }

                // ---------------- Misc -----------------------------------
                Op::Unit => {
                    self.stack.push(Value::new_unit());
                }
                Op::RequireFail => {
                    let msg = self.pop_str()?;
                    let message = if msg.is_empty() { None } else { Some(msg) };
                    return Err(RuntimeError::RequireError {
                        span: dummy_span(),
                        message,
                    });
                }
                Op::RequireWarn => {
                    let msg = self.pop_str()?;
                    let display = if msg.is_empty() {
                        "require condition evaluated to false".to_string()
                    } else {
                        msg
                    };
                    eprintln!("warning: require failed: {}", display);
                }

                // ---------------- M4: structs / enums / tuples ------------
                Op::MakeStruct(n) => {
                    let n = n as usize;
                    if self.stack.len() < n {
                        return Err(underflow());
                    }
                    let start = self.stack.len() - n;
                    let fields: Vec<Value> = self.stack.drain(start..).collect();
                    self.stack.push(Value::new_struct(fields));
                }
                Op::GetField(idx) => {
                    let value = self.pop()?;
                    match value {
                        Value::Struct(fields) => {
                            let v = fields.get(idx as usize).cloned().ok_or_else(|| {
                                RuntimeError::InvalidOperation {
                                    op: format!("GetField({}) out of range", idx),
                                    span: dummy_span(),
                                }
                            })?;
                            self.stack.push(v);
                        }
                        v => return Err(type_mismatch("Struct", &v)),
                    }
                }
                Op::MakeVariant(variant_idx, n) => {
                    let n = n as usize;
                    if self.stack.len() < n {
                        return Err(underflow());
                    }
                    let start = self.stack.len() - n;
                    let fields: Vec<Value> = self.stack.drain(start..).collect();
                    self.stack.push(Value::new_variant(variant_idx, fields));
                }
                Op::MatchVariant(idx) => {
                    let value = self.pop()?;
                    match value {
                        Value::Variant(v, _) => {
                            self.stack.push(Value::new_bool(v == idx));
                        }
                        v => return Err(type_mismatch("Variant", &v)),
                    }
                }
                Op::UnpackVariant => {
                    let value = self.pop()?;
                    match value {
                        Value::Variant(_, fields) => {
                            for f in fields {
                                self.stack.push(f);
                            }
                        }
                        v => return Err(type_mismatch("Variant", &v)),
                    }
                }
                Op::MakeTuple(n) => {
                    let n = n as usize;
                    if self.stack.len() < n {
                        return Err(underflow());
                    }
                    let start = self.stack.len() - n;
                    let elements: Vec<Value> = self.stack.drain(start..).collect();
                    self.stack.push(Value::new_tuple(elements));
                }
                Op::GetTupleField(idx) => {
                    let value = self.pop()?;
                    match value {
                        Value::Tuple(elems) => {
                            let v = elems.get(idx as usize).cloned().ok_or_else(|| {
                                RuntimeError::InvalidOperation {
                                    op: format!("GetTupleField({}) out of range", idx),
                                    span: dummy_span(),
                                }
                            })?;
                            self.stack.push(v);
                        }
                        v => return Err(type_mismatch("Tuple", &v)),
                    }
                }
            }
        }

        // Result is whatever sits on top of the value stack at exit.
        Ok(self.stack.pop().unwrap_or(Value::new_unit()))
    }
}

// ============================================================================
// Private helpers
// ============================================================================

impl BytecodeVM {
    fn pop(&mut self) -> Result<Value, RuntimeError> {
        self.stack.pop().ok_or_else(underflow)
    }

    fn pop_int(&mut self) -> Result<i64, RuntimeError> {
        match self.pop()? {
            Value::Int(n) => Ok(n),
            v => Err(type_mismatch("Int", &v)),
        }
    }

    fn pop_float(&mut self) -> Result<f64, RuntimeError> {
        match self.pop()? {
            Value::Float(f) => Ok(f),
            v => Err(type_mismatch("Float", &v)),
        }
    }

    fn pop_bool(&mut self) -> Result<bool, RuntimeError> {
        match self.pop()? {
            Value::Bool(b) => Ok(b),
            v => Err(type_mismatch("Bool", &v)),
        }
    }

    fn pop_str(&mut self) -> Result<String, RuntimeError> {
        match self.pop()? {
            Value::Str(s) => Ok(s),
            v => Err(type_mismatch("Str", &v)),
        }
    }
}

fn constant_to_value(c: &Constant) -> Value {
    match c {
        Constant::Int(n) => Value::new_int(*n),
        Constant::Float(f) => Value::new_float(*f),
        Constant::Str(s) => Value::new_str(s.clone()),
        Constant::Bool(b) => Value::new_bool(*b),
        Constant::Fn(idx) => Value::new_fn(*idx),
        Constant::NativeFn(sym) => Value::new_native_fn(*sym),
    }
}

fn value_to_native(v: &Value) -> NativeValue {
    match v {
        Value::Int(n) => NativeValue::Int(*n),
        Value::Float(f) => NativeValue::Float(*f),
        Value::Bool(b) => NativeValue::Bool(*b),
        Value::Str(s) => NativeValue::Str(s.clone()),
        Value::Unit => NativeValue::Unit,
        Value::ShellOutput(out) => NativeValue::ShellOutput(out.clone()),
        // Compound and function values cannot cross the native boundary;
        // surface them as Unit so a wrong-type native call produces a
        // descriptive error inside the native rather than a panic here.
        Value::Fn(_)
        | Value::NativeFn(_)
        | Value::Struct(_)
        | Value::Variant(_, _)
        | Value::Tuple(_) => NativeValue::Unit,
    }
}

fn native_to_value(v: NativeValue) -> Value {
    match v {
        NativeValue::Int(n) => Value::new_int(n),
        NativeValue::Float(f) => Value::new_float(f),
        NativeValue::Bool(b) => Value::new_bool(b),
        NativeValue::Str(s) => Value::new_str(s),
        NativeValue::Unit => Value::new_unit(),
        NativeValue::ShellOutput(out) => Value::ShellOutput(out),
    }
}

/// Adds a post-advance jump offset to an `ip`.
fn apply_offset(ip: usize, off: i16) -> usize {
    (ip as i64 + off as i64) as usize
}

fn dummy_span() -> Span {
    Span::new(0, 0)
}

fn underflow() -> RuntimeError {
    RuntimeError::StackUnderflow { span: dummy_span() }
}

fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Int(_) => "Int",
        Value::Float(_) => "Float",
        Value::Bool(_) => "Bool",
        Value::Str(_) => "Str",
        Value::Unit => "Unit",
        Value::Fn(_) => "Fn",
        Value::NativeFn(_) => "NativeFn",
        Value::ShellOutput(_) => "ShellOutput",
        Value::Struct(_) => "Struct",
        Value::Variant(_, _) => "Variant",
        Value::Tuple(_) => "Tuple",
    }
}

fn type_mismatch(expected: &str, found: &Value) -> RuntimeError {
    RuntimeError::TypeMismatch {
        expected: expected.to_string(),
        found: type_name(found).to_string(),
        span: dummy_span(),
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use ferric_common::{Interner, Symbol};
    use ferric_lexer::lex;
    use ferric_parser::parse;
    use ferric_resolve::resolve_with_natives;
    use ferric_infer::typecheck;
    use ferric_stdlib::register_stdlib;

    fn run_source(src: &str) -> Result<Value, RuntimeError> {
        let mut interner = Interner::new();
        let mut natives = NativeRegistry::new();
        register_stdlib(&mut natives, &mut interner);

        let native_fns: Vec<(Symbol, Vec<Symbol>)> = vec![
            (interner.intern("println"),         vec![interner.intern("s")]),
            (interner.intern("print"),           vec![interner.intern("s")]),
            (interner.intern("int_to_str"),      vec![interner.intern("n")]),
            (interner.intern("float_to_str"),    vec![interner.intern("n")]),
            (interner.intern("bool_to_str"),     vec![interner.intern("b")]),
            (interner.intern("int_to_float"),    vec![interner.intern("n")]),
            (interner.intern("shell_stdout"),    vec![interner.intern("output")]),
            (interner.intern("shell_exit_code"), vec![interner.intern("output")]),
        ];

        let lex_result = lex(src, &mut interner);
        assert!(lex_result.errors.is_empty(), "lex: {:?}", lex_result.errors);
        let parse_result = parse(&lex_result);
        assert!(parse_result.errors.is_empty(), "parse: {:?}", parse_result.errors);
        let resolve_result = resolve_with_natives(&parse_result, &native_fns);
        assert!(resolve_result.errors.is_empty(), "resolve: {:?}", resolve_result.errors);
        let type_result = typecheck(&parse_result, &resolve_result, &interner);
        assert!(type_result.errors.is_empty(), "types: {:?}", type_result.errors);
        let program = ferric_compiler::compile(&parse_result, &resolve_result, &type_result, &interner);

        let mut vm = BytecodeVM::new();
        vm.run(program, natives, &interner)
    }

    #[test]
    fn integer_add() {
        assert_eq!(run_source("1 + 2").unwrap(), Value::Int(3));
    }

    #[test]
    fn integer_precedence() {
        assert_eq!(run_source("1 + 2 * 3").unwrap(), Value::Int(7));
    }

    #[test]
    fn let_then_use() {
        assert_eq!(run_source("let x: Int = 5\nx").unwrap(), Value::Int(5));
    }

    #[test]
    fn if_true_branch() {
        assert_eq!(
            run_source("if true { 1 } else { 2 }").unwrap(),
            Value::Int(1)
        );
    }

    #[test]
    fn if_false_branch() {
        assert_eq!(
            run_source("if false { 1 } else { 2 }").unwrap(),
            Value::Int(2)
        );
    }

    #[test]
    fn while_counts_to_five() {
        // Sum 0..5 → 0+1+2+3+4 = 10.
        let src = "\
let mut i: Int = 0
let mut sum: Int = 0
while i < 5 {
    sum = sum + i
    i = i + 1
}
sum
";
        assert_eq!(run_source(src).unwrap(), Value::Int(10));
    }

    #[test]
    fn break_exits_loop() {
        let src = "\
let mut i: Int = 0
loop {
    if i == 3 { break }
    i = i + 1
}
i
";
        assert_eq!(run_source(src).unwrap(), Value::Int(3));
    }

    #[test]
    fn recursive_function() {
        // Straight recursion is a good exercise for frame push/pop.
        let src = "\
fn fib(n: Int) -> Int {
    if n <= 1 { n } else { fib(n: n - 1) + fib(n: n - 2) }
}
fib(n: 10)
";
        assert_eq!(run_source(src).unwrap(), Value::Int(55));
    }

    #[test]
    fn string_concat() {
        let src = r#""hello " + "world""#;
        assert_eq!(
            run_source(src).unwrap(),
            Value::Str("hello world".to_string())
        );
    }

    #[test]
    fn division_by_zero_reports() {
        let err = run_source("5 / 0").unwrap_err();
        assert!(matches!(err, RuntimeError::DivisionByZero { .. }));
    }

    #[test]
    fn native_println_does_not_crash() {
        // println returns Unit.
        let src = r#"println(s: "hi")"#;
        assert_eq!(run_source(src).unwrap(), Value::Unit);
    }

    #[test]
    fn require_passes_silently() {
        let src = "\
let x: Int = 5
require x > 0
x
";
        assert_eq!(run_source(src).unwrap(), Value::Int(5));
    }

    #[test]
    fn require_fails_with_message() {
        let src = "\
let x: Int = -1
require x > 0, \"x must be positive\"
";
        let err = run_source(src).unwrap_err();
        match err {
            RuntimeError::RequireError { message, .. } => {
                assert_eq!(message.as_deref(), Some("x must be positive"));
            }
            other => panic!("expected RequireError, got {:?}", other),
        }
    }

    // ============================================================
    // M4 — structs / enums / tuples / patterns
    // ============================================================

    #[test]
    fn struct_literal_and_field_access() {
        let src = "\
struct Point { x: Int, y: Int }
let p = Point { x: 7, y: 9 }
p.x + p.y
";
        assert_eq!(run_source(src).unwrap(), Value::Int(16));
    }

    #[test]
    fn enum_match_picks_correct_arm() {
        let src = "\
enum Shape { Circle(Int), Rectangle(Int, Int) }
fn area(s: Shape) -> Int {
    match s {
        Shape::Circle(r) => r * r * 3,
        Shape::Rectangle(w, h) => w * h,
    }
}
area(s: Shape::Rectangle(3, 4))
";
        assert_eq!(run_source(src).unwrap(), Value::Int(12));
    }

    #[test]
    fn match_wildcard_covers_remaining_variants() {
        let src = "\
enum Color { Red, Green, Blue }
fn name(c: Color) -> Str {
    match c {
        Color::Red => \"red\",
        _ => \"other\",
    }
}
name(c: Color::Blue)
";
        assert_eq!(
            run_source(src).unwrap(),
            Value::Str("other".to_string())
        );
    }

    #[test]
    fn tuple_pattern_destructures() {
        let src = "\
let t = (10, 20)
match t {
    (a, b) => a + b,
}
";
        assert_eq!(run_source(src).unwrap(), Value::Int(30));
    }

    #[test]
    fn struct_pattern_with_literal_subpattern() {
        let src = "\
struct Pt { x: Int, y: Int }
fn classify(p: Pt) -> Str {
    match p {
        Pt { x: 0, y: 0 } => \"origin\",
        Pt { x, y: 0 } => \"x-axis\",
        Pt { x: 0, y } => \"y-axis\",
        Pt { x, y } => \"plane\",
    }
}
classify(p: Pt { x: 3, y: 0 })
";
        assert_eq!(
            run_source(src).unwrap(),
            Value::Str("x-axis".to_string())
        );
    }

    #[test]
    fn require_set_recovers() {
        // set_fn's `x = 5` must mutate the outer mutable slot.
        let src = "\
let mut x: Int = -1
require x > 0, \"x must be positive\", set: || { x = 5 }
x
";
        assert_eq!(run_source(src).unwrap(), Value::Int(5));
    }
}
