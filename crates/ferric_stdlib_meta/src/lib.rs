//! # `ferric_stdlib_meta` — single source of truth for stdlib native metadata.
//!
//! Holds the names, parameter names, type signatures, and display strings
//! for every native function `ferric_stdlib` registers. `ferric_stdlib`,
//! `ferric_infer`, and `ferric_lsp` all read from these tables instead of
//! keeping their own hand-curated mirrors.
//!
//! This crate has zero deps beyond `ferric_common` so the LSP can pull
//! metadata without inheriting `ferric_stdlib`'s heavy runtime deps
//! (reqwest, tiny_http, chrono, regex, sha2, ...).

use ferric_common::{Ty, TyVar, TypeScheme};

pub mod async_intrinsics;
pub mod builtin_enums;

// One module per `ferric_stdlib` module, mirroring the layout. Each holds a
// `pub const FOO_FNS: &[FunctionMeta]`.
pub mod bytes;
pub mod core;
pub mod crypto;
pub mod dotenv;
pub mod encode;
pub mod env;
pub mod fmt;
pub mod http;
pub mod io;
pub mod json;
pub mod list;
pub mod llm;
pub mod log;
pub mod map;
pub mod math;
pub mod path;
pub mod proc_;
pub mod rand_;
pub mod re;
pub mod serve;
pub mod set;
pub mod sock;
pub mod sort;
pub mod str_;
pub mod sync;
pub mod time_;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Metadata for one native function.
#[derive(Debug)]
pub struct FunctionMeta {
    /// The name as called from Ferric source (also the registry key).
    pub name: &'static str,
    /// Parameter names in declaration order. Used by the resolver to validate
    /// named-arg call sites.
    pub params: &'static [&'static str],
    /// Type signature, if known. `Unknown` means the resolver still binds
    /// the name but the typechecker has nothing to constrain calls — the
    /// return type stays a free `Ty::Var(_)`.
    pub signature: Signature,
    /// Display string for completion menus, e.g. `"fn(path: Str) -> Str"`.
    /// For `Signature::Unknown` entries this is `"fn(...)"`.
    pub display: &'static str,
}

/// A function's type signature, or `Unknown` if not yet wired.
pub enum Signature {
    /// No type info yet. Resolver still binds the name; typecheck leaves
    /// the call's return type as a free var.
    Unknown,
    /// Monomorphic. The builder fn returns `(param Tys, return Ty)`.
    /// We use a fn pointer because `Vec<Ty>` / `Box<Ty>` aren't `const`.
    Mono(fn() -> (Vec<Ty>, Ty)),
    /// Polymorphic. Takes a `&mut dyn TyVarGen` so it can mint fresh
    /// `TyVar`s for the universally quantified params.
    Poly(fn(&mut dyn TyVarGen) -> TypeScheme),
}

impl std::fmt::Debug for Signature {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Signature::Unknown => write!(f, "Unknown"),
            Signature::Mono(_) => write!(f, "Mono(..)"),
            Signature::Poly(_) => write!(f, "Poly(..)"),
        }
    }
}

/// Source of fresh `TyVar`s for polymorphic signature builders. Implemented
/// by `ferric_infer` against its own `next_tyvar` counter; tests can use
/// the simple impl below.
pub trait TyVarGen {
    fn fresh(&mut self) -> TyVar;
}

/// Trivial `TyVarGen` that increments an internal counter. Useful for tests
/// and for `ferric_lsp`'s display-string consumers that don't care about
/// var freshness.
pub struct CountingGen {
    pub next: u32,
}

impl CountingGen {
    pub fn new() -> Self {
        Self { next: 0 }
    }
    pub fn starting_at(start: u32) -> Self {
        Self { next: start }
    }
}

impl Default for CountingGen {
    fn default() -> Self {
        Self::new()
    }
}

impl TyVarGen for CountingGen {
    fn fresh(&mut self) -> TyVar {
        let v = TyVar(self.next);
        self.next += 1;
        v
    }
}

// ---------------------------------------------------------------------------
// Aggregation
// ---------------------------------------------------------------------------

/// Every per-module metadata table, in registration order. Excludes the
/// async intrinsics (which the VM dispatches separately — see
/// [`async_intrinsics::ASYNC_INTRINSICS`]).
pub const ALL_MODULES: &[&[FunctionMeta]] = &[
    str_::STR_FNS,
    bytes::BYTES_FNS,
    list::LIST_FNS,
    sort::SORT_FNS,
    map::MAP_FNS,
    set::SET_FNS,
    math::MATH_FNS,
    io::IO_FNS,
    path::PATH_FNS,
    env::ENV_FNS,
    time_::TIME_FNS,
    rand_::RAND_FNS,
    re::RE_FNS,
    json::JSON_FNS,
    fmt::FMT_FNS,
    encode::ENCODE_FNS,
    crypto::CRYPTO_FNS,
    http::HTTP_FNS,
    serve::SERVE_FNS,
    sock::SOCK_FNS,
    proc_::PROC_FNS,
    log::LOG_FNS,
    sync::SYNC_FNS,
    dotenv::DOTENV_FNS,
    llm::LLM_FNS,
    core::CORE_FNS,
];

/// Iterate every native registered through `register_stdlib`. Excludes
/// async intrinsics — call [`async_intrinsics::ASYNC_INTRINSICS`] for those.
pub fn iter_all() -> impl Iterator<Item = &'static FunctionMeta> {
    ALL_MODULES.iter().copied().flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iter_all_is_nonempty() {
        let count = iter_all().count();
        assert!(count > 0, "no metadata registered");
    }

    #[test]
    fn names_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for m in iter_all() {
            assert!(seen.insert(m.name), "duplicate native name: {}", m.name);
        }
    }

    #[test]
    fn mono_signatures_match_param_count() {
        for m in iter_all() {
            if let Signature::Mono(builder) = &m.signature {
                let (params, _ret) = builder();
                assert_eq!(
                    params.len(),
                    m.params.len(),
                    "param count mismatch for {}: meta has {} names, signature has {} types",
                    m.name,
                    m.params.len(),
                    params.len(),
                );
            }
        }
    }
}
