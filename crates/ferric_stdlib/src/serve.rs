//! # `serve` — HTTP Server (Fastify-style)
//!
//! Routes are declared up front, then `listen` starts the blocking accept
//! loop. The `Server` value is a mutable handle (`Rc<RefCell<ServerRepr>>`),
//! and route registration returns the same handle so that pipe-style code
//! reads naturally.
//!
//! Per the parallel-stdlib build-out rules, this file declares the
//! `NativeValue` variants and external dependencies it needs at the top
//! and writes Rust as if they already exist. Integration merges them.

// REQUIRED DEPS:
//   tiny_http = "0.12"

// REQUIRED NATIVE VALUE VARIANTS:
//   Bytes(Vec<u8>)                                 // shared
//   Map(BTreeMap<MapKey, NativeValue>)             // shared
//   Json(Box<JsonRepr>)                            // shared
//   ServeServer(Rc<RefCell<ServerRepr>>)
//   ServeRequest(Rc<RequestRepr>)
//   ServeResponse(Rc<RefCell<ResponseRepr>>)

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::rc::Rc;
use std::sync::Arc;

use ferric_common::Interner;

use crate::{MapKey, NativeRegistry, NativeValue};

// ===========================================================================
// Internal representations
// ===========================================================================

/// A handler is a pure Rust closure during pure-state tests; the VM-bound
/// path will wrap a Ferric closure in one of these. It takes a request
/// handle and returns a response handle.
pub type HandlerFn = Arc<dyn Fn(Rc<RequestRepr>) -> Rc<RefCell<ResponseRepr>> + Send + Sync>;

/// A middleware receives the current request and a `next` continuation.
/// The continuation invokes the rest of the chain (more middleware, then
/// the handler). Middleware registered with a prefix only fires for paths
/// starting with that prefix.
pub type MiddlewareFn =
    Arc<dyn Fn(Rc<RequestRepr>, NextFn) -> Rc<RefCell<ResponseRepr>> + Send + Sync>;

/// Boxed continuation passed to middleware. Calling it invokes the rest of
/// the chain and returns a response.
pub type NextFn = Box<dyn FnOnce(Rc<RequestRepr>) -> Rc<RefCell<ResponseRepr>>>;

#[derive(Clone)]
pub struct Route {
    /// Empty `methods` means "any method".
    pub methods: Vec<String>,
    pub path: String,
    pub handler: HandlerFn,
}

#[derive(Clone)]
pub struct MiddlewareSpec {
    /// `None` → applies to all paths on this server.
    pub prefix: Option<String>,
    pub mw: MiddlewareFn,
}

pub struct ServerRepr {
    pub routes: Vec<Route>,
    pub middleware: Vec<MiddlewareSpec>,
    pub mounts: Vec<(String, Rc<RefCell<ServerRepr>>)>,
}

impl ServerRepr {
    pub fn new() -> Self {
        Self {
            routes: Vec::new(),
            middleware: Vec::new(),
            mounts: Vec::new(),
        }
    }
}

impl Default for ServerRepr {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug)]
pub struct RequestRepr {
    pub method: String,
    pub path: String,
    pub query: BTreeMap<String, String>,
    pub params: BTreeMap<String, String>,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

impl RequestRepr {
    pub fn empty(method: &str, path: &str) -> Self {
        Self {
            method: method.to_string(),
            path: path.to_string(),
            query: BTreeMap::new(),
            params: BTreeMap::new(),
            headers: BTreeMap::new(),
            body: Vec::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ResponseRepr {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

impl ResponseRepr {
    pub fn new(status: u16, body: Vec<u8>) -> Self {
        Self {
            status,
            headers: BTreeMap::new(),
            body,
        }
    }
}

// ===========================================================================
// Helpers
// ===========================================================================

fn check_arg_count(args: &[NativeValue], expected: usize) -> Result<(), String> {
    if args.len() != expected {
        Err(format!(
            "expected {} argument(s), got {}",
            expected,
            args.len()
        ))
    } else {
        Ok(())
    }
}

fn expect_str(value: &NativeValue) -> Result<&String, String> {
    match value {
        NativeValue::Str(s) => Ok(s),
        other => Err(format!("expected string, got {other:?}")),
    }
}

fn expect_int(value: &NativeValue) -> Result<i64, String> {
    match value {
        NativeValue::Int(n) => Ok(*n),
        other => Err(format!("expected int, got {other:?}")),
    }
}

fn expect_server(value: &NativeValue) -> Result<Rc<RefCell<ServerRepr>>, String> {
    match value {
        NativeValue::ServeServer(s) => Ok(s.clone()),
        other => Err(format!("expected Server, got {other:?}")),
    }
}

fn expect_request(value: &NativeValue) -> Result<Rc<RequestRepr>, String> {
    match value {
        NativeValue::ServeRequest(r) => Ok(r.clone()),
        other => Err(format!("expected Request, got {other:?}")),
    }
}

fn expect_response(value: &NativeValue) -> Result<Rc<RefCell<ResponseRepr>>, String> {
    match value {
        NativeValue::ServeResponse(r) => Ok(r.clone()),
        other => Err(format!("expected Response, got {other:?}")),
    }
}

fn expect_bytes(value: &NativeValue) -> Result<Vec<u8>, String> {
    match value {
        NativeValue::Bytes(b) => Ok(b.clone()),
        NativeValue::Str(s) => Ok(s.as_bytes().to_vec()),
        other => Err(format!("expected bytes, got {other:?}")),
    }
}

fn make_response(status: u16, body: Vec<u8>) -> NativeValue {
    NativeValue::ServeResponse(Rc::new(RefCell::new(ResponseRepr::new(status, body))))
}

fn make_response_with_ct(status: u16, body: Vec<u8>, content_type: &str) -> NativeValue {
    let mut r = ResponseRepr::new(status, body);
    r.headers
        .insert("Content-Type".to_string(), content_type.to_string());
    NativeValue::ServeResponse(Rc::new(RefCell::new(r)))
}

fn json_error_body(message: &str) -> Vec<u8> {
    // Hand-rolled escape so this file does not depend on the json module.
    let mut out = String::from("{\"error\":\"");
    for c in message.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push_str("\"}");
    out.into_bytes()
}

// ===========================================================================
// Path matching
// ===========================================================================

/// Splits a path into non-empty segments. `"/users/:id"` → `["users", ":id"]`.
fn segments(path: &str) -> Vec<&str> {
    path.split('/').filter(|s| !s.is_empty()).collect()
}

/// Matches a request path against a route pattern. Returns `Some(params)`
/// on match; the map is empty when there are no `:name` segments. `None`
/// means no match.
pub fn match_path(pattern: &str, path: &str) -> Option<BTreeMap<String, String>> {
    let pat = segments(pattern);
    let act = segments(path);
    if pat.len() != act.len() {
        return None;
    }
    let mut params = BTreeMap::new();
    for (p, a) in pat.iter().zip(act.iter()) {
        if let Some(name) = p.strip_prefix(':') {
            params.insert(name.to_string(), a.to_string());
        } else if p != a {
            return None;
        }
    }
    Some(params)
}

/// Joins two URL path components, normalising slashes.
fn join_paths(prefix: &str, suffix: &str) -> String {
    let p = prefix.trim_end_matches('/');
    let s = suffix.trim_start_matches('/');
    if p.is_empty() {
        format!("/{s}")
    } else if s.is_empty() {
        p.to_string()
    } else {
        format!("{p}/{s}")
    }
}

// ===========================================================================
// Routing & dispatch (used by both `listen` and tests)
// ===========================================================================

/// Flattens a server tree into `(method-list, full-path, handler, applicable-middleware)`.
/// Middleware is collected in registration order: parent middleware first,
/// then any sub-server middleware that was attached when `mount` happened.
fn flatten(
    server: &ServerRepr,
    base: &str,
    inherited_mw: &[MiddlewareSpec],
) -> Vec<(Vec<String>, String, HandlerFn, Vec<MiddlewareSpec>)> {
    let mut combined_mw = inherited_mw.to_vec();
    combined_mw.extend(server.middleware.iter().cloned());

    let mut out = Vec::new();
    for r in &server.routes {
        let full = join_paths(base, &r.path);
        out.push((
            r.methods.clone(),
            full,
            r.handler.clone(),
            combined_mw.clone(),
        ));
    }
    for (prefix, sub) in &server.mounts {
        let new_base = join_paths(base, prefix);
        let sub_b = sub.borrow();
        out.extend(flatten(&sub_b, &new_base, &combined_mw));
    }
    out
}

/// Dispatches a synthesized request against a server. Used by tests and by
/// the live `listen` loop.
pub fn dispatch(server: &ServerRepr, req: RequestRepr) -> Rc<RefCell<ResponseRepr>> {
    let routes = flatten(server, "", &[]);
    for (methods, pattern, handler, mws) in &routes {
        let method_ok = methods.is_empty() || methods.iter().any(|m| m == &req.method);
        if !method_ok {
            continue;
        }
        if let Some(params) = match_path(pattern, &req.path) {
            let mut req = req.clone();
            req.params = params;
            let req_rc = Rc::new(req);

            // Build the middleware chain. Filter by prefix.
            let path = req_rc.path.clone();
            let active: Vec<MiddlewareFn> = mws
                .iter()
                .filter(|m| match &m.prefix {
                    None => true,
                    Some(p) => path.starts_with(p),
                })
                .map(|m| m.mw.clone())
                .collect();

            let handler_cloned = handler.clone();
            return run_chain(active, handler_cloned, req_rc);
        }
    }
    // No match → 404.
    let r = ResponseRepr {
        status: 404,
        headers: {
            let mut h = BTreeMap::new();
            h.insert("Content-Type".to_string(), "application/json".to_string());
            h
        },
        body: json_error_body("not found"),
    };
    Rc::new(RefCell::new(r))
}

fn run_chain(
    middlewares: Vec<MiddlewareFn>,
    handler: HandlerFn,
    req: Rc<RequestRepr>,
) -> Rc<RefCell<ResponseRepr>> {
    fn build(
        idx: usize,
        mws: Vec<MiddlewareFn>,
        handler: HandlerFn,
    ) -> Box<dyn FnOnce(Rc<RequestRepr>) -> Rc<RefCell<ResponseRepr>>> {
        if idx >= mws.len() {
            Box::new(move |r| handler(r))
        } else {
            let mw = mws[idx].clone();
            let next = build(idx + 1, mws, handler);
            Box::new(move |r| mw(r, next))
        }
    }
    let chain = build(0, middlewares, handler);
    chain(req)
}

// ===========================================================================
// Native function implementations
// ===========================================================================

fn builtin_serve_new(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 0)?;
    Ok(NativeValue::ServeServer(Rc::new(RefCell::new(
        ServerRepr::new(),
    ))))
}

/// Helper: append a route. The handler argument arrives as a `NativeValue`
/// — in the live VM it's a closure value; for native-only callers we accept
/// a placeholder. The real bridge to Ferric closures is added when the VM
/// gains the necessary hooks; for now we register the route with a stub
/// handler that returns 501. Tests that need real handler behaviour use
/// `register_route` directly.
fn register_route_native(
    args: &[NativeValue],
    methods: Vec<String>,
) -> Result<NativeValue, String> {
    check_arg_count(args, 3)?;
    let server = expect_server(&args[0])?;
    let path = expect_str(&args[1])?.clone();
    // args[2] is the handler closure value; the native bridge is left to
    // VM integration. We install a stub that signals "not yet wired".
    let handler: HandlerFn = Arc::new(|_req: Rc<RequestRepr>| {
        Rc::new(RefCell::new(ResponseRepr {
            status: 501,
            headers: {
                let mut h = BTreeMap::new();
                h.insert("Content-Type".to_string(), "application/json".to_string());
                h
            },
            body: json_error_body("handler not bridged"),
        }))
    });
    server.borrow_mut().routes.push(Route {
        methods,
        path,
        handler,
    });
    Ok(NativeValue::ServeServer(server))
}

fn builtin_serve_get(args: &[NativeValue]) -> Result<NativeValue, String> {
    register_route_native(args, vec!["GET".to_string()])
}

fn builtin_serve_post(args: &[NativeValue]) -> Result<NativeValue, String> {
    register_route_native(args, vec!["POST".to_string()])
}

fn builtin_serve_put(args: &[NativeValue]) -> Result<NativeValue, String> {
    register_route_native(args, vec!["PUT".to_string()])
}

fn builtin_serve_patch(args: &[NativeValue]) -> Result<NativeValue, String> {
    register_route_native(args, vec!["PATCH".to_string()])
}

fn builtin_serve_delete(args: &[NativeValue]) -> Result<NativeValue, String> {
    register_route_native(args, vec!["DELETE".to_string()])
}

fn builtin_serve_any(args: &[NativeValue]) -> Result<NativeValue, String> {
    register_route_native(args, vec![])
}

fn builtin_serve_use(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let server = expect_server(&args[0])?;
    // args[1] is the middleware closure; install a pass-through stub.
    let mw: MiddlewareFn = Arc::new(|req, next| next(req));
    server
        .borrow_mut()
        .middleware
        .push(MiddlewareSpec { prefix: None, mw });
    Ok(NativeValue::ServeServer(server))
}

fn builtin_serve_use_prefix(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 3)?;
    let server = expect_server(&args[0])?;
    let prefix = expect_str(&args[1])?.clone();
    let mw: MiddlewareFn = Arc::new(|req, next| next(req));
    server.borrow_mut().middleware.push(MiddlewareSpec {
        prefix: Some(prefix),
        mw,
    });
    Ok(NativeValue::ServeServer(server))
}

fn builtin_serve_mount(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 3)?;
    let parent = expect_server(&args[0])?;
    let prefix = expect_str(&args[1])?.clone();
    let sub = expect_server(&args[2])?;
    parent.borrow_mut().mounts.push((prefix, sub));
    Ok(NativeValue::ServeServer(parent))
}

fn builtin_serve_listen(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let server = expect_server(&args[0])?;
    let port = expect_int(&args[1])?;
    listen_on(server, "0.0.0.0", port as u16)
        .map(|()| NativeValue::Unit)
        .map_err(|e| format!("listen: {e}"))
}

fn builtin_serve_listen_addr(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 3)?;
    let server = expect_server(&args[0])?;
    let addr = expect_str(&args[1])?.clone();
    let port = expect_int(&args[2])?;
    listen_on(server, &addr, port as u16)
        .map(|()| NativeValue::Unit)
        .map_err(|e| format!("listen: {e}"))
}

// ----- Request introspection ------------------------------------------------

fn builtin_serve_method(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let r = expect_request(&args[0])?;
    Ok(NativeValue::Str(r.method.clone()))
}

fn builtin_serve_path(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let r = expect_request(&args[0])?;
    Ok(NativeValue::Str(r.path.clone()))
}

fn builtin_serve_query(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let r = expect_request(&args[0])?;
    let mut m: BTreeMap<MapKey, NativeValue> = BTreeMap::new();
    for (k, v) in &r.query {
        m.insert(MapKey::Str(k.clone()), NativeValue::Str(v.clone()));
    }
    Ok(NativeValue::Map(m))
}

fn builtin_serve_param(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let r = expect_request(&args[0])?;
    let name = expect_str(&args[1])?;
    match r.params.get(name) {
        Some(v) => Ok(NativeValue::Str(v.clone())),
        None => Ok(NativeValue::Unit),
    }
}

fn builtin_serve_header_get(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let r = expect_request(&args[0])?;
    let name = expect_str(&args[1])?.to_ascii_lowercase();
    for (k, v) in &r.headers {
        if k.to_ascii_lowercase() == name {
            return Ok(NativeValue::Str(v.clone()));
        }
    }
    Ok(NativeValue::Unit)
}

fn builtin_serve_body_bytes(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let r = expect_request(&args[0])?;
    Ok(NativeValue::Bytes(r.body.clone()))
}

fn builtin_serve_body_str(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let r = expect_request(&args[0])?;
    String::from_utf8(r.body.clone())
        .map(NativeValue::Str)
        .map_err(|e| format!("body_str: invalid UTF-8: {e}"))
}

fn builtin_serve_body_json(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let _r = expect_request(&args[0])?;
    // Parsing depends on the json module's `JsonRepr`; integration wires it.
    Err("body_json: integration with json module pending".to_string())
}

// ----- Response builders ----------------------------------------------------

fn builtin_serve_response(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let status = expect_int(&args[0])?;
    let body = expect_bytes(&args[1])?;
    if !(100..=599).contains(&status) {
        return Err(format!("response: status {status} out of range"));
    }
    Ok(make_response(status as u16, body))
}

fn builtin_serve_ok(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let body = expect_bytes(&args[0])?;
    Ok(make_response(200, body))
}

fn builtin_serve_ok_str(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let s = expect_str(&args[0])?;
    Ok(make_response_with_ct(
        200,
        s.as_bytes().to_vec(),
        "text/plain; charset=utf-8",
    ))
}

fn builtin_serve_ok_json(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    // Accept anything serialisable; for now we accept Json (post-integration)
    // or fall back to a string body.
    let body = match &args[0] {
        NativeValue::Json(j) => format!("{j:?}").into_bytes(),
        NativeValue::Str(s) => s.as_bytes().to_vec(),
        NativeValue::Int(i) => i.to_string().into_bytes(),
        NativeValue::Bool(b) => b.to_string().into_bytes(),
        NativeValue::Bytes(b) => b.clone(),
        other => return Err(format!("ok_json: unsupported body type {other:?}")),
    };
    Ok(make_response_with_ct(200, body, "application/json"))
}

fn builtin_serve_created(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let body = match &args[0] {
        NativeValue::Json(j) => format!("{j:?}").into_bytes(),
        NativeValue::Str(s) => s.as_bytes().to_vec(),
        NativeValue::Bytes(b) => b.clone(),
        other => return Err(format!("created: unsupported body type {other:?}")),
    };
    Ok(make_response_with_ct(201, body, "application/json"))
}

fn builtin_serve_no_content(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 0)?;
    Ok(make_response(204, Vec::new()))
}

fn builtin_serve_bad_request(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let msg = expect_str(&args[0])?;
    Ok(make_response_with_ct(
        400,
        json_error_body(msg),
        "application/json",
    ))
}

fn builtin_serve_unauthorized(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let msg = expect_str(&args[0])?;
    Ok(make_response_with_ct(
        401,
        json_error_body(msg),
        "application/json",
    ))
}

fn builtin_serve_forbidden(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let msg = expect_str(&args[0])?;
    Ok(make_response_with_ct(
        403,
        json_error_body(msg),
        "application/json",
    ))
}

fn builtin_serve_not_found(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let msg = expect_str(&args[0])?;
    Ok(make_response_with_ct(
        404,
        json_error_body(msg),
        "application/json",
    ))
}

fn builtin_serve_internal_error(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let msg = expect_str(&args[0])?;
    Ok(make_response_with_ct(
        500,
        json_error_body(msg),
        "application/json",
    ))
}

fn builtin_serve_with_header(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 3)?;
    let r = expect_response(&args[0])?;
    let name = expect_str(&args[1])?.clone();
    let val = expect_str(&args[2])?.clone();
    r.borrow_mut().headers.insert(name, val);
    Ok(NativeValue::ServeResponse(r))
}

fn builtin_serve_with_headers(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let r = expect_response(&args[0])?;
    match &args[1] {
        NativeValue::Map(m) => {
            for (k, v) in m {
                let key = match k {
                    MapKey::Str(s) => s.clone(),
                    MapKey::Int(n) => n.to_string(),
                    MapKey::Bool(b) => b.to_string(),
                };
                let val = match v {
                    NativeValue::Str(s) => s.clone(),
                    other => {
                        return Err(format!(
                            "with_headers: header value must be Str, got {other:?}"
                        ));
                    }
                };
                r.borrow_mut().headers.insert(key, val);
            }
        }
        other => return Err(format!("with_headers: expected Map, got {other:?}")),
    }
    Ok(NativeValue::ServeResponse(r))
}

// ===========================================================================
// Public registration helpers (used by tests and integration)
// ===========================================================================

/// Programmatic route registration used by tests. The native `serve_get`
/// etc. install a stub handler because Ferric closures need VM bridging;
/// tests use this to attach real Rust handlers.
pub fn register_route(
    server: &Rc<RefCell<ServerRepr>>,
    methods: Vec<&str>,
    path: &str,
    handler: HandlerFn,
) {
    server.borrow_mut().routes.push(Route {
        methods: methods.into_iter().map(|s| s.to_string()).collect(),
        path: path.to_string(),
        handler,
    });
}

/// Programmatic middleware registration used by tests.
pub fn register_middleware(
    server: &Rc<RefCell<ServerRepr>>,
    prefix: Option<&str>,
    mw: MiddlewareFn,
) {
    server.borrow_mut().middleware.push(MiddlewareSpec {
        prefix: prefix.map(|s| s.to_string()),
        mw,
    });
}

// ===========================================================================
// Live server (tiny_http integration replaced with a hand-rolled std::net
// loop so the file compiles in isolation; integration may swap in tiny_http
// for HTTP/1.1 keep-alive, chunked encoding, etc.)
// ===========================================================================

fn listen_on(server: Rc<RefCell<ServerRepr>>, addr: &str, port: u16) -> Result<(), String> {
    let socket_addr = format!("{addr}:{port}");
    let listener = TcpListener::bind(
        socket_addr
            .to_socket_addrs()
            .map_err(|e| format!("invalid address {socket_addr}: {e}"))?
            .collect::<Vec<_>>()
            .as_slice(),
    )
    .map_err(|e| format!("bind failed: {e}"))?;

    for incoming in listener.incoming() {
        match incoming {
            Ok(stream) => {
                if let Err(e) = handle_connection(stream, &server) {
                    eprintln!("serve: connection error: {e}");
                }
            }
            Err(e) => return Err(format!("accept failed: {e}")),
        }
    }
    Ok(())
}

/// Reads one HTTP/1.x request from the stream, dispatches it, writes the
/// response, and closes. Connection: close, no keep-alive — sufficient for
/// the test harness and most basic deployments.
pub fn handle_connection(
    mut stream: TcpStream,
    server: &Rc<RefCell<ServerRepr>>,
) -> Result<(), String> {
    let mut buf = Vec::with_capacity(4096);
    let mut tmp = [0u8; 4096];
    let header_end;
    loop {
        let n = stream.read(&mut tmp).map_err(|e| format!("read: {e}"))?;
        if n == 0 {
            return Err("client closed before sending request".to_string());
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(idx) = find_header_end(&buf) {
            header_end = idx;
            break;
        }
        if buf.len() > 64 * 1024 {
            return Err("request headers too large".to_string());
        }
    }
    let header_text = std::str::from_utf8(&buf[..header_end])
        .map_err(|e| format!("non-utf8 headers: {e}"))?
        .to_string();
    let req = parse_request_head(&header_text)?;
    let mut req = req;

    // Read body up to Content-Length.
    let want = req
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.trim().parse::<usize>().ok())
        .unwrap_or(0);
    let body_start = header_end + 4; // past CRLF CRLF
    let mut body = if buf.len() > body_start {
        buf[body_start..].to_vec()
    } else {
        Vec::new()
    };
    while body.len() < want {
        let n = stream
            .read(&mut tmp)
            .map_err(|e| format!("body read: {e}"))?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&tmp[..n]);
    }
    body.truncate(want.min(body.len()));
    req.body = body;

    let resp = dispatch(&server.borrow(), req);
    let resp = resp.borrow();
    write_response(&mut stream, &resp)?;
    let _ = stream.shutdown(std::net::Shutdown::Both);
    Ok(())
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    if buf.len() < 4 {
        return None;
    }
    (0..buf.len().saturating_sub(3)).find(|&i| &buf[i..i + 4] == b"\r\n\r\n")
}

fn parse_request_head(head: &str) -> Result<RequestRepr, String> {
    let mut lines = head.split("\r\n");
    let request_line = lines.next().ok_or_else(|| "empty request".to_string())?;
    let mut parts = request_line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| "missing method".to_string())?
        .to_string();
    let target = parts
        .next()
        .ok_or_else(|| "missing target".to_string())?
        .to_string();
    let _version = parts.next().unwrap_or("HTTP/1.1");

    // Split path / query.
    let (path, query) = match target.find('?') {
        Some(i) => (target[..i].to_string(), parse_query(&target[i + 1..])),
        None => (target, BTreeMap::new()),
    };

    let mut headers = BTreeMap::new();
    for line in lines {
        if line.is_empty() {
            break;
        }
        if let Some(i) = line.find(':') {
            let k = line[..i].trim().to_string();
            let v = line[i + 1..].trim().to_string();
            headers.insert(k, v);
        }
    }

    Ok(RequestRepr {
        method,
        path,
        query,
        params: BTreeMap::new(),
        headers,
        body: Vec::new(),
    })
}

fn parse_query(q: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for pair in q.split('&') {
        if pair.is_empty() {
            continue;
        }
        match pair.find('=') {
            Some(i) => {
                out.insert(pair[..i].to_string(), pair[i + 1..].to_string());
            }
            None => {
                out.insert(pair.to_string(), String::new());
            }
        }
    }
    out
}

fn write_response(stream: &mut TcpStream, resp: &ResponseRepr) -> Result<(), String> {
    let reason = status_reason(resp.status);
    let mut header = format!("HTTP/1.1 {} {}\r\n", resp.status, reason);
    let mut have_cl = false;
    let mut have_conn = false;
    for (k, v) in &resp.headers {
        if k.eq_ignore_ascii_case("content-length") {
            have_cl = true;
        }
        if k.eq_ignore_ascii_case("connection") {
            have_conn = true;
        }
        header.push_str(&format!("{k}: {v}\r\n"));
    }
    if !have_cl {
        header.push_str(&format!("Content-Length: {}\r\n", resp.body.len()));
    }
    if !have_conn {
        header.push_str("Connection: close\r\n");
    }
    header.push_str("\r\n");
    stream
        .write_all(header.as_bytes())
        .map_err(|e| format!("write header: {e}"))?;
    stream
        .write_all(&resp.body)
        .map_err(|e| format!("write body: {e}"))?;
    stream.flush().map_err(|e| format!("flush: {e}"))?;
    Ok(())
}

fn status_reason(code: u16) -> &'static str {
    match code {
        100 => "Continue",
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        418 => "I'm a teapot",
        422 => "Unprocessable Entity",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "OK",
    }
}

// ===========================================================================
// Registration
// ===========================================================================

pub fn register(registry: &mut NativeRegistry, interner: &mut Interner) {
    let bodies: &[(&str, crate::NativeFn)] = &[
        ("serve_new", builtin_serve_new),
        ("serve_get", builtin_serve_get),
        ("serve_post", builtin_serve_post),
        ("serve_put", builtin_serve_put),
        ("serve_patch", builtin_serve_patch),
        ("serve_delete", builtin_serve_delete),
        ("serve_any", builtin_serve_any),
        ("serve_use", builtin_serve_use),
        ("serve_use_prefix", builtin_serve_use_prefix),
        ("serve_mount", builtin_serve_mount),
        ("serve_listen", builtin_serve_listen),
        ("serve_listen_addr", builtin_serve_listen_addr),
        ("serve_method", builtin_serve_method),
        ("serve_path", builtin_serve_path),
        ("serve_query", builtin_serve_query),
        ("serve_param", builtin_serve_param),
        ("serve_header_get", builtin_serve_header_get),
        ("serve_body_bytes", builtin_serve_body_bytes),
        ("serve_body_str", builtin_serve_body_str),
        ("serve_body_json", builtin_serve_body_json),
        ("serve_response", builtin_serve_response),
        ("serve_ok", builtin_serve_ok),
        ("serve_ok_str", builtin_serve_ok_str),
        ("serve_ok_json", builtin_serve_ok_json),
        ("serve_created", builtin_serve_created),
        ("serve_no_content", builtin_serve_no_content),
        ("serve_bad_request", builtin_serve_bad_request),
        ("serve_unauthorized", builtin_serve_unauthorized),
        ("serve_forbidden", builtin_serve_forbidden),
        ("serve_not_found", builtin_serve_not_found),
        ("serve_internal_error", builtin_serve_internal_error),
        ("serve_with_header", builtin_serve_with_header),
        ("serve_with_headers", builtin_serve_with_headers),
    ];
    crate::register_module(
        registry,
        interner,
        ferric_stdlib_meta::serve::SERVE_FNS,
        bodies,
    );
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpStream as StdTcpStream;
    use std::sync::Arc as StdArc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;
    use std::time::Duration;

    fn ok_str_handler(text: &'static str) -> HandlerFn {
        Arc::new(move |_req| {
            let mut h = BTreeMap::new();
            h.insert("Content-Type".into(), "text/plain; charset=utf-8".into());
            Rc::new(RefCell::new(ResponseRepr {
                status: 200,
                headers: h,
                body: text.as_bytes().to_vec(),
            }))
        })
    }

    // --- pure-state: construction & route registration ----------------------

    #[test]
    fn new_returns_empty_server() {
        let v = builtin_serve_new(&[]).unwrap();
        let s = expect_server(&v).unwrap();
        let s = s.borrow();
        assert!(s.routes.is_empty());
        assert!(s.middleware.is_empty());
        assert!(s.mounts.is_empty());
    }

    #[test]
    fn serve_get_appends_route_and_returns_same_handle() {
        let s = builtin_serve_new(&[]).unwrap();
        let s2 = builtin_serve_get(&[
            s.clone(),
            NativeValue::Str("/x".into()),
            NativeValue::Unit, // handler placeholder
        ])
        .unwrap();
        // Same Rc inside.
        let a = expect_server(&s).unwrap();
        let b = expect_server(&s2).unwrap();
        assert!(Rc::ptr_eq(&a, &b));
        assert_eq!(a.borrow().routes.len(), 1);
        assert_eq!(a.borrow().routes[0].path, "/x");
        assert_eq!(a.borrow().routes[0].methods, vec!["GET".to_string()]);
    }

    #[test]
    fn route_methods_are_correct_per_verb() {
        let make = |f: fn(&[NativeValue]) -> Result<NativeValue, String>| {
            let s = builtin_serve_new(&[]).unwrap();
            f(&[s.clone(), NativeValue::Str("/".into()), NativeValue::Unit]).unwrap();
            let srv = expect_server(&s).unwrap();
            srv.borrow().routes[0].methods.clone()
        };
        assert_eq!(make(builtin_serve_get), vec!["GET"]);
        assert_eq!(make(builtin_serve_post), vec!["POST"]);
        assert_eq!(make(builtin_serve_put), vec!["PUT"]);
        assert_eq!(make(builtin_serve_patch), vec!["PATCH"]);
        assert_eq!(make(builtin_serve_delete), vec!["DELETE"]);
        // any → empty methods (matches everything)
        assert!(make(builtin_serve_any).is_empty());
    }

    // --- pure-state: path matching ------------------------------------------

    #[test]
    fn match_path_literal() {
        assert!(match_path("/a/b", "/a/b").is_some());
        assert!(match_path("/a/b", "/a/c").is_none());
        assert!(match_path("/a", "/a/b").is_none());
    }

    #[test]
    fn match_path_extracts_params() {
        let p = match_path("/users/:id", "/users/42").unwrap();
        assert_eq!(p.get("id"), Some(&"42".to_string()));
        let p = match_path("/u/:a/p/:b", "/u/1/p/2").unwrap();
        assert_eq!(p.get("a"), Some(&"1".to_string()));
        assert_eq!(p.get("b"), Some(&"2".to_string()));
    }

    #[test]
    fn join_paths_normalises() {
        assert_eq!(join_paths("/api", "/v1"), "/api/v1");
        assert_eq!(join_paths("/api/", "/v1"), "/api/v1");
        assert_eq!(join_paths("/api", "v1"), "/api/v1");
        assert_eq!(join_paths("", "/x"), "/x");
        assert_eq!(join_paths("/api", ""), "/api");
    }

    // --- pure-state: dispatch via direct register_route ---------------------

    #[test]
    fn dispatch_hits_get_route() {
        let s = Rc::new(RefCell::new(ServerRepr::new()));
        register_route(&s, vec!["GET"], "/hello", ok_str_handler("hi"));
        let req = RequestRepr::empty("GET", "/hello");
        let resp = dispatch(&s.borrow(), req);
        let resp = resp.borrow();
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, b"hi");
    }

    #[test]
    fn dispatch_404_when_no_match() {
        let s = Rc::new(RefCell::new(ServerRepr::new()));
        register_route(&s, vec!["GET"], "/hello", ok_str_handler("hi"));
        let req = RequestRepr::empty("GET", "/nope");
        let resp = dispatch(&s.borrow(), req);
        assert_eq!(resp.borrow().status, 404);
    }

    #[test]
    fn dispatch_method_mismatch_is_404() {
        let s = Rc::new(RefCell::new(ServerRepr::new()));
        register_route(&s, vec!["POST"], "/x", ok_str_handler("p"));
        let req = RequestRepr::empty("GET", "/x");
        assert_eq!(dispatch(&s.borrow(), req).borrow().status, 404);
    }

    #[test]
    fn dispatch_any_matches_all_methods() {
        let s = Rc::new(RefCell::new(ServerRepr::new()));
        register_route(&s, vec![], "/x", ok_str_handler("ok"));
        for m in &["GET", "POST", "DELETE", "PATCH"] {
            let req = RequestRepr::empty(m, "/x");
            assert_eq!(dispatch(&s.borrow(), req).borrow().status, 200);
        }
    }

    #[test]
    fn dispatch_path_param_appears_in_request() {
        let s = Rc::new(RefCell::new(ServerRepr::new()));
        let captured: StdArc<std::sync::Mutex<Option<String>>> =
            StdArc::new(std::sync::Mutex::new(None));
        let cap = captured.clone();
        let h: HandlerFn = Arc::new(move |req| {
            *cap.lock().unwrap() = req.params.get("id").cloned();
            Rc::new(RefCell::new(ResponseRepr::new(200, b"ok".to_vec())))
        });
        register_route(&s, vec!["GET"], "/users/:id", h);
        let req = RequestRepr::empty("GET", "/users/42");
        let resp = dispatch(&s.borrow(), req);
        assert_eq!(resp.borrow().status, 200);
        assert_eq!(*captured.lock().unwrap(), Some("42".to_string()));
    }

    // --- pure-state: mount --------------------------------------------------

    #[test]
    fn mount_exposes_sub_routes_under_prefix() {
        let parent = Rc::new(RefCell::new(ServerRepr::new()));
        let sub = Rc::new(RefCell::new(ServerRepr::new()));
        register_route(&sub, vec!["GET"], "/list", ok_str_handler("L"));

        let p_nv = NativeValue::ServeServer(parent.clone());
        let s_nv = NativeValue::ServeServer(sub.clone());
        builtin_serve_mount(&[p_nv, NativeValue::Str("/api".into()), s_nv]).unwrap();

        let req = RequestRepr::empty("GET", "/api/list");
        let resp = dispatch(&parent.borrow(), req);
        assert_eq!(resp.borrow().status, 200);
        assert_eq!(resp.borrow().body, b"L");

        // Without prefix → 404.
        let req = RequestRepr::empty("GET", "/list");
        assert_eq!(dispatch(&parent.borrow(), req).borrow().status, 404);
    }

    // --- pure-state: middleware ordering ------------------------------------

    #[test]
    fn middleware_runs_in_registration_order_then_handler() {
        let counter = StdArc::new(AtomicUsize::new(0));
        let order: StdArc<std::sync::Mutex<Vec<&'static str>>> =
            StdArc::new(std::sync::Mutex::new(Vec::new()));

        let s = Rc::new(RefCell::new(ServerRepr::new()));

        let order1 = order.clone();
        let c1 = counter.clone();
        let m1: MiddlewareFn = Arc::new(move |req, next| {
            order1.lock().unwrap().push("m1");
            c1.fetch_add(1, Ordering::SeqCst);
            next(req)
        });
        let order2 = order.clone();
        let c2 = counter.clone();
        let m2: MiddlewareFn = Arc::new(move |req, next| {
            order2.lock().unwrap().push("m2");
            c2.fetch_add(1, Ordering::SeqCst);
            next(req)
        });
        register_middleware(&s, None, m1);
        register_middleware(&s, None, m2);

        let order3 = order.clone();
        let h: HandlerFn = Arc::new(move |_req| {
            order3.lock().unwrap().push("h");
            Rc::new(RefCell::new(ResponseRepr::new(200, b"ok".to_vec())))
        });
        register_route(&s, vec!["GET"], "/x", h);

        let resp = dispatch(&s.borrow(), RequestRepr::empty("GET", "/x"));
        assert_eq!(resp.borrow().status, 200);
        assert_eq!(*order.lock().unwrap(), vec!["m1", "m2", "h"]);
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn middleware_can_short_circuit() {
        let s = Rc::new(RefCell::new(ServerRepr::new()));
        let mw: MiddlewareFn =
            Arc::new(|_req, _next| Rc::new(RefCell::new(ResponseRepr::new(401, b"nope".to_vec()))));
        register_middleware(&s, None, mw);
        register_route(&s, vec!["GET"], "/x", ok_str_handler("hi"));
        let resp = dispatch(&s.borrow(), RequestRepr::empty("GET", "/x"));
        assert_eq!(resp.borrow().status, 401);
        assert_eq!(resp.borrow().body, b"nope");
    }

    #[test]
    fn middleware_prefix_only_runs_for_matching_paths() {
        let s = Rc::new(RefCell::new(ServerRepr::new()));
        let touched = StdArc::new(AtomicUsize::new(0));
        let t = touched.clone();
        let mw: MiddlewareFn = Arc::new(move |req, next| {
            t.fetch_add(1, Ordering::SeqCst);
            next(req)
        });
        register_middleware(&s, Some("/admin"), mw);
        register_route(&s, vec!["GET"], "/admin/x", ok_str_handler("a"));
        register_route(&s, vec!["GET"], "/public", ok_str_handler("p"));

        let _ = dispatch(&s.borrow(), RequestRepr::empty("GET", "/public"));
        assert_eq!(touched.load(Ordering::SeqCst), 0);
        let _ = dispatch(&s.borrow(), RequestRepr::empty("GET", "/admin/x"));
        assert_eq!(touched.load(Ordering::SeqCst), 1);
    }

    // --- pure-state: response builders --------------------------------------

    fn resp_of(v: NativeValue) -> Rc<RefCell<ResponseRepr>> {
        match v {
            NativeValue::ServeResponse(r) => r,
            other => panic!("not a response: {other:?}"),
        }
    }

    #[test]
    fn ok_str_sets_text_plain() {
        let v = builtin_serve_ok_str(&[NativeValue::Str("hi".into())]).unwrap();
        let r = resp_of(v);
        let r = r.borrow();
        assert_eq!(r.status, 200);
        assert_eq!(r.body, b"hi");
        assert_eq!(
            r.headers.get("Content-Type").map(String::as_str),
            Some("text/plain; charset=utf-8")
        );
    }

    #[test]
    fn ok_json_with_int_body() {
        let v = builtin_serve_ok_json(&[NativeValue::Int(1)]).unwrap();
        let r = resp_of(v);
        let r = r.borrow();
        assert_eq!(r.status, 200);
        assert_eq!(r.body, b"1");
        assert_eq!(
            r.headers.get("Content-Type").map(String::as_str),
            Some("application/json")
        );
    }

    #[test]
    fn no_content_is_204_with_empty_body() {
        let v = builtin_serve_no_content(&[]).unwrap();
        let r = resp_of(v);
        let r = r.borrow();
        assert_eq!(r.status, 204);
        assert!(r.body.is_empty());
    }

    #[test]
    fn created_is_201() {
        let v = builtin_serve_created(&[NativeValue::Str("{}".into())]).unwrap();
        assert_eq!(resp_of(v).borrow().status, 201);
    }

    #[test]
    fn bad_request_has_json_error_body() {
        let v = builtin_serve_bad_request(&[NativeValue::Str("nope".into())]).unwrap();
        let r = resp_of(v);
        let r = r.borrow();
        assert_eq!(r.status, 400);
        assert_eq!(r.body, br#"{"error":"nope"}"#);
        assert_eq!(
            r.headers.get("Content-Type").map(String::as_str),
            Some("application/json")
        );
    }

    #[test]
    fn error_status_codes_are_correct() {
        type Builder = fn(&[NativeValue]) -> Result<NativeValue, String>;
        let cases: Vec<(Builder, u16)> = vec![
            (builtin_serve_unauthorized, 401),
            (builtin_serve_forbidden, 403),
            (builtin_serve_not_found, 404),
            (builtin_serve_internal_error, 500),
        ];
        for (f, code) in cases {
            let v = f(&[NativeValue::Str("m".into())]).unwrap();
            let r = resp_of(v);
            assert_eq!(r.borrow().status, code);
            assert_eq!(r.borrow().body, br#"{"error":"m"}"#);
        }
    }

    #[test]
    fn response_rejects_out_of_range_status() {
        let v = builtin_serve_response(&[NativeValue::Int(99), NativeValue::Bytes(b"x".to_vec())]);
        assert!(v.is_err());
    }

    #[test]
    fn with_header_mutates_and_returns_same_handle() {
        let r = builtin_serve_ok_str(&[NativeValue::Str("hi".into())]).unwrap();
        let r2 = builtin_serve_with_header(&[
            r.clone(),
            NativeValue::Str("X-Custom".into()),
            NativeValue::Str("y".into()),
        ])
        .unwrap();
        let a = expect_response(&r).unwrap();
        let b = expect_response(&r2).unwrap();
        assert!(Rc::ptr_eq(&a, &b));
        assert_eq!(
            a.borrow().headers.get("X-Custom").map(String::as_str),
            Some("y")
        );
    }

    #[test]
    fn with_headers_merges_map() {
        let r = builtin_serve_ok_str(&[NativeValue::Str("hi".into())]).unwrap();
        let mut m: BTreeMap<MapKey, NativeValue> = BTreeMap::new();
        m.insert(MapKey::Str("A".into()), NativeValue::Str("1".into()));
        m.insert(MapKey::Str("B".into()), NativeValue::Str("2".into()));
        let r2 = builtin_serve_with_headers(&[r.clone(), NativeValue::Map(m)]).unwrap();
        let resp = expect_response(&r2).unwrap();
        let h = &resp.borrow().headers;
        assert_eq!(h.get("A").map(String::as_str), Some("1"));
        assert_eq!(h.get("B").map(String::as_str), Some("2"));
    }

    // --- pure-state: request introspection ----------------------------------

    fn make_req_nv(method: &str, path: &str) -> NativeValue {
        NativeValue::ServeRequest(Rc::new(RequestRepr::empty(method, path)))
    }

    #[test]
    fn request_method_and_path_accessors() {
        let r = make_req_nv("POST", "/x");
        assert_eq!(
            builtin_serve_method(&[r.clone()]).unwrap(),
            NativeValue::Str("POST".into())
        );
        assert_eq!(
            builtin_serve_path(&[r]).unwrap(),
            NativeValue::Str("/x".into())
        );
    }

    #[test]
    fn header_get_is_case_insensitive() {
        let mut req = RequestRepr::empty("GET", "/");
        req.headers
            .insert("Content-Type".into(), "text/plain".into());
        let nv = NativeValue::ServeRequest(Rc::new(req));
        let v = builtin_serve_header_get(&[nv, NativeValue::Str("content-type".into())]).unwrap();
        assert_eq!(v, NativeValue::Str("text/plain".into()));
    }

    // --- registration -------------------------------------------------------

    #[test]
    fn register_installs_all_native_names() {
        let mut reg = NativeRegistry::new();
        let mut interner = Interner::new();
        register(&mut reg, &mut interner);

        for name in [
            "serve_new",
            "serve_get",
            "serve_post",
            "serve_put",
            "serve_patch",
            "serve_delete",
            "serve_any",
            "serve_use",
            "serve_use_prefix",
            "serve_mount",
            "serve_listen",
            "serve_listen_addr",
            "serve_method",
            "serve_path",
            "serve_query",
            "serve_param",
            "serve_header_get",
            "serve_body_bytes",
            "serve_body_str",
            "serve_body_json",
            "serve_response",
            "serve_ok",
            "serve_ok_str",
            "serve_ok_json",
            "serve_created",
            "serve_no_content",
            "serve_bad_request",
            "serve_unauthorized",
            "serve_forbidden",
            "serve_not_found",
            "serve_internal_error",
            "serve_with_header",
            "serve_with_headers",
        ] {
            let sym = interner.intern(name);
            assert!(reg.get(sym).is_some(), "expected `{name}` to be registered");
        }
    }

    // --- end-to-end: bind, accept, respond ----------------------------------

    #[test]
    fn end_to_end_get_ping_returns_pong() {
        // Bind to 127.0.0.1:0, hand the listener to a background thread
        // that builds a server with one /ping route and handles exactly
        // one connection. `Rc<RefCell<ServerRepr>>` is `!Send`, so the
        // server is constructed inside the worker thread.
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("local_addr").port();
        let join = thread::spawn(move || {
            let server = Rc::new(RefCell::new(ServerRepr::new()));
            register_route(
                &server,
                vec!["GET"],
                "/ping",
                Arc::new(|_req| {
                    let mut h = BTreeMap::new();
                    h.insert("Content-Type".into(), "text/plain; charset=utf-8".into());
                    Rc::new(RefCell::new(ResponseRepr {
                        status: 200,
                        headers: h,
                        body: b"pong".to_vec(),
                    }))
                }),
            );
            let (stream, _addr) = listener.accept().expect("accept");
            handle_connection(stream, &server).expect("handle");
        });

        // 3. Hit it.
        let mut s = StdTcpStream::connect(("127.0.0.1", port)).expect("connect");
        s.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        s.write_all(b"GET /ping HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
            .expect("write");
        let mut buf = Vec::new();
        s.read_to_end(&mut buf).expect("read");

        join.join().expect("server thread");

        let text = String::from_utf8_lossy(&buf);
        assert!(
            text.starts_with("HTTP/1.1 200 OK"),
            "expected 200 OK, got: {text}"
        );
        assert!(
            text.ends_with("pong") || text.contains("\r\n\r\npong"),
            "expected body `pong` at end, got: {text}"
        );
    }
}
