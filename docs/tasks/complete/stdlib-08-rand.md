# Task: Stdlib — `rand`

> Read `docs/tasks/stdlib-00-overview.md` first for the global rules.

## Scope

Implement the `rand` module from `docs/stdlib.md` (section "`rand` — Random
Numbers"). Non-cryptographic, seeded, reproducible.

## Output File

- `crates/ferric_stdlib/src/rand_.rs` (file is `rand_` to avoid the
  external-crate-name conflict).

Exposes `pub fn register(registry, interner)`.

## Required `NativeValue` Variants

```rust
// REQUIRED NATIVE VALUE VARIANTS:
//   Rng(Rc<RefCell<RngRepr>>)
//
// where RngRepr wraps rand_chacha::ChaCha20Rng (or any reproducible Rng).
```

## Required Deps

```rust
// REQUIRED DEPS:
//   rand = "0.8"
//   rand_chacha = "0.3"
```

## Function Surface (must register)

Unseeded:

`int`, `float`, `bool`, `pick`, `shuffle`, `sample`.

Seeded:

`seeded`, `rng_int`, `rng_float`, `rng_pick`, `rng_shuffle`.

## Implementation Notes

- Unseeded helpers use `rand::thread_rng()`.
- `seeded(seed)` creates a `ChaCha20Rng::seed_from_u64(seed as u64)` wrapped
  in `Rc<RefCell<...>>`.
- `rng_int(r, low, high)` is *functional* per spec — returns
  `(Int, Rng)`. The `Rng` returned is the same handle (since `Rc<RefCell>`
  mutates internally), but documenting "returns new Rng" preserves the
  pure-function signature surface. Tests assert the same handle is returned.
- `int(low, high)` is **inclusive** on both bounds.
- `float()` returns `[0.0, 1.0)`.
- `pick(l)` → `Some(item)` for non-empty, `None` for empty.
- `shuffle` returns a new `List` (Fisher-Yates copy; do not shuffle in
  place).
- `sample(l, n)` returns `n` distinct items without replacement; `n > l.len()`
  errs.

## Tests

- `int(1, 10)` returns value in `[1, 10]`; loop 100×, assert all in range.
- `float()` ∈ `[0.0, 1.0)`; loop 100×.
- `bool()` returns both true and false across many trials (probabilistic;
  use 200 iterations and assert at least one of each — false-flake risk
  ~1e-60).
- `pick([])` → None; `pick([7])` → `Some(7)`.
- `shuffle(l)` returns a list with the same multiset.
- `sample([1,2,3,4,5], 3).len()` == 3; all distinct.
- `sample(l, l.len() + 1)` errs.
- **Seeded determinism:**
  - `seeded(42)` then `rng_int(_, 0, 100)` ×3 produces a fixed sequence;
    assert against the literal expected values.
  - Two `seeded(42)` instances produce the same sequence.
  - `seeded(42)` and `seeded(43)` produce different sequences.
- `rng_shuffle(seeded(0), [1,2,3,4,5])` matches a fixed expected order.

## Constraints

- Do **not** modify `lib.rs` or `Cargo.toml`.
- Tests must be deterministic for seeded paths and tolerant for unseeded
  paths.

## Acceptance

- One file with the listed register coverage.
- Tests cover unseeded ranges + seeded determinism.
- No edits outside the new file.
