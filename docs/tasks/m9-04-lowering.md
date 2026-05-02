# M9 — Task 4: `ferric_effects` Lowering Pass

> Creates the new `ferric_effects` crate. It replaces `ferric_async` in the same
> pipeline slot, performs a selective CPS transform across handler boundaries,
> and emits an AST in which all `Expr::Perform` / `Expr::Handle` / `Expr::Resume`
> nodes have been compiled away.
>
> **Prerequisites:** [Task 1](m9-01-common-types.md), [Task 2](m9-02-lexer-parser.md),
> and [Task 3](m9-03-typecheck.md) must all be complete — this task reads
> `ParseResult` and `TypeResult` and produces `EffectResult`.
>
> See [m9-00-overview.md](m9-00-overview.md) for milestone-level design
> decisions.

---

## What this task does

Adds the new `ferric_effects` workspace crate. Its sole public entry point
takes the parser's AST and the type checker's output and returns an
`EffectResult` whose `.ast` field is an ordinary `ParseResult` — every effects
construct in the input is replaced with handler-dispatch calls and continuation
plumbing that the existing compiler can compile and the existing VM
(plus the four new opcodes from Task 5) can run.

This task does **not** wire `ferric_effects` into `main.rs`; that one-line
change happens once Task 5 lands the VM-side opcode support.

---

## Public entry point

```rust
// ferric_effects/src/lib.rs
pub fn lower_effects(ast: &ParseResult, types: &TypeResult) -> EffectResult;
```

This is the **only** public function in the crate. Everything else is private.

---

## What the lowering does

The lowering pass converts `Expr::Perform` and `Expr::Handle` into sequences of
ordinary bytecode-friendly constructs. It does not emit bytecode directly — it
produces a `ParseResult` with all effect expressions replaced by handler dispatch
calls that the compiler and VM will execute using the four new opcodes from
Task 1.

The core transform is a **selective CPS transform** over handler boundaries:

1. For each `Expr::Handle` node, identify all `Expr::Perform` calls (for the
   handled effects) reachable within the body without crossing another handler
   for the same effect.
2. Split each such function at perform points into before/after segments —
   these become closures that capture the live variables.
3. Replace `Expr::Perform` with a call that pushes the captured continuation
   and dispatches to the handler clause.
4. `Expr::Resume` becomes a call that pops and invokes the stored continuation.

For effects that have no handler within the compilation unit (because they are
propagated out), the lowering pass emits `Op::Perform` directly — the VM's
handler stack will resolve them at runtime.

---

## Output invariant

```
1. No Expr::Perform variants remain for effects that are handled within the same compilation unit.
2. No Expr::Handle variants remain — replaced by PushHandler / PopHandler boundary calls.
3. No Expr::Resume variants remain — replaced by Resume calls with captured ContinuationId.
4. All generated names are unique (double-underscore prefix).
```

Add debug-mode assertions for all four at the end of `lower_effects`. The
assertions exist so the compiler and VM can rely on the invariant without
re-walking the tree.

---

## Errors emitted

```rust
EffectError::UnhandledEffect      // perform with no statically resolvable handler in a closed row
EffectError::ResumedTwice         // detected statically when possible (e.g. resume k inside a loop)
EffectError::ResumeOutsideHandler // belt-and-suspenders alongside Task 2's parser check
```

`EffectWarning::ContinuationDropped` — emitted when a handler clause binds `k`
but never resumes it. This is sometimes intentional (one-shot abort) so it is a
warning, not an error.

All errors and warnings carry `Span` (Rule 5).

---

## `main.rs` changes

> These are documented here for completeness. The actual `main.rs` edit lands
> once Task 5 ships the new opcodes — until then, `ferric_async` is still the
> stage in use.

```rust
use ferric_effects::lower_effects;   // replaces: use ferric_async::lower_async

let effect_result = lower_effects(&parse_result, &type_result);
if !effect_result.errors.is_empty() { render_and_exit(&effect_result.errors); }
for w in &effect_result.warnings { renderer.warn(w); }
let program = compile(&effect_result.ast, &resolve_result, &type_result);
//                     ^^^^^^^^^^^^^^^^^ was: &async_result.ast
```

Total delta: one import swap, one type rename, one argument change. No other
changes to `main.rs` or any other stage.

---

## Done when

- [ ] `ferric_effects` crate exists in the workspace `Cargo.toml`
- [ ] `pub fn lower_effects(ast: &ParseResult, types: &TypeResult) -> EffectResult` is the only public function
- [ ] Every `Expr::Handle` whose body performs handled effects is rewritten — no `Expr::Handle` survives in the output AST
- [ ] Every `Expr::Perform` for an effect handled within the same compilation unit is rewritten — no such `Expr::Perform` survives
- [ ] Every `Expr::Resume` is rewritten into a `Resume` opcode call site — no `Expr::Resume` survives
- [ ] `Expr::Perform` for effects with no in-unit handler is left in place; the compiler will lower it to `Op::Perform`
- [ ] All four output invariants are asserted in debug builds at the end of `lower_effects`
- [ ] Generated names use a double-underscore prefix and never collide with user names
- [ ] `EffectError::UnhandledEffect` is emitted for closed-row perform sites with no resolvable handler, with span
- [ ] `EffectError::ResumedTwice` is emitted when a static resume-twice pattern is detected, with span
- [ ] `EffectError::ResumeOutsideHandler` is emitted as a defence-in-depth check, with span
- [ ] `EffectWarning::ContinuationDropped` is emitted when a handler clause never resumes `k`, with span
- [ ] Lowered AST passes the existing compiler unchanged — no new compiler features required
- [ ] All M1–M8 programs that desugar to effects still compile and produce the same output (checked once Task 5 lands)
