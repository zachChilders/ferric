# Task: M3 Bytecode Compiler Implementation

## Objective
Create the `ferric_compiler` crate as a new pipeline stage that compiles the AST to bytecode. This is inserted between type checking and execution.

## Architecture Context
- This is a **new stage** added to the pipeline
- Has its own public entry point: `pub fn compile(...) -> Program`
- Consumes AST, resolve, and type information
- Produces bytecode `Program` for the VM
- `main.rs` adds one function call - no other changes

## Public Interface (Non-Negotiable)

```rust
// ferric_compiler/src/lib.rs
pub fn compile(
    ast: &ParseResult,
    resolve: &ResolveResult,
    types: &TypeResult,
) -> Program;
```

## Feature Requirements

### Update Program Type in ferric_common

Replace the M1/M2 Program (which just held the AST):
```rust
pub struct Program {
    pub chunks: Vec<Chunk>,
    pub entry: u16,  // index of main chunk
}

pub struct Chunk {
    pub code: Vec<Op>,
    pub constants: Vec<Constant>,
    pub name: Symbol,  // function name for debugging
}

pub enum Constant {
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
}
```

### Instruction Set (M3)

```rust
pub enum Op {
    // Stack manipulation
    LoadConst(u8),      // Push constant from constant pool
    LoadSlot(u8),       // Push local variable from slot
    StoreSlot(u8),      // Pop and store in local variable slot
    Pop,                // Pop and discard top of stack
    Dup,                // Duplicate top of stack

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
    LtInt,
    GtInt,
    LeInt,
    GeInt,
    EqFloat,
    LtFloat,
    GtFloat,
    LeFloat,
    GeFloat,
    EqBool,
    EqStr,

    // Boolean logic
    Not,
    AndBool,
    OrBool,

    // String operations
    Concat,             // String concatenation

    // Control flow
    Jump(i16),          // Unconditional jump (signed offset)
    JumpIfFalse(i16),   // Jump if top of stack is false
    JumpIfTrue(i16),    // Jump if top of stack is true
    Return,             // Return from function

    // Function calls
    Call(u8),           // Call function (u8 = arg count)
    CallNative(u8),     // Call native function (u8 = native index)
    TailCall(u8),       // Tail call optimization

    // Data construction
    MakeTuple(u8),      // Create tuple with u8 elements
    Unit,               // Push Unit value
}
```

### Compilation Strategy

**Function compilation:**
1. Each function becomes a `Chunk`
2. Parameters are assigned to slots 0..n
3. Local variables get slots n+1..m
4. Function body is compiled to instructions
5. Implicit return at end if no explicit return

**Expression compilation:**
- Most expressions leave their result on the stack
- Use the type information to emit correct typed instructions
  - `1 + 2` → `LoadConst(0), LoadConst(1), AddInt`
  - `1.0 + 2.0` → `LoadConst(0), LoadConst(1), AddFloat`

**Control flow:**
- `if` → `JumpIfFalse` to else branch, `Jump` to skip else
- `while` → `JumpIfFalse` to end, `Jump` back to condition
- `loop` → unconditional `Jump` back to start
- `break` → `Jump` to loop end (tracked on stack)
- `continue` → `Jump` to loop start

**Slot allocation:**
Use `resolve.def_slots` mapping for local variables.

## Implementation Notes

### Compiler Structure
```rust
struct Compiler<'a> {
    ast: &'a ParseResult,
    resolve: &'a ResolveResult,
    types: &'a TypeResult,

    // Current chunk being compiled
    current_chunk: Chunk,

    // Loop context for break/continue
    loop_stack: Vec<LoopContext>,

    // All compiled chunks
    chunks: Vec<Chunk>,
}

struct LoopContext {
    start_offset: usize,     // Jump target for continue
    break_jumps: Vec<usize>, // Patch points for break
}

impl<'a> Compiler<'a> {
    fn new(ast: &'a ParseResult, resolve: &'a ResolveResult, types: &'a TypeResult) -> Self;

    fn compile(&mut self) -> Program;
    fn compile_item(&mut self, item: &Item) -> u16;
    fn compile_stmt(&mut self, stmt: &Stmt);
    fn compile_expr(&mut self, expr: &Expr);

    fn emit(&mut self, op: Op);
    fn emit_jump(&mut self, op: Op) -> usize;  // Returns patch address
    fn patch_jump(&mut self, addr: usize);
    fn current_offset(&self) -> usize;

    fn add_constant(&mut self, c: Constant) -> u8;
}
```

### Expression Compilation Examples

**Binary operation:**
```rust
fn compile_expr(&mut self, expr: &Expr) {
    match expr {
        Expr::Binary { op, left, right, id, .. } => {
            // Compile left and right (leaves values on stack)
            self.compile_expr(left);
            self.compile_expr(right);

            // Look up the type of this expression
            let ty = &self.types.node_types[id];

            // Emit the appropriate instruction
            match (op, ty) {
                (BinOp::Add, Ty::Int) => self.emit(Op::AddInt),
                (BinOp::Add, Ty::Float) => self.emit(Op::AddFloat),
                (BinOp::Add, Ty::Str) => self.emit(Op::Concat),
                (BinOp::Sub, Ty::Int) => self.emit(Op::SubInt),
                (BinOp::Sub, Ty::Float) => self.emit(Op::SubFloat),
                (BinOp::Lt, Ty::Int) => self.emit(Op::LtInt),
                (BinOp::Lt, Ty::Float) => self.emit(Op::LtFloat),
                // ... etc
            }
        }
        // ... other cases
    }
}
```

**If expression:**
```rust
Expr::If { cond, then_branch, else_branch, .. } => {
    // Compile condition
    self.compile_expr(cond);

    // Jump to else if condition is false
    let else_jump = self.emit_jump(Op::JumpIfFalse(0));

    // Compile then branch
    self.compile_expr(then_branch);

    // Jump over else branch
    let end_jump = self.emit_jump(Op::Jump(0));

    // Patch else jump to here
    self.patch_jump(else_jump);

    // Compile else branch (or emit Unit)
    if let Some(else_br) = else_branch {
        self.compile_expr(else_br);
    } else {
        self.emit(Op::Unit);
    }

    // Patch end jump
    self.patch_jump(end_jump);
}
```

**While loop:**
```rust
Expr::While { cond, body, .. } => {
    let loop_start = self.current_offset();

    // Compile condition
    self.compile_expr(cond);

    // Jump to end if false
    let exit_jump = self.emit_jump(Op::JumpIfFalse(0));

    // Push loop context for break/continue
    self.loop_stack.push(LoopContext {
        start_offset: loop_start,
        break_jumps: vec![],
    });

    // Compile body
    self.compile_expr(body);
    self.emit(Op::Pop);  // Discard body result

    // Jump back to start
    let offset = loop_start as i16 - self.current_offset() as i16 - 1;
    self.emit(Op::Jump(offset));

    // Patch exit jump
    self.patch_jump(exit_jump);

    // Patch all break jumps
    let loop_ctx = self.loop_stack.pop().unwrap();
    for addr in loop_ctx.break_jumps {
        self.patch_jump(addr);
    }

    // While returns Unit
    self.emit(Op::Unit);
}
```

**Variable load:**
```rust
Expr::Variable { id, .. } => {
    let def_id = self.resolve.resolutions[id];
    let slot = self.resolve.def_slots[&def_id];
    self.emit(Op::LoadSlot(slot as u8));
}
```

**Variable store (assignment):**
```rust
Stmt::Assign { target, value, .. } => {
    // Compile value (leaves it on stack)
    self.compile_expr(value);

    // Store to slot
    if let Expr::Variable { id, .. } = target {
        let def_id = self.resolve.resolutions[id];
        let slot = self.resolve.def_slots[&def_id];
        self.emit(Op::StoreSlot(slot as u8));
    }
}
```

### Jump Patching
```rust
fn emit_jump(&mut self, op: Op) -> usize {
    let addr = self.current_chunk.code.len();
    self.emit(op);
    addr
}

fn patch_jump(&mut self, addr: usize) {
    let offset = self.current_chunk.code.len() as i16 - addr as i16 - 1;
    match &mut self.current_chunk.code[addr] {
        Op::Jump(ref mut o) => *o = offset,
        Op::JumpIfFalse(ref mut o) => *o = offset,
        Op::JumpIfTrue(ref mut o) => *o = offset,
        _ => panic!("patch_jump called on non-jump instruction"),
    }
}
```

### Constant Pool Management
```rust
fn add_constant(&mut self, c: Constant) -> u8 {
    // Check if constant already exists (deduplication)
    for (i, existing) in self.current_chunk.constants.iter().enumerate() {
        if existing == &c {
            return i as u8;
        }
    }

    // Add new constant
    let idx = self.current_chunk.constants.len();
    self.current_chunk.constants.push(c);
    idx as u8
}
```

## Test Cases

Create unit tests for:
1. Simple integer expression `1 + 2` compiles to correct bytecode
2. Variable load compiles to `LoadSlot` with correct slot index
3. If expression compiles with correct jump offsets
4. While loop compiles with correct backward jump
5. Function definition creates a Chunk with correct code
6. Constants are deduplicated in constant pool
7. Break compiles to jump to loop end
8. Type information is used to select correct instruction (AddInt vs AddFloat)
9. All M1 and M2 features compile correctly
10. Compiled bytecode matches expected instruction sequence

## Integration Changes

### In main.rs
```rust
// Add after type checking:
let program = ferric_compiler::compile(&parse_result, &resolve_result, &type_result);

// Change VM execution:
// Before: let program = Program { items: parse_result.items.clone() };
// After: (program already compiled above)
match vm.run(program, natives) { ... }
```

## Acceptance Criteria
- [ ] `ferric_compiler` crate created with public `compile()` function
- [ ] All M3 instructions defined in `ferric_common::Op`
- [ ] Program and Chunk types updated in `ferric_common`
- [ ] All M1 and M2 language features compile to bytecode
- [ ] Jump instructions have correct offsets
- [ ] Constant pool is populated correctly
- [ ] Slot indices from resolver are used correctly
- [ ] Type information is used to select typed instructions
- [ ] All unit tests pass
- [ ] Integration requires **one new call in main.rs**
- [ ] No changes to lexer, parser, resolver, or type checker

## Critical Rules to Enforce

### Rule 1 - Stages communicate only through output types
Compiler depends on `ParseResult`, `ResolveResult`, `TypeResult`.
Never imports from the stage crates themselves.

### Rule 2 - Exactly one public entry point
Only `compile()` is public.

## Notes for Agent
- This is a new stage, not a replacement - easier than inference
- Focus on correctness of jump offsets - test thoroughly
- Constant deduplication is an optimization, not required for M3
- Make sure to use type information to emit the right instructions
- Test with all M1/M2 programs to ensure bytecode is correct
- Consider adding a disassembler for debugging (not required, but helpful)
