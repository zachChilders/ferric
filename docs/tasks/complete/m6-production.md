# Task: M6 Production Features

## Objective
Add closures, arrays, Option/Result types, expanded stdlib, REPL, and production-quality multi-span error messages. Replace diagnostics for the second time. No other stage replacements - only additions.

## Architecture Context
- This is the final milestone before shipping
- One stage replacement: diagnostics (proving Rule 5 again)
- All other stages receive additive changes only
- Focus on completeness and polish

## M6 Target Programs

**Closures:**
```rust
let nums = [1, 2, 3, 4, 5]
let doubled = nums.map(|x| x * 2)
```

**Result type:**
```rust
fn safe_divide(a: Int, b: Int) -> Result<Int, Str> {
    if b == 0 { Err("division by zero") } else { Ok(a / b) }
}

match safe_divide(10, 0) {
    Ok(n) => println(n),
    Err(e) => println("Error: " + e),
}
```

**Arrays:**
```rust
let nums = [1, 2, 3, 4, 5]
let sum = nums.fold(0, |acc, x| acc + x)
println(sum)
```

## Changes to ferric_common

### Add built-in generic types
```rust
pub enum Ty {
    // ... existing variants
    Array(Box<Ty>),      // [T]
    Option(Box<Ty>),     // Option<T>
    Result(Box<Ty>, Box<Ty>),  // Result<T, E>
}
```

### Add closures to expressions
```rust
pub enum Expr {
    // ... existing variants

    Closure {           // NEW: |x, y| x + y
        params: Vec<(Symbol, Option<TypeAnnotation>)>,
        body: Box<Expr>,
        id: NodeId,
        span: Span,
    },

    ArrayLit {          // NEW: [1, 2, 3]
        elements: Vec<Expr>,
        id: NodeId,
        span: Span,
    },

    Index {             // NEW: arr[0]
        array: Box<Expr>,
        index: Box<Expr>,
        id: NodeId,
        span: Span,
    },
}
```

### Add to statements
```rust
pub enum Stmt {
    // ... existing variants

    For {               // NEW: for x in arr { ... }
        var: Symbol,
        iter: Expr,
        body: Expr,
        id: NodeId,
        span: Span,
    },
}
```

## Stage Changes

### 1. ferric_lexer Additions

Add `|` for closures (already have), add `[` and `]` (already have).

### 2. ferric_parser Additions

**Parse closure:**
```rust
fn parse_closure(&mut self) -> Expr {
    self.expect(TokenKind::Pipe, "expected '|'");

    let mut params = vec![];
    while !self.check(TokenKind::Pipe) {
        let name = self.expect_ident();
        let ty = if self.check(TokenKind::Colon) {
            self.advance();
            Some(self.parse_type())
        } else {
            None
        };
        params.push((name, ty));

        if !self.check(TokenKind::Pipe) {
            self.expect(TokenKind::Comma, "expected ','");
        }
    }

    self.expect(TokenKind::Pipe, "expected '|'");
    let body = self.parse_expr();

    Expr::Closure {
        params,
        body: Box::new(body),
        id: self.node_id_gen.next(),
        span: ...
    }
}
```

**Parse array literal:**
```rust
fn parse_array_lit(&mut self) -> Expr {
    self.expect(TokenKind::LBracket, "expected '['");

    let mut elements = vec![];
    while !self.check(TokenKind::RBracket) {
        elements.push(self.parse_expr());
        if !self.check(TokenKind::RBracket) {
            self.expect(TokenKind::Comma, "expected ','");
        }
    }

    self.expect(TokenKind::RBracket, "expected ']'");
    Expr::ArrayLit { elements, ... }
}
```

**Parse array indexing:**
```rust
fn parse_postfix_expr(&mut self) -> Expr {
    let mut expr = self.parse_primary();

    loop {
        if self.check(TokenKind::LBracket) {
            self.advance();
            let index = self.parse_expr();
            self.expect(TokenKind::RBracket, "expected ']'");
            expr = Expr::Index {
                array: Box::new(expr),
                index: Box::new(index),
                id: self.node_id_gen.next(),
                span: ...
            };
        }
        // ... other postfix operators
    }

    expr
}
```

**Parse for loop:**
```rust
fn parse_for_loop(&mut self) -> Stmt {
    self.expect(TokenKind::For, "expected 'for'");
    let var = self.expect_ident();
    self.expect_keyword("in");
    let iter = self.parse_expr();
    let body = self.parse_block();

    Stmt::For { var, iter, body, ... }
}
```

### 3. ferric_resolve Additions

**Closure capture analysis:**

```rust
struct Resolver {
    // ... existing fields
    captures: HashMap<NodeId, Vec<DefId>>,  // closure id -> captured variables
}

impl Resolver {
    fn resolve_closure(&mut self, closure: &Expr) {
        if let Expr::Closure { params, body, id, .. } = closure {
            // Push new scope for parameters
            self.push_scope();

            // Define parameters
            for (param_name, _) in params {
                self.define(*param_name, false, closure.span());
            }

            // Track which variables from outer scopes are used
            let captures_start = self.captures.len();

            // Resolve body
            self.resolve_expr(body);

            // Record captures
            let captured = self.captures[captures_start..].to_vec();
            self.captures.insert(*id, captured);

            self.pop_scope();
        }
    }

    fn resolve_variable(&mut self, id: NodeId, name: Symbol, span: Span) {
        if let Some(binding) = self.lookup(name) {
            self.resolutions.insert(id, binding.def_id);

            // Check if this is a capture (defined in outer scope)
            if !self.is_in_current_scope(name) {
                // Record as captured variable
                self.captures.push(binding.def_id);
            }
        } else {
            self.errors.push(ResolveError::UndefinedVariable { name, span });
        }
    }
}
```

### 4. ferric_infer Additions

**Type-check closures:**
```rust
Expr::Closure { params, body, id, span } => {
    // Create function type
    let param_tys: Vec<_> = params.iter().map(|(_, ty_ann)| {
        match ty_ann {
            Some(ann) => self.convert_type_annotation(ann),
            None => self.fresh_tyvar(),  // infer parameter type
        }
    }).collect();

    // Type-check body
    let body_ty = self.infer_expr(body)?;

    let fn_ty = Ty::Fn {
        params: param_tys,
        ret: Box::new(body_ty),
    };

    self.node_types.insert(*id, fn_ty.clone());
    Ok(fn_ty)
}
```

**Type-check arrays:**
```rust
Expr::ArrayLit { elements, id, span } => {
    if elements.is_empty() {
        // Empty array - infer element type as type variable
        let elem_ty = self.fresh_tyvar();
        let array_ty = Ty::Array(Box::new(elem_ty));
        self.node_types.insert(*id, array_ty.clone());
        Ok(array_ty)
    } else {
        // Infer element type from first element
        let first_ty = self.infer_expr(&elements[0])?;

        // Check all other elements have same type
        for elem in &elements[1..] {
            let elem_ty = self.infer_expr(elem)?;
            self.unify(&first_ty, &elem_ty, *span)?;
        }

        let array_ty = Ty::Array(Box::new(first_ty));
        self.node_types.insert(*id, array_ty.clone());
        Ok(array_ty)
    }
}
```

**Type-check array indexing:**
```rust
Expr::Index { array, index, id, span } => {
    let array_ty = self.infer_expr(array)?;
    let index_ty = self.infer_expr(index)?;

    // Index must be Int
    self.unify(&index_ty, &Ty::Int, *span)?;

    // Array must be Array<T>
    match array_ty {
        Ty::Array(elem_ty) => {
            self.node_types.insert(*id, (*elem_ty).clone());
            Ok((*elem_ty).clone())
        }
        _ => Err(TypeError::NotAnArray { ty: array_ty, span: *span }),
    }
}
```

**For loop desugaring:**
```rust
// Desugar:  for x in arr { body }
// Into:     arr.for_each(|x| body)
//
// This requires the Iterator trait with a for_each method
```

### 5. ferric_compiler Additions

**Compile closures:**

Add instruction:
```rust
pub enum Op {
    // ... existing ops
    MakeClosure(u16, u8),  // (function_index, capture_count)
}
```

Compilation:
```rust
Expr::Closure { params, body, id, .. } => {
    // Compile closure body as a separate function
    let closure_fn_idx = self.compile_closure_function(params, body);

    // Push captured variables onto stack
    let captures = &self.resolve.captures[id];
    for capture_def_id in captures {
        let slot = self.resolve.def_slots[capture_def_id];
        self.emit(Op::LoadSlot(slot as u8));
    }

    // Create closure value
    self.emit(Op::MakeClosure(closure_fn_idx, captures.len() as u8));
}

fn compile_closure_function(&mut self, params: &[(Symbol, Option<TypeAnnotation>)], body: &Expr) -> u16 {
    // Create new chunk for closure
    let mut chunk = Chunk {
        code: vec![],
        constants: vec![],
        name: self.interner.intern("<closure>"),
    };

    // Compile body
    // ... (similar to regular function)

    // Add chunk and return index
    let idx = self.chunks.len();
    self.chunks.push(chunk);
    idx as u16
}
```

**Compile array literals:**
```rust
Expr::ArrayLit { elements, .. } => {
    // Push all elements
    for elem in elements {
        self.compile_expr(elem);
    }

    // Create array
    self.emit(Op::MakeArray(elements.len() as u8));
}
```

Add instructions:
```rust
pub enum Op {
    // ... existing ops
    MakeArray(u8),      // Create array with u8 elements
    ArrayGet,           // Get array[index]
    ArraySet,           // Set array[index] = value
    ArrayLen,           // Get array length
}
```

### 6. ferric_vm Additions

**Add closure and array values:**
```rust
pub enum Value {
    // ... existing variants
    Closure {
        fn_idx: u16,
        captures: Vec<Value>,
    },
    Array(Vec<Value>),
}

impl Value {
    pub fn new_closure(fn_idx: u16, captures: Vec<Value>) -> Self {
        Value::Closure { fn_idx, captures }
    }
    pub fn new_array(elements: Vec<Value>) -> Self {
        Value::Array(elements)
    }
}
```

**Execute closure creation:**
```rust
Op::MakeClosure(fn_idx, capture_count) => {
    let mut captures = vec![];
    for _ in 0..*capture_count {
        captures.push(self.stack.pop().unwrap());
    }
    captures.reverse();
    self.stack.push(Value::new_closure(*fn_idx, captures));
}
```

**Execute closure calls:**
```rust
Op::Call(argc) => {
    let callee = self.stack.pop().unwrap();
    match callee {
        Value::Fn(chunk_idx) => {
            // Regular function call (existing code)
        }
        Value::Closure { fn_idx, captures } => {
            // Pop arguments
            let mut args = vec![];
            for _ in 0..*argc {
                args.push(self.stack.pop().unwrap());
            }
            args.reverse();

            // Create new call frame with captured variables in slots
            let mut slots = captures.clone();
            slots.extend(args);

            let frame = CallFrame {
                chunk_idx: fn_idx,
                ip: 0,
                slots,
                stack_base: self.stack.len(),
            };
            self.call_stack.push(frame);
        }
        _ => return Err(RuntimeError::NotCallable { ... }),
    }
}
```

**Execute array operations:**
```rust
Op::MakeArray(count) => {
    let mut elements = vec![];
    for _ in 0..*count {
        elements.push(self.stack.pop().unwrap());
    }
    elements.reverse();
    self.stack.push(Value::new_array(elements));
}

Op::ArrayGet => {
    let index = self.pop_int()?;
    let array = self.stack.pop().unwrap();
    match array {
        Value::Array(elements) => {
            if index < 0 || index >= elements.len() as i64 {
                return Err(RuntimeError::IndexOutOfBounds { ... });
            }
            self.stack.push(elements[index as usize].clone());
        }
        _ => return Err(RuntimeError::NotAnArray { ... }),
    }
}
```

### 7. ferric_stdlib Expansion

Add new built-in functions:

**Array methods:**
```rust
fn builtin_array_len(args: &[Value]) -> Result<Value, String> {
    check_arg_count(args, 1)?;
    match &args[0] {
        Value::Array(elements) => Ok(Value::new_int(elements.len() as i64)),
        _ => Err("expected array".to_string()),
    }
}

fn builtin_array_push(args: &[Value]) -> Result<Value, String> {
    check_arg_count(args, 2)?;
    // Note: This requires mutable arrays - may need to refactor Value
}

fn builtin_array_map(args: &[Value]) -> Result<Value, String> {
    check_arg_count(args, 2)?;
    let array = expect_array(&args[0])?;
    let closure = expect_closure(&args[1])?;

    let mut result = vec![];
    for elem in array {
        // Call closure with elem
        let mapped = call_closure(closure, &[elem])?;
        result.push(mapped);
    }

    Ok(Value::new_array(result))
}

// Similar for filter, fold, etc.
```

**String methods:**
```rust
fn builtin_str_len(args: &[Value]) -> Result<Value, String>;
fn builtin_str_split(args: &[Value]) -> Result<Value, String>;
fn builtin_str_trim(args: &[Value]) -> Result<Value, String>;
fn builtin_str_contains(args: &[Value]) -> Result<Value, String>;
fn builtin_str_starts_with(args: &[Value]) -> Result<Value, String>;
fn builtin_str_parse_int(args: &[Value]) -> Result<Value, String>;
```

**Math functions:**
```rust
fn builtin_abs(args: &[Value]) -> Result<Value, String>;
fn builtin_sqrt(args: &[Value]) -> Result<Value, String>;
fn builtin_pow(args: &[Value]) -> Result<Value, String>;
fn builtin_min(args: &[Value]) -> Result<Value, String>;
fn builtin_max(args: &[Value]) -> Result<Value, String>;
fn builtin_floor(args: &[Value]) -> Result<Value, String>;
fn builtin_ceil(args: &[Value]) -> Result<Value, String>;
```

**I/O functions:**
```rust
fn builtin_read_line(args: &[Value]) -> Result<Value, String> {
    check_arg_count(args, 0)?;
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)
        .map_err(|e| e.to_string())?;
    Ok(Value::new_str(line.trim_end().to_string()))
}
```

**Register Option and Result:**

Define Option and Result as built-in enums:
```rust
// In ferric_common, add built-in enum definitions
pub fn builtin_option_def(interner: &mut Interner) -> Item {
    Item::EnumDef {
        name: interner.intern("Option"),
        variants: vec![
            (interner.intern("Some"), vec![TypeAnnotation::Generic { name: interner.intern("T"), bounds: vec![] }]),
            (interner.intern("None"), vec![]),
        ],
        ...
    }
}

pub fn builtin_result_def(interner: &mut Interner) -> Item {
    Item::EnumDef {
        name: interner.intern("Result"),
        variants: vec![
            (interner.intern("Ok"), vec![TypeAnnotation::Generic { name: interner.intern("T"), bounds: vec![] }]),
            (interner.intern("Err"), vec![TypeAnnotation::Generic { name: interner.intern("E"), bounds: vec![] }]),
        ],
        ...
    }
}
```

Register these at startup in the trait registry.

### 8. REPL Implementation

Add REPL mode to `main.rs`:

```rust
fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() == 1 {
        // No arguments - start REPL
        run_repl();
    } else {
        // File argument - run file
        run_file(&args[1]);
    }
}

fn run_repl() {
    println!("Ferric REPL v1.0");

    let mut interner = Interner::new();
    let mut env = Environment::new();  // Persistent environment
    let mut natives = NativeRegistry::new();
    register_stdlib(&mut natives, &mut interner);

    loop {
        print!("> ");
        std::io::stdout().flush().unwrap();

        let mut input = String::new();
        std::io::stdin().read_line(&mut input).unwrap();

        if input.trim() == "exit" {
            break;
        }

        // Run pipeline on input
        let lex_result = ferric_lexer::lex(&input, &mut interner);
        let parse_result = ferric_parser::parse(&lex_result);
        let resolve_result = ferric_resolve::resolve(&parse_result);
        let trait_registry = ferric_traits::build_registry(&parse_result, &interner);
        let type_result = ferric_infer::typecheck(&parse_result, &resolve_result, &trait_registry);
        let exhaust_result = ferric_exhaust::check_exhaustiveness(&parse_result, &type_result);

        // Report errors
        if has_errors(&lex_result, &parse_result, &resolve_result, &type_result, &exhaust_result) {
            report_errors(&input, ...);
            continue;
        }

        // Compile and execute
        let program = ferric_compiler::compile(&parse_result, &resolve_result, &type_result);
        let mut vm = BytecodeVM::new();
        match vm.run(program, natives.clone()) {
            Ok(value) => {
                if !matches!(value, Value::Unit) {
                    println!("{:?}", value);
                }
            }
            Err(e) => {
                eprintln!("{}", render_runtime_error(&e));
            }
        }
    }
}
```

### 9. ferric_diagnostics REPLACEMENT (Second Time)

Replace span renderer with multi-label renderer:

```rust
pub struct Renderer {
    source: String,
    line_starts: Vec<usize>,
}

impl Renderer {
    // ... existing methods

    pub fn render_error_with_labels(&self, error: &dyn DiagnosticError) -> String {
        // Render with primary span, secondary spans, notes, and help messages
    }
}

pub trait DiagnosticError {
    fn primary_span(&self) -> Span;
    fn secondary_spans(&self) -> Vec<(Span, String)>;
    fn message(&self) -> String;
    fn notes(&self) -> Vec<String>;
    fn help(&self) -> Option<String>;
}
```

**Example output:**
```
error[E003]: type mismatch
  --> main.fe:8:18
   |
 6 |     let x: Int = "hello"
   |             --- expected `Int` because of this annotation
 8 |     x + 1.0
   |         ^^^ found `Float`
   |
   = note: consider using int_to_float() to convert
   = help: remove the type annotation to let the type be inferred
```

**Critical: This replacement requires ZERO changes to any other stage.**

All stages already emit Spans (Rule 5), so the new renderer just uses them.

## Acceptance Criteria

- [ ] Closures work with capture analysis
- [ ] Arrays with map, filter, fold work
- [ ] For loops work
- [ ] Option and Result pattern-match correctly
- [ ] Full stdlib available
- [ ] Multi-label error rendering works
- [ ] REPL starts and maintains state
- [ ] All M1-M5 tests still pass
- [ ] Diagnostics replacement requires **zero changes to other stages**

## Test Cases

1. Closure captures variables correctly
2. Array map/filter/fold work
3. For loop over array works
4. Option.Some and Option.None pattern-match
5. Result.Ok and Result.Err pattern-match
6. REPL maintains state across inputs
7. Multi-label errors render correctly
8. All string/math/io functions work

## Notes for Agent
- This is the biggest milestone - take it slow
- Closures are the hardest part - test capture analysis thoroughly
- REPL should be simple - reuse the pipeline
- Make sure Option/Result are properly generic
- Validate the diagnostics replacement requires zero stage changes
- Consider adding a standard prelude that auto-imports Option/Result
- Document all new stdlib functions clearly
