# M7 — Task 4: Type Checker + VM

> **Prerequisite:** Tasks 1, 2, and 3 must all be complete before starting this
> task. This task wires opaque types into the type checker and module namespace
> values into the VM. It is the final task of M7.

---

## What this task does

Two additions that have no dependency on each other but both require the earlier
tasks to be in place:

**A — Opaque type aliases in the type checker.** `type Url = Str` creates a
`Ty::Opaque` that the type checker treats as distinct from `Str`. Construction
and unwrap require explicit `as` casts. At runtime, opaque types erase completely —
`as` compiles to a no-op.

**B — Module namespace values in the VM.** `import * as db from "./db"` binds `db`
to a `Value::Module` — a struct-like map of exported names to values. Field access
(`db.connect(...)`) works through the existing field access mechanism from M4.

---

## Part A — Opaque types in the type checker

### Registering type aliases

When the type checker encounters `Item::TypeAlias(TypeAliasItem)`, it registers
the alias in a new `TypeAliasTable` (internal to `ferric_typecheck` — not a public
stage output). The table maps `DefId → TypeAliasEntry`:

```rust
struct TypeAliasEntry {
    params: Vec<Symbol>,
    inner:  Ty,
    opaque: bool,
}
```

For non-generic aliases (`type Url = Str`), `params` is empty and `inner` is `Ty::Str`.
For generic aliases (`type Result<T> = StdResult<T, AppError>`), `params` contains
the type parameter names and `inner` contains the body with those parameters as
`Ty::Var` placeholders.

### `Ty::Opaque` in the type system

When a `TypeAliasItem` with `opaque: true` is used as a type annotation, the type
checker resolves it to `Ty::Opaque { def_id, inner }` — not to the inner type
directly. This is what makes `Url` and `Str` distinct.

Rules:

- `Ty::Opaque { def_id: A, .. }` and `Ty::Opaque { def_id: B, .. }` are never
  equal, even if their `inner` types are identical. `Url` and `Email` are both
  `Str` underneath but are distinct opaque types.
- `Ty::Opaque { .. }` and its `inner` type are never equal without an explicit cast.
  Assigning a `Str` to a `Url` binding without `as Url` is `TypeError::OpaqueTypeMismatch`.

### Cast expression type checking

`CastExpr { expr, target }` is valid in exactly two directions:

**Wrapping** — `expr` has type `T` and `target` is `Ty::Opaque { inner: T, .. }`:
```rust
let u: Url = "example.com" as Url    // Str → Url — legal
```

**Unwrapping** — `expr` has type `Ty::Opaque { inner: T, .. }` and `target` is `T`:
```rust
let s: Str = u as Str                // Url → Str — legal
```

Any other cast is `TypeError::InvalidCast { from, to, span }`:
```rust
let n = u as Int                     // Url → Int — illegal
let v = u as Email                   // Url → Email — illegal (even though both wrap Str)
```

### Generic alias instantiation

For generic aliases (`type Result<T> = StdResult<T, AppError>`), instantiation
substitutes concrete types for parameters at use sites. This falls out of the
existing HM inference machinery — treat the alias as a type constructor with arity
equal to `params.len()`. No new type checker infrastructure is needed beyond
registration and substitution.

### Exported type aliases

When `export type Url = Str` is imported in another file, the importer receives
the `DefId` for `Url`. The type checker looks up the alias in the `TypeAliasTable`
by `DefId` and resolves it to `Ty::Opaque { def_id, inner }` as normal. Opaque
types are opaque across module boundaries — a module that imports `Url` cannot
access the inner `Str` without an explicit `as Str` cast.

### New error variants

Declared in Task 1, emitted here:

```rust
TypeError::OpaqueTypeMismatch { expected: Ty, found: Ty, span: Span }
TypeError::InvalidCast        { from: Ty, to: Ty, span: Span }
```

### Diagnostics

```
error: type mismatch — expected `Url`, found `Str`
  --> src/main.fe:4:18
   |
 3 | let u: Url = "example.com"
   |         --- expected `Url` because of this annotation
 4 |              ^^^^^^^^^^^^^ this is `Str`; cast with `"example.com" as Url`
   |
   = help: use `as Url` to construct an opaque type

error: cannot cast `Url` to `Email`
  --> src/main.fe:7:14
   |
 7 |     let e = u as Email
   |               ^^^^^^^^ both wrap `Str` but are distinct opaque types
   |
   = help: unwrap first with `u as Str`, then cast to `Email`
```

---

## Part B — Module namespace values in the VM

### `Value::Module`

Namespace imports (`import * as db from "./db"`) bind the local name `db` to a
`Value::Module`. A module value is a map of exported `Symbol`s to their `Value`s,
evaluated at import time.

Construction follows Rule 7 — `Value::Module` is never constructed directly outside
`ferric_vm`:

```rust
// ILLEGAL outside ferric_vm
let v = Value::Module(map);

// LEGAL everywhere
let v = Value::new_module(fields: HashMap<Symbol, Value>);
```

Add `Value::new_module(fields: HashMap<Symbol, Value>) -> Value` to `ferric_vm`.

### Field access on module values

`db.connect` resolves `connect` against the module's field map. This reuses the
existing field access mechanism introduced in M4 for structs. No new VM instruction
is needed — `GetField` already handles this if the VM's field access dispatches on
value type.

If the field name is not in the module's export map, this is a runtime error:
`RuntimeError::NoSuchField { name: Symbol, span: Span }`. This should already
exist from M4 — if not, add it here.

### Calling through a namespace import

```rust
import * as db from "./db"

db.connect(host: "localhost", port: 5432)
```

`db.connect` resolves to a `Value::Fn` (or `Value::Closure`) from the module map.
The call then proceeds exactly as any other function call — no special casing needed.

### Evaluation order

Module values are populated before the entry point runs. The VM evaluates each
imported file's top-level items in dependency order (leaves first, determined by
the import graph from Task 3). Exported values are extracted from the resulting
environment and placed in the `Value::Module` map.

Named imports (`import { connect } from "./db"`) bind `connect` directly to the
`Value::Fn` — no `Value::Module` wrapper. The module resolver in Task 3 has already
mapped the local name to the source `DefId`; the VM just looks up the value by slot.

### Async compatibility

`Value::Module` contains `HashMap<Symbol, Value>`. `Symbol` is `u32` (Copy, Send).
`Value` must already be `Send` (async gate from M2.5 Task 4). Therefore
`Value::Module` is `Send` without any additional work.

Add `Value::Module` to the compile-time assertion in `ferric_vm/src/lib.rs`:

```rust
fn _assert_value_send() {
    fn check<T: Send>() {}
    check::<Value>();   // covers all variants including Module
}
```

---

## Done when

**Opaque types:**
- [ ] `type Url = Str` registers an opaque type alias in the type checker
- [ ] `Ty::Opaque` is distinct from its inner type — assignment without cast is `TypeError::OpaqueTypeMismatch`
- [ ] Two opaque types with the same inner type are distinct from each other
- [ ] `expr as OpaqueType` (wrapping) type-checks when `expr` has the inner type
- [ ] `expr as InnerType` (unwrapping) type-checks when `expr` has the opaque type
- [ ] All other casts produce `TypeError::InvalidCast`
- [ ] Generic aliases (`type Result<T> = ...`) instantiate correctly at use sites
- [ ] Opaque types imported from another module remain opaque — no inner access without `as`
- [ ] `TypeError::OpaqueTypeMismatch` and `TypeError::InvalidCast` render correctly
- [ ] At runtime, `as` is a no-op — no `Value` variant for opaque types

**Module namespace values:**
- [ ] `import * as db from "./db"` binds `db` to a `Value::Module`
- [ ] `db.connect` resolves through field access to the exported function value
- [ ] `db.connect(host: "localhost", port: 5432)` calls correctly
- [ ] Named imports (`import { connect }`) bind the value directly — no `Value::Module` wrapper
- [ ] Module values are populated in dependency order before the entry point runs
- [ ] `Value::new_module(...)` is the only construction path outside `ferric_vm`
- [ ] `Value::Module` is covered by the `Send` compile-time assertion
- [ ] `RuntimeError::NoSuchField` fires for field access on a module with no matching export

**General:**
- [ ] All new `TypeError` variants carry `Span` (Rule 5)
- [ ] All M1–M6 tests still pass
- [ ] Full M7 integration test: multi-file program with named imports, namespace imports, opaque types, and a manifest — runs correctly end to end
