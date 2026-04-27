//! M8 Task 5 Option A — wall-clock proof of true async parallelism.
//!
//! The eager Path B used to run `spawn(task: f(x))` sequentially because
//! `f(x)` evaluated to a fully-resolved `Value::Async` before `spawn` saw
//! it. Option A made async fn bodies *lazy*: calling `f(x)` builds a
//! `Pending` Async that captures the deferred call; `spawn` moves the
//! deferred work onto an OS thread before the body runs.
//!
//! This test asserts that two spawned tasks each sleeping ~250ms complete
//! together in noticeably less time than the ~500ms a sequential
//! implementation would take. The threshold is generous to avoid flaking
//! on overloaded CI runners.

use std::time::Instant;

use ferric_common::{Interner, Symbol};
use ferric_lexer::lex;
use ferric_parser::parse_with_interner;
use ferric_resolve::resolve_with_natives;
use ferric_traits::build_registry;
use ferric_vm::{BytecodeVM, Executor, Value};

fn run(src: &str) -> Value {
    let mut interner = Interner::new();
    let native_fns = native_table(&mut interner);
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
    assert!(
        type_result.errors.is_empty(),
        "type errors: {:?}",
        type_result.errors,
    );
    let async_result = ferric_async::lower_async(&parse_result, &type_result);
    assert!(
        async_result.errors.is_empty(),
        "async lower errors: {:?}",
        async_result.errors,
    );
    let program = ferric_compiler::compile(
        &async_result.ast,
        &resolve_result,
        &type_result,
        &interner,
    );
    let natives = ferric_stdlib::NativeRegistry::new();
    // Re-register stdlib bodies so println / int_to_str work.
    let mut natives = natives;
    let mut interner_for_stdlib = interner.clone();
    ferric_stdlib::register_stdlib(&mut natives, &mut interner_for_stdlib);
    let mut vm = BytecodeVM::new();
    vm.run(program, natives, &interner).expect("run failed")
}

fn native_table(interner: &mut Interner) -> Vec<(Symbol, Vec<Symbol>)> {
    let mut intern = |name: &str| interner.intern(name);
    let entries: &[(&str, &[&str])] = &[
        ("println",         &["s"]),
        ("int_to_str",      &["n"]),
        ("spawn",           &["task"]),
        ("join",            &["a", "b"]),
        ("sleep",           &["ms"]),
        ("block_on",        &["task"]),
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

#[test]
fn two_spawned_tasks_run_in_parallel() {
    // Two 250ms sleeps spawned. If genuinely parallel: ~250ms wall clock.
    // If serialised: ~500ms. The 400ms threshold leaves headroom for slow
    // CI without making the test meaningless.
    let src = r#"
        async fn slow(n: Int) -> Int {
            await sleep(ms: 250)
            n
        }
        let h1 = spawn(task: slow(n: 1))
        let h2 = spawn(task: slow(n: 2))
        let pair = join(a: h1, b: h2)
        match pair { (a, b) => a + b }
    "#;

    let start = Instant::now();
    let result = run(src);
    let elapsed = start.elapsed();

    // Result correctness: 1 + 2 = 3.
    assert_eq!(result, Value::Int(3), "expected sum to be 3, got {result:?}");

    // Parallelism: should be much closer to 250ms than 500ms.
    assert!(
        elapsed.as_millis() < 400,
        "two parallel 250ms sleeps should complete in under 400ms; took {}ms — \
         this likely means async fn bodies are running eagerly instead of \
         being deferred until spawn moves them to a worker thread.",
        elapsed.as_millis(),
    );
}

#[test]
fn sequential_block_on_baseline_takes_full_time() {
    // Baseline: same workload but with two sequential `block_on`s instead
    // of `spawn`. Should take roughly 2x the parallel version. Not a
    // strict requirement (the runtime could in principle batch sleeps),
    // just a sanity check that the parallel version's speedup is real.
    let src = r#"
        async fn slow(n: Int) -> Int {
            await sleep(ms: 250)
            n
        }
        let a = block_on(task: slow(n: 1))
        let b = block_on(task: slow(n: 2))
        a + b
    "#;

    let start = Instant::now();
    let result = run(src);
    let elapsed = start.elapsed();

    assert_eq!(result, Value::Int(3));
    assert!(
        elapsed.as_millis() >= 450,
        "two sequential 250ms sleeps should take at least 450ms; took {}ms — \
         if this is fast, the parallelism test isn't actually proving anything.",
        elapsed.as_millis(),
    );
}
