# Coercion — Task 2: Stdlib `To<T>` Trait + Orphan Rule

> Adds the stdlib `trait To<T>`, registers built-in impls, and enforces the
> orphan rule for `To` in `ferric_traits`.
>
> **Prerequisite:** [Task 1](coercion-01-common-types.md) must be complete.
>
> See [coercion-00-overview.md](coercion-00-overview.md) for design decisions.

---

## What this task does

Defines the `To<T>` trait in stdlib, registers two built-in impls
(`To<Str> for ShellOutput`, `To<Float> for Int`), and adds orphan-rule
enforcement so user-defined impls in third-party crates can't insert
surprise coercions between types they don't own.

This task does **not** make the type checker use `To` for implicit coercion —
that's Task 3. After this task, the trait exists and impls compile, but
calling `.to()` is the only way to use it (explicit coercion).

---

## The trait

Defined in `ferric_stdlib`:

```ferric
trait To<T> {
    fn to(self) -> T
}
```

Generic in `T`. One impl per conversion pair. No supertrait requirements.
No default methods.

Register the trait in `ferric_stdlib` at startup, alongside other built-in
traits. The trait's `DefId` should be discoverable by `ferric_traits` /
`ferric_infer` for coercion lookups in Task 3 — pick whichever pattern
matches existing built-in trait registration.

---

## Built-in impls

Registered in `ferric_stdlib` at startup:

```ferric
impl To<Str> for ShellOutput {
    fn to(self) -> Str { self.stdout }
}

impl To<Float> for Int {
    fn to(self) -> Float { int_to_float(n: self) }
}
```

Both impls' `to` methods get their own `DefId`s. The compiler in Task 4 will
emit a `Call` to the resolved `to` method — same machinery as any other trait
method call.

If `ShellOutput` is currently a `Value::ShellOutput` runtime-only construct
without a corresponding stdlib type, surface it as a stdlib type so the impl
can be declared. Check `ferric_stdlib` for how shell-related types are
currently exposed.

---

## Orphan rule (in `ferric_traits`)

When registering a `To<T>` impl, enforce:

- The impl is accepted if the crate being compiled owns the source type
  **or** owns `T`.
- If neither: emit `ResolveError::OrphanImpl { trait_name: "To", source,
  target, span }`.

If `ferric_traits` already has orphan-rule logic for other traits, extend it
generically rather than special-case `To`. The orphan rule should apply to
all impls of any trait the compiler doesn't own (consistent coherence).

For built-in impls (`To<Str> for ShellOutput`, `To<Float> for Int`):
`ferric_stdlib` owns both source and target types in both cases, so no
orphan-rule violation occurs.

---

## What "owns the type" means

A crate owns a type if the type's `DefId` was registered by that crate's
declarations (struct/enum/native registration). For built-ins (`Int`, `Str`,
`Float`, `Bool`, `Unit`), `ferric_stdlib` is the owner. For user-defined
types in the program being compiled, the program itself is the owner.

If `ferric_traits` already has an "owning crate" concept for trait impls,
reuse it. If not, the simplest model: every `DefId` carries a crate origin
(stdlib vs. user vs. external). A `To<T> for Source` impl is OK if either
`T` or `Source`'s crate origin matches the current compilation unit.

---

## Done when

- [ ] `trait To<T>` defined in `ferric_stdlib`
- [ ] `impl To<Str> for ShellOutput` registered with a `to` method whose body returns the stdout field
- [ ] `impl To<Float> for Int` registered with a `to` method that calls `int_to_float`
- [ ] Both built-in impls' `DefId`s are discoverable from the `TraitRegistry`
- [ ] Orphan rule enforced when registering `To<T>` impls — non-owning impls emit `ResolveError::OrphanImpl` with span
- [ ] Built-in impls are NOT flagged by the orphan rule (stdlib owns Int/Float/Str/ShellOutput)
- [ ] At least one test demonstrates explicit `.to()` works: `let f: Float = (5).to()`
- [ ] At least one test demonstrates orphan rule rejection: a user `impl To<Float> for Bool` defined in user code where `Bool` and `Float` are stdlib types is accepted (stdlib owns target `Float`); but a hypothetical impl for two non-owned types is rejected
- [ ] `cargo build --workspace` clean
- [ ] All existing tests still pass — explicit `.to()` is new functionality but doesn't change existing behaviour
