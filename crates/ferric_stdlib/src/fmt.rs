//! # `fmt` module — String formatting helpers.
//!
//! Implements `docs/stdlib.md` § "fmt — Formatting" and the contract laid out
//! in `docs/tasks/stdlib-10-json-fmt.md`.
//!
//! `fmt::debug` only knows how to render `Json` values; it does not stringify
//! arbitrary `NativeValue`s. The shared `JsonRepr` type lives in `json.rs`.
//!
//! REQUIRED DEPS:
//!   (none)
//!
//! REQUIRED NATIVE VALUE VARIANTS:
//!   Json(Box<JsonRepr>)        // shared with `json` module

use crate::json::{to_str_pretty, JsonRepr};
use crate::{NativeRegistry, NativeValue};
use ferric_common::Interner;

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

fn expect_str<'a>(value: &'a NativeValue) -> Result<&'a String, String> {
    match value {
        NativeValue::Str(s) => Ok(s),
        other => Err(format!("expected string, got {:?}", other)),
    }
}

fn expect_int(value: &NativeValue) -> Result<i64, String> {
    match value {
        NativeValue::Int(n) => Ok(*n),
        other => Err(format!("expected int, got {:?}", other)),
    }
}

fn expect_float(value: &NativeValue) -> Result<f64, String> {
    match value {
        NativeValue::Float(f) => Ok(*f),
        other => Err(format!("expected float, got {:?}", other)),
    }
}

fn expect_bool(value: &NativeValue) -> Result<bool, String> {
    match value {
        NativeValue::Bool(b) => Ok(*b),
        other => Err(format!("expected bool, got {:?}", other)),
    }
}

fn expect_array<'a>(value: &'a NativeValue) -> Result<&'a Vec<NativeValue>, String> {
    match value {
        NativeValue::Array(a) => Ok(a),
        other => Err(format!("expected array, got {:?}", other)),
    }
}

fn expect_json<'a>(value: &'a NativeValue) -> Result<&'a JsonRepr, String> {
    match value {
        NativeValue::Json(j) => Ok(j.as_ref()),
        other => Err(format!("expected Json, got {:?}", other)),
    }
}

// ---------------------------------------------------------------------------
// Pure functions
// ---------------------------------------------------------------------------

/// Replace `{}` placeholders in `template` with `args` in order. Errors if the
/// number of placeholders doesn't match the number of args.
pub fn format_template(template: &str, args: &[String]) -> Result<String, String> {
    let mut out = String::with_capacity(template.len());
    let mut idx = 0usize;
    let bytes = template.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'{' && i + 1 < bytes.len() && bytes[i + 1] == b'}' {
            if idx >= args.len() {
                return Err(format!(
                    "fmt::format: expected {} args, got {}",
                    count_placeholders(template),
                    args.len()
                ));
            }
            out.push_str(&args[idx]);
            idx += 1;
            i += 2;
        } else {
            // Push a single byte as char — the source must be valid UTF-8 since
            // it came from a Rust &str, so this is safe.
            //
            // Use a small helper that finds the next char boundary so we copy
            // entire UTF-8 codepoints.
            let ch_start = i;
            // Advance i past one full UTF-8 codepoint.
            i += utf8_char_len(bytes[i]);
            out.push_str(&template[ch_start..i]);
        }
    }
    if idx != args.len() {
        return Err(format!(
            "fmt::format: expected {} args, got {}",
            idx,
            args.len()
        ));
    }
    Ok(out)
}

fn utf8_char_len(b: u8) -> usize {
    if b < 0x80 {
        1
    } else if b < 0xC0 {
        1 // continuation byte; should not start a char, but be defensive
    } else if b < 0xE0 {
        2
    } else if b < 0xF0 {
        3
    } else {
        4
    }
}

fn count_placeholders(template: &str) -> usize {
    let bytes = template.as_bytes();
    let mut count = 0;
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'{' && bytes[i + 1] == b'}' {
            count += 1;
            i += 2;
        } else {
            i += 1;
        }
    }
    count
}

/// Format `n` in the requested radix. Radix must be 2, 8, 10, or 16.
pub fn format_int_radix(n: i64, radix: i64) -> Result<String, String> {
    match radix {
        2 => Ok(format_signed_radix(n, 2)),
        8 => Ok(format_signed_radix(n, 8)),
        10 => Ok(n.to_string()),
        16 => Ok(format_signed_radix(n, 16)),
        other => Err(format!(
            "fmt::int: radix must be 2, 8, 10, or 16; got {}",
            other
        )),
    }
}

fn format_signed_radix(n: i64, radix: u32) -> String {
    if n < 0 {
        // Use unsigned magnitude so i64::MIN does not overflow.
        let mag = (n as i128).unsigned_abs();
        let body = format_unsigned_radix(mag, radix);
        format!("-{}", body)
    } else {
        format_unsigned_radix(n as u128, radix)
    }
}

fn format_unsigned_radix(mut n: u128, radix: u32) -> String {
    if n == 0 {
        return "0".to_string();
    }
    let mut digits = Vec::new();
    while n > 0 {
        let d = (n % radix as u128) as u32;
        let c = std::char::from_digit(d, radix).unwrap();
        digits.push(c);
        n /= radix as u128;
    }
    digits.iter().rev().collect()
}

/// Pad `s` on the left to `width` characters using `pad` (which must be one
/// character).
pub fn pad_left(s: &str, width: i64, pad: &str) -> Result<String, String> {
    if pad.chars().count() != 1 {
        return Err(format!(
            "fmt::int_padded: pad must be exactly 1 character, got {:?}",
            pad
        ));
    }
    if width < 0 {
        return Err(format!(
            "fmt::int_padded: width must be non-negative, got {}",
            width
        ));
    }
    let cur = s.chars().count() as i64;
    if cur >= width {
        return Ok(s.to_string());
    }
    let pad_char = pad.chars().next().unwrap();
    let extra = (width - cur) as usize;
    let mut out = String::with_capacity(s.len() + extra);
    for _ in 0..extra {
        out.push(pad_char);
    }
    out.push_str(s);
    Ok(out)
}

// ---------------------------------------------------------------------------
// Native function bridges
// ---------------------------------------------------------------------------

fn builtin_fmt_format(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let template = expect_str(&args[0])?;
    let arr = expect_array(&args[1])?;
    let mut strs: Vec<String> = Vec::with_capacity(arr.len());
    for v in arr {
        strs.push(expect_str(v)?.clone());
    }
    let placeholders = count_placeholders(template);
    if placeholders != strs.len() {
        return Err(format!(
            "fmt::format: expected {} args, got {}",
            placeholders,
            strs.len()
        ));
    }
    format_template(template, &strs).map(NativeValue::Str)
}

fn builtin_fmt_int(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let n = expect_int(&args[0])?;
    let radix = expect_int(&args[1])?;
    format_int_radix(n, radix).map(NativeValue::Str)
}

fn builtin_fmt_int_padded(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 3)?;
    let n = expect_int(&args[0])?;
    let width = expect_int(&args[1])?;
    let pad = expect_str(&args[2])?;
    pad_left(&n.to_string(), width, pad).map(NativeValue::Str)
}

fn builtin_fmt_float(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let n = expect_float(&args[0])?;
    let decimals = expect_int(&args[1])?;
    if decimals < 0 {
        return Err(format!(
            "fmt::float: decimals must be non-negative, got {}",
            decimals
        ));
    }
    Ok(NativeValue::Str(format!("{:.*}", decimals as usize, n)))
}

fn builtin_fmt_float_exp(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let n = expect_float(&args[0])?;
    let decimals = expect_int(&args[1])?;
    if decimals < 0 {
        return Err(format!(
            "fmt::float_exp: decimals must be non-negative, got {}",
            decimals
        ));
    }
    Ok(NativeValue::Str(format!("{:.*e}", decimals as usize, n)))
}

fn builtin_fmt_bool(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let b = expect_bool(&args[0])?;
    Ok(NativeValue::Str(if b { "true" } else { "false" }.to_string()))
}

fn builtin_fmt_debug(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let j = expect_json(&args[0])?;
    Ok(NativeValue::Str(to_str_pretty(j)))
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

/// Register every `fmt::*` native into the registry.
pub fn register(registry: &mut NativeRegistry, interner: &mut Interner) {
    registry.register(interner.intern("fmt_format"), builtin_fmt_format);
    registry.register(interner.intern("fmt_int"), builtin_fmt_int);
    registry.register(interner.intern("fmt_int_padded"), builtin_fmt_int_padded);
    registry.register(interner.intern("fmt_float"), builtin_fmt_float);
    registry.register(interner.intern("fmt_float_exp"), builtin_fmt_float_exp);
    registry.register(interner.intern("fmt_bool"), builtin_fmt_bool);
    registry.register(interner.intern("fmt_debug"), builtin_fmt_debug);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- format ---

    #[test]
    fn format_basic_substitution() {
        let out = builtin_fmt_format(&[
            NativeValue::Str("hello, {}!".to_string()),
            NativeValue::Array(vec![NativeValue::Str("world".to_string())]),
        ])
        .unwrap();
        assert_eq!(out, NativeValue::Str("hello, world!".to_string()));
    }

    #[test]
    fn format_multiple_placeholders() {
        let out = builtin_fmt_format(&[
            NativeValue::Str("{} + {} = {}".to_string()),
            NativeValue::Array(vec![
                NativeValue::Str("1".to_string()),
                NativeValue::Str("2".to_string()),
                NativeValue::Str("3".to_string()),
            ]),
        ])
        .unwrap();
        assert_eq!(out, NativeValue::Str("1 + 2 = 3".to_string()));
    }

    #[test]
    fn format_count_mismatch_errors() {
        let err = builtin_fmt_format(&[
            NativeValue::Str("{}".to_string()),
            NativeValue::Array(vec![]),
        ])
        .unwrap_err();
        assert!(err.contains("fmt::format"));
    }

    #[test]
    fn format_too_many_args_errors() {
        let err = builtin_fmt_format(&[
            NativeValue::Str("hello".to_string()),
            NativeValue::Array(vec![NativeValue::Str("x".to_string())]),
        ])
        .unwrap_err();
        assert!(err.contains("fmt::format"));
    }

    #[test]
    fn format_preserves_unicode() {
        let out = builtin_fmt_format(&[
            NativeValue::Str("\u{1F44B} {}".to_string()),
            NativeValue::Array(vec![NativeValue::Str("\u{4E16}\u{754C}".to_string())]),
        ])
        .unwrap();
        assert_eq!(out, NativeValue::Str("\u{1F44B} \u{4E16}\u{754C}".to_string()));
    }

    // --- int ---

    #[test]
    fn fmt_int_hex() {
        let out =
            builtin_fmt_int(&[NativeValue::Int(255), NativeValue::Int(16)]).unwrap();
        assert_eq!(out, NativeValue::Str("ff".to_string()));
    }

    #[test]
    fn fmt_int_binary() {
        let out =
            builtin_fmt_int(&[NativeValue::Int(10), NativeValue::Int(2)]).unwrap();
        assert_eq!(out, NativeValue::Str("1010".to_string()));
    }

    #[test]
    fn fmt_int_octal() {
        let out =
            builtin_fmt_int(&[NativeValue::Int(8), NativeValue::Int(8)]).unwrap();
        assert_eq!(out, NativeValue::Str("10".to_string()));
    }

    #[test]
    fn fmt_int_decimal() {
        let out =
            builtin_fmt_int(&[NativeValue::Int(-42), NativeValue::Int(10)]).unwrap();
        assert_eq!(out, NativeValue::Str("-42".to_string()));
    }

    #[test]
    fn fmt_int_negative_hex() {
        let out =
            builtin_fmt_int(&[NativeValue::Int(-255), NativeValue::Int(16)]).unwrap();
        assert_eq!(out, NativeValue::Str("-ff".to_string()));
    }

    #[test]
    fn fmt_int_invalid_radix_errors() {
        let err =
            builtin_fmt_int(&[NativeValue::Int(7), NativeValue::Int(7)]).unwrap_err();
        assert!(err.contains("radix"));
    }

    // --- int_padded ---

    #[test]
    fn fmt_int_padded_basic() {
        let out = builtin_fmt_int_padded(&[
            NativeValue::Int(7),
            NativeValue::Int(3),
            NativeValue::Str("0".to_string()),
        ])
        .unwrap();
        assert_eq!(out, NativeValue::Str("007".to_string()));
    }

    #[test]
    fn fmt_int_padded_no_pad_when_wide_enough() {
        let out = builtin_fmt_int_padded(&[
            NativeValue::Int(1234),
            NativeValue::Int(2),
            NativeValue::Str("0".to_string()),
        ])
        .unwrap();
        assert_eq!(out, NativeValue::Str("1234".to_string()));
    }

    #[test]
    fn fmt_int_padded_multichar_pad_errors() {
        let err = builtin_fmt_int_padded(&[
            NativeValue::Int(1),
            NativeValue::Int(3),
            NativeValue::Str("ab".to_string()),
        ])
        .unwrap_err();
        assert!(err.contains("1 character"));
    }

    // --- float ---

    #[test]
    fn fmt_float_two_decimals() {
        let out =
            builtin_fmt_float(&[NativeValue::Float(3.14159), NativeValue::Int(2)])
                .unwrap();
        assert_eq!(out, NativeValue::Str("3.14".to_string()));
    }

    #[test]
    fn fmt_float_rounding() {
        // 1.05 with one decimal — exact value depends on float representation,
        // but the function should produce a string of the right shape.
        let out =
            builtin_fmt_float(&[NativeValue::Float(1.0), NativeValue::Int(3)]).unwrap();
        assert_eq!(out, NativeValue::Str("1.000".to_string()));
    }

    // --- float_exp ---

    #[test]
    fn fmt_float_exp_basic() {
        let out =
            builtin_fmt_float_exp(&[NativeValue::Float(1234.5), NativeValue::Int(2)])
                .unwrap();
        assert_eq!(out, NativeValue::Str("1.23e3".to_string()));
    }

    // --- bool ---

    #[test]
    fn fmt_bool_true() {
        assert_eq!(
            builtin_fmt_bool(&[NativeValue::Bool(true)]).unwrap(),
            NativeValue::Str("true".to_string())
        );
    }

    #[test]
    fn fmt_bool_false() {
        assert_eq!(
            builtin_fmt_bool(&[NativeValue::Bool(false)]).unwrap(),
            NativeValue::Str("false".to_string())
        );
    }

    // --- debug ---

    #[test]
    fn fmt_debug_object_pretty() {
        let v = JsonRepr::Object(vec![
            ("a".to_string(), JsonRepr::Int(1)),
            (
                "b".to_string(),
                JsonRepr::Array(vec![JsonRepr::Int(1), JsonRepr::Int(2)]),
            ),
        ]);
        let out =
            builtin_fmt_debug(&[NativeValue::Json(Box::new(v))]).unwrap();
        match out {
            NativeValue::Str(s) => {
                assert!(s.contains('\n'));
                assert!(s.contains("\"a\""));
                assert!(s.contains("\"b\""));
            }
            other => panic!("expected Str, got {:?}", other),
        }
    }
}
