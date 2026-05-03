# Task: Stdlib — `log`

> Read `docs/tasks/stdlib-00-overview.md` first for the global rules.

## Scope

Implement the `log` module from `docs/stdlib.md` (section "`log` —
Structured Logging"). Logs are structured, go to stderr, no log files, no
config file. `LOG_FORMAT=pretty|json` env var controls output.

## Output File

- `crates/ferric_stdlib/src/log.rs`

Exposes `pub fn register(registry, interner)`.

## Required `NativeValue` Variants

```rust
// REQUIRED NATIVE VALUE VARIANTS:
//   Map(BTreeMap<MapKey, NativeValue>)            // shared
//   Logger(Rc<LoggerRepr>)
//   LogLevel(LogLevelRepr)
//
// pub struct LoggerRepr {
//     pub fields: Vec<(String, String)>,    // ordered for stable output
// }
//
// #[derive(Copy, Clone, PartialEq, PartialOrd)]
// pub enum LogLevelRepr { Debug = 0, Info = 1, Warn = 2, Error = 3 }
```

## Required Deps

None — std + manual JSON serialisation (or borrow `serde_json` if the
json task picked it up; either way, document the choice).

```rust
// REQUIRED DEPS:
//   chrono = { version = "0.4", default-features = false, features = ["clock"] }
//   (already required by the time task — duplicate listing is fine; integration
//   dedupes)
```

## Function Surface (must register)

Direct-level:

`debug`, `info`, `warn`, `error`.

With fields:

`debug_fields`, `info_fields`, `warn_fields`, `error_fields`.

Logger:

`new`, `with`, `log_debug`, `log_info`, `log_warn`, `log_error`.

Level control:

`set_level`, `get_level`.

## Implementation Notes

- The current minimum level is process-wide. Use `OnceLock<AtomicU8>` —
  initialize-once cell holding an atomic mutated by `set_level`. This is
  the same shape as `http::Client` and is acceptable per the
  no-mutable-globals rule (single atomic, documented).
- Default level is `Info`.
- Output format chosen at first call by reading `LOG_FORMAT` env var:
  - `"json"` → `{"ts":"...","level":"INFO","msg":"...","key":"val"}`
  - any other value → `2026-04-25T14:30:00Z INFO  message  key=val`
    (default; called `pretty`).
- All output goes to stderr.
- Field order is stable (insertion order). `Logger::with` returns a new
  `Logger` with the field appended.
- `*_fields(msg, fields)` accepts a `Map<Str, Str>`. Order: BTreeMap is
  alphabetical — document that pretty-format fields are sorted; integration
  may switch to insertion order when a hash map lands.

## Tests

Use a thread-local `io::stderr()` capture (or a custom global writer) for
testing. Pragmatic alternative: since modifying global stderr is fragile,
expose an *internal* test hook — a `pub(crate) fn render(...)` that
produces the formatted string for a log event without touching stderr.
Test `render`, not the side effect.

- `render("INFO", "hello", &[])` (pretty mode) →
  matches `r"^\d{4}-\d{2}-\d{2}T.*Z INFO\s+hello\s*$"` (regex).
- `render("INFO", "hello", &[("k", "v")])` ends with `k=v`.
- JSON mode (set `LOG_FORMAT=json` for the test) →
  `{"ts":"...","level":"INFO","msg":"hello","k":"v"}` parses as JSON.
- Field ordering: pretty mode sorts alphabetically; JSON mode same.
- Level filtering:
  - `set_level(Warn)` then `info(...)` → suppressed (no output).
  - `set_level(Warn)` then `warn(...)` → emitted.
- `Logger::with(l, "req_id", "abc")` returns a logger with that field.
- `log_info(l, "msg")` includes all of `l`'s fields plus the new ones.
- `set_level(...)` then `get_level()` round-trips.

Aim for ~15 tests. Tests must restore the global level after running
(`set_level(Info)` in a finalizer / Drop guard) so test order doesn't
matter.

## Constraints

- Do **not** modify `lib.rs` or `Cargo.toml`.
- Tests should not write to actual stderr (use the `render` test hook).
- Tests must serialize via `#[serial_test::serial]` if any of them
  manipulate the global level concurrently. Add `serial_test` to
  `// REQUIRED DEPS` if needed.

## Acceptance

- One file with the listed register coverage.
- Both `pretty` and `json` formats are tested via the `render` hook.
- Level filtering is tested.
