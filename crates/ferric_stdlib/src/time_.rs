// REQUIRED DEPS:
//   chrono = { version = "0.4", default-features = false, features = ["clock", "std"] }
//
// chrono is required for strftime-style formatting and ISO 8601 parsing.
// If integration prefers `time` crate, swap it out — function surface stays
// the same.

// REQUIRED NATIVE VALUE VARIANTS:
//   Time(TimeRepr)
//   Duration(DurationRepr)
//
// where:
//   pub struct TimeRepr { pub unix_ns: i128 }     // nanoseconds since epoch
//   pub struct DurationRepr { pub ns: i64 }       // signed nanoseconds
//
// These are deliberately simple. The integration task may swap to
// `chrono::DateTime<Utc>` or the `time` crate later without changing the
// public function surface.

//! `time` stdlib module — timestamps, durations, formatting, and sleep.
//!
//! All functions are registered with a `time_` prefix. The Ferric-side
//! syntax is `time::function`; the native registry is flat so the prefix
//! is the disambiguator.
//!
//! `Time` values carry a signed `i128` nanosecond offset from the Unix
//! epoch. `Duration` values are signed `i64` nanoseconds. Both
//! representations live in this file as `TimeRepr` / `DurationRepr` and
//! ride inside `NativeValue::Time` / `NativeValue::Duration` (added
//! during integration).

use std::sync::OnceLock;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Datelike, Local, NaiveDate, NaiveDateTime, TimeZone, Timelike, Utc};

use ferric_common::Interner;

use crate::{NativeRegistry, NativeValue};

// ============================================================================
// Repr types (what `NativeValue::Time` / `NativeValue::Duration` carry)
// ============================================================================

/// Wall-clock instant, stored as signed nanoseconds since the Unix epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TimeRepr {
    pub unix_ns: i128,
}

/// Signed duration, stored as nanoseconds. Can be negative.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DurationRepr {
    pub ns: i64,
}

// ============================================================================
// Local helpers (private to this file per stdlib-00 rules)
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

fn expect_int(value: &NativeValue) -> Result<i64, String> {
    match value {
        NativeValue::Int(n) => Ok(*n),
        other => Err(format!("expected int, got {other:?}")),
    }
}

fn expect_str(value: &NativeValue) -> Result<&String, String> {
    match value {
        NativeValue::Str(s) => Ok(s),
        other => Err(format!("expected string, got {other:?}")),
    }
}

fn expect_time(value: &NativeValue) -> Result<TimeRepr, String> {
    match value {
        NativeValue::Time(t) => Ok(*t),
        other => Err(format!("expected Time, got {other:?}")),
    }
}

fn expect_duration(value: &NativeValue) -> Result<DurationRepr, String> {
    match value {
        NativeValue::Duration(d) => Ok(*d),
        other => Err(format!("expected Duration, got {other:?}")),
    }
}

/// Convert a `TimeRepr` to a `chrono::DateTime<Utc>`. Falls back to the
/// epoch if the offset is out of `chrono`'s supported range — we never
/// panic.
fn time_to_chrono_utc(t: TimeRepr) -> Result<DateTime<Utc>, String> {
    // Split nanos into seconds + sub-second nanos using Euclidean division
    // so negative values produce a non-negative remainder.
    let secs_i128 = t.unix_ns.div_euclid(1_000_000_000);
    let nanos_i128 = t.unix_ns.rem_euclid(1_000_000_000);
    let secs: i64 = secs_i128
        .try_into()
        .map_err(|_| "time out of range".to_string())?;
    let nanos: u32 = nanos_i128 as u32;
    Utc.timestamp_opt(secs, nanos)
        .single()
        .ok_or_else(|| "time out of range".to_string())
}

fn chrono_utc_to_time(dt: DateTime<Utc>) -> TimeRepr {
    let secs = dt.timestamp() as i128;
    let nanos = dt.timestamp_subsec_nanos() as i128;
    TimeRepr {
        unix_ns: secs * 1_000_000_000 + nanos,
    }
}

/// Process-wide monotonic anchor, initialised on first call.
fn monotonic_anchor() -> Instant {
    static ANCHOR: OnceLock<Instant> = OnceLock::new();
    *ANCHOR.get_or_init(Instant::now)
}

// ============================================================================
// Construction / current time
// ============================================================================

fn builtin_time_now(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 0)?;
    let now = SystemTime::now();
    let unix_ns = match now.duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_nanos() as i128,
        Err(e) => -(e.duration().as_nanos() as i128),
    };
    Ok(NativeValue::Time(TimeRepr { unix_ns }))
}

fn builtin_time_now_local(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 0)?;
    // Wall-clock value is identical to `now()` — the local TZ is observable
    // only via formatting. We still call into `Local::now()` so that any
    // future refactor keeping a local-aware `Time` representation only
    // touches this function.
    let _local: DateTime<Local> = Local::now();
    let now = SystemTime::now();
    let unix_ns = match now.duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_nanos() as i128,
        Err(e) => -(e.duration().as_nanos() as i128),
    };
    Ok(NativeValue::Time(TimeRepr { unix_ns }))
}

fn builtin_time_monotonic(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 0)?;
    let anchor = monotonic_anchor();
    let elapsed = Instant::now().saturating_duration_since(anchor);
    Ok(NativeValue::Int(elapsed.as_nanos() as i64))
}

fn builtin_time_from_unix(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let secs = expect_int(&args[0])?;
    let unix_ns = (secs as i128) * 1_000_000_000;
    Ok(NativeValue::Time(TimeRepr { unix_ns }))
}

fn builtin_time_from_unix_ms(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let ms = expect_int(&args[0])?;
    let unix_ns = (ms as i128) * 1_000_000;
    Ok(NativeValue::Time(TimeRepr { unix_ns }))
}

fn builtin_time_from_unix_ns(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let ns = expect_int(&args[0])?;
    Ok(NativeValue::Time(TimeRepr {
        unix_ns: ns as i128,
    }))
}

// ============================================================================
// Decomposition
// ============================================================================

fn builtin_time_unix(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let t = expect_time(&args[0])?;
    let secs = t.unix_ns.div_euclid(1_000_000_000);
    Ok(NativeValue::Int(secs as i64))
}

fn builtin_time_unix_ms(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let t = expect_time(&args[0])?;
    let ms = t.unix_ns.div_euclid(1_000_000);
    Ok(NativeValue::Int(ms as i64))
}

fn builtin_time_year(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let t = expect_time(&args[0])?;
    let dt = time_to_chrono_utc(t)?;
    Ok(NativeValue::Int(dt.year() as i64))
}

fn builtin_time_month(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let t = expect_time(&args[0])?;
    let dt = time_to_chrono_utc(t)?;
    Ok(NativeValue::Int(dt.month() as i64))
}

fn builtin_time_day(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let t = expect_time(&args[0])?;
    let dt = time_to_chrono_utc(t)?;
    Ok(NativeValue::Int(dt.day() as i64))
}

fn builtin_time_hour(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let t = expect_time(&args[0])?;
    let dt = time_to_chrono_utc(t)?;
    Ok(NativeValue::Int(dt.hour() as i64))
}

fn builtin_time_minute(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let t = expect_time(&args[0])?;
    let dt = time_to_chrono_utc(t)?;
    Ok(NativeValue::Int(dt.minute() as i64))
}

fn builtin_time_second(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let t = expect_time(&args[0])?;
    let dt = time_to_chrono_utc(t)?;
    Ok(NativeValue::Int(dt.second() as i64))
}

fn builtin_time_weekday(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let t = expect_time(&args[0])?;
    let dt = time_to_chrono_utc(t)?;
    // chrono's `num_days_from_monday` already returns 0=Mon..6=Sun.
    Ok(NativeValue::Int(dt.weekday().num_days_from_monday() as i64))
}

// ============================================================================
// Arithmetic
// ============================================================================

fn builtin_time_add_secs(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let t = expect_time(&args[0])?;
    let secs = expect_int(&args[1])?;
    let unix_ns = t.unix_ns + (secs as i128) * 1_000_000_000;
    Ok(NativeValue::Time(TimeRepr { unix_ns }))
}

fn builtin_time_add_ms(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let t = expect_time(&args[0])?;
    let ms = expect_int(&args[1])?;
    let unix_ns = t.unix_ns + (ms as i128) * 1_000_000;
    Ok(NativeValue::Time(TimeRepr { unix_ns }))
}

fn builtin_time_add_days(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let t = expect_time(&args[0])?;
    let days = expect_int(&args[1])?;
    let unix_ns = t.unix_ns + (days as i128) * 86_400 * 1_000_000_000;
    Ok(NativeValue::Time(TimeRepr { unix_ns }))
}

fn builtin_time_diff_secs(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let a = expect_time(&args[0])?;
    let b = expect_time(&args[1])?;
    let diff = (a.unix_ns - b.unix_ns).div_euclid(1_000_000_000);
    Ok(NativeValue::Int(diff as i64))
}

fn builtin_time_diff_ms(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let a = expect_time(&args[0])?;
    let b = expect_time(&args[1])?;
    let diff = (a.unix_ns - b.unix_ns).div_euclid(1_000_000);
    Ok(NativeValue::Int(diff as i64))
}

// ============================================================================
// Formatting / parsing
// ============================================================================

fn builtin_time_format(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let t = expect_time(&args[0])?;
    let layout = expect_str(&args[1])?;
    let dt = time_to_chrono_utc(t)?;
    // chrono's `format` panics on invalid strftime specifiers when the
    // resulting `DelayedFormat` is rendered. Wrap the rendering in
    // `catch_unwind` so we surface a clean error to Ferric.
    let layout_owned = layout.clone();
    let result = std::panic::catch_unwind(|| dt.format(&layout_owned).to_string());
    match result {
        Ok(s) => Ok(NativeValue::Str(s)),
        Err(_) => Err(format!("invalid format layout: {layout:?}")),
    }
}

fn builtin_time_format_iso(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let t = expect_time(&args[0])?;
    let dt = time_to_chrono_utc(t)?;
    // RFC 3339 with `Z` suffix (always UTC since `TimeRepr` is naive epoch
    // ns). Use a strftime layout to control trailing fractional seconds —
    // we omit them for whole-second values and use millisecond precision
    // when sub-second nanos are present, matching common ISO 8601 practice.
    let s = if dt.timestamp_subsec_nanos() == 0 {
        dt.format("%Y-%m-%dT%H:%M:%SZ").to_string()
    } else {
        dt.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
    };
    Ok(NativeValue::Str(s))
}

fn builtin_time_format_rfc2822(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let t = expect_time(&args[0])?;
    let dt = time_to_chrono_utc(t)?;
    Ok(NativeValue::Str(dt.to_rfc2822()))
}

fn builtin_time_parse(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let input = expect_str(&args[0])?;
    let layout = expect_str(&args[1])?;
    // First try as a full datetime.
    if let Ok(ndt) = NaiveDateTime::parse_from_str(input, layout) {
        let dt = Utc.from_utc_datetime(&ndt);
        return Ok(NativeValue::Time(chrono_utc_to_time(dt)));
    }
    // Otherwise fall back to a date-only parse (e.g. `%Y-%m-%d`) and
    // assume midnight UTC.
    if let Ok(nd) = NaiveDate::parse_from_str(input, layout) {
        let ndt = nd
            .and_hms_opt(0, 0, 0)
            .ok_or_else(|| "out of range".to_string())?;
        let dt = Utc.from_utc_datetime(&ndt);
        return Ok(NativeValue::Time(chrono_utc_to_time(dt)));
    }
    Err(format!(
        "parse: failed to parse {input:?} with layout {layout:?}"
    ))
}

fn builtin_time_parse_iso(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let input = expect_str(&args[0])?;
    // RFC 3339 covers both `Z` and `+HH:MM` zone suffixes.
    match DateTime::parse_from_rfc3339(input) {
        Ok(dt) => Ok(NativeValue::Time(chrono_utc_to_time(
            dt.with_timezone(&Utc),
        ))),
        Err(e) => Err(format!("parse_iso: {e}")),
    }
}

// ============================================================================
// Duration
// ============================================================================

fn builtin_time_secs(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let n = expect_int(&args[0])?;
    let ns = n
        .checked_mul(1_000_000_000)
        .ok_or_else(|| "duration overflow".to_string())?;
    Ok(NativeValue::Duration(DurationRepr { ns }))
}

fn builtin_time_ms(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let n = expect_int(&args[0])?;
    let ns = n
        .checked_mul(1_000_000)
        .ok_or_else(|| "duration overflow".to_string())?;
    Ok(NativeValue::Duration(DurationRepr { ns }))
}

fn builtin_time_minutes(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let n = expect_int(&args[0])?;
    let ns = n
        .checked_mul(60 * 1_000_000_000)
        .ok_or_else(|| "duration overflow".to_string())?;
    Ok(NativeValue::Duration(DurationRepr { ns }))
}

fn builtin_time_hours(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let n = expect_int(&args[0])?;
    let ns = n
        .checked_mul(3_600 * 1_000_000_000)
        .ok_or_else(|| "duration overflow".to_string())?;
    Ok(NativeValue::Duration(DurationRepr { ns }))
}

fn builtin_time_days(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let n = expect_int(&args[0])?;
    let ns = n
        .checked_mul(86_400 * 1_000_000_000)
        .ok_or_else(|| "duration overflow".to_string())?;
    Ok(NativeValue::Duration(DurationRepr { ns }))
}

fn builtin_time_sleep(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let d = expect_duration(&args[0])?;
    if d.ns > 0 {
        let secs = (d.ns / 1_000_000_000) as u64;
        let nanos = (d.ns % 1_000_000_000) as u32;
        std::thread::sleep(std::time::Duration::new(secs, nanos));
    }
    Ok(NativeValue::Unit)
}

// ============================================================================
// Registration
// ============================================================================

/// Registers every `time::*` function with the native registry under the
/// `time_*` flat-name convention.
pub fn register(registry: &mut NativeRegistry, interner: &mut Interner) {
    type Entry = (&'static str, &'static [&'static str], crate::NativeFn);
    let entries: &[Entry] = &[
        // Construction / current time
        ("time_now", &[], builtin_time_now),
        ("time_now_local", &[], builtin_time_now_local),
        ("time_monotonic", &[], builtin_time_monotonic),
        ("time_from_unix", &["secs"], builtin_time_from_unix),
        ("time_from_unix_ms", &["ms"], builtin_time_from_unix_ms),
        ("time_from_unix_ns", &["ns"], builtin_time_from_unix_ns),
        // Decomposition
        ("time_unix", &["t"], builtin_time_unix),
        ("time_unix_ms", &["t"], builtin_time_unix_ms),
        ("time_year", &["t"], builtin_time_year),
        ("time_month", &["t"], builtin_time_month),
        ("time_day", &["t"], builtin_time_day),
        ("time_hour", &["t"], builtin_time_hour),
        ("time_minute", &["t"], builtin_time_minute),
        ("time_second", &["t"], builtin_time_second),
        ("time_weekday", &["t"], builtin_time_weekday),
        // Arithmetic
        ("time_add_secs", &["t", "secs"], builtin_time_add_secs),
        ("time_add_ms", &["t", "ms"], builtin_time_add_ms),
        ("time_add_days", &["t", "days"], builtin_time_add_days),
        ("time_diff_secs", &["a", "b"], builtin_time_diff_secs),
        ("time_diff_ms", &["a", "b"], builtin_time_diff_ms),
        // Formatting / parsing
        ("time_format", &["t", "layout"], builtin_time_format),
        ("time_format_iso", &["t"], builtin_time_format_iso),
        ("time_format_rfc2822", &["t"], builtin_time_format_rfc2822),
        ("time_parse", &["s", "layout"], builtin_time_parse),
        ("time_parse_iso", &["s"], builtin_time_parse_iso),
        // Duration
        ("time_secs", &["n"], builtin_time_secs),
        ("time_ms", &["n"], builtin_time_ms),
        ("time_minutes", &["n"], builtin_time_minutes),
        ("time_hours", &["n"], builtin_time_hours),
        ("time_days", &["n"], builtin_time_days),
        ("time_sleep", &["d"], builtin_time_sleep),
    ];
    for (name, params, f) in entries {
        registry.register_named(interner, name, params, *f);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// `from_unix(1714052700)` corresponds to 2024-04-25 13:45:00 UTC,
    /// which is a Thursday.
    const KNOWN_UNIX: i64 = 1_714_052_700;

    fn unwrap_time(v: NativeValue) -> TimeRepr {
        match v {
            NativeValue::Time(t) => t,
            other => panic!("expected Time, got {other:?}"),
        }
    }

    fn unwrap_duration(v: NativeValue) -> DurationRepr {
        match v {
            NativeValue::Duration(d) => d,
            other => panic!("expected Duration, got {other:?}"),
        }
    }

    fn unwrap_int(v: NativeValue) -> i64 {
        match v {
            NativeValue::Int(n) => n,
            other => panic!("expected Int, got {other:?}"),
        }
    }

    fn unwrap_str(v: NativeValue) -> String {
        match v {
            NativeValue::Str(s) => s,
            other => panic!("expected Str, got {other:?}"),
        }
    }

    // ---- now / unix ----

    #[test]
    fn test_now_returns_time_in_modern_range() {
        let t = unwrap_time(builtin_time_now(&[]).unwrap());
        let secs = t.unix_ns / 1_000_000_000;
        // After 2023-11-14 (1_700_000_000), well before any reasonable
        // far-future timestamp.
        assert!(secs > 1_700_000_000, "now() unix secs = {secs}");
        assert!(secs < 10_000_000_000, "now() unix secs = {secs}");
    }

    #[test]
    fn test_now_local_returns_time_in_modern_range() {
        let t = unwrap_time(builtin_time_now_local(&[]).unwrap());
        let secs = t.unix_ns / 1_000_000_000;
        assert!(secs > 1_700_000_000);
    }

    // ---- from_unix / unix roundtrips ----

    #[test]
    fn test_from_unix_zero_roundtrip() {
        let t = builtin_time_from_unix(&[NativeValue::Int(0)]).unwrap();
        let unix = unwrap_int(builtin_time_unix(&[t]).unwrap());
        assert_eq!(unix, 0);
    }

    #[test]
    fn test_from_unix_ms_roundtrip() {
        let t = builtin_time_from_unix_ms(&[NativeValue::Int(1_234_567_890_123)]).unwrap();
        let ms = unwrap_int(builtin_time_unix_ms(&[t]).unwrap());
        assert_eq!(ms, 1_234_567_890_123);
    }

    #[test]
    fn test_from_unix_ns_roundtrip() {
        let t = builtin_time_from_unix_ns(&[NativeValue::Int(987_654_321)]).unwrap();
        match t {
            NativeValue::Time(repr) => assert_eq!(repr.unix_ns, 987_654_321),
            other => panic!("expected Time, got {other:?}"),
        }
    }

    // ---- decomposition (known timestamp) ----

    #[test]
    fn test_decomposition_known_timestamp() {
        let t = builtin_time_from_unix(&[NativeValue::Int(KNOWN_UNIX)]).unwrap();
        assert_eq!(unwrap_int(builtin_time_year(&[t.clone()]).unwrap()), 2024);
        assert_eq!(unwrap_int(builtin_time_month(&[t.clone()]).unwrap()), 4);
        assert_eq!(unwrap_int(builtin_time_day(&[t.clone()]).unwrap()), 25);
        assert_eq!(unwrap_int(builtin_time_hour(&[t.clone()]).unwrap()), 13);
        assert_eq!(unwrap_int(builtin_time_minute(&[t.clone()]).unwrap()), 45);
        assert_eq!(unwrap_int(builtin_time_second(&[t.clone()]).unwrap()), 0);
    }

    #[test]
    fn test_weekday_known_timestamp() {
        // 2024-04-25 was a Thursday (Monday=0, so Thu=3).
        let t = builtin_time_from_unix(&[NativeValue::Int(KNOWN_UNIX)]).unwrap();
        assert_eq!(unwrap_int(builtin_time_weekday(&[t]).unwrap()), 3);
    }

    #[test]
    fn test_weekday_epoch_is_thursday() {
        // 1970-01-01 was a Thursday (Mon=0..Sun=6).
        let t = builtin_time_from_unix(&[NativeValue::Int(0)]).unwrap();
        assert_eq!(unwrap_int(builtin_time_weekday(&[t]).unwrap()), 3);
    }

    // ---- arithmetic ----

    #[test]
    fn test_add_secs_then_diff_secs_roundtrips() {
        let t = builtin_time_from_unix(&[NativeValue::Int(KNOWN_UNIX)]).unwrap();
        let t2 = builtin_time_add_secs(&[t.clone(), NativeValue::Int(60)]).unwrap();
        let diff = unwrap_int(builtin_time_diff_secs(&[t2, t]).unwrap());
        assert_eq!(diff, 60);
    }

    #[test]
    fn test_add_ms_then_diff_ms_roundtrips() {
        let t = builtin_time_from_unix(&[NativeValue::Int(KNOWN_UNIX)]).unwrap();
        let t2 = builtin_time_add_ms(&[t.clone(), NativeValue::Int(2_500)]).unwrap();
        let diff = unwrap_int(builtin_time_diff_ms(&[t2, t]).unwrap());
        assert_eq!(diff, 2_500);
    }

    #[test]
    fn test_add_days_increases_unix_by_seven_days() {
        let t = builtin_time_from_unix(&[NativeValue::Int(KNOWN_UNIX)]).unwrap();
        let t2 = builtin_time_add_days(&[t, NativeValue::Int(7)]).unwrap();
        let unix2 = unwrap_int(builtin_time_unix(&[t2]).unwrap());
        assert_eq!(unix2, KNOWN_UNIX + 7 * 86_400);
    }

    // ---- formatting ----

    #[test]
    fn test_format_iso_epoch() {
        let t = builtin_time_from_unix(&[NativeValue::Int(0)]).unwrap();
        let s = unwrap_str(builtin_time_format_iso(&[t]).unwrap());
        assert_eq!(s, "1970-01-01T00:00:00Z");
    }

    #[test]
    fn test_format_iso_known_timestamp() {
        let t = builtin_time_from_unix(&[NativeValue::Int(KNOWN_UNIX)]).unwrap();
        let s = unwrap_str(builtin_time_format_iso(&[t]).unwrap());
        assert_eq!(s, "2024-04-25T13:45:00Z");
    }

    #[test]
    fn test_parse_iso_roundtrip_through_format_iso() {
        let original = "2026-04-25T14:30:00Z";
        let t = builtin_time_parse_iso(&[NativeValue::Str(original.to_string())]).unwrap();
        let s = unwrap_str(builtin_time_format_iso(&[t]).unwrap());
        assert_eq!(s, original);
    }

    #[test]
    fn test_parse_iso_accepts_offset_suffix() {
        let t =
            builtin_time_parse_iso(&[NativeValue::Str("2026-04-25T14:30:00+00:00".to_string())])
                .unwrap();
        let s = unwrap_str(builtin_time_format_iso(&[t]).unwrap());
        assert_eq!(s, "2026-04-25T14:30:00Z");
    }

    #[test]
    fn test_parse_iso_invalid_input_errs() {
        let r = builtin_time_parse_iso(&[NativeValue::Str("not a timestamp".to_string())]);
        assert!(r.is_err());
    }

    #[test]
    fn test_format_rfc2822_known_timestamp() {
        // 2024-04-25 13:45:00 UTC → "Thu, 25 Apr 2024 13:45:00 +0000"
        let t = builtin_time_from_unix(&[NativeValue::Int(KNOWN_UNIX)]).unwrap();
        let s = unwrap_str(builtin_time_format_rfc2822(&[t]).unwrap());
        assert_eq!(s, "Thu, 25 Apr 2024 13:45:00 +0000");
    }

    #[test]
    fn test_format_strftime_layout() {
        let t = builtin_time_from_unix(&[NativeValue::Int(KNOWN_UNIX)]).unwrap();
        let layout = NativeValue::Str("%Y-%m-%d".to_string());
        let s = unwrap_str(builtin_time_format(&[t, layout]).unwrap());
        assert_eq!(s, "2024-04-25");
    }

    #[test]
    fn test_parse_strftime_layout() {
        let input = NativeValue::Str("2026-04-25".to_string());
        let layout = NativeValue::Str("%Y-%m-%d".to_string());
        let t = builtin_time_parse(&[input, layout]).unwrap();
        assert_eq!(unwrap_int(builtin_time_year(&[t.clone()]).unwrap()), 2026);
        assert_eq!(unwrap_int(builtin_time_month(&[t.clone()]).unwrap()), 4);
        assert_eq!(unwrap_int(builtin_time_day(&[t]).unwrap()), 25);
    }

    #[test]
    fn test_parse_bad_input_errs() {
        let input = NativeValue::Str("not a date".to_string());
        let layout = NativeValue::Str("%Y-%m-%d".to_string());
        let r = builtin_time_parse(&[input, layout]);
        assert!(r.is_err());
    }

    // ---- duration ----

    #[test]
    fn test_secs_and_minutes_equal() {
        let a = unwrap_duration(builtin_time_secs(&[NativeValue::Int(60)]).unwrap());
        let b = unwrap_duration(builtin_time_minutes(&[NativeValue::Int(1)]).unwrap());
        assert_eq!(a, b);
    }

    #[test]
    fn test_ms_and_secs_consistent() {
        let a = unwrap_duration(builtin_time_ms(&[NativeValue::Int(1_000)]).unwrap());
        let b = unwrap_duration(builtin_time_secs(&[NativeValue::Int(1)]).unwrap());
        assert_eq!(a, b);
    }

    #[test]
    fn test_hours_and_minutes_consistent() {
        let a = unwrap_duration(builtin_time_hours(&[NativeValue::Int(1)]).unwrap());
        let b = unwrap_duration(builtin_time_minutes(&[NativeValue::Int(60)]).unwrap());
        assert_eq!(a, b);
    }

    #[test]
    fn test_days_and_hours_consistent() {
        let a = unwrap_duration(builtin_time_days(&[NativeValue::Int(1)]).unwrap());
        let b = unwrap_duration(builtin_time_hours(&[NativeValue::Int(24)]).unwrap());
        assert_eq!(a, b);
    }

    // ---- monotonic / sleep ----

    #[test]
    fn test_monotonic_is_non_decreasing() {
        let a = unwrap_int(builtin_time_monotonic(&[]).unwrap());
        let b = unwrap_int(builtin_time_monotonic(&[]).unwrap());
        assert!(b >= a, "monotonic went backwards: {a} -> {b}");
    }

    #[test]
    fn test_sleep_zero_is_noop() {
        let d = builtin_time_ms(&[NativeValue::Int(0)]).unwrap();
        let result = builtin_time_sleep(&[d]).unwrap();
        assert_eq!(result, NativeValue::Unit);
    }

    #[test]
    fn test_sleep_negative_is_noop() {
        let d = NativeValue::Duration(DurationRepr { ns: -1_000_000 });
        let result = builtin_time_sleep(&[d]).unwrap();
        assert_eq!(result, NativeValue::Unit);
    }

    // ---- registration smoke test ----

    #[test]
    fn test_register_populates_all_functions() {
        let mut registry = NativeRegistry::new();
        let mut interner = Interner::new();
        register(&mut registry, &mut interner);

        let names = [
            "time_now",
            "time_now_local",
            "time_monotonic",
            "time_from_unix",
            "time_from_unix_ms",
            "time_from_unix_ns",
            "time_unix",
            "time_unix_ms",
            "time_year",
            "time_month",
            "time_day",
            "time_hour",
            "time_minute",
            "time_second",
            "time_weekday",
            "time_add_secs",
            "time_add_ms",
            "time_add_days",
            "time_diff_secs",
            "time_diff_ms",
            "time_format",
            "time_format_iso",
            "time_format_rfc2822",
            "time_parse",
            "time_parse_iso",
            "time_secs",
            "time_ms",
            "time_minutes",
            "time_hours",
            "time_days",
            "time_sleep",
        ];
        for n in names {
            let sym = interner.intern(n);
            assert!(registry.get(sym).is_some(), "missing registration: {n}");
        }
    }

    // ---- arg-count / type-error sanity ----

    #[test]
    fn test_wrong_arg_count_errs() {
        assert!(builtin_time_now(&[NativeValue::Int(0)]).is_err());
        assert!(builtin_time_from_unix(&[]).is_err());
        assert!(builtin_time_add_secs(&[NativeValue::Int(0)]).is_err());
    }

    #[test]
    fn test_wrong_arg_type_errs() {
        assert!(builtin_time_unix(&[NativeValue::Int(0)]).is_err());
        assert!(builtin_time_sleep(&[NativeValue::Int(0)]).is_err());
        assert!(
            builtin_time_format(&[
                NativeValue::Time(TimeRepr { unix_ns: 0 }),
                NativeValue::Int(0),
            ])
            .is_err()
        );
    }
}
