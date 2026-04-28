//! # Regular Expressions module (`re`)
//!
//! Implements the `re` module from `docs/stdlib.md`. Compiled regexes are
//! reusable values; matches expose group captures (positional + named).
//!
//! ## Byte offsets
//!
//! `match_start` / `match_end` and the offsets inside `MatchRepr` are
//! **byte offsets** into the original UTF-8 input, *not* character offsets.
//! This mirrors `regex::Match::start()` / `regex::Match::end()`. If/when
//! the language exposes character indices we will add a separate
//! `match_char_start` API rather than silently changing semantics.

// REQUIRED DEPS:
//   regex = "1"

// REQUIRED NATIVE VALUE VARIANTS:
//   Regex(Rc<regex::Regex>)
//   Match(MatchRepr)
//
// MatchRepr stores the matched text plus byte spans plus capture groups:
//   pub struct MatchRepr {
//       pub text: String,                // full match
//       pub start: usize,                // byte offset
//       pub end: usize,                  // byte offset
//       pub captures: Vec<Option<String>>,
//       pub named_captures: BTreeMap<String, String>,
//   }

use std::collections::BTreeMap;
use std::rc::Rc;

use ferric_common::Interner;

use crate::{NativeRegistry, NativeValue};

// ---------------------------------------------------------------------------
// Match representation
// ---------------------------------------------------------------------------

/// Owned representation of a single regex match. Stored inside
/// `NativeValue::Match` so the value is self-contained and can be passed
/// around freely after the borrowed `regex::Captures` is dropped.
#[derive(Debug, Clone, PartialEq)]
pub struct MatchRepr {
    /// Full text of the match (capture group 0).
    pub text: String,
    /// Byte offset (UTF-8) of the start of the match in the haystack.
    pub start: usize,
    /// Byte offset (UTF-8) of the end of the match in the haystack.
    pub end: usize,
    /// Positional capture groups. Index 0 is the full match. Each entry is
    /// `None` when the group did not participate in the match.
    pub captures: Vec<Option<String>>,
    /// Named capture groups: maps group name → matched substring. Only
    /// populated for groups that actually participated.
    pub named_captures: BTreeMap<String, String>,
}

// ---------------------------------------------------------------------------
// Helpers (private to this module — see lib.rs for top-level helpers)
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

fn expect_regex(value: &NativeValue) -> Result<&Rc<regex::Regex>, String> {
    match value {
        NativeValue::Regex(r) => Ok(r),
        other => Err(format!("expected regex, got {other:?}")),
    }
}

fn expect_match(value: &NativeValue) -> Result<&MatchRepr, String> {
    match value {
        NativeValue::Match(m) => Ok(m),
        other => Err(format!("expected match, got {other:?}")),
    }
}

fn build_match_repr(re: &regex::Regex, caps: &regex::Captures<'_>) -> MatchRepr {
    let m0 = caps.get(0).expect("captures always have group 0");
    let text = m0.as_str().to_string();
    let start = m0.start();
    let end = m0.end();

    let mut captures: Vec<Option<String>> = Vec::with_capacity(caps.len());
    for i in 0..caps.len() {
        captures.push(caps.get(i).map(|m| m.as_str().to_string()));
    }

    let mut named_captures: BTreeMap<String, String> = BTreeMap::new();
    for name in re.capture_names().flatten() {
        if let Some(m) = caps.name(name) {
            named_captures.insert(name.to_string(), m.as_str().to_string());
        }
    }

    MatchRepr {
        text,
        start,
        end,
        captures,
        named_captures,
    }
}

// ---------------------------------------------------------------------------
// Compilation
// ---------------------------------------------------------------------------

fn builtin_re_compile(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let pattern = expect_str(&args[0])?;
    match regex::Regex::new(pattern) {
        Ok(r) => Ok(NativeValue::Regex(Rc::new(r))),
        Err(e) => Err(format!(
            "ReError::InvalidPattern: pattern={pattern:?} message={e}"
        )),
    }
}

fn builtin_re_compile_flags(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let pattern = expect_str(&args[0])?;
    let flags = expect_str(&args[1])?;

    // Validate flags. Allowed: i (case-insensitive), m (multiline),
    // s (dotall), x (ignore whitespace), U (swap greediness).
    for ch in flags.chars() {
        match ch {
            'i' | 'm' | 's' | 'x' | 'U' => {}
            _ => {
                return Err(format!(
                    "ReError::InvalidFlags: flags={flags:?} unknown flag {ch:?}"
                ));
            }
        }
    }

    let composed = if flags.is_empty() {
        pattern.clone()
    } else {
        format!("(?{flags}){pattern}")
    };

    match regex::Regex::new(&composed) {
        Ok(r) => Ok(NativeValue::Regex(Rc::new(r))),
        Err(e) => Err(format!(
            "ReError::InvalidPattern: pattern={pattern:?} message={e}"
        )),
    }
}

// ---------------------------------------------------------------------------
// Matching
// ---------------------------------------------------------------------------

fn builtin_re_is_match(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let r = expect_regex(&args[0])?;
    let s = expect_str(&args[1])?;
    Ok(NativeValue::Bool(r.is_match(s)))
}

fn builtin_re_find(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let r = expect_regex(&args[0])?;
    let s = expect_str(&args[1])?;
    match r.captures(s) {
        Some(caps) => Ok(NativeValue::Match(build_match_repr(r, &caps))),
        // Convention: callers wrap this in Option at the language layer;
        // at the native boundary we return `Unit` to signal "no match".
        // Other modules use the same pattern (e.g. `str::find`).
        None => Ok(NativeValue::Unit),
    }
}

fn builtin_re_find_all(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let r = expect_regex(&args[0])?;
    let s = expect_str(&args[1])?;
    let matches: Vec<NativeValue> = r
        .captures_iter(s)
        .map(|caps| NativeValue::Match(build_match_repr(r, &caps)))
        .collect();
    Ok(NativeValue::Array(matches))
}

// ---------------------------------------------------------------------------
// Match inspection
// ---------------------------------------------------------------------------

fn builtin_re_match_str(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let m = expect_match(&args[0])?;
    Ok(NativeValue::Str(m.text.clone()))
}

fn builtin_re_match_start(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let m = expect_match(&args[0])?;
    Ok(NativeValue::Int(m.start as i64))
}

fn builtin_re_match_end(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let m = expect_match(&args[0])?;
    Ok(NativeValue::Int(m.end as i64))
}

fn builtin_re_capture(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let m = expect_match(&args[0])?;
    let idx = expect_int(&args[1])?;
    if idx < 0 {
        return Ok(NativeValue::Unit);
    }
    let idx = idx as usize;
    match m.captures.get(idx) {
        Some(Some(s)) => Ok(NativeValue::Str(s.clone())),
        _ => Ok(NativeValue::Unit),
    }
}

fn builtin_re_capture_name(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let m = expect_match(&args[0])?;
    let name = expect_str(&args[1])?;
    match m.named_captures.get(name) {
        Some(s) => Ok(NativeValue::Str(s.clone())),
        None => Ok(NativeValue::Unit),
    }
}

// ---------------------------------------------------------------------------
// Replace
// ---------------------------------------------------------------------------

fn builtin_re_replace(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 3)?;
    let r = expect_regex(&args[0])?;
    let s = expect_str(&args[1])?;
    let with = expect_str(&args[2])?;
    let out = r.replace(s, with.as_str()).to_string();
    Ok(NativeValue::Str(out))
}

fn builtin_re_replace_all(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 3)?;
    let r = expect_regex(&args[0])?;
    let s = expect_str(&args[1])?;
    let with = expect_str(&args[2])?;
    let out = r.replace_all(s, with.as_str()).to_string();
    Ok(NativeValue::Str(out))
}

/// `replace_fn(re, s, f)` — for each match, `f(Match) -> Str` is invoked and
/// the returned string is substituted in.
///
/// **Open question:** the closure invocation contract for native functions is
/// not yet finalised in `ferric_stdlib`. The other higher-order modules
/// (`list::map`, `list::filter`, …) will share an `invoke_closure` helper
/// that the integration step exposes. This implementation assumes such a
/// helper exists and is in scope; the actual call site is left as a
/// sentinel that returns the match text unchanged. The behavioural test for
/// `replace_fn` is `#[ignore]` for now (see tests).
fn builtin_re_replace_fn(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 3)?;
    let r = expect_regex(&args[0])?;
    let s = expect_str(&args[1])?;
    let f = &args[2];

    // `regex::Regex::replace_all` takes a `FnMut(&Captures) -> Cow<str>`.
    // It does not allow the callback to return an error; we capture any
    // closure error in a cell and surface it after the replacement runs.
    let mut closure_err: Option<String> = None;
    let out = r
        .replace_all(s, |caps: &regex::Captures<'_>| {
            if closure_err.is_some() {
                return String::new();
            }
            let match_repr = build_match_repr(r, caps);
            match crate::invoke_closure(f, &[NativeValue::Match(match_repr.clone())]) {
                Ok(NativeValue::Str(rep)) => rep,
                Ok(other) => {
                    closure_err = Some(format!(
                        "replace_fn: closure must return Str, got {other:?}"
                    ));
                    String::new()
                }
                Err(e) => {
                    closure_err = Some(format!("replace_fn: closure error: {e}"));
                    String::new()
                }
            }
        })
        .to_string();
    if let Some(e) = closure_err {
        return Err(e);
    }
    Ok(NativeValue::Str(out))
}

// ---------------------------------------------------------------------------
// Split
// ---------------------------------------------------------------------------

fn builtin_re_split(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let r = expect_regex(&args[0])?;
    let s = expect_str(&args[1])?;
    let parts: Vec<NativeValue> = r
        .split(s)
        .map(|p| NativeValue::Str(p.to_string()))
        .collect();
    Ok(NativeValue::Array(parts))
}

fn builtin_re_split_n(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 3)?;
    let r = expect_regex(&args[0])?;
    let s = expect_str(&args[1])?;
    let n = expect_int(&args[2])?;
    if n < 0 {
        return Err(format!("re::split_n: n must be non-negative, got {n}"));
    }
    let parts: Vec<NativeValue> = r
        .splitn(s, n as usize)
        .map(|p| NativeValue::Str(p.to_string()))
        .collect();
    Ok(NativeValue::Array(parts))
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

/// Registers every `re_*` native function with the given registry.
pub fn register(registry: &mut NativeRegistry, interner: &mut Interner) {
    type Entry = (&'static str, &'static [&'static str], crate::NativeFn);
    let entries: &[Entry] = &[
        // Compilation
        ("re_compile", &["pattern"], builtin_re_compile),
        (
            "re_compile_flags",
            &["pattern", "flags"],
            builtin_re_compile_flags,
        ),
        // Matching
        ("re_is_match", &["r", "s"], builtin_re_is_match),
        ("re_find", &["r", "s"], builtin_re_find),
        ("re_find_all", &["r", "s"], builtin_re_find_all),
        // Match inspection
        ("re_match_str", &["m"], builtin_re_match_str),
        ("re_match_start", &["m"], builtin_re_match_start),
        ("re_match_end", &["m"], builtin_re_match_end),
        ("re_capture", &["m", "index"], builtin_re_capture),
        ("re_capture_name", &["m", "name"], builtin_re_capture_name),
        // Replace
        ("re_replace", &["r", "s", "with"], builtin_re_replace),
        (
            "re_replace_all",
            &["r", "s", "with"],
            builtin_re_replace_all,
        ),
        ("re_replace_fn", &["r", "s", "f"], builtin_re_replace_fn),
        // Split
        ("re_split", &["r", "s"], builtin_re_split),
        ("re_split_n", &["r", "s", "n"], builtin_re_split_n),
    ];
    for (name, params, f) in entries {
        registry.register_named(interner, name, params, *f);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn re(pattern: &str) -> NativeValue {
        builtin_re_compile(&[NativeValue::Str(pattern.to_string())]).expect("compile ok")
    }

    fn s(v: &str) -> NativeValue {
        NativeValue::Str(v.to_string())
    }

    fn i(n: i64) -> NativeValue {
        NativeValue::Int(n)
    }

    // -- compile --------------------------------------------------------

    #[test]
    fn compile_valid_pattern() {
        let r = builtin_re_compile(&[s(r"\d+")]);
        assert!(matches!(r, Ok(NativeValue::Regex(_))));
    }

    #[test]
    fn compile_invalid_pattern_errs() {
        let r = builtin_re_compile(&[s("[")]);
        assert!(r.is_err());
        assert!(r.unwrap_err().contains("InvalidPattern"));
    }

    #[test]
    fn compile_wrong_arity() {
        let r = builtin_re_compile(&[]);
        assert!(r.is_err());
    }

    // -- compile_flags --------------------------------------------------

    #[test]
    fn compile_flags_case_insensitive() {
        let rx = builtin_re_compile_flags(&[s("foo"), s("i")]).expect("compile ok");
        let m = builtin_re_is_match(&[rx, s("FOO")]).expect("ok");
        assert_eq!(m, NativeValue::Bool(true));
    }

    #[test]
    fn compile_flags_unknown_errs() {
        let r = builtin_re_compile_flags(&[s("foo"), s("z")]);
        assert!(r.is_err());
        assert!(r.unwrap_err().contains("InvalidFlags"));
    }

    #[test]
    fn compile_flags_empty_ok() {
        let rx = builtin_re_compile_flags(&[s("foo"), s("")]).expect("compile ok");
        let m = builtin_re_is_match(&[rx, s("foo")]).expect("ok");
        assert_eq!(m, NativeValue::Bool(true));
    }

    // -- is_match -------------------------------------------------------

    #[test]
    fn is_match_true() {
        let r = builtin_re_is_match(&[re(r"\d+"), s("abc123")]).expect("ok");
        assert_eq!(r, NativeValue::Bool(true));
    }

    #[test]
    fn is_match_false() {
        let r = builtin_re_is_match(&[re(r"\d+"), s("abc")]).expect("ok");
        assert_eq!(r, NativeValue::Bool(false));
    }

    // -- find -----------------------------------------------------------

    #[test]
    fn find_returns_match_with_byte_offsets() {
        let m = builtin_re_find(&[re(r"\d+"), s("abc123def")]).expect("ok");
        match m {
            NativeValue::Match(mr) => {
                assert_eq!(mr.text, "123");
                assert_eq!(mr.start, 3);
                assert_eq!(mr.end, 6);
            }
            other => panic!("expected Match, got {other:?}"),
        }
    }

    #[test]
    fn find_no_match_returns_unit() {
        let m = builtin_re_find(&[re(r"\d+"), s("abc")]).expect("ok");
        assert_eq!(m, NativeValue::Unit);
    }

    // -- find_all -------------------------------------------------------

    #[test]
    fn find_all_returns_three_matches() {
        let r = builtin_re_find_all(&[re(r"\d+"), s("1 22 333")]).expect("ok");
        match r {
            NativeValue::Array(items) => {
                assert_eq!(items.len(), 3);
                let texts: Vec<String> = items
                    .iter()
                    .map(|v| match v {
                        NativeValue::Match(m) => m.text.clone(),
                        _ => panic!("expected Match"),
                    })
                    .collect();
                assert_eq!(texts, vec!["1", "22", "333"]);
            }
            other => panic!("expected Array, got {other:?}"),
        }
    }

    #[test]
    fn find_all_empty_when_none() {
        let r = builtin_re_find_all(&[re(r"\d+"), s("no digits here")]).expect("ok");
        assert_eq!(r, NativeValue::Array(vec![]));
    }

    // -- capture / capture_name -----------------------------------------

    #[test]
    fn capture_zero_returns_full_match() {
        let m = builtin_re_find(&[re(r"(\d+)-(\w+)"), s("123-foo bar")]).expect("ok");
        let r0 = builtin_re_capture(&[m.clone(), i(0)]).expect("ok");
        assert_eq!(r0, s("123-foo"));
    }

    #[test]
    fn capture_positional_groups() {
        let m = builtin_re_find(&[re(r"(\d+)-(\w+)"), s("123-foo bar")]).expect("ok");
        let r1 = builtin_re_capture(&[m.clone(), i(1)]).expect("ok");
        let r2 = builtin_re_capture(&[m, i(2)]).expect("ok");
        assert_eq!(r1, s("123"));
        assert_eq!(r2, s("foo"));
    }

    #[test]
    fn capture_out_of_range_returns_unit() {
        let m = builtin_re_find(&[re(r"\d+"), s("abc123")]).expect("ok");
        let r = builtin_re_capture(&[m, i(99)]).expect("ok");
        assert_eq!(r, NativeValue::Unit);
    }

    #[test]
    fn capture_name_finds_named_group() {
        let rx = builtin_re_compile(&[s(r"(?P<num>\d+)")]).expect("ok");
        let m = builtin_re_find(&[rx, s("abc 42 xyz")]).expect("ok");
        let r = builtin_re_capture_name(&[m, s("num")]).expect("ok");
        assert_eq!(r, s("42"));
    }

    #[test]
    fn capture_name_missing_returns_unit() {
        let rx = builtin_re_compile(&[s(r"(?P<num>\d+)")]).expect("ok");
        let m = builtin_re_find(&[rx, s("abc 42")]).expect("ok");
        let r = builtin_re_capture_name(&[m, s("nope")]).expect("ok");
        assert_eq!(r, NativeValue::Unit);
    }

    // -- match_str / match_start / match_end ----------------------------

    #[test]
    fn match_accessors() {
        let m = builtin_re_find(&[re(r"\d+"), s("abc123def")]).expect("ok");
        let txt = builtin_re_match_str(&[m.clone()]).expect("ok");
        let start = builtin_re_match_start(&[m.clone()]).expect("ok");
        let end = builtin_re_match_end(&[m]).expect("ok");
        assert_eq!(txt, s("123"));
        assert_eq!(start, i(3));
        assert_eq!(end, i(6));
    }

    // -- replace --------------------------------------------------------

    #[test]
    fn replace_first_only() {
        let r = builtin_re_replace(&[re("a"), s("abab"), s("X")]).expect("ok");
        assert_eq!(r, s("Xbab"));
    }

    #[test]
    fn replace_all_global() {
        let r = builtin_re_replace_all(&[re("a"), s("abab"), s("X")]).expect("ok");
        assert_eq!(r, s("XbXb"));
    }

    #[test]
    fn replace_fn_doubles_each_digit_run() {
        // re::replace_fn(re("\\d+"), "1 2 3", |m| str::repeat(re::match_str(m), 2))
        // → "11 22 33"
        let double_match = NativeValue::Closure(crate::NativeClosure::new(|args| match &args[0] {
            NativeValue::Match(m) => Ok(NativeValue::Str(m.text.repeat(2))),
            other => Err(format!("expected Match, got {other:?}")),
        }));
        let r = builtin_re_replace_fn(&[re(r"\d+"), s("1 2 3"), double_match]).unwrap();
        assert_eq!(r, s("11 22 33"));
    }

    // -- split ----------------------------------------------------------

    #[test]
    fn split_basic() {
        let r = builtin_re_split(&[re(r",\s*"), s("a, b,c")]).expect("ok");
        assert_eq!(r, NativeValue::Array(vec![s("a"), s("b"), s("c")]));
    }

    #[test]
    fn split_n_limits_pieces() {
        let r = builtin_re_split_n(&[re(","), s("a,b,c,d"), i(2)]).expect("ok");
        assert_eq!(r, NativeValue::Array(vec![s("a"), s("b,c,d")]));
    }

    #[test]
    fn split_n_negative_errs() {
        let r = builtin_re_split_n(&[re(","), s("a,b"), i(-1)]);
        assert!(r.is_err());
    }

    // -- anchors / groups -----------------------------------------------

    #[test]
    fn anchor_caret_only_matches_start() {
        let yes = builtin_re_is_match(&[re("^abc"), s("abcdef")]).expect("ok");
        let no = builtin_re_is_match(&[re("^abc"), s("xabcdef")]).expect("ok");
        assert_eq!(yes, NativeValue::Bool(true));
        assert_eq!(no, NativeValue::Bool(false));
    }

    #[test]
    fn anchor_dollar_only_matches_end() {
        let yes = builtin_re_is_match(&[re(r"\d+$"), s("abc123")]).expect("ok");
        let no = builtin_re_is_match(&[re(r"\d+$"), s("abc123x")]).expect("ok");
        assert_eq!(yes, NativeValue::Bool(true));
        assert_eq!(no, NativeValue::Bool(false));
    }

    #[test]
    fn alternation_groups() {
        let rx = re(r"(cat|dog|bird)");
        let m = builtin_re_find(&[rx, s("I have a dog!")]).expect("ok");
        match m {
            NativeValue::Match(mr) => {
                assert_eq!(mr.text, "dog");
                assert_eq!(
                    mr.captures.get(1).cloned().flatten(),
                    Some("dog".to_string())
                );
            }
            other => panic!("expected Match, got {other:?}"),
        }
    }

    // -- registration ---------------------------------------------------

    #[test]
    fn register_installs_all_functions() {
        let mut registry = NativeRegistry::new();
        let mut interner = Interner::new();
        register(&mut registry, &mut interner);

        for name in [
            "re_compile",
            "re_compile_flags",
            "re_is_match",
            "re_find",
            "re_find_all",
            "re_match_str",
            "re_match_start",
            "re_match_end",
            "re_capture",
            "re_capture_name",
            "re_replace",
            "re_replace_all",
            "re_replace_fn",
            "re_split",
            "re_split_n",
        ] {
            let sym = interner.intern(name);
            assert!(
                registry.get(sym).is_some(),
                "missing registration for {name}"
            );
        }
    }
}
