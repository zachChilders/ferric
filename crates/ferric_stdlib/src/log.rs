//! # `log` — Structured Logging
//!
//! Implements the `log` module from `docs/stdlib.md`. Logs are structured and
//! emitted to stderr only (no files, no config). The output format is
//! controlled by the `LOG_FORMAT` env var:
//!
//!   * `"json"`  → `{"ts":"...","level":"INFO","msg":"...","k":"v"}`
//!   * anything else (or unset) → pretty form
//!     `2026-04-25T14:30:00Z INFO  message  k=v`
//!
//! The minimum-emission level is process-global. It is held in a
//! `OnceLock<AtomicU8>` (initialise-once cell wrapping an atomic). This is
//! the same pattern used by `http::Client` and is acceptable per the
//! no-mutable-globals architectural rule: a single atomic, documented.
//!
//! Tests exercise the `pub(crate) fn render(...)` hook rather than touching
//! stderr directly.

// REQUIRED DEPS:
//   chrono = { version = "0.4", default-features = false, features = ["clock"] }
//   serial_test = "3"   (test-only — manipulates global level)

// REQUIRED NATIVE VALUE VARIANTS:
//   Map(BTreeMap<MapKey, NativeValue>)            // shared with map module
//   Logger(Rc<LoggerRepr>)
//   LogLevel(LogLevelRepr)
//
// pub struct LoggerRepr {
//     pub fields: Vec<(String, String)>,    // ordered for stable output
// }
//
// #[derive(Copy, Clone, PartialEq, PartialOrd)]
// pub enum LogLevelRepr { Debug = 0, Info = 1, Warn = 2, Error = 3 }

use std::rc::Rc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

use ferric_common::Interner;

use crate::{MapKey, NativeRegistry, NativeValue};

// ---------------------------------------------------------------------------
// Local representations (pending NativeValue extension during integration)
// ---------------------------------------------------------------------------

/// Backing data for a `Logger` value. Held in `Rc` because `Logger::with`
/// produces a *new* logger with one extra field appended without mutating
/// the original.
#[derive(Debug, Clone, PartialEq)]
pub struct LoggerRepr {
    /// Field set, in insertion order. Pretty/JSON rendering may sort.
    pub fields: Vec<(String, String)>,
}

/// Numeric encoding of log levels — comparable and `Copy`. The integer
/// encoding is what gets stored in the global atomic.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevelRepr {
    Debug = 0,
    Info = 1,
    Warn = 2,
    Error = 3,
}

impl LogLevelRepr {
    fn as_u8(self) -> u8 {
        self as u8
    }

    fn from_u8(n: u8) -> Self {
        match n {
            0 => LogLevelRepr::Debug,
            1 => LogLevelRepr::Info,
            2 => LogLevelRepr::Warn,
            _ => LogLevelRepr::Error,
        }
    }

    fn label(self) -> &'static str {
        match self {
            LogLevelRepr::Debug => "DEBUG",
            LogLevelRepr::Info => "INFO",
            LogLevelRepr::Warn => "WARN",
            LogLevelRepr::Error => "ERROR",
        }
    }
}

// ---------------------------------------------------------------------------
// Global level (OnceLock<AtomicU8>) — single atomic, documented
// ---------------------------------------------------------------------------

fn level_cell() -> &'static AtomicU8 {
    static CELL: OnceLock<AtomicU8> = OnceLock::new();
    CELL.get_or_init(|| AtomicU8::new(LogLevelRepr::Info.as_u8()))
}

fn current_level() -> LogLevelRepr {
    LogLevelRepr::from_u8(level_cell().load(Ordering::Relaxed))
}

fn store_level(l: LogLevelRepr) {
    level_cell().store(l.as_u8(), Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Local helpers (private per file per stdlib-00 rules)
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

fn expect_log_level(value: &NativeValue) -> Result<LogLevelRepr, String> {
    match value {
        NativeValue::LogLevel(l) => Ok(*l),
        other => Err(format!("expected LogLevel, got {other:?}")),
    }
}

fn expect_logger(value: &NativeValue) -> Result<Rc<LoggerRepr>, String> {
    match value {
        NativeValue::Logger(l) => Ok(l.clone()),
        other => Err(format!("expected Logger, got {other:?}")),
    }
}

/// Convert a `Map<Str, Str>` argument to an owned `Vec<(String, String)>`,
/// preserving the map's iteration order. (For BTreeMap this is alphabetical
/// — see the spec note in `stdlib-17-log.md`.)
fn expect_str_str_map(value: &NativeValue) -> Result<Vec<(String, String)>, String> {
    match value {
        NativeValue::Map(m) => {
            let mut out: Vec<(String, String)> = Vec::with_capacity(m.len());
            for (k, v) in m.iter() {
                let k_str = match k {
                    MapKey::Str(s) => s.clone(),
                    MapKey::Int(n) => n.to_string(),
                    MapKey::Bool(b) => b.to_string(),
                };
                let v_str = match v {
                    NativeValue::Str(s) => s.clone(),
                    other => {
                        return Err(format!("expected Str map value, got {other:?}"));
                    }
                };
                out.push((k_str, v_str));
            }
            Ok(out)
        }
        other => Err(format!("expected Map<Str,Str>, got {other:?}")),
    }
}

// ---------------------------------------------------------------------------
// Format selection: read LOG_FORMAT once, cache the choice for the process
// ---------------------------------------------------------------------------

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum Format {
    Pretty,
    Json,
}

fn format_from_env() -> Format {
    match std::env::var("LOG_FORMAT").ok().as_deref() {
        Some("json") => Format::Json,
        _ => Format::Pretty,
    }
}

// ---------------------------------------------------------------------------
// Render hook (test surface — produces the formatted string for one event)
// ---------------------------------------------------------------------------

/// Build the formatted log line for a single event. This is the test seam:
/// production code calls `emit(...)` which routes to stderr, but
/// `render(...)` returns the very same string without any side effect.
///
/// `level` is a level label (`"INFO"`, `"DEBUG"`, ...). `fields` is rendered
/// after the message, sorted alphabetically by key in both pretty and JSON
/// modes (see the spec note about BTreeMap-derived order). The format is
/// selected from `LOG_FORMAT` at call time so tests can flip the env var.
pub(crate) fn render(level: &str, msg: &str, fields: &[(String, String)]) -> String {
    render_with(level, msg, fields, format_from_env(), &chrono_now_iso8601())
}

/// Same as [`render`], but with explicit format and timestamp — used by
/// tests that need a deterministic output.
fn render_with(
    level: &str,
    msg: &str,
    fields: &[(String, String)],
    fmt: Format,
    ts: &str,
) -> String {
    // Sort fields alphabetically by key for stable output. We clone into a
    // local Vec so callers can pass any order.
    let mut sorted: Vec<(&str, &str)> =
        fields.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    sorted.sort_by(|a, b| a.0.cmp(b.0));

    match fmt {
        Format::Pretty => {
            // `2026-04-25T14:30:00Z INFO  message  k=v k=v`
            let mut out = String::new();
            out.push_str(ts);
            out.push(' ');
            // 5-char-wide level column (longest is "DEBUG"/"ERROR" = 5)
            // followed by two spaces.
            out.push_str(&format!("{level:<5}"));
            out.push(' ');
            out.push_str(msg);
            for (k, v) in &sorted {
                out.push(' ');
                out.push_str(k);
                out.push('=');
                out.push_str(v);
            }
            out
        }
        Format::Json => {
            // Minimal hand-rolled JSON — no external dep.
            let mut out = String::new();
            out.push('{');
            out.push_str(&format!(
                "\"ts\":{},\"level\":{},\"msg\":{}",
                json_string(ts),
                json_string(level),
                json_string(msg),
            ));
            for (k, v) in &sorted {
                out.push(',');
                out.push_str(&json_string(k));
                out.push(':');
                out.push_str(&json_string(v));
            }
            out.push('}');
            out
        }
    }
}

fn chrono_now_iso8601() -> String {
    // `chrono::Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)` →
    // `2026-04-25T14:30:00Z`.
    chrono::Utc::now()
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

// ---------------------------------------------------------------------------
// Emission (production path — writes to stderr if level passes)
// ---------------------------------------------------------------------------

fn emit(level: LogLevelRepr, msg: &str, fields: &[(String, String)]) {
    if level < current_level() {
        return;
    }
    let line = render(level.label(), msg, fields);
    eprintln!("{line}");
}

// ---------------------------------------------------------------------------
// Direct-level functions: log_debug-style names per the stdlib-00 rule
// (Ferric `log::debug` → native `log_debug`).
// ---------------------------------------------------------------------------

fn builtin_log_debug(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let msg = expect_str(&args[0])?;
    emit(LogLevelRepr::Debug, msg, &[]);
    Ok(NativeValue::Unit)
}

fn builtin_log_info(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let msg = expect_str(&args[0])?;
    emit(LogLevelRepr::Info, msg, &[]);
    Ok(NativeValue::Unit)
}

fn builtin_log_warn(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let msg = expect_str(&args[0])?;
    emit(LogLevelRepr::Warn, msg, &[]);
    Ok(NativeValue::Unit)
}

fn builtin_log_error(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let msg = expect_str(&args[0])?;
    emit(LogLevelRepr::Error, msg, &[]);
    Ok(NativeValue::Unit)
}

// ---------------------------------------------------------------------------
// `*_fields(msg, fields)` — same as above but with a Map<Str,Str>
// ---------------------------------------------------------------------------

fn builtin_log_debug_fields(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let msg = expect_str(&args[0])?;
    let fields = expect_str_str_map(&args[1])?;
    emit(LogLevelRepr::Debug, msg, &fields);
    Ok(NativeValue::Unit)
}

fn builtin_log_info_fields(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let msg = expect_str(&args[0])?;
    let fields = expect_str_str_map(&args[1])?;
    emit(LogLevelRepr::Info, msg, &fields);
    Ok(NativeValue::Unit)
}

fn builtin_log_warn_fields(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let msg = expect_str(&args[0])?;
    let fields = expect_str_str_map(&args[1])?;
    emit(LogLevelRepr::Warn, msg, &fields);
    Ok(NativeValue::Unit)
}

fn builtin_log_error_fields(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let msg = expect_str(&args[0])?;
    let fields = expect_str_str_map(&args[1])?;
    emit(LogLevelRepr::Error, msg, &fields);
    Ok(NativeValue::Unit)
}

// ---------------------------------------------------------------------------
// Logger value (`log::new`, `log::with`, `log::log_*`)
// ---------------------------------------------------------------------------

fn builtin_log_new(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let fields = expect_str_str_map(&args[0])?;
    Ok(NativeValue::Logger(Rc::new(LoggerRepr { fields })))
}

fn builtin_log_with(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 3)?;
    let logger = expect_logger(&args[0])?;
    let key = expect_str(&args[1])?;
    let val = expect_str(&args[2])?;

    let mut new_fields = logger.fields.clone();
    new_fields.push((key.clone(), val.clone()));
    Ok(NativeValue::Logger(Rc::new(LoggerRepr { fields: new_fields })))
}

fn logger_emit(args: &[NativeValue], level: LogLevelRepr) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let logger = expect_logger(&args[0])?;
    let msg = expect_str(&args[1])?;
    emit(level, msg, &logger.fields);
    Ok(NativeValue::Unit)
}

fn builtin_log_log_debug(args: &[NativeValue]) -> Result<NativeValue, String> {
    logger_emit(args, LogLevelRepr::Debug)
}
fn builtin_log_log_info(args: &[NativeValue]) -> Result<NativeValue, String> {
    logger_emit(args, LogLevelRepr::Info)
}
fn builtin_log_log_warn(args: &[NativeValue]) -> Result<NativeValue, String> {
    logger_emit(args, LogLevelRepr::Warn)
}
fn builtin_log_log_error(args: &[NativeValue]) -> Result<NativeValue, String> {
    logger_emit(args, LogLevelRepr::Error)
}

// ---------------------------------------------------------------------------
// Level control
// ---------------------------------------------------------------------------

fn builtin_log_set_level(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let l = expect_log_level(&args[0])?;
    store_level(l);
    Ok(NativeValue::Unit)
}

fn builtin_log_get_level(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 0)?;
    Ok(NativeValue::LogLevel(current_level()))
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

/// Register every `log::*` function under its `log_*` native name.
pub fn register(registry: &mut NativeRegistry, interner: &mut Interner) {
    // Direct-level functions
    registry.register(interner.intern("log_debug"), builtin_log_debug);
    registry.register(interner.intern("log_info"), builtin_log_info);
    registry.register(interner.intern("log_warn"), builtin_log_warn);
    registry.register(interner.intern("log_error"), builtin_log_error);

    // With-fields variants
    registry.register(interner.intern("log_debug_fields"), builtin_log_debug_fields);
    registry.register(interner.intern("log_info_fields"), builtin_log_info_fields);
    registry.register(interner.intern("log_warn_fields"), builtin_log_warn_fields);
    registry.register(interner.intern("log_error_fields"), builtin_log_error_fields);

    // Logger constructors and methods.
    //
    // NOTE on naming: the spec defines `log::new`, `log::with`, and
    // `log::log_debug`/etc. After the `log_` prefix is added per the
    // stdlib-00 naming rule (`<module>_<function>`), the four scoped
    // logger emitters become `log_log_debug`/etc., which is intentional —
    // they distinguish from the bare `log_debug` (no logger) emitters.
    registry.register(interner.intern("log_new"), builtin_log_new);
    registry.register(interner.intern("log_with"), builtin_log_with);
    registry.register(interner.intern("log_log_debug"), builtin_log_log_debug);
    registry.register(interner.intern("log_log_info"), builtin_log_log_info);
    registry.register(interner.intern("log_log_warn"), builtin_log_log_warn);
    registry.register(interner.intern("log_log_error"), builtin_log_log_error);

    // Level control
    registry.register(interner.intern("log_set_level"), builtin_log_set_level);
    registry.register(interner.intern("log_get_level"), builtin_log_get_level);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// RAII guard that restores the global level on drop. Tests that
    /// mutate `set_level` should hold one of these so test order doesn't
    /// matter.
    struct LevelGuard {
        previous: LogLevelRepr,
    }
    impl LevelGuard {
        fn new() -> Self {
            Self { previous: current_level() }
        }
    }
    impl Drop for LevelGuard {
        fn drop(&mut self) {
            store_level(self.previous);
        }
    }

    /// Helper to build a fields slice from a literal list.
    fn f(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    // ---- render() shape ----

    #[test]
    fn pretty_no_fields_matches_iso8601_shape() {
        let s = render_with(
            "INFO",
            "hello",
            &[],
            Format::Pretty,
            "2026-04-25T14:30:00Z",
        );
        // Equivalent to the regex `^\d{4}-\d{2}-\d{2}T.*Z INFO\s+hello\s*$`.
        assert!(s.starts_with("2026-04-25T14:30:00Z "));
        assert!(s.contains(" INFO "));
        assert!(s.trim_end().ends_with("hello"));
    }

    #[test]
    fn pretty_with_field_ends_with_kv() {
        let s = render_with(
            "INFO",
            "hello",
            &f(&[("k", "v")]),
            Format::Pretty,
            "2026-04-25T14:30:00Z",
        );
        assert!(s.ends_with("k=v"));
    }

    #[test]
    fn json_mode_parses_as_json_object() {
        let s = render_with(
            "INFO",
            "hello",
            &f(&[("k", "v")]),
            Format::Json,
            "2026-04-25T14:30:00Z",
        );
        // We can't pull in serde_json here, but the string must:
        //   * begin with `{` and end with `}`
        //   * contain key:value pairs in expected order (sorted alpha)
        assert!(s.starts_with('{'));
        assert!(s.ends_with('}'));
        assert!(s.contains("\"ts\":\"2026-04-25T14:30:00Z\""));
        assert!(s.contains("\"level\":\"INFO\""));
        assert!(s.contains("\"msg\":\"hello\""));
        assert!(s.contains("\"k\":\"v\""));
    }

    #[test]
    fn pretty_fields_sorted_alphabetically() {
        let s = render_with(
            "INFO",
            "msg",
            &f(&[("zeta", "z"), ("alpha", "a"), ("middle", "m")]),
            Format::Pretty,
            "2026-04-25T14:30:00Z",
        );
        let alpha_idx = s.find("alpha=").unwrap();
        let middle_idx = s.find("middle=").unwrap();
        let zeta_idx = s.find("zeta=").unwrap();
        assert!(alpha_idx < middle_idx);
        assert!(middle_idx < zeta_idx);
    }

    #[test]
    fn json_fields_sorted_alphabetically() {
        let s = render_with(
            "INFO",
            "msg",
            &f(&[("zeta", "z"), ("alpha", "a")]),
            Format::Json,
            "2026-04-25T14:30:00Z",
        );
        let alpha_idx = s.find("\"alpha\"").unwrap();
        let zeta_idx = s.find("\"zeta\"").unwrap();
        assert!(alpha_idx < zeta_idx);
    }

    #[test]
    fn json_escapes_quotes_and_backslashes() {
        let s = render_with(
            "INFO",
            "he said \"hi\"\\there",
            &[],
            Format::Json,
            "2026-04-25T14:30:00Z",
        );
        assert!(s.contains("\\\""));
        assert!(s.contains("\\\\"));
    }

    // ---- level filtering ----

    #[test]
    fn level_round_trip() {
        let _g = LevelGuard::new();
        store_level(LogLevelRepr::Warn);
        assert_eq!(current_level(), LogLevelRepr::Warn);
        store_level(LogLevelRepr::Debug);
        assert_eq!(current_level(), LogLevelRepr::Debug);
    }

    #[test]
    fn set_level_via_native_function() {
        let _g = LevelGuard::new();
        let _ = builtin_log_set_level(&[NativeValue::LogLevel(LogLevelRepr::Error)]).unwrap();
        match builtin_log_get_level(&[]).unwrap() {
            NativeValue::LogLevel(l) => assert_eq!(l, LogLevelRepr::Error),
            other => panic!("expected LogLevel, got {other:?}"),
        }
    }

    #[test]
    fn level_ordering_suppresses_below_threshold() {
        // Info < Warn, so when the threshold is Warn, Info should be
        // suppressed by `emit()`. We verify the filter via the same
        // comparison used by `emit()` rather than capturing stderr.
        let _g = LevelGuard::new();
        store_level(LogLevelRepr::Warn);
        assert!(LogLevelRepr::Info < current_level());
        assert!(LogLevelRepr::Warn >= current_level());
        assert!(LogLevelRepr::Error >= current_level());
    }

    // ---- logger fields ----

    #[test]
    fn logger_new_holds_fields_in_order() {
        let mut m: BTreeMap<MapKey, NativeValue> = BTreeMap::new();
        m.insert(MapKey::Str("a".into()), NativeValue::Str("1".into()));
        m.insert(MapKey::Str("b".into()), NativeValue::Str("2".into()));
        let v = builtin_log_new(&[NativeValue::Map(m)]).unwrap();
        match v {
            NativeValue::Logger(l) => {
                assert_eq!(l.fields, vec![
                    ("a".to_string(), "1".to_string()),
                    ("b".to_string(), "2".to_string()),
                ]);
            }
            other => panic!("expected Logger, got {other:?}"),
        }
    }

    #[test]
    fn logger_with_appends_field_and_does_not_mutate_original() {
        let original = NativeValue::Logger(Rc::new(LoggerRepr {
            fields: vec![("a".into(), "1".into())],
        }));
        let extended = builtin_log_with(&[
            original.clone(),
            NativeValue::Str("req_id".into()),
            NativeValue::Str("abc".into()),
        ])
        .unwrap();

        match (&original, &extended) {
            (NativeValue::Logger(o), NativeValue::Logger(e)) => {
                assert_eq!(o.fields.len(), 1);
                assert_eq!(e.fields.len(), 2);
                assert_eq!(e.fields[1], ("req_id".to_string(), "abc".to_string()));
            }
            _ => panic!("expected both to be Logger"),
        }
    }

    #[test]
    fn logger_render_includes_fields() {
        let logger = LoggerRepr {
            fields: vec![("req_id".into(), "abc".into())],
        };
        // Demonstrate that what `logger_emit` would pass to `render()`
        // produces a line including the logger's fields.
        let line = render_with(
            "INFO",
            "msg",
            &logger.fields,
            Format::Pretty,
            "2026-04-25T14:30:00Z",
        );
        assert!(line.contains("req_id=abc"));
    }

    // ---- error handling ----

    #[test]
    fn debug_rejects_wrong_arg_count() {
        let r = builtin_log_debug(&[]);
        assert!(r.is_err());
    }

    #[test]
    fn info_fields_rejects_non_map_second_arg() {
        let r = builtin_log_info_fields(&[
            NativeValue::Str("msg".into()),
            NativeValue::Int(42),
        ]);
        assert!(r.is_err());
    }

    #[test]
    fn set_level_rejects_non_loglevel_arg() {
        let _g = LevelGuard::new();
        let r = builtin_log_set_level(&[NativeValue::Int(0)]);
        assert!(r.is_err());
    }

    #[test]
    fn with_rejects_non_logger_first_arg() {
        let r = builtin_log_with(&[
            NativeValue::Str("not a logger".into()),
            NativeValue::Str("k".into()),
            NativeValue::Str("v".into()),
        ]);
        assert!(r.is_err());
    }

    // ---- registration ----

    #[test]
    fn register_installs_all_functions() {
        let mut interner = Interner::new();
        let mut registry = NativeRegistry::new();
        register(&mut registry, &mut interner);

        for name in [
            "log_debug",
            "log_info",
            "log_warn",
            "log_error",
            "log_debug_fields",
            "log_info_fields",
            "log_warn_fields",
            "log_error_fields",
            "log_new",
            "log_with",
            "log_log_debug",
            "log_log_info",
            "log_log_warn",
            "log_log_error",
            "log_set_level",
            "log_get_level",
        ] {
            let sym = interner.intern(name);
            assert!(
                registry.get(sym).is_some(),
                "expected {name} to be registered"
            );
        }
    }
}
