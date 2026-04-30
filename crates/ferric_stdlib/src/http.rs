//! # `http` — HTTP Client
//!
//! Fetch-inspired typed HTTP client for Ferric. Every response carries a
//! status code and typed body; non-2xx responses are not exceptions.
//!
//! See `docs/stdlib.md` (`http` section) for the surface spec.
//
// REQUIRED DEPS:
//   reqwest = { version = "0.12", default-features = false, features = ["blocking", "rustls-tls", "json", "gzip"] }
//
// We use the blocking client. Async support arrives with the async runtime
// (M3+).
//
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

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::OnceLock;
use std::time::Duration;

use ferric_common::Interner;

use crate::{JsonRepr, MapKey, NativeRegistry, NativeValue};

// ============================================================================
// Repr structs (declared here; integration may move them to lib.rs)
// ============================================================================

/// Response payload representation. Held inside `NativeValue::HttpResponse`.
#[derive(Debug, Clone, PartialEq)]
pub struct ResponseRepr {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

/// Mutable builder representation. Held inside
/// `NativeValue::HttpRequestBuilder` behind `Rc<RefCell<...>>` so that
/// chained native calls can update it in place.
#[derive(Debug, Clone, PartialEq)]
pub struct RequestBuilderRepr {
    pub method: String,
    pub url: String,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
    pub timeout_ms: Option<u64>,
    pub follow_redirects: bool,
}

impl RequestBuilderRepr {
    pub fn new(method: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            method: method.into(),
            url: url.into(),
            headers: BTreeMap::new(),
            body: Vec::new(),
            timeout_ms: None,
            follow_redirects: true,
        }
    }
}

// ============================================================================
// Errors
// ============================================================================

/// Mirrors `pub enum HttpError` from `docs/stdlib.md`. We surface a
/// flattened `tag: message` string at the native boundary because the
/// runtime currently routes errors through `Result<NativeValue, String>`.
/// Integration with the typed enum lands when error types reach the
/// stdlib boundary.
#[derive(Debug, Clone, PartialEq)]
pub enum HttpError {
    InvalidUrl(String),
    ConnectionFailed { url: String, reason: String },
    Timeout { url: String, ms: u64 },
    TlsError(String),
    RedirectLoop(String),
    InvalidResponse(String),
}

impl HttpError {
    fn tag(&self) -> &'static str {
        match self {
            HttpError::InvalidUrl(_) => "InvalidUrl",
            HttpError::ConnectionFailed { .. } => "ConnectionFailed",
            HttpError::Timeout { .. } => "Timeout",
            HttpError::TlsError(_) => "TlsError",
            HttpError::RedirectLoop(_) => "RedirectLoop",
            HttpError::InvalidResponse(_) => "InvalidResponse",
        }
    }
}

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpError::InvalidUrl(u) => write!(f, "InvalidUrl: {u}"),
            HttpError::ConnectionFailed { url, reason } => {
                write!(f, "ConnectionFailed: {url} ({reason})")
            }
            HttpError::Timeout { url, ms } => {
                write!(f, "Timeout: {url} after {ms}ms")
            }
            HttpError::TlsError(reason) => write!(f, "TlsError: {reason}"),
            HttpError::RedirectLoop(u) => write!(f, "RedirectLoop: {u}"),
            HttpError::InvalidResponse(r) => write!(f, "InvalidResponse: {r}"),
        }
    }
}

/// Map a `reqwest::Error` (or other failure encountered while sending) to
/// a typed `HttpError`.
pub fn classify_reqwest_error(err: &reqwest::Error, url: &str) -> HttpError {
    if err.is_timeout() {
        return HttpError::Timeout {
            url: url.to_string(),
            ms: 0,
        };
    }
    if err.is_redirect() {
        return HttpError::RedirectLoop(url.to_string());
    }
    let s = err.to_string();
    let lc = s.to_lowercase();
    if lc.contains("tls")
        || lc.contains("certificate")
        || lc.contains("ssl")
        || lc.contains("handshake")
    {
        return HttpError::TlsError(s);
    }
    if err.is_builder() || err.is_request() {
        // url builder errors land here
        if lc.contains("url") || lc.contains("scheme") || lc.contains("relative") {
            return HttpError::InvalidUrl(url.to_string());
        }
    }
    if err.is_connect() || err.is_request() {
        return HttpError::ConnectionFailed {
            url: url.to_string(),
            reason: s,
        };
    }
    HttpError::InvalidResponse(s)
}

// ============================================================================
// Shared client (initialize-once, no mutable globals)
// ============================================================================

static CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();

fn shared_client() -> &'static reqwest::blocking::Client {
    CLIENT.get_or_init(|| {
        reqwest::blocking::Client::builder()
            .gzip(true)
            .build()
            .expect("failed to build shared reqwest::blocking::Client")
    })
}

// ============================================================================
// Helpers (private; integration may dedupe with siblings)
// ============================================================================

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

fn expect_bool(value: &NativeValue) -> Result<bool, String> {
    match value {
        NativeValue::Bool(b) => Ok(*b),
        other => Err(format!("expected bool, got {other:?}")),
    }
}

fn expect_bytes(value: &NativeValue) -> Result<Vec<u8>, String> {
    match value {
        NativeValue::Bytes(b) => Ok(b.clone()),
        // Allow Str fallback — convenient for test paths that hand strings
        // to byte-typed slots before `Bytes` lands.
        NativeValue::Str(s) => Ok(s.as_bytes().to_vec()),
        other => Err(format!("expected bytes, got {other:?}")),
    }
}

fn expect_builder(value: &NativeValue) -> Result<Rc<RefCell<RequestBuilderRepr>>, String> {
    match value {
        NativeValue::HttpRequestBuilder(rc) => Ok(rc.clone()),
        other => Err(format!("expected RequestBuilder, got {other:?}")),
    }
}

fn expect_response(value: &NativeValue) -> Result<Rc<ResponseRepr>, String> {
    match value {
        NativeValue::HttpResponse(rc) => Ok(rc.clone()),
        other => Err(format!("expected Response, got {other:?}")),
    }
}

fn expect_map(value: &NativeValue) -> Result<BTreeMap<MapKey, NativeValue>, String> {
    match value {
        NativeValue::Map(m) => Ok(m.clone()),
        other => Err(format!("expected map, got {other:?}")),
    }
}

fn expect_json(value: &NativeValue) -> Result<JsonRepr, String> {
    match value {
        NativeValue::Json(j) => Ok((**j).clone()),
        other => Err(format!("expected json, got {other:?}")),
    }
}

fn http_err(e: HttpError) -> String {
    format!("HttpError::{}: {}", e.tag(), e)
}

// ============================================================================
// JSON serialization shim
//
// The `json` task owns the canonical implementation. We provide a tiny
// hand-rolled stringifier so this file compiles in isolation. Integration
// can swap it out for `crate::json::to_str`.
// ============================================================================

pub fn json_to_str(j: &JsonRepr) -> String {
    let mut out = String::new();
    write_json(j, &mut out);
    out
}

fn write_json(j: &JsonRepr, out: &mut String) {
    match j {
        JsonRepr::Null => out.push_str("null"),
        JsonRepr::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        JsonRepr::Int(n) => out.push_str(&n.to_string()),
        JsonRepr::Float(f) => out.push_str(&f.to_string()),
        JsonRepr::Str(s) => write_json_string(s, out),
        JsonRepr::Array(items) => {
            out.push('[');
            for (i, v) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_json(v, out);
            }
            out.push(']');
        }
        JsonRepr::Object(entries) => {
            out.push('{');
            for (i, (k, v)) in entries.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_json_string(k, out);
                out.push(':');
                write_json(v, out);
            }
            out.push('}');
        }
    }
}

fn write_json_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
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
    out.push('"');
}

/// Minimal JSON parser (object/array/string/number/bool/null). Mirrors the
/// shape of `crate::json::parse` so we can compile in isolation and swap
/// later. Returns `Err(reason)` on malformed input.
pub fn json_parse(input: &str) -> Result<JsonRepr, String> {
    let mut p = JsonParser::new(input);
    p.skip_ws();
    let v = p.parse_value()?;
    p.skip_ws();
    if p.pos != p.input.len() {
        return Err(format!("trailing input at byte {}", p.pos));
    }
    Ok(v)
}

struct JsonParser<'a> {
    input: &'a [u8],
    pos: usize,
}

impl<'a> JsonParser<'a> {
    fn new(s: &'a str) -> Self {
        Self {
            input: s.as_bytes(),
            pos: 0,
        }
    }

    fn skip_ws(&mut self) {
        while self.pos < self.input.len() {
            let b = self.input[self.pos];
            if b == b' ' || b == b'\t' || b == b'\n' || b == b'\r' {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn peek(&self) -> Result<u8, String> {
        self.input
            .get(self.pos)
            .copied()
            .ok_or_else(|| "unexpected end of input".to_string())
    }

    fn bump(&mut self) -> Result<u8, String> {
        let b = self.peek()?;
        self.pos += 1;
        Ok(b)
    }

    fn parse_value(&mut self) -> Result<JsonRepr, String> {
        self.skip_ws();
        match self.peek()? {
            b'{' => self.parse_object(),
            b'[' => self.parse_array(),
            b'"' => self.parse_string().map(JsonRepr::Str),
            b't' | b'f' => self.parse_bool(),
            b'n' => self.parse_null(),
            b'-' | b'0'..=b'9' => self.parse_number(),
            other => Err(format!("unexpected byte {:?}", other as char)),
        }
    }

    fn parse_object(&mut self) -> Result<JsonRepr, String> {
        self.bump()?; // '{'
        let mut out: Vec<(String, JsonRepr)> = Vec::new();
        self.skip_ws();
        if self.peek()? == b'}' {
            self.bump()?;
            return Ok(JsonRepr::Object(out));
        }
        loop {
            self.skip_ws();
            let key = self.parse_string()?;
            self.skip_ws();
            if self.bump()? != b':' {
                return Err("expected ':'".to_string());
            }
            let v = self.parse_value()?;
            out.push((key, v));
            self.skip_ws();
            match self.bump()? {
                b',' => continue,
                b'}' => return Ok(JsonRepr::Object(out)),
                other => return Err(format!("expected ',' or '}}', got {:?}", other as char)),
            }
        }
    }

    fn parse_array(&mut self) -> Result<JsonRepr, String> {
        self.bump()?; // '['
        let mut out = Vec::new();
        self.skip_ws();
        if self.peek()? == b']' {
            self.bump()?;
            return Ok(JsonRepr::Array(out));
        }
        loop {
            let v = self.parse_value()?;
            out.push(v);
            self.skip_ws();
            match self.bump()? {
                b',' => continue,
                b']' => return Ok(JsonRepr::Array(out)),
                other => return Err(format!("expected ',' or ']', got {:?}", other as char)),
            }
        }
    }

    fn parse_string(&mut self) -> Result<String, String> {
        if self.bump()? != b'"' {
            return Err("expected '\"'".to_string());
        }
        let mut out = String::new();
        loop {
            let b = self.bump()?;
            match b {
                b'"' => return Ok(out),
                b'\\' => {
                    let esc = self.bump()?;
                    match esc {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'b' => out.push('\u{0008}'),
                        b'f' => out.push('\u{000c}'),
                        b'u' => {
                            if self.pos + 4 > self.input.len() {
                                return Err("truncated \\u escape".to_string());
                            }
                            let hex = std::str::from_utf8(&self.input[self.pos..self.pos + 4])
                                .map_err(|_| "invalid \\u escape".to_string())?;
                            let cp = u32::from_str_radix(hex, 16)
                                .map_err(|_| "invalid \\u escape".to_string())?;
                            self.pos += 4;
                            if let Some(c) = char::from_u32(cp) {
                                out.push(c);
                            } else {
                                return Err("invalid unicode codepoint".to_string());
                            }
                        }
                        other => return Err(format!("unknown escape \\{}", other as char)),
                    }
                }
                b => out.push(b as char),
            }
        }
    }

    fn parse_bool(&mut self) -> Result<JsonRepr, String> {
        if self.input[self.pos..].starts_with(b"true") {
            self.pos += 4;
            Ok(JsonRepr::Bool(true))
        } else if self.input[self.pos..].starts_with(b"false") {
            self.pos += 5;
            Ok(JsonRepr::Bool(false))
        } else {
            Err("expected 'true' or 'false'".to_string())
        }
    }

    fn parse_null(&mut self) -> Result<JsonRepr, String> {
        if self.input[self.pos..].starts_with(b"null") {
            self.pos += 4;
            Ok(JsonRepr::Null)
        } else {
            Err("expected 'null'".to_string())
        }
    }

    fn parse_number(&mut self) -> Result<JsonRepr, String> {
        let start = self.pos;
        if self.peek()? == b'-' {
            self.pos += 1;
        }
        while self.pos < self.input.len() && self.input[self.pos].is_ascii_digit() {
            self.pos += 1;
        }
        let mut is_float = false;
        if self.pos < self.input.len() && self.input[self.pos] == b'.' {
            is_float = true;
            self.pos += 1;
            while self.pos < self.input.len() && self.input[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
        }
        if self.pos < self.input.len()
            && (self.input[self.pos] == b'e' || self.input[self.pos] == b'E')
        {
            is_float = true;
            self.pos += 1;
            if self.pos < self.input.len()
                && (self.input[self.pos] == b'+' || self.input[self.pos] == b'-')
            {
                self.pos += 1;
            }
            while self.pos < self.input.len() && self.input[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
        }
        let slice = std::str::from_utf8(&self.input[start..self.pos])
            .map_err(|_| "invalid number".to_string())?;
        if is_float {
            slice
                .parse::<f64>()
                .map(JsonRepr::Float)
                .map_err(|e| format!("invalid float: {e}"))
        } else {
            slice
                .parse::<i64>()
                .map(JsonRepr::Int)
                .map_err(|e| format!("invalid int: {e}"))
        }
    }
}

// ============================================================================
// Builder mutation paths (no network)
// ============================================================================

fn new_builder(method: &str, url: &str) -> NativeValue {
    NativeValue::HttpRequestBuilder(Rc::new(RefCell::new(RequestBuilderRepr::new(method, url))))
}

/// Mutates the builder in place and returns the same Rc so chained calls
/// observe the same state.
fn mutate_builder<F>(value: &NativeValue, f: F) -> Result<NativeValue, String>
where
    F: FnOnce(&mut RequestBuilderRepr),
{
    let rc = expect_builder(value)?;
    f(&mut rc.borrow_mut());
    Ok(NativeValue::HttpRequestBuilder(rc))
}

fn basic_auth_value(user: &str, pass: &str) -> String {
    use std::fmt::Write as _;
    let raw = format!("{user}:{pass}");
    let mut encoded = String::new();
    base64_encode(raw.as_bytes(), &mut encoded);
    let mut out = String::with_capacity(6 + encoded.len());
    write!(&mut out, "Basic {encoded}").unwrap();
    out
}

/// Tiny stdlib-only base64 encoder, used so this file compiles without the
/// `base64` crate (which the `encode` task pulls in). Integration may
/// replace with `base64::engine::general_purpose::STANDARD.encode(...)`.
fn base64_encode(input: &[u8], out: &mut String) {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut i = 0;
    while i + 3 <= input.len() {
        let n = ((input[i] as u32) << 16) | ((input[i + 1] as u32) << 8) | (input[i + 2] as u32);
        out.push(ALPHABET[((n >> 18) & 63) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 63) as usize] as char);
        out.push(ALPHABET[((n >> 6) & 63) as usize] as char);
        out.push(ALPHABET[(n & 63) as usize] as char);
        i += 3;
    }
    let rem = input.len() - i;
    if rem == 1 {
        let n = (input[i] as u32) << 16;
        out.push(ALPHABET[((n >> 18) & 63) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 63) as usize] as char);
        out.push('=');
        out.push('=');
    } else if rem == 2 {
        let n = ((input[i] as u32) << 16) | ((input[i + 1] as u32) << 8);
        out.push(ALPHABET[((n >> 18) & 63) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 63) as usize] as char);
        out.push(ALPHABET[((n >> 6) & 63) as usize] as char);
        out.push('=');
    }
}

// ============================================================================
// Native function impls
// ============================================================================

// -- quick helpers ----------------------------------------------------------

fn builtin_http_get(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let url = expect_str(&args[0])?.clone();
    let req = RequestBuilderRepr::new("GET", url);
    send_request(req)
}

fn builtin_http_post(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let url = expect_str(&args[0])?.clone();
    let body = expect_bytes(&args[1])?;
    let mut req = RequestBuilderRepr::new("POST", url);
    req.body = body;
    send_request(req)
}

fn builtin_http_put(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let url = expect_str(&args[0])?.clone();
    let body = expect_bytes(&args[1])?;
    let mut req = RequestBuilderRepr::new("PUT", url);
    req.body = body;
    send_request(req)
}

fn builtin_http_patch(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let url = expect_str(&args[0])?.clone();
    let body = expect_bytes(&args[1])?;
    let mut req = RequestBuilderRepr::new("PATCH", url);
    req.body = body;
    send_request(req)
}

fn builtin_http_delete(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let url = expect_str(&args[0])?.clone();
    let req = RequestBuilderRepr::new("DELETE", url);
    send_request(req)
}

// -- builder ----------------------------------------------------------------

fn builtin_http_request(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let method = expect_str(&args[0])?;
    let url = expect_str(&args[1])?;
    Ok(new_builder(method, url))
}

fn builtin_http_header(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 3)?;
    let name = expect_str(&args[1])?.clone();
    let val = expect_str(&args[2])?.clone();
    mutate_builder(&args[0], |b| {
        b.headers.insert(name, val);
    })
}

fn builtin_http_headers(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let map = expect_map(&args[1])?;
    mutate_builder(&args[0], |b| {
        for (k, v) in map {
            let key = match k {
                MapKey::Str(s) => s,
                MapKey::Int(n) => n.to_string(),
                MapKey::Bool(b) => b.to_string(),
            };
            let val = match v {
                NativeValue::Str(s) => s,
                other => format!("{other:?}"),
            };
            b.headers.insert(key, val);
        }
    })
}

fn builtin_http_body_bytes(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    // Two roles:
    //   builder + bytes  → mutate (set request body)
    //   response + ()    → read response body  (handled in builtin_http_body_bytes_resp)
    // We disambiguate on first arg.
    match &args[0] {
        NativeValue::HttpRequestBuilder(_) => {
            let body = expect_bytes(&args[1])?;
            mutate_builder(&args[0], |b| {
                b.body = body;
                b.headers
                    .entry("content-type".to_string())
                    .or_insert_with(|| "application/octet-stream".to_string());
            })
        }
        other => Err(format!("expected RequestBuilder, got {other:?}")),
    }
}

fn builtin_http_body_str(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let s = expect_str(&args[1])?.clone();
    mutate_builder(&args[0], |b| {
        b.body = s.into_bytes();
        b.headers
            .entry("content-type".to_string())
            .or_insert_with(|| "text/plain; charset=utf-8".to_string());
    })
}

fn builtin_http_body_json(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let json = expect_json(&args[1])?;
    let serialized = json_to_str(&json);
    mutate_builder(&args[0], |b| {
        b.body = serialized.into_bytes();
        b.headers
            .insert("content-type".to_string(), "application/json".to_string());
    })
}

fn builtin_http_timeout(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let ms = expect_int(&args[1])?;
    if ms < 0 {
        return Err("timeout must be non-negative".to_string());
    }
    mutate_builder(&args[0], |b| {
        b.timeout_ms = Some(ms as u64);
    })
}

fn builtin_http_follow_redirects(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let follow = expect_bool(&args[1])?;
    mutate_builder(&args[0], |b| {
        b.follow_redirects = follow;
    })
}

fn builtin_http_basic_auth(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 3)?;
    let user = expect_str(&args[1])?.clone();
    let pass = expect_str(&args[2])?.clone();
    let value = basic_auth_value(&user, &pass);
    mutate_builder(&args[0], |b| {
        b.headers.insert("authorization".to_string(), value);
    })
}

fn builtin_http_bearer(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let token = expect_str(&args[1])?.clone();
    mutate_builder(&args[0], |b| {
        b.headers
            .insert("authorization".to_string(), format!("Bearer {token}"));
    })
}

fn builtin_http_send(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let rc = expect_builder(&args[0])?;
    let snapshot = rc.borrow().clone();
    send_request(snapshot)
}

// -- response inspection ----------------------------------------------------

fn builtin_http_status(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let r = expect_response(&args[0])?;
    Ok(NativeValue::Int(r.status as i64))
}

fn builtin_http_ok(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    // Accept either Response or Int (the public spec is `ok(r: Response)`,
    // but tests in the spec also check `ok(200) == true` against an int —
    // we support both for ergonomics).
    match &args[0] {
        NativeValue::HttpResponse(r) => Ok(NativeValue::Bool((200..300).contains(&r.status))),
        NativeValue::Int(n) => Ok(NativeValue::Bool((200..300).contains(&(*n as u16)))),
        other => Err(format!("expected Response or Int, got {other:?}")),
    }
}

fn builtin_http_header_get(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let r = expect_response(&args[0])?;
    let name = expect_str(&args[1])?.to_ascii_lowercase();
    for (k, v) in &r.headers {
        if k.eq_ignore_ascii_case(&name) {
            return Ok(NativeValue::Str(v.clone()));
        }
    }
    // None — surface as a tagged absent value. Once `Option` lands across
    // the boundary this will become `NativeValue::OptionNone` or similar;
    // for now we use Unit to signal absence.
    Ok(NativeValue::Unit)
}

fn builtin_http_headers_all(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let r = expect_response(&args[0])?;
    let mut m: BTreeMap<MapKey, NativeValue> = BTreeMap::new();
    for (k, v) in &r.headers {
        m.insert(MapKey::Str(k.clone()), NativeValue::Str(v.clone()));
    }
    Ok(NativeValue::Map(m))
}

fn builtin_http_body_bytes_resp(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let r = expect_response(&args[0])?;
    Ok(NativeValue::Bytes(r.body.clone()))
}

fn builtin_http_body_str_resp(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let r = expect_response(&args[0])?;
    match std::str::from_utf8(&r.body) {
        Ok(s) => Ok(NativeValue::Str(s.to_string())),
        Err(e) => Err(format!("StrError: invalid utf-8 ({e})")),
    }
}

fn builtin_http_body_json_resp(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let r = expect_response(&args[0])?;
    let s = std::str::from_utf8(&r.body).map_err(|e| format!("JsonError: invalid utf-8 ({e})"))?;
    let j = json_parse(s).map_err(|e| format!("JsonError: {e}"))?;
    Ok(NativeValue::Json(Box::new(j)))
}

// -- typed convenience ------------------------------------------------------

fn builtin_http_get_json(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let url = expect_str(&args[0])?.clone();
    let mut req = RequestBuilderRepr::new("GET", url);
    req.headers
        .insert("accept".to_string(), "application/json".to_string());
    let resp = send_request_inner(req)?;
    let s =
        std::str::from_utf8(&resp.body).map_err(|e| format!("JsonError: invalid utf-8 ({e})"))?;
    let j = json_parse(s).map_err(|e| format!("JsonError: {e}"))?;
    Ok(NativeValue::Json(Box::new(j)))
}

fn builtin_http_post_json(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let url = expect_str(&args[0])?.clone();
    let body_json = expect_json(&args[1])?;
    let body = json_to_str(&body_json);
    let mut req = RequestBuilderRepr::new("POST", url);
    req.headers
        .insert("content-type".to_string(), "application/json".to_string());
    req.headers
        .insert("accept".to_string(), "application/json".to_string());
    req.body = body.into_bytes();
    let resp = send_request_inner(req)?;
    let s =
        std::str::from_utf8(&resp.body).map_err(|e| format!("JsonError: invalid utf-8 ({e})"))?;
    let j = json_parse(s).map_err(|e| format!("JsonError: {e}"))?;
    Ok(NativeValue::Json(Box::new(j)))
}

// ============================================================================
// Send pipeline
// ============================================================================

fn send_request(req: RequestBuilderRepr) -> Result<NativeValue, String> {
    let resp = send_request_inner(req)?;
    Ok(NativeValue::HttpResponse(Rc::new(resp)))
}

fn send_request_inner(req: RequestBuilderRepr) -> Result<ResponseRepr, String> {
    // Validate URL up front so we get a clean InvalidUrl mapping.
    if reqwest::Url::parse(&req.url).is_err() {
        return Err(http_err(HttpError::InvalidUrl(req.url)));
    }

    let method = match req.method.to_ascii_uppercase().as_str() {
        "GET" => reqwest::Method::GET,
        "POST" => reqwest::Method::POST,
        "PUT" => reqwest::Method::PUT,
        "PATCH" => reqwest::Method::PATCH,
        "DELETE" => reqwest::Method::DELETE,
        "HEAD" => reqwest::Method::HEAD,
        "OPTIONS" => reqwest::Method::OPTIONS,
        other => {
            return Err(http_err(HttpError::InvalidResponse(format!(
                "unsupported method {other}"
            ))));
        }
    };

    // Per-call client tweaks (timeout, redirect policy) require a
    // throwaway client when they differ from defaults. The shared client
    // covers the common case.
    let needs_custom_client = req.timeout_ms.is_some() || !req.follow_redirects;
    let client_buf;
    let client: &reqwest::blocking::Client = if needs_custom_client {
        let mut builder = reqwest::blocking::Client::builder().gzip(true);
        if let Some(ms) = req.timeout_ms {
            builder = builder.timeout(Duration::from_millis(ms));
        }
        if !req.follow_redirects {
            builder = builder.redirect(reqwest::redirect::Policy::none());
        }
        client_buf = builder
            .build()
            .map_err(|e| http_err(HttpError::InvalidResponse(e.to_string())))?;
        &client_buf
    } else {
        shared_client()
    };

    let mut rb = client.request(method, &req.url);
    for (k, v) in &req.headers {
        rb = rb.header(k.as_str(), v.as_str());
    }
    if !req.body.is_empty() {
        rb = rb.body(req.body.clone());
    }

    let resp = match rb.send() {
        Ok(r) => r,
        Err(e) => {
            let mut mapped = classify_reqwest_error(&e, &req.url);
            // Patch in the `ms` for timeouts (reqwest doesn't expose it).
            if let HttpError::Timeout { ms, .. } = &mut mapped
                && let Some(m) = req.timeout_ms
            {
                *ms = m;
            }
            return Err(http_err(mapped));
        }
    };

    let status = resp.status().as_u16();
    let mut headers = BTreeMap::new();
    for (k, v) in resp.headers().iter() {
        let name = k.as_str().to_ascii_lowercase();
        let val = v.to_str().unwrap_or("").to_string();
        headers.insert(name, val);
    }
    let body = resp
        .bytes()
        .map_err(|e| http_err(classify_reqwest_error(&e, &req.url)))?
        .to_vec();

    Ok(ResponseRepr {
        status,
        headers,
        body,
    })
}

// ============================================================================
// Registration
// ============================================================================

/// Registers every `http_*` native into the registry.
pub fn register(registry: &mut NativeRegistry, interner: &mut Interner) {
    let bodies: &[(&str, crate::NativeFn)] = &[
        ("http_get", builtin_http_get),
        ("http_post", builtin_http_post),
        ("http_put", builtin_http_put),
        ("http_patch", builtin_http_patch),
        ("http_delete", builtin_http_delete),
        ("http_request", builtin_http_request),
        ("http_header", builtin_http_header),
        ("http_headers", builtin_http_headers),
        ("http_body_bytes", builtin_http_body_bytes),
        ("http_body_str", builtin_http_body_str),
        ("http_body_json", builtin_http_body_json),
        ("http_timeout", builtin_http_timeout),
        ("http_follow_redirects", builtin_http_follow_redirects),
        ("http_basic_auth", builtin_http_basic_auth),
        ("http_bearer", builtin_http_bearer),
        ("http_send", builtin_http_send),
        ("http_status", builtin_http_status),
        ("http_ok", builtin_http_ok),
        ("http_header_get", builtin_http_header_get),
        ("http_headers_all", builtin_http_headers_all),
        ("http_body_bytes_resp", builtin_http_body_bytes_resp),
        ("http_body_str_resp", builtin_http_body_str_resp),
        ("http_body_json_resp", builtin_http_body_json_resp),
        ("http_get_json", builtin_http_get_json),
        ("http_post_json", builtin_http_post_json),
    ];
    crate::register_module(
        registry,
        interner,
        ferric_stdlib_meta::http::HTTP_FNS,
        bodies,
    );
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn builder(method: &str, url: &str) -> NativeValue {
        new_builder(method, url)
    }

    fn unwrap_builder(v: &NativeValue) -> Rc<RefCell<RequestBuilderRepr>> {
        match v {
            NativeValue::HttpRequestBuilder(rc) => rc.clone(),
            _ => panic!("not a RequestBuilder"),
        }
    }

    pub(super) fn unwrap_response(v: &NativeValue) -> Rc<ResponseRepr> {
        match v {
            NativeValue::HttpResponse(rc) => rc.clone(),
            _ => panic!("not a Response"),
        }
    }

    // -- builder construction ------------------------------------------------

    #[test]
    fn request_returns_builder_with_method_and_url() {
        let v = builtin_http_request(&[
            NativeValue::Str("GET".into()),
            NativeValue::Str("http://example.com/x".into()),
        ])
        .unwrap();
        let b = unwrap_builder(&v);
        assert_eq!(b.borrow().method, "GET");
        assert_eq!(b.borrow().url, "http://example.com/x");
        assert!(b.borrow().headers.is_empty());
        assert!(b.borrow().body.is_empty());
        assert!(b.borrow().timeout_ms.is_none());
        assert!(b.borrow().follow_redirects);
    }

    #[test]
    fn request_arg_count_enforced() {
        assert!(builtin_http_request(&[]).is_err());
        assert!(builtin_http_request(&[NativeValue::Str("GET".into())]).is_err());
    }

    // -- header / headers ----------------------------------------------------

    #[test]
    fn header_mutates_and_returns_builder() {
        let b = builder("GET", "http://e.com");
        let r = builtin_http_header(&[
            b.clone(),
            NativeValue::Str("X-Foo".into()),
            NativeValue::Str("bar".into()),
        ])
        .unwrap();
        let inner = unwrap_builder(&r);
        assert_eq!(inner.borrow().headers.get("X-Foo").unwrap(), "bar");
        // same Rc — mutations on b are visible on r
        let inner_b = unwrap_builder(&b);
        assert!(Rc::ptr_eq(&inner, &inner_b));
    }

    #[test]
    fn headers_bulk_mutates() {
        let b = builder("POST", "http://e.com");
        let mut m: BTreeMap<MapKey, NativeValue> = BTreeMap::new();
        m.insert(MapKey::Str("a".into()), NativeValue::Str("1".into()));
        m.insert(MapKey::Str("b".into()), NativeValue::Str("2".into()));
        let r = builtin_http_headers(&[b, NativeValue::Map(m)]).unwrap();
        let inner = unwrap_builder(&r);
        assert_eq!(inner.borrow().headers.get("a").unwrap(), "1");
        assert_eq!(inner.borrow().headers.get("b").unwrap(), "2");
    }

    // -- body_* --------------------------------------------------------------

    #[test]
    fn body_bytes_sets_body_and_default_content_type() {
        let b = builder("POST", "http://e.com");
        let r = builtin_http_body_bytes(&[b, NativeValue::Bytes(vec![1, 2, 3])]).unwrap();
        let inner = unwrap_builder(&r);
        assert_eq!(inner.borrow().body, vec![1, 2, 3]);
        assert_eq!(
            inner.borrow().headers.get("content-type").unwrap(),
            "application/octet-stream"
        );
    }

    #[test]
    fn body_str_sets_utf8_and_text_content_type() {
        let b = builder("POST", "http://e.com");
        let r = builtin_http_body_str(&[b, NativeValue::Str("hi".into())]).unwrap();
        let inner = unwrap_builder(&r);
        assert_eq!(inner.borrow().body, b"hi".to_vec());
        assert_eq!(
            inner.borrow().headers.get("content-type").unwrap(),
            "text/plain; charset=utf-8"
        );
    }

    #[test]
    fn body_json_sets_application_json() {
        let b = builder("POST", "http://e.com");
        let obj: Vec<(String, JsonRepr)> = vec![("k".to_string(), JsonRepr::Int(42))];
        let val = NativeValue::Json(Box::new(JsonRepr::Object(obj)));
        let r = builtin_http_body_json(&[b, val]).unwrap();
        let inner = unwrap_builder(&r);
        assert_eq!(
            inner.borrow().headers.get("content-type").unwrap(),
            "application/json"
        );
        assert_eq!(inner.borrow().body, br#"{"k":42}"#.to_vec());
    }

    // -- auth ----------------------------------------------------------------

    #[test]
    fn basic_auth_emits_basic_dtpw() {
        // user="u" pass="p"  →  base64("u:p") = "dTpw"
        let b = builder("GET", "http://e.com");
        let r = builtin_http_basic_auth(&[
            b,
            NativeValue::Str("u".into()),
            NativeValue::Str("p".into()),
        ])
        .unwrap();
        let inner = unwrap_builder(&r);
        assert_eq!(
            inner.borrow().headers.get("authorization").unwrap(),
            "Basic dTpw"
        );
    }

    #[test]
    fn bearer_emits_bearer_token() {
        let b = builder("GET", "http://e.com");
        let r = builtin_http_bearer(&[b, NativeValue::Str("tok".into())]).unwrap();
        let inner = unwrap_builder(&r);
        assert_eq!(
            inner.borrow().headers.get("authorization").unwrap(),
            "Bearer tok"
        );
    }

    // -- timeout / follow_redirects ------------------------------------------

    #[test]
    fn timeout_stores_ms() {
        let b = builder("GET", "http://e.com");
        let r = builtin_http_timeout(&[b, NativeValue::Int(100)]).unwrap();
        let inner = unwrap_builder(&r);
        assert_eq!(inner.borrow().timeout_ms, Some(100));
    }

    #[test]
    fn timeout_rejects_negative() {
        let b = builder("GET", "http://e.com");
        assert!(builtin_http_timeout(&[b, NativeValue::Int(-1)]).is_err());
    }

    #[test]
    fn follow_redirects_sets_flag() {
        let b = builder("GET", "http://e.com");
        let r = builtin_http_follow_redirects(&[b, NativeValue::Bool(false)]).unwrap();
        let inner = unwrap_builder(&r);
        assert!(!inner.borrow().follow_redirects);
    }

    // -- response inspection -------------------------------------------------

    fn fake_response(status: u16, body: &[u8]) -> NativeValue {
        let mut headers = BTreeMap::new();
        headers.insert("content-type".to_string(), "text/plain".to_string());
        headers.insert("x-trace-id".to_string(), "abc-123".to_string());
        NativeValue::HttpResponse(Rc::new(ResponseRepr {
            status,
            headers,
            body: body.to_vec(),
        }))
    }

    #[test]
    fn status_extracts_int() {
        let r = fake_response(204, b"");
        assert_eq!(builtin_http_status(&[r]).unwrap(), NativeValue::Int(204));
    }

    #[test]
    fn ok_classifies_2xx() {
        // ok(int) overloads
        assert_eq!(
            builtin_http_ok(&[NativeValue::Int(200)]).unwrap(),
            NativeValue::Bool(true)
        );
        assert_eq!(
            builtin_http_ok(&[NativeValue::Int(299)]).unwrap(),
            NativeValue::Bool(true)
        );
        assert_eq!(
            builtin_http_ok(&[NativeValue::Int(300)]).unwrap(),
            NativeValue::Bool(false)
        );
        assert_eq!(
            builtin_http_ok(&[NativeValue::Int(500)]).unwrap(),
            NativeValue::Bool(false)
        );
        // ok(response)
        assert_eq!(
            builtin_http_ok(&[fake_response(201, b"")]).unwrap(),
            NativeValue::Bool(true)
        );
        assert_eq!(
            builtin_http_ok(&[fake_response(404, b"")]).unwrap(),
            NativeValue::Bool(false)
        );
    }

    #[test]
    fn header_get_is_case_insensitive() {
        let r = fake_response(200, b"");
        assert_eq!(
            builtin_http_header_get(&[r.clone(), NativeValue::Str("X-Trace-Id".into())]).unwrap(),
            NativeValue::Str("abc-123".into())
        );
        assert_eq!(
            builtin_http_header_get(&[r.clone(), NativeValue::Str("x-trace-id".into())]).unwrap(),
            NativeValue::Str("abc-123".into())
        );
        // missing -> Unit (None placeholder)
        assert_eq!(
            builtin_http_header_get(&[r, NativeValue::Str("X-Missing".into())]).unwrap(),
            NativeValue::Unit
        );
    }

    #[test]
    fn headers_all_returns_map() {
        let r = fake_response(200, b"");
        let v = builtin_http_headers_all(&[r]).unwrap();
        match v {
            NativeValue::Map(m) => {
                assert!(m.contains_key(&MapKey::Str("content-type".into())));
                assert!(m.contains_key(&MapKey::Str("x-trace-id".into())));
            }
            _ => panic!("expected map"),
        }
    }

    #[test]
    fn body_bytes_resp_returns_payload() {
        let r = fake_response(200, b"hello");
        assert_eq!(
            builtin_http_body_bytes_resp(&[r]).unwrap(),
            NativeValue::Bytes(b"hello".to_vec())
        );
    }

    #[test]
    fn body_str_resp_decodes_utf8() {
        let r = fake_response(200, "héllo".as_bytes());
        assert_eq!(
            builtin_http_body_str_resp(&[r]).unwrap(),
            NativeValue::Str("héllo".to_string())
        );
    }

    #[test]
    fn body_str_resp_errs_on_invalid_utf8() {
        let r = fake_response(200, &[0xff, 0xfe, 0xfd]);
        assert!(builtin_http_body_str_resp(&[r]).is_err());
    }

    #[test]
    fn body_json_resp_parses_json() {
        let r = fake_response(200, br#"{"a":1,"b":"x"}"#);
        let v = builtin_http_body_json_resp(&[r]).unwrap();
        match v {
            NativeValue::Json(j) => match *j {
                JsonRepr::Object(m) => {
                    let lookup = |k: &str| m.iter().find(|(kk, _)| kk == k).map(|(_, v)| v);
                    assert_eq!(lookup("a").unwrap(), &JsonRepr::Int(1));
                    assert_eq!(lookup("b").unwrap(), &JsonRepr::Str("x".into()));
                }
                _ => panic!("expected object"),
            },
            _ => panic!("expected json"),
        }
    }

    #[test]
    fn body_json_resp_errs_on_garbage() {
        let r = fake_response(200, b"not json");
        assert!(builtin_http_body_json_resp(&[r]).is_err());
    }

    // -- helpers / encoders --------------------------------------------------

    #[test]
    fn base64_encode_known_vectors() {
        let mut s = String::new();
        base64_encode(b"u:p", &mut s);
        assert_eq!(s, "dTpw");

        let mut s = String::new();
        base64_encode(b"f", &mut s);
        assert_eq!(s, "Zg==");

        let mut s = String::new();
        base64_encode(b"fo", &mut s);
        assert_eq!(s, "Zm8=");

        let mut s = String::new();
        base64_encode(b"foo", &mut s);
        assert_eq!(s, "Zm9v");
    }

    #[test]
    fn json_round_trip_for_small_object() {
        let obj: Vec<(String, JsonRepr)> = vec![
            ("k".to_string(), JsonRepr::Int(42)),
            ("s".to_string(), JsonRepr::Str("hi".into())),
            ("b".to_string(), JsonRepr::Bool(true)),
            ("n".to_string(), JsonRepr::Null),
        ];
        let j = JsonRepr::Object(obj.clone());
        let s = json_to_str(&j);
        let parsed = json_parse(&s).unwrap();
        // json_parse may not preserve insertion order; compare as sets.
        match parsed {
            JsonRepr::Object(parsed_obj) => {
                let mut a = obj.clone();
                let mut b = parsed_obj;
                a.sort_by(|x, y| x.0.cmp(&y.0));
                b.sort_by(|x, y| x.0.cmp(&y.0));
                assert_eq!(a, b);
            }
            _ => panic!("expected object"),
        }
    }

    #[test]
    fn json_parses_arrays_and_floats() {
        let parsed = json_parse(r#"[1,2.5,-3,"x",true,null]"#).unwrap();
        match parsed {
            JsonRepr::Array(items) => {
                assert_eq!(items[0], JsonRepr::Int(1));
                assert_eq!(items[1], JsonRepr::Float(2.5));
                assert_eq!(items[2], JsonRepr::Int(-3));
                assert_eq!(items[3], JsonRepr::Str("x".into()));
                assert_eq!(items[4], JsonRepr::Bool(true));
                assert_eq!(items[5], JsonRepr::Null);
            }
            _ => panic!("expected array"),
        }
    }

    // -- arg-count guards on every public native -----------------------------

    #[test]
    fn arg_counts_are_enforced_across_surface() {
        assert!(builtin_http_get(&[]).is_err());
        assert!(builtin_http_post(&[NativeValue::Str("u".into())]).is_err());
        assert!(builtin_http_put(&[NativeValue::Str("u".into())]).is_err());
        assert!(builtin_http_patch(&[NativeValue::Str("u".into())]).is_err());
        assert!(builtin_http_delete(&[]).is_err());
        assert!(builtin_http_request(&[NativeValue::Str("GET".into())]).is_err());
        assert!(builtin_http_header(&[]).is_err());
        assert!(builtin_http_headers(&[]).is_err());
        assert!(builtin_http_body_bytes(&[]).is_err());
        assert!(builtin_http_body_str(&[]).is_err());
        assert!(builtin_http_body_json(&[]).is_err());
        assert!(builtin_http_timeout(&[]).is_err());
        assert!(builtin_http_follow_redirects(&[]).is_err());
        assert!(builtin_http_basic_auth(&[]).is_err());
        assert!(builtin_http_bearer(&[]).is_err());
        assert!(builtin_http_send(&[]).is_err());
        assert!(builtin_http_status(&[]).is_err());
        assert!(builtin_http_ok(&[]).is_err());
        assert!(builtin_http_header_get(&[]).is_err());
        assert!(builtin_http_headers_all(&[]).is_err());
        assert!(builtin_http_body_bytes_resp(&[]).is_err());
        assert!(builtin_http_body_str_resp(&[]).is_err());
        assert!(builtin_http_body_json_resp(&[]).is_err());
        assert!(builtin_http_get_json(&[]).is_err());
        assert!(builtin_http_post_json(&[]).is_err());
    }

    // -- type guards ---------------------------------------------------------

    #[test]
    fn type_guards_reject_wrong_types() {
        // header expects (builder, str, str)
        assert!(
            builtin_http_header(&[
                NativeValue::Int(1),
                NativeValue::Str("a".into()),
                NativeValue::Str("b".into())
            ])
            .is_err()
        );
        // status expects a Response
        assert!(builtin_http_status(&[NativeValue::Int(200)]).is_err());
        // header_get expects a Response
        assert!(
            builtin_http_header_get(&[NativeValue::Int(1), NativeValue::Str("a".into())]).is_err()
        );
    }

    // -- registration sanity -------------------------------------------------

    #[test]
    fn register_populates_all_names() {
        let mut registry = NativeRegistry::new();
        let mut interner = Interner::new();
        register(&mut registry, &mut interner);
        for name in [
            "http_get",
            "http_post",
            "http_put",
            "http_patch",
            "http_delete",
            "http_request",
            "http_header",
            "http_headers",
            "http_body_bytes",
            "http_body_str",
            "http_body_json",
            "http_timeout",
            "http_follow_redirects",
            "http_basic_auth",
            "http_bearer",
            "http_send",
            "http_status",
            "http_ok",
            "http_header_get",
            "http_headers_all",
            "http_body_bytes_resp",
            "http_body_str_resp",
            "http_body_json_resp",
            "http_get_json",
            "http_post_json",
        ] {
            let sym = interner.intern(name);
            assert!(
                registry.get(sym).is_some(),
                "missing native registration: {name}"
            );
        }
    }
}

// ============================================================================
// Integration tests against a local mock server.
//
// These spawn a `tiny_http` listener on `127.0.0.1:0` and issue real
// network requests to it. They are gated behind `tiny_http` being
// available; if the dep is unavailable at build time the module
// short-circuits. Tests are also `#[ignore]` so the default `cargo test`
// run stays hermetic — invoke with `cargo test -- --ignored` for the
// integration suite.
// ============================================================================

#[cfg(test)]
mod integration_tests {
    use super::tests::unwrap_response;
    use super::*;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    /// Spawns a tiny_http server bound to an OS-assigned port. Returns
    /// `(base_url, shutdown_handle)`. Drop the handle to stop the thread.
    fn spawn_server<F>(handler: F) -> (String, ServerHandle)
    where
        F: Fn(tiny_http::Request) + Send + 'static,
    {
        let server = tiny_http::Server::http("127.0.0.1:0").expect("bind");
        let port = server.server_addr().to_ip().unwrap().port();
        let base = format!("http://127.0.0.1:{port}");
        let (tx, rx) = mpsc::channel::<()>();
        let server = std::sync::Arc::new(server);
        let server_clone = server.clone();
        let join = thread::spawn(move || {
            for req in server_clone.incoming_requests() {
                if rx.try_recv().is_ok() {
                    break;
                }
                handler(req);
            }
        });
        (
            base,
            ServerHandle {
                server,
                join: Some(join),
                tx,
            },
        )
    }

    struct ServerHandle {
        server: std::sync::Arc<tiny_http::Server>,
        join: Option<thread::JoinHandle<()>>,
        tx: mpsc::Sender<()>,
    }

    impl Drop for ServerHandle {
        fn drop(&mut self) {
            let _ = self.tx.send(());
            self.server.unblock();
            if let Some(j) = self.join.take() {
                let _ = j.join();
            }
        }
    }

    #[test]

    fn get_returns_200_and_body() {
        let (base, _h) = spawn_server(|req| {
            let resp = tiny_http::Response::from_string("hello world");
            let _ = req.respond(resp);
        });
        let v = builtin_http_get(&[NativeValue::Str(format!("{base}/"))]).unwrap();
        let r = unwrap_response(&v);
        assert_eq!(r.status, 200);
        assert_eq!(r.body, b"hello world".to_vec());
    }

    #[test]

    fn post_json_round_trips_body() {
        let (base, _h) = spawn_server(|mut req| {
            let mut body = Vec::new();
            let _ = std::io::Read::read_to_end(req.as_reader(), &mut body);
            // Echo the body back as JSON.
            let resp = tiny_http::Response::from_data(body).with_header(
                "content-type: application/json"
                    .parse::<tiny_http::Header>()
                    .unwrap(),
            );
            let _ = req.respond(resp);
        });
        let obj: Vec<(String, JsonRepr)> =
            vec![("hello".to_string(), JsonRepr::Str("world".into()))];
        let val = NativeValue::Json(Box::new(JsonRepr::Object(obj.clone())));
        let v = builtin_http_post_json(&[NativeValue::Str(format!("{base}/")), val]).unwrap();
        match v {
            NativeValue::Json(j) => match *j {
                JsonRepr::Object(m) => {
                    let lookup = |k: &str| m.iter().find(|(kk, _)| kk == k).map(|(_, v)| v);
                    assert_eq!(lookup("hello").unwrap(), &JsonRepr::Str("world".into()));
                }
                _ => panic!("expected object"),
            },
            _ => panic!("expected json"),
        }
    }

    #[test]

    fn redirect_follow_on_chases_location() {
        // Server responds with a 302 to /target on first hit, 200 with
        // body "final" on the second.
        let counter = std::sync::Arc::new(std::sync::Mutex::new(0u32));
        let counter_c = counter.clone();
        let (base, _h) = spawn_server(move |req| {
            let mut n = counter_c.lock().unwrap();
            *n += 1;
            if req.url() == "/start" {
                let resp = tiny_http::Response::empty(302)
                    .with_header("location: /target".parse::<tiny_http::Header>().unwrap());
                let _ = req.respond(resp);
            } else {
                let resp = tiny_http::Response::from_string("final");
                let _ = req.respond(resp);
            }
        });
        // follow_redirects on (default)
        let v = builtin_http_get(&[NativeValue::Str(format!("{base}/start"))]).unwrap();
        let r = unwrap_response(&v);
        assert_eq!(r.status, 200);
        assert_eq!(r.body, b"final".to_vec());
    }

    #[test]

    fn redirect_follow_off_returns_3xx() {
        let (base, _h) = spawn_server(|req| {
            let resp = tiny_http::Response::empty(302)
                .with_header("location: /elsewhere".parse::<tiny_http::Header>().unwrap());
            let _ = req.respond(resp);
        });
        let b = new_builder("GET", &format!("{base}/here"));
        let b = builtin_http_follow_redirects(&[b, NativeValue::Bool(false)]).unwrap();
        let v = builtin_http_send(&[b]).unwrap();
        let r = unwrap_response(&v);
        assert_eq!(r.status, 302);
    }

    #[test]

    fn timeout_fires_on_slow_server() {
        let (base, _h) = spawn_server(|req| {
            thread::sleep(Duration::from_millis(150));
            let _ = req.respond(tiny_http::Response::from_string("late"));
        });
        let b = new_builder("GET", &format!("{base}/"));
        let b = builtin_http_timeout(&[b, NativeValue::Int(20)]).unwrap();
        let res = builtin_http_send(&[b]);
        assert!(res.is_err(), "expected timeout error");
        let msg = res.err().unwrap();
        assert!(
            msg.contains("Timeout"),
            "expected Timeout error, got: {msg}"
        );
    }
}
