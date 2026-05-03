//! Integration tests for M9 algebraic-effects opcodes.
//!
//! These tests drive the full pipeline (lex → parse → resolve → typecheck →
//! lower_effects → compile → run) so the `EffectTags` table the compiler
//! reads is populated with real values, giving the VM real `(effect_tag,
//! op_tag)` pairs to match on.

use ferric_common::{Interner, Symbol, TraitRegistry};
use ferric_effects::lower_effects;
use ferric_infer::typecheck;
use ferric_lexer::lex;
use ferric_parser::{parse_with_interner, pre_intern_desugar_names};
use ferric_resolve::resolve_with_natives;
use ferric_stdlib::{NativeRegistry, register_stdlib};
use ferric_vm::{BytecodeVM, Executor, RuntimeError, Value};

fn run_source(src: &str) -> Result<Value, RuntimeError> {
    let mut interner = Interner::new();
    let mut natives = NativeRegistry::new();
    register_stdlib(&mut natives, &mut interner);

    let native_fns: Vec<(Symbol, Vec<Symbol>)> = vec![
        (interner.intern("println"), vec![interner.intern("s")]),
        (interner.intern("print"), vec![interner.intern("s")]),
        (interner.intern("int_to_str"), vec![interner.intern("n")]),
        (interner.intern("float_to_str"), vec![interner.intern("n")]),
        (interner.intern("bool_to_str"), vec![interner.intern("b")]),
        (interner.intern("int_to_float"), vec![interner.intern("n")]),
    ];

    let lex_result = lex(src, &mut interner);
    assert!(lex_result.errors.is_empty(), "lex: {:?}", lex_result.errors);
    pre_intern_desugar_names(&mut interner);
    let parse_result = parse_with_interner(&lex_result, &interner);
    assert!(
        parse_result.errors.is_empty(),
        "parse: {:?}",
        parse_result.errors
    );
    let resolve_result = resolve_with_natives(&parse_result, &native_fns);
    assert!(
        resolve_result.errors.is_empty(),
        "resolve: {:?}",
        resolve_result.errors
    );
    let registry = TraitRegistry::new();
    let type_result = typecheck(&parse_result, &resolve_result, &interner, &registry);
    assert!(
        type_result.errors.is_empty(),
        "types: {:?}",
        type_result.errors
    );

    let effect_result = lower_effects(&parse_result, &type_result);
    assert!(
        effect_result.errors.is_empty(),
        "effects: {:?}",
        effect_result.errors
    );

    let program = ferric_compiler::compile_with_effects(
        &effect_result.ast,
        &resolve_result,
        &type_result,
        &interner,
        &effect_result.tags,
    );

    let mut vm = BytecodeVM::new();
    vm.run(program, natives, &interner)
}

// ----------------------------------------------------------------------
// 1. PushHandler / PopHandler with no `perform` — frame in/out only.
// ----------------------------------------------------------------------
#[test]
fn handle_with_no_perform_returns_body_value() {
    // The body never performs `Foo::Op`, so the clause is irrelevant.
    // Result should be the body's value.
    let src = "\
effect Foo {
    Bar(x: Int) -> Int,
}
let r = handle {
    42
} with {
    Foo::Bar(x) => resume k with x,
}
r
";
    assert_eq!(run_source(src).unwrap(), Value::Int(42));
}

// ----------------------------------------------------------------------
// 2. Perform with matching handler that resumes — clause runs, body resumes.
// ----------------------------------------------------------------------
#[test]
fn perform_with_resume_threads_value_through_body() {
    // The body performs once. The clause resumes with a constant. The
    // body uses the resumed value as the handle expression's result.
    let src = "\
effect Config {
    Get(key: Str) -> Str,
}
let host = handle {
    perform Config::Get(key: \"db.host\")
} with {
    Config::Get(key) => resume k with \"localhost\",
}
host
";
    assert_eq!(
        run_source(src).unwrap(),
        Value::Str("localhost".to_string())
    );
}

// ----------------------------------------------------------------------
// 3. Perform inside a called function — perform site is one frame above
//    the handler-installing frame. Continuation must capture that frame.
// ----------------------------------------------------------------------
#[test]
fn perform_inside_called_fn_resumes_correctly() {
    let src = "\
effect Config {
    Get(key: Str) -> Str,
}
fn read_host() -> Str {
    perform Config::Get(key: \"db.host\")
}
let host = handle {
    read_host()
} with {
    Config::Get(key) => resume k with \"localhost\",
}
host
";
    assert_eq!(
        run_source(src).unwrap(),
        Value::Str("localhost".to_string())
    );
}

// ----------------------------------------------------------------------
// 4. No-resume clause — clause's value flows out as the handle result,
//    body's "rest" is discarded.
// ----------------------------------------------------------------------
#[test]
fn perform_without_resume_yields_clause_value() {
    // The clause does NOT resume — its return value is the handle result.
    // The body's `+ 1000` after the perform is discarded.
    let src = "\
effect Foo {
    Trigger(x: Int) -> Int,
}
let r = handle {
    perform Foo::Trigger(x: 7) + 1000
} with {
    Foo::Trigger(x) => x * 10,
}
r
";
    assert_eq!(run_source(src).unwrap(), Value::Int(70));
}

// ----------------------------------------------------------------------
// 5. UnhandledEffect — perform outside any handler produces a runtime
//    error with the right tags.
// ----------------------------------------------------------------------
#[test]
fn perform_outside_handler_raises_unhandled_effect() {
    let src = "\
effect Foo {
    Bar(x: Int) -> Int,
}
perform Foo::Bar(x: 1)
";
    let err = run_source(src).unwrap_err();
    match err {
        RuntimeError::UnhandledEffect { .. } => {}
        other => panic!("expected UnhandledEffect, got {other:?}"),
    }
}

// ----------------------------------------------------------------------
// 6. Double-resume — exercised at the VM layer directly.
//
//    The user-facing language statically rejects double-resume inside
//    `ferric_effects`, so we synthesise bytecode here to drive the
//    VM's runtime guard directly. The clause leaks an extra copy of the
//    continuation handle onto the operand stack via `Dup`, the first
//    `Resume` succeeds (consuming the slab slot), and the body — which
//    inherits that stale handle on its restored operand stack —
//    triggers a second `Resume` against the already-consumed handle.
// ----------------------------------------------------------------------
#[test]
fn resuming_a_continuation_twice_errors() {
    use ferric_common::{Chunk, Constant, HandlerEntry, HandlerTable, Op, Program, Symbol};

    // Program:
    //
    //   chunk 0 (entry):
    //     PushHandler(0, body_end_offset = 6)
    //     Perform(0, 0, 0)        ; clause runs, returns via Resume → ip 2 here
    //     Pop                     ; drop the resumed value (Int 10)
    //     LoadConst(Int 99)       ; new value
    //     Resume                  ; pops 99, pops the leaked Continuation
    //     Pop                     ; (unreachable: Resume must error)
    //     PopHandler
    //     Return
    //
    //   chunk 1 (clause):
    //     LoadSlot(0)             ; load continuation
    //     Dup                     ; leak a second copy onto the value stack
    //     LoadConst(Int 10)       ; value to resume with
    //     Resume                  ; pops 10 + matched Cont; transfers to body
    //     Return                  ; (dead code)
    //
    // After the first Resume:
    //   - the slab slot is `None`
    //   - the body's stack on restore is `[dup_cont, 10]`
    //   - body executes `Pop; LoadConst(99); Resume` → pops 99 then
    //     `dup_cont`; the slab lookup hits `None` → `ResumedTwice`.
    let mut entry = Chunk::new(Symbol::new(0));
    entry.constants.push(Constant::Int(99));
    entry.code = vec![
        Op::PushHandler(0, 6),
        Op::Perform(0, 0, 0),
        Op::Pop,
        Op::LoadConst(0),
        Op::Resume,
        Op::Pop,
        Op::PopHandler,
        Op::Return,
    ];

    let mut clause = Chunk::new(Symbol::new(0));
    clause.constants.push(Constant::Int(10));
    clause.code = vec![
        Op::LoadSlot(0),
        Op::Dup,
        Op::LoadConst(0),
        Op::Resume,
        Op::Return,
    ];

    let table = HandlerTable {
        entries: vec![HandlerEntry {
            effect_tag: 0,
            op_tag: 0,
            clause_chunk_idx: 1,
        }],
    };
    let program = Program::with_handler_tables(vec![entry, clause], 0, vec![table]);

    let interner = Interner::new();
    let natives = NativeRegistry::new();
    let mut vm = BytecodeVM::new();
    let err = vm.run(program, natives, &interner).unwrap_err();
    match err {
        RuntimeError::ResumedTwice { .. } => {}
        other => panic!("expected ResumedTwice, got {other:?}"),
    }
}

// ----------------------------------------------------------------------
// 7. Nested handlers — inner handle catches its own effect; outer
//    handle catches a different one.
// ----------------------------------------------------------------------
#[test]
fn nested_handlers_dispatch_by_effect() {
    let src = "\
effect Foo {
    A() -> Int,
}
effect Bar {
    B() -> Int,
}
let r = handle {
    handle {
        perform Foo::A() + perform Bar::B()
    } with {
        Foo::A() => resume k with 3,
    }
} with {
    Bar::B() => resume k with 4,
}
r
";
    assert_eq!(run_source(src).unwrap(), Value::Int(7));
}
