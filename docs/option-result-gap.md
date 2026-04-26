# `Option<T>` / `Result<T, E>` Are Now Native Enums

**Status: resolved.** `Option<T>` and `Result<T, E>` are pre-registered as built-in enums during resolve; user code can construct and pattern-match their variants without redeclaring them. See `examples/m7_option_result/` for a worked example.

The historical context below is preserved for the design notes.

## Implementation summary

- `ferric_stdlib::builtin_enum_table` produces the `(enum, variants)` table consumed by the resolver.
- `ferric_resolve::resolve_with_natives_and_builtins` (and the `_with_imports_` variant) pre-register `Option` and `Result` in `type_defs` / `enum_variants` so `resolve_variant_ref` and the compiler's `variant_index` work uniformly with user-defined enums.
- `ferric_infer` keeps the dedicated `Ty::Option` / `Ty::Result` for nicer messages and bridges variant constructors and patterns through `infer_builtin_variant_ctor` / `check_builtin_variant_pattern`.
- `lookup_user_type` returns `None` for the `Option` / `Result` symbols so the existing `Ty::Enum` path can't reintroduce a mismatch with `Ty::Option` / `Ty::Result`.

## Historical state (pre-implementation)

### Type system (knows about them)

- `ferric_common/src/types.rs:64-67` — `Ty` includes `Option(Box<Ty>)` and `Result(Box<Ty>, Box<Ty>)` as first-class variants.
- `ferric_common/src/ast.rs` — `TypeAnnotation::Generic { head, args }` resolves `Option` and `Result` heads by name in `ferric_infer` / `ferric_traits`.
- The inference engine recognizes `Option<T>` and `Result<T, E>` syntax in annotations and infers them through expressions.

### Resolver / runtime (does not know about them)

- `ferric_resolve` performs no special registration for `Option` or `Result` — no `DefId`s allocated for `Some` / `None` / `Ok` / `Err`.
- `ferric_stdlib` registers no enum machinery for them.
- Pattern matching and constructor calls (`Some(x)`, `None`, `Ok(v)`, `Err(e)`) only work when the user has defined matching enums in their own source.

### Net effect

- A program that writes `let x: Option<Int> = Some(5)` without a user-defined `Option` enum will type-check the annotation but fail at the constructor call (no `Some` in scope).
- Idiomatic error handling requires every Ferric program to either redefine `Option` / `Result` or import them from a future stdlib module.

## What "native" would require

1. **Resolver registration**: pre-register `Option<T>` and `Result<T, E>` as enums during `resolve()`, allocating `DefId`s for the type and each variant constructor.
2. **Bridge to existing `Ty` variants**: ensure the resolver-side enum and the typechecker-side `Ty::Option` / `Ty::Result` agree — or replace the dedicated `Ty` variants with `Ty::Enum(DefId, Vec<Ty>)` and let them flow through the normal enum path.
3. **Stdlib registration in `register_stdlib`**: surface the enum types so they're in scope at every call site without import.
4. **Pattern matching**: wire variant `DefId`s through to the existing match-lowering path in the compiler — likely zero VM changes since enum dispatch already exists (M4).
5. **Decision point**: keep `Ty::Option` / `Ty::Result` for nicer inference messages, or unify under `Ty::Enum` for simplicity. The current dedicated variants suggest the former was the original intent but wasn't completed.

## Impact

- **High user impact**: every program that wants idiomatic error handling has to redefine these.
- **Low implementation risk**: M4 enum machinery already exists; this is plumbing, not new design.
- **Blocking dependency**: M7 (modules) gives the natural mechanism for a stdlib `prelude` — it's reasonable to land Option/Result there rather than as ad-hoc pre-registrations.
