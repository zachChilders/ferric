//! Trait registry types.
//!
//! Built by the `ferric_traits` stage and consumed by `ferric_infer` and
//! `ferric_compiler`. Lives in `ferric_common` because it crosses stage
//! boundaries.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::{DefId, Symbol, Ty};

/// A Ferric type used as a key for impl lookup. We use a pruned form of `Ty`
/// (no `Ty::Var`, no nested fn types) because trait dispatch only ever
/// matches concrete heads.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ImplTy {
    Int,
    Float,
    Bool,
    Str,
    Unit,
    ShellOutput,
    Struct(DefId),
    Enum(DefId),
    Tuple(usize),
}

impl ImplTy {
    /// Maps a fully-resolved `Ty` to an `ImplTy`. Returns `None` for type
    /// variables (which means the receiver type wasn't concrete enough at
    /// dispatch time).
    pub fn from_ty(ty: &Ty) -> Option<Self> {
        Some(match ty {
            Ty::Int => ImplTy::Int,
            Ty::Float => ImplTy::Float,
            Ty::Bool => ImplTy::Bool,
            Ty::Str => ImplTy::Str,
            Ty::Unit => ImplTy::Unit,
            Ty::ShellOutput => ImplTy::ShellOutput,
            Ty::Struct { def_id, .. } => ImplTy::Struct(*def_id),
            Ty::Enum { def_id, .. } => ImplTy::Enum(*def_id),
            Ty::Tuple(elems) => ImplTy::Tuple(elems.len()),
            // Opaque types erase at runtime — trait dispatch falls through to
            // the inner type. The type checker still treats them as distinct.
            Ty::Opaque { inner, .. } => return ImplTy::from_ty(inner),
            // Generic / inference / function types don't appear as impl heads.
            // Async/Handle/Poll likewise: these are runtime-only wrappers; trait
            // dispatch is resolved on the inner result type at await sites.
            Ty::Var(_)
            | Ty::Fn { .. }
            | Ty::Array(_)
            | Ty::Option(_)
            | Ty::Result(_, _)
            | Ty::Async(_)
            | Ty::Handle(_)
            | Ty::Poll(_)
            | Ty::Eff(_, _)
            | Ty::EffVar(_) => return None,
        })
    }
}

/// One method signature inside a trait declaration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MethodSignature {
    /// Parameter types in declaration order. The first entry is `self`.
    pub params: Vec<Ty>,
    /// Return type.
    pub ret: Ty,
}

/// A user-defined trait — a name plus a method table. May declare its own
/// generic type parameters (e.g. `trait To<T>`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TraitDef {
    pub name: Symbol,
    /// Generic type parameter names declared on the trait. Empty for
    /// non-generic traits.
    #[serde(default)]
    pub type_params: Vec<Symbol>,
    pub methods: HashMap<Symbol, MethodSignature>,
    /// Whether this trait is built into the stdlib (and therefore stdlib-owned
    /// for orphan-rule purposes).
    #[serde(default)]
    pub origin: TraitOrigin,
}

/// Whether a trait or impl was registered by stdlib (built-in) or by user
/// source code. Determines orphan-rule ownership.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum TraitOrigin {
    /// Built into the standard library — stdlib owns the trait/impl.
    Stdlib,
    /// Declared in user source code — the user crate owns the trait/impl.
    #[default]
    User,
}

/// One impl block: maps each trait method name to its concrete `DefId`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImplDef {
    pub trait_name: Symbol,
    pub for_type: ImplTy,
    /// Concrete type arguments supplied to the trait at the impl site
    /// (e.g. `[ImplTy::Str]` for `impl To<Str> for ShellOutput`). Empty for
    /// non-generic traits like `Describable`.
    #[serde(default)]
    pub trait_args: Vec<ImplTy>,
    /// Method name → DefId of the concrete function that implements it.
    pub methods: HashMap<Symbol, DefId>,
    /// Whether this impl was built into the stdlib (and therefore stdlib-owned
    /// for orphan-rule purposes).
    #[serde(default)]
    pub origin: TraitOrigin,
}

/// Registry of every trait and impl in the program.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TraitRegistry {
    pub traits: HashMap<Symbol, TraitDef>,
    /// One bucket per `(trait, source_type)` pair. A bucket may contain
    /// multiple `ImplDef`s when the trait is generic and several impls share
    /// a source type but differ in their `trait_args` (e.g. `impl To<Str> for
    /// Int` and `impl To<Float> for Int`).
    pub impls: HashMap<(Symbol, ImplTy), Vec<ImplDef>>,
    /// Symbol of the built-in `To` trait, if it has been registered. Set by
    /// `ferric_stdlib::register_builtin_traits`. `None` until stdlib seeds it.
    #[serde(default)]
    pub to_trait: Option<Symbol>,
    /// Symbol of the `to` method on the `To` trait. Set alongside `to_trait`.
    #[serde(default)]
    pub to_method: Option<Symbol>,
}

impl TraitRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts an impl into the registry, appending to the bucket for its
    /// `(trait_name, for_type)` pair.
    pub fn insert_impl(&mut self, impl_def: ImplDef) {
        let key = (impl_def.trait_name, impl_def.for_type.clone());
        self.impls.entry(key).or_default().push(impl_def);
    }

    /// Returns true if `trait_name` is implemented for `ty` (any args).
    pub fn has_impl(&self, trait_name: Symbol, ty: &Ty) -> bool {
        match ImplTy::from_ty(ty) {
            Some(key) => self
                .impls
                .get(&(trait_name, key))
                .map(|v| !v.is_empty())
                .unwrap_or(false),
            None => false,
        }
    }

    /// Looks up the impl method DefId for `trait_name::method_name` on `ty`.
    /// When several impls match (e.g. `To<Str> for Int` and `To<Float> for
    /// Int`), returns the first registered — callers that care about which
    /// impl should call [`Self::find_to_impls`] instead.
    pub fn lookup_method(&self, trait_name: Symbol, ty: &Ty, method_name: Symbol) -> Option<DefId> {
        let key = ImplTy::from_ty(ty)?;
        let impls = self.impls.get(&(trait_name, key))?;
        impls
            .iter()
            .find_map(|impl_def| impl_def.methods.get(&method_name).copied())
    }

    /// Finds any (trait, method DefId) pair where `ty` implements `trait` and
    /// the trait declares a method named `method_name`. Used for method-call
    /// dispatch where the trait is inferred from the receiver type.
    pub fn find_method(&self, ty: &Ty, method_name: Symbol) -> Option<(Symbol, DefId)> {
        let key = ImplTy::from_ty(ty)?;
        for ((trait_name, impl_ty), impls) in &self.impls {
            if *impl_ty != key {
                continue;
            }
            for impl_def in impls {
                if let Some(def_id) = impl_def.methods.get(&method_name) {
                    return Some((*trait_name, *def_id));
                }
            }
        }
        None
    }

    /// Returns the `DefId`s of every `to` method registered on a `To<target>
    /// for source` impl. Used by the type checker (Coercion Task 3) to look up
    /// candidate coercion methods.
    pub fn find_to_impls(&self, source: &Ty, target: &Ty) -> Vec<DefId> {
        let to_trait = match self.to_trait {
            Some(t) => t,
            None => return Vec::new(),
        };
        let to_method = match self.to_method {
            Some(m) => m,
            None => return Vec::new(),
        };
        let src_key = match ImplTy::from_ty(source) {
            Some(k) => k,
            None => return Vec::new(),
        };
        let tgt_key = match ImplTy::from_ty(target) {
            Some(k) => k,
            None => return Vec::new(),
        };
        let bucket = match self.impls.get(&(to_trait, src_key)) {
            Some(b) => b,
            None => return Vec::new(),
        };
        bucket
            .iter()
            .filter(|i| i.trait_args.first() == Some(&tgt_key))
            .filter_map(|i| i.methods.get(&to_method).copied())
            .collect()
    }
}
