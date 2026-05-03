# M9 — Task 3: Type Checker

> Extends the type checker with effect rows, `Eff<R, T>` inference, handler
> clause typing, and the new effect-related error variants.
>
> **Prerequisite:** [Task 1](m9-01-common-types.md) must be complete — the
> `Ty::Eff`, `Ty::EffVar`, and `EffectRef` types this task relies on live in
> `ferric_common`. This task can proceed in parallel with [Task 2](m9-02-lexer-parser.md);
> together they unblock [Task 4](m9-04-lowering.md).
>
> See [m9-00-overview.md](m9-00-overview.md) for milestone-level design
> decisions.

---

## What this task does

Teaches the type checker about effects. Effect rows are inferred alongside
types, using the same Hindley-Milner machinery already in place — effect
variables unify like type variables. After this task, every function has a
typed effect row, every `perform` site requires the effect to be in scope, and
every `handle` block subtracts the handled effect from the row.

Explicit effect annotations on function signatures are supported but never
required — the inferred row is always sufficient.

---

## Effect row inference

Effect rows are unified alongside types. An effect variable `?ε` may unify with
a concrete row `[Async, Gen<Int>]` or remain polymorphic (for functions that
propagate their callee's effects).

Rules:

- `perform Effect::Op(...)` — the current function's effect row must contain
  `Effect`. If the row is a variable, constrain it. If it is a closed row that
  does not contain `Effect`, emit `TypeError::UnhandledEffect`.
- `handle { expr } with { ... }` — the handler removes the handled effect from
  the row. If `expr` has type `Eff<[Async, Config], T>` and the handler handles
  `Async`, the overall `handle` expression has type `Eff<[Config], T>`.
- A function that calls another function propagates the callee's unhandled
  effects into its own row (row polymorphism by default).

Existing rules continue to apply — non-effect types and value types still unify
exactly as they do in M2.5+.

---

## Handler clause typing

For a handler clause `Effect::Op(x) => body`:

- `x` has the type declared in the corresponding `EffectOp::params`.
- `k` has type `Fn(ResumeType) -> Eff<R, T>` where `ResumeType` is
  `EffectOp::resume_type` and `T` is the outer handle expression's value type.
- `body` must have type `Eff<R, T>` (the same row and value type as the whole
  expression).

`resume k with v` requires `v` to have type `ResumeType`; mismatched resume
types emit `TypeError::ResumeTypeMismatch`.

---

## New error variants

```rust
// Add to TypeError:
TypeError::UnhandledEffect    { effect: Symbol, op: Symbol, span: Span },
TypeError::EffectRowMismatch  { expected: Vec<EffectRef>, found: Vec<EffectRef>, span: Span },
TypeError::ResumeTypeMismatch { expected: Ty, found: Ty, span: Span },
```

All variants carry `Span` (Rule 5). They were added to `ferric_common` in Task 1
— this task wires them into the type checker.

---

## Interaction with existing M8 typing

The M8 type checker recognised `async fn` / `.await` directly via `Ty::Async`
and produced `TypeError::AwaitOutsideAsync` etc. After Task 2, the parser
desugars these forms before the type checker runs, so this code path becomes
unreachable for new programs. The existing M8 type-checker logic for
`Ty::Async` / `Ty::Handle` remains in place — Task 7 deletes it once nothing
constructs those types anymore.

---

## Done when

- [ ] `Ty::Eff` and `Ty::EffVar` participate in unification
- [ ] Effect variables (`Ty::EffVar`) unify with rows the same way type variables unify with types
- [ ] `perform Effect::Op(...)` constrains the enclosing function's effect row to contain `Effect`
- [ ] `perform` against a closed row that does not contain the effect emits `TypeError::UnhandledEffect` with span
- [ ] `handle { expr } with { ... }` subtracts the handled effect from the row of `expr`
- [ ] Calling a function with effect row `R1` from inside a function with row `R2` constrains `R2 ⊇ R1` (row polymorphism)
- [ ] Handler clause params (`x` in `Effect::Op(x) =>`) are typed from `EffectOp::params`
- [ ] The continuation variable `k` is bound with type `Fn(ResumeType) -> Eff<R, T>` in handler clause bodies
- [ ] `resume k with v` requires `v` to have the declared `ResumeType`; mismatch emits `TypeError::ResumeTypeMismatch` with span
- [ ] Explicit `Eff<[E1, E2], T>` annotations on function signatures are accepted and unify against inferred rows
- [ ] All new error variants carry `Span` (Rule 5)
- [ ] All M1–M8 typecheck fixtures still pass — sync code is unchanged, async desugaring produces equivalent typing
- [ ] At least one new fixture under `examples/` exercises a custom `effect` declaration with a `handle...with` block end-to-end (or is documented as deferred until Task 6)
