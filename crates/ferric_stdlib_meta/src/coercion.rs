//! Built-in `To<T>` coercion natives + trait-registry seeding.
//!
//! `ferric_stdlib::register_stdlib` registers two synthetic natives that
//! back the stdlib's built-in `To<T>` impls (`To<Float> for Int` and
//! `To<Str> for ShellOutput`). The LSP doesn't link `ferric_stdlib` (which
//! pulls heavy runtime deps), so the names — and the matching trait
//! registry entries the type checker needs to find them — live here in the
//! `_meta` crate. Both `ferric_stdlib::register_builtin_traits` and the LSP
//! pipeline call [`seed_to_trait`] to install the same registry shape.

use ferric_common::{
    DefId, ImplDef, ImplTy, Interner, MethodSignature, ResolveResult, Symbol, TraitDef,
    TraitOrigin, TraitRegistry, Ty, TyVar,
};

/// Internal name of the built-in `To<Float> for Int` method body.
pub const TO_INT_TO_FLOAT_NATIVE: &str = "__to_int_to_float";
/// Internal name of the built-in `To<Str> for ShellOutput` method body.
pub const TO_SHELLOUT_TO_STR_NATIVE: &str = "__to_shellout_to_str";
/// Symbol name of the built-in `To<T>` trait.
pub const TO_TRAIT_NAME: &str = "To";
/// Method name on the `To<T>` trait.
pub const TO_METHOD_NAME: &str = "to";

/// `(name, params)` entries for every `To<T>` native body. Resolver-facing
/// — added to the native table so each native lands a `DefId` the seeding
/// step can look up.
pub fn to_native_param_table(interner: &mut Interner) -> Vec<(Symbol, Vec<Symbol>)> {
    let self_sym = interner.intern("self");
    vec![
        (interner.intern(TO_INT_TO_FLOAT_NATIVE), vec![self_sym]),
        (interner.intern(TO_SHELLOUT_TO_STR_NATIVE), vec![self_sym]),
    ]
}

/// Builds (or extends) a `TraitRegistry` with the built-in `To<T>` trait
/// declaration and its two stdlib impls. Mirrors
/// `ferric_stdlib::register_builtin_traits` so non-stdlib consumers (notably
/// `ferric_lsp`) can construct the same registry shape without depending on
/// `ferric_stdlib`'s heavy runtime crate.
///
/// The supplied `resolve` must already include the `__to_*` natives in its
/// `defs` map (call [`to_native_param_table`] when constructing the
/// resolver's native fn list); this fn looks the natives up by name to wire
/// each impl's method `DefId`.
pub fn seed_to_trait(interner: &mut Interner, resolve: &ResolveResult) -> TraitRegistry {
    let mut registry = TraitRegistry::new();
    extend_with_to_trait(&mut registry, interner, resolve);
    registry
}

/// Variant of [`seed_to_trait`] that mutates an existing registry — used by
/// `ferric_traits::build_registry_seeded` callers that want stdlib impls
/// merged into a registry assembled from user `impl` blocks.
pub fn extend_with_to_trait(
    registry: &mut TraitRegistry,
    interner: &mut Interner,
    resolve: &ResolveResult,
) {
    let to_trait_sym = interner.intern(TO_TRAIT_NAME);
    let to_method_sym = interner.intern(TO_METHOD_NAME);
    let t_param_sym = interner.intern("T");

    // Trait declaration: `trait To<T> { fn to(self) -> T }`. Encoded with
    // `TyVar(0)` for both `self` and `T` since the registry doesn't yet
    // model named generics; `ferric_infer` substitutes the concrete target
    // when resolving a coercion.
    let mut methods = std::collections::HashMap::new();
    methods.insert(
        to_method_sym,
        MethodSignature {
            params: vec![Ty::Var(TyVar(0))],
            ret: Ty::Var(TyVar(0)),
        },
    );
    registry.traits.insert(
        to_trait_sym,
        TraitDef {
            name: to_trait_sym,
            type_params: vec![t_param_sym],
            methods,
            origin: TraitOrigin::Stdlib,
        },
    );
    registry.to_trait = Some(to_trait_sym);
    registry.to_method = Some(to_method_sym);

    let trait_syms = ToTraitSyms {
        trait_sym: to_trait_sym,
        method_sym: to_method_sym,
    };
    insert_impl(
        registry,
        resolve,
        interner,
        TO_INT_TO_FLOAT_NATIVE,
        ImplTy::Int,
        ImplTy::Float,
        &trait_syms,
    );
    insert_impl(
        registry,
        resolve,
        interner,
        TO_SHELLOUT_TO_STR_NATIVE,
        ImplTy::ShellOutput,
        ImplTy::Str,
        &trait_syms,
    );
}

/// Bundle of the two interned symbols every `To<T>` impl needs at install
/// time. Pulled into its own struct so [`insert_impl`] doesn't trip
/// clippy's `too_many_arguments` lint.
struct ToTraitSyms {
    trait_sym: Symbol,
    method_sym: Symbol,
}

fn insert_impl(
    registry: &mut TraitRegistry,
    resolve: &ResolveResult,
    interner: &mut Interner,
    native_name: &str,
    for_ty: ImplTy,
    target: ImplTy,
    syms: &ToTraitSyms,
) {
    let native_sym = interner.intern(native_name);
    let Some(def_id) = resolve.find_def_by_name(native_sym) else {
        return;
    };
    let mut method_def_ids: std::collections::HashMap<Symbol, DefId> =
        std::collections::HashMap::new();
    method_def_ids.insert(syms.method_sym, def_id);
    registry.insert_impl(ImplDef {
        trait_name: syms.trait_sym,
        for_type: for_ty,
        trait_args: vec![target],
        methods: method_def_ids,
        origin: TraitOrigin::Stdlib,
    });
}
