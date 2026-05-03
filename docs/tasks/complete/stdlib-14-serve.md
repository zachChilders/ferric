# Task: Stdlib — `serve`

> Read `docs/tasks/stdlib-00-overview.md` first for the global rules.

## Scope

Implement the `serve` module from `docs/stdlib.md` (section "`serve` — HTTP
Server"). Fastify-style: routes are declared up front, then `listen`.

## Output File

- `crates/ferric_stdlib/src/serve.rs`

Exposes `pub fn register(registry, interner)`.

## Required `NativeValue` Variants

```rust
// REQUIRED NATIVE VALUE VARIANTS:
//   Bytes(Vec<u8>)                                 // shared
//   Map(BTreeMap<MapKey, NativeValue>)             // shared
//   Json(Box<JsonRepr>)                            // shared
//   ServeServer(Rc<RefCell<ServerRepr>>)
//   ServeRequest(Rc<RequestRepr>)
//   ServeResponse(Rc<RefCell<ResponseRepr>>)
//
// pub struct ServerRepr {
//     pub routes: Vec<Route>,
//     pub middleware: Vec<MiddlewareSpec>,
//     pub mounts: Vec<(String, Rc<RefCell<ServerRepr>>)>,
// }
//
// pub struct Route {
//     pub method: String,           // or Vec<String> for `any`
//     pub path: String,             // with :param syntax
//     pub handler: HandlerFn,
// }
//
// pub struct RequestRepr {
//     pub method: String,
//     pub path: String,
//     pub query: BTreeMap<String, String>,
//     pub params: BTreeMap<String, String>,
//     pub headers: BTreeMap<String, String>,
//     pub body: Vec<u8>,
// }
//
// pub struct ResponseRepr {
//     pub status: u16,
//     pub headers: BTreeMap<String, String>,
//     pub body: Vec<u8>,
// }
```

## Required Deps

```rust
// REQUIRED DEPS:
//   tiny_http = "0.12"
//   (or) hyper = { version = "1", features = ["http1", "server"] } + bytes
//
// tiny_http is simpler for the blocking, single-threaded path. Switch to
// hyper when async lands.
```

## Function Surface (must register)

Construction:

`new`.

Routes:

`get`, `post`, `put`, `patch`, `delete`, `any`.

Middleware:

`use`, `use_prefix`.

Mounting:

`mount`.

Lifecycle:

`listen`, `listen_addr`.

Request introspection:

`method`, `path`, `query`, `param`, `header_get`, `body_bytes`, `body_str`,
`body_json`.

Response builders:

`response`, `ok`, `ok_str`, `ok_json`, `created`, `no_content`,
`bad_request`, `unauthorized`, `forbidden`, `not_found`, `internal_error`,
`with_header`, `with_headers`.

## Implementation Notes

- The `Server` value is mutable through `Rc<RefCell<ServerRepr>>`. Each
  `serve::get/post/etc.` mutates the inner routes and returns the same
  handle (so the spec's pipe-style reads naturally).
- Path matching: support `:name` segments for path params (e.g., `/users/:id`).
- Middleware: each middleware is `Fn(Request, NextFn) -> Response`. Call
  middleware in registration order, ending with the handler.
- `mount(parent, prefix, sub)` flattens `sub`'s routes into `parent` with
  the prefix prepended.
- `listen(s, port)` binds, accepts in a loop, dispatches synchronously.
  Returns only on error or shutdown.
- Request body: read fully into bytes; expose via `body_*` accessors.
- Response builders set sensible defaults (`ok_json` sets
  `Content-Type: application/json`).
- `bad_request(msg)` etc. produce JSON bodies of shape
  `{"error": <msg>}`.

## Tests

Pure-state tests (no actual listening) cover the bulk:

- `new()` returns a `Server` with empty routes.
- `serve::get(s, "/x", h)` mutates and returns same handle.
- Mounting: `mount(parent, "/api", sub)` exposes `sub`'s routes under
  `/api/...`.
- Middleware ordering: register `m1`, `m2`, then a route. Synthesize a
  request and trace the call order: m1 → m2 → handler.
- Response builders:
  - `ok_str("hi")` → status 200, content-type `text/plain`, body `"hi"`.
  - `ok_json(Json::Int(1))` → status 200, content-type
    `application/json`, body `"1"`.
  - `created(j)` → 201; `no_content()` → 204.
  - `bad_request("nope")` → 400, body `{"error":"nope"}`.
  - `not_found(...)` → 404; etc.
- `with_header(r, "X", "y")` mutates and returns.

End-to-end (one or two tests, port-bound):

- Bind to `127.0.0.1:0`, register `GET /ping → ok_str("pong")`, spawn
  `listen` in a thread, hit it with `reqwest::blocking` (or raw `TcpStream`),
  assert 200/`pong`. Tear the thread down by shutting the listener.

Path-param test:

- `GET /users/:id`; request `GET /users/42`. Inside the handler, assert
  `param(req, "id") == Some("42")`.

Aim for ~20 tests.

## Constraints

- Do **not** modify `lib.rs` or `Cargo.toml`.
- `Server` is the only mutable handle; per the no-global-mutable-state
  rule, it is owned by the caller and passed in explicitly.

## Acceptance

- One file with the listed register coverage.
- Pure-state tests cover routing, middleware, mount, response builders.
- At least one end-to-end test confirming bind + serve + respond.
