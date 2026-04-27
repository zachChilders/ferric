# Task: Stdlib — `http`

> Read `docs/tasks/stdlib-00-overview.md` first for the global rules.

## Scope

Implement the `http` module from `docs/stdlib.md` (section "`http` — HTTP
Client"). Fetch-inspired client. Every response carries status + typed
body; non-2xx is not an exception.

## Output File

- `crates/ferric_stdlib/src/http.rs`

Exposes `pub fn register(registry, interner)`.

## Required `NativeValue` Variants

```rust
// REQUIRED NATIVE VALUE VARIANTS:
//   Bytes(Vec<u8>)                                 // shared
//   Map(BTreeMap<MapKey, NativeValue>)             // shared
//   Json(Box<JsonRepr>)                            // shared with json task
//   HttpResponse(Rc<ResponseRepr>)
//   HttpRequestBuilder(Rc<RefCell<RequestBuilderRepr>>)
//
// where:
//   pub struct ResponseRepr {
//       pub status: u16,
//       pub headers: BTreeMap<String, String>,
//       pub body: Vec<u8>,
//   }
//
//   pub struct RequestBuilderRepr {
//       pub method: String,
//       pub url: String,
//       pub headers: BTreeMap<String, String>,
//       pub body: Vec<u8>,
//       pub timeout_ms: Option<u64>,
//       pub follow_redirects: bool,
//   }
```

## Required Deps

```rust
// REQUIRED DEPS:
//   reqwest = { version = "0.12", default-features = false, features = ["blocking", "rustls-tls", "json", "gzip"] }
//
// We use the blocking client. Async support arrives with the async
// runtime (M3+).
```

## Function Surface (must register)

Quick helpers:

`get`, `post`, `put`, `patch`, `delete`.

Builder:

`request`, `header`, `headers`, `body_bytes`, `body_str`, `body_json`,
`timeout`, `follow_redirects`, `basic_auth`, `bearer`, `send`.

Response:

`status`, `ok`, `header_get`, `headers_all`, `body_bytes`, `body_str`,
`body_json`.

Typed convenience:

`get_json`, `post_json`.

## Implementation Notes

- Construct one shared `reqwest::blocking::Client` per process behind a
  `OnceLock<Client>` (no global mutable state — `OnceLock` is initialize-once,
  read-many; this satisfies the "no globals" rule the same way `log::new`
  does).
- Errors map:
  - URL parse failure → `HttpError::InvalidUrl`
  - Connection refused / DNS → `HttpError::ConnectionFailed`
  - Timeout → `HttpError::Timeout`
  - TLS → `HttpError::TlsError`
  - Redirect loop → `HttpError::RedirectLoop`
- `body_json(req, val)` sets `Content-Type: application/json` and serializes
  via `json::to_str`.
- `body_str(req, s)` sets body bytes from UTF-8.
- `header_get(r, name)` is case-insensitive on header names.
- `body_str(r)` returns `Result<Str, StrError>` (UTF-8 fails if body is
  binary).
- `body_json(r)` parses the body as JSON.
- `get_json` / `post_json` — convenience wrappers that auto-set/parse JSON
  headers and body.

## Tests

The trick: testing an HTTP client without flaky external dependencies.

**Use a local mock server for tests.** Spawn a `tiny_http` listener (or
`hyper` minimal server) on `127.0.0.1:0`, capture the assigned port, and
issue requests against it. Tear down per-test.

If `tiny_http` adds too much weight, alternatively gate tests behind
`#[cfg(feature = "integration")]` and provide unit tests that exercise
**non-network** paths only (URL parsing, builder mutation, error mapping).

Required tests:

- `request("GET", url)` returns a builder with method/URL set.
- `header(req, "X-Foo", "bar")` mutates and returns the builder.
- `body_json(req, val)` sets `Content-Type: application/json`.
- `basic_auth(req, "u", "p")` sets `Authorization: Basic dTpw`.
- `bearer(req, "tok")` sets `Authorization: Bearer tok`.
- `timeout(req, 100)` stores 100ms.
- `follow_redirects(req, false)` sets the flag.
- `status` extracts the int; `ok(200)` true, `ok(500)` false, `ok(299)`
  true.
- `header_get` is case-insensitive.
- `body_bytes` returns the raw payload.
- `body_str` errs on non-UTF-8.
- `body_json` parses JSON; errs on non-JSON.
- **Integration (mock server):** `get(url) → 200`, body matches expected.
- **Integration:** `post_json(url, val)` round-trips JSON body.
- **Integration:** Redirect handling on/off.
- **Integration:** Timeout fires (`timeout(req, 1)` against a 100ms slow
  server) → `HttpError::Timeout`.

Aim for ~20 tests, with the integration ones grouped under a
`#[cfg(test)] mod integration_tests` you can `#[ignore]` if needed.

## Constraints

- Do **not** modify `lib.rs` or `Cargo.toml`.
- `OnceLock<reqwest::Client>` is acceptable (it is initialize-once, not
  mutable).

## Acceptance

- One file with the listed register coverage.
- Builder mutation paths fully unit-tested.
- At least one end-to-end integration test against a local mock server.
