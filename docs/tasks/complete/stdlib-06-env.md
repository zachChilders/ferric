# Task: Stdlib — `env`

> Read `docs/tasks/stdlib-00-overview.md` first for the global rules.

## Scope

Implement the `env` module from `docs/stdlib.md` (section "`env` —
Environment Variables and Process").

## Output File

- `crates/ferric_stdlib/src/env.rs`

Exposes `pub fn register(registry, interner)`.

## Required `NativeValue` Variants

```rust
// REQUIRED NATIVE VALUE VARIANTS:
//   Map(BTreeMap<MapKey, NativeValue>)    // shared with map-set task
```

## Required Deps

None — `std::env`.

## Function Surface (must register)

`get`, `get_or`, `require`, `set`, `remove`, `all`, `args`, `cwd`,
`home_dir`, `temp_dir`, `exit`.

## Implementation Notes

- `env::get(name)` → `Some(val)` if set, else `None`.
- `env::require(name)` errs with `EnvError::NotSet` if missing.
- `env::all()` returns a `Map<Str, Str>` of every env var.
- `env::args()` returns `std::env::args().collect()` as `List<Str>`.
- `env::cwd()` wraps `std::env::current_dir()` and serializes the
  `PathBuf` to a `Str`. Errors map to `IoError` shape (use a string
  representation; the integration step can rewire to a shared error
  type).
- `env::home_dir()` — `std::env::var_os("HOME")` on Unix,
  `USERPROFILE` on Windows. Return `None` if unset.
- `env::temp_dir()` — `std::env::temp_dir()`.
- `env::exit(code)` calls `std::process::exit(code as i32)`. **Do not test
  this function in unit tests** — it would terminate the test process.

## Tests

- `set("FERRIC_TEST_KEY", "value")` then `get` returns `Some("value")`
- `get` on unset name → `None`
- `get_or("MISSING", "default")` → `"default"`
- `require` on set name → `Ok(val)`; on unset → `Err`
- `remove` clears variable
- `all` includes a variable you just `set`
- `args` returns at least one element (the binary name)
- `cwd` returns a non-empty string
- `temp_dir` returns a path that `io::is_dir` would accept (don't actually
  call io; just assert non-empty)
- `home_dir` is `Option` — assert it's `Some` when `HOME` is set; clear
  `HOME` and assert `None`
- **No test for `exit`** — document a `#[ignore]` placeholder if you want.

Use unique variable names (`FERRIC_TEST_<random>`) to avoid pollution
between test runs. Always `remove` set variables in cleanup.

## Constraints

- Do **not** modify `lib.rs` or `Cargo.toml`.
- Tests must not leak env vars or change the process working directory.

## Acceptance

- One file with the env surface.
- Tests cover happy + missing-key + cleanup paths.
- `exit` is registered but untested (commented).
