# Task: Stdlib — `time`

> Read `docs/tasks/stdlib-00-overview.md` first for the global rules.

## Scope

Implement the `time` module from `docs/stdlib.md` (section "`time` —
Timestamps and Durations").

## Output File

- `crates/ferric_stdlib/src/time_.rs` (file is `time_` to avoid the
  reserved-word conflict — the module identity is still `time` in Ferric).

Exposes `pub fn register(registry, interner)`.

## Required `NativeValue` Variants

```rust
// REQUIRED NATIVE VALUE VARIANTS:
//   Time(TimeRepr)
//   Duration(DurationRepr)
//
// where:
//   pub struct TimeRepr { pub unix_ns: i128 }     // nanoseconds since epoch
//   pub struct DurationRepr { pub ns: i64 }       // signed nanoseconds
//
// These are deliberately simple. The integration task may swap to
// `chrono::DateTime<Utc>` or the `time` crate later without changing the
// public function surface.
```

## Required Deps

```rust
// REQUIRED DEPS:
//   chrono = { version = "0.4", default-features = false, features = ["clock", "std"] }
//
// chrono is required for strftime-style formatting and ISO 8601 parsing.
// If integration prefers `time` crate, swap it out — function surface stays
// the same.
```

## Function Surface (must register)

Current/construction:

`now`, `now_local`, `monotonic`, `from_unix`, `from_unix_ms`,
`from_unix_ns`.

Decomposition:

`unix`, `unix_ms`, `year`, `month`, `day`, `hour`, `minute`, `second`,
`weekday`.

Arithmetic:

`add_secs`, `add_ms`, `add_days`, `diff_secs`, `diff_ms`.

Formatting:

`format`, `format_iso`, `format_rfc2822`, `parse`, `parse_iso`.

Duration:

`secs`, `ms`, `minutes`, `hours`, `days`, `sleep`.

## Implementation Notes

- `now()` returns wall clock UTC (`SystemTime::now()`).
- `monotonic()` returns nanoseconds since process start. Use a process-wide
  `Once<Instant>` initialized lazily.
- `weekday(t)` → 0=Monday … 6=Sunday.
- `format(t, layout)` uses strftime layout strings. Use `chrono::DateTime`
  for this.
- `format_iso(t)` → RFC 3339 / ISO 8601 (`2026-04-25T14:30:00Z`).
- `parse_iso(s)` accepts both `Z` and `+00:00` zone suffixes.
- `parse(s, layout)` mirrors strftime parsing.
- Duration values: `secs(n)` → `Duration { ns: n * 1_000_000_000 }`, etc.
- `sleep(d)` blocks the current thread (`std::thread::sleep`). Negative or
  zero duration is a no-op.

## Tests

- `now()` returns a Time; `unix(now)` is within a few seconds of
  `SystemTime::now().duration_since(UNIX_EPOCH)` — assert > 1700000000 (2023).
- `from_unix(0)` then `unix` roundtrip → 0
- `from_unix_ms(1234567890123)` then `unix_ms` roundtrip
- `year`, `month`, `day` on a known timestamp (e.g.,
  `from_unix(1714052700)` → 2024-04-25 14:25:00 UTC; verify each accessor)
- `weekday` for a known date
- `add_secs(t, 60)` then `diff_secs` → 60
- `add_days(t, 7)` increases unix by 7*86400
- `format_iso(from_unix(0))` → `"1970-01-01T00:00:00Z"`
- `parse_iso("2026-04-25T14:30:00Z")` roundtrips through `format_iso`
- `format_rfc2822` on a known timestamp matches expected output
- `format(t, "%Y-%m-%d")` works
- `parse("2026-04-25", "%Y-%m-%d")` works; bad input errs
- `secs(60)` / `minutes(1)` produce equal Duration
- `monotonic()` second call is `>=` first call
- `sleep(ms(0))` returns quickly

Avoid `sleep`-based timing assertions in tests beyond the trivial
no-op case.

## Constraints

- Do **not** modify `lib.rs` or `Cargo.toml`.
- All tests must be deterministic — never assert exact `now()` values.

## Acceptance

- One file with the listed register coverage.
- Tests roundtrip through known-fixed timestamps.
- No edits outside the new file.
