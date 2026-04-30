//! Stdlib function display catalogue, used by completion menus.
//!
//! Reads from `ferric_stdlib_meta` (the single source of truth) so this
//! file is no longer a hand-curated mirror — it just exposes
//! `iter_stdlib_functions()` to the completion handler.

/// Iterate `(name, display)` pairs for every stdlib native + async
/// intrinsic. `display` is the completion menu's `detail` string, e.g.
/// `"fn(path: Str) -> Str"` for natives that have a typed signature, or
/// `"fn(...)"` for natives whose signature hasn't been filled in yet.
pub fn iter_stdlib_functions() -> impl Iterator<Item = (&'static str, &'static str)> {
    ferric_stdlib_meta::iter_all()
        .chain(ferric_stdlib_meta::async_intrinsics::ASYNC_INTRINSICS.iter())
        .map(|m| (m.name, m.display))
}
