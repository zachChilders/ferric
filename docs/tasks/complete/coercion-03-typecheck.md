# Coercion — Task 3: Type Checker Lookup

> Teaches the type checker to attempt a `To<T>` lookup at function call
> argument positions when normal unification fails. Records the chosen impl
> in `TypeResult::coercions`.
>
> **Prerequisites:** [Task 1](coercion-01-common-types.md) and
> [Task 2](coercion-02-stdlib-trait.md) must be complete.
>
> See [coercion-00-overview.md](coercion-00-overview.md) for design decisions.

---

## What this task does

In the type checker (`ferric_infer`), when a `CallExpr` argument's type
doesn't unify with its parameter's type, look up `To<param_type>` impls
where the source is `arg_type`. If exactly one is found, record the
coercion and continue. If zero or many, emit the appropriate error.

After this task, programs like `print_float(x: 5)` (where `print_float` takes
`Float` and `5` is an `Int`) typecheck. The compiler doesn't yet emit the
`.to()` call — Task 4 does that.

---

## Where coercion is attempted

Only at function call argument positions. Concretely: when type-checking a
`CallExpr`, after resolver canonicalisation has put args in definition order,
for each `(arg_type, param_type)` pair where `arg_type ≠ param_type`:

1. Attempt normal unification. If it succeeds, no coercion check.
2. If unification fails:
   - Look up all `impl To<param_type>` in the `TraitRegistry` whose source
     type is `arg_type`.
   - If exactly one impl is found: record the coercion in
     `TypeResult::coercions` keyed by the argument expression's `NodeId`,
     valued by the impl's `to` method `DefId`. Treat the argument's effective
     type as `param_type`. No error.
   - If zero impls: emit `TypeError::TypeMismatch` (existing variant — same
     as current behaviour).
   - If two or more impls: emit `TypeError::AmbiguousCoercion` with the
     candidate `DefId`s and the argument's span.

The coercion check happens **after** normal unification fails — it is a
fallback, not first resort. Same-type calls and calls with normal type
inference success do not trigger any lookup.

---

## Where coercion is NOT attempted

- **Assignments** (`let x: Float = an_int`): a type mismatch is still
  `TypeError::TypeMismatch`. No coercion lookup.
- **Return expressions** (`fn f() -> Float { 5 }`): a type mismatch is still
  `TypeError::TypeMismatch`. No coercion lookup.
- **Method receivers**: `x.foo()` where `x: A` and `foo` is defined for `B`
  is still a type error. Coercion does not apply to the receiver.
- **Operator operands**: `1 + 1.0` (Int + Float) is still a type error. The
  user must write `(1).to() + 1.0` or use explicit syntax. (Or pick whatever
  the existing language semantics are — coercion does not change them.)

A test must verify each of these cases still errors.

---

## No-chaining rule

The coercion lookup is non-recursive. After finding one valid `To` impl and
recording the coercion, the result type is `param_type` — not re-checked for
further coercions. If a second hop would be needed (e.g., `A → B → C`), the
check simply fails with `TypeError::TypeMismatch`.

Concretely: when looking up `To<C>` for source `A`, do NOT also look up
`To<B>` for source `A` and then `To<C>` for source `B`. The lookup is a
single hop with the candidate set restricted to direct `arg_type → param_type`
impls.

---

## Exactly-one-candidate rule

Two-or-more candidates: `TypeError::AmbiguousCoercion`. Test: register
`impl To<Float> for Int` and a hypothetical `impl To<Float> for Int` second
copy via a generic impl that happens to apply (if generic impls exist) or
construct the test with two distinct `To<Float>` impls covering the same
source. The renderer should list all candidate impl source locations.

---

## Lookup mechanics

The `TraitRegistry` is built by `ferric_traits` from `ParseResult` +
`ResolveResult` + the stdlib registration. To find `To<T>` impls:

1. Resolve the trait `To` to its `DefId` (it's a built-in trait registered
   by `ferric_stdlib` in Task 2).
2. Iterate impls of `To` in the registry.
3. Filter by impl's type arg `T = param_type` and impl's `for_type =
   arg_type`.

If the existing `TraitRegistry` API doesn't expose this filter cleanly, add a
small helper method (`fn find_to_impls(&self, source: &Ty, target: &Ty) ->
Vec<DefId>`). This stays inside `ferric_traits`; the type checker just calls it.

---

## Tests

Add type-checker tests covering:

- `print_float(x: 5)` where `print_float(x: Float)` typechecks
  (`Int → Float` coercion); `coercions` map has one entry for the arg `NodeId`.
- `print_str(s: $ "hello")` where `print_str(s: Str)` typechecks
  (`ShellOutput → Str` coercion).
- `print_str(s: 5)` (no `Int → Str` impl) emits `TypeError::TypeMismatch` with
  the existing message.
- A test with two impls registered for the same `(source, target)` pair emits
  `TypeError::AmbiguousCoercion`.
- `let x: Float = 5` emits `TypeError::TypeMismatch` (call boundary only —
  this is an assignment).
- `fn f() -> Float { 5 }` emits `TypeError::TypeMismatch` (return value).
- `print_str(s: 5.0)` where neither direct types nor a one-hop coercion exists
  emits `TypeError::TypeMismatch` (no `To<Str> for Float` registered) — this
  also implicitly tests no-chaining if `Float → Int → Str` would otherwise
  apply.

---

## Done when

- [ ] Type checker attempts `To<T>` lookup only at function call argument positions
- [ ] Lookup happens only after normal unification fails
- [ ] Exactly-one-candidate rule: zero impls → `TypeError::TypeMismatch`, two-or-more → `TypeError::AmbiguousCoercion`
- [ ] Found coercions are recorded in `TypeResult::coercions` keyed by argument `NodeId`
- [ ] No-chaining: two-hop coercions emit `TypeError::TypeMismatch`
- [ ] Assignments (`let x: T = expr`) do not trigger coercion lookup
- [ ] Return expressions do not trigger coercion lookup
- [ ] All `TypeError::AmbiguousCoercion` instances carry `Span` and the candidate `DefId` list
- [ ] At least 6 unit tests covering the cases listed above
- [ ] `cargo build --workspace` clean
- [ ] `cargo test -p ferric_infer` passes
- [ ] All existing tests still pass — type checker behaviour at non-coercion sites is unchanged
