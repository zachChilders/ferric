# Task: Stdlib — `map` and `set`

> Read `docs/tasks/stdlib-00-overview.md` first for the global rules.

## Scope

Implement the `map` and `set` modules from `docs/stdlib.md` (sections "`map`
— Hash Maps" and "`set` — Hash Sets").

Paired because both are hash collections built on the same key-equality and
hashing semantics. Both expose `from_list`/`to_list` interplay with `List`.

## Output Files

- `crates/ferric_stdlib/src/map.rs`
- `crates/ferric_stdlib/src/set.rs`

Each exposes `pub fn register(registry, interner)`.

## Required `NativeValue` Variants

```rust
// REQUIRED NATIVE VALUE VARIANTS:
//   Map(BTreeMap<MapKey, NativeValue>)
//   MapMut(Rc<RefCell<BTreeMap<MapKey, NativeValue>>>)
//   Set(BTreeSet<MapKey>)
//   Tuple2(Box<NativeValue>, Box<NativeValue>)
//
// MapKey is a hashable+orderable subset of NativeValue. Define it as:
//   pub enum MapKey { Int(i64), Str(String), Bool(bool) }
// Float keys are forbidden (NaN). The integration task may upgrade to
// HashMap once NativeValue gains Eq+Hash.
```

## Required Deps

None — `BTreeMap` / `BTreeSet` from std.

## Function Surface

### `map` (must register)

`new`, `from_list`, `from_lists`, `get`, `get_or`, `contains_key`, `len`,
`is_empty`, `insert`, `remove`, `merge`, `merge_with`, `keys`, `values`,
`entries`, `map_values`, `filter`, `filter_map`, `fold`, `group_by`,
`count_by`, `builder`, `set` (the builder version), `build`.

### `set` (must register)

`new`, `from_list`, `contains`, `len`, `is_empty`, `insert`, `remove`,
`union`, `intersection`, `difference`, `symmetric_difference`, `is_subset`,
`is_superset`, `is_disjoint`, `to_list`, `filter`, `map`.

## Implementation Notes

- Maps are immutable: `insert`/`remove`/`merge` return a *new* map. Use
  `BTreeMap::clone` (acceptable for now; perf improvement deferred).
- `from_list(pairs)` consumes a `List<(K, V)>`. Last write wins on dup keys.
- `from_lists(keys, vals)` errs if lengths differ
  (`MapError::LengthMismatch`).
- `merge(a, b)`: b wins on conflict.
- `merge_with(a, b, f)`: when both sides have the key, value is
  `f(a_val, b_val)`.
- `keys` / `values` return `List`. Order is **insertion order undefined**
  for hash maps, but `BTreeMap` orders by key. Document that `keys` is in
  ascending key order until the impl swaps to a hash map.
- `group_by(l, f)` builds `Map<K, List<T>>` by applying `f` to each item.
- `count_by(l, f)` builds `Map<K, Int>` of bucket sizes.
- `set::to_list` order is the natural `BTreeSet` order until hashing lands.
- `set::union/intersection/difference/symmetric_difference` are pure.

## Tests

### `map` tests

- `from_list([("a",1),("b",2)])` length 2; `get("a")` → `Some(1)`; `get("z")` → `None`
- `from_list` last-wins on dup key
- `from_lists(["a","b"], [1,2])` works; `from_lists(["a"], [1,2])` errs
- `insert` returns new map; original unchanged
- `remove` no-op on absent key
- `contains_key`, `len`, `is_empty`
- `merge({a:1,b:2}, {b:3,c:4})` → `{a:1, b:3, c:4}`
- `merge_with(a, b, |x, y| x + y)` sums on conflict
- `keys`, `values`, `entries`
- `map_values` transforms values
- `filter` keeps subset
- `fold` sums all values
- `group_by([1,2,3,4], |x| x % 2)` → `{0: [2,4], 1: [1,3]}`
- `count_by(["a","b","a","c"], identity)` → `{"a":2,"b":1,"c":1}`
- Builder: `builder` + `set` ×3 + `build`

### `set` tests

- `from_list([1,2,2,3])` len 3
- `contains` true/false
- `insert(s, 1)` no change if 1 already in
- `union({1,2}, {2,3})` → `{1,2,3}`
- `intersection({1,2}, {2,3})` → `{2}`
- `difference({1,2}, {2,3})` → `{1}`
- `symmetric_difference({1,2}, {2,3})` → `{1,3}`
- `is_subset({1,2}, {1,2,3})` → true
- `is_superset({1,2,3}, {1,2})` → true
- `is_disjoint({1,2}, {3,4})` → true
- `to_list` length matches `len`
- `filter`, `map`

Aim for ~25+ tests combined.

## Constraints

- Do **not** modify `lib.rs` or `Cargo.toml`.
- Higher-order functions (`filter`, `fold`, `merge_with`, `group_by`,
  `count_by`, `map_values`) follow the same closure-invocation contract as
  `list` — assume `invoke_closure` exists.

## Acceptance

- Two files with the listed register coverage.
- Tests cover non-closure paths; closure paths covered where plumbing
  permits.
- No edits outside the two new files.
