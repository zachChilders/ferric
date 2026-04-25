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
            // Generic / inference / function types don't appear as impl heads.
            Ty::Var(_)
            | Ty::Fn { .. }
            | Ty::Array(_)
            | Ty::Option(_)
            | Ty::Result(_, _) => return None,
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

/// A user-defined trait — a name plus a method table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TraitDef {
    pub name: Symbol,
    pub methods: HashMap<Symbol, MethodSignature>,
}

/// One impl block: maps each trait method name to its concrete `DefId`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImplDef {
    pub trait_name: Symbol,
    pub for_type: ImplTy,
    /// Method name → DefId of the concrete function that implements it.
    pub methods: HashMap<Symbol, DefId>,
}

/// Registry of every trait and impl in the program.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TraitRegistry {
    pub traits: HashMap<Symbol, TraitDef>,
    pub impls: HashMap<(Symbol, ImplTy), ImplDef>,
}

impl TraitRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns true if `trait_name` is implemented for `ty`.
    pub fn has_impl(&self, trait_name: Symbol, ty: &Ty) -> bool {
        match ImplTy::from_ty(ty) {
            Some(key) => self.impls.contains_key(&(trait_name, key)),
            None => false,
        }
    }

    /// Looks up the impl method DefId for `trait_name::method_name` on `ty`.
    pub fn lookup_method(
        &self,
        trait_name: Symbol,
        ty: &Ty,
        method_name: Symbol,
    ) -> Option<DefId> {
        let key = ImplTy::from_ty(ty)?;
        let impl_def = self.impls.get(&(trait_name, key))?;
        impl_def.methods.get(&method_name).copied()
    }

    /// Finds any (trait, method DefId) pair where `ty` implements `trait` and
    /// the trait declares a method named `method_name`. Used for method-call
    /// dispatch where the trait is inferred from the receiver type.
    pub fn find_method(
        &self,
        ty: &Ty,
        method_name: Symbol,
    ) -> Option<(Symbol, DefId)> {
        let key = ImplTy::from_ty(ty)?;
        for ((trait_name, impl_ty), impl_def) in &self.impls {
            if *impl_ty != key {
                continue;
            }
            if let Some(def_id) = impl_def.methods.get(&method_name) {
                return Some((*trait_name, *def_id));
            }
        }
        None
    }
}
