# M9 — Task 1: Common Types

> **Do this task first.** It adds all new types to `ferric_common` that every
> other M9 task depends on. Tasks 2, 3, 4, 5, 6, and 7 may not begin until this
> task is complete and the codebase compiles cleanly.
>
> See [m9-00-overview.md](m9-00-overview.md) for milestone-level design
> decisions and pipeline shape.

---

## What this task does

Adds the AST nodes, stage output types, `Ty` variants, and error/warning
variants that the algebraic effects system requires. No behaviour changes —
this task only extends data structures. Nothing is wired up yet; that happens
in Tasks 2–7.

This task does **not** remove the old M8 async types (`AsyncFnItem`, `AwaitExpr`,
`AsyncBlockExpr`, `Ty::Async`, `Ty::Handle`, `AsyncResult`, `AsyncLowerError`).
They are removed in Task 7 once `ferric_effects` has fully replaced
`ferric_async`.

---

## Future removal (documented now, executed in Task 7)

These M8 types are retired at the end of M9:

- `AsyncFnItem`, `AwaitExpr`, `AsyncBlockExpr` from the `Item` / `Expr` enums
- `Ty::Async`, `Ty::Handle`
- `AsyncResult`, `AsyncLowerError`
- `Op::Await`

Do **not** delete them in this task — the parser still emits them until Task 2
completes the desugaring. Task 7 deletes them once nothing references them.

---

## ferric_common — additions required

### 1. Effect declaration AST node

```rust
pub struct EffectDecl {
    pub span: Span,
    pub name: Symbol,
    pub type_params: Vec<Symbol>,      // e.g. T in Gen<T>
    pub ops: Vec<EffectOp>,
}

pub struct EffectOp {
    pub span:        Span,
    pub name:        Symbol,
    pub params:      Vec<(Symbol, Ty)>,
    pub resume_type: Ty,               // what perform evaluates to
}
```

Add `Item::EffectDecl(EffectDecl)` to the `Item` enum.

### 2. `perform` expression

```rust
pub struct PerformExpr {
    pub span:   Span,
    pub effect: Symbol,                // e.g. Async
    pub op:     Symbol,                // e.g. Suspend
    pub args:   Vec<(Symbol, Expr)>,   // named args, matching EffectOp::params
}
```

Add `Expr::Perform(PerformExpr)` to the `Expr` enum.

### 3. Handler block expression

```rust
pub struct HandleExpr {
    pub span:    Span,
    pub body:    Box<Expr>,
    pub clauses: Vec<HandlerClause>,
}

pub struct HandlerClause {
    pub span:    Span,
    pub effect:  Symbol,
    pub op:      Symbol,
    pub params:  Vec<Symbol>,          // bound arg names
    pub cont:    Symbol,               // the continuation variable (default: k)
    pub handler: Box<Expr>,
}
```

Add `Expr::Handle(HandleExpr)` to the `Expr` enum.

### 4. `resume` expression

```rust
pub struct ResumeExpr {
    pub span:  Span,
    pub cont:  Symbol,                 // must refer to the handler clause's cont var
    pub value: Box<Expr>,
}
```

Add `Expr::Resume(ResumeExpr)` to the `Expr` enum.

### 5. Effect row type

```rust
// Add to Ty enum:
Ty::Eff(Vec<EffectRef>, Box<Ty>),      // Eff<[Async, Gen<Int>], Str>
Ty::EffVar(Symbol),                    // unification variable for effect rows
```

`EffectRef` names an effect and its type arguments:

```rust
pub struct EffectRef {
    pub name: Symbol,
    pub args: Vec<Ty>,
}
```

### 6. Stage output type

```rust
pub struct EffectResult {
    pub ast:      ParseResult,
    pub errors:   Vec<EffectError>,
    pub warnings: Vec<EffectWarning>,
}

pub enum EffectError {
    UnhandledEffect      { effect: Symbol, op: Symbol, span: Span },
    ResumedTwice         { cont: Symbol, span: Span },
    ResumeOutsideHandler { span: Span },
    EffectRowMismatch    { expected: Vec<EffectRef>, found: Vec<EffectRef>, span: Span },
}

pub enum EffectWarning {
    ContinuationDropped { cont: Symbol, span: Span },  // k bound but never resumed
}
```

All error variants carry `Span` (Rule 5).

`EffectResult::ast` is a full `ParseResult` — same type as the parser's output.
Downstream stages (`ferric_compiler`) accept a `&ParseResult`, so the lowered
AST slots in without any signature change.

### 7. New bytecode opcodes

```rust
// Add to Op enum in ferric_common:
Op::Perform,       // pops (effect_tag: u16, op_tag: u8, arg_count: u8), pushes resume value
Op::PushHandler,   // pops handler table ptr; pushes handler frame onto handler stack
Op::PopHandler,    // pops handler frame from handler stack
Op::Resume,        // pops (cont: ContinuationId, value), resumes the continuation
```

The compiler emits these; the VM interprets them. No other stage sees them.

### 8. `Display` additions for new `Ty` variants

The `Ty::Display` impl in `ferric_common` must cover the new variants. Add arms
now so the build fails immediately if a future variant is added without a
display form:

```rust
// Add to impl Display for Ty:
Ty::Eff(row, t)    => /* render as "Eff<[E1, E2, ...], T>" */,
Ty::EffVar(name)   => write!(f, "?{name}"),
```

### 9. Serialisation and async gate

All new types must:
- Derive `Serialize + Deserialize` (consistent with M2.5 Task 4)
- Derive `Debug + Clone + PartialEq`
- Be `Send + Sync` — add them to the compile-time assertion in
  `ferric_common/src/lib.rs`:

```rust
fn _assert_send_sync() {
    fn check<T: Send + Sync>() {}
    // ... existing checks ...
    check::<EffectDecl>();
    check::<EffectOp>();
    check::<PerformExpr>();
    check::<HandleExpr>();
    check::<HandlerClause>();
    check::<ResumeExpr>();
    check::<EffectRef>();
    check::<EffectResult>();
    check::<EffectError>();
    check::<EffectWarning>();
}
```

---

## Done when

- [ ] `EffectDecl`, `EffectOp` exist in `ferric_common`
- [ ] `PerformExpr`, `HandleExpr`, `HandlerClause`, `ResumeExpr` exist in `ferric_common`
- [ ] `Item::EffectDecl` variant exists
- [ ] `Expr::Perform`, `Expr::Handle`, `Expr::Resume` variants exist
- [ ] `Ty::Eff` and `Ty::EffVar` variants exist
- [ ] `EffectRef` exists
- [ ] `EffectResult` exists with `ast`, `errors`, `warnings` fields
- [ ] `EffectError` enum exists with all four variants
- [ ] `EffectWarning` enum exists with `ContinuationDropped` variant
- [ ] `Op::Perform`, `Op::PushHandler`, `Op::PopHandler`, `Op::Resume` exist in the bytecode `Op` enum
- [ ] `Ty::Display` covers `Ty::Eff` and `Ty::EffVar` — missing arm is a compile error
- [ ] All new types derive `Serialize + Deserialize + Debug + Clone + PartialEq`
- [ ] All new types are covered by the `Send + Sync` compile-time assertion
- [ ] Codebase compiles cleanly with no warnings
- [ ] All M1–M8 tests still pass (no behaviour change — data structures only)
