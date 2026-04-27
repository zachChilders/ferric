# Task: Stdlib — `re`

> Read `docs/tasks/stdlib-00-overview.md` first for the global rules.

## Scope

Implement the `re` module from `docs/stdlib.md` (section "`re` — Regular
Expressions"). Compiled regexes are values reused across calls.

## Output File

- `crates/ferric_stdlib/src/re.rs`

Exposes `pub fn register(registry, interner)`.

## Required `NativeValue` Variants

```rust
// REQUIRED NATIVE VALUE VARIANTS:
//   Regex(Rc<regex::Regex>)
//   Match(MatchRepr)
//
// MatchRepr stores the matched text plus byte spans plus capture groups:
//   pub struct MatchRepr {
//       pub text: String,                // full match
//       pub start: usize,
//       pub end: usize,
//       pub captures: Vec<Option<String>>,
//       pub named_captures: BTreeMap<String, String>,
//   }
```

## Required Deps

```rust
// REQUIRED DEPS:
//   regex = "1"
```

## Function Surface (must register)

Compilation:

`compile`, `compile_flags`.

Matching:

`is_match`, `find`, `find_all`.

Match inspection:

`match_str`, `match_start`, `match_end`, `capture`, `capture_name`.

Replace:

`replace`, `replace_all`, `replace_fn`.

Split:

`split`, `split_n`.

## Implementation Notes

- `compile(pattern)` wraps `regex::Regex::new`. On failure return
  `ReError::InvalidPattern { pattern, message }`.
- `compile_flags(pattern, flags)`: parse flag chars (`i`, `m`, `s`, `x`,
  `U`) and prepend as inline group: `(?<flags>)pattern`. Reject unknown
  flags with `ReError::InvalidFlags`.
- `is_match`: `regex.is_match(s)`.
- `find`: first `Captures` → `Match`. Build `MatchRepr` with byte offsets
  (Ferric exposes them as `Int` — document that they are byte offsets,
  not char offsets, until/unless we expand).
- `find_all`: iterate `regex.captures_iter`.
- `replace_fn`: take a closure that maps `Match → Str`. Use
  `regex::Regex::replace_all` with a closure callback. (Same closure
  invocation contract as `list`/`map`.)
- `split` / `split_n` use `regex::Regex::split` / `splitn`.

## Tests

- `compile("\\d+")` ok; `compile("[")` errs
- `compile_flags("foo", "i")` matches `"FOO"`; unknown flag errs
- `is_match(re, "abc123")` true; false case
- `find(re("\\d+"), "abc123def")` → match with `match_str = "123"`,
  `start = 3`, `end = 6`
- `find_all(re("\\d+"), "1 22 333")` → 3 matches
- `capture(m, 0)` returns full match
- Named groups: `compile("(?P<num>\\d+)")` then `capture_name(m, "num")`
- `replace(re("a"), "abab", "X")` → `"Xbab"`
- `replace_all(re("a"), "abab", "X")` → `"XbXb"`
- `replace_fn(re("\\d+"), "1 2 3", |m| str_repeat(match_str(m), 2))` → `"11 22 33"`
- `split(re(",\\s*"), "a, b,c")` → `["a", "b", "c"]`
- `split_n(re(","), "a,b,c,d", 2)` → `["a", "b,c,d"]`
- Anchors and groups behave correctly

Aim for ~20 tests.

## Constraints

- Do **not** modify `lib.rs` or `Cargo.toml`.

## Acceptance

- One file with the listed register coverage.
- Tests cover compile errors, match/no-match, captures (positional + named),
  replace, split.
