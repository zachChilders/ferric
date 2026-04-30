//! # `encode` — Encoding / Decoding
//!
//! Implements the `encode` module from `docs/stdlib.md`:
//! Base64, hex, URL-encoding, HTML entities, and simple CSV.
//!
//! Functions are registered with the `encode_*` prefix (e.g. `encode_base64`).

// REQUIRED DEPS:
//   base64 = "0.22"
//   hex = "0.4"
//   urlencoding = "2"
//   (HTML escape and CSV are hand-rolled — small enough not to pull a crate.)

// REQUIRED NATIVE VALUE VARIANTS:
//   Bytes(Vec<u8>)        // shared with str-bytes task

use crate::{NativeRegistry, NativeValue};
use base64::Engine;
use ferric_common::Interner;

// ============================================================================
// Local Helpers (private to this module — see overview rule 5)
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

fn expect_bytes(value: &NativeValue) -> Result<&Vec<u8>, String> {
    match value {
        NativeValue::Bytes(b) => Ok(b),
        other => Err(format!("expected bytes, got {other:?}")),
    }
}

fn expect_list_of_str(value: &NativeValue) -> Result<Vec<&String>, String> {
    match value {
        NativeValue::Array(items) => items
            .iter()
            .map(|v| match v {
                NativeValue::Str(s) => Ok(s),
                other => Err(format!("expected list of strings, found {other:?}")),
            })
            .collect(),
        other => Err(format!("expected list of strings, got {other:?}")),
    }
}

// ============================================================================
// Base64
// ============================================================================

fn builtin_encode_base64(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let b = expect_bytes(&args[0])?;
    Ok(NativeValue::Str(
        base64::engine::general_purpose::STANDARD.encode(b),
    ))
}

fn builtin_encode_base64_url(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let b = expect_bytes(&args[0])?;
    Ok(NativeValue::Str(
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b),
    ))
}

fn builtin_encode_base64_decode(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let s = expect_str(&args[0])?;
    base64::engine::general_purpose::STANDARD
        .decode(s.as_bytes())
        .map(NativeValue::Bytes)
        .map_err(|_| "InvalidBase64".to_string())
}

fn builtin_encode_base64_url_decode(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let s = expect_str(&args[0])?;
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(s.as_bytes())
        .map(NativeValue::Bytes)
        .map_err(|_| "InvalidBase64".to_string())
}

// ============================================================================
// Hex
// ============================================================================

fn builtin_encode_hex(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let b = expect_bytes(&args[0])?;
    Ok(NativeValue::Str(hex::encode(b)))
}

fn builtin_encode_hex_upper(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let b = expect_bytes(&args[0])?;
    Ok(NativeValue::Str(hex::encode_upper(b)))
}

fn builtin_encode_hex_decode(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let s = expect_str(&args[0])?;
    hex::decode(s.as_bytes())
        .map(NativeValue::Bytes)
        .map_err(|_| "InvalidHex".to_string())
}

// ============================================================================
// URL encoding
// ============================================================================

/// Reserved-but-preserved characters for whole-URL `url_encode`.
/// Per the spec: `/?:#[]@!$&'()*+,;=` plus the unreserved set.
fn is_url_preserved(c: char) -> bool {
    // Unreserved per RFC 3986: A-Z a-z 0-9 - _ . ~
    if c.is_ascii_alphanumeric() {
        return true;
    }
    matches!(
        c,
        '-' | '_' | '.' | '~'
        // reserved that we leave alone in a full URL
        | '/' | '?' | ':' | '#' | '[' | ']' | '@'
        | '!' | '$' | '&' | '\'' | '(' | ')' | '*' | '+' | ',' | ';' | '='
    )
}

/// Percent-encode a single byte as `%XX`, uppercase hex.
fn percent_encode_byte(byte: u8, out: &mut String) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    out.push('%');
    out.push(HEX[(byte >> 4) as usize] as char);
    out.push(HEX[(byte & 0x0f) as usize] as char);
}

fn builtin_encode_url_encode(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let s = expect_str(&args[0])?;
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if is_url_preserved(c) {
            out.push(c);
        } else {
            // Encode this character's UTF-8 bytes.
            let mut buf = [0u8; 4];
            let encoded = c.encode_utf8(&mut buf);
            for &byte in encoded.as_bytes() {
                percent_encode_byte(byte, &mut out);
            }
        }
    }
    Ok(NativeValue::Str(out))
}

fn builtin_encode_url_encode_component(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let s = expect_str(&args[0])?;
    // urlencoding crate encodes everything except the unreserved set.
    Ok(NativeValue::Str(urlencoding::encode(s).into_owned()))
}

fn percent_decode(s: &str) -> Result<String, String> {
    // Hand-rolled: walk bytes, expand %XX into a byte buffer, validate UTF-8.
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'%' {
            if i + 2 >= bytes.len() {
                return Err("InvalidPercent".to_string());
            }
            let h1 = (bytes[i + 1] as char)
                .to_digit(16)
                .ok_or_else(|| "InvalidPercent".to_string())?;
            let h2 = (bytes[i + 2] as char)
                .to_digit(16)
                .ok_or_else(|| "InvalidPercent".to_string())?;
            out.push(((h1 << 4) | h2) as u8);
            i += 3;
        } else {
            out.push(b);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| "InvalidUtf8".to_string())
}

fn builtin_encode_url_decode(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let s = expect_str(&args[0])?;
    percent_decode(s).map(NativeValue::Str)
}

fn builtin_encode_url_decode_component(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let s = expect_str(&args[0])?;
    // Same algorithm — `%XX` is `%XX` regardless of whether it's a "component".
    percent_decode(s).map(NativeValue::Str)
}

// ============================================================================
// HTML entities — only the five standard entities per the task's hard constraint
// ============================================================================

fn builtin_encode_html_escape(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let s = expect_str(&args[0])?;
    // Order matters: `&` must run first so the other entities' `&` aren't
    // double-escaped.
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
    Ok(NativeValue::Str(out))
}

fn builtin_encode_html_unescape(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let s = expect_str(&args[0])?;
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'&' {
            // Try each of the five known entities. We compare against ASCII
            // byte slices since each entity is pure ASCII.
            if bytes[i..].starts_with(b"&amp;") {
                out.push('&');
                i += 5;
                continue;
            }
            if bytes[i..].starts_with(b"&lt;") {
                out.push('<');
                i += 4;
                continue;
            }
            if bytes[i..].starts_with(b"&gt;") {
                out.push('>');
                i += 4;
                continue;
            }
            if bytes[i..].starts_with(b"&quot;") {
                out.push('"');
                i += 6;
                continue;
            }
            if bytes[i..].starts_with(b"&#39;") {
                out.push('\'');
                i += 5;
                continue;
            }
            // Unknown entity — leave the `&` as-is (we do not implement the
            // full named-entity table).
            out.push('&');
            i += 1;
        } else {
            // Push the next UTF-8 codepoint. The string is valid UTF-8, so we
            // can find the next char boundary by re-decoding from this offset.
            // Cheaper: walk char_indices once. Switch strategy here.
            let rest = &s[i..];
            let c = rest.chars().next().expect("byte index is on char boundary");
            out.push(c);
            i += c.len_utf8();
        }
    }
    Ok(NativeValue::Str(out))
}

// ============================================================================
// CSV (single row, no multi-line support)
//
// Edge cases (documented):
//   - `csv_row(fields: [])` → `""`
//   - `csv_row(fields: [""])` → `""`        (one empty unquoted field)
//   - `csv_row(fields: ["a", ""])` → `"a,"`
//   - A field is quoted if it contains `,`, `"`, `\n`, or `\r`.
//   - Inside a quoted field, `"` is doubled to `""`.
//
//   - `csv_parse_row("")` → `[""]`           (one empty field — see test)
//   - `csv_parse_row(",")` → `["", ""]`
//   - Embedded `\n` / `\r` inside a quoted field is *not* supported here:
//     only single-line input. The parser accepts them as raw bytes inside
//     quotes, but the upper-level caller is expected to split on newlines
//     first.
//   - A stray `"` outside a quoted field is treated as a literal character.
// ============================================================================

fn csv_field_needs_quoting(field: &str) -> bool {
    field
        .chars()
        .any(|c| c == ',' || c == '"' || c == '\n' || c == '\r')
}

fn csv_quote_field(field: &str) -> String {
    let mut out = String::with_capacity(field.len() + 2);
    out.push('"');
    for c in field.chars() {
        if c == '"' {
            out.push('"');
            out.push('"');
        } else {
            out.push(c);
        }
    }
    out.push('"');
    out
}

fn builtin_encode_csv_row(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let fields = expect_list_of_str(&args[0])?;
    let parts: Vec<String> = fields
        .iter()
        .map(|f| {
            if csv_field_needs_quoting(f) {
                csv_quote_field(f)
            } else {
                (*f).clone()
            }
        })
        .collect();
    Ok(NativeValue::Str(parts.join(",")))
}

fn builtin_encode_csv_parse_row(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let s = expect_str(&args[0])?;

    let mut fields: Vec<NativeValue> = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;

    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if in_quotes {
            if c == '"' {
                // Doubled quote escape, or end of quoted field.
                if i + 1 < chars.len() && chars[i + 1] == '"' {
                    current.push('"');
                    i += 2;
                    continue;
                }
                // End of quoted field — note we consume the closing quote
                // and continue scanning. Subsequent characters (until the
                // next comma) are appended literally per typical CSV
                // tolerance of trailing junk.
                in_quotes = false;
                i += 1;
                continue;
            }
            current.push(c);
            i += 1;
        } else if c == ',' {
            fields.push(NativeValue::Str(std::mem::take(&mut current)));
            i += 1;
        } else if c == '"' && current.is_empty() {
            // Opening quote of a quoted field.
            in_quotes = true;
            i += 1;
        } else {
            // Stray quotes outside of an opening position are treated as
            // literal characters (lenient parsing).
            current.push(c);
            i += 1;
        }
    }
    fields.push(NativeValue::Str(current));

    Ok(NativeValue::Array(fields))
}

// ============================================================================
// Registration
// ============================================================================

/// Registers all `encode` module functions with the given native registry.
///
/// Native names use the `encode_*` prefix per overview rule 8:
/// `encode::base64` → `encode_base64`, etc.
pub fn register(registry: &mut NativeRegistry, interner: &mut Interner) {
    let bodies: &[(&str, crate::NativeFn)] = &[
        ("encode_base64", builtin_encode_base64),
        ("encode_base64_url", builtin_encode_base64_url),
        ("encode_base64_decode", builtin_encode_base64_decode),
        ("encode_base64_url_decode", builtin_encode_base64_url_decode),
        ("encode_hex", builtin_encode_hex),
        ("encode_hex_upper", builtin_encode_hex_upper),
        ("encode_hex_decode", builtin_encode_hex_decode),
        ("encode_url_encode", builtin_encode_url_encode),
        ("encode_url_decode", builtin_encode_url_decode),
        ("encode_url_encode_component", builtin_encode_url_encode_component),
        ("encode_url_decode_component", builtin_encode_url_decode_component),
        ("encode_html_escape", builtin_encode_html_escape),
        ("encode_html_unescape", builtin_encode_html_unescape),
        ("encode_csv_row", builtin_encode_csv_row),
        ("encode_csv_parse_row", builtin_encode_csv_parse_row),
    ];
    crate::register_module(
        registry,
        interner,
        ferric_stdlib_meta::encode::ENCODE_FNS,
        bodies,
    );
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &str) -> NativeValue {
        NativeValue::Str(v.to_string())
    }

    fn b(v: &[u8]) -> NativeValue {
        NativeValue::Bytes(v.to_vec())
    }

    fn unwrap_str(v: NativeValue) -> String {
        match v {
            NativeValue::Str(s) => s,
            other => panic!("expected Str, got {other:?}"),
        }
    }

    fn unwrap_bytes(v: NativeValue) -> Vec<u8> {
        match v {
            NativeValue::Bytes(b) => b,
            other => panic!("expected Bytes, got {other:?}"),
        }
    }

    fn unwrap_array(v: NativeValue) -> Vec<NativeValue> {
        match v {
            NativeValue::Array(a) => a,
            other => panic!("expected Array, got {other:?}"),
        }
    }

    // ---- Base64 ----------------------------------------------------------

    #[test]
    fn test_base64_roundtrip_hello() {
        let encoded = unwrap_str(builtin_encode_base64(&[b(b"Hello")]).unwrap());
        assert_eq!(encoded, "SGVsbG8=");
        let decoded = unwrap_bytes(builtin_encode_base64_decode(&[s(&encoded)]).unwrap());
        assert_eq!(decoded, b"Hello");
    }

    #[test]
    fn test_base64_empty() {
        let encoded = unwrap_str(builtin_encode_base64(&[b(b"")]).unwrap());
        assert_eq!(encoded, "");
        let decoded = unwrap_bytes(builtin_encode_base64_decode(&[s("")]).unwrap());
        assert_eq!(decoded, b"");
    }

    #[test]
    fn test_base64_random_roundtrip() {
        let bytes: Vec<u8> = (0u8..=255).collect();
        let encoded = unwrap_str(builtin_encode_base64(&[b(&bytes)]).unwrap());
        let decoded = unwrap_bytes(builtin_encode_base64_decode(&[s(&encoded)]).unwrap());
        assert_eq!(decoded, bytes);
    }

    #[test]
    fn test_base64_url_no_plus_or_slash() {
        // 0xFF 0xFF 0xFF in standard base64 is "////"; URL-safe must avoid /.
        let encoded = unwrap_str(builtin_encode_base64_url(&[b(&[0xFF, 0xFF, 0xFF])]).unwrap());
        assert!(!encoded.contains('+'));
        assert!(!encoded.contains('/'));
    }

    #[test]
    fn test_base64_url_roundtrip() {
        let data = b"\xff\xfe\xfd?>";
        let encoded = unwrap_str(builtin_encode_base64_url(&[b(data)]).unwrap());
        let decoded = unwrap_bytes(builtin_encode_base64_url_decode(&[s(&encoded)]).unwrap());
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_base64_decode_invalid() {
        // Mismatched alphabet (URL-safe `-` not valid in standard).
        let result = builtin_encode_base64_decode(&[s("aGVsbG8-")]);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "InvalidBase64");
    }

    // ---- Hex -------------------------------------------------------------

    #[test]
    fn test_hex_roundtrip() {
        let data = b"\x00\x01\x02\xff";
        let encoded = unwrap_str(builtin_encode_hex(&[b(data)]).unwrap());
        assert_eq!(encoded, "000102ff");
        let decoded = unwrap_bytes(builtin_encode_hex_decode(&[s(&encoded)]).unwrap());
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_hex_upper() {
        let encoded = unwrap_str(builtin_encode_hex_upper(&[b(b"\x01\x0f")]).unwrap());
        assert_eq!(encoded, "010F");
    }

    #[test]
    fn test_hex_decode_mixed_case() {
        // `hex_decode` accepts both cases.
        let decoded = unwrap_bytes(builtin_encode_hex_decode(&[s("aB1F")]).unwrap());
        assert_eq!(decoded, vec![0xab, 0x1f]);
    }

    #[test]
    fn test_hex_decode_invalid_chars() {
        let result = builtin_encode_hex_decode(&[s("ZZ")]);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "InvalidHex");
    }

    #[test]
    fn test_hex_decode_odd_length() {
        let result = builtin_encode_hex_decode(&[s("a")]);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "InvalidHex");
    }

    // ---- URL encode ------------------------------------------------------

    #[test]
    fn test_url_encode_component_basic() {
        let encoded = unwrap_str(builtin_encode_url_encode_component(&[s("a b/c?")]).unwrap());
        assert_eq!(encoded, "a%20b%2Fc%3F");
    }

    #[test]
    fn test_url_encode_full_preserves_reserved() {
        // Whole-URL encoding leaves reserved chars alone.
        let encoded = unwrap_str(builtin_encode_url_encode(&[s("https://x/y?a=b&c=d")]).unwrap());
        assert_eq!(encoded, "https://x/y?a=b&c=d");
    }

    #[test]
    fn test_url_encode_full_encodes_space() {
        let encoded = unwrap_str(builtin_encode_url_encode(&[s("hello world")]).unwrap());
        assert_eq!(encoded, "hello%20world");
    }

    #[test]
    fn test_url_decode_component_roundtrip() {
        let original = "name=François & Co.";
        let encoded = unwrap_str(builtin_encode_url_encode_component(&[s(original)]).unwrap());
        let decoded = unwrap_str(builtin_encode_url_decode_component(&[s(&encoded)]).unwrap());
        assert_eq!(decoded, original);
    }

    #[test]
    fn test_url_decode_invalid_percent() {
        let result = builtin_encode_url_decode(&[s("%ZZ")]);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "InvalidPercent");
    }

    #[test]
    fn test_url_decode_truncated_percent() {
        let result = builtin_encode_url_decode(&[s("abc%2")]);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "InvalidPercent");
    }

    // ---- HTML ------------------------------------------------------------

    #[test]
    fn test_html_escape_attribute() {
        let escaped = unwrap_str(builtin_encode_html_escape(&[s("<a href=\"x\">")]).unwrap());
        assert_eq!(escaped, "&lt;a href=&quot;x&quot;&gt;");
    }

    #[test]
    fn test_html_escape_ampersand_first() {
        // Make sure `&` is escaped before others — `&lt;` should not become `&amp;lt;`.
        let escaped = unwrap_str(builtin_encode_html_escape(&[s("a & b < c")]).unwrap());
        assert_eq!(escaped, "a &amp; b &lt; c");
    }

    #[test]
    fn test_html_escape_apostrophe() {
        let escaped = unwrap_str(builtin_encode_html_escape(&[s("it's")]).unwrap());
        assert_eq!(escaped, "it&#39;s");
    }

    #[test]
    fn test_html_roundtrip() {
        let original = "<a href=\"https://x?q=1&r=2\">it's</a>";
        let escaped = unwrap_str(builtin_encode_html_escape(&[s(original)]).unwrap());
        let unescaped = unwrap_str(builtin_encode_html_unescape(&[s(&escaped)]).unwrap());
        assert_eq!(unescaped, original);
    }

    #[test]
    fn test_html_unescape_unknown_entity_passthrough() {
        // We only handle the five standard entities — `&copy;` is left as-is.
        let unescaped = unwrap_str(builtin_encode_html_unescape(&[s("&copy; 2026")]).unwrap());
        assert_eq!(unescaped, "&copy; 2026");
    }

    // ---- CSV -------------------------------------------------------------

    #[test]
    fn test_csv_row_simple() {
        let row = unwrap_str(
            builtin_encode_csv_row(&[NativeValue::Array(vec![s("a"), s("b"), s("c")])]).unwrap(),
        );
        assert_eq!(row, "a,b,c");
    }

    #[test]
    fn test_csv_row_quoting() {
        let row = unwrap_str(
            builtin_encode_csv_row(&[NativeValue::Array(vec![s("a"), s("b,c"), s("d\"e")])])
                .unwrap(),
        );
        assert_eq!(row, "a,\"b,c\",\"d\"\"e\"");
    }

    #[test]
    fn test_csv_parse_row_simple() {
        let parsed = unwrap_array(builtin_encode_csv_parse_row(&[s("a,b,c")]).unwrap());
        assert_eq!(parsed, vec![s("a"), s("b"), s("c")]);
    }

    #[test]
    fn test_csv_parse_row_quoted() {
        let parsed =
            unwrap_array(builtin_encode_csv_parse_row(&[s("a,\"b,c\",\"d\"\"e\"")]).unwrap());
        assert_eq!(parsed, vec![s("a"), s("b,c"), s("d\"e")]);
    }

    #[test]
    fn test_csv_roundtrip() {
        let fields = vec![s("a"), s("b,c"), s("d\"e"), s("plain")];
        let row =
            unwrap_str(builtin_encode_csv_row(&[NativeValue::Array(fields.clone())]).unwrap());
        let parsed = unwrap_array(builtin_encode_csv_parse_row(&[s(&row)]).unwrap());
        assert_eq!(parsed, fields);
    }

    #[test]
    fn test_csv_parse_row_empty() {
        // Documented choice: empty input yields one empty field.
        let parsed = unwrap_array(builtin_encode_csv_parse_row(&[s("")]).unwrap());
        assert_eq!(parsed, vec![s("")]);
    }

    #[test]
    fn test_csv_parse_row_trailing_comma() {
        // ",b" yields ["", "b"]; "a," yields ["a", ""].
        let parsed = unwrap_array(builtin_encode_csv_parse_row(&[s("a,")]).unwrap());
        assert_eq!(parsed, vec![s("a"), s("")]);
    }
}
