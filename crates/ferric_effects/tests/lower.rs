//! Integration tests for `ferric_effects::lower_effects`.
//!
//! These tests run the full lex+parse pipeline so the input AST has the
//! same shape `main.rs` would feed to the effects pass.

use ferric_common::{EffectError, EffectWarning, Interner};
use ferric_effects::lower_effects;
use ferric_infer::typecheck;
use ferric_lexer::lex;
use ferric_parser::{parse_with_interner, pre_intern_desugar_names};
use ferric_resolve::resolve_with_natives_and_builtins;
use ferric_traits::build_registry;

/// Lex+parse+typecheck `src` and run the effects pass over the result.
/// Returns the EffectResult plus the interner so callers can resolve symbols.
fn run_effects(src: &str) -> (Interner, ferric_common::EffectResult) {
    let mut interner = Interner::new();
    let lex_result = lex(src, &mut interner);
    pre_intern_desugar_names(&mut interner);
    let parse = parse_with_interner(&lex_result, &interner);
    assert!(parse.errors.is_empty(), "parse errors: {:?}", parse.errors);

    let resolve = resolve_with_natives_and_builtins(&parse, &[], &[]);
    let traits = build_registry(&parse, &resolve, &interner);
    let types = typecheck(&parse, &resolve, &interner, &traits);

    let er = lower_effects(&parse, &types);
    (interner, er)
}

// ---------------------------------------------------------------------------
// Tag assignment
// ---------------------------------------------------------------------------

#[test]
fn lower_effects_assigns_tags_to_user_effect() {
    let src = r#"
        effect Config {
            Get(key: Str) -> Str,
        }

        fn read_host() -> Str {
            perform Config::Get(key: "db.host")
        }
    "#;
    let (interner, er) = run_effects(src);
    assert!(er.errors.is_empty(), "errors: {:?}", er.errors);

    let config = interner.lookup("Config").expect("Config interned");
    let get = interner.lookup("Get").expect("Get interned");

    let pair = er.tags.lookup(config, get);
    assert!(pair.is_some(), "Config::Get should be tagged");
    let (_eff_tag, op_tag) = pair.unwrap();
    assert_eq!(op_tag, 0, "first op of an effect gets op_tag 0");
}

#[test]
fn lower_effects_assigns_distinct_tags_to_two_effects() {
    let src = r#"
        effect A {
            Op(x: Int) -> Int,
        }
        effect B {
            Op(x: Int) -> Int,
        }

        fn touch() -> Int {
            perform A::Op(x: 1)
        }
    "#;
    let (interner, er) = run_effects(src);

    let a = interner.lookup("A").unwrap();
    let b = interner.lookup("B").unwrap();
    let op = interner.lookup("Op").unwrap();

    let (a_tag, _) = er.tags.lookup(a, op).unwrap();
    let (b_tag, _) = er.tags.lookup(b, op).unwrap();
    assert_ne!(a_tag, b_tag, "distinct effects must get distinct tags");
}

#[test]
fn lower_effects_handles_async_desugar_no_user_effect_decl() {
    // `async fn` desugars to perform Async::Suspend at parse time. The
    // effects pass should still produce a tag for the Async effect even
    // though no `effect Async { ... }` declaration appears.
    let src = r#"
        async fn fetch(url: Str) -> Str { url }

        let task: Async<Str> = async { await fetch(url: "x") }
    "#;
    let (interner, er) = run_effects(src);

    let async_eff = interner.lookup("Async").unwrap();
    let suspend_op = interner.lookup("Suspend").unwrap();

    assert!(
        er.tags.lookup(async_eff, suspend_op).is_some(),
        "Async::Suspend should have a tag after lower_effects"
    );
}

// ---------------------------------------------------------------------------
// Continuation diagnostics
// ---------------------------------------------------------------------------

#[test]
fn lower_effects_warns_on_dropped_continuation() {
    // Handler clause never resumes `k` — that's "intentional abort"
    // territory and worth flagging as a warning.
    let src = r#"
        effect Abort {
            Bail(msg: Str) -> Str,
        }

        fn run() -> Str {
            handle {
                perform Abort::Bail(msg: "oops")
            } with {
                Abort::Bail(msg) => "fallback"
            }
        }
    "#;
    let (_, er) = run_effects(src);
    assert!(
        er.warnings
            .iter()
            .any(|w| matches!(w, EffectWarning::ContinuationDropped { .. })),
        "expected ContinuationDropped warning, got: {:?}",
        er.warnings
    );
}

#[test]
fn lower_effects_no_warning_when_continuation_is_resumed() {
    let src = r#"
        effect Reader {
            Get(key: Str) -> Str,
        }

        fn run() -> Str {
            handle {
                perform Reader::Get(key: "x")
            } with {
                Reader::Get(key) => resume k with "value"
            }
        }
    "#;
    let (_, er) = run_effects(src);
    let dropped: Vec<_> = er
        .warnings
        .iter()
        .filter(|w| matches!(w, EffectWarning::ContinuationDropped { .. }))
        .collect();
    assert!(
        dropped.is_empty(),
        "expected no ContinuationDropped, got: {:?}",
        dropped
    );
}

#[test]
fn lower_effects_detects_resumed_twice_in_sequence() {
    // Two `resume`s in the same clause body, sequentially. A trivial
    // static double-resume the analyzer can flag.
    let src = r#"
        effect E {
            Op(x: Int) -> Int,
        }

        fn run() -> Int {
            handle {
                perform E::Op(x: 1)
            } with {
                E::Op(x) => {
                    let _a = resume k with 1
                    resume k with 2
                }
            }
        }
    "#;
    let (_, er) = run_effects(src);
    assert!(
        er.errors
            .iter()
            .any(|e| matches!(e, EffectError::ResumedTwice { .. })),
        "expected ResumedTwice, got: {:?}",
        er.errors
    );
}

// ---------------------------------------------------------------------------
// Identity / no-op behaviour
// ---------------------------------------------------------------------------

#[test]
fn lower_effects_pure_program_is_no_op() {
    // A program with no effect machinery at all should pass through
    // `lower_effects` unchanged, with empty tag tables.
    let src = r#"
        fn double(n: Int) -> Int { n + n }
        let x = double(n: 21)
        println(s: int_to_str(n: x))
    "#;
    let (_, er) = run_effects(src);
    assert!(er.errors.is_empty(), "unexpected errors: {:?}", er.errors);
    assert!(er.tags.effect_tags.is_empty());
    assert!(er.tags.op_tags.is_empty());
}
