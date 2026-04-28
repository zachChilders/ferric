//! `bytes` — Raw byte sequences.
//!
//! See `docs/stdlib.md` § "`bytes` — Raw Byte Sequences" for the spec.
//!
//! `Bytes` is an immutable byte slice. `BytesMut` is a mutable builder backed
//! by `Rc<RefCell<Vec<u8>>>` so subsequent `write_*` calls mutate in place.

// REQUIRED NATIVE VALUE VARIANTS:
//   Bytes(Vec<u8>)
//   BytesMut(Rc<RefCell<Vec<u8>>>)

// REQUIRED DEPS:
//   none — base64 and hex are needed by the `encode` module; route bytes::to_hex
//   etc. through the same crates the `encode` task selects (likely `base64` and
//   `hex`). For now use std + manual hex implementation.

use std::cell::RefCell;
use std::rc::Rc;

use crate::{NativeRegistry, NativeValue};
use ferric_common::Interner;

// ---------------------------------------------------------------------------
// Private helpers (per-file, per the parallel-build convention)
// ---------------------------------------------------------------------------

fn check_arg_count(args: &[NativeValue], expected: usize) -> Result<(), String> {
    if args.len() != expected {
        Err(format!("expected {} argument(s), got {}", expected, args.len()))
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

fn expect_bytes(value: &NativeValue) -> Result<Vec<u8>, String> {
    match value {
        NativeValue::Bytes(b) => Ok(b.clone()),
        NativeValue::BytesMut(rc) => Ok(rc.borrow().clone()),
        other => Err(format!("expected bytes, got {other:?}")),
    }
}

fn expect_bytes_mut(value: &NativeValue) -> Result<Rc<RefCell<Vec<u8>>>, String> {
    match value {
        NativeValue::BytesMut(rc) => Ok(rc.clone()),
        other => Err(format!("expected BytesMut, got {other:?}")),
    }
}

fn expect_array(value: &NativeValue) -> Result<&Vec<NativeValue>, String> {
    match value {
        NativeValue::Array(v) => Ok(v),
        other => Err(format!("expected list, got {other:?}")),
    }
}

// Native ABI is `Result<NativeValue, String>`. Language-level Result/Option
// are encoded as a tagged array — same convention as `str_.rs`.
fn lang_ok(v: NativeValue) -> NativeValue {
    NativeValue::Array(vec![NativeValue::Str("Ok".to_string()), v])
}

fn lang_err(msg: String) -> NativeValue {
    NativeValue::Array(vec![NativeValue::Str("Err".to_string()), NativeValue::Str(msg)])
}

fn lang_some(v: NativeValue) -> NativeValue {
    NativeValue::Array(vec![NativeValue::Str("Some".to_string()), v])
}

fn lang_none() -> NativeValue {
    NativeValue::Array(vec![NativeValue::Str("None".to_string())])
}

fn bytes_err_out_of_bounds(idx: i64, len: usize) -> String {
    format!("BytesError::OutOfBounds {{ index: {idx}, len: {len} }}")
}

fn bytes_err_invalid_hex() -> String {
    "BytesError::InvalidHex".to_string()
}

fn bytes_err_invalid_base64() -> String {
    "BytesError::InvalidBase64".to_string()
}

fn bytes_err_invalid_byte(value: i64) -> String {
    format!("BytesError::InvalidByte {{ value: {value} }}")
}

fn bytes_err_invalid_utf8() -> String {
    "BytesError::InvalidUtf8".to_string()
}

// ---------------------------------------------------------------------------
// Hex (manual implementation; lowercase output, case-insensitive input,
// rejects odd-length input).
// ---------------------------------------------------------------------------

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if !s.is_ascii() {
        return None;
    }
    let bytes = s.as_bytes();
    if bytes.len() % 2 != 0 {
        return None;
    }
    fn from_hex(c: u8) -> Option<u8> {
        match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            b'A'..=b'F' => Some(c - b'A' + 10),
            _ => None,
        }
    }
    let mut out = Vec::with_capacity(bytes.len() / 2);
    let mut i = 0;
    while i < bytes.len() {
        let hi = from_hex(bytes[i])?;
        let lo = from_hex(bytes[i + 1])?;
        out.push((hi << 4) | lo);
        i += 2;
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// Base64 (RFC 4648 standard alphabet, with padding). Manual implementation —
// the integration step may swap to the `base64` crate.
// ---------------------------------------------------------------------------

fn b64_encode(bytes: &[u8]) -> String {
    const ALPHA: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    let mut i = 0;
    while i + 3 <= bytes.len() {
        let b0 = bytes[i] as u32;
        let b1 = bytes[i + 1] as u32;
        let b2 = bytes[i + 2] as u32;
        let combined = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHA[((combined >> 18) & 0x3f) as usize] as char);
        out.push(ALPHA[((combined >> 12) & 0x3f) as usize] as char);
        out.push(ALPHA[((combined >> 6) & 0x3f) as usize] as char);
        out.push(ALPHA[(combined & 0x3f) as usize] as char);
        i += 3;
    }
    let rem = bytes.len() - i;
    if rem == 1 {
        let b0 = bytes[i] as u32;
        out.push(ALPHA[((b0 >> 2) & 0x3f) as usize] as char);
        out.push(ALPHA[((b0 << 4) & 0x3f) as usize] as char);
        out.push('=');
        out.push('=');
    } else if rem == 2 {
        let b0 = bytes[i] as u32;
        let b1 = bytes[i + 1] as u32;
        out.push(ALPHA[((b0 >> 2) & 0x3f) as usize] as char);
        out.push(ALPHA[(((b0 << 4) | (b1 >> 4)) & 0x3f) as usize] as char);
        out.push(ALPHA[((b1 << 2) & 0x3f) as usize] as char);
        out.push('=');
    }
    out
}

fn b64_decode(s: &str) -> Option<Vec<u8>> {
    fn val(c: u8) -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let bytes = s.as_bytes();
    if bytes.len() % 4 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity((bytes.len() / 4) * 3);
    let mut i = 0;
    while i < bytes.len() {
        let c0 = bytes[i];
        let c1 = bytes[i + 1];
        let c2 = bytes[i + 2];
        let c3 = bytes[i + 3];
        let v0 = val(c0)?;
        let v1 = val(c1)?;
        let (v2, has2) = if c2 == b'=' { (0u8, false) } else { (val(c2)?, true) };
        let (v3, has3) = if c3 == b'=' { (0u8, false) } else { (val(c3)?, true) };
        let combined = ((v0 as u32) << 18)
            | ((v1 as u32) << 12)
            | ((v2 as u32) << 6)
            | (v3 as u32);
        out.push(((combined >> 16) & 0xff) as u8);
        if has2 {
            out.push(((combined >> 8) & 0xff) as u8);
        }
        if has3 {
            out.push((combined & 0xff) as u8);
        }
        // No characters allowed after padding.
        if !has2 && i + 4 != bytes.len() {
            return None;
        }
        if !has3 && i + 4 != bytes.len() {
            return None;
        }
        i += 4;
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// Construction
// ---------------------------------------------------------------------------

fn builtin_bytes_new(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 0)?;
    Ok(NativeValue::Bytes(Vec::new()))
}

fn builtin_bytes_from_hex(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let s = expect_str(&args[0])?;
    match hex_decode(s) {
        Some(b) => Ok(lang_ok(NativeValue::Bytes(b))),
        None => Ok(lang_err(bytes_err_invalid_hex())),
    }
}

fn builtin_bytes_from_base64(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let s = expect_str(&args[0])?;
    match b64_decode(s) {
        Some(b) => Ok(lang_ok(NativeValue::Bytes(b))),
        None => Ok(lang_err(bytes_err_invalid_base64())),
    }
}

fn builtin_bytes_repeat(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let b = expect_bytes(&args[0])?;
    let n = expect_int(&args[1])?;
    if n <= 0 {
        return Ok(NativeValue::Bytes(Vec::new()));
    }
    let mut out = Vec::with_capacity(b.len() * (n as usize));
    for _ in 0..n {
        out.extend_from_slice(&b);
    }
    Ok(NativeValue::Bytes(out))
}

// ---------------------------------------------------------------------------
// Access
// ---------------------------------------------------------------------------

fn builtin_bytes_len(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let b = expect_bytes(&args[0])?;
    Ok(NativeValue::Int(b.len() as i64))
}

fn builtin_bytes_is_empty(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let b = expect_bytes(&args[0])?;
    Ok(NativeValue::Bool(b.is_empty()))
}

fn builtin_bytes_get(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let b = expect_bytes(&args[0])?;
    let idx = expect_int(&args[1])?;
    if idx < 0 || (idx as usize) >= b.len() {
        return Ok(lang_err(bytes_err_out_of_bounds(idx, b.len())));
    }
    Ok(lang_ok(NativeValue::Int(b[idx as usize] as i64)))
}

fn builtin_bytes_slice(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 3)?;
    let b = expect_bytes(&args[0])?;
    let from = expect_int(&args[1])?;
    let to = expect_int(&args[2])?;
    if from < 0 || to < 0 || (from as usize) > b.len() || (to as usize) > b.len() || from > to {
        let bad = if from < 0 || (from as usize) > b.len() {
            from
        } else {
            to
        };
        return Ok(lang_err(bytes_err_out_of_bounds(bad, b.len())));
    }
    Ok(lang_ok(NativeValue::Bytes(
        b[from as usize..to as usize].to_vec(),
    )))
}

// ---------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    if needle.len() > haystack.len() {
        return None;
    }
    (0..=haystack.len() - needle.len()).find(|&i| &haystack[i..i + needle.len()] == needle)
}

fn builtin_bytes_contains(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let h = expect_bytes(&args[0])?;
    let n = expect_bytes(&args[1])?;
    Ok(NativeValue::Bool(find_subsequence(&h, &n).is_some()))
}

fn builtin_bytes_find(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let h = expect_bytes(&args[0])?;
    let n = expect_bytes(&args[1])?;
    match find_subsequence(&h, &n) {
        Some(i) => Ok(lang_some(NativeValue::Int(i as i64))),
        None => Ok(lang_none()),
    }
}

// ---------------------------------------------------------------------------
// Combine
// ---------------------------------------------------------------------------

fn builtin_bytes_concat(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let a = expect_bytes(&args[0])?;
    let b = expect_bytes(&args[1])?;
    let mut out = Vec::with_capacity(a.len() + b.len());
    out.extend_from_slice(&a);
    out.extend_from_slice(&b);
    Ok(NativeValue::Bytes(out))
}

// ---------------------------------------------------------------------------
// Convert
// ---------------------------------------------------------------------------

fn builtin_bytes_to_hex(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let b = expect_bytes(&args[0])?;
    Ok(NativeValue::Str(hex_encode(&b)))
}

fn builtin_bytes_to_base64(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let b = expect_bytes(&args[0])?;
    Ok(NativeValue::Str(b64_encode(&b)))
}

fn builtin_bytes_to_str(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let b = expect_bytes(&args[0])?;
    match std::str::from_utf8(&b) {
        Ok(s) => Ok(lang_ok(NativeValue::Str(s.to_string()))),
        Err(_) => Ok(lang_err(bytes_err_invalid_utf8())),
    }
}

fn builtin_bytes_to_list(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let b = expect_bytes(&args[0])?;
    let elems: Vec<NativeValue> = b.iter().map(|byte| NativeValue::Int(*byte as i64)).collect();
    Ok(NativeValue::Array(elems))
}

fn builtin_bytes_from_list(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let arr = expect_array(&args[0])?;
    let mut out = Vec::with_capacity(arr.len());
    for elem in arr {
        let n = expect_int(elem)?;
        if !(0..=255).contains(&n) {
            return Ok(lang_err(bytes_err_invalid_byte(n)));
        }
        out.push(n as u8);
    }
    Ok(lang_ok(NativeValue::Bytes(out)))
}

// ---------------------------------------------------------------------------
// Mutable builder
// ---------------------------------------------------------------------------

fn builtin_bytes_builder(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 0)?;
    Ok(NativeValue::BytesMut(Rc::new(RefCell::new(Vec::new()))))
}

fn builtin_bytes_write_byte(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let buf = expect_bytes_mut(&args[0])?;
    let n = expect_int(&args[1])?;
    if !(0..=255).contains(&n) {
        return Err(bytes_err_invalid_byte(n));
    }
    buf.borrow_mut().push(n as u8);
    Ok(NativeValue::Unit)
}

fn builtin_bytes_write_bytes(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let buf = expect_bytes_mut(&args[0])?;
    let b = expect_bytes(&args[1])?;
    buf.borrow_mut().extend_from_slice(&b);
    Ok(NativeValue::Unit)
}

fn builtin_bytes_write_str(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let buf = expect_bytes_mut(&args[0])?;
    let s = expect_str(&args[1])?;
    buf.borrow_mut().extend_from_slice(s.as_bytes());
    Ok(NativeValue::Unit)
}

fn builtin_bytes_write_u16_le(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let buf = expect_bytes_mut(&args[0])?;
    let n = expect_int(&args[1])?;
    let v = (n as u32 & 0xffff) as u16;
    buf.borrow_mut().extend_from_slice(&v.to_le_bytes());
    Ok(NativeValue::Unit)
}

fn builtin_bytes_write_u16_be(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let buf = expect_bytes_mut(&args[0])?;
    let n = expect_int(&args[1])?;
    let v = (n as u32 & 0xffff) as u16;
    buf.borrow_mut().extend_from_slice(&v.to_be_bytes());
    Ok(NativeValue::Unit)
}

fn builtin_bytes_write_u32_le(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let buf = expect_bytes_mut(&args[0])?;
    let n = expect_int(&args[1])?;
    let v = (n as u64 & 0xffff_ffff) as u32;
    buf.borrow_mut().extend_from_slice(&v.to_le_bytes());
    Ok(NativeValue::Unit)
}

fn builtin_bytes_write_u32_be(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let buf = expect_bytes_mut(&args[0])?;
    let n = expect_int(&args[1])?;
    let v = (n as u64 & 0xffff_ffff) as u32;
    buf.borrow_mut().extend_from_slice(&v.to_be_bytes());
    Ok(NativeValue::Unit)
}

fn builtin_bytes_write_i64_le(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let buf = expect_bytes_mut(&args[0])?;
    let n = expect_int(&args[1])?;
    buf.borrow_mut().extend_from_slice(&n.to_le_bytes());
    Ok(NativeValue::Unit)
}

fn builtin_bytes_write_i64_be(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let buf = expect_bytes_mut(&args[0])?;
    let n = expect_int(&args[1])?;
    buf.borrow_mut().extend_from_slice(&n.to_be_bytes());
    Ok(NativeValue::Unit)
}

fn builtin_bytes_build(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let buf = expect_bytes_mut(&args[0])?;
    // Take ownership of the inner Vec, leaving the builder empty. If other
    // refs exist, fall back to cloning. Either way the spec says "consumes
    // the builder" — we don't enforce that at the type level here, just
    // produce the final Bytes value.
    let out = std::mem::take(&mut *buf.borrow_mut());
    Ok(NativeValue::Bytes(out))
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

pub fn register(registry: &mut NativeRegistry, interner: &mut Interner) {
    registry.register(interner.intern("bytes_new"), builtin_bytes_new);
    registry.register(interner.intern("bytes_from_hex"), builtin_bytes_from_hex);
    registry.register(interner.intern("bytes_from_base64"), builtin_bytes_from_base64);
    registry.register(interner.intern("bytes_repeat"), builtin_bytes_repeat);
    registry.register(interner.intern("bytes_len"), builtin_bytes_len);
    registry.register(interner.intern("bytes_is_empty"), builtin_bytes_is_empty);
    registry.register(interner.intern("bytes_get"), builtin_bytes_get);
    registry.register(interner.intern("bytes_slice"), builtin_bytes_slice);
    registry.register(interner.intern("bytes_contains"), builtin_bytes_contains);
    registry.register(interner.intern("bytes_find"), builtin_bytes_find);
    registry.register(interner.intern("bytes_concat"), builtin_bytes_concat);
    registry.register(interner.intern("bytes_to_hex"), builtin_bytes_to_hex);
    registry.register(interner.intern("bytes_to_base64"), builtin_bytes_to_base64);
    registry.register(interner.intern("bytes_to_str"), builtin_bytes_to_str);
    registry.register(interner.intern("bytes_to_list"), builtin_bytes_to_list);
    registry.register(interner.intern("bytes_from_list"), builtin_bytes_from_list);
    registry.register(interner.intern("bytes_builder"), builtin_bytes_builder);
    registry.register(interner.intern("bytes_write_byte"), builtin_bytes_write_byte);
    registry.register(interner.intern("bytes_write_bytes"), builtin_bytes_write_bytes);
    registry.register(interner.intern("bytes_write_str"), builtin_bytes_write_str);
    registry.register(interner.intern("bytes_write_u16_le"), builtin_bytes_write_u16_le);
    registry.register(interner.intern("bytes_write_u16_be"), builtin_bytes_write_u16_be);
    registry.register(interner.intern("bytes_write_u32_le"), builtin_bytes_write_u32_le);
    registry.register(interner.intern("bytes_write_u32_be"), builtin_bytes_write_u32_be);
    registry.register(interner.intern("bytes_write_i64_le"), builtin_bytes_write_i64_le);
    registry.register(interner.intern("bytes_write_i64_be"), builtin_bytes_write_i64_be);
    registry.register(interner.intern("bytes_build"), builtin_bytes_build);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn s(x: &str) -> NativeValue {
        NativeValue::Str(x.to_string())
    }
    fn i(x: i64) -> NativeValue {
        NativeValue::Int(x)
    }
    fn b(x: &[u8]) -> NativeValue {
        NativeValue::Bytes(x.to_vec())
    }

    fn unwrap_ok(v: NativeValue) -> NativeValue {
        match v {
            NativeValue::Array(mut xs) => {
                assert_eq!(xs.len(), 2);
                let tag = xs.remove(0);
                let payload = xs.remove(0);
                assert_eq!(tag, NativeValue::Str("Ok".to_string()));
                payload
            }
            other => panic!("expected Ok-encoded array, got {other:?}"),
        }
    }

    fn assert_err(v: NativeValue) -> String {
        match v {
            NativeValue::Array(xs) => {
                assert_eq!(xs.len(), 2);
                assert_eq!(xs[0], NativeValue::Str("Err".to_string()));
                match &xs[1] {
                    NativeValue::Str(s) => s.clone(),
                    other => panic!("expected Err payload string, got {other:?}"),
                }
            }
            other => panic!("expected Err-encoded array, got {other:?}"),
        }
    }

    fn assert_some(v: NativeValue) -> NativeValue {
        match v {
            NativeValue::Array(mut xs) => {
                assert_eq!(xs.len(), 2);
                let tag = xs.remove(0);
                let payload = xs.remove(0);
                assert_eq!(tag, NativeValue::Str("Some".to_string()));
                payload
            }
            other => panic!("expected Some-encoded array, got {other:?}"),
        }
    }

    fn assert_none(v: NativeValue) {
        match v {
            NativeValue::Array(xs) => {
                assert_eq!(xs.len(), 1);
                assert_eq!(xs[0], NativeValue::Str("None".to_string()));
            }
            other => panic!("expected None-encoded array, got {other:?}"),
        }
    }

    fn unwrap_bytes(v: NativeValue) -> Vec<u8> {
        match v {
            NativeValue::Bytes(b) => b,
            other => panic!("expected Bytes, got {other:?}"),
        }
    }

    #[test]
    fn new_is_empty() {
        let r = builtin_bytes_new(&[]).unwrap();
        assert_eq!(r, NativeValue::Bytes(Vec::new()));
    }

    #[test]
    fn from_hex_decodes() {
        let r = builtin_bytes_from_hex(&[s("48656c6c6f")]).unwrap();
        assert_eq!(unwrap_bytes(unwrap_ok(r)), b"Hello".to_vec());
    }

    #[test]
    fn from_hex_case_insensitive() {
        let r = builtin_bytes_from_hex(&[s("48656C6C6F")]).unwrap();
        assert_eq!(unwrap_bytes(unwrap_ok(r)), b"Hello".to_vec());
    }

    #[test]
    fn from_hex_odd_length_errs() {
        let r = builtin_bytes_from_hex(&[s("abc")]).unwrap();
        let msg = assert_err(r);
        assert!(msg.contains("InvalidHex"), "got: {msg}");
    }

    #[test]
    fn from_hex_non_hex_errs() {
        let r = builtin_bytes_from_hex(&[s("zz")]).unwrap();
        let msg = assert_err(r);
        assert!(msg.contains("InvalidHex"), "got: {msg}");
    }

    #[test]
    fn to_hex_is_lowercase() {
        let r = builtin_bytes_to_hex(&[b(&[0xab, 0xCD, 0xef])]).unwrap();
        assert_eq!(r, NativeValue::Str("abcdef".to_string()));
    }

    #[test]
    fn hex_roundtrip() {
        let orig = b"\x00\x10\x80\xff\x7fABC".to_vec();
        let hex = builtin_bytes_to_hex(&[NativeValue::Bytes(orig.clone())]).unwrap();
        let back = builtin_bytes_from_hex(&[hex]).unwrap();
        assert_eq!(unwrap_bytes(unwrap_ok(back)), orig);
    }

    #[test]
    fn base64_roundtrip() {
        for input in [
            b"".to_vec(),
            b"f".to_vec(),
            b"fo".to_vec(),
            b"foo".to_vec(),
            b"foob".to_vec(),
            b"fooba".to_vec(),
            b"foobar".to_vec(),
        ] {
            let enc = builtin_bytes_to_base64(&[NativeValue::Bytes(input.clone())]).unwrap();
            let dec = builtin_bytes_from_base64(&[enc]).unwrap();
            assert_eq!(unwrap_bytes(unwrap_ok(dec)), input);
        }
    }

    #[test]
    fn base64_known_vector() {
        let r = builtin_bytes_to_base64(&[b(b"Hello")]).unwrap();
        assert_eq!(r, NativeValue::Str("SGVsbG8=".to_string()));
        let back = builtin_bytes_from_base64(&[s("SGVsbG8=")]).unwrap();
        assert_eq!(unwrap_bytes(unwrap_ok(back)), b"Hello".to_vec());
    }

    #[test]
    fn base64_invalid_errs() {
        let r = builtin_bytes_from_base64(&[s("Not!Valid")]).unwrap();
        let msg = assert_err(r);
        assert!(msg.contains("InvalidBase64"), "got: {msg}");
    }

    #[test]
    fn len_and_is_empty() {
        assert_eq!(builtin_bytes_len(&[b(b"abc")]).unwrap(), NativeValue::Int(3));
        assert_eq!(
            builtin_bytes_is_empty(&[b(b"")]).unwrap(),
            NativeValue::Bool(true)
        );
        assert_eq!(
            builtin_bytes_is_empty(&[b(b"x")]).unwrap(),
            NativeValue::Bool(false)
        );
    }

    #[test]
    fn get_in_bounds() {
        let r = builtin_bytes_get(&[b(b"ABC"), i(1)]).unwrap();
        assert_eq!(unwrap_ok(r), NativeValue::Int(b'B' as i64));
    }

    #[test]
    fn get_out_of_bounds() {
        let r = builtin_bytes_get(&[b(b"AB"), i(5)]).unwrap();
        let msg = assert_err(r);
        assert!(msg.contains("OutOfBounds"), "got: {msg}");
    }

    #[test]
    fn get_negative_oob() {
        let r = builtin_bytes_get(&[b(b"AB"), i(-1)]).unwrap();
        let msg = assert_err(r);
        assert!(msg.contains("OutOfBounds"), "got: {msg}");
    }

    #[test]
    fn slice_happy() {
        let r = builtin_bytes_slice(&[b(b"abcdef"), i(1), i(4)]).unwrap();
        assert_eq!(unwrap_bytes(unwrap_ok(r)), b"bcd".to_vec());
    }

    #[test]
    fn slice_oob() {
        let r = builtin_bytes_slice(&[b(b"ab"), i(0), i(99)]).unwrap();
        let msg = assert_err(r);
        assert!(msg.contains("OutOfBounds"), "got: {msg}");
    }

    #[test]
    fn concat_combines() {
        let r = builtin_bytes_concat(&[b(b"foo"), b(b"bar")]).unwrap();
        assert_eq!(unwrap_bytes(r), b"foobar".to_vec());
    }

    #[test]
    fn repeat_works() {
        let r = builtin_bytes_repeat(&[b(b"ab"), i(3)]).unwrap();
        assert_eq!(unwrap_bytes(r), b"ababab".to_vec());
    }

    #[test]
    fn repeat_zero_is_empty() {
        let r = builtin_bytes_repeat(&[b(b"ab"), i(0)]).unwrap();
        assert_eq!(unwrap_bytes(r), Vec::<u8>::new());
    }

    #[test]
    fn contains_and_find() {
        assert_eq!(
            builtin_bytes_contains(&[b(b"hello world"), b(b"world")]).unwrap(),
            NativeValue::Bool(true)
        );
        assert_eq!(
            builtin_bytes_contains(&[b(b"hello"), b(b"zzz")]).unwrap(),
            NativeValue::Bool(false)
        );
        let r = builtin_bytes_find(&[b(b"hello"), b(b"ll")]).unwrap();
        assert_eq!(assert_some(r), NativeValue::Int(2));
        let r = builtin_bytes_find(&[b(b"hello"), b(b"zz")]).unwrap();
        assert_none(r);
    }

    #[test]
    fn to_str_valid() {
        let r = builtin_bytes_to_str(&[b(b"hello")]).unwrap();
        assert_eq!(unwrap_ok(r), NativeValue::Str("hello".to_string()));
    }

    #[test]
    fn to_str_invalid_errs() {
        let r = builtin_bytes_to_str(&[b(&[0xff, 0xfe])]).unwrap();
        let msg = assert_err(r);
        assert!(msg.contains("InvalidUtf8"), "got: {msg}");
    }

    #[test]
    fn to_list_from_list_roundtrip() {
        let bytes = b(&[0, 127, 255]);
        let list = builtin_bytes_to_list(&[bytes.clone()]).unwrap();
        match &list {
            NativeValue::Array(v) => {
                assert_eq!(v.len(), 3);
                assert_eq!(v[0], NativeValue::Int(0));
                assert_eq!(v[1], NativeValue::Int(127));
                assert_eq!(v[2], NativeValue::Int(255));
            }
            other => panic!("expected array, got {other:?}"),
        }
        let back = builtin_bytes_from_list(&[list]).unwrap();
        assert_eq!(unwrap_bytes(unwrap_ok(back)), vec![0, 127, 255]);
    }

    #[test]
    fn from_list_rejects_oversized() {
        let arr = NativeValue::Array(vec![i(0), i(256)]);
        let r = builtin_bytes_from_list(&[arr]).unwrap();
        let msg = assert_err(r);
        assert!(msg.contains("InvalidByte"), "got: {msg}");
    }

    #[test]
    fn from_list_rejects_negative() {
        let arr = NativeValue::Array(vec![i(0), i(-1)]);
        let r = builtin_bytes_from_list(&[arr]).unwrap();
        let msg = assert_err(r);
        assert!(msg.contains("InvalidByte"), "got: {msg}");
    }

    #[test]
    fn builder_write_three_bytes() {
        let buf = builtin_bytes_builder(&[]).unwrap();
        builtin_bytes_write_byte(&[buf.clone(), i(1)]).unwrap();
        builtin_bytes_write_byte(&[buf.clone(), i(2)]).unwrap();
        builtin_bytes_write_byte(&[buf.clone(), i(3)]).unwrap();
        let out = builtin_bytes_build(&[buf]).unwrap();
        assert_eq!(unwrap_bytes(out), vec![1, 2, 3]);
    }

    #[test]
    fn builder_write_str() {
        let buf = builtin_bytes_builder(&[]).unwrap();
        builtin_bytes_write_str(&[buf.clone(), s("hi")]).unwrap();
        let out = builtin_bytes_build(&[buf]).unwrap();
        assert_eq!(unwrap_bytes(out), b"hi".to_vec());
    }

    #[test]
    fn builder_write_bytes() {
        let buf = builtin_bytes_builder(&[]).unwrap();
        builtin_bytes_write_bytes(&[buf.clone(), b(b"abc")]).unwrap();
        builtin_bytes_write_bytes(&[buf.clone(), b(b"def")]).unwrap();
        let out = builtin_bytes_build(&[buf]).unwrap();
        assert_eq!(unwrap_bytes(out), b"abcdef".to_vec());
    }

    #[test]
    fn write_u32_le() {
        let buf = builtin_bytes_builder(&[]).unwrap();
        builtin_bytes_write_u32_le(&[buf.clone(), i(0x01020304)]).unwrap();
        let out = builtin_bytes_build(&[buf]).unwrap();
        assert_eq!(unwrap_bytes(out), vec![0x04, 0x03, 0x02, 0x01]);
    }

    #[test]
    fn write_u32_be() {
        let buf = builtin_bytes_builder(&[]).unwrap();
        builtin_bytes_write_u32_be(&[buf.clone(), i(0x01020304)]).unwrap();
        let out = builtin_bytes_build(&[buf]).unwrap();
        assert_eq!(unwrap_bytes(out), vec![0x01, 0x02, 0x03, 0x04]);
    }

    #[test]
    fn write_u16_le_and_be() {
        let buf = builtin_bytes_builder(&[]).unwrap();
        builtin_bytes_write_u16_le(&[buf.clone(), i(0x0102)]).unwrap();
        builtin_bytes_write_u16_be(&[buf.clone(), i(0x0304)]).unwrap();
        let out = builtin_bytes_build(&[buf]).unwrap();
        assert_eq!(unwrap_bytes(out), vec![0x02, 0x01, 0x03, 0x04]);
    }

    #[test]
    fn write_i64_le_and_be() {
        let buf = builtin_bytes_builder(&[]).unwrap();
        builtin_bytes_write_i64_le(&[buf.clone(), i(1)]).unwrap();
        builtin_bytes_write_i64_be(&[buf.clone(), i(1)]).unwrap();
        let out = unwrap_bytes(builtin_bytes_build(&[buf]).unwrap());
        assert_eq!(
            out,
            vec![1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]
        );
    }
}
