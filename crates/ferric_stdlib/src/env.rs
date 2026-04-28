//! # `env` — Environment Variables and Process
//!
//! Implements the `env` module as described in `docs/stdlib.md`.
//!
//! Function surface (registered with the `env_` prefix as
//! `<module>_<function>`):
//!
//! - `env::get(name)`               → `Option<Str>`
//! - `env::get_or(name, default)`   → `Str`
//! - `env::require(name)`           → `Result<Str, EnvError>`
//! - `env::set(name, val)`          → `Unit`
//! - `env::remove(name)`            → `Unit`
//! - `env::all()`                   → `Map<Str, Str>`
//! - `env::args()`                  → `List<Str>`
//! - `env::cwd()`                   → `Result<Str, IoError>`
//! - `env::home_dir()`              → `Option<Str>`
//! - `env::temp_dir()`              → `Str`
//! - `env::exit(code)`              → never returns
//!
//! ## NOTE on `NativeValue` variants
//!
//! Per `docs/tasks/stdlib-00-overview.md` rule (3), this file is written as
//! if the `Option`, `Result`, and `Map` variants already exist on
//! `NativeValue`. The integration step merges them into `lib.rs`. The crate
//! is expected NOT to compile during the parallel phase.
//!
//! REQUIRED NATIVE VALUE VARIANTS:
//!   Option(Option<Box<NativeValue>>)
//!   Result(Result<Box<NativeValue>, Box<NativeValue>>)
//!   Map(BTreeMap<MapKey, NativeValue>)        // shared with map-set task
//!
//! REQUIRED DEPS:
//!   (none — uses std::env / std::process)

use std::collections::BTreeMap;

use ferric_common::{Interner, Symbol};

use crate::{MapKey, NativeRegistry, NativeValue};

// ---------------------------------------------------------------------------
// Local helpers (private per file, per overview rule 5)
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

fn expect_str(value: &NativeValue) -> Result<&str, String> {
    match value {
        NativeValue::Str(s) => Ok(s.as_str()),
        other => Err(format!("expected string, got {other:?}")),
    }
}

fn expect_int(value: &NativeValue) -> Result<i64, String> {
    match value {
        NativeValue::Int(n) => Ok(*n),
        other => Err(format!("expected int, got {other:?}")),
    }
}

/// Construct a Ferric `Option::None`.
fn none() -> NativeValue {
    NativeValue::Option(None)
}

/// Construct a Ferric `Option::Some(v)`.
fn some(v: NativeValue) -> NativeValue {
    NativeValue::Option(Some(Box::new(v)))
}

/// Construct a Ferric `Result::Ok(v)`.
fn ok(v: NativeValue) -> NativeValue {
    NativeValue::Result(Box::new(Ok(v)))
}

/// Construct a Ferric `Result::Err(e)`.
fn err(e: NativeValue) -> NativeValue {
    NativeValue::Result(Box::new(Err(e)))
}

/// Build an `EnvError::NotSet { name }` payload as a string. Once integration
/// rewires error variants this can be swapped to a structured value.
fn env_error_not_set(name: &str) -> NativeValue {
    NativeValue::Str(format!("EnvError::NotSet {{ name: {name:?} }}"))
}

/// Build an `IoError::Other { message }` payload as a string. Same caveat.
fn io_error_other(message: &str) -> NativeValue {
    NativeValue::Str(format!("IoError::Other {{ message: {message:?} }}"))
}

// ---------------------------------------------------------------------------
// env::get(name: Str) -> Option<Str>
// ---------------------------------------------------------------------------

fn builtin_env_get(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let name = expect_str(&args[0])?;
    match std::env::var(name) {
        Ok(v) => Ok(some(NativeValue::Str(v))),
        Err(_) => Ok(none()),
    }
}

// ---------------------------------------------------------------------------
// env::get_or(name: Str, default: Str) -> Str
// ---------------------------------------------------------------------------

fn builtin_env_get_or(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let name = expect_str(&args[0])?;
    let default = expect_str(&args[1])?;
    let value = std::env::var(name).unwrap_or_else(|_| default.to_string());
    Ok(NativeValue::Str(value))
}

// ---------------------------------------------------------------------------
// env::require(name: Str) -> Result<Str, EnvError>
// ---------------------------------------------------------------------------

fn builtin_env_require(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let name = expect_str(&args[0])?;
    match std::env::var(name) {
        Ok(v) => Ok(ok(NativeValue::Str(v))),
        Err(_) => Ok(err(env_error_not_set(name))),
    }
}

// ---------------------------------------------------------------------------
// env::set(name: Str, val: Str) -> Unit
// ---------------------------------------------------------------------------

fn builtin_env_set(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let name = expect_str(&args[0])?;
    let val = expect_str(&args[1])?;
    // SAFETY: `set_var` is sound on every platform Ferric targets; the
    // `unsafe` wrapper exists for multi-threaded native libs that read
    // env directly. The interpreter is single-threaded for stdlib calls.
    #[allow(unused_unsafe)]
    unsafe {
        std::env::set_var(name, val);
    }
    Ok(NativeValue::Unit)
}

// ---------------------------------------------------------------------------
// env::remove(name: Str) -> Unit
// ---------------------------------------------------------------------------

fn builtin_env_remove(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let name = expect_str(&args[0])?;
    #[allow(unused_unsafe)]
    unsafe {
        std::env::remove_var(name);
    }
    Ok(NativeValue::Unit)
}

// ---------------------------------------------------------------------------
// env::all() -> Map<Str, Str>
// ---------------------------------------------------------------------------

fn builtin_env_all(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 0)?;
    let mut m: BTreeMap<MapKey, NativeValue> = BTreeMap::new();
    for (k, v) in std::env::vars() {
        m.insert(MapKey::Str(k), NativeValue::Str(v));
    }
    Ok(NativeValue::Map(m))
}

// ---------------------------------------------------------------------------
// env::args() -> List<Str>
// ---------------------------------------------------------------------------

fn builtin_env_args(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 0)?;
    let v: Vec<NativeValue> = std::env::args().map(NativeValue::Str).collect();
    Ok(NativeValue::Array(v))
}

// ---------------------------------------------------------------------------
// env::cwd() -> Result<Str, IoError>
// ---------------------------------------------------------------------------

fn builtin_env_cwd(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 0)?;
    match std::env::current_dir() {
        Ok(path) => Ok(ok(NativeValue::Str(path.to_string_lossy().into_owned()))),
        Err(e) => Ok(err(io_error_other(&format!("cwd: {e}")))),
    }
}

// ---------------------------------------------------------------------------
// env::home_dir() -> Option<Str>
// ---------------------------------------------------------------------------

fn builtin_env_home_dir(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 0)?;
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    match std::env::var_os(var) {
        Some(s) if !s.is_empty() => Ok(some(NativeValue::Str(
            s.to_string_lossy().into_owned(),
        ))),
        _ => Ok(none()),
    }
}

// ---------------------------------------------------------------------------
// env::temp_dir() -> Str
// ---------------------------------------------------------------------------

fn builtin_env_temp_dir(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 0)?;
    let path = std::env::temp_dir();
    Ok(NativeValue::Str(path.to_string_lossy().into_owned()))
}

// ---------------------------------------------------------------------------
// env::exit(code: Int) -> Unit (never returns)
// ---------------------------------------------------------------------------

/// Terminates the process with the given exit code.
///
/// **DO NOT call this in unit tests.** Doing so will terminate the test
/// process. The function is registered for runtime use only; see the
/// `#[ignore]` placeholder test below.
fn builtin_env_exit(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let code = expect_int(&args[0])?;
    std::process::exit(code as i32);
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

/// Registers every `env` module function with the given native registry.
///
/// Function names follow the `<module>_<function>` convention from
/// `docs/tasks/stdlib-00-overview.md` §8: e.g. `env::get` is registered as
/// the native `env_get`.
pub fn register(registry: &mut NativeRegistry, interner: &mut Interner) {
    #[allow(clippy::type_complexity)]
    let entries: &[(&str, fn(&[NativeValue]) -> Result<NativeValue, String>)] = &[
        ("env_get",       builtin_env_get),
        ("env_get_or",    builtin_env_get_or),
        ("env_require",   builtin_env_require),
        ("env_set",       builtin_env_set),
        ("env_remove",    builtin_env_remove),
        ("env_all",       builtin_env_all),
        ("env_args",      builtin_env_args),
        ("env_cwd",       builtin_env_cwd),
        ("env_home_dir",  builtin_env_home_dir),
        ("env_temp_dir",  builtin_env_temp_dir),
        ("env_exit",      builtin_env_exit),
    ];
    for (name, f) in entries {
        let sym: Symbol = interner.intern(name);
        registry.register(sym, *f);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Generates a unique env var name per test invocation. We combine the
    /// process id, a monotonic counter, and the caller-supplied tag to keep
    /// names from colliding across parallel test threads or repeated runs.
    fn unique_var(tag: &str) -> String {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        format!(
            "FERRIC_TEST_{}_{}_{}_{}",
            tag,
            std::process::id(),
            n,
            // nanos since UNIX_EPOCH for extra entropy across re-runs
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        )
    }

    /// RAII guard that removes a process env var when dropped, even on panic.
    struct EnvGuard(String);
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            #[allow(unused_unsafe)]
            unsafe {
                std::env::remove_var(&self.0);
            }
        }
    }

    fn call(f: fn(&[NativeValue]) -> Result<NativeValue, String>, args: Vec<NativeValue>)
        -> Result<NativeValue, String>
    {
        f(&args)
    }

    // -------- get / set / remove ------------------------------------------

    #[test]
    fn set_then_get_returns_some_value() {
        let name = unique_var("GETSET");
        let _g = EnvGuard(name.clone());

        let _ = call(builtin_env_set, vec![
            NativeValue::Str(name.clone()),
            NativeValue::Str("value".to_string()),
        ]).unwrap();

        let got = call(builtin_env_get, vec![NativeValue::Str(name.clone())]).unwrap();
        assert_eq!(got, some(NativeValue::Str("value".to_string())));
    }

    #[test]
    fn get_unset_returns_none() {
        let name = unique_var("UNSET");
        // Make sure it is not set.
        #[allow(unused_unsafe)]
        unsafe { std::env::remove_var(&name); }

        let got = call(builtin_env_get, vec![NativeValue::Str(name)]).unwrap();
        assert_eq!(got, none());
    }

    #[test]
    fn remove_clears_variable() {
        let name = unique_var("REMOVE");
        let _g = EnvGuard(name.clone());

        let _ = call(builtin_env_set, vec![
            NativeValue::Str(name.clone()),
            NativeValue::Str("x".to_string()),
        ]).unwrap();
        // Sanity: it's set.
        assert_eq!(
            call(builtin_env_get, vec![NativeValue::Str(name.clone())]).unwrap(),
            some(NativeValue::Str("x".to_string())),
        );

        let _ = call(builtin_env_remove, vec![NativeValue::Str(name.clone())]).unwrap();

        let got = call(builtin_env_get, vec![NativeValue::Str(name.clone())]).unwrap();
        assert_eq!(got, none());
    }

    // -------- get_or ------------------------------------------------------

    #[test]
    fn get_or_returns_default_when_missing() {
        let name = unique_var("GETOR_MISS");
        #[allow(unused_unsafe)]
        unsafe { std::env::remove_var(&name); }

        let got = call(builtin_env_get_or, vec![
            NativeValue::Str(name),
            NativeValue::Str("default".to_string()),
        ]).unwrap();
        assert_eq!(got, NativeValue::Str("default".to_string()));
    }

    #[test]
    fn get_or_returns_value_when_set() {
        let name = unique_var("GETOR_HIT");
        let _g = EnvGuard(name.clone());

        let _ = call(builtin_env_set, vec![
            NativeValue::Str(name.clone()),
            NativeValue::Str("real".to_string()),
        ]).unwrap();

        let got = call(builtin_env_get_or, vec![
            NativeValue::Str(name),
            NativeValue::Str("default".to_string()),
        ]).unwrap();
        assert_eq!(got, NativeValue::Str("real".to_string()));
    }

    // -------- require -----------------------------------------------------

    #[test]
    fn require_set_returns_ok() {
        let name = unique_var("REQ_OK");
        let _g = EnvGuard(name.clone());

        let _ = call(builtin_env_set, vec![
            NativeValue::Str(name.clone()),
            NativeValue::Str("v".to_string()),
        ]).unwrap();

        let got = call(builtin_env_require, vec![NativeValue::Str(name)]).unwrap();
        assert_eq!(got, ok(NativeValue::Str("v".to_string())));
    }

    #[test]
    fn require_unset_returns_err() {
        let name = unique_var("REQ_ERR");
        #[allow(unused_unsafe)]
        unsafe { std::env::remove_var(&name); }

        let got = call(builtin_env_require, vec![NativeValue::Str(name.clone())]).unwrap();
        match got {
            NativeValue::Result(boxed) => match *boxed {
                Err(_) => {}
                other => panic!("expected Result::Err, got {other:?}"),
            },
            other => panic!("expected Result, got {other:?}"),
        }
    }

    // -------- all ---------------------------------------------------------

    #[test]
    fn all_includes_a_just_set_variable() {
        let name = unique_var("ALL");
        let _g = EnvGuard(name.clone());

        let _ = call(builtin_env_set, vec![
            NativeValue::Str(name.clone()),
            NativeValue::Str("present".to_string()),
        ]).unwrap();

        let all = call(builtin_env_all, vec![]).unwrap();
        match all {
            NativeValue::Map(m) => {
                let v = m.get(&MapKey::Str(name.clone()));
                assert_eq!(v, Some(&NativeValue::Str("present".to_string())));
            }
            other => panic!("expected Map, got {other:?}"),
        }
    }

    // -------- args --------------------------------------------------------

    #[test]
    fn args_returns_at_least_one_element() {
        let v = call(builtin_env_args, vec![]).unwrap();
        match v {
            NativeValue::Array(items) => {
                assert!(!items.is_empty(), "args should include the binary name");
                // First arg should at least be a Str.
                assert!(matches!(&items[0], NativeValue::Str(_)));
            }
            other => panic!("expected Array, got {other:?}"),
        }
    }

    // -------- cwd ---------------------------------------------------------

    #[test]
    fn cwd_returns_non_empty_string() {
        let v = call(builtin_env_cwd, vec![]).unwrap();
        match v {
            NativeValue::Result(boxed) => match *boxed {
                Ok(NativeValue::Str(s)) => assert!(!s.is_empty(), "cwd should not be empty"),
                other => panic!("expected Result::Ok(Str), got {other:?}"),
            },
            other => panic!("expected Result, got {other:?}"),
        }
    }

    // -------- temp_dir ----------------------------------------------------

    #[test]
    fn temp_dir_is_non_empty() {
        let v = call(builtin_env_temp_dir, vec![]).unwrap();
        match v {
            NativeValue::Str(s) => assert!(!s.is_empty(), "temp_dir should not be empty"),
            other => panic!("expected Str, got {other:?}"),
        }
    }

    // -------- home_dir ----------------------------------------------------

    /// Serialises the two `home_dir_*` tests below — they both mutate the
    /// HOME / USERPROFILE env var, and `cargo test` runs tests in parallel
    /// by default. Without this lock the two races and one observes the
    /// other's intermediate state.
    fn home_dir_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
        LOCK.get_or_init(|| std::sync::Mutex::new(()))
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn home_dir_returns_some_when_home_is_set() {
        let _guard = home_dir_lock();
        // Choose the right var per platform.
        let key = if cfg!(windows) { "USERPROFILE" } else { "HOME" };

        // Snapshot whatever the test process currently has so we can restore
        // it on exit. Avoids polluting other tests.
        let original = std::env::var_os(key);

        #[allow(unused_unsafe)]
        unsafe { std::env::set_var(key, "/some/home"); }

        let got = call(builtin_env_home_dir, vec![]).unwrap();
        // Restore BEFORE asserting so an assertion failure doesn't leak.
        #[allow(unused_unsafe)]
        unsafe {
            match original {
                Some(orig) => std::env::set_var(key, orig),
                None => std::env::remove_var(key),
            }
        }

        assert_eq!(got, some(NativeValue::Str("/some/home".to_string())));
    }

    #[test]
    fn home_dir_returns_none_when_home_is_unset() {
        let _guard = home_dir_lock();
        let key = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
        let original = std::env::var_os(key);

        #[allow(unused_unsafe)]
        unsafe { std::env::remove_var(key); }

        let got = call(builtin_env_home_dir, vec![]).unwrap();

        // Restore.
        #[allow(unused_unsafe)]
        unsafe {
            if let Some(orig) = original {
                std::env::set_var(key, orig);
            }
        }

        assert_eq!(got, none());
    }

    // -------- exit (subprocess test) -------------------------------------

    /// `env::exit` calls `std::process::exit`, which terminates the whole
    /// process. To test it we spawn a small Rust subprocess that invokes
    /// `builtin_env_exit` and check the exit code from the parent.
    #[test]
    fn exit_terminates_process_with_given_code() {
        // Spawn the current test binary with `FERRIC_ENV_EXIT_CODE=7`. The
        // helper test below picks that up, calls `builtin_env_exit`, and
        // never returns. The parent observes the exit code.
        let exe = std::env::current_exe().expect("current_exe");
        let output = std::process::Command::new(&exe)
            .args(["--exact", "env::tests::__env_exit_helper", "--nocapture"])
            .env("FERRIC_ENV_EXIT_CODE", "7")
            .output()
            .expect("spawn child");
        assert_eq!(output.status.code(), Some(7), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    }

    /// Helper invoked only by the parent test above (recognised by the
    /// `FERRIC_ENV_EXIT_CODE` env var). Hidden from default runs by checking
    /// the env var first; if absent, the test is a no-op so it doesn't
    /// pollute the suite.
    #[test]
    fn __env_exit_helper() {
        if let Ok(s) = std::env::var("FERRIC_ENV_EXIT_CODE") {
            let code: i64 = s.parse().expect("FERRIC_ENV_EXIT_CODE must parse");
            // Calls std::process::exit; never returns.
            let _ = builtin_env_exit(&[NativeValue::Int(code)]);
            unreachable!("env::exit must terminate the process");
        }
        // No env var set → no-op (default test runs).
    }

    // -------- registration ------------------------------------------------

    #[test]
    fn register_installs_every_function() {
        let mut registry = NativeRegistry::new();
        let mut interner = Interner::new();

        register(&mut registry, &mut interner);

        for name in [
            "env_get",
            "env_get_or",
            "env_require",
            "env_set",
            "env_remove",
            "env_all",
            "env_args",
            "env_cwd",
            "env_home_dir",
            "env_temp_dir",
            "env_exit",
        ] {
            let sym = interner.intern(name);
            assert!(
                registry.get(sym).is_some(),
                "expected `{name}` to be registered",
            );
        }
    }

    // -------- argument validation surface -------------------------------

    #[test]
    fn get_rejects_wrong_arg_count() {
        let r = call(builtin_env_get, vec![]);
        assert!(r.is_err());
    }

    #[test]
    fn get_rejects_wrong_arg_type() {
        let r = call(builtin_env_get, vec![NativeValue::Int(1)]);
        assert!(r.is_err());
    }

    #[test]
    fn set_rejects_wrong_arg_count() {
        let r = call(builtin_env_set, vec![NativeValue::Str("x".to_string())]);
        assert!(r.is_err());
    }
}
