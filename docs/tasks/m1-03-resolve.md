# Task: M1 Name Resolution Implementation

## Objective
Implement the `ferric_resolve` crate that performs name resolution (scope analysis) on the AST, catching undefined variables and duplicate definitions.

## Architecture Context
- Name resolution is the third stage in the pipeline
- It must expose exactly one public function: `pub fn resolve(ast: &ParseResult) -> ResolveResult`
- Assigns slot indices to local variables and functions
- All scope tracking logic must be private

## Public Interface (Non-Negotiable)

```rust
// ferric_resolve/src/lib.rs
pub fn resolve(ast: &ParseResult) -> ResolveResult;
```

## Feature Requirements

### Scope Management
Track scopes as a stack. Each scope contains:
- Variable bindings (name → DefId)
- Mutability status
- Slot assignments (for later VM execution)

### DefId Assignment
Every definition (variable or function) gets a unique `DefId`.
This ID is stable across the pipeline and used by typechecking, compilation, and runtime.

```rust
struct DefIdGen {
    next: u32,
}

impl DefIdGen {
    fn new() -> Self { Self { next: 0 } }
    fn next(&mut self) -> DefId {
        let id = DefId(self.next);
        self.next += 1;
        id
    }
}
```

### Slot Assignment
Every local variable gets a slot index (u32) for stack allocation.
Functions get function slot indices for the function table.

Slots are assigned in declaration order within each scope.
Scopes are independent - nested scopes can reuse slot indices from outer scopes if desired, but for M1, just assign monotonically increasing indices.

### Resolution Mapping
Build a `HashMap<NodeId, DefId>` that maps every variable use (NodeId from the AST) to its definition (DefId).

This map is consumed by later stages to look up which definition a variable refers to.

## Error Types (Rule 5 - All carry Span)

```rust
pub enum ResolveError {
    UndefinedVariable { name: Symbol, span: Span },
    DuplicateDefinition { name: Symbol, first: Span, second: Span },
}
```

For M1, these are the only errors. M2 will add more (e.g., `AssignToImmutable`, `BreakOutsideLoop`).

## Implementation Notes

### Resolver Structure
```rust
struct Resolver {
    scopes: Vec<Scope>,
    def_id_gen: DefIdGen,
    next_slot: u32,
    next_fn_slot: u32,

    // Output
    resolutions: HashMap<NodeId, DefId>,
    def_slots: HashMap<DefId, u32>,
    fn_slots: HashMap<DefId, u32>,
    errors: Vec<ResolveError>,
}

struct Scope {
    bindings: HashMap<Symbol, Binding>,
}

struct Binding {
    def_id: DefId,
    mutable: bool,
    span: Span,  // for error reporting
}

impl Resolver {
    fn new() -> Self;

    fn push_scope(&mut self);
    fn pop_scope(&mut self);
    fn define(&mut self, name: Symbol, mutable: bool, span: Span) -> DefId;
    fn lookup(&self, name: Symbol) -> Option<&Binding>;

    fn resolve_item(&mut self, item: &Item);
    fn resolve_stmt(&mut self, stmt: &Stmt);
    fn resolve_expr(&mut self, expr: &Expr);
}
```

### Resolution Algorithm
1. **For each function definition:**
   - Create a `DefId` for the function
   - Assign it a function slot
   - Push a new scope
   - Define each parameter in the scope (assign slot indices)
   - Resolve the function body
   - Pop the scope

2. **For each let binding:**
   - Resolve the initializer expression first
   - Create a `DefId` for the binding
   - Assign it a variable slot
   - Define the binding in the current scope
   - Check for duplicate definitions

3. **For each variable use:**
   - Look up the name in the scope stack (innermost to outermost)
   - If found, add `NodeId → DefId` to resolutions map
   - If not found, emit `UndefinedVariable` error

4. **For blocks:**
   - Push a new scope
   - Resolve all statements and expressions
   - Pop the scope

### Scope Lookup
Search from innermost to outermost scope:
```rust
fn lookup(&self, name: Symbol) -> Option<&Binding> {
    for scope in self.scopes.iter().rev() {
        if let Some(binding) = scope.bindings.get(&name) {
            return Some(binding);
        }
    }
    None
}
```

## ResolveResult Structure

```rust
pub struct ResolveResult {
    // Maps each variable use (NodeId) to its definition (DefId)
    pub resolutions: HashMap<NodeId, DefId>,

    // Maps each DefId to its stack slot
    pub def_slots: HashMap<DefId, u32>,

    // Maps each function DefId to its function table slot
    pub fn_slots: HashMap<DefId, u32>,

    // Errors encountered during resolution
    pub errors: Vec<ResolveError>,
}
```

## Test Cases

Create unit tests for:
1. Simple variable resolution: `let x = 5; x`
2. Function parameter resolution: `fn foo(x: Int) { x }`
3. Shadowing: `let x = 1; { let x = 2; x }` (inner x shadows outer x)
4. Undefined variable produces error: `let x = y` where y is not defined
5. Duplicate definition produces error: `let x = 1; let x = 2`
6. Nested scopes: variables from outer scope are visible in inner scope
7. Block scope: `{ let x = 1; } x` - x is not visible outside block (error)
8. Function definition creates a new scope

## Acceptance Criteria
- [ ] Public API is exactly `pub fn resolve(ast: &ParseResult) -> ResolveResult`
- [ ] All variable uses are mapped to their definitions via NodeId → DefId
- [ ] All definitions are assigned unique DefIds
- [ ] All local variables are assigned stack slots
- [ ] All functions are assigned function slots
- [ ] Undefined variables produce errors with accurate Spans
- [ ] Duplicate definitions produce errors with both Spans
- [ ] Shadowing works correctly (inner binding hides outer)
- [ ] Block scopes are properly isolated
- [ ] All errors carry Spans
- [ ] Resolver never panics - errors are accumulated
- [ ] All unit tests pass
- [ ] No public exports other than `resolve` function

## Critical Rules to Enforce

### Rule 1 - Stages communicate only through output types
The resolver depends on `ferric_common::ParseResult`, not on `ferric_parser` internals.

### Rule 2 - Exactly one public entry point
Only `resolve()` should be public.

### Rule 5 - Every error carries a Span
Every `ResolveError` variant must include a `Span`.

### Rule 4 - No mutable global state
All state is local to the `Resolver` struct, no globals.

## Notes for Agent
- Keep the scope stack simple - just a Vec of HashMaps is fine
- Make sure to track the span of each definition for error reporting
- DefIds must be unique and stable - they're used throughout the rest of the pipeline
- Slot assignment is straightforward for M1 - just count up
- Test shadowing carefully - it's a common source of bugs
