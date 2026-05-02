# Task: Stdlib — `str` and `bytes`

> Read `docs/tasks/stdlib-00-overview.md` first for the global rules.

## Scope

Implement the `str` and `bytes` modules from `docs/stdlib.md` (sections "`str`
— String Manipulation" and "`bytes` — Raw Byte Sequences").

These two modules are paired because they cross-reference each other:
`str::to_bytes`, `str::from_bytes`, `str::from_bytes_lossy` produce/consume
`Bytes`; `bytes::to_str` produces `Str`. They also share UTF-8 validation
logic.

## Output Files

- `crates/ferric_stdlib/src/str_.rs` (the `str` module — file is `str_` to
  avoid the Rust reserved word)
- `crates/ferric_stdlib/src/bytes.rs`

Each file exposes one public entry:

```rust
pub fn register(registry: &mut NativeRegistry, interner: &mut Interner) { ... }
```

## Required `NativeValue` Variants

Place this comment block at the top of `bytes.rs`:

```rust
// REQUIRED NATIVE VALUE VARIANTS:
//   Bytes(Vec<u8>)
//   BytesMut(Rc<RefCell<Vec<u8>>>)
```

`str` does not need new variants — `NativeValue::Str(String)` already exists.

## Required Deps

None for `str`. For `bytes`, document needed deps in a comment:

```rust
// REQUIRED DEPS:
//   none — base64 and hex are needed by the `encode` module; route bytes::to_hex
//   etc. through the same crates the `encode` task selects (likely `base64` and
//   `hex`). For now use std + manual hex implementation.
```

If you reach for an external crate, add it to the comment so integration
picks it up.

## Function Surface

Implement every function listed under `str` and `bytes` in `docs/stdlib.md`,
preserving:

- the module-prefixed Ferric name (`str::trim` → register `str_trim`)
- argument names and order
- return types (`Result<...>`, `Option<...>`)

### `str` (must register)

`len`, `slice`, `contains`, `starts_with`, `ends_with`, `find`, `find_all`,
`split`, `split_once`, `lines`, `chars`, `trim`, `trim_start`, `trim_end`,
`trim_chars`, `to_upper`, `to_lower`, `to_title`, `pad_start`, `pad_end`,
`center`, `replace`, `replace_first`, `replace_n`, `join`, `repeat`,
`parse_int`, `parse_float`, `parse_bool`, `eq_ignore_case`, `to_bytes`,
`from_bytes`, `from_bytes_lossy`, `is_empty`, `is_ascii`, `is_numeric`.

### `bytes` (must register)

`new`, `from_hex`, `from_base64`, `repeat`, `len`, `is_empty`, `get`, `slice`,
`contains`, `find`, `concat`, `to_hex`, `to_base64`, `to_str`, `to_list`,
`from_list`, `builder`, `write_byte`, `write_bytes`, `write_str`,
`write_u16_le`, `write_u16_be`, `write_u32_le`, `write_u32_be`,
`write_i64_le`, `write_i64_be`, `build`.

## Implementation Notes

- `str::len` counts Unicode scalar values (`s.chars().count()`), not bytes.
- `str::slice(from, to)` is character-index based — convert to byte indices
  via `char_indices()` and return `StrError::OutOfBounds` if either bound is
  past `s.chars().count()`.
- `str::find` / `find_all` / `split` use character indices in the public
  contract.
- Errors are returned as native error values. For now represent them as
  `NativeValue::Str(format!("StrError::OutOfBounds {{ index: {idx}, len: {len} }}"))`
  inside the `Err` arm of `Result<T, String>` (the existing native ABI is
  `Result<NativeValue, String>` — keep that). The Ferric-side `StrError`
  enum is the *language-level* error; the Rust-side error string is what
  the VM converts into a `RuntimeError`.
- `bytes::builder` returns a `BytesMut` value. Use
  `Rc<RefCell<Vec<u8>>>` so subsequent `write_*` calls mutate through.
- Endianness writers use `to_le_bytes` / `to_be_bytes`.
- `bytes::to_hex` is lowercase. `bytes::from_hex` accepts both cases and
  rejects odd-length input.

## Tests

`#[cfg(test)] mod tests` in each file. Cover at minimum:

### `str` tests

- `len` returns char count, not byte count, for multi-byte input
  (`"café"` → 4)
- `slice` happy path + out-of-bounds error
- `contains`, `starts_with`, `ends_with` true/false
- `split("a,b,c", ",")` → 3 parts; `split("", ",")` edge
- `split_once("a=b=c", "=")` → `Some(("a", "b=c"))`
- `lines("a\r\nb\nc")` → 3 entries
- `trim*` variants strip whitespace
- `replace` replaces all; `replace_first` only first; `replace_n` exactly n
- `pad_start("7", 3, "0")` → `"007"`; `pad_end`, `center`
- `parse_int("42")` → `Ok(42)`; `parse_int("foo")` → `Err`
- `parse_float`, `parse_bool` happy + sad
- `eq_ignore_case("ABC", "abc")` → `true`
- `to_bytes` / `from_bytes` roundtrip
- `from_bytes` on invalid UTF-8 errs; `from_bytes_lossy` substitutes U+FFFD
- `is_empty`, `is_ascii`, `is_numeric` boundary cases

### `bytes` tests

- `from_hex("48656c6c6f")` → `Bytes(b"Hello")`; odd-length errs
- `to_hex` / `from_hex` roundtrip; case insensitivity on input
- `from_base64` / `to_base64` roundtrip
- `get` in-bounds + out-of-bounds
- `slice`, `concat`, `repeat`
- `to_str` on valid UTF-8 → `Ok`; on invalid → `Err`
- `to_list` / `from_list` roundtrip; `from_list` rejects values >255
- Builder: `builder` + `write_byte` ×3 + `build` produces 3-byte `Bytes`
- `write_u32_le(0x01020304)` produces `[0x04, 0x03, 0x02, 0x01]`
- `write_u32_be(0x01020304)` produces `[0x01, 0x02, 0x03, 0x04]`

Aim for ~30+ tests across the two files combined.

## Constraints

- Do **not** modify `crates/ferric_stdlib/src/lib.rs`.
- Do **not** modify `crates/ferric_stdlib/Cargo.toml`.
- Do **not** add a `mod str_;` / `mod bytes;` declaration anywhere — that
  is the integration step's job.
- Tests reference `NativeValue::Bytes` and `NativeValue::BytesMut` even
  though they don't exist yet. That is expected — they'll exist after
  integration.

## Acceptance

- Two files exist with the function surface above.
- Each module's `register` function registers every listed function.
- Each module has its own `#[cfg(test)] mod tests`.
- No edits outside the two new files.
