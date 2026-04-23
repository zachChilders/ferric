# Task: M2 Language Enhancements

## Objective
Extend all pipeline stages to support M2 features: control flow (while, loop, break, continue), mutable variables, float literals, and assignment expressions. This milestone adds features **without replacing any stages** except diagnostics.

## Architecture Context
- This demonstrates **additive changes** within stages
- No stage boundaries change - only internals grow
- Only diagnostics is replaced (to prove the architecture)
- All other stages just add new cases to existing code

## M2 Target Program

```rust
fn fibonacci(n: Int) -> Int {
    if n <= 1 { n } else { fibonacci(n - 1) + fibonacci(n - 2) }
}

println(fibonacci(10))

let mut counter = 0
while counter < 5 {
    println(counter)
    counter = counter + 1
}
```

## Changes by Stage

### 1. ferric_common Additions

Add to `TokenKind`:
```rust
Mut,           // mut keyword
While,         // while keyword
Loop,          // loop keyword
Break,         // break keyword
Continue,      // continue keyword
FloatLit(f64), // float literals
AndAnd,        // &&
OrOr,          // ||
LtEq,          // <=
GtEq,          // >=
```

Add to `Ty`:
```rust
Float,  // float type
```

Add to `Stmt`:
```rust
Assign {
    target: Expr,  // must be a variable
    value: Expr,
    id: NodeId,
    span: Span,
}
```

Add to `Expr`:
```rust
While {
    cond: Box<Expr>,
    body: Box<Expr>,
    id: NodeId,
    span: Span,
},
Loop {
    body: Box<Expr>,
    id: NodeId,
    span: Span,
},
Break {
    id: NodeId,
    span: Span,
},
Continue {
    id: NodeId,
    span: Span,
},
```

Add to `ResolveError`:
```rust
AssignToImmutable { name: Symbol, span: Span },
BreakOutsideLoop { span: Span },
ContinueOutsideLoop { span: Span },
ReturnOutsideFn { span: Span },
```

### 2. ferric_lexer Additions

**Add to keyword recognition:**
```rust
"mut" => TokenKind::Mut,
"while" => TokenKind::While,
"loop" => TokenKind::Loop,
"break" => TokenKind::Break,
"continue" => TokenKind::Continue,
```

**Add float literal lexing:**
```rust
fn lex_number(&mut self) -> Token {
    // Parse digits
    // If '.' followed by digits, parse as float
    // Otherwise parse as int
}
```

**Add multi-char operators:**
```rust
'&' => {
    if self.peek() == Some('&') {
        self.advance();
        TokenKind::AndAnd
    } else {
        // error or single &
    }
}
'|' => {
    if self.peek() == Some('|') {
        self.advance();
        TokenKind::OrOr
    } else {
        // error or single |
    }
}
'<' => {
    if self.peek() == Some('=') {
        self.advance();
        TokenKind::LtEq
    } else {
        TokenKind::Lt
    }
}
'>' => {
    if self.peek() == Some('=') {
        self.advance();
        TokenKind::GtEq
    } else {
        TokenKind::Gt
    }
}
```

### 3. ferric_parser Additions

**Add to statement parsing:**
```rust
fn parse_stmt(&mut self) -> Option<Stmt> {
    // ... existing let_stmt
    // Add: assignment stmt
    if self.check_assignment() {
        return Some(self.parse_assignment());
    }
    // ... existing expr_stmt
}

fn parse_assignment(&mut self) -> Stmt {
    let target = self.parse_expr();
    self.expect(TokenKind::Eq, "expected '=' in assignment");
    let value = self.parse_expr();
    // Create Stmt::Assign
}
```

**Add to expression parsing:**
```rust
fn parse_primary(&mut self) -> Expr {
    match self.peek().kind {
        TokenKind::While => self.parse_while(),
        TokenKind::Loop => self.parse_loop(),
        TokenKind::Break => self.parse_break(),
        TokenKind::Continue => self.parse_continue(),
        TokenKind::FloatLit(f) => {
            let token = self.advance();
            Expr::Literal {
                value: Literal::Float(f),
                id: self.node_id_gen.next(),
                span: token.span,
            }
        }
        // ... existing cases
    }
}

fn parse_while(&mut self) -> Expr { ... }
fn parse_loop(&mut self) -> Expr { ... }
fn parse_break(&mut self) -> Expr { ... }
fn parse_continue(&mut self) -> Expr { ... }
```

**Add `let mut` parsing:**
```rust
fn parse_let_stmt(&mut self) -> Stmt {
    self.expect(TokenKind::Let, "expected 'let'");
    let mutable = if self.check(TokenKind::Mut) {
        self.advance();
        true
    } else {
        false
    };
    // ... rest of let parsing
}
```

### 4. ferric_resolve Additions

**Track mutability:**
```rust
struct Binding {
    def_id: DefId,
    mutable: bool,  // track this
    span: Span,
}
```

**Track loop depth:**
```rust
struct Resolver {
    // ... existing fields
    loop_depth: u32,
    fn_depth: u32,
}
```

**Check assignment legality:**
```rust
fn resolve_stmt(&mut self, stmt: &Stmt) {
    match stmt {
        Stmt::Assign { target, value, .. } => {
            // Resolve value first
            self.resolve_expr(value);

            // Check target is a variable
            if let Expr::Variable { name, id, span } = target {
                // Look up binding
                if let Some(binding) = self.lookup(*name) {
                    if !binding.mutable {
                        self.errors.push(ResolveError::AssignToImmutable {
                            name: *name,
                            span: *span,
                        });
                    }
                    self.resolutions.insert(*id, binding.def_id);
                } else {
                    // undefined variable error
                }
            } else {
                // can only assign to variables
                self.errors.push(...);
            }
        }
        // ... existing cases
    }
}
```

**Check break/continue legality:**
```rust
fn resolve_expr(&mut self, expr: &Expr) {
    match expr {
        Expr::Break { span, .. } => {
            if self.loop_depth == 0 {
                self.errors.push(ResolveError::BreakOutsideLoop { span: *span });
            }
        }
        Expr::Continue { span, .. } => {
            if self.loop_depth == 0 {
                self.errors.push(ResolveError::ContinueOutsideLoop { span: *span });
            }
        }
        Expr::While { body, .. } | Expr::Loop { body, .. } => {
            self.loop_depth += 1;
            self.resolve_expr(body);
            self.loop_depth -= 1;
        }
        Expr::Return { span, .. } => {
            if self.fn_depth == 0 {
                self.errors.push(ResolveError::ReturnOutsideFn { span: *span });
            }
        }
        // ... existing cases
    }
}
```

### 5. ferric_typecheck Additions

**Add Float type:**
```rust
fn check_expr(&mut self, expr: &Expr) -> Ty {
    match expr {
        Expr::Literal { value: Literal::Float(_), .. } => Ty::Float,
        // ... existing cases
    }
}
```

**Check while condition is Bool:**
```rust
Expr::While { cond, body, .. } => {
    let cond_ty = self.check_expr(cond);
    self.unify(&Ty::Bool, &cond_ty, cond.span());
    let body_ty = self.check_expr(body);
    Ty::Unit  // while expressions return Unit
}
```

**Check assignment:**
```rust
Stmt::Assign { target, value, .. } => {
    let target_ty = self.check_expr(target);
    let value_ty = self.check_expr(value);
    self.unify(&target_ty, &value_ty, value.span());
}
```

**Check if/else branches match:**
```rust
Expr::If { then_branch, else_branch, .. } => {
    let then_ty = self.check_expr(then_branch);
    if let Some(else_br) = else_branch {
        let else_ty = self.check_expr(else_br);
        self.unify(&then_ty, &else_ty, else_br.span())
    } else {
        Ty::Unit
    }
}
```

### 6. ferric_vm Additions

**Add Float to Value:**
```rust
pub enum Value {
    Int(i64),
    Float(f64),  // add this
    Bool(bool),
    Str(String),
    Unit,
    Fn(DefId),
}

impl Value {
    pub fn new_float(f: f64) -> Self { Value::Float(f) }
}
```

**Add control flow to evaluator:**
```rust
// Use a ControlFlow type
enum ControlFlow {
    Continue,
    Break,
    Return(Value),
}

fn eval_expr(&mut self, expr: &Expr) -> Result<Value, RuntimeError> {
    match expr {
        Expr::While { cond, body, .. } => {
            loop {
                let cond_val = self.eval_expr(cond)?;
                if !cond_val.as_bool()? {
                    break;
                }
                match self.eval_expr(body) {
                    Err(ControlFlow::Break) => break,
                    Err(ControlFlow::Continue) => continue,
                    Err(e) => return Err(e),
                    Ok(_) => {}
                }
            }
            Ok(Value::new_unit())
        }
        Expr::Loop { body, .. } => {
            loop {
                match self.eval_expr(body) {
                    Err(ControlFlow::Break) => break,
                    Err(ControlFlow::Continue) => continue,
                    Err(e) => return Err(e),
                    Ok(_) => {}
                }
            }
            Ok(Value::new_unit())
        }
        Expr::Break { .. } => Err(ControlFlow::Break),
        Expr::Continue { .. } => Err(ControlFlow::Continue),
        // ... existing cases
    }
}
```

**Add assignment:**
```rust
Stmt::Assign { target, value, .. } => {
    let val = self.eval_expr(value)?;
    if let Expr::Variable { id, .. } = target {
        let def_id = self.resolve.resolutions[id];
        // Update in environment
        self.env_stack.last_mut().unwrap().insert(def_id, val);
    }
    Ok(())
}
```

### 7. ferric_stdlib Additions

Add new conversion functions:
```rust
fn builtin_float_to_str(args: &[Value]) -> Result<Value, String> {
    check_arg_count(args, 1)?;
    let f = expect_float(&args[0])?;
    Ok(Value::new_str(f.to_string()))
}

fn builtin_bool_to_str(args: &[Value]) -> Result<Value, String> {
    check_arg_count(args, 1)?;
    let b = expect_bool(&args[0])?;
    Ok(Value::new_str(b.to_string()))
}

fn builtin_int_to_float(args: &[Value]) -> Result<Value, String> {
    check_arg_count(args, 1)?;
    let n = expect_int(&args[0])?;
    Ok(Value::new_float(n as f64))
}
```

Register them in `register_stdlib()`.

### 8. ferric_diagnostics REPLACEMENT

**This is a full stage replacement.**

Replace the M1 line-number-only renderer with span-annotated rendering:

```
// Before (M1)
error at line 5: undefined variable `x`

// After (M2)
error: undefined variable `x`
  --> main.fe:5:10
   |
 5 |     let y = x + 1
   |             ^ not found in this scope
```

**New implementation structure:**
```rust
pub struct Renderer {
    source: String,
    line_starts: Vec<usize>,  // pre-computed for O(1) lookup
}

impl Renderer {
    pub fn new(source: String) -> Self {
        let line_starts = compute_line_starts(&source);
        Self { source, line_starts }
    }

    fn span_to_line_col(&self, span: Span) -> (usize, usize) {
        // Binary search in line_starts
    }

    fn extract_line(&self, line: usize) -> &str {
        // Extract source line
    }

    pub fn render_lex_error(&self, error: &LexError) -> String {
        // Render with span annotation
    }

    // ... similar for all error types
}

fn compute_line_starts(source: &str) -> Vec<usize> {
    let mut starts = vec![0];
    for (i, ch) in source.char_indices() {
        if ch == '\n' {
            starts.push(i + 1);
        }
    }
    starts
}
```

**Critical: This replacement requires ZERO changes to any other stage.**
All stages already emit Spans (Rule 5), so the new renderer just uses them.

## Acceptance Criteria

- [ ] All M2 language features work correctly
- [ ] Fibonacci example runs and produces correct output
- [ ] While loop with mutable counter works
- [ ] `let mut` and assignment work
- [ ] Break and continue work in loops
- [ ] Break/continue outside loops produce errors
- [ ] Assignment to immutable variable produces error
- [ ] Float literals and Float type work
- [ ] Diagnostics replacement requires **zero changes to other stages**
- [ ] Span-annotated errors render correctly
- [ ] All M1 tests still pass
- [ ] All new M2 tests pass

## Test Cases

1. Recursive fibonacci works
2. Iterative counter with while works
3. `loop { break }` terminates
4. `loop { if x { break } }` works
5. `break` outside loop produces error with correct span
6. `let x = 1; x = 2` produces "immutable" error
7. `let mut x = 1; x = 2` works
8. Float arithmetic: `1.5 + 2.5` = `4.0`
9. `if` as expression: `let x = if true { 1 } else { 2 }` binds 1 to x
10. All error types render with span annotations

## Critical Architecture Validation

**This milestone proves Rule 5 pays off:**
- Diagnostics is completely replaced
- New implementation has richer rendering
- **Zero changes required to lexer, parser, resolver, type checker, or VM**
- This is only possible because all errors carried Spans from M1

Document this success in the replacement log.

## Notes for Agent
- This is additive work - extend, don't replace
- Test each feature in isolation before integration
- Make sure loop depth tracking is correct
- Validate the diagnostics replacement requires zero stage changes
- Update all error rendering to use the new format
- Keep the old M1 tests passing - this is regression testing
