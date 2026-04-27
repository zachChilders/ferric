//! M8 Task 3 — type checker tests for async/await.
//!
//! These tests live at the workspace root (rather than inside `ferric_infer`)
//! because they exercise the full lex → parse → resolve → typecheck pipeline,
//! and `ferric_infer` cannot dev-depend on the upstream stage crates without
//! creating a dependency cycle.

use ferric_common::{Interner, Symbol, Ty, TypeError};
use ferric_lexer::lex;
use ferric_parser::parse_with_interner;
use ferric_resolve::resolve_with_natives;
use ferric_traits::build_registry;

/// Lex + parse + resolve + typecheck a source program. Returns the populated
/// interner, the typecheck output, and any parse errors so callers can assert
/// that parser errors aren't masking typecheck behaviour.
fn check(src: &str) -> (
    Interner,
    ferric_common::TypeResult,
    ferric_common::ParseResult,
) {
    let mut interner = Interner::new();
    let native_fns = m8_native_table(&mut interner);
    let lex_result = lex(src, &mut interner);
    let parse_result = parse_with_interner(&lex_result, &interner);
    let resolve_result = resolve_with_natives(&parse_result, &native_fns);
    let registry = build_registry(&parse_result, &resolve_result, &interner);
    let type_result = ferric_infer::typecheck(
        &parse_result,
        &resolve_result,
        &interner,
        &registry,
    );
    (interner, type_result, parse_result)
}

fn m8_native_table(interner: &mut Interner) -> Vec<(Symbol, Vec<Symbol>)> {
    // Mirrors `src/main.rs::native_fn_table`. Kept in sync manually — when a
    // native is added in main.rs, add it here too if it is referenced by an
    // M8 typecheck test.
    let mut intern = |name: &str| interner.intern(name);
    let entries: &[(&str, &[&str])] = &[
        ("println",  &["s"]),
        ("print",    &["s"]),
        ("spawn",    &["task"]),
        ("join",     &["a", "b"]),
    ];
    entries
        .iter()
        .map(|(name, params)| {
            let n = intern(name);
            let ps: Vec<Symbol> = params.iter().map(|p| intern(p)).collect();
            (n, ps)
        })
        .collect()
}

fn assert_no_type_errors(types: &ferric_common::TypeResult) {
    assert!(
        types.errors.is_empty(),
        "unexpected type errors: {:?}",
        types.errors
    );
}

// --- async fn return type wrapping --------------------------------------

#[test]
fn async_fn_call_site_returns_async_t() {
    let (_, types, _) = check(r#"
        async fn fetch(url: Str) -> Str { url }
        let task: Async<Str> = fetch(url: "x")
    "#);
    assert_no_type_errors(&types);
}

#[test]
fn async_fn_body_returns_inner_t_not_async() {
    // The body returns `Str`, not `Async<Str>`. If the type checker wrapped
    // the body return in Async we'd see Mismatch.
    let (_, types, _) = check(r#"
        async fn fetch(url: Str) -> Str { url }
    "#);
    assert_no_type_errors(&types);
}

// --- await type inference -----------------------------------------------

#[test]
fn await_unwraps_async() {
    let (_, types, _) = check(r#"
        async fn fetch(url: Str) -> Str { url }
        async fn caller() -> Str { await fetch(url: "x") }
    "#);
    assert_no_type_errors(&types);
}

#[test]
fn await_on_non_async_is_error() {
    let (_, types, _) = check(r#"
        async fn bad() -> Int {
            let x: Int = 5
            await x
        }
    "#);
    assert!(
        types.errors.iter().any(|e| matches!(e, TypeError::AwaitOnNonAsync { .. })),
        "expected AwaitOnNonAsync, got: {:?}", types.errors
    );
}

#[test]
fn await_outside_async_is_error() {
    let (_, types, _) = check(r#"
        async fn fetch() -> Str { "x" }
        fn sync_caller() -> Str { await fetch() }
    "#);
    assert!(
        types.errors.iter().any(|e| matches!(e, TypeError::AwaitOutsideAsync { .. })),
        "expected AwaitOutsideAsync, got: {:?}", types.errors,
    );
}

// --- async blocks --------------------------------------------------------

#[test]
fn async_block_has_async_t_type() {
    let (_, types, _) = check(r#"
        let task: Async<Int> = async { 1 + 2 }
    "#);
    assert_no_type_errors(&types);
}

#[test]
fn empty_async_block_is_async_unit() {
    let (_, types, _) = check(r#"
        let t: Async<Unit> = async { }
    "#);
    assert_no_type_errors(&types);
}

#[test]
fn await_inside_async_block_is_legal() {
    let (_, types, _) = check(r#"
        async fn fetch() -> Str { "x" }
        let r: Async<Str> = async { await fetch() }
    "#);
    assert_no_type_errors(&types);
}

// --- spawn ---------------------------------------------------------------

#[test]
fn spawn_returns_handle_of_inner() {
    let (_, types, _) = check(r#"
        async fn fetch() -> Str { "x" }
        let h: Handle<Str> = spawn(task: fetch())
    "#);
    assert_no_type_errors(&types);
}

#[test]
fn spawn_with_non_async_arg_errors() {
    let (_, types, _) = check(r#"
        let h = spawn(task: 5)
    "#);
    assert!(
        types.errors.iter().any(|e| matches!(e, TypeError::SpawnNonAsync { .. })),
        "expected SpawnNonAsync, got: {:?}", types.errors,
    );
}

#[test]
fn handle_await_unwraps_to_inner() {
    let (_, types, _) = check(r#"
        async fn fetch() -> Str { "x" }
        async fn caller() -> Str {
            let h: Handle<Str> = spawn(task: fetch())
            await h
        }
    "#);
    assert_no_type_errors(&types);
}

// --- join ---------------------------------------------------------------

#[test]
fn join_returns_tuple_of_inner_types() {
    let (_, types, _) = check(r#"
        async fn fetch_str() -> Str { "x" }
        async fn fetch_int() -> Int { 1 }
        let r: (Str, Int) = join(
            a: spawn(task: fetch_str()),
            b: spawn(task: fetch_int())
        )
    "#);
    assert_no_type_errors(&types);
}

#[test]
fn join_with_non_handle_arg_errors() {
    let (_, types, _) = check(r#"
        let r = join(a: 1, b: 2)
    "#);
    assert!(
        types.errors.iter().any(|e| matches!(e, TypeError::JoinNonHandle { .. })),
        "expected JoinNonHandle, got: {:?}", types.errors,
    );
}

// --- AsyncNotAwaited ----------------------------------------------------

#[test]
fn async_not_awaited_when_let_expects_inner() {
    // `let s: Str = fetch(url: "x")` — fetch returns Async<Str>, expected Str.
    let (_, types, _) = check(r#"
        async fn fetch(url: Str) -> Str { url }
        let s: Str = fetch(url: "x")
    "#);
    assert!(
        types.errors.iter().any(|e| matches!(e, TypeError::AsyncNotAwaited { .. })),
        "expected AsyncNotAwaited, got: {:?}", types.errors,
    );
}

// --- Async<T>/Handle<T> annotations parse + resolve ---------------------

#[test]
fn async_and_handle_annotations_resolve_to_dedicated_ty_variants() {
    let (_, types, _) = check(r#"
        async fn fetch() -> Int { 1 }
        let task: Async<Int>  = fetch()
        let h:    Handle<Int> = spawn(task: fetch())
    "#);
    assert_no_type_errors(&types);

    // The `let task` binding should have type Async<Int>; `let h` Handle<Int>.
    // We don't have a stable NodeId map here, so just scan node_types.
    let has_async_int = types.node_types.values().any(|t| {
        matches!(t, Ty::Async(inner) if matches!(inner.as_ref(), Ty::Int))
    });
    let has_handle_int = types.node_types.values().any(|t| {
        matches!(t, Ty::Handle(inner) if matches!(inner.as_ref(), Ty::Int))
    });
    assert!(has_async_int, "no Async<Int> in node_types");
    assert!(has_handle_int, "no Handle<Int> in node_types");
}
