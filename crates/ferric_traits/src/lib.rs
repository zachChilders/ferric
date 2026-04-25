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

use ferric_common::{
    ImplDef, ImplTy, Interner, Item, MethodSignature, ParseResult, ResolveResult,
    Symbol, TraitDef, TraitRegistry, Ty, TypeAnnotation,
};

/// Builds a trait registry from parser + resolver output.
///
/// Trait bodies must be type-only — each method has a signature but no
/// body. Impl bodies are concrete functions whose `DefId`s the resolver
/// has already assigned.
pub fn build_registry(
    ast: &ParseResult,
    resolve: &ResolveResult,
    interner: &Interner,
) -> TraitRegistry {
    let mut registry = TraitRegistry::new();

    // Pass 1: collect every trait declaration.
    for item in &ast.items {
        if let Item::TraitDef { name, methods, .. } = item {
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
                    methods: method_table,
                },
            );
        }
    }

    // Pass 2: collect every impl block.
    for item in &ast.items {
        if let Item::ImplBlock {
            trait_name,
            type_name,
            methods,
            ..
        } = item
        {
            let for_type = match impl_ty_for_name(*type_name, interner, resolve) {
                Some(t) => t,
                None => continue,
            };

            let mut method_def_ids = std::collections::HashMap::new();
            for m in methods {
                if let Some(def_id) = resolve.method_def_ids.get(&m.id) {
                    method_def_ids.insert(m.name, *def_id);
                }
            }

            registry.impls.insert(
                (*trait_name, for_type.clone()),
                ImplDef {
                    trait_name: *trait_name,
                    for_type,
                    methods: method_def_ids,
                },
            );
        }
    }

    registry
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
                                                convert_annotation(
                                                    t, self_trait, interner, resolve,
                                                )
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
    }
}

/// Maps a Symbol naming an impl's "for type" to a registry-level `ImplTy`.
fn impl_ty_for_name(
    name: Symbol,
    interner: &Interner,
    resolve: &ResolveResult,
) -> Option<ImplTy> {
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
