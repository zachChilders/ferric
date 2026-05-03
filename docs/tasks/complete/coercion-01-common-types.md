# Coercion — Task 1: Common Types

> **Do this task first.** Adds the shared types in `ferric_common` that
> Tasks 2–4 depend on. No behaviour changes.
>
> See [coercion-00-overview.md](coercion-00-overview.md) for design decisions.

---

## What this task does

Extends `TypeResult` with a `coercions` field and adds two new error variants
(`TypeError::AmbiguousCoercion`, `ResolveError::OrphanImpl`). Wires no-op
arms / default initialisers across downstream stages so the workspace
compiles cleanly. Nothing constructs the new error variants yet — Tasks 2 and
3 do that.

---

## ferric_common — additions required

### 1. `TypeResult::coercions` field

```rust
// In ferric_common — extend TypeResult:
pub struct TypeResult {
    pub node_types:  HashMap<NodeId, Ty>,
    pub coercions:   HashMap<NodeId, DefId>,   // arg NodeId → To impl DefId
    pub errors:      Vec<TypeError>,
    // ... any existing fields stay unchanged
}
```

`coercions` maps the `NodeId` of a call argument expression to the `DefId` of
the `To` impl that was chosen. The compiler reads this in Task 4 to insert
the coercion call.

All existing constructors of `TypeResult` must initialise `coercions` to
`HashMap::new()`. Use `#[serde(default)]` if `TypeResult` is serialised so
older serialised states still load.

### 2. `TypeError::AmbiguousCoercion`

```rust
// Add to TypeError:
TypeError::AmbiguousCoercion {
    found:       Ty,
    expected:    Ty,
    candidates:  Vec<DefId>,
    span:        Span,
}
```

Carries `Span` (Rule 5). The error message names the source type, the target
type, and lists the conflicting impl `DefId`s so the renderer can resolve
them to source locations.

Add a renderer arm in `ferric_diagnostics` that prints the source type, the
target type, and one source location per candidate impl. Format consistent
with existing trait-related errors.

### 3. `ResolveError::OrphanImpl`

```rust
// Add to ResolveError:
ResolveError::OrphanImpl {
    trait_name: Symbol,
    source:     Ty,
    target:     Ty,
    span:       Span,
}
```

Carries `Span` (Rule 5). The renderer message: `impl <Trait>` for `<Source>`
where neither `<Source>` nor `<Target>` is owned by the current crate.

Add a renderer arm in `ferric_diagnostics`. If `ResolveError::OrphanImpl`
already exists from earlier work (check `ferric_traits` for prior orphan-rule
checks), extend it rather than duplicate. The `trait_name: Symbol` param
keeps the variant general — it isn't `To`-specific.

### 4. Send + Sync gate

Add the new error variants to the compile-time `Send + Sync` assertion in
`ferric_common/src/lib.rs` if they include any non-trivial nested types.
`Vec<DefId>` is fine; just confirm.

### 5. Display / description arms

If `TypeError`, `ResolveError`, or `Ty` have `description()` / `Display`
helpers that match exhaustively, add arms for the new variants. The
`AmbiguousCoercion` description: `"ambiguous implicit coercion"`. The
`OrphanImpl` description: `"orphan trait impl"`.

---

## Downstream stages

No code changes beyond adding the new field initialisers and exhaustive-match
arms wherever `TypeResult` is constructed or `TypeError`/`ResolveError` is
matched. `unreachable!()` is appropriate for the new error arms in code that
shouldn't ever see them at this stage (e.g. the parser doesn't construct
either).

---

## Done when

- [ ] `TypeResult::coercions: HashMap<NodeId, DefId>` exists
- [ ] All `TypeResult` construction sites initialise `coercions` to `HashMap::new()`
- [ ] `TypeError::AmbiguousCoercion { found, expected, candidates, span }` exists
- [ ] `ResolveError::OrphanImpl { trait_name, source, target, span }` exists
- [ ] Diagnostics renderer arms exist for both new variants
- [ ] All new types derive `Serialize + Deserialize + Debug + Clone + PartialEq`
- [ ] Send + Sync gate updated
- [ ] `cargo build --workspace` clean
- [ ] All existing tests still pass — no behaviour change yet
