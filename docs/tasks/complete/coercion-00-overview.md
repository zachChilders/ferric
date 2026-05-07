# Implicit Coercion via `To<T>`: Overview

> Adds a `To<T>` trait and teaches the type checker to invoke it implicitly at
> call sites when a type mismatch would otherwise occur. The user never writes
> `.to()` — the compiler inserts it. Most cases are static; the runtime is
> unchanged.
>
> Task 1 settles the shared `TypeResult` field and error variants. Task 2 ships
> the stdlib trait + built-in impls + orphan-rule enforcement. Task 3 wires
> the coercion lookup into the type checker. Task 4 emits the call in the
> compiler and surfaces hints in the LSP.

---

## Goal

Pass an `Int` where `Float` is expected without writing `as Float`. Pass a
`ShellOutput` where `Str` is expected without unwrapping `.stdout`. The
mechanism is a stdlib `trait To<T> { fn to(self) -> T }`; the type checker
inserts a one-hop call to `.to()` when normal unification fails and exactly
one valid `To` impl exists.

---

## Design decisions (settled — do not relitigate)

- **Call boundaries only.** Implicit coercion fires only when passing an
  argument to a function. Assignment targets, return expressions, and `let`
  bindings are never implicitly coerced. A type mismatch at those sites is
  still an error.
- **No chaining.** A coercion is at most one hop. `A → B` plus `B → C` does
  not implicitly coerce `A` to `C`. The user must write `.to()` explicitly
  for the second hop.
- **Exactly one candidate.** Zero or two-or-more eligible `To` impls both
  produce errors. Ambiguity is never silently resolved.
- **Orphan rule enforced.** A `To<T>` impl must be defined in the crate that
  owns either the source type or `T`. Same coherence rule as other traits.
- **User-extensible.** No whitelist. Any `To<T>` impl that follows the orphan
  rule is eligible. This is what makes the feature principled rather than ad hoc.
- **Coercion is a fallback, not first resort.** The check happens *after*
  normal unification fails. If types unify directly, no coercion lookup occurs.
- **Compiles to an ordinary method call.** No new opcodes. The compiler emits
  a `Call(1)` to the resolved `to` function before pushing the argument for the
  outer call.

---

## Pipeline impact

No new stages. The only stage I/O contract change is `TypeResult::coercions:
HashMap<NodeId, DefId>`. `main.rs` is untouched.

```
lex → parse → resolve → typecheck (now records coercions) → lower_effects → compile (now emits .to() calls) → run
```

---

## Task breakdown

| Task | What it does | Prerequisite |
|------|--------------|--------------|
| [Task 1](coercion-01-common-types.md) | Add `TypeResult::coercions` field, `TypeError::AmbiguousCoercion`, `ResolveError::OrphanImpl` | None — do first |
| [Task 2](coercion-02-stdlib-trait.md) | `trait To<T>` in stdlib, built-in impls (`To<Str> for ShellOutput`, `To<Float> for Int`), orphan-rule enforcement in `ferric_traits` | Task 1 |
| [Task 3](coercion-03-typecheck.md) | Type checker: coercion lookup at call sites, exactly-one-candidate enforcement, no-chaining enforcement | Tasks 1, 2 |
| [Task 4](coercion-04-compile-lsp.md) | Compiler emits `.to()` call from `TypeResult::coercions`; LSP inlay hint renders `as T` at coerced argument positions | Task 3 |

---

## Built-in impls

```ferric
impl To<Str> for ShellOutput {
    fn to(self) -> Str { self.stdout }
}

impl To<Float> for Int {
    fn to(self) -> Float { int_to_float(n: self) }
}
```

Additional numeric and string coercions can be added later. Each one requires
a conscious decision — the trait does not auto-derive.

---

## Done when

- [ ] All Task 1–4 checklists complete
- [ ] `$ cmd` result passes as `Str` to functions expecting `Str` without explicit cast
- [ ] `Int` value passes as `Float` to functions expecting `Float` without explicit cast
- [ ] Chained coercions (e.g. two-hop `A → B → C`) emit `TypeError::TypeMismatch`
- [ ] Ambiguous coercions (two candidate `To` impls) emit `TypeError::AmbiguousCoercion` with span and candidate list
- [ ] `let x: Float = an_int` (assignment to annotation) is still a `TypeError::TypeMismatch` — call-boundary-only
- [ ] LSP inlay hints render `as T` at every coerced argument position
- [ ] `To<T>` impls in third-party crates without the source type or `T` emit `ResolveError::OrphanImpl`
- [ ] All existing tests pass — no behaviour change at non-coercion call sites
- [ ] All new error types carry `Span` (Rule 5)
- [ ] Workspace builds clean, `cargo test` and `cargo clippy` are 0 errors / 0 warnings
