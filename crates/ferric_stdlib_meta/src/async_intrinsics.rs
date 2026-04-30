//! Async runtime intrinsics — `spawn`, `join`, `sleep`, `block_on`,
//! `shell_run_async`. These are dispatched by the VM (with access to
//! scheduler state) rather than registered through `NativeRegistry`, but
//! the resolver still needs their names + param shapes, and the
//! typechecker needs their signatures, so they live here alongside the
//! per-module tables.
//!
//! Mirrors:
//!   - `ferric_stdlib::async_intrinsic_param_table` (resolver-side)
//!   - `ferric_infer::register_native_signatures` (typecheck-side, lines
//!     703-784 today)

use ferric_common::{Ty, TypeScheme};

use crate::{FunctionMeta, Signature, TyVarGen};

/// `spawn(task: Async<T>) -> Handle<T>` — generic in `T`.
fn sig_spawn(g: &mut dyn TyVarGen) -> TypeScheme {
    let t = g.fresh();
    TypeScheme {
        forall: vec![t],
        ty: Ty::Fn {
            params: vec![Ty::Async(Box::new(Ty::Var(t)))],
            ret: Box::new(Ty::Handle(Box::new(Ty::Var(t)))),
        },
    }
}

/// `join(a: Handle<A>, b: Handle<B>) -> (A, B)` — generic in `A`, `B`.
/// 3-arity / 4-arity overloads are deferred until a real call site demands
/// them (matches the existing M8 milestone scope).
fn sig_join(g: &mut dyn TyVarGen) -> TypeScheme {
    let a = g.fresh();
    let b = g.fresh();
    TypeScheme {
        forall: vec![a, b],
        ty: Ty::Fn {
            params: vec![
                Ty::Handle(Box::new(Ty::Var(a))),
                Ty::Handle(Box::new(Ty::Var(b))),
            ],
            ret: Box::new(Ty::Tuple(vec![Ty::Var(a), Ty::Var(b)])),
        },
    }
}

/// `sleep(ms: Int) -> Async<Unit>`.
fn sig_sleep() -> (Vec<Ty>, Ty) {
    (vec![Ty::Int], Ty::Async(Box::new(Ty::Unit)))
}

/// `shell_run_async(cmd: Str) -> Async<ShellOutput>`.
fn sig_shell_run_async() -> (Vec<Ty>, Ty) {
    (vec![Ty::Str], Ty::Async(Box::new(Ty::ShellOutput)))
}

/// `block_on(task: Async<T>) -> T` — generic in `T`.
fn sig_block_on(g: &mut dyn TyVarGen) -> TypeScheme {
    let t = g.fresh();
    TypeScheme {
        forall: vec![t],
        ty: Ty::Fn {
            params: vec![Ty::Async(Box::new(Ty::Var(t)))],
            ret: Box::new(Ty::Var(t)),
        },
    }
}

pub const ASYNC_INTRINSICS: &[FunctionMeta] = &[
    FunctionMeta {
        name: "spawn",
        params: &["task"],
        signature: Signature::Poly(sig_spawn),
        display: "fn(task: Async<T>) -> Handle<T>",
    },
    FunctionMeta {
        name: "join",
        params: &["a", "b"],
        signature: Signature::Poly(sig_join),
        display: "fn(a: Handle<A>, b: Handle<B>) -> (A, B)",
    },
    FunctionMeta {
        name: "sleep",
        params: &["ms"],
        signature: Signature::Mono(sig_sleep),
        display: "fn(ms: Int) -> Async<Unit>",
    },
    FunctionMeta {
        name: "shell_run_async",
        params: &["cmd"],
        signature: Signature::Mono(sig_shell_run_async),
        display: "fn(cmd: Str) -> Async<ShellOutput>",
    },
    FunctionMeta {
        name: "block_on",
        params: &["task"],
        signature: Signature::Poly(sig_block_on),
        display: "fn(task: Async<T>) -> T",
    },
];
