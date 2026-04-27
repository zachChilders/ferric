# Task: Stdlib — `json` and `fmt`

> Read `docs/tasks/stdlib-00-overview.md` first for the global rules.

## Scope

Implement the `json` and `fmt` modules from `docs/stdlib.md` (sections
"`json` — JSON" and "`fmt` — Formatting").

Paired because `fmt::debug(v: Json) -> Str` directly consumes the `Json`
value type. They share the `Json` enum representation.

## Output Files

- `crates/ferric_stdlib/src/json.rs`
- `crates/ferric_stdlib/src/fmt.rs`

Each exposes `pub fn register(registry, interner)`.

## Required `NativeValue` Variants

```rust
// REQUIRED NATIVE VALUE VARIANTS:
//   Json(Box<JsonRepr>)
//
// where JsonRepr mirrors the Ferric Json enum:
//   pub enum JsonRepr {
//       Null,
//       Bool(bool),
//       Int(i64),
//       Float(f64),
//       Str(String),
//       Array(Vec<JsonRepr>),
//       Object(Vec<(String, JsonRepr)>),  // ordered for stable output
//   }
//
// Object uses Vec<(K, V)> for stable iteration order; conversion to/from
// Map<Str, Json> happens through json::as_object.
```

## Required Deps

```rust
// REQUIRED DEPS:
//   serde_json = "1"           // for parsing / serialisation; we wrap it
```

(A hand-rolled parser is acceptable too — it is a single-file effort. The
crate is suggested for speed.)

## Function Surface

### `json` (must register)

Serialise:

`to_str`, `to_str_pretty`.

Parse:

`parse`.

Access helpers:

`as_bool`, `as_int`, `as_float`, `as_str`, `as_array`, `as_object`,
`is_null`, `get`, `get_path`.

Construction helpers:

`null`, `bool`, `int`, `float`, `str`, `array`, `object`.

### `fmt` (must register)

`format`, `int`, `int_padded`, `float`, `float_exp`, `bool`, `debug`.

## Implementation Notes

### `json`

- `parse(s)` errs with `JsonError::SyntaxError { message, line, col }` on
  malformed input.
- `to_str` is compact (no whitespace). `to_str_pretty` uses 2-space indent.
- Numeric parsing: integers without `.` parse as `Json::Int`, otherwise
  `Json::Float`. Document that very large integers (> i64) overflow into
  Float.
- `as_*` return `None` on type mismatch — never panic, never coerce.
  `as_int(Float)` is `None` (no implicit int-from-float).
- `get(obj, key)` works only on `Object`. Returns `None` on non-Object or
  missing key.
- `get_path(v, ["a", "b", "c"])` walks. Returns `None` at first miss.
- Construction helpers are trivial wrappers.

### `fmt`

- `format(template, args)` replaces `{}` placeholders **in order** with
  args. Mismatched count is an error
  (`"fmt::format: expected N args, got M"`).
- `int(n, radix)` — radix must be 2, 8, 10, or 16. Otherwise err.
- `int_padded(n, width, pad)` — `pad` must be 1 char (in chars, not bytes);
  pads on the **left**.
- `float(n, decimals)` uses `format!("{:.*}", decimals, n)`.
- `float_exp(n, decimals)` uses `format!("{:.*e}", decimals, n)`.
- `bool` returns `"true"` / `"false"`.
- `debug(v)` pretty-prints a `Json` value (≈ `to_str_pretty`).

## Tests

### `json` tests

- `parse("null")` → `Null`
- `parse("true")` → `Bool(true)`
- `parse("42")` → `Int(42)`; `parse("3.14")` → `Float(3.14)`
- `parse("\"hi\"")` → `Str("hi")`
- `parse("[1,2,3]")` → array of 3
- `parse("{\"a\":1}")` → object
- `parse("{")` errs with line/col
- `to_str(Int(42))` → `"42"`; `to_str(Null)` → `"null"`
- `to_str_pretty({a:1, b:[1,2]})` produces multiline output
- Roundtrip: `parse(to_str(v)) == v` for each variant
- `as_*` accessors return `None` on mismatch
- `get({a:1}, "a")` → `Some(Int(1))`; `get(Null, "a")` → `None`
- `get_path({a:{b:{c:7}}}, ["a","b","c"])` → `Some(Int(7))`
- `is_null(Null)` true; `is_null(Bool(false))` false
- Construction helpers produce the expected variant

### `fmt` tests

- `format("hello, {}!", ["world"])` → `"hello, world!"`
- `format("{} + {} = {}", ["1", "2", "3"])` → `"1 + 2 = 3"`
- `format("{}", [])` errs (placeholder count mismatch)
- `int(255, 16)` → `"ff"`; `int(10, 2)` → `"1010"`; `int(_, 7)` errs
- `int_padded(7, 3, "0")` → `"007"`
- `float(3.14159, 2)` → `"3.14"`
- `float_exp(1234.5, 2)` → `"1.23e3"`
- `bool(true)` → `"true"`
- `debug(Object(...))` produces pretty multiline JSON

Aim for ~25 tests combined.

## Constraints

- Do **not** modify `lib.rs` or `Cargo.toml`.
- `fmt::debug` only knows how to render `Json` values; it does not stringify
  arbitrary `NativeValue`.

## Acceptance

- Two files with the listed register coverage.
- Tests cover parse / serialise / accessors / format.
- No edits outside the two new files.
