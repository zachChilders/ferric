# Task: M1 Type Checker Implementation

## Objective
Implement the `ferric_typecheck` crate with a simple recursive type checker. Uses `Ty::Unknown` as an intentional escape hatch for features not yet fully implemented.

## Architecture Context
- Type checking is the fourth stage in the pipeline
- It must expose exactly one public function: `pub fn typecheck(ast: &ParseResult, resolve: &ResolveResult) -> TypeResult`
- This implementation will be replaced in M3 with a full Hindley-Milner inference engine
- Keep it simple - this is intentional technical debt

## Public Interface (Non-Negotiable)

```rust
// ferric_typecheck/src/lib.rs
pub fn typecheck(ast: &ParseResult, resolve: &ResolveResult) -> TypeResult;
```

## Feature Requirements

### Type System (M1)
From `ferric_common::Ty`:
```rust
pub enum Ty {
    Int,
    Float,
    Bool,
    Str,
    Unit,
    Fn { params: Vec<Ty>, ret: Box<Ty> },
    Unknown,  // escape hatch - use liberally in M1
}
```

### Unknown as Escape Hatch
Any expression the checker doesn't understand yet resolves to `Ty::Unknown`.
`Ty::Unknown` is accepted everywhere without error.

This is **intentional** - it allows M1 to ship without a complete type system.
The entire `Unknown` variant is removed in M3.

Examples where Unknown is acceptable in M1:
- Complex nested expressions
- Edge cases in type inference
- When in doubt, use Unknown

### Type Checking Rules (M1)

**Literals:**
- Integer literals → `Ty::Int`
- Float literals → `Ty::Float`
- String literals → `Ty::Str`
- `true`, `false` → `Ty::Bool`

**Variables:**
- Look up the variable's definition via `resolve.resolutions`
- Look up the definition's type (if it has a type annotation)
- If no annotation, infer from initializer
- If can't infer, use `Ty::Unknown`

**Binary operations:**
- `+`, `-`, `*`, `/`, `%` on `Int` → `Int`
- `+`, `-`, `*`, `/` on `Float` → `Float`
- `+` on `Str` → `Str` (string concatenation)
- `==`, `!=`, `<`, `<=`, `>`, `>=` → `Bool`
- `&&`, `||` on `Bool` → `Bool`
- If operands don't match or are Unknown, emit error or use Unknown

**Unary operations:**
- `-` on `Int` → `Int`
- `-` on `Float` → `Float`
- `!` on `Bool` → `Bool`

**Function calls:**
- Look up the function's type
- Check argument count matches parameter count
- Check argument types match parameter types
- Return type is the function's return type
- If checks fail, emit error but continue

**If expressions:**
- Condition must be `Bool`
- Both branches must have the same type
- If no else branch, type is `Unit`

**Blocks:**
- Type is the type of the final expression
- If no final expression, type is `Unit`

**Return:**
- Type is the type of the returned expression
- Check it matches the function's return type

### Type Environment
Track types of definitions (variables and functions).

```rust
struct TypeEnv {
    def_types: HashMap<DefId, Ty>,
}

impl TypeEnv {
    fn define(&mut self, def_id: DefId, ty: Ty);
    fn lookup(&self, def_id: DefId) -> Option<&Ty>;
}
```

## Error Types (Rule 5)

```rust
pub enum TypeError {
    Mismatch { expected: Ty, found: Ty, span: Span },
    WrongArgumentCount { expected: usize, found: usize, span: Span },
    NotCallable { ty: Ty, span: Span },
}
```

For M1, be lenient. If in doubt, emit a warning-level error or use `Ty::Unknown`.

## Implementation Notes

### TypeChecker Structure
```rust
struct TypeChecker<'a> {
    ast: &'a ParseResult,
    resolve: &'a ResolveResult,
    env: TypeEnv,
    current_fn_ret: Option<Ty>,  // for checking return statements

    // Output
    node_types: HashMap<NodeId, Ty>,
    errors: Vec<TypeError>,
}

impl<'a> TypeChecker<'a> {
    fn new(ast: &'a ParseResult, resolve: &'a ResolveResult) -> Self;

    fn check_item(&mut self, item: &Item);
    fn check_stmt(&mut self, stmt: &Stmt);
    fn check_expr(&mut self, expr: &Expr) -> Ty;

    fn unify(&mut self, expected: &Ty, found: &Ty, span: Span) -> Ty;
}
```

### Type Checking Algorithm
1. **For each function:**
   - Build a type for the function: `Fn { params, ret }`
   - Store it in the environment
   - Set `current_fn_ret` to the return type
   - Check the function body
   - Verify the body type matches the return type

2. **For each let binding:**
   - Check the initializer expression
   - If there's a type annotation, verify the initializer matches
   - Store the type in the environment

3. **For each expression:**
   - Recursively check sub-expressions
   - Apply typing rules based on the expression kind
   - Store the resulting type in `node_types`
   - If type checking fails, use `Ty::Unknown` and emit an error

### Unification
Simple structural equality for M1. No inference, no constraint solving.

```rust
fn unify(&mut self, expected: &Ty, found: &Ty, span: Span) -> Ty {
    if expected == found {
        expected.clone()
    } else if matches!(expected, Ty::Unknown) || matches!(found, Ty::Unknown) {
        Ty::Unknown  // escape hatch
    } else {
        self.errors.push(TypeError::Mismatch {
            expected: expected.clone(),
            found: found.clone(),
            span,
        });
        Ty::Unknown
    }
}
```

## Test Cases

Create unit tests for:
1. Integer literal has type Int
2. Function with annotated parameters type-checks correctly
3. Binary operation `1 + 2` has type Int
4. String concatenation `"a" + "b"` has type Str
5. If expression with Bool condition type-checks
6. Type mismatch produces error: `let x: Int = "hello"`
7. Function call with correct arguments type-checks
8. Function call with wrong argument count produces error
9. Unknown is accepted everywhere (escape hatch works)

## Acceptance Criteria
- [ ] Public API is exactly `pub fn typecheck(ast: &ParseResult, resolve: &ResolveResult) -> TypeResult`
- [ ] All basic type checking rules are implemented
- [ ] `Ty::Unknown` is used liberally as an escape hatch
- [ ] Every expression gets a type in `node_types` map
- [ ] All errors carry Spans
- [ ] Type checker never panics - errors are accumulated
- [ ] All unit tests pass
- [ ] No public exports other than `typecheck` function
- [ ] Documentation clearly states Unknown will be removed in M3

## Critical Rules to Enforce

### Rule 1 - Stages communicate only through output types
The type checker depends on `ParseResult` and `ResolveResult`, not on parser or resolver internals.

### Rule 2 - Exactly one public entry point
Only `typecheck()` should be public.

### Rule 5 - Every error carries a Span
Every `TypeError` variant must include a `Span`.

## Notes for Agent
- Don't over-engineer this - it's temporary
- When in doubt, use `Ty::Unknown` and move on
- Document why you used Unknown in comments
- The goal is to ship M1 quickly, not to build a perfect type system
- M3 will replace this entire crate, so simplicity > completeness
- Focus on making the common cases work correctly
