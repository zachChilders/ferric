//! End-to-end tests for the Coercion task 2 orphan rule and built-in
//! `To<T>` registration. Drives the full lex → parse → resolve → traits
//! pipeline so we exercise the same paths `main.rs` does.

use ferric_common::{Interner, ResolveError};
use ferric_lexer::lex;
use ferric_parser::{parse_with_interner, pre_intern_desugar_names};
use ferric_resolve::resolve_with_natives_and_builtins;
use ferric_stdlib::{
    NativeRegistry, TO_METHOD_NAME, TO_TRAIT_NAME, async_intrinsic_param_table, builtin_enum_table,
    register_builtin_traits, register_stdlib,
};
use ferric_traits::build_registry_seeded;

fn run_pipeline(source: &str) -> (Vec<ResolveError>, ferric_common::TraitRegistry, Interner) {
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

    (traits_result.errors, traits_result.registry, interner)
}

#[test]
fn builtin_to_trait_is_registered() {
    let (errors, registry, mut interner) = run_pipeline("");
    assert!(errors.is_empty(), "no orphan errors expected: {errors:?}");

    let to_sym = interner.intern(TO_TRAIT_NAME);
    let to_method = interner.intern(TO_METHOD_NAME);
    assert_eq!(registry.to_trait, Some(to_sym));
    assert_eq!(registry.to_method, Some(to_method));
    assert!(
        registry.traits.contains_key(&to_sym),
        "stdlib `To` trait must be in the registry"
    );
}

#[test]
fn builtin_to_int_to_float_impl_is_registered() {
    let (_errors, registry, _interner) = run_pipeline("");
    let int_to_float = registry.find_to_impls(&ferric_common::Ty::Int, &ferric_common::Ty::Float);
    assert_eq!(
        int_to_float.len(),
        1,
        "expected exactly one `To<Float> for Int` impl, got {int_to_float:?}",
    );
}

#[test]
fn builtin_to_shellout_to_str_impl_is_registered() {
    let (_errors, registry, _interner) = run_pipeline("");
    let shell_to_str =
        registry.find_to_impls(&ferric_common::Ty::ShellOutput, &ferric_common::Ty::Str);
    assert_eq!(
        shell_to_str.len(),
        1,
        "expected exactly one `To<Str> for ShellOutput` impl, got {shell_to_str:?}",
    );
}

#[test]
fn user_impl_to_float_for_int_is_orphan() {
    // Neither `Int` nor `Float` is owned by user code, and `To` is stdlib —
    // every component is foreign, so this must trigger the orphan rule.
    let source = r#"
        impl To<Float> for Int {
            fn to(self) -> Float { (self).to() }
        }
    "#;
    let (errors, _registry, _interner) = run_pipeline(source);
    let orphan_count = errors
        .iter()
        .filter(|e| matches!(e, ResolveError::OrphanImpl { .. }))
        .count();
    assert_eq!(
        orphan_count, 1,
        "expected one OrphanImpl error, got {errors:?}"
    );
}

#[test]
fn user_impl_to_user_struct_for_int_is_allowed() {
    // The user owns `MyStruct`, so the orphan rule accepts the impl.
    let source = r#"
        struct MyStruct {
            n: Int
        }

        impl To<MyStruct> for Int {
            fn to(self) -> MyStruct { MyStruct { n: self } }
        }
    "#;
    let (errors, registry, mut interner) = run_pipeline(source);
    let orphan_errors: Vec<_> = errors
        .iter()
        .filter(|e| matches!(e, ResolveError::OrphanImpl { .. }))
        .collect();
    assert!(
        orphan_errors.is_empty(),
        "user-owned target should not trigger orphan rule: {orphan_errors:?}",
    );

    // The user impl should also be discoverable through find_to_impls.
    let to_sym = interner.intern(TO_TRAIT_NAME);
    let int_key = ferric_common::ImplTy::Int;
    let bucket = registry
        .impls
        .get(&(to_sym, int_key))
        .expect("expected at least one impl bucket for To/Int");
    // Stdlib impl + user impl => 2 entries.
    assert_eq!(
        bucket.len(),
        2,
        "expected 2 To/Int impls (stdlib + user), got {bucket:?}",
    );
}

#[test]
fn user_impl_for_user_struct_is_allowed() {
    // The user owns `MyStruct`; even though `Int` is stdlib, the source type
    // being user-owned satisfies coherence.
    let source = r#"
        struct MyStruct { n: Int }

        impl To<Int> for MyStruct {
            fn to(self) -> Int { self.n }
        }
    "#;
    let (errors, _registry, _interner) = run_pipeline(source);
    let orphan_errors: Vec<_> = errors
        .iter()
        .filter(|e| matches!(e, ResolveError::OrphanImpl { .. }))
        .collect();
    assert!(
        orphan_errors.is_empty(),
        "user-owned source should satisfy the orphan rule: {orphan_errors:?}",
    );
}
