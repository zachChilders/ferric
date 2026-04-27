//! M8 Task 4 — `ferric_async` lowering pass tests.
//!
//! These tests live at the workspace root because they exercise the full
//! lex → parse → typecheck → lower pipeline, and `ferric_async` cannot
//! dev-depend on the upstream stage crates without creating a cycle.
//!
//! The lowered AST does not (yet) round-trip through the compiler — that
//! requires Task 5's runtime. These tests focus on the **structural** output
//! invariant: no async AST nodes remain after lowering, and the right
//! number of items are emitted per async fn / async block.

use ferric_async::lower_async;
use ferric_common::{
    AsyncLowerError, AsyncWarningKind, Expr, Interner, Item, ParseResult, Stmt, Symbol,
    TypeResult,
};
use ferric_lexer::lex;
use ferric_parser::parse_with_interner;
use ferric_resolve::resolve_with_natives;
use ferric_traits::build_registry;
use std::collections::HashMap;

fn pipeline(src: &str) -> (Interner, ParseResult, TypeResult) {
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
    (interner, parse_result, type_result)
}

fn m8_native_table(interner: &mut Interner) -> Vec<(Symbol, Vec<Symbol>)> {
    // Mirrors `tests/m8_typecheck.rs::m8_native_table`. Kept in sync so spawn
    // and join are recognised when they appear in the source.
    let mut intern = |name: &str| interner.intern(name);
    let entries: &[(&str, &[&str])] = &[
        ("println", &["s"]),
        ("print",   &["s"]),
        ("spawn",   &["task"]),
        ("join",    &["a", "b"]),
    ];
    entries
        .iter()
        .map(|(name, params)| {
            let n = intern(name);
            let ps = params.iter().map(|p| intern(p)).collect();
            (n, ps)
        })
        .collect()
}

// --- pass-through invariant ---------------------------------------------

#[test]
fn m1_m7_program_passes_through_unchanged() {
    // A representative M1–M7 program with several constructs (fn, struct,
    // let, if, while, return). After lowering: structurally identical AST.
    let src = r#"
        fn add(x: Int, y: Int) -> Int { x + y }
        struct Point { x: Int, y: Int }
        let p = Point { x: 1, y: 2 }
        let mut counter = 0
        while counter < 5 {
            counter = counter + 1
        }
    "#;
    let (_, parse, types) = pipeline(src);
    let result = lower_async(&parse, &types);

    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    assert!(result.warnings.is_empty(), "warnings: {:?}", result.warnings);
    assert_eq!(result.ast.items.len(), parse.items.len(),
        "item count should be unchanged for M1–M7 program");

    // NodeIds preserved across the pass: build a Set of every NodeId in
    // the original AST and verify every NodeId in the lowered AST is in
    // that set. (Equivalent to "no new IDs were minted" for a pure
    // pass-through.)
    let original_ids = collect_node_ids(&parse);
    let lowered_ids = collect_node_ids(&result.ast);
    assert_eq!(original_ids, lowered_ids,
        "lowering should preserve NodeIds for non-async programs");
}

fn collect_node_ids(ast: &ParseResult) -> std::collections::HashSet<u32> {
    let mut out = HashMap::new();
    for item in &ast.items {
        gather_item(item, &mut out);
    }
    out.into_keys().collect()
}

fn gather_item(item: &Item, out: &mut HashMap<u32, ()>) {
    out.insert(item.id().0, ());
    match item {
        Item::Fn(f) => gather_expr(&f.body, out),
        Item::AsyncFn(decl) => gather_expr(&decl.item.body, out),
        Item::ImplBlock { methods, .. } => {
            for m in methods {
                out.insert(m.id.0, ());
                gather_expr(&m.body, out);
            }
        }
        Item::Script { stmt, .. } => gather_stmt(stmt, out),
        Item::Export(decl) => gather_item(&decl.item, out),
        Item::TraitDef { methods, .. } => {
            for tm in methods {
                out.insert(tm.id.0, ());
            }
        }
        _ => {}
    }
}

fn gather_stmt(stmt: &Stmt, out: &mut HashMap<u32, ()>) {
    out.insert(stmt.id().0, ());
    match stmt {
        Stmt::Let { init, .. } => gather_expr(init, out),
        Stmt::Assign { target, value, .. } => {
            gather_expr(target, out);
            gather_expr(value, out);
        }
        Stmt::Expr { expr } => gather_expr(expr, out),
        Stmt::Require(req) => {
            gather_expr(&req.expr, out);
            if let Some(m) = &req.message { gather_expr(m, out); }
            if let Some(s) = &req.set_fn { gather_expr(s, out); }
        }
        Stmt::For { iter, body, .. } => {
            gather_expr(iter, out);
            gather_expr(body, out);
        }
    }
}

fn gather_expr(expr: &Expr, out: &mut HashMap<u32, ()>) {
    out.insert(expr.id().0, ());
    match expr {
        Expr::Literal { .. }
        | Expr::Variable { .. }
        | Expr::Break { .. }
        | Expr::Continue { .. } => {}
        Expr::Binary { left, right, .. } => {
            gather_expr(left, out);
            gather_expr(right, out);
        }
        Expr::Unary { expr: e, .. } => gather_expr(e, out),
        Expr::Call { callee, args, .. } => {
            gather_expr(callee, out);
            for a in args { gather_expr(&a.value, out); }
        }
        Expr::If { cond, then_branch, else_branch, .. } => {
            gather_expr(cond, out);
            gather_expr(then_branch, out);
            if let Some(e) = else_branch { gather_expr(e, out); }
        }
        Expr::Block { stmts, expr: tail, .. } => {
            for s in stmts { gather_stmt(s, out); }
            if let Some(t) = tail { gather_expr(t, out); }
        }
        Expr::Return { expr: e, .. } => {
            if let Some(inner) = e { gather_expr(inner, out); }
        }
        Expr::While { cond, body, .. } => {
            gather_expr(cond, out);
            gather_expr(body, out);
        }
        Expr::Loop { body, .. } | Expr::Closure { body, .. } => gather_expr(body, out),
        Expr::Shell { parts, .. } => {
            for p in parts {
                if let ferric_common::ShellPart::Interpolated(e) = p {
                    gather_expr(e, out);
                }
            }
        }
        Expr::StructLit { fields, .. } => {
            for (_, e) in fields { gather_expr(e, out); }
        }
        Expr::FieldAccess { expr: e, .. } => gather_expr(e, out),
        Expr::Match { scrutinee, arms, .. } => {
            gather_expr(scrutinee, out);
            for arm in arms { gather_expr(&arm.body, out); }
        }
        Expr::Tuple { elements, .. } | Expr::ArrayLit { elements, .. } => {
            for e in elements { gather_expr(e, out); }
        }
        Expr::VariantCtor { args, .. } => {
            for a in args { gather_expr(a, out); }
        }
        Expr::Index { array, index, .. } => {
            gather_expr(array, out);
            gather_expr(index, out);
        }
        Expr::MethodCall { receiver, args, .. } => {
            gather_expr(receiver, out);
            for a in args { gather_expr(&a.value, out); }
        }
        Expr::Cast(c) => gather_expr(&c.expr, out),
        Expr::Await(a) => gather_expr(&a.operand, out),
        Expr::AsyncBlock(b) => gather_expr(&b.block, out),
    }
}

// --- AsyncFn lowering (Path B Option A — near-identity, AsyncFn survives) ---

#[test]
fn async_fn_survives_lowering_as_async_fn() {
    // Path B Option A: lower_async leaves Item::AsyncFn intact. The
    // compiler handles the lazy wrapping (constructor chunk + body chunk)
    // directly at the AsyncFn site — moving the wrap there gives access
    // to the fn's params, which the resolver-time capture analysis would
    // not have seen if the wrap happened in this pass.
    let src = r#"
        async fn fetch(url: Str) -> Str { url }
    "#;
    let (_, parse, types) = pipeline(src);
    let result = lower_async(&parse, &types);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

    assert_eq!(result.ast.items.len(), 1,
        "expected 1 item, got: {:#?}", result.ast.items);
    let item = &result.ast.items[0];
    assert!(matches!(item, Item::AsyncFn(_)),
        "AsyncFn should survive lowering, got: {item:#?}");
}

// --- AsyncBlock lowering ------------------------------------------------

#[test]
fn async_block_passes_through_unchanged() {
    // Path B keeps AsyncBlock as an AST node — the compiler emits
    // Op::MakeAsync for it (after hoisting the body to its own chunk).
    let src = r#"
        let task = async { 42 }
    "#;
    let (_, parse, types) = pipeline(src);
    let result = lower_async(&parse, &types);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    assert_eq!(result.ast.items.len(), 1, "no extra items emitted for AsyncBlock");

    let init = match &result.ast.items[0] {
        Item::Script { stmt: Stmt::Let { init, .. }, .. } => init,
        other => panic!("expected script let, got {other:?}"),
    };
    assert!(matches!(init, Expr::AsyncBlock(_)),
        "AsyncBlock should pass through (compiler handles via Op::MakeAsync), got {init:?}");
}

// --- Await lowering -----------------------------------------------------

#[test]
fn await_passes_through_unchanged() {
    // Path B keeps Await as an AST node — the compiler emits Op::Await.
    let src = r#"
        async fn fetch(url: Str) -> Str { url }
        async fn caller() -> Str { await fetch(url: "x") }
    "#;
    let (_, parse, types) = pipeline(src);
    let result = lower_async(&parse, &types);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

    // caller's body (post-lowering, still AsyncFn) is the original block
    // containing the Await as its tail expression.
    let body = match &result.ast.items[1] {
        Item::AsyncFn(decl) => &decl.item.body,
        other => panic!("expected AsyncFn, got {other:?}"),
    };
    let tail = match body {
        Expr::Block { expr: Some(t), .. } => t.as_ref(),
        other => panic!("expected Block body, got {other:?}"),
    };
    assert!(matches!(tail, Expr::Await(_)),
        "tail expr should still be Await, got {tail:?}");
}

// --- InfiniteAsyncRecursion -------------------------------------------

#[test]
fn direct_self_recursion_is_an_error() {
    let src = r#"
        async fn forever() -> Int { await forever() }
    "#;
    let (_, parse, types) = pipeline(src);
    let result = lower_async(&parse, &types);
    assert!(
        result.errors.iter().any(|e| matches!(e, AsyncLowerError::InfiniteAsyncRecursion { .. })),
        "expected InfiniteAsyncRecursion, got: {:?}", result.errors,
    );
}

// --- Shell expression handling ---------------------------------------

#[test]
fn shell_outside_async_in_pure_m1m7_program_is_silent() {
    // No async syntax anywhere → the BlockingShell warning is suppressed
    // (it's noise for users who haven't opted into async).
    let src = r#"
        let x = $ echo hi
    "#;
    let (_, parse, types) = pipeline(src);
    let result = lower_async(&parse, &types);
    assert!(
        result.warnings.is_empty(),
        "pure M1–M7 programs should not get async warnings; got: {:?}",
        result.warnings
    );
}

#[test]
fn shell_outside_async_when_async_is_used_warns() {
    // The program also uses `async fn`, so the user has opted into async.
    // A blocking shell expression at script level now warrants the warning.
    let src = r#"
        async fn noop() -> Int { 1 }
        let x = $ echo hi
    "#;
    let (_, parse, types) = pipeline(src);
    let result = lower_async(&parse, &types);
    assert!(
        result.warnings.iter().any(|w| matches!(w.kind, AsyncWarningKind::BlockingShell)),
        "expected BlockingShell warning, got: {:?}", result.warnings,
    );
}
