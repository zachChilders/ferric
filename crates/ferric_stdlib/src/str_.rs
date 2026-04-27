//! `str` — UTF-8 string manipulation.
//!
//! See `docs/stdlib.md` § "`str` — String Manipulation" for the spec.
//!
//! All character-level operations count Unicode scalar values, not bytes.
//! Byte-level operations live in the `bytes` module.

// REQUIRED NATIVE VALUE VARIANTS:
//   (none beyond what already exists — Str(String) and Array(Vec<NativeValue>))
//   This module assumes `NativeValue::Bytes(Vec<u8>)` exists for to_bytes /
//   from_bytes interop. That variant is declared by the `bytes` module.

// REQUIRED DEPS:
//   none — pure std.

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
        other => Err(format!("expected string, got {:?}", other)),
    }
}

fn expect_int(value: &NativeValue) -> Result<i64, String> {
    match value {
        NativeValue::Int(n) => Ok(*n),
        other => Err(format!("expected int, got {:?}", other)),
    }
}

fn expect_bytes(value: &NativeValue) -> Result<&Vec<u8>, String> {
    match value {
        NativeValue::Bytes(b) => Ok(b),
        other => Err(format!("expected bytes, got {:?}", other)),
    }
}

/// Returns the byte index in `s` corresponding to the given character index,
/// or `s.len()` if `char_idx == s.chars().count()`. Returns `None` if
/// `char_idx > s.chars().count()`.
fn char_idx_to_byte_idx(s: &str, char_idx: usize) -> Option<usize> {
    if char_idx == 0 {
        return Some(0);
    }
    let mut count = 0usize;
    for (b, _) in s.char_indices() {
        if count == char_idx {
            return Some(b);
        }
        count += 1;
    }
    if count == char_idx {
        Some(s.len())
    } else {
        None
    }
}

/// Result helpers: native ABI is `Result<NativeValue, String>`. Language-level
/// Result/Option values are encoded as a `NativeValue::Str(...)` placeholder
/// that names the variant — this matches the convention from the task file.
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

fn str_err_out_of_bounds(idx: i64, len: usize) -> String {
    format!("StrError::OutOfBounds {{ index: {}, len: {} }}", idx, len)
}

fn str_err_parse_failed(input: &str) -> String {
    format!("StrError::ParseFailed {{ input: {:?} }}", input)
}

fn str_err_invalid_utf8() -> String {
    "StrError::InvalidUtf8".to_string()
}

// ---------------------------------------------------------------------------
// str::len
// ---------------------------------------------------------------------------

fn builtin_str_len(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let s = expect_str(&args[0])?;
    Ok(NativeValue::Int(s.chars().count() as i64))
}

// ---------------------------------------------------------------------------
// str::slice — char-index based, returns Result<Str, StrError>
// ---------------------------------------------------------------------------

fn builtin_str_slice(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 3)?;
    let s = expect_str(&args[0])?;
    let from = expect_int(&args[1])?;
    let to = expect_int(&args[2])?;
    let total = s.chars().count();

    if from < 0 || to < 0 {
        return Ok(lang_err(str_err_out_of_bounds(from.min(to), total)));
    }
    let from_u = from as usize;
    let to_u = to as usize;
    if from_u > total || to_u > total || from_u > to_u {
        let bad = if from_u > total { from } else { to };
        return Ok(lang_err(str_err_out_of_bounds(bad, total)));
    }
    let from_b = match char_idx_to_byte_idx(s, from_u) {
        Some(b) => b,
        None => return Ok(lang_err(str_err_out_of_bounds(from, total))),
    };
    let to_b = match char_idx_to_byte_idx(s, to_u) {
        Some(b) => b,
        None => return Ok(lang_err(str_err_out_of_bounds(to, total))),
    };
    Ok(lang_ok(NativeValue::Str(s[from_b..to_b].to_string())))
}

// ---------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------

fn builtin_str_contains(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let s = expect_str(&args[0])?;
    let needle = expect_str(&args[1])?;
    Ok(NativeValue::Bool(s.contains(needle.as_str())))
}

fn builtin_str_starts_with(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let s = expect_str(&args[0])?;
    let prefix = expect_str(&args[1])?;
    Ok(NativeValue::Bool(s.starts_with(prefix.as_str())))
}

fn builtin_str_ends_with(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let s = expect_str(&args[0])?;
    let suffix = expect_str(&args[1])?;
    Ok(NativeValue::Bool(s.ends_with(suffix.as_str())))
}

/// Returns Option<Int> — the character index of the first match, or None.
fn builtin_str_find(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let s = expect_str(&args[0])?;
    let needle = expect_str(&args[1])?;
    if needle.is_empty() {
        return Ok(lang_some(NativeValue::Int(0)));
    }
    match s.find(needle.as_str()) {
        Some(byte_idx) => {
            // Convert byte index → char index.
            let char_idx = s[..byte_idx].chars().count() as i64;
            Ok(lang_some(NativeValue::Int(char_idx)))
        }
        None => Ok(lang_none()),
    }
}

/// Returns List<Int> — character indices of every (non-overlapping) match.
fn builtin_str_find_all(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let s = expect_str(&args[0])?;
    let needle = expect_str(&args[1])?;
    let mut out = Vec::new();
    if needle.is_empty() {
        // Match between every character (including ends) — len+1 positions.
        for i in 0..=s.chars().count() {
            out.push(NativeValue::Int(i as i64));
        }
        return Ok(NativeValue::Array(out));
    }
    let mut start = 0usize;
    while start <= s.len() {
        match s[start..].find(needle.as_str()) {
            Some(rel) => {
                let abs = start + rel;
                let char_idx = s[..abs].chars().count() as i64;
                out.push(NativeValue::Int(char_idx));
                start = abs + needle.len();
            }
            None => break,
        }
    }
    Ok(NativeValue::Array(out))
}

// ---------------------------------------------------------------------------
// Splitting
// ---------------------------------------------------------------------------

fn builtin_str_split(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let s = expect_str(&args[0])?;
    let on = expect_str(&args[1])?;
    let parts: Vec<NativeValue> = if on.is_empty() {
        s.chars().map(|c| NativeValue::Str(c.to_string())).collect()
    } else {
        s.split(on.as_str())
            .map(|p| NativeValue::Str(p.to_string()))
            .collect()
    };
    Ok(NativeValue::Array(parts))
}

fn builtin_str_split_once(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let s = expect_str(&args[0])?;
    let on = expect_str(&args[1])?;
    if on.is_empty() {
        return Ok(lang_none());
    }
    match s.split_once(on.as_str()) {
        Some((a, b)) => Ok(lang_some(NativeValue::Array(vec![
            NativeValue::Str(a.to_string()),
            NativeValue::Str(b.to_string()),
        ]))),
        None => Ok(lang_none()),
    }
}

fn builtin_str_lines(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let s = expect_str(&args[0])?;
    // `str::lines` splits on \n and strips a trailing \r — this is the same
    // semantics described by the spec.
    let parts: Vec<NativeValue> = s
        .lines()
        .map(|l| NativeValue::Str(l.to_string()))
        .collect();
    Ok(NativeValue::Array(parts))
}

fn builtin_str_chars(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let s = expect_str(&args[0])?;
    let parts: Vec<NativeValue> = s
        .chars()
        .map(|c| NativeValue::Str(c.to_string()))
        .collect();
    Ok(NativeValue::Array(parts))
}

// ---------------------------------------------------------------------------
// Trimming
// ---------------------------------------------------------------------------

fn builtin_str_trim(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let s = expect_str(&args[0])?;
    Ok(NativeValue::Str(s.trim().to_string()))
}

fn builtin_str_trim_start(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let s = expect_str(&args[0])?;
    Ok(NativeValue::Str(s.trim_start().to_string()))
}

fn builtin_str_trim_end(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let s = expect_str(&args[0])?;
    Ok(NativeValue::Str(s.trim_end().to_string()))
}

fn builtin_str_trim_chars(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let s = expect_str(&args[0])?;
    let chars = expect_str(&args[1])?;
    let chars_set: Vec<char> = chars.chars().collect();
    Ok(NativeValue::Str(
        s.trim_matches(|c: char| chars_set.contains(&c)).to_string(),
    ))
}

// ---------------------------------------------------------------------------
// Case
// ---------------------------------------------------------------------------

fn builtin_str_to_upper(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let s = expect_str(&args[0])?;
    Ok(NativeValue::Str(s.to_uppercase()))
}

fn builtin_str_to_lower(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let s = expect_str(&args[0])?;
    Ok(NativeValue::Str(s.to_lowercase()))
}

fn builtin_str_to_title(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let s = expect_str(&args[0])?;
    let mut out = String::with_capacity(s.len());
    let mut new_word = true;
    for c in s.chars() {
        if c.is_whitespace() {
            new_word = true;
            out.push(c);
        } else if new_word {
            for u in c.to_uppercase() {
                out.push(u);
            }
            new_word = false;
        } else {
            for l in c.to_lowercase() {
                out.push(l);
            }
        }
    }
    Ok(NativeValue::Str(out))
}

// ---------------------------------------------------------------------------
// Padding
// ---------------------------------------------------------------------------

fn pad_with_repeat(s: &str, target_chars: usize, with: &str, at_start: bool) -> String {
    let cur = s.chars().count();
    if cur >= target_chars || with.is_empty() {
        return s.to_string();
    }
    let needed = target_chars - cur;
    let with_chars: Vec<char> = with.chars().collect();
    let with_len = with_chars.len();
    let mut pad = String::new();
    for i in 0..needed {
        pad.push(with_chars[i % with_len]);
    }
    if at_start {
        format!("{}{}", pad, s)
    } else {
        format!("{}{}", s, pad)
    }
}

fn builtin_str_pad_start(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 3)?;
    let s = expect_str(&args[0])?;
    let to = expect_int(&args[1])?;
    let with = expect_str(&args[2])?;
    if to < 0 {
        return Ok(NativeValue::Str(s.clone()));
    }
    Ok(NativeValue::Str(pad_with_repeat(s, to as usize, with, true)))
}

fn builtin_str_pad_end(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 3)?;
    let s = expect_str(&args[0])?;
    let to = expect_int(&args[1])?;
    let with = expect_str(&args[2])?;
    if to < 0 {
        return Ok(NativeValue::Str(s.clone()));
    }
    Ok(NativeValue::Str(pad_with_repeat(s, to as usize, with, false)))
}

fn builtin_str_center(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 3)?;
    let s = expect_str(&args[0])?;
    let to = expect_int(&args[1])?;
    let with = expect_str(&args[2])?;
    if to < 0 {
        return Ok(NativeValue::Str(s.clone()));
    }
    let target = to as usize;
    let cur = s.chars().count();
    if cur >= target || with.is_empty() {
        return Ok(NativeValue::Str(s.clone()));
    }
    let total_pad = target - cur;
    let left_pad = total_pad / 2;
    let right_pad = total_pad - left_pad;
    let with_chars: Vec<char> = with.chars().collect();
    let with_len = with_chars.len();
    let mut left = String::new();
    for i in 0..left_pad {
        left.push(with_chars[i % with_len]);
    }
    let mut right = String::new();
    for i in 0..right_pad {
        right.push(with_chars[i % with_len]);
    }
    Ok(NativeValue::Str(format!("{}{}{}", left, s, right)))
}

// ---------------------------------------------------------------------------
// Replace
// ---------------------------------------------------------------------------

fn builtin_str_replace(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 3)?;
    let s = expect_str(&args[0])?;
    let from = expect_str(&args[1])?;
    let to = expect_str(&args[2])?;
    if from.is_empty() {
        return Ok(NativeValue::Str(s.clone()));
    }
    Ok(NativeValue::Str(s.replace(from.as_str(), to.as_str())))
}

fn builtin_str_replace_first(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 3)?;
    let s = expect_str(&args[0])?;
    let from = expect_str(&args[1])?;
    let to = expect_str(&args[2])?;
    if from.is_empty() {
        return Ok(NativeValue::Str(s.clone()));
    }
    Ok(NativeValue::Str(s.replacen(from.as_str(), to.as_str(), 1)))
}

fn builtin_str_replace_n(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 4)?;
    let s = expect_str(&args[0])?;
    let from = expect_str(&args[1])?;
    let to = expect_str(&args[2])?;
    let n = expect_int(&args[3])?;
    if from.is_empty() || n <= 0 {
        return Ok(NativeValue::Str(s.clone()));
    }
    Ok(NativeValue::Str(
        s.replacen(from.as_str(), to.as_str(), n as usize),
    ))
}

// ---------------------------------------------------------------------------
// Join
// ---------------------------------------------------------------------------

fn builtin_str_join(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let parts = match &args[0] {
        NativeValue::Array(v) => v,
        other => return Err(format!("expected list, got {:?}", other)),
    };
    let sep = expect_str(&args[1])?;
    let mut out = String::new();
    for (i, p) in parts.iter().enumerate() {
        if i > 0 {
            out.push_str(sep.as_str());
        }
        match p {
            NativeValue::Str(s) => out.push_str(s),
            other => return Err(format!("expected list of strings, got {:?}", other)),
        }
    }
    Ok(NativeValue::Str(out))
}

// ---------------------------------------------------------------------------
// Repeat
// ---------------------------------------------------------------------------

fn builtin_str_repeat(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let s = expect_str(&args[0])?;
    let n = expect_int(&args[1])?;
    if n <= 0 {
        return Ok(NativeValue::Str(String::new()));
    }
    Ok(NativeValue::Str(s.repeat(n as usize)))
}

// ---------------------------------------------------------------------------
// Parse
// ---------------------------------------------------------------------------

fn builtin_str_parse_int(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let s = expect_str(&args[0])?;
    match s.trim().parse::<i64>() {
        Ok(n) => Ok(lang_ok(NativeValue::Int(n))),
        Err(_) => Ok(lang_err(str_err_parse_failed(s))),
    }
}

fn builtin_str_parse_float(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let s = expect_str(&args[0])?;
    match s.trim().parse::<f64>() {
        Ok(f) => Ok(lang_ok(NativeValue::Float(f))),
        Err(_) => Ok(lang_err(str_err_parse_failed(s))),
    }
}

fn builtin_str_parse_bool(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let s = expect_str(&args[0])?;
    match s.as_str() {
        "true" => Ok(lang_ok(NativeValue::Bool(true))),
        "false" => Ok(lang_ok(NativeValue::Bool(false))),
        _ => Ok(lang_err(str_err_parse_failed(s))),
    }
}

// ---------------------------------------------------------------------------
// Comparison
// ---------------------------------------------------------------------------

fn builtin_str_eq_ignore_case(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let a = expect_str(&args[0])?;
    let b = expect_str(&args[1])?;
    Ok(NativeValue::Bool(a.eq_ignore_ascii_case(b.as_str())))
}

// ---------------------------------------------------------------------------
// Bytes interop
// ---------------------------------------------------------------------------

fn builtin_str_to_bytes(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let s = expect_str(&args[0])?;
    Ok(NativeValue::Bytes(s.as_bytes().to_vec()))
}

fn builtin_str_from_bytes(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let bytes = expect_bytes(&args[0])?;
    match std::str::from_utf8(bytes) {
        Ok(s) => Ok(lang_ok(NativeValue::Str(s.to_string()))),
        Err(_) => Ok(lang_err(str_err_invalid_utf8())),
    }
}

fn builtin_str_from_bytes_lossy(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let bytes = expect_bytes(&args[0])?;
    let cow = String::from_utf8_lossy(bytes);
    Ok(NativeValue::Str(cow.into_owned()))
}

// ---------------------------------------------------------------------------
// Predicates
// ---------------------------------------------------------------------------

fn builtin_str_is_empty(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let s = expect_str(&args[0])?;
    Ok(NativeValue::Bool(s.is_empty()))
}

fn builtin_str_is_ascii(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let s = expect_str(&args[0])?;
    Ok(NativeValue::Bool(s.is_ascii()))
}

fn builtin_str_is_numeric(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let s = expect_str(&args[0])?;
    if s.is_empty() {
        return Ok(NativeValue::Bool(false));
    }
    Ok(NativeValue::Bool(s.chars().all(|c| c.is_ascii_digit())))
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

pub fn register(registry: &mut NativeRegistry, interner: &mut Interner) {
    registry.register(interner.intern("str_len"), builtin_str_len);
    registry.register(interner.intern("str_slice"), builtin_str_slice);
    registry.register(interner.intern("str_contains"), builtin_str_contains);
    registry.register(interner.intern("str_starts_with"), builtin_str_starts_with);
    registry.register(interner.intern("str_ends_with"), builtin_str_ends_with);
    registry.register(interner.intern("str_find"), builtin_str_find);
    registry.register(interner.intern("str_find_all"), builtin_str_find_all);
    registry.register(interner.intern("str_split"), builtin_str_split);
    registry.register(interner.intern("str_split_once"), builtin_str_split_once);
    registry.register(interner.intern("str_lines"), builtin_str_lines);
    registry.register(interner.intern("str_chars"), builtin_str_chars);
    registry.register(interner.intern("str_trim"), builtin_str_trim);
    registry.register(interner.intern("str_trim_start"), builtin_str_trim_start);
    registry.register(interner.intern("str_trim_end"), builtin_str_trim_end);
    registry.register(interner.intern("str_trim_chars"), builtin_str_trim_chars);
    registry.register(interner.intern("str_to_upper"), builtin_str_to_upper);
    registry.register(interner.intern("str_to_lower"), builtin_str_to_lower);
    registry.register(interner.intern("str_to_title"), builtin_str_to_title);
    registry.register(interner.intern("str_pad_start"), builtin_str_pad_start);
    registry.register(interner.intern("str_pad_end"), builtin_str_pad_end);
    registry.register(interner.intern("str_center"), builtin_str_center);
    registry.register(interner.intern("str_replace"), builtin_str_replace);
    registry.register(interner.intern("str_replace_first"), builtin_str_replace_first);
    registry.register(interner.intern("str_replace_n"), builtin_str_replace_n);
    registry.register(interner.intern("str_join"), builtin_str_join);
    registry.register(interner.intern("str_repeat"), builtin_str_repeat);
    registry.register(interner.intern("str_parse_int"), builtin_str_parse_int);
    registry.register(interner.intern("str_parse_float"), builtin_str_parse_float);
    registry.register(interner.intern("str_parse_bool"), builtin_str_parse_bool);
    registry.register(interner.intern("str_eq_ignore_case"), builtin_str_eq_ignore_case);
    registry.register(interner.intern("str_to_bytes"), builtin_str_to_bytes);
    registry.register(interner.intern("str_from_bytes"), builtin_str_from_bytes);
    registry.register(interner.intern("str_from_bytes_lossy"), builtin_str_from_bytes_lossy);
    registry.register(interner.intern("str_is_empty"), builtin_str_is_empty);
    registry.register(interner.intern("str_is_ascii"), builtin_str_is_ascii);
    registry.register(interner.intern("str_is_numeric"), builtin_str_is_numeric);
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

    fn unwrap_ok(v: NativeValue) -> NativeValue {
        match v {
            NativeValue::Array(mut xs) => {
                assert_eq!(xs.len(), 2);
                let tag = xs.remove(0);
                let payload = xs.remove(0);
                assert_eq!(tag, NativeValue::Str("Ok".to_string()));
                payload
            }
            other => panic!("expected Ok-encoded array, got {:?}", other),
        }
    }

    fn assert_err(v: NativeValue) -> String {
        match v {
            NativeValue::Array(xs) => {
                assert_eq!(xs.len(), 2);
                assert_eq!(xs[0], NativeValue::Str("Err".to_string()));
                match &xs[1] {
                    NativeValue::Str(s) => s.clone(),
                    other => panic!("expected Err payload string, got {:?}", other),
                }
            }
            other => panic!("expected Err-encoded array, got {:?}", other),
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
            other => panic!("expected Some-encoded array, got {:?}", other),
        }
    }

    fn assert_none(v: NativeValue) {
        match v {
            NativeValue::Array(xs) => {
                assert_eq!(xs.len(), 1);
                assert_eq!(xs[0], NativeValue::Str("None".to_string()));
            }
            other => panic!("expected None-encoded array, got {:?}", other),
        }
    }

    #[test]
    fn len_counts_chars_not_bytes() {
        let r = builtin_str_len(&[s("café")]).unwrap();
        assert_eq!(r, NativeValue::Int(4));
    }

    #[test]
    fn len_empty() {
        let r = builtin_str_len(&[s("")]).unwrap();
        assert_eq!(r, NativeValue::Int(0));
    }

    #[test]
    fn len_ascii() {
        let r = builtin_str_len(&[s("hello")]).unwrap();
        assert_eq!(r, NativeValue::Int(5));
    }

    #[test]
    fn slice_happy_path() {
        let r = builtin_str_slice(&[s("hello"), i(1), i(4)]).unwrap();
        assert_eq!(unwrap_ok(r), NativeValue::Str("ell".to_string()));
    }

    #[test]
    fn slice_unicode() {
        let r = builtin_str_slice(&[s("café"), i(0), i(4)]).unwrap();
        assert_eq!(unwrap_ok(r), NativeValue::Str("café".to_string()));
    }

    #[test]
    fn slice_out_of_bounds() {
        let r = builtin_str_slice(&[s("hi"), i(0), i(99)]).unwrap();
        let msg = assert_err(r);
        assert!(msg.contains("OutOfBounds"), "got: {}", msg);
    }

    #[test]
    fn contains_true() {
        let r = builtin_str_contains(&[s("hello world"), s("world")]).unwrap();
        assert_eq!(r, NativeValue::Bool(true));
    }

    #[test]
    fn contains_false() {
        let r = builtin_str_contains(&[s("hello"), s("zzz")]).unwrap();
        assert_eq!(r, NativeValue::Bool(false));
    }

    #[test]
    fn starts_with_variants() {
        assert_eq!(
            builtin_str_starts_with(&[s("hello"), s("he")]).unwrap(),
            NativeValue::Bool(true)
        );
        assert_eq!(
            builtin_str_starts_with(&[s("hello"), s("lo")]).unwrap(),
            NativeValue::Bool(false)
        );
    }

    #[test]
    fn ends_with_variants() {
        assert_eq!(
            builtin_str_ends_with(&[s("hello"), s("lo")]).unwrap(),
            NativeValue::Bool(true)
        );
        assert_eq!(
            builtin_str_ends_with(&[s("hello"), s("he")]).unwrap(),
            NativeValue::Bool(false)
        );
    }

    #[test]
    fn split_three_parts() {
        let r = builtin_str_split(&[s("a,b,c"), s(",")]).unwrap();
        match r {
            NativeValue::Array(v) => {
                assert_eq!(v.len(), 3);
                assert_eq!(v[0], NativeValue::Str("a".to_string()));
                assert_eq!(v[1], NativeValue::Str("b".to_string()));
                assert_eq!(v[2], NativeValue::Str("c".to_string()));
            }
            other => panic!("expected array, got {:?}", other),
        }
    }

    #[test]
    fn split_empty_string() {
        let r = builtin_str_split(&[s(""), s(",")]).unwrap();
        match r {
            NativeValue::Array(v) => {
                assert_eq!(v.len(), 1);
                assert_eq!(v[0], NativeValue::Str("".to_string()));
            }
            other => panic!("expected array, got {:?}", other),
        }
    }

    #[test]
    fn split_once_some() {
        let r = builtin_str_split_once(&[s("a=b=c"), s("=")]).unwrap();
        let payload = assert_some(r);
        match payload {
            NativeValue::Array(v) => {
                assert_eq!(v[0], NativeValue::Str("a".to_string()));
                assert_eq!(v[1], NativeValue::Str("b=c".to_string()));
            }
            other => panic!("expected array tuple, got {:?}", other),
        }
    }

    #[test]
    fn split_once_none() {
        let r = builtin_str_split_once(&[s("abc"), s("=")]).unwrap();
        assert_none(r);
    }

    #[test]
    fn lines_three_entries() {
        let r = builtin_str_lines(&[s("a\r\nb\nc")]).unwrap();
        match r {
            NativeValue::Array(v) => {
                assert_eq!(v.len(), 3);
                assert_eq!(v[0], NativeValue::Str("a".to_string()));
                assert_eq!(v[1], NativeValue::Str("b".to_string()));
                assert_eq!(v[2], NativeValue::Str("c".to_string()));
            }
            other => panic!("expected array, got {:?}", other),
        }
    }

    #[test]
    fn trim_strips_ws() {
        let r = builtin_str_trim(&[s("  hi  ")]).unwrap();
        assert_eq!(r, NativeValue::Str("hi".to_string()));
    }

    #[test]
    fn trim_start_only() {
        let r = builtin_str_trim_start(&[s("  hi  ")]).unwrap();
        assert_eq!(r, NativeValue::Str("hi  ".to_string()));
    }

    #[test]
    fn trim_end_only() {
        let r = builtin_str_trim_end(&[s("  hi  ")]).unwrap();
        assert_eq!(r, NativeValue::Str("  hi".to_string()));
    }

    #[test]
    fn trim_chars_set() {
        let r = builtin_str_trim_chars(&[s("xxhelloxx"), s("x")]).unwrap();
        assert_eq!(r, NativeValue::Str("hello".to_string()));
    }

    #[test]
    fn case_conversions() {
        assert_eq!(
            builtin_str_to_upper(&[s("héllo")]).unwrap(),
            NativeValue::Str("HÉLLO".to_string())
        );
        assert_eq!(
            builtin_str_to_lower(&[s("HÉLLO")]).unwrap(),
            NativeValue::Str("héllo".to_string())
        );
        assert_eq!(
            builtin_str_to_title(&[s("hello world")]).unwrap(),
            NativeValue::Str("Hello World".to_string())
        );
    }

    #[test]
    fn pad_start_zeroes() {
        let r = builtin_str_pad_start(&[s("7"), i(3), s("0")]).unwrap();
        assert_eq!(r, NativeValue::Str("007".to_string()));
    }

    #[test]
    fn pad_end_dots() {
        let r = builtin_str_pad_end(&[s("hi"), i(5), s(".")]).unwrap();
        assert_eq!(r, NativeValue::Str("hi...".to_string()));
    }

    #[test]
    fn center_balanced() {
        let r = builtin_str_center(&[s("hi"), i(6), s("-")]).unwrap();
        assert_eq!(r, NativeValue::Str("--hi--".to_string()));
    }

    #[test]
    fn pad_no_op_when_already_long_enough() {
        let r = builtin_str_pad_start(&[s("hello"), i(3), s("0")]).unwrap();
        assert_eq!(r, NativeValue::Str("hello".to_string()));
    }

    #[test]
    fn replace_replaces_all() {
        let r = builtin_str_replace(&[s("a-b-c"), s("-"), s("/")]).unwrap();
        assert_eq!(r, NativeValue::Str("a/b/c".to_string()));
    }

    #[test]
    fn replace_first_only_first() {
        let r = builtin_str_replace_first(&[s("a-b-c"), s("-"), s("/")]).unwrap();
        assert_eq!(r, NativeValue::Str("a/b-c".to_string()));
    }

    #[test]
    fn replace_n_exactly_n() {
        let r = builtin_str_replace_n(&[s("a-b-c-d"), s("-"), s("/"), i(2)]).unwrap();
        assert_eq!(r, NativeValue::Str("a/b/c-d".to_string()));
    }

    #[test]
    fn parse_int_ok() {
        let r = builtin_str_parse_int(&[s("42")]).unwrap();
        assert_eq!(unwrap_ok(r), NativeValue::Int(42));
    }

    #[test]
    fn parse_int_err() {
        let r = builtin_str_parse_int(&[s("foo")]).unwrap();
        let msg = assert_err(r);
        assert!(msg.contains("ParseFailed"), "got: {}", msg);
    }

    #[test]
    fn parse_float_ok_and_err() {
        let ok = builtin_str_parse_float(&[s("3.14")]).unwrap();
        match unwrap_ok(ok) {
            NativeValue::Float(f) => assert!((f - 3.14).abs() < 1e-9),
            other => panic!("expected float, got {:?}", other),
        }
        let err = builtin_str_parse_float(&[s("nope")]).unwrap();
        let msg = assert_err(err);
        assert!(msg.contains("ParseFailed"));
    }

    #[test]
    fn parse_bool_ok_and_err() {
        assert_eq!(
            unwrap_ok(builtin_str_parse_bool(&[s("true")]).unwrap()),
            NativeValue::Bool(true)
        );
        assert_eq!(
            unwrap_ok(builtin_str_parse_bool(&[s("false")]).unwrap()),
            NativeValue::Bool(false)
        );
        let msg = assert_err(builtin_str_parse_bool(&[s("True")]).unwrap());
        assert!(msg.contains("ParseFailed"));
    }

    #[test]
    fn eq_ignore_case_true() {
        let r = builtin_str_eq_ignore_case(&[s("ABC"), s("abc")]).unwrap();
        assert_eq!(r, NativeValue::Bool(true));
    }

    #[test]
    fn eq_ignore_case_false() {
        let r = builtin_str_eq_ignore_case(&[s("abc"), s("def")]).unwrap();
        assert_eq!(r, NativeValue::Bool(false));
    }

    #[test]
    fn to_bytes_from_bytes_roundtrip() {
        let bytes = builtin_str_to_bytes(&[s("hello")]).unwrap();
        match &bytes {
            NativeValue::Bytes(b) => assert_eq!(b, b"hello"),
            other => panic!("expected bytes, got {:?}", other),
        }
        let back = builtin_str_from_bytes(&[bytes]).unwrap();
        assert_eq!(unwrap_ok(back), NativeValue::Str("hello".to_string()));
    }

    #[test]
    fn from_bytes_invalid_utf8_errs() {
        let bad = NativeValue::Bytes(vec![0xff, 0xfe, 0xfd]);
        let r = builtin_str_from_bytes(&[bad]).unwrap();
        let msg = assert_err(r);
        assert!(msg.contains("InvalidUtf8"));
    }

    #[test]
    fn from_bytes_lossy_substitutes_replacement() {
        let bad = NativeValue::Bytes(vec![0xff, 0xfe, 0xfd]);
        let r = builtin_str_from_bytes_lossy(&[bad]).unwrap();
        match r {
            NativeValue::Str(s) => {
                assert!(s.contains('\u{FFFD}'), "expected U+FFFD in {:?}", s);
            }
            other => panic!("expected str, got {:?}", other),
        }
    }

    #[test]
    fn is_empty_boundaries() {
        assert_eq!(
            builtin_str_is_empty(&[s("")]).unwrap(),
            NativeValue::Bool(true)
        );
        assert_eq!(
            builtin_str_is_empty(&[s("a")]).unwrap(),
            NativeValue::Bool(false)
        );
    }

    #[test]
    fn is_ascii_boundaries() {
        assert_eq!(
            builtin_str_is_ascii(&[s("hello")]).unwrap(),
            NativeValue::Bool(true)
        );
        assert_eq!(
            builtin_str_is_ascii(&[s("café")]).unwrap(),
            NativeValue::Bool(false)
        );
    }

    #[test]
    fn is_numeric_boundaries() {
        assert_eq!(
            builtin_str_is_numeric(&[s("123")]).unwrap(),
            NativeValue::Bool(true)
        );
        assert_eq!(
            builtin_str_is_numeric(&[s("12a")]).unwrap(),
            NativeValue::Bool(false)
        );
        assert_eq!(
            builtin_str_is_numeric(&[s("")]).unwrap(),
            NativeValue::Bool(false)
        );
    }

    #[test]
    fn find_some_and_none() {
        let r = builtin_str_find(&[s("hello"), s("ll")]).unwrap();
        assert_eq!(assert_some(r), NativeValue::Int(2));
        let r = builtin_str_find(&[s("hello"), s("zz")]).unwrap();
        assert_none(r);
    }

    #[test]
    fn find_all_indices() {
        let r = builtin_str_find_all(&[s("ababab"), s("ab")]).unwrap();
        match r {
            NativeValue::Array(v) => {
                assert_eq!(v.len(), 3);
                assert_eq!(v[0], NativeValue::Int(0));
                assert_eq!(v[1], NativeValue::Int(2));
                assert_eq!(v[2], NativeValue::Int(4));
            }
            other => panic!("expected array, got {:?}", other),
        }
    }

    #[test]
    fn chars_split_codepoints() {
        let r = builtin_str_chars(&[s("abc")]).unwrap();
        match r {
            NativeValue::Array(v) => {
                assert_eq!(v.len(), 3);
                assert_eq!(v[0], NativeValue::Str("a".to_string()));
                assert_eq!(v[1], NativeValue::Str("b".to_string()));
                assert_eq!(v[2], NativeValue::Str("c".to_string()));
            }
            other => panic!("expected array, got {:?}", other),
        }
    }

    #[test]
    fn join_concatenates_with_sep() {
        let parts = NativeValue::Array(vec![s("a"), s("b"), s("c")]);
        let r = builtin_str_join(&[parts, s("-")]).unwrap();
        assert_eq!(r, NativeValue::Str("a-b-c".to_string()));
    }

    #[test]
    fn repeat_basic() {
        let r = builtin_str_repeat(&[s("ab"), i(3)]).unwrap();
        assert_eq!(r, NativeValue::Str("ababab".to_string()));
    }
}
