//! # Ferric Trait Registry Builder (M5)
//!
//! Walks `ParseResult` and `ResolveResult` to build a `TraitRegistry`:
//! a flat lookup table of trait declarations, plus impl bindings keyed by
//! `(trait_name, type_key) -> ImplDef`.
//!
//! Per the architectural rules this stage reads only `ParseResult` and
//! `ResolveResult` from `ferric_common` and produces a `TraitRegistry`
//! (also defined in `ferric_common`). Downstream stages (`ferric_infer`
//! and `ferric_compiler`) consume the registry through the common type.
//!
//! Coercion task 2 added an optional pre-seeded "built-in" registry passed
//! by stdlib (containing the `To<T>` trait and its built-in impls), and an
//! orphan-rule pass that flags user impls of stdlib-owned types/traits.

use ferric_common::{
    ImplDef, ImplTy, Interner, Item, MethodSignature, ParseResult, ResolveError, ResolveResult,
    Symbol, TraitDef, TraitOrigin, TraitRegistry, Ty, TypeAnnotation,
};

/// Output of the trait stage. Returned in addition to the registry so
/// orphan-rule diagnostics can be merged into the resolver's error stream
/// before rendering.
#[derive(Debug, Clone, Default)]
pub struct TraitsResult {
    pub registry: TraitRegistry,
    /// Orphan-rule violations on user impls. Append to `ResolveResult::errors`
    /// before reporting so they render through the standard pipeline.
    pub errors: Vec<ResolveError>,
}

/// Builds a trait registry from parser + resolver output.
///
/// `seeded` is an already-populated registry containing built-in traits and
/// impls (typically produced by `ferric_stdlib::register_builtin_traits`).
/// User-defined entries from the AST are appended on top.
///
/// Trait bodies must be type-only — each method has a signature but no
/// body. Impl bodies are concrete functions whose `DefId`s the resolver
/// has already assigned.
pub fn build_registry_seeded(
    ast: &ParseResult,
    resolve: &ResolveResult,
    interner: &Interner,
    seeded: TraitRegistry,
) -> TraitsResult {
    let mut registry = seeded;
    let mut errors: Vec<ResolveError> = Vec::new();

    // Pass 1: collect every user trait declaration. (Stdlib traits are already
    // in the seeded registry.)
    for item in &ast.items {
        if let Item::TraitDef {
            name,
            type_params,
            methods,
            ..
        } = item
        {
            let mut method_table = std::collections::HashMap::new();
            for m in methods {
                let params: Vec<Ty> = m
                    .params
                    .iter()
                    .map(|p| convert_annotation(&p.ty, *name, interner, resolve))
                    .collect();
                let ret = convert_annotation(&m.ret_ty, *name, interner, resolve);
                method_table.insert(m.name, MethodSignature { params, ret });
            }
            registry.traits.insert(
                *name,
                TraitDef {
                    name: *name,
                    type_params: type_params.iter().map(|p| p.name).collect(),
                    methods: method_table,
                    origin: TraitOrigin::User,
                },
            );
        }
    }

    // Pass 2: collect every user impl block, applying the orphan rule to
    // each one as it is registered.
    for item in &ast.items {
        if let Item::ImplBlock {
            trait_name,
            trait_args,
            type_name,
            methods,
            span,
            ..
        } = item
        {
            let for_type = match impl_ty_for_name(*type_name, interner, resolve) {
                Some(t) => t,
                None => continue,
            };

            let arg_keys: Vec<ImplTy> = trait_args
                .iter()
                .filter_map(|ann| impl_ty_for_annotation(ann, interner, resolve))
                .collect();

            // Orphan-rule check: a user impl is allowed only if at least one
            // of (trait, source, target args) is owned by user code. Built-in
            // traits/impls are always stdlib-owned and skipped here.
            let trait_is_user = registry
                .traits
                .get(trait_name)
                .map(|t| t.origin == TraitOrigin::User)
                .unwrap_or(true);
            let source_is_user = is_user_owned(&for_type);
            let any_arg_is_user = arg_keys.iter().any(is_user_owned);

            if !trait_is_user && !source_is_user && !any_arg_is_user {
                let target_ty = trait_args
                    .first()
                    .and_then(|ann| ty_for_annotation(ann, interner, resolve))
                    .unwrap_or(Ty::Unit);
                errors.push(ResolveError::OrphanImpl {
                    trait_name: *trait_name,
                    source: ty_for_impl_ty(&for_type, *type_name),
                    target: target_ty,
                    span: *span,
                });
            }

            let mut method_def_ids = std::collections::HashMap::new();
            for m in methods {
                if let Some(def_id) = resolve.method_def_ids.get(&m.id) {
                    method_def_ids.insert(m.name, *def_id);
                }
            }

            registry.insert_impl(ImplDef {
                trait_name: *trait_name,
                for_type,
                trait_args: arg_keys,
                methods: method_def_ids,
                origin: TraitOrigin::User,
            });
        }
    }

    TraitsResult { registry, errors }
}

/// Backwards-compatible entry point: builds a registry with no built-in
/// traits seeded. Callers that don't care about `To<T>` (e.g. unit tests)
/// can keep using this; production callers should prefer
/// [`build_registry_seeded`].
pub fn build_registry(
    ast: &ParseResult,
    resolve: &ResolveResult,
    interner: &Interner,
) -> TraitRegistry {
    build_registry_seeded(ast, resolve, interner, TraitRegistry::new()).registry
}

/// Returns true if `ty` is owned by user code (a struct/enum declared in
/// the program being compiled). Built-in primitives and `ShellOutput` are
/// stdlib-owned and never qualify.
fn is_user_owned(ty: &ImplTy) -> bool {
    matches!(ty, ImplTy::Struct(_) | ImplTy::Enum(_))
}

/// Best-effort `Ty` reconstruction from an `ImplTy`. Used to fill the
/// `source` field of `OrphanImpl`. We don't have the original `Ty` on hand
/// and don't need full fidelity — the renderer only reads its name.
fn ty_for_impl_ty(it: &ImplTy, fallback_name: Symbol) -> Ty {
    match it {
        ImplTy::Int => Ty::Int,
        ImplTy::Float => Ty::Float,
        ImplTy::Bool => Ty::Bool,
        ImplTy::Str => Ty::Str,
        ImplTy::Unit => Ty::Unit,
        ImplTy::ShellOutput => Ty::ShellOutput,
        ImplTy::Struct(def_id) => Ty::Struct {
            def_id: *def_id,
            name: fallback_name,
            fields: Vec::new(),
        },
        ImplTy::Enum(def_id) => Ty::Enum {
            def_id: *def_id,
            name: fallback_name,
            variants: Vec::new(),
        },
        ImplTy::Tuple(n) => Ty::Tuple(vec![Ty::Unit; *n]),
    }
}

/// Best-effort `Ty` from an annotation. Used to populate `OrphanImpl.target`.
fn ty_for_annotation(
    ann: &TypeAnnotation,
    interner: &Interner,
    resolve: &ResolveResult,
) -> Option<Ty> {
    match ann {
        TypeAnnotation::Named(sym) => {
            let name = interner.resolve(*sym);
            Some(match name {
                "Int" => Ty::Int,
                "Float" => Ty::Float,
                "Bool" => Ty::Bool,
                "Str" => Ty::Str,
                "Unit" | "" => Ty::Unit,
                "ShellOutput" => Ty::ShellOutput,
                _ => {
                    let def_id = *resolve.type_defs.get(sym)?;
                    if resolve.struct_fields.contains_key(&def_id) {
                        Ty::Struct {
                            def_id,
                            name: *sym,
                            fields: Vec::new(),
                        }
                    } else if resolve.enum_variants.contains_key(&def_id) {
                        Ty::Enum {
                            def_id,
                            name: *sym,
                            variants: Vec::new(),
                        }
                    } else {
                        return None;
                    }
                }
            })
        }
        _ => None,
    }
}

/// Best-effort `ImplTy` from an annotation. Used to compute trait_args
/// keys for impl insertion.
fn impl_ty_for_annotation(
    ann: &TypeAnnotation,
    interner: &Interner,
    resolve: &ResolveResult,
) -> Option<ImplTy> {
    let ty = ty_for_annotation(ann, interner, resolve)?;
    ImplTy::from_ty(&ty)
}

/// Resolves a type annotation into a concrete `Ty` for use in a trait
/// method signature. The trait-method `self` parameter is encoded as
/// `Ty::Var` of a sentinel TyVar (id 0); the inferencer unifies it with
/// the actual receiver type at the call site.
fn convert_annotation(
    ann: &TypeAnnotation,
    self_trait: Symbol,
    interner: &Interner,
    resolve: &ResolveResult,
) -> Ty {
    match ann {
        TypeAnnotation::Named(sym) => {
            let name = interner.resolve(*sym);
            match name {
                "Int" => Ty::Int,
                "Float" => Ty::Float,
                "Bool" => Ty::Bool,
                "Str" => Ty::Str,
                "Unit" | "" => Ty::Unit,
                "ShellOutput" => Ty::ShellOutput,
                _ => {
                    // Treat the trait's own name and "Self" as the self type
                    // — a sentinel TyVar that the inferencer unifies with the
                    // actual receiver type. Using TyVar(0) is fine because
                    // every signature is rebuilt at each call site.
                    if *sym == self_trait || name == "Self" {
                        return Ty::Var(ferric_common::TyVar(0));
                    }
                    // Look up user-defined struct/enum types.
                    if let Some(def_id) = resolve.type_defs.get(sym) {
                        if let Some(fields) = resolve.struct_fields.get(def_id) {
                            let resolved_fields: Vec<(Symbol, Ty)> = fields
                                .iter()
                                .map(|(n, ty)| {
                                    (*n, convert_annotation(ty, self_trait, interner, resolve))
                                })
                                .collect();
                            return Ty::Struct {
                                def_id: *def_id,
                                name: *sym,
                                fields: resolved_fields,
                            };
                        }
                        if let Some(variants) = resolve.enum_variants.get(def_id) {
                            let resolved_variants: Vec<(Symbol, Vec<Ty>)> = variants
                                .iter()
                                .map(|(vn, payload)| {
                                    (
                                        *vn,
                                        payload
                                            .iter()
                                            .map(|t| {
                                                convert_annotation(t, self_trait, interner, resolve)
                                            })
                                            .collect(),
                                    )
                                })
                                .collect();
                            return Ty::Enum {
                                def_id: *def_id,
                                name: *sym,
                                variants: resolved_variants,
                            };
                        }
                    }
                    // Unknown name — treat as a generic var.
                    Ty::Var(ferric_common::TyVar(0))
                }
            }
        }
        TypeAnnotation::Array(inner) => Ty::Array(Box::new(convert_annotation(
            inner, self_trait, interner, resolve,
        ))),
        TypeAnnotation::Generic { head, args } => {
            let name = interner.resolve(*head);
            match (name, args.as_slice()) {
                ("Option", [inner]) => Ty::Option(Box::new(convert_annotation(
                    inner, self_trait, interner, resolve,
                ))),
                ("Result", [ok, err]) => Ty::Result(
                    Box::new(convert_annotation(ok, self_trait, interner, resolve)),
                    Box::new(convert_annotation(err, self_trait, interner, resolve)),
                ),
                _ => Ty::Var(ferric_common::TyVar(0)),
            }
        }
        TypeAnnotation::Eff { row, result } => {
            let effects = row
                .iter()
                .map(|e| ferric_common::EffectRef {
                    name: e.name,
                    args: e
                        .args
                        .iter()
                        .map(|a| convert_annotation(a, self_trait, interner, resolve))
                        .collect(),
                })
                .collect();
            Ty::Eff(
                effects,
                Box::new(convert_annotation(result, self_trait, interner, resolve)),
            )
        }
        TypeAnnotation::Infer => Ty::Var(ferric_common::TyVar(0)),
    }
}

/// Maps a Symbol naming an impl's "for type" to a registry-level `ImplTy`.
fn impl_ty_for_name(name: Symbol, interner: &Interner, resolve: &ResolveResult) -> Option<ImplTy> {
    let s = interner.resolve(name);
    Some(match s {
        "Int" => ImplTy::Int,
        "Float" => ImplTy::Float,
        "Bool" => ImplTy::Bool,
        "Str" => ImplTy::Str,
        "Unit" | "" => ImplTy::Unit,
        "ShellOutput" => ImplTy::ShellOutput,
        _ => {
            let def_id = *resolve.type_defs.get(&name)?;
            if resolve.struct_fields.contains_key(&def_id) {
                ImplTy::Struct(def_id)
            } else if resolve.enum_variants.contains_key(&def_id) {
                ImplTy::Enum(def_id)
            } else {
                return None;
            }
        }
    })
}
