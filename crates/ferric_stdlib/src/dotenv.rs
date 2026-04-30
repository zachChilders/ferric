//! # `dotenv` — Load `.env` files into a Map or the process environment.
//!
//! Hand-rolled parser. Supports:
//!   * `KEY=value` — bare value, trimmed of surrounding whitespace.
//!   * `KEY="value"` / `KEY='value'` — quoted; preserves whitespace and `=`.
//!     Double-quoted values support `\n`, `\t`, `\r`, `\\`, `\"` escapes.
//!     Single-quoted values are literal.
//!   * `# comment` — full-line and trailing comments outside quotes.
//!   * `export KEY=value` — `export ` prefix is stripped.
//!   * Blank lines.
//!
//! Doesn't support: variable interpolation (`${HOME}`), multi-line values,
//! shell-style heredocs. The 80% case for an `.env` file.

use std::collections::BTreeMap;
use std::path::PathBuf;

use ferric_common::Interner;

use crate::{MapKey, NativeRegistry, NativeValue};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn check_arg_count(args: &[NativeValue], expected: usize) -> Result<(), String> {
    if args.len() != expected {
        Err(format!(
            "expected {expected} argument(s), got {}",
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

fn err_result(message: impl Into<String>) -> NativeValue {
    NativeValue::Result(Box::new(Err(NativeValue::Str(message.into()))))
}

fn ok_result(v: NativeValue) -> NativeValue {
    NativeValue::Result(Box::new(Ok(v)))
}

fn map_from(entries: BTreeMap<MapKey, NativeValue>) -> NativeValue {
    NativeValue::Map(entries)
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

/// Parses dotenv-style content into key/value pairs. Last write wins on
/// duplicates.
pub fn parse_content(content: &str) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    for (lineno, raw) in content.lines().enumerate() {
        let line_num = lineno + 1;
        let line = raw.trim_start();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // Strip optional `export ` prefix.
        let line = line.strip_prefix("export ").unwrap_or(line).trim_start();

        // Find the `=`.
        let Some(eq) = line.find('=') else {
            return Err(format!("line {line_num}: missing '='"));
        };
        let key = line[..eq].trim();
        if key.is_empty() {
            return Err(format!("line {line_num}: empty key"));
        }
        if !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(format!(
                "line {line_num}: invalid key '{key}' (must match [A-Za-z0-9_]+)"
            ));
        }
        let rhs = line[eq + 1..].trim_start();
        let value = parse_rhs(rhs).map_err(|e| format!("line {line_num}: {e}"))?;
        out.insert(key.to_string(), value);
    }
    Ok(out)
}

fn parse_rhs(rhs: &str) -> Result<String, String> {
    let bytes = rhs.as_bytes();
    if bytes.is_empty() {
        return Ok(String::new());
    }
    match bytes[0] {
        b'"' => parse_double_quoted(&rhs[1..]),
        b'\'' => parse_single_quoted(&rhs[1..]),
        _ => {
            // Bare value: terminated by `#` (comment) or end of line.
            // Trim trailing whitespace.
            let end = rhs
                .find('#')
                .map(|i| {
                    // Only treat `#` as a comment if preceded by whitespace
                    // or at start; otherwise it's part of the value.
                    if i == 0 || rhs.as_bytes()[i - 1].is_ascii_whitespace() {
                        i
                    } else {
                        rhs.len()
                    }
                })
                .unwrap_or(rhs.len());
            Ok(rhs[..end].trim_end().to_string())
        }
    }
}

fn parse_double_quoted(rest: &str) -> Result<String, String> {
    let mut out = String::with_capacity(rest.len());
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Ok(out),
            '\\' => match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('\\') => out.push('\\'),
                Some('"') => out.push('"'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => return Err("unterminated escape inside double-quoted value".into()),
            },
            other => out.push(other),
        }
    }
    Err("unterminated double-quoted value".into())
}

fn parse_single_quoted(rest: &str) -> Result<String, String> {
    if let Some(end) = rest.find('\'') {
        Ok(rest[..end].to_string())
    } else {
        Err("unterminated single-quoted value".into())
    }
}

// ---------------------------------------------------------------------------
// Native bindings
// ---------------------------------------------------------------------------

fn map_from_pairs(entries: BTreeMap<String, String>) -> NativeValue {
    let mut m = BTreeMap::new();
    for (k, v) in entries {
        m.insert(MapKey::Str(k), NativeValue::Str(v));
    }
    map_from(m)
}

/// `dotenv::parse(content: Str) -> Result<Map<Str, Str>, DotenvError>`
fn builtin_dotenv_parse(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let content = expect_str(&args[0])?;
    match parse_content(content) {
        Ok(entries) => Ok(ok_result(map_from_pairs(entries))),
        Err(e) => Ok(err_result(format!("DotenvError::ParseError: {e}"))),
    }
}

/// `dotenv::load() -> Result<Map<Str, Str>, DotenvError>` — loads `./.env`.
fn builtin_dotenv_load(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 0)?;
    load_from_path(&PathBuf::from(".env"))
}

/// `dotenv::load_path(path: Str) -> Result<Map<Str, Str>, DotenvError>`.
fn builtin_dotenv_load_path(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let p = expect_str(&args[0])?;
    load_from_path(&PathBuf::from(p))
}

fn load_from_path(p: &PathBuf) -> Result<NativeValue, String> {
    match std::fs::read_to_string(p) {
        Ok(content) => match parse_content(&content) {
            Ok(entries) => Ok(ok_result(map_from_pairs(entries))),
            Err(e) => Ok(err_result(format!("DotenvError::ParseError: {e}"))),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(err_result(format!(
            "DotenvError::NotFound: {}",
            p.display()
        ))),
        Err(e) => Ok(err_result(format!("DotenvError::IoError: {e}"))),
    }
}

/// `dotenv::load_into_env() -> Result<Int, DotenvError>` — loads `./.env` and
/// sets each variable via `std::env::set_var`. Returns the number of vars
/// loaded. Existing process env vars are NOT overwritten (matches the common
/// dotenv convention).
fn builtin_dotenv_load_into_env(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 0)?;
    load_into_env_from_path(&PathBuf::from(".env"), false)
}

/// `dotenv::load_into_env_path(path: Str, override_: Bool) -> Result<Int, DotenvError>`
fn builtin_dotenv_load_into_env_path(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let p = expect_str(&args[0])?;
    let override_ = match &args[1] {
        NativeValue::Bool(b) => *b,
        other => return Err(format!("expected bool, got {other:?}")),
    };
    load_into_env_from_path(&PathBuf::from(p), override_)
}

fn load_into_env_from_path(p: &PathBuf, override_: bool) -> Result<NativeValue, String> {
    let content = match std::fs::read_to_string(p) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(err_result(format!(
                "DotenvError::NotFound: {}",
                p.display()
            )));
        }
        Err(e) => return Ok(err_result(format!("DotenvError::IoError: {e}"))),
    };
    let entries = match parse_content(&content) {
        Ok(e) => e,
        Err(e) => return Ok(err_result(format!("DotenvError::ParseError: {e}"))),
    };
    let mut count = 0i64;
    for (k, v) in entries {
        if !override_ && std::env::var_os(&k).is_some() {
            continue;
        }
        // SAFETY: set_var became unsafe in Rust 2024. The caller asked us to
        // mutate the process environment; this is the documented behaviour
        // of `dotenv::load_into_env`.
        unsafe { std::env::set_var(&k, &v) };
        count += 1;
    }
    Ok(ok_result(NativeValue::Int(count)))
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

pub fn register(registry: &mut NativeRegistry, interner: &mut Interner) {
    let bodies: &[(&str, crate::NativeFn)] = &[
        ("dotenv_parse", builtin_dotenv_parse),
        ("dotenv_load", builtin_dotenv_load),
        ("dotenv_load_path", builtin_dotenv_load_path),
        ("dotenv_load_into_env", builtin_dotenv_load_into_env),
        ("dotenv_load_into_env_path", builtin_dotenv_load_into_env_path),
    ];
    crate::register_module(
        registry,
        interner,
        ferric_stdlib_meta::dotenv::DOTENV_FNS,
        bodies,
    );
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_ok(s: &str) -> BTreeMap<String, String> {
        parse_content(s).expect("parse ok")
    }

    #[test]
    fn parse_basic_pairs() {
        let m = parse_ok("FOO=bar\nBAZ=qux");
        assert_eq!(m.get("FOO"), Some(&"bar".to_string()));
        assert_eq!(m.get("BAZ"), Some(&"qux".to_string()));
    }

    #[test]
    fn parse_blank_lines_and_comments() {
        let m = parse_ok("\n# top comment\nFOO=bar\n\n# trailing\n");
        assert_eq!(m.len(), 1);
        assert_eq!(m.get("FOO"), Some(&"bar".to_string()));
    }

    #[test]
    fn parse_export_prefix_stripped() {
        let m = parse_ok("export A=1\nexport B=2");
        assert_eq!(m.get("A"), Some(&"1".to_string()));
        assert_eq!(m.get("B"), Some(&"2".to_string()));
    }

    #[test]
    fn parse_double_quoted_with_escapes() {
        let m = parse_ok(r#"GREETING="hello\n\tworld""#);
        assert_eq!(m.get("GREETING"), Some(&"hello\n\tworld".to_string()));
    }

    #[test]
    fn parse_single_quoted_is_literal() {
        let m = parse_ok(r#"X='hello\n'"#);
        assert_eq!(m.get("X"), Some(&"hello\\n".to_string()));
    }

    #[test]
    fn parse_quoted_preserves_spaces() {
        let m = parse_ok(r#"S="  spaced  ""#);
        assert_eq!(m.get("S"), Some(&"  spaced  ".to_string()));
    }

    #[test]
    fn parse_bare_value_trims_trailing_whitespace() {
        let m = parse_ok("FOO=bar    ");
        assert_eq!(m.get("FOO"), Some(&"bar".to_string()));
    }

    #[test]
    fn parse_trailing_comment_on_bare_value() {
        let m = parse_ok("FOO=bar # comment");
        assert_eq!(m.get("FOO"), Some(&"bar".to_string()));
    }

    #[test]
    fn parse_hash_inside_value_kept_when_no_preceding_space() {
        let m = parse_ok("HASHED=ab#cd");
        assert_eq!(m.get("HASHED"), Some(&"ab#cd".to_string()));
    }

    #[test]
    fn parse_invalid_key() {
        assert!(parse_content("FOO-BAR=1").is_err());
    }

    #[test]
    fn parse_missing_eq() {
        assert!(parse_content("JUST_A_KEY").is_err());
    }

    #[test]
    fn parse_unterminated_double_quote() {
        assert!(parse_content(r#"FOO="bar"#).is_err());
    }

    #[test]
    fn parse_last_write_wins_on_duplicate() {
        let m = parse_ok("X=1\nX=2");
        assert_eq!(m.get("X"), Some(&"2".to_string()));
    }

    #[test]
    fn dotenv_parse_native_returns_ok_map() {
        let r = builtin_dotenv_parse(&[NativeValue::Str("FOO=bar\nBAZ=qux".into())]).unwrap();
        let inner = match r {
            NativeValue::Result(b) => match *b {
                Ok(v) => v,
                Err(e) => panic!("expected Ok, got Err({e:?})"),
            },
            other => panic!("expected Result, got {other:?}"),
        };
        match inner {
            NativeValue::Map(m) => {
                assert_eq!(
                    m.get(&MapKey::Str("FOO".into())),
                    Some(&NativeValue::Str("bar".into()))
                );
                assert_eq!(
                    m.get(&MapKey::Str("BAZ".into())),
                    Some(&NativeValue::Str("qux".into()))
                );
            }
            other => panic!("expected Map, got {other:?}"),
        }
    }

    #[test]
    fn dotenv_parse_native_surfaces_error() {
        let r = builtin_dotenv_parse(&[NativeValue::Str("BAD-KEY=1".into())]).unwrap();
        match r {
            NativeValue::Result(b) => match *b {
                Err(NativeValue::Str(s)) => assert!(s.contains("DotenvError::ParseError")),
                _ => panic!(),
            },
            _ => panic!(),
        }
    }

    #[test]
    fn dotenv_load_path_missing_file() {
        let r =
            builtin_dotenv_load_path(&[NativeValue::Str("/definitely/does/not/exist/.env".into())])
                .unwrap();
        match r {
            NativeValue::Result(b) => match *b {
                Err(NativeValue::Str(s)) => assert!(s.contains("DotenvError::NotFound")),
                _ => panic!(),
            },
            _ => panic!(),
        }
    }

    #[test]
    fn dotenv_load_path_reads_real_file() {
        let dir = std::env::temp_dir().join(format!(
            "ferric_dotenv_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(".env");
        std::fs::write(&path, "ALPHA=one\nBETA=two\n").unwrap();

        let r = builtin_dotenv_load_path(&[NativeValue::Str(path.to_string_lossy().into_owned())])
            .unwrap();
        match r {
            NativeValue::Result(b) => match *b {
                Ok(NativeValue::Map(m)) => {
                    assert_eq!(
                        m.get(&MapKey::Str("ALPHA".into())),
                        Some(&NativeValue::Str("one".into()))
                    );
                    assert_eq!(
                        m.get(&MapKey::Str("BETA".into())),
                        Some(&NativeValue::Str("two".into()))
                    );
                }
                _ => panic!("expected Ok(Map)"),
            },
            _ => panic!(),
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dotenv_load_into_env_sets_unset_keys_only() {
        let dir = std::env::temp_dir().join(format!(
            "ferric_dotenv_into_env_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(".env");
        let key_new = format!("FERRIC_DOTENV_TEST_NEW_{}", std::process::id());
        let key_set = format!("FERRIC_DOTENV_TEST_SET_{}", std::process::id());
        std::fs::write(&path, format!("{key_new}=newval\n{key_set}=fromenv\n")).unwrap();

        // Pre-set one key in the process environment.
        unsafe {
            std::env::set_var(&key_set, "preserved");
        }

        let r = builtin_dotenv_load_into_env_path(&[
            NativeValue::Str(path.to_string_lossy().into_owned()),
            NativeValue::Bool(false),
        ])
        .unwrap();
        match r {
            NativeValue::Result(b) => match *b {
                Ok(NativeValue::Int(n)) => {
                    assert_eq!(n, 1, "should have loaded only the unset key")
                }
                _ => panic!("expected Ok(Int)"),
            },
            _ => panic!(),
        }
        assert_eq!(std::env::var(&key_new).ok(), Some("newval".to_string()));
        assert_eq!(std::env::var(&key_set).ok(), Some("preserved".to_string()));

        // Now override.
        let r = builtin_dotenv_load_into_env_path(&[
            NativeValue::Str(path.to_string_lossy().into_owned()),
            NativeValue::Bool(true),
        ])
        .unwrap();
        match r {
            NativeValue::Result(b) => match *b {
                Ok(NativeValue::Int(n)) => assert_eq!(n, 2),
                _ => panic!(),
            },
            _ => panic!(),
        }
        assert_eq!(std::env::var(&key_set).ok(), Some("fromenv".to_string()));

        // Cleanup
        unsafe {
            std::env::remove_var(&key_new);
            std::env::remove_var(&key_set);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn register_installs_every_function() {
        let mut registry = NativeRegistry::new();
        let mut interner = Interner::new();
        register(&mut registry, &mut interner);
        for name in [
            "dotenv_parse",
            "dotenv_load",
            "dotenv_load_path",
            "dotenv_load_into_env",
            "dotenv_load_into_env_path",
        ] {
            let sym = interner.intern(name);
            assert!(registry.get(sym).is_some(), "missing: {name}");
        }
    }
}
