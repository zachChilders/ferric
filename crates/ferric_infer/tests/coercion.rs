//! End-to-end tests for Coercion task 3: implicit `To<T>` coercion at call
//! argument positions. Drives the full lex → parse → resolve → traits →
//! typecheck pipeline so the coercion lookup runs against the real registry
//! seeded by `ferric_stdlib::register_builtin_traits`.

use ferric_common::{DefId, Interner, Span, Symbol, TraitRegistry, Ty, TypeError, TypeResult};
use ferric_infer::typecheck;
use ferric_lexer::lex;
use ferric_parser::{parse_with_interner, pre_intern_desugar_names};
use ferric_resolve::resolve_with_natives_and_builtins;
use ferric_stdlib::{
    NativeRegistry, async_intrinsic_param_table, builtin_enum_table, register_builtin_traits,
    register_stdlib,
};
use ferric_traits::build_registry_seeded;

/// Runs the full pipeline up through type inference, returning everything a
/// test might want to inspect. `interner` is retained even when individual
/// tests don't read it because every Symbol in `type_result` resolves
/// through it; future assertions that touch names will need it.
#[allow(dead_code)]
struct PipelineOutput {
    type_result: TypeResult,
    interner: Interner,
    registry: TraitRegistry,
}

fn run_pipeline(source: &str) -> PipelineOutput {
    let mut interner = Interner::new();
    let mut natives = NativeRegistry::new();
    register_stdlib(&mut natives, &mut interner);

    let mut native_fns = natives.fn_table();
    native_fns.extend(async_intrinsic_param_table(&mut interner));
    let builtin_enums = builtin_enum_table(&mut interner);

    let lex_result = lex(source, &mut interner);
    pre_intern_desugar_names(&mut interner);
    let parse_result = parse_with_interner(&lex_result, &interner);
    let resolve_result =
        resolve_with_natives_and_builtins(&parse_result, &native_fns, &builtin_enums);

    let seeded = register_builtin_traits(&mut interner, &resolve_result);
    let traits_result = build_registry_seeded(&parse_result, &resolve_result, &interner, seeded);

    let type_result = typecheck(
        &parse_result,
        &resolve_result,
        &interner,
        &traits_result.registry,
    );
    PipelineOutput {
        type_result,
        interner,
        registry: traits_result.registry,
    }
}

/// Returns the `DefId` of the `to` impl method registered for the
/// `(source, target)` pair. Panics if the pair has zero or multiple impls;
/// every test that uses it expects the unique stdlib impl.
fn unique_to_def_id(registry: &TraitRegistry, source: &Ty, target: &Ty) -> DefId {
    let candidates = registry.find_to_impls(source, target);
    assert_eq!(
        candidates.len(),
        1,
        "expected a single To<{target:?}> for {source:?} impl, got {candidates:?}"
    );
    candidates[0]
}

/// Filters `errors` to those of `TypeError::Mismatch`. Useful where a test
/// fixture intentionally provokes a single mismatch and we want to ignore
/// any incidental `CannotInfer` resulting from the same mistake.
fn mismatches(errors: &[TypeError]) -> Vec<&TypeError> {
    errors
        .iter()
        .filter(|e| matches!(e, TypeError::Mismatch { .. }))
        .collect()
}

// ============================================================================
// Successful coercions
// ============================================================================

#[test]
fn int_to_float_coerces_at_call_site() {
    // `int_to_float` itself takes Int; we call a user fn that takes Float
    // with an Int literal, exercising the coercion lookup.
    let source = r#"
        fn print_float(x: Float) -> Float { x }
        let result = print_float(x: 5)
    "#;
    let out = run_pipeline(source);
    assert!(
        out.type_result.errors.is_empty(),
        "expected no type errors with Int→Float coercion, got {:?}",
        out.type_result.errors,
    );
    assert_eq!(
        out.type_result.coercions.len(),
        1,
        "expected exactly one coercion entry, got {:?}",
        out.type_result.coercions,
    );
    let expected_def = unique_to_def_id(&out.registry, &Ty::Int, &Ty::Float);
    let (_, def_id) = out
        .type_result
        .coercions
        .iter()
        .next()
        .expect("at least one coercion entry");
    assert_eq!(*def_id, expected_def);
}

#[test]
fn shellout_to_str_coerces_at_call_site() {
    // `$ "echo hi"` is a shell expression yielding ShellOutput. `print_str`
    // takes Str — coercion via `To<Str> for ShellOutput` should fire.
    let source = r#"
        fn print_str(s: Str) -> Str { s }
        let result = print_str(s: $ "echo hi")
    "#;
    let out = run_pipeline(source);
    assert!(
        out.type_result.errors.is_empty(),
        "expected no type errors with ShellOutput→Str coercion, got {:?}",
        out.type_result.errors,
    );
    assert_eq!(out.type_result.coercions.len(), 1);
    let expected_def = unique_to_def_id(&out.registry, &Ty::ShellOutput, &Ty::Str);
    let (_, def_id) = out
        .type_result
        .coercions
        .iter()
        .next()
        .expect("at least one coercion entry");
    assert_eq!(*def_id, expected_def);
}

#[test]
fn matching_arg_does_not_record_coercion() {
    // Sanity check: when the argument already matches, no coercion is
    // recorded and no error is emitted.
    let source = r#"
        fn id_int(x: Int) -> Int { x }
        let r = id_int(x: 7)
    "#;
    let out = run_pipeline(source);
    assert!(out.type_result.errors.is_empty());
    assert!(
        out.type_result.coercions.is_empty(),
        "expected no coercions for direct-match call, got {:?}",
        out.type_result.coercions,
    );
}

// ============================================================================
// Sites where coercion is NOT attempted
// ============================================================================

#[test]
fn let_annotation_int_to_float_is_mismatch() {
    // Assignment to a type-annotated `let` is NOT a call boundary —
    // coercion must NOT fire. The mismatch surfaces as a normal
    // TypeError::Mismatch, just like before Coercion task 3.
    let source = r#"
        let x: Float = 5
    "#;
    let out = run_pipeline(source);
    let mismatches = mismatches(&out.type_result.errors);
    assert!(
        !mismatches.is_empty(),
        "expected Mismatch on `let x: Float = 5`, got {:?}",
        out.type_result.errors,
    );
    assert!(
        out.type_result.coercions.is_empty(),
        "let-annotation must not trigger a coercion, got {:?}",
        out.type_result.coercions,
    );
    // Every error variant must NOT be `AmbiguousCoercion` either —
    // ambiguity belongs only to the call-site path.
    assert!(
        !out.type_result
            .errors
            .iter()
            .any(|e| matches!(e, TypeError::AmbiguousCoercion { .. })),
        "let-annotation must not surface AmbiguousCoercion",
    );
}

#[test]
fn return_int_in_float_fn_is_mismatch() {
    // The return-position type mismatch must remain an ordinary
    // TypeError::Mismatch — no coercion at function returns.
    let source = r#"
        fn f() -> Float { 5 }
    "#;
    let out = run_pipeline(source);
    let mismatches = mismatches(&out.type_result.errors);
    assert!(
        !mismatches.is_empty(),
        "expected Mismatch on `fn f() -> Float {{ 5 }}`, got {:?}",
        out.type_result.errors,
    );
    assert!(
        out.type_result.coercions.is_empty(),
        "function return must not record a coercion, got {:?}",
        out.type_result.coercions,
    );
}

// ============================================================================
// Lookup miss (no impl)
// ============================================================================

#[test]
fn no_to_impl_falls_back_to_type_mismatch() {
    // No `To<Str> for Int` impl exists, so the call must error out as a
    // plain TypeError::Mismatch, not a coercion success or
    // AmbiguousCoercion.
    let source = r#"
        fn print_str(s: Str) -> Str { s }
        let r = print_str(s: 5)
    "#;
    let out = run_pipeline(source);
    let mismatches = mismatches(&out.type_result.errors);
    assert!(
        !mismatches.is_empty(),
        "expected Mismatch when no To impl is registered, got {:?}",
        out.type_result.errors,
    );
    assert!(
        out.type_result.coercions.is_empty(),
        "expected no coercion entries when no impl is registered",
    );
}

#[test]
fn no_chain_through_intermediate_type() {
    // Even though Int→Float is registered, neither Float→Str nor Int→Str
    // is, so calling `print_str(s: 5.0)` must NOT silently chain
    // Float→Int→Str (or any other multi-hop). It must surface as a plain
    // mismatch.
    let source = r#"
        fn print_str(s: Str) -> Str { s }
        let r = print_str(s: 5.0)
    "#;
    let out = run_pipeline(source);
    let mismatches = mismatches(&out.type_result.errors);
    assert!(
        !mismatches.is_empty(),
        "expected Mismatch (no chaining), got {:?}",
        out.type_result.errors,
    );
    assert!(out.type_result.coercions.is_empty());
}

// ============================================================================
// Ambiguity
// ============================================================================

#[test]
fn ambiguous_coercion_emits_ambiguous_error() {
    // The runtime registry rejects duplicate impls only via the orphan rule,
    // not via coherence — two `To<Float> for Int` entries can both live in
    // the same bucket. To exercise the ambiguity path, we hand-build a
    // registry with two impls for the same `(source, target)` pair and run
    // the type checker directly against it. (We can't write user code that
    // declares a duplicate without tripping the orphan rule, since
    // both Int and Float are stdlib-owned.)

    // Build a minimal pipeline through resolve so we have a baseline AST.
    let source = r#"
        fn print_float(x: Float) -> Float { x }
        let r = print_float(x: 5)
    "#;
    let mut interner = Interner::new();
    let mut natives = NativeRegistry::new();
    register_stdlib(&mut natives, &mut interner);
    let mut native_fns = natives.fn_table();
    native_fns.extend(async_intrinsic_param_table(&mut interner));
    let builtin_enums = builtin_enum_table(&mut interner);
    let lex_result = lex(source, &mut interner);
    pre_intern_desugar_names(&mut interner);
    let parse_result = parse_with_interner(&lex_result, &interner);
    let resolve_result =
        resolve_with_natives_and_builtins(&parse_result, &native_fns, &builtin_enums);

    // Build the trait registry, then add a SECOND `To<Float> for Int` impl
    // pointing at any plausible DefId. The second impl is a sentinel —
    // we never run the program; we only verify the type checker rejects
    // ambiguity.
    let seeded = register_builtin_traits(&mut interner, &resolve_result);
    let traits_result = build_registry_seeded(&parse_result, &resolve_result, &interner, seeded);
    let mut registry = traits_result.registry;

    // Find the existing Int→Float impl and clone it so we can install a
    // second `to`-method DefId for the same (source, target) bucket.
    let existing = registry.find_to_impls(&Ty::Int, &Ty::Float);
    assert_eq!(existing.len(), 1, "expected one stdlib Int→Float impl");
    let first_def = existing[0];
    // Synthesise a distinct DefId. The compiler never actually reads it —
    // the diagnostic only needs the value to differ from `first_def`.
    let phony_def = DefId::new(first_def.0.saturating_add(1_000_000));

    let to_trait = registry
        .to_trait
        .expect("To trait should be registered by stdlib");
    let to_method = registry
        .to_method
        .expect("to method should be registered by stdlib");
    let mut method_def_ids: std::collections::HashMap<Symbol, DefId> =
        std::collections::HashMap::new();
    method_def_ids.insert(to_method, phony_def);
    registry.insert_impl(ferric_common::ImplDef {
        trait_name: to_trait,
        for_type: ferric_common::ImplTy::Int,
        trait_args: vec![ferric_common::ImplTy::Float],
        methods: method_def_ids,
        origin: ferric_common::TraitOrigin::User,
    });
    assert_eq!(
        registry.find_to_impls(&Ty::Int, &Ty::Float).len(),
        2,
        "fixture must register two Int→Float impls",
    );

    let type_result = typecheck(&parse_result, &resolve_result, &interner, &registry);

    let ambiguous: Vec<&TypeError> = type_result
        .errors
        .iter()
        .filter(|e| matches!(e, TypeError::AmbiguousCoercion { .. }))
        .collect();
    assert_eq!(
        ambiguous.len(),
        1,
        "expected one AmbiguousCoercion, got {:?}",
        type_result.errors,
    );
    if let TypeError::AmbiguousCoercion {
        candidates,
        found,
        expected,
        ..
    } = ambiguous[0]
    {
        assert_eq!(candidates.len(), 2);
        assert_eq!(found, &Ty::Int);
        assert_eq!(expected, &Ty::Float);
    } else {
        unreachable!();
    }
    // No coercion recorded because the lookup was ambiguous.
    assert!(
        type_result.coercions.is_empty(),
        "ambiguous coercion must not record any chosen impl",
    );
}

// ============================================================================
// Sanity: errors carry spans (architectural rule)
// ============================================================================

#[test]
fn ambiguous_coercion_error_carries_span() {
    // Same setup as `ambiguous_coercion_emits_ambiguous_error` but only
    // checks the architectural invariant: every TypeError has a non-zero
    // span pointing at the offending argument.
    let source = "fn f(x: Float) -> Float { x }\nlet r = f(x: 5)\n";
    let mut interner = Interner::new();
    let mut natives = NativeRegistry::new();
    register_stdlib(&mut natives, &mut interner);
    let mut native_fns = natives.fn_table();
    native_fns.extend(async_intrinsic_param_table(&mut interner));
    let builtin_enums = builtin_enum_table(&mut interner);
    let lex_result = lex(source, &mut interner);
    pre_intern_desugar_names(&mut interner);
    let parse_result = parse_with_interner(&lex_result, &interner);
    let resolve_result =
        resolve_with_natives_and_builtins(&parse_result, &native_fns, &builtin_enums);
    let seeded = register_builtin_traits(&mut interner, &resolve_result);
    let traits_result = build_registry_seeded(&parse_result, &resolve_result, &interner, seeded);
    let mut registry = traits_result.registry;

    let to_trait = registry.to_trait.unwrap();
    let to_method = registry.to_method.unwrap();
    let mut method_def_ids: std::collections::HashMap<Symbol, DefId> =
        std::collections::HashMap::new();
    method_def_ids.insert(to_method, DefId::new(99_999));
    registry.insert_impl(ferric_common::ImplDef {
        trait_name: to_trait,
        for_type: ferric_common::ImplTy::Int,
        trait_args: vec![ferric_common::ImplTy::Float],
        methods: method_def_ids,
        origin: ferric_common::TraitOrigin::User,
    });

    let type_result = typecheck(&parse_result, &resolve_result, &interner, &registry);
    let amb = type_result
        .errors
        .iter()
        .find(|e| matches!(e, TypeError::AmbiguousCoercion { .. }))
        .expect("expected AmbiguousCoercion error");
    let span: Span = amb.span();
    assert!(
        span.end > span.start,
        "AmbiguousCoercion span must be non-empty: {span:?}",
    );
    // The "5" literal lies on line 2; sanity-check the span resolves to
    // something inside the source.
    assert!(
        (span.end as usize) <= source.len(),
        "span must lie inside source (len {})",
        source.len(),
    );
}
