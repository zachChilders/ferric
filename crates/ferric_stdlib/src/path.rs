//! # `path` — Filesystem Path Manipulation
//!
//! Implements the `path` module from `docs/stdlib.md`. These are pure string
//! transforms; no I/O. Paths are surfaced as `Str` to the user, matching the
//! design decision in the spec ("No `Path` type at the surface").

// REQUIRED NATIVE VALUE VARIANTS:
//   none — `path` operates entirely on Str / List<Str>.

// REQUIRED DEPS:
//   none — std::path is sufficient.

use std::path::{Component, MAIN_SEPARATOR, Path, PathBuf};

use ferric_common::Interner;

use crate::{NativeRegistry, NativeValue};

// ============================================================================
// Local helpers
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

fn expect_array(value: &NativeValue) -> Result<&Vec<NativeValue>, String> {
    match value {
        NativeValue::Array(a) => Ok(a),
        other => Err(format!("expected list, got {other:?}")),
    }
}

fn make_some(s: String) -> NativeValue {
    // The Option/Result enums are user-visible; native code surfaces an
    // ergonomic stand-in. We use Array([Str]) for Some(s) and Array([])
    // for None until the bridge to Ty::Option<T> in NativeValue lands.
    NativeValue::Array(vec![NativeValue::Str(s)])
}

fn make_none() -> NativeValue {
    NativeValue::Array(Vec::new())
}

fn make_ok(s: String) -> NativeValue {
    NativeValue::Array(vec![NativeValue::Bool(true), NativeValue::Str(s)])
}

fn make_err(s: String) -> NativeValue {
    NativeValue::Array(vec![NativeValue::Bool(false), NativeValue::Str(s)])
}

// ============================================================================
// path::join
// ============================================================================

fn builtin_path_join(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let base = expect_str(&args[0])?;
    let parts = expect_array(&args[1])?;
    let mut out = PathBuf::from(base.as_str());
    for p in parts {
        let s = expect_str(p)?;
        out.push(s.as_str());
    }
    Ok(NativeValue::Str(out.to_string_lossy().into_owned()))
}

// ============================================================================
// parent / filename / stem / extension / with_extension
// ============================================================================

fn builtin_path_parent(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let p = expect_str(&args[0])?;
    let path = Path::new(p.as_str());
    match path.parent() {
        // `Path::parent("a")` returns `Some("")`. Per the task doc we treat
        // an empty parent as `None` so `parent("a") == None`.
        Some(parent) if !parent.as_os_str().is_empty() => {
            Ok(make_some(parent.to_string_lossy().into_owned()))
        }
        _ => Ok(make_none()),
    }
}

fn builtin_path_filename(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let p = expect_str(&args[0])?;
    match Path::new(p.as_str()).file_name() {
        Some(n) => Ok(make_some(n.to_string_lossy().into_owned())),
        None => Ok(make_none()),
    }
}

fn builtin_path_stem(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let p = expect_str(&args[0])?;
    match Path::new(p.as_str()).file_stem() {
        Some(n) => Ok(make_some(n.to_string_lossy().into_owned())),
        None => Ok(make_none()),
    }
}

fn builtin_path_extension(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let p = expect_str(&args[0])?;
    match Path::new(p.as_str()).extension() {
        Some(n) => Ok(make_some(n.to_string_lossy().into_owned())),
        None => Ok(make_none()),
    }
}

fn builtin_path_with_extension(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let p = expect_str(&args[0])?;
    let ext = expect_str(&args[1])?;
    let new = Path::new(p.as_str()).with_extension(ext.as_str());
    Ok(NativeValue::Str(new.to_string_lossy().into_owned()))
}

// ============================================================================
// is_absolute / is_relative
// ============================================================================

fn builtin_path_is_absolute(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let p = expect_str(&args[0])?;
    Ok(NativeValue::Bool(Path::new(p.as_str()).is_absolute()))
}

fn builtin_path_is_relative(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let p = expect_str(&args[0])?;
    Ok(NativeValue::Bool(Path::new(p.as_str()).is_relative()))
}

// ============================================================================
// normalize — lexical only, no I/O
// ============================================================================

fn lexical_normalize(input: &str) -> String {
    let path = Path::new(input);
    let mut out: Vec<Component<'_>> = Vec::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {
                // skip "."
            }
            Component::ParentDir => {
                // pop a previous Normal component; otherwise keep the ".."
                match out.last() {
                    Some(Component::Normal(_)) => {
                        out.pop();
                    }
                    Some(Component::ParentDir) | None => {
                        // leading ".." chain on a relative path — preserve
                        out.push(Component::ParentDir);
                    }
                    Some(Component::RootDir) | Some(Component::Prefix(_)) => {
                        // ".." above root collapses to root — drop it
                    }
                    Some(Component::CurDir) => {
                        out.pop();
                        out.push(Component::ParentDir);
                    }
                }
            }
            other => out.push(other),
        }
    }

    if out.is_empty() {
        // The input collapsed to nothing — represent as "."
        return ".".to_string();
    }

    let mut buf = PathBuf::new();
    for c in out {
        buf.push(c.as_os_str());
    }
    buf.to_string_lossy().into_owned()
}

fn builtin_path_normalize(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let p = expect_str(&args[0])?;
    Ok(NativeValue::Str(lexical_normalize(p.as_str())))
}

// ============================================================================
// relative_to
// ============================================================================

fn builtin_path_relative_to(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let p = expect_str(&args[0])?;
    let base = expect_str(&args[1])?;
    let path = Path::new(p.as_str());
    let base_path = Path::new(base.as_str());
    match path.strip_prefix(base_path) {
        Ok(rel) => Ok(make_ok(rel.to_string_lossy().into_owned())),
        Err(_) => Ok(make_err(format!(
            "PathError::NotRelativeTo {{ path: {p:?}, base: {base:?} }}"
        ))),
    }
}

// ============================================================================
// components
// ============================================================================

fn builtin_path_components(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let p = expect_str(&args[0])?;
    let mut out: Vec<NativeValue> = Vec::new();
    for c in Path::new(p.as_str()).components() {
        match c {
            Component::Normal(s) => out.push(NativeValue::Str(s.to_string_lossy().into_owned())),
            Component::CurDir => out.push(NativeValue::Str(".".to_string())),
            Component::ParentDir => out.push(NativeValue::Str("..".to_string())),
            Component::RootDir => out.push(NativeValue::Str(MAIN_SEPARATOR.to_string())),
            Component::Prefix(p) => out.push(NativeValue::Str(
                p.as_os_str().to_string_lossy().into_owned(),
            )),
        }
    }
    Ok(NativeValue::Array(out))
}

// ============================================================================
// Registration
// ============================================================================

/// Registers all `path_*` native functions with the registry.
pub fn register(registry: &mut NativeRegistry, interner: &mut Interner) {
    type Entry = (&'static str, &'static [&'static str], crate::NativeFn);
    let entries: &[Entry] = &[
        ("path_join", &["base", "parts"], builtin_path_join),
        ("path_parent", &["p"], builtin_path_parent),
        ("path_filename", &["p"], builtin_path_filename),
        ("path_stem", &["p"], builtin_path_stem),
        ("path_extension", &["p"], builtin_path_extension),
        (
            "path_with_extension",
            &["p", "ext"],
            builtin_path_with_extension,
        ),
        ("path_is_absolute", &["p"], builtin_path_is_absolute),
        ("path_is_relative", &["p"], builtin_path_is_relative),
        ("path_normalize", &["p"], builtin_path_normalize),
        ("path_relative_to", &["p", "base"], builtin_path_relative_to),
        ("path_components", &["p"], builtin_path_components),
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

    fn s(v: &str) -> NativeValue {
        NativeValue::Str(v.to_string())
    }

    fn arr(items: Vec<&str>) -> NativeValue {
        NativeValue::Array(items.into_iter().map(s).collect())
    }

    fn unwrap_str(v: &NativeValue) -> &str {
        match v {
            NativeValue::Str(s) => s.as_str(),
            other => panic!("expected Str, got {other:?}"),
        }
    }

    fn unwrap_some(v: &NativeValue) -> &str {
        match v {
            NativeValue::Array(items) if items.len() == 1 => unwrap_str(&items[0]),
            _ => panic!("expected Some(Str), got {v:?}"),
        }
    }

    fn is_none(v: &NativeValue) -> bool {
        matches!(v, NativeValue::Array(items) if items.is_empty())
    }

    fn is_ok(v: &NativeValue) -> Option<&str> {
        match v {
            NativeValue::Array(items) if items.len() == 2 => match (&items[0], &items[1]) {
                (NativeValue::Bool(true), NativeValue::Str(s)) => Some(s.as_str()),
                _ => None,
            },
            _ => None,
        }
    }

    fn is_err(v: &NativeValue) -> bool {
        matches!(
            v,
            NativeValue::Array(items)
                if items.len() == 2 && matches!(&items[0], NativeValue::Bool(false))
        )
    }

    fn sep() -> char {
        MAIN_SEPARATOR
    }

    // ---- join ------------------------------------------------------------

    #[test]
    fn join_concatenates_components() {
        let r = builtin_path_join(&[s("a"), arr(vec!["b", "c"])]).unwrap();
        let expected = format!("a{sep}b{sep}c", sep = sep());
        assert_eq!(unwrap_str(&r), expected);
    }

    #[test]
    fn join_empty_parts_returns_base() {
        let r = builtin_path_join(&[s("a"), arr(vec![])]).unwrap();
        assert_eq!(unwrap_str(&r), "a");
    }

    // ---- parent ----------------------------------------------------------

    #[test]
    fn parent_of_nested_returns_parent() {
        let p = format!("a{sep}b{sep}c", sep = sep());
        let r = builtin_path_parent(&[s(&p)]).unwrap();
        let want = format!("a{sep}b", sep = sep());
        assert_eq!(unwrap_some(&r), want);
    }

    #[test]
    fn parent_of_single_component_is_none() {
        let r = builtin_path_parent(&[s("a")]).unwrap();
        assert!(is_none(&r), "got: {r:?}");
    }

    // ---- filename --------------------------------------------------------

    #[test]
    fn filename_returns_last_component() {
        let p = format!("a{sep}b.txt", sep = sep());
        let r = builtin_path_filename(&[s(&p)]).unwrap();
        assert_eq!(unwrap_some(&r), "b.txt");
    }

    #[test]
    fn filename_of_dot_is_none() {
        let r = builtin_path_filename(&[s(".")]).unwrap();
        assert!(is_none(&r), "got: {r:?}");
    }

    // ---- stem ------------------------------------------------------------

    #[test]
    fn stem_strips_extension() {
        let p = format!("a{sep}b.txt", sep = sep());
        let r = builtin_path_stem(&[s(&p)]).unwrap();
        assert_eq!(unwrap_some(&r), "b");
    }

    #[test]
    fn stem_of_no_extension_is_filename() {
        let r = builtin_path_stem(&[s("Makefile")]).unwrap();
        assert_eq!(unwrap_some(&r), "Makefile");
    }

    // ---- extension -------------------------------------------------------

    #[test]
    fn extension_returns_ext() {
        let p = format!("a{sep}b.txt", sep = sep());
        let r = builtin_path_extension(&[s(&p)]).unwrap();
        assert_eq!(unwrap_some(&r), "txt");
    }

    #[test]
    fn extension_of_makefile_is_none() {
        let r = builtin_path_extension(&[s("Makefile")]).unwrap();
        assert!(is_none(&r), "got: {r:?}");
    }

    // ---- with_extension --------------------------------------------------

    #[test]
    fn with_extension_replaces() {
        let p = format!("a{sep}b.txt", sep = sep());
        let r = builtin_path_with_extension(&[s(&p), s("rs")]).unwrap();
        let want = format!("a{sep}b.rs", sep = sep());
        assert_eq!(unwrap_str(&r), want);
    }

    #[test]
    fn with_extension_adds_when_absent() {
        let r = builtin_path_with_extension(&[s("Makefile"), s("bak")]).unwrap();
        assert_eq!(unwrap_str(&r), "Makefile.bak");
    }

    // ---- is_absolute / is_relative ---------------------------------------

    #[test]
    #[cfg(unix)]
    fn is_absolute_true_for_unix_root() {
        let r = builtin_path_is_absolute(&[s("/foo")]).unwrap();
        assert_eq!(r, NativeValue::Bool(true));
    }

    #[test]
    fn is_relative_true_for_relative() {
        let r = builtin_path_is_relative(&[s("foo")]).unwrap();
        assert_eq!(r, NativeValue::Bool(true));
    }

    #[test]
    fn is_relative_false_for_absolute() {
        #[cfg(unix)]
        let p = "/foo";
        #[cfg(windows)]
        let p = r"C:\foo";
        let r = builtin_path_is_relative(&[s(p)]).unwrap();
        assert_eq!(r, NativeValue::Bool(false));
    }

    // ---- normalize -------------------------------------------------------

    #[test]
    fn normalize_resolves_dot_and_dotdot() {
        let p = format!("a{sep}.{sep}b{sep}..{sep}c", sep = sep());
        let r = builtin_path_normalize(&[s(&p)]).unwrap();
        let want = format!("a{sep}c", sep = sep());
        assert_eq!(unwrap_str(&r), want);
    }

    #[test]
    fn normalize_preserves_leading_dotdot() {
        let p = format!("..{sep}a", sep = sep());
        let r = builtin_path_normalize(&[s(&p)]).unwrap();
        assert_eq!(unwrap_str(&r), p);
    }

    #[test]
    fn normalize_collapses_to_dot_when_empty() {
        let p = format!("a{sep}..", sep = sep());
        let r = builtin_path_normalize(&[s(&p)]).unwrap();
        assert_eq!(unwrap_str(&r), ".");
    }

    // ---- relative_to -----------------------------------------------------

    #[test]
    fn relative_to_descendant_ok() {
        let base = format!("{sep}a", sep = sep());
        let path = format!("{sep}a{sep}b{sep}c", sep = sep());
        let r = builtin_path_relative_to(&[s(&path), s(&base)]).unwrap();
        let got = is_ok(&r).expect("expected Ok variant");
        let want = format!("b{sep}c", sep = sep());
        assert_eq!(got, want);
    }

    #[test]
    fn relative_to_unrelated_err() {
        let base = format!("{sep}a", sep = sep());
        let path = format!("{sep}x", sep = sep());
        let r = builtin_path_relative_to(&[s(&path), s(&base)]).unwrap();
        assert!(is_err(&r), "got: {r:?}");
    }

    // ---- components ------------------------------------------------------

    #[test]
    fn components_splits_relative() {
        let p = format!("a{sep}b{sep}c", sep = sep());
        let r = builtin_path_components(&[s(&p)]).unwrap();
        if let NativeValue::Array(items) = r {
            let strs: Vec<String> = items
                .into_iter()
                .map(|v| match v {
                    NativeValue::Str(s) => s,
                    _ => panic!(),
                })
                .collect();
            assert_eq!(strs, vec!["a", "b", "c"]);
        } else {
            panic!("expected Array");
        }
    }

    #[test]
    #[cfg(unix)]
    fn components_includes_root_for_absolute_unix() {
        let r = builtin_path_components(&[s("/a/b")]).unwrap();
        if let NativeValue::Array(items) = r {
            let strs: Vec<String> = items
                .into_iter()
                .map(|v| match v {
                    NativeValue::Str(s) => s,
                    _ => panic!(),
                })
                .collect();
            assert_eq!(strs, vec!["/", "a", "b"]);
        } else {
            panic!("expected Array");
        }
    }

    // ---- Argument errors -------------------------------------------------

    #[test]
    fn join_rejects_wrong_arg_count() {
        let r = builtin_path_join(&[s("a")]);
        assert!(r.is_err());
    }

    #[test]
    fn join_rejects_non_string_part() {
        let bad = NativeValue::Array(vec![NativeValue::Int(1)]);
        let r = builtin_path_join(&[s("a"), bad]);
        assert!(r.is_err());
    }

    // ---- Registration ----------------------------------------------------

    #[test]
    fn register_installs_all_path_natives() {
        let mut reg = NativeRegistry::new();
        let mut int = Interner::new();
        register(&mut reg, &mut int);

        for name in [
            "path_join",
            "path_parent",
            "path_filename",
            "path_stem",
            "path_extension",
            "path_with_extension",
            "path_is_absolute",
            "path_is_relative",
            "path_normalize",
            "path_relative_to",
            "path_components",
        ] {
            let sym = int.intern(name);
            assert!(reg.get(sym).is_some(), "missing native: {name}");
        }
    }
}
