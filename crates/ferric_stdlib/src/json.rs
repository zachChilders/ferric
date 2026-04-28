//! # `json` module — JSON serialisation, parsing, and access helpers.
//!
//! Implements `docs/stdlib.md` § "json — JSON" and the contract laid out in
//! `docs/tasks/stdlib-10-json-fmt.md`.
//!
//! Hand-rolled parser is used here (not serde_json) so the module is fully
//! self-contained and integrates cleanly with the `JsonRepr` value tree
//! without an intermediate `serde_json::Value`. The full set of features
//! required by the spec (line/col on syntax errors, ordered object keys,
//! `Int` vs `Float` parsing, very-large-integer overflow into Float) is
//! easier to deliver in a hand-written walker than via serde adaptors.
//!
//! REQUIRED DEPS:
//!   serde_json = "1"           // declared per task spec, but the implementation
//!                              // below is hand-rolled and does NOT depend on it.
//!                              // Integration may drop the dep, or swap the
//!                              // parser/serialiser for serde_json without
//!                              // changing the public function surface.
//!
//! REQUIRED NATIVE VALUE VARIANTS:
//!   Json(Box<JsonRepr>)
//!
//! where JsonRepr mirrors the Ferric Json enum:
//!   pub enum JsonRepr {
//!       Null,
//!       Bool(bool),
//!       Int(i64),
//!       Float(f64),
//!       Str(String),
//!       Array(Vec<JsonRepr>),
//!       Object(Vec<(String, JsonRepr)>),  // ordered for stable output
//!   }
//!
//! Object uses Vec<(K, V)> for stable iteration order; conversion to/from
//! Map<Str, Json> happens through `json::as_object`.

use crate::{NativeRegistry, NativeValue};
use ferric_common::Interner;

// ---------------------------------------------------------------------------
// JsonRepr
// ---------------------------------------------------------------------------

/// In-memory representation of a Ferric `Json` value.
#[derive(Debug, Clone, PartialEq)]
pub enum JsonRepr {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Array(Vec<JsonRepr>),
    /// Ordered key/value pairs so iteration / serialisation is stable.
    Object(Vec<(String, JsonRepr)>),
}

// ---------------------------------------------------------------------------
// Per-file helpers
// ---------------------------------------------------------------------------

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

fn expect_float(value: &NativeValue) -> Result<f64, String> {
    match value {
        NativeValue::Float(f) => Ok(*f),
        other => Err(format!("expected float, got {other:?}")),
    }
}

fn expect_bool(value: &NativeValue) -> Result<bool, String> {
    match value {
        NativeValue::Bool(b) => Ok(*b),
        other => Err(format!("expected bool, got {other:?}")),
    }
}

fn expect_array(value: &NativeValue) -> Result<&Vec<NativeValue>, String> {
    match value {
        NativeValue::Array(a) => Ok(a),
        other => Err(format!("expected array, got {other:?}")),
    }
}

fn expect_json(value: &NativeValue) -> Result<&JsonRepr, String> {
    match value {
        NativeValue::Json(j) => Ok(j.as_ref()),
        other => Err(format!("expected Json, got {other:?}")),
    }
}

fn json_value(j: JsonRepr) -> NativeValue {
    NativeValue::Json(Box::new(j))
}

/// Wrap an Option<JsonRepr> as a Ferric `Option<Json>` represented in
/// `NativeValue` form. The runtime convention used elsewhere in the stdlib
/// for `Option` is the existing variant constructor naming
/// (`Some(value)` / `None`); we mirror that with the `Array`-of-tag
/// representation that the integration layer converts. To keep tests local
/// and meaningful without depending on that layer, we model `Option<Json>`
/// in this module as either an `Array` of length 1 (Some) or length 0
/// (None) of `Json` values. Integration will adapt this to the real
/// `Option` representation if it differs.
fn some_json(j: JsonRepr) -> NativeValue {
    NativeValue::Array(vec![json_value(j)])
}

fn none_json() -> NativeValue {
    NativeValue::Array(vec![])
}

/// Wrap a primitive (Bool / Int / Float / Str) Option similarly.
fn some_value(v: NativeValue) -> NativeValue {
    NativeValue::Array(vec![v])
}

fn none_value() -> NativeValue {
    NativeValue::Array(vec![])
}

// ---------------------------------------------------------------------------
// Serialisation
// ---------------------------------------------------------------------------

fn escape_str(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\x08' => out.push_str("\\b"),
            '\x0c' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn write_compact(out: &mut String, v: &JsonRepr) {
    match v {
        JsonRepr::Null => out.push_str("null"),
        JsonRepr::Bool(true) => out.push_str("true"),
        JsonRepr::Bool(false) => out.push_str("false"),
        JsonRepr::Int(n) => out.push_str(&n.to_string()),
        JsonRepr::Float(f) => {
            if f.is_finite() {
                let mut s = format!("{f}");
                if !s.contains('.') && !s.contains('e') && !s.contains('E') {
                    s.push_str(".0");
                }
                out.push_str(&s);
            } else {
                // JSON has no representation for NaN/Inf; emit null.
                out.push_str("null");
            }
        }
        JsonRepr::Str(s) => escape_str(out, s),
        JsonRepr::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_compact(out, item);
            }
            out.push(']');
        }
        JsonRepr::Object(fields) => {
            out.push('{');
            for (i, (k, val)) in fields.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                escape_str(out, k);
                out.push(':');
                write_compact(out, val);
            }
            out.push('}');
        }
    }
}

fn write_pretty(out: &mut String, v: &JsonRepr, indent: usize) {
    fn pad(out: &mut String, n: usize) {
        for _ in 0..n {
            out.push_str("  ");
        }
    }
    match v {
        JsonRepr::Array(items) if items.is_empty() => out.push_str("[]"),
        JsonRepr::Object(fields) if fields.is_empty() => out.push_str("{}"),
        JsonRepr::Array(items) => {
            out.push_str("[\n");
            for (i, item) in items.iter().enumerate() {
                pad(out, indent + 1);
                write_pretty(out, item, indent + 1);
                if i + 1 < items.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            pad(out, indent);
            out.push(']');
        }
        JsonRepr::Object(fields) => {
            out.push_str("{\n");
            for (i, (k, val)) in fields.iter().enumerate() {
                pad(out, indent + 1);
                escape_str(out, k);
                out.push_str(": ");
                write_pretty(out, val, indent + 1);
                if i + 1 < fields.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            pad(out, indent);
            out.push('}');
        }
        // Scalars share the compact path.
        _ => write_compact(out, v),
    }
}

/// Serialise compactly.
pub fn to_str(v: &JsonRepr) -> String {
    let mut out = String::new();
    write_compact(&mut out, v);
    out
}

/// Serialise with 2-space indentation.
pub fn to_str_pretty(v: &JsonRepr) -> String {
    let mut out = String::new();
    write_pretty(&mut out, v, 0);
    out
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

struct Parser<'a> {
    src: &'a [u8],
    pos: usize,
    line: usize, // 1-based
    col: usize,  // 1-based
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParseErr {
    pub message: String,
    pub line: usize,
    pub col: usize,
}

impl<'a> Parser<'a> {
    fn new(src: &'a str) -> Self {
        Self {
            src: src.as_bytes(),
            pos: 0,
            line: 1,
            col: 1,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let b = self.peek()?;
        self.pos += 1;
        if b == b'\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(b)
    }

    fn err(&self, msg: impl Into<String>) -> ParseErr {
        ParseErr {
            message: msg.into(),
            line: self.line,
            col: self.col,
        }
    }

    fn skip_ws(&mut self) {
        while let Some(b) = self.peek() {
            if matches!(b, b' ' | b'\t' | b'\n' | b'\r') {
                self.bump();
            } else {
                break;
            }
        }
    }

    fn expect(&mut self, b: u8) -> Result<(), ParseErr> {
        match self.peek() {
            Some(c) if c == b => {
                self.bump();
                Ok(())
            }
            Some(c) => Err(self.err(format!(
                "expected '{}', found '{}'",
                b as char, c as char
            ))),
            None => Err(self.err(format!("expected '{}', found end of input", b as char))),
        }
    }

    fn parse_value(&mut self) -> Result<JsonRepr, ParseErr> {
        self.skip_ws();
        match self.peek() {
            None => Err(self.err("unexpected end of input")),
            Some(b'n') => self.parse_keyword("null", JsonRepr::Null),
            Some(b't') => self.parse_keyword("true", JsonRepr::Bool(true)),
            Some(b'f') => self.parse_keyword("false", JsonRepr::Bool(false)),
            Some(b'"') => self.parse_string().map(JsonRepr::Str),
            Some(b'[') => self.parse_array(),
            Some(b'{') => self.parse_object(),
            Some(b) if b == b'-' || b.is_ascii_digit() => self.parse_number(),
            Some(b) => Err(self.err(format!("unexpected character '{}'", b as char))),
        }
    }

    fn parse_keyword(&mut self, kw: &str, val: JsonRepr) -> Result<JsonRepr, ParseErr> {
        for byte in kw.bytes() {
            match self.peek() {
                Some(c) if c == byte => {
                    self.bump();
                }
                _ => return Err(self.err(format!("invalid literal, expected '{kw}'"))),
            }
        }
        Ok(val)
    }

    fn parse_string(&mut self) -> Result<String, ParseErr> {
        self.expect(b'"')?;
        let mut out = String::new();
        loop {
            match self.bump() {
                None => return Err(self.err("unterminated string")),
                Some(b'"') => return Ok(out),
                Some(b'\\') => match self.bump() {
                    Some(b'"') => out.push('"'),
                    Some(b'\\') => out.push('\\'),
                    Some(b'/') => out.push('/'),
                    Some(b'n') => out.push('\n'),
                    Some(b'r') => out.push('\r'),
                    Some(b't') => out.push('\t'),
                    Some(b'b') => out.push('\x08'),
                    Some(b'f') => out.push('\x0c'),
                    Some(b'u') => {
                        let cp = self.parse_hex4()?;
                        // Handle surrogate pair.
                        if (0xD800..=0xDBFF).contains(&cp) {
                            // High surrogate → expect \uXXXX low surrogate.
                            if self.bump() != Some(b'\\') || self.bump() != Some(b'u') {
                                return Err(self.err("expected low surrogate"));
                            }
                            let low = self.parse_hex4()?;
                            if !(0xDC00..=0xDFFF).contains(&low) {
                                return Err(self.err("invalid low surrogate"));
                            }
                            let combined =
                                0x10000 + (((cp - 0xD800) << 10) | (low - 0xDC00));
                            if let Some(c) = char::from_u32(combined) {
                                out.push(c);
                            } else {
                                return Err(self.err("invalid surrogate pair"));
                            }
                        } else if let Some(c) = char::from_u32(cp) {
                            out.push(c);
                        } else {
                            return Err(self.err("invalid unicode escape"));
                        }
                    }
                    Some(c) => {
                        return Err(self.err(format!("invalid escape '\\{}'", c as char)));
                    }
                    None => return Err(self.err("unterminated escape")),
                },
                Some(b) => {
                    if b < 0x20 {
                        return Err(self.err("control character in string"));
                    }
                    // We allow raw multi-byte UTF-8: push each byte and let the
                    // resulting String be UTF-8 because the source must be UTF-8.
                    out.push(b as char);
                }
            }
        }
    }

    fn parse_hex4(&mut self) -> Result<u32, ParseErr> {
        let mut acc: u32 = 0;
        for _ in 0..4 {
            let b = self
                .bump()
                .ok_or_else(|| self.err("unexpected end in unicode escape"))?;
            let d = match b {
                b'0'..=b'9' => (b - b'0') as u32,
                b'a'..=b'f' => 10 + (b - b'a') as u32,
                b'A'..=b'F' => 10 + (b - b'A') as u32,
                _ => return Err(self.err("invalid hex digit in unicode escape")),
            };
            acc = (acc << 4) | d;
        }
        Ok(acc)
    }

    fn parse_array(&mut self) -> Result<JsonRepr, ParseErr> {
        self.expect(b'[')?;
        self.skip_ws();
        let mut items = Vec::new();
        if self.peek() == Some(b']') {
            self.bump();
            return Ok(JsonRepr::Array(items));
        }
        loop {
            let v = self.parse_value()?;
            items.push(v);
            self.skip_ws();
            match self.peek() {
                Some(b',') => {
                    self.bump();
                    self.skip_ws();
                }
                Some(b']') => {
                    self.bump();
                    return Ok(JsonRepr::Array(items));
                }
                Some(c) => {
                    return Err(self.err(format!(
                        "expected ',' or ']' in array, found '{}'",
                        c as char
                    )));
                }
                None => return Err(self.err("unterminated array")),
            }
        }
    }

    fn parse_object(&mut self) -> Result<JsonRepr, ParseErr> {
        self.expect(b'{')?;
        self.skip_ws();
        let mut fields: Vec<(String, JsonRepr)> = Vec::new();
        if self.peek() == Some(b'}') {
            self.bump();
            return Ok(JsonRepr::Object(fields));
        }
        loop {
            self.skip_ws();
            if self.peek() != Some(b'"') {
                return Err(self.err("expected string key in object"));
            }
            let k = self.parse_string()?;
            self.skip_ws();
            self.expect(b':')?;
            self.skip_ws();
            let v = self.parse_value()?;
            fields.push((k, v));
            self.skip_ws();
            match self.peek() {
                Some(b',') => {
                    self.bump();
                }
                Some(b'}') => {
                    self.bump();
                    return Ok(JsonRepr::Object(fields));
                }
                Some(c) => {
                    return Err(self.err(format!(
                        "expected ',' or '}}' in object, found '{}'",
                        c as char
                    )));
                }
                None => return Err(self.err("unterminated object")),
            }
        }
    }

    fn parse_number(&mut self) -> Result<JsonRepr, ParseErr> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.bump();
        }
        // integer part
        match self.peek() {
            Some(b'0') => {
                self.bump();
            }
            Some(b'1'..=b'9') => {
                while let Some(b'0'..=b'9') = self.peek() {
                    self.bump();
                }
            }
            _ => return Err(self.err("invalid number")),
        }
        let mut is_float = false;
        if self.peek() == Some(b'.') {
            is_float = true;
            self.bump();
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(self.err("expected digit after '.'"));
            }
            while let Some(b'0'..=b'9') = self.peek() {
                self.bump();
            }
        }
        if matches!(self.peek(), Some(b'e') | Some(b'E')) {
            is_float = true;
            self.bump();
            if matches!(self.peek(), Some(b'+') | Some(b'-')) {
                self.bump();
            }
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(self.err("expected digit in exponent"));
            }
            while let Some(b'0'..=b'9') = self.peek() {
                self.bump();
            }
        }
        let raw = std::str::from_utf8(&self.src[start..self.pos])
            .map_err(|_| self.err("invalid utf-8 in number"))?;
        if is_float {
            raw.parse::<f64>()
                .map(JsonRepr::Float)
                .map_err(|_| self.err("invalid float"))
        } else {
            // Try i64 first; on overflow fall back to f64.
            match raw.parse::<i64>() {
                Ok(n) => Ok(JsonRepr::Int(n)),
                Err(_) => raw
                    .parse::<f64>()
                    .map(JsonRepr::Float)
                    .map_err(|_| self.err("invalid number")),
            }
        }
    }
}

/// Public parse entry point used by both the native function and tests.
pub fn parse(s: &str) -> Result<JsonRepr, ParseErr> {
    let mut p = Parser::new(s);
    p.skip_ws();
    let v = p.parse_value()?;
    p.skip_ws();
    if p.pos != p.src.len() {
        return Err(p.err("trailing data after JSON value"));
    }
    Ok(v)
}

// ---------------------------------------------------------------------------
// Native function bridges
// ---------------------------------------------------------------------------

fn builtin_json_to_str(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let j = expect_json(&args[0])?;
    Ok(NativeValue::Str(to_str(j)))
}

fn builtin_json_to_str_pretty(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let j = expect_json(&args[0])?;
    Ok(NativeValue::Str(to_str_pretty(j)))
}

fn builtin_json_parse(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let s = expect_str(&args[0])?;
    match parse(s) {
        Ok(j) => Ok(NativeValue::Array(vec![
            NativeValue::Str("Ok".to_string()),
            json_value(j),
        ])),
        Err(e) => Ok(NativeValue::Array(vec![
            NativeValue::Str("Err".to_string()),
            NativeValue::Str(format!(
                "JsonError::SyntaxError: {} (line {}, col {})",
                e.message, e.line, e.col
            )),
            NativeValue::Int(e.line as i64),
            NativeValue::Int(e.col as i64),
        ])),
    }
}

fn builtin_json_as_bool(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let j = expect_json(&args[0])?;
    Ok(match j {
        JsonRepr::Bool(b) => some_value(NativeValue::Bool(*b)),
        _ => none_value(),
    })
}

fn builtin_json_as_int(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let j = expect_json(&args[0])?;
    Ok(match j {
        JsonRepr::Int(n) => some_value(NativeValue::Int(*n)),
        _ => none_value(),
    })
}

fn builtin_json_as_float(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let j = expect_json(&args[0])?;
    Ok(match j {
        JsonRepr::Float(f) => some_value(NativeValue::Float(*f)),
        _ => none_value(),
    })
}

fn builtin_json_as_str(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let j = expect_json(&args[0])?;
    Ok(match j {
        JsonRepr::Str(s) => some_value(NativeValue::Str(s.clone())),
        _ => none_value(),
    })
}

fn builtin_json_as_array(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let j = expect_json(&args[0])?;
    Ok(match j {
        JsonRepr::Array(items) => {
            let arr: Vec<NativeValue> = items.iter().cloned().map(json_value).collect();
            some_value(NativeValue::Array(arr))
        }
        _ => none_value(),
    })
}

fn builtin_json_as_object(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let j = expect_json(&args[0])?;
    Ok(match j {
        JsonRepr::Object(fields) => {
            // Represent as an Array of [key, value] pairs (Array of length 2),
            // since the integration `Map`/`MapMut` variant is not yet defined
            // in `NativeValue`. Order is preserved.
            let entries: Vec<NativeValue> = fields
                .iter()
                .map(|(k, v)| {
                    NativeValue::Array(vec![NativeValue::Str(k.clone()), json_value(v.clone())])
                })
                .collect();
            some_value(NativeValue::Array(entries))
        }
        _ => none_value(),
    })
}

fn builtin_json_is_null(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let j = expect_json(&args[0])?;
    Ok(NativeValue::Bool(matches!(j, JsonRepr::Null)))
}

fn builtin_json_get(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let j = expect_json(&args[0])?;
    let key = expect_str(&args[1])?;
    Ok(match j {
        JsonRepr::Object(fields) => fields
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| some_json(v.clone()))
            .unwrap_or_else(none_json),
        _ => none_json(),
    })
}

fn builtin_json_get_path(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let mut cur = expect_json(&args[0])?.clone();
    let path = expect_array(&args[1])?;
    for seg in path {
        let key = expect_str(seg)?;
        match cur {
            JsonRepr::Object(fields) => match fields.iter().find(|(k, _)| k == key) {
                Some((_, v)) => cur = v.clone(),
                None => return Ok(none_json()),
            },
            _ => return Ok(none_json()),
        }
    }
    Ok(some_json(cur))
}

fn builtin_json_null(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 0)?;
    Ok(json_value(JsonRepr::Null))
}

fn builtin_json_bool(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let b = expect_bool(&args[0])?;
    Ok(json_value(JsonRepr::Bool(b)))
}

fn builtin_json_int(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let n = expect_int(&args[0])?;
    Ok(json_value(JsonRepr::Int(n)))
}

fn builtin_json_float(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let f = expect_float(&args[0])?;
    Ok(json_value(JsonRepr::Float(f)))
}

fn builtin_json_str(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let s = expect_str(&args[0])?;
    Ok(json_value(JsonRepr::Str(s.clone())))
}

fn builtin_json_array(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let arr = expect_array(&args[0])?;
    let mut out = Vec::with_capacity(arr.len());
    for v in arr {
        out.push(expect_json(v)?.clone());
    }
    Ok(json_value(JsonRepr::Array(out)))
}

fn builtin_json_object(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    // Argument is a List<(Str, Json)> — encoded as Array of length-2 Arrays.
    let pairs = expect_array(&args[0])?;
    let mut fields = Vec::with_capacity(pairs.len());
    for pair in pairs {
        let p = expect_array(pair)?;
        if p.len() != 2 {
            return Err(format!(
                "json::object: expected (key, value) pair, got tuple of length {}",
                p.len()
            ));
        }
        let k = expect_str(&p[0])?.clone();
        let v = expect_json(&p[1])?.clone();
        fields.push((k, v));
    }
    Ok(json_value(JsonRepr::Object(fields)))
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

/// Register every `json::*` native into the registry.
pub fn register(registry: &mut NativeRegistry, interner: &mut Interner) {
    registry.register(interner.intern("json_to_str"), builtin_json_to_str);
    registry.register(
        interner.intern("json_to_str_pretty"),
        builtin_json_to_str_pretty,
    );
    registry.register(interner.intern("json_parse"), builtin_json_parse);
    registry.register(interner.intern("json_as_bool"), builtin_json_as_bool);
    registry.register(interner.intern("json_as_int"), builtin_json_as_int);
    registry.register(interner.intern("json_as_float"), builtin_json_as_float);
    registry.register(interner.intern("json_as_str"), builtin_json_as_str);
    registry.register(interner.intern("json_as_array"), builtin_json_as_array);
    registry.register(interner.intern("json_as_object"), builtin_json_as_object);
    registry.register(interner.intern("json_is_null"), builtin_json_is_null);
    registry.register(interner.intern("json_get"), builtin_json_get);
    registry.register(interner.intern("json_get_path"), builtin_json_get_path);
    registry.register(interner.intern("json_null"), builtin_json_null);
    registry.register(interner.intern("json_bool"), builtin_json_bool);
    registry.register(interner.intern("json_int"), builtin_json_int);
    registry.register(interner.intern("json_float"), builtin_json_float);
    registry.register(interner.intern("json_str"), builtin_json_str);
    registry.register(interner.intern("json_array"), builtin_json_array);
    registry.register(interner.intern("json_object"), builtin_json_object);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- parse ---

    #[test]
    fn parse_null() {
        assert_eq!(parse("null").unwrap(), JsonRepr::Null);
    }

    #[test]
    fn parse_true() {
        assert_eq!(parse("true").unwrap(), JsonRepr::Bool(true));
    }

    #[test]
    fn parse_false() {
        assert_eq!(parse("false").unwrap(), JsonRepr::Bool(false));
    }

    #[test]
    fn parse_int() {
        assert_eq!(parse("42").unwrap(), JsonRepr::Int(42));
        assert_eq!(parse("-7").unwrap(), JsonRepr::Int(-7));
    }

    #[test]
    #[allow(clippy::approx_constant)]
    fn parse_float() {
        assert_eq!(parse("3.14").unwrap(), JsonRepr::Float(3.14));
        assert_eq!(parse("1e3").unwrap(), JsonRepr::Float(1000.0));
    }

    #[test]
    fn parse_huge_int_overflows_to_float() {
        // 2^70 is well outside i64 — must come back as Float.
        let v = parse("1180591620717411303424").unwrap();
        assert!(matches!(v, JsonRepr::Float(_)));
    }

    #[test]
    fn parse_string() {
        assert_eq!(parse("\"hi\"").unwrap(), JsonRepr::Str("hi".to_string()));
        assert_eq!(
            parse("\"a\\nb\"").unwrap(),
            JsonRepr::Str("a\nb".to_string())
        );
        assert_eq!(
            parse("\"\\u0041\"").unwrap(),
            JsonRepr::Str("A".to_string())
        );
    }

    #[test]
    fn parse_array() {
        let v = parse("[1,2,3]").unwrap();
        match v {
            JsonRepr::Array(items) => assert_eq!(items.len(), 3),
            _ => panic!("expected array"),
        }
    }

    #[test]
    fn parse_object() {
        let v = parse("{\"a\":1}").unwrap();
        match v {
            JsonRepr::Object(fields) => {
                assert_eq!(fields.len(), 1);
                assert_eq!(fields[0].0, "a");
                assert_eq!(fields[0].1, JsonRepr::Int(1));
            }
            _ => panic!("expected object"),
        }
    }

    #[test]
    fn parse_object_preserves_order() {
        let v = parse("{\"b\":1,\"a\":2,\"c\":3}").unwrap();
        match v {
            JsonRepr::Object(fields) => {
                let keys: Vec<&str> = fields.iter().map(|(k, _)| k.as_str()).collect();
                assert_eq!(keys, vec!["b", "a", "c"]);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn parse_syntax_error_has_position() {
        let err = parse("{").unwrap_err();
        assert!(err.line >= 1);
        assert!(err.col >= 1);
    }

    #[test]
    fn parse_trailing_data_errors() {
        assert!(parse("1 2").is_err());
    }

    // --- serialise ---

    #[test]
    fn to_str_int() {
        assert_eq!(to_str(&JsonRepr::Int(42)), "42");
    }

    #[test]
    fn to_str_null() {
        assert_eq!(to_str(&JsonRepr::Null), "null");
    }

    #[test]
    fn to_str_pretty_object() {
        let v = JsonRepr::Object(vec![
            ("a".to_string(), JsonRepr::Int(1)),
            (
                "b".to_string(),
                JsonRepr::Array(vec![JsonRepr::Int(1), JsonRepr::Int(2)]),
            ),
        ]);
        let s = to_str_pretty(&v);
        assert!(s.contains('\n'));
        assert!(s.contains("\"a\""));
        assert!(s.contains("\"b\""));
    }

    #[test]
    fn to_str_escapes_specials() {
        let v = JsonRepr::Str("a\"b\nc".to_string());
        assert_eq!(to_str(&v), "\"a\\\"b\\nc\"");
    }

    // --- roundtrip ---

    #[test]
    fn roundtrip_each_variant() {
        let cases = vec![
            JsonRepr::Null,
            JsonRepr::Bool(true),
            JsonRepr::Bool(false),
            JsonRepr::Int(0),
            JsonRepr::Int(-99),
            JsonRepr::Float(1.5),
            JsonRepr::Str("hello".to_string()),
            JsonRepr::Array(vec![JsonRepr::Int(1), JsonRepr::Bool(true)]),
            JsonRepr::Object(vec![("k".to_string(), JsonRepr::Int(7))]),
        ];
        for v in cases {
            let s = to_str(&v);
            let v2 = parse(&s).unwrap();
            assert_eq!(v, v2, "roundtrip failed for {s}");
        }
    }

    // --- accessors ---

    #[test]
    fn as_bool_only_on_bool() {
        assert_eq!(
            builtin_json_as_bool(&[json_value(JsonRepr::Bool(true))]).unwrap(),
            some_value(NativeValue::Bool(true))
        );
        assert_eq!(
            builtin_json_as_bool(&[json_value(JsonRepr::Int(1))]).unwrap(),
            none_value()
        );
    }

    #[test]
    fn as_int_does_not_coerce_float() {
        assert_eq!(
            builtin_json_as_int(&[json_value(JsonRepr::Float(2.0))]).unwrap(),
            none_value()
        );
        assert_eq!(
            builtin_json_as_int(&[json_value(JsonRepr::Int(2))]).unwrap(),
            some_value(NativeValue::Int(2))
        );
    }

    #[test]
    fn json_get_basic() {
        let obj = JsonRepr::Object(vec![("a".to_string(), JsonRepr::Int(1))]);
        let r = builtin_json_get(&[
            json_value(obj),
            NativeValue::Str("a".to_string()),
        ])
        .unwrap();
        assert_eq!(r, some_json(JsonRepr::Int(1)));
    }

    #[test]
    fn json_get_on_non_object_returns_none() {
        let r = builtin_json_get(&[
            json_value(JsonRepr::Null),
            NativeValue::Str("a".to_string()),
        ])
        .unwrap();
        assert_eq!(r, none_json());
    }

    #[test]
    fn json_get_path_walks_nested() {
        // {a:{b:{c:7}}}
        let inner = JsonRepr::Object(vec![("c".to_string(), JsonRepr::Int(7))]);
        let mid = JsonRepr::Object(vec![("b".to_string(), inner)]);
        let top = JsonRepr::Object(vec![("a".to_string(), mid)]);
        let path = NativeValue::Array(vec![
            NativeValue::Str("a".to_string()),
            NativeValue::Str("b".to_string()),
            NativeValue::Str("c".to_string()),
        ]);
        let r = builtin_json_get_path(&[json_value(top), path]).unwrap();
        assert_eq!(r, some_json(JsonRepr::Int(7)));
    }

    #[test]
    fn is_null_predicate() {
        assert_eq!(
            builtin_json_is_null(&[json_value(JsonRepr::Null)]).unwrap(),
            NativeValue::Bool(true)
        );
        assert_eq!(
            builtin_json_is_null(&[json_value(JsonRepr::Bool(false))]).unwrap(),
            NativeValue::Bool(false)
        );
    }

    // --- construction helpers ---

    #[test]
    fn ctor_helpers_produce_expected_variants() {
        assert_eq!(
            builtin_json_null(&[]).unwrap(),
            json_value(JsonRepr::Null)
        );
        assert_eq!(
            builtin_json_bool(&[NativeValue::Bool(true)]).unwrap(),
            json_value(JsonRepr::Bool(true))
        );
        assert_eq!(
            builtin_json_int(&[NativeValue::Int(5)]).unwrap(),
            json_value(JsonRepr::Int(5))
        );
        assert_eq!(
            builtin_json_float(&[NativeValue::Float(2.5)]).unwrap(),
            json_value(JsonRepr::Float(2.5))
        );
        assert_eq!(
            builtin_json_str(&[NativeValue::Str("x".into())]).unwrap(),
            json_value(JsonRepr::Str("x".into()))
        );
        let arr = builtin_json_array(&[NativeValue::Array(vec![
            json_value(JsonRepr::Int(1)),
            json_value(JsonRepr::Int(2)),
        ])])
        .unwrap();
        assert_eq!(
            arr,
            json_value(JsonRepr::Array(vec![JsonRepr::Int(1), JsonRepr::Int(2)]))
        );
        let obj = builtin_json_object(&[NativeValue::Array(vec![NativeValue::Array(vec![
            NativeValue::Str("k".into()),
            json_value(JsonRepr::Int(9)),
        ])])])
        .unwrap();
        assert_eq!(
            obj,
            json_value(JsonRepr::Object(vec![("k".into(), JsonRepr::Int(9))]))
        );
    }
}
