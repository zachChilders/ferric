# Task: Stdlib — `list` and `sort`

> Read `docs/tasks/stdlib-00-overview.md` first for the global rules.

## Scope

Implement the `list` and `sort` modules from `docs/stdlib.md` (sections
"`list` — Dynamic Arrays" and "`sort` — Sorting and Ordering").

Paired because every `sort` function operates on `List<T>` and returns
`List<T>`. They share the underlying `Vec<NativeValue>` representation.

## Output Files

- `crates/ferric_stdlib/src/list.rs`
- `crates/ferric_stdlib/src/sort.rs`

Each exposes `pub fn register(registry, interner)`.

## Required `NativeValue` Variants

```rust
// REQUIRED NATIVE VALUE VARIANTS:
//   List(Vec<NativeValue>)         // already exists as Array — reuse it
//   ListMut(Rc<RefCell<Vec<NativeValue>>>)
//   Closure(Rc<dyn Fn(&[NativeValue]) -> Result<NativeValue, String>>)
//     — needed for callbacks (map/filter/sort_by, etc.). The VM already
//     has its own closure representation; this comment indicates the
//     integration task should provide a way to invoke a Ferric closure
//     from native code, or pass the closure type through.
//   Ordering(std::cmp::Ordering)
//     — for sort::cmp_* return values
//   Tuple2(Box<NativeValue>, Box<NativeValue>)  // for split_once, zip, partition, unzip
```

The `Closure` requirement is the trickiest part — list operations are
higher-order. For now, in `list.rs`, write the implementations assuming a
helper `fn invoke_closure(c: &NativeValue, args: &[NativeValue]) ->
Result<NativeValue, String>` exists. Integration will wire it.

## Required Deps

None — pure std.

## Function Surface

### `list` (must register)

`new`, `of`, `repeat`, `range`, `range_inclusive`, `len`, `is_empty`, `get`,
`first`, `last`, `slice`, `contains`, `find`, `find_index`, `index_of`,
`map`, `flat_map`, `filter`, `filter_map`, `reduce`, `fold`, `scan`, `zip`,
`zip_with`, `enumerate`, `flatten`, `chunk`, `window`, `take`, `drop`,
`take_while`, `drop_while`, `partition`, `unzip`, `any`, `all`, `count`,
`sum`, `sum_float`, `min`, `max`, `concat`, `prepend`, `append`, `insert`,
`remove`, `reverse`, `unique`, `intersperse`, `join_str`, `builder`, `push`,
`pop`, `build`.

### `sort` (must register)

`asc`, `desc`, `by`, `by_desc`, `with`, `cmp_int`, `cmp_float`, `cmp_str`,
`cmp_str_natural`, `reverse`, `then`, `top`, `bottom`, `is_sorted`,
`is_sorted_by`.

Plus the `Ordering` enum constants (`Less`, `Equal`, `Greater`) — surface
them as native values that comparator functions return.

## Implementation Notes

- `list::range(0, 5)` → `[0,1,2,3,4]` (exclusive end). `range_inclusive(0,5)`
  → `[0,1,2,3,4,5]`.
- `list::sum` requires every element to be `Int`; mixed types return error.
  `list::sum_float` requires every element to be `Float`.
- `list::min` / `max` use `Ord` semantics. Until M5 traits, only support
  `Int`, `Float`, and `Str` element types and return error otherwise.
- `list::contains` / `index_of` use structural equality on `NativeValue`
  for the supported scalar types.
- `list::reduce(empty)` → `Option::None` (i.e., `NativeValue::Unit`-tagged
  None equivalent — represent Option as `Tuple2`-wrapped or a dedicated
  variant; document choice in `// REQUIRED NATIVE VALUE VARIANTS:` if you
  add one).
- `list::partition` returns `(truthy, falsy)`; preserve original order
  inside each side.
- `list::chunk(l, 0)` and `window(l, 0)` return error.
- `sort::cmp_str_natural` — implement digit-aware comparison: `"file9" <
  "file10"`. Splits on digit/non-digit runs, compares numerically when both
  sides are digit runs, lexicographically otherwise.
- All sorts are **stable**. Use `Vec::sort_by` (stable in Rust std).
- `sort::top(l, n, key)` returns the `n` largest by key, in descending key
  order. `sort::bottom` is the opposite.

## Tests

### `list` tests

- `range(0, 5)` length 5; `range_inclusive(0, 5)` length 6
- `repeat(7, 3)` → `[7,7,7]`
- `get(l, 0)` Some; `get(l, 99)` None
- `first` / `last` on empty → None
- `map(double, [1,2,3])` → `[2,4,6]` (write a fixture closure)
- `filter(is_even, [1,2,3,4])` → `[2,4]`
- `fold(add, 0, [1,2,3])` → `6`
- `reduce(add, [1,2,3])` → `Some(6)`; `reduce(add, [])` → `None`
- `flat_map`, `filter_map`, `flatten`
- `zip([1,2,3], ["a","b"])` → 2 pairs (length is min)
- `enumerate(["a","b"])` → `[(0,"a"),(1,"b")]`
- `chunk([1,2,3,4,5], 2)` → `[[1,2],[3,4],[5]]`
- `window([1,2,3,4], 2)` → `[[1,2],[2,3],[3,4]]`
- `take`, `drop`, `take_while`, `drop_while`
- `partition(is_even, [1,2,3,4])` → `([2,4], [1,3])`
- `any`, `all`, `count`
- `sum([1,2,3])` → 6; `sum_float([1.0, 2.5])` → 3.5
- `min`/`max` on Int / Float / Str / mixed (errors on mixed)
- `concat`, `prepend`, `append`, `insert`, `remove`
- `reverse`, `unique` (preserves first occurrence)
- `intersperse([1,2,3], 0)` → `[1,0,2,0,3]`
- Builder: `builder` + `push` ×3 + `build`

### `sort` tests

- `asc([3,1,2])` → `[1,2,3]`
- `desc([3,1,2])` → `[3,2,1]`
- `by([{name:"b"}, {name:"a"}], |x| x.name)` → ascending
- `with([3,1,2], cmp_int)` matches `asc`
- `cmp_int(1, 2)` → `Less`; `cmp_int(2, 2)` → `Equal`
- `cmp_str_natural("file9", "file10")` → `Less`
- `cmp_str("file9", "file10")` → `Greater` (lexicographic)
- `reverse(cmp_int)(1, 2)` → `Greater`
- `then(cmp_int, cmp_int)(1, 1)` → `Equal`
- `top([5,3,9,1,7], 2, identity)` → `[9, 7]`
- `is_sorted([1,2,3])` → true; `is_sorted([1,3,2])` → false
- Stability: equal-key items preserve input order

Aim for ~40+ tests combined.

## Constraints

- Do **not** modify `lib.rs` or `Cargo.toml`.
- Closures: write the function bodies as if `invoke_closure` exists. If you
  cannot test higher-order functions cleanly, leave a `#[ignore]` test that
  documents the intended behavior.

## Acceptance

- Two files with the listed register coverage.
- Tests cover scalar paths fully; higher-order paths covered where
  `invoke_closure` plumbing permits.
- No edits outside the two new files.
