# Task: Stdlib — `io` and `path`

> Read `docs/tasks/stdlib-00-overview.md` first for the global rules.

## Scope

Implement the `io` and `path` modules from `docs/stdlib.md` (sections "`io`
— File I/O and Standard Streams" and "`path` — Filesystem Path
Manipulation").

Paired because `path` is the natural companion to `io`: callers join
paths, then read/write them. They share `Str`-based path representation
and similar error semantics.

## Output Files

- `crates/ferric_stdlib/src/io.rs`
- `crates/ferric_stdlib/src/path.rs`

Each exposes `pub fn register(registry, interner)`.

## Required `NativeValue` Variants

```rust
// REQUIRED NATIVE VALUE VARIANTS:
//   Bytes(Vec<u8>)                        // shared with str-bytes task
//   Time(...)                             // shared with time task
//   FileWriter(Rc<RefCell<BufWriter<File>>>)
```

## Required Deps

```rust
// REQUIRED DEPS:
//   none — std::fs, std::io, std::path are sufficient. The `Time` value
//   produced by io::modified_at uses whatever representation the `time`
//   module settles on; for now wrap a SystemTime.
```

## Function Surface

### `io` (must register)

`print`, `println`, `eprint`, `eprintln`, `read_line`, `read_all`,
`read_file`, `write_file`, `append_file`, `read_lines`, `read_bytes`,
`write_bytes`, `exists`, `is_file`, `is_dir`, `file_size`, `modified_at`,
`list_dir`, `list_dir_recursive`, `make_dir`, `make_dir_all`,
`remove_file`, `remove_dir`, `remove_dir_all`, `rename`, `copy`,
`file_writer`, `write` (FileWriter), `writeln`, `flush`, `close`.

> Note: `print`, `println`, `eprint`, `eprintln`, `read_line` already exist
> in `lib.rs`. The `io` module must register them under their `io::`
> namespace as well (e.g., `io_print`, `io_println`). The integration step
> may keep both registrations.

### `path` (must register)

`join`, `parent`, `filename`, `stem`, `extension`, `with_extension`,
`is_absolute`, `is_relative`, `normalize`, `relative_to`, `components`.

## Implementation Notes

- All `io::read_*` / `write_*` translate `std::io::Error` into native
  error strings keyed on `ErrorKind`:
  - `NotFound` → `"IoError::NotFound { path: ... }"`
  - `PermissionDenied`, `AlreadyExists`, etc.
- `read_lines` returns `List<Str>` split on `\n` (and strips `\r`).
- `list_dir` returns filenames (last component), not full paths.
- `list_dir_recursive` does a depth-first walk, returns paths relative to
  the input directory.
- `modified_at` returns a `Time` value (wraps `SystemTime` via the
  representation chosen by the `time` task).
- `file_writer` wraps `BufWriter<File>` in `Rc<RefCell<...>>` so multiple
  `write` calls accumulate before `flush`/`close`.
- `path::join(base, parts)` is variadic — accept `(Str, List<Str>)` and
  fold.
- `path::components` splits on `/` (or `\\` on Windows) preserving
  semantics.
- `path::normalize` resolves `.` and `..` lexically, no I/O.
- `path::relative_to(p, base)` returns `Err` if `p` is not a descendant of
  `base`.
- `path::extension("main.rs")` → `Some("rs")`; `path::stem("main.rs")` →
  `Some("main")`.

## Tests

### `io` tests

Use `tempfile::tempdir()` *if added as a dev-dep*; otherwise
manually build paths under `std::env::temp_dir()` with a unique prefix
and clean up in each test.

- `write_file` then `read_file` roundtrips
- `append_file` extends; `read_lines` returns N lines
- `read_bytes` / `write_bytes` roundtrip on binary
- `exists`, `is_file`, `is_dir`, `file_size`
- `make_dir`, `make_dir_all`, `remove_file`, `remove_dir`, `remove_dir_all`
- `rename` moves a file
- `copy` returns byte count
- `read_file` on missing path errs with `NotFound`
- `list_dir` returns short names; `list_dir_recursive` walks
- `file_writer` + multiple `write` + `close` produces expected file

### `path` tests

- `join("a", ["b", "c"])` → `"a/b/c"` (or `"a\\b\\c"` on Windows)
- `parent("a/b/c")` → `Some("a/b")`; `parent("a")` → `None` (or `Some("")`,
  document choice)
- `filename("a/b.txt")` → `Some("b.txt")`
- `stem("a/b.txt")` → `Some("b")`
- `extension("a/b.txt")` → `Some("txt")`; `extension("Makefile")` → `None`
- `with_extension("a/b.txt", "rs")` → `"a/b.rs"`
- `is_absolute("/foo")` → true on Unix; `is_relative("foo")` → true
- `normalize("a/./b/../c")` → `"a/c"`
- `relative_to("/a/b/c", "/a")` → `Ok("b/c")`; `relative_to("/x", "/a")`
  → `Err`
- `components("a/b/c")` → `["a", "b", "c"]`

Aim for ~30 tests combined. Avoid leaving temp files on failure; use
RAII guards.

## Constraints

- Do **not** modify `lib.rs` or `Cargo.toml`.
- The existing `print`/`println`/`read_line` natives in `lib.rs` are
  untouched — your `io` module registers `io_print` etc. alongside them.

## Acceptance

- Two files with the listed register coverage.
- Tests cover happy + error + edge paths.
- Tests clean up temp resources reliably.
