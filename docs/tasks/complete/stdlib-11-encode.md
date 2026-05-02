# Task: Stdlib — `encode`

> Read `docs/tasks/stdlib-00-overview.md` first for the global rules.

## Scope

Implement the `encode` module from `docs/stdlib.md` (section "`encode` —
Encoding / Decoding"). Base64, hex, URL-encoding, HTML entities, simple CSV.

## Output File

- `crates/ferric_stdlib/src/encode.rs`

Exposes `pub fn register(registry, interner)`.

## Required `NativeValue` Variants

```rust
// REQUIRED NATIVE VALUE VARIANTS:
//   Bytes(Vec<u8>)        // shared with str-bytes task
```

## Required Deps

```rust
// REQUIRED DEPS:
//   base64 = "0.22"
//   hex = "0.4"
//   urlencoding = "2"
//   (HTML escape and CSV are hand-rolled — small enough not to pull a crate.)
```

## Function Surface (must register)

Base64:

`base64`, `base64_url`, `base64_decode`, `base64_url_decode`.

Hex:

`hex`, `hex_upper`, `hex_decode`.

URL:

`url_encode`, `url_decode`, `url_encode_component`, `url_decode_component`.

HTML:

`html_escape`, `html_unescape`.

CSV:

`csv_row`, `csv_parse_row`.

## Implementation Notes

- `base64(b)` standard alphabet, with padding.
- `base64_url(b)` URL-safe alphabet (`-` and `_`), no padding.
- `base64_decode` accepts the standard alphabet only; mismatched alphabet
  errs `EncodeError::InvalidBase64`.
- `hex(b)` lowercase; `hex_upper(b)` uppercase.
- `hex_decode` accepts both cases; rejects odd length.
- `url_encode(s)` encodes a full URL — preserves reserved chars
  (`/?:#[]@!$&'()*+,;=`).
- `url_encode_component(s)` encodes a path/query component — also encodes
  reserved chars.
- `url_decode` errs on malformed `%XX` sequences.
- `html_escape(s)`: `&` → `&amp;` (must run first), then `<` → `&lt;`,
  `>` → `&gt;`, `"` → `&quot;`, `'` → `&#39;`.
- `html_unescape(s)`: reverse of the above; do not implement full named
  entity table — only the five above.
- `csv_row(fields)`: join with commas; quote any field containing `,`,
  `"`, `\n`, or `\r`. Inside a quoted field, escape `"` as `""`.
- `csv_parse_row(s)`: parse one CSV row, handling quoted fields and
  escaped quotes. No multi-line support.

## Tests

- Base64 roundtrip on `b"Hello"` and on random bytes
- `base64(b"")` → `""`; `base64_decode("")` → `Bytes(b"")`
- `base64_url(b"\xff\xff\xff")` produces no `+` or `/`
- Hex roundtrip; `hex_decode("ZZ")` errs; `hex_decode("a")` errs (odd len)
- `hex_upper(b"\x01\x0f")` → `"010F"`
- URL encode: `url_encode_component("a b/c?")` → `"a%20b%2Fc%3F"`
- URL roundtrip via `url_encode_component` / `url_decode_component`
- `url_decode("%ZZ")` errs
- HTML: `html_escape("<a href=\"x\">")` → `"&lt;a href=&quot;x&quot;&gt;"`
- HTML roundtrip via escape→unescape on a known string
- CSV: `csv_row(["a", "b,c", "d\"e"])` → `"a,\"b,c\",\"d\"\"e\""`
- CSV: `csv_parse_row("a,\"b,c\",\"d\"\"e\"")` roundtrips
- CSV: empty row `csv_parse_row("")` → `[""]` (document choice)

Aim for ~25 tests.

## Constraints

- Do **not** modify `lib.rs` or `Cargo.toml`.

## Acceptance

- One file with the listed register coverage.
- Tests cover encode/decode roundtrips and error paths for each codec.
