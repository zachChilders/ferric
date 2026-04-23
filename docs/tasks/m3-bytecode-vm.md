# Task: M3 Bytecode VM Replacement

## Objective
Replace `TreeWalker` with `BytecodeVM`, both implementing the `Executor` trait. The bytecode VM executes compiled bytecode instead of walking the AST.

## Architecture Context
- This is a **stage replacement** (second time replacing the VM)
- Both VMs implement the same `Executor` trait
- The public interface remains identical
- `main.rs` changes: one import swap
- All other stages remain completely untouched

## Public Interface (UNCHANGED)

```rust
// ferric_vm/src/lib.rs
// Executor trait is unchanged
pub trait Executor {
    fn run(&mut self, program: Program, natives: NativeRegistry) -> Result<Value, RuntimeError>;
}

// TreeWalker is replaced with BytecodeVM
pub struct BytecodeVM {
    // private fields
}

impl BytecodeVM {
    pub fn new() -> Self;
}

impl Executor for BytecodeVM {
    fn run(&mut self, program: Program, natives: NativeRegistry) -> Result<Value, RuntimeError>;
}

// Value type is unchanged (Rule 7)
pub enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Unit,
    Fn(u16),  // M3: now stores chunk index instead of DefId
}

// Constructor functions unchanged
impl Value {
    pub fn new_int(n: i64) -> Self { Value::Int(n) }
    pub fn new_float(f: f64) -> Self { Value::Float(f) }
    pub fn new_bool(b: bool) -> Self { Value::Bool(b) }
    pub fn new_str(s: String) -> Self { Value::Str(s) }
    pub fn new_unit() -> Self { Value::Unit }
}
```

## Feature Requirements

### VM State
```rust
pub struct BytecodeVM {
    stack: Vec<Value>,              // Value stack
    call_stack: Vec<CallFrame>,     // Call frames
    natives: Option<NativeRegistry>, // Cached after first run
}

struct CallFrame {
    chunk_idx: u16,          // Which chunk we're executing
    ip: usize,               // Instruction pointer (index into chunk.code)
    slots: Vec<Value>,       // Local variable slots
    stack_base: usize,       // Where this frame's stack starts
}
```

### Execution Loop
Classic bytecode interpreter with fetch-decode-execute loop:

```rust
impl Executor for BytecodeVM {
    fn run(&mut self, program: Program, natives: NativeRegistry) -> Result<Value, RuntimeError> {
        self.natives = Some(natives);

        // Find and execute entry chunk
        let entry_idx = program.entry;
        let chunk = &program.chunks[entry_idx as usize];

        // Create initial call frame
        let frame = CallFrame {
            chunk_idx: entry_idx,
            ip: 0,
            slots: vec![],
            stack_base: 0,
        };
        self.call_stack.push(frame);

        // Execute until return or error
        loop {
            let frame = self.call_stack.last_mut().unwrap();
            let chunk = &program.chunks[frame.chunk_idx as usize];

            if frame.ip >= chunk.code.len() {
                // Implicit return
                break;
            }

            let op = &chunk.code[frame.ip];
            frame.ip += 1;

            match op {
                Op::LoadConst(idx) => {
                    let constant = &chunk.constants[*idx as usize];
                    let value = constant_to_value(constant);
                    self.stack.push(value);
                }

                Op::LoadSlot(slot) => {
                    let frame = self.call_stack.last().unwrap();
                    let value = frame.slots[*slot as usize].clone();
                    self.stack.push(value);
                }

                Op::StoreSlot(slot) => {
                    let value = self.stack.pop().unwrap();
                    let frame = self.call_stack.last_mut().unwrap();
                    if (*slot as usize) >= frame.slots.len() {
                        frame.slots.resize(*slot as usize + 1, Value::Unit);
                    }
                    frame.slots[*slot as usize] = value;
                }

                Op::Pop => {
                    self.stack.pop();
                }

                Op::Dup => {
                    let value = self.stack.last().unwrap().clone();
                    self.stack.push(value);
                }

                // Arithmetic operations
                Op::AddInt => {
                    let b = self.pop_int()?;
                    let a = self.pop_int()?;
                    self.stack.push(Value::new_int(a + b));
                }

                Op::SubInt => {
                    let b = self.pop_int()?;
                    let a = self.pop_int()?;
                    self.stack.push(Value::new_int(a - b));
                }

                // ... similar for all arithmetic ops

                // Comparison
                Op::LtInt => {
                    let b = self.pop_int()?;
                    let a = self.pop_int()?;
                    self.stack.push(Value::new_bool(a < b));
                }

                // ... similar for all comparison ops

                // Control flow
                Op::Jump(offset) => {
                    let frame = self.call_stack.last_mut().unwrap();
                    frame.ip = (frame.ip as i16 + offset) as usize;
                }

                Op::JumpIfFalse(offset) => {
                    let cond = self.pop_bool()?;
                    if !cond {
                        let frame = self.call_stack.last_mut().unwrap();
                        frame.ip = (frame.ip as i16 + offset) as usize;
                    }
                }

                Op::Return => {
                    self.call_stack.pop();
                    if self.call_stack.is_empty() {
                        break;
                    }
                }

                // Function calls
                Op::Call(argc) => {
                    let callee = self.stack.pop().unwrap();
                    match callee {
                        Value::Fn(chunk_idx) => {
                            // Pop arguments
                            let mut args = vec![];
                            for _ in 0..*argc {
                                args.push(self.stack.pop().unwrap());
                            }
                            args.reverse();

                            // Create new call frame
                            let frame = CallFrame {
                                chunk_idx,
                                ip: 0,
                                slots: args,  // Parameters go in slots
                                stack_base: self.stack.len(),
                            };
                            self.call_stack.push(frame);
                        }
                        _ => return Err(RuntimeError::NotCallable { ... }),
                    }
                }

                Op::CallNative(native_idx) => {
                    // Look up native function
                    // Pop arguments
                    // Call native
                    // Push result
                }

                Op::Unit => {
                    self.stack.push(Value::new_unit());
                }

                // ... other operations
            }
        }

        // Return top of stack or Unit
        Ok(self.stack.pop().unwrap_or(Value::new_unit()))
    }
}
```

### Helper Methods
```rust
impl BytecodeVM {
    fn pop_int(&mut self) -> Result<i64, RuntimeError> {
        match self.stack.pop() {
            Some(Value::Int(n)) => Ok(n),
            Some(v) => Err(RuntimeError::TypeMismatch {
                expected: "Int".to_string(),
                found: format!("{:?}", v),
                span: Span { start: 0, end: 0 },  // TODO: track spans in bytecode
            }),
            None => Err(RuntimeError::StackUnderflow { ... }),
        }
    }

    fn pop_float(&mut self) -> Result<f64, RuntimeError> { ... }
    fn pop_bool(&mut self) -> Result<bool, RuntimeError> { ... }
    fn pop_str(&mut self) -> Result<String, RuntimeError> { ... }
}

fn constant_to_value(c: &Constant) -> Value {
    match c {
        Constant::Int(n) => Value::new_int(*n),
        Constant::Float(f) => Value::new_float(*f),
        Constant::Str(s) => Value::new_str(s.clone()),
        Constant::Bool(b) => Value::new_bool(*b),
    }
}
```

## Error Handling

Runtime errors in bytecode are trickier because we don't have AST spans.

**M3 Simplification:** For now, use dummy spans or carry spans through bytecode.

**Future improvement:** Add a separate debug info table mapping bytecode offsets to spans.

## Test Cases

Create unit tests for:
1. Simple arithmetic: bytecode for `1 + 2` executes to 3
2. Variable load/store: `let x = 5; x` works
3. Function call: calling a user-defined function works
4. Native function call: calling `println` works
5. If expression: conditional execution works
6. While loop: loop executes correct number of times
7. Break statement: loop exits early
8. Stack management: stack doesn't overflow or underflow
9. Call stack: nested function calls work
10. All M1 and M2 programs execute correctly with bytecode

## Performance Validation

The bytecode VM should be noticeably faster than the tree-walker for programs with loops.

Create a benchmark:
```rust
fn fibonacci(n: Int) -> Int {
    if n <= 1 { n } else { fibonacci(n - 1) + fibonacci(n - 2) }
}

fibonacci(20)
```

Time this with:
1. TreeWalker (before replacement)
2. BytecodeVM (after replacement)

BytecodeVM should be 2-5x faster. Document the results.

## Integration Changes

### In ferric_vm/src/lib.rs
Keep both implementations temporarily for comparison:
```rust
pub mod tree_walker;  // old implementation
pub mod bytecode;     // new implementation

pub use tree_walker::TreeWalker;
pub use bytecode::BytecodeVM;

// Keep Executor trait, Value, etc. in lib.rs
```

### In main.rs
```rust
// Change from:
let mut vm = TreeWalker::new();

// To:
let mut vm = BytecodeVM::new();

// Everything else is UNCHANGED because of the Executor trait
```

## Acceptance Criteria
- [ ] `BytecodeVM` implements `Executor` trait
- [ ] All M3 instructions are handled in execution loop
- [ ] Stack and call stack are managed correctly
- [ ] Function calls (user-defined and native) work
- [ ] Control flow (jumps, loops, breaks) works
- [ ] All M1 and M2 programs execute correctly
- [ ] Performance is better than TreeWalker
- [ ] Integration requires **only changing the VM constructor in main.rs**
- [ ] No changes to lexer, parser, resolver, type checker, compiler, diagnostics, or stdlib

## Critical Architecture Validation

**This milestone proves Rule 6 pays off:**
- Complete VM replacement (second time)
- Same `Executor` trait
- **Only main.rs changes (one constructor swap)**
- All other stages completely untouched
- This is only possible because the VM is behind a trait

Document this success in the replacement log.

## Notes for Agent
- Keep the execution loop clean and readable
- Test each instruction in isolation before integration
- Stack management is critical - test boundary conditions
- Make sure jump offsets are calculated correctly (they're relative)
- Consider adding debug logging for instruction execution
- The TreeWalker can be kept around for testing/comparison
- Focus on correctness first, performance optimization can come later
