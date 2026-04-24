# ferric_vm — Async Compatibility Notes

These constraints must be maintained across all milestones so that adding
`async`/`await` is an additive pass on the compiler and VM, not a rewrite.

| Constraint                                | Status                                       | Action needed at async milestone                                  |
| ----------------------------------------- | -------------------------------------------- | ----------------------------------------------------------------- |
| `ferric_common` types are `Send + Sync`   | Enforced by compile-time check in `ferric_common/src/lib.rs` | None                                                              |
| `Value` is `Send`                          | Enforced by compile-time check in `ferric_vm/src/lib.rs`     | None                                                              |
| `NativeRegistry` fn type is sync           | Documented at the type definition (intentional) | Update fn type to return `Pin<Box<dyn Future<Output=...> + Send>>` |
| Frame stack is heap-allocated (M3+)        | TreeWalker uses Rust stack (acceptable)      | BytecodeVM uses a heap `Vec<Frame>` — done in M3                  |

## Why these matter

A future async runtime requires that any value crossing an `.await` point
implements `Send`. The lex/parse/resolve/typecheck output types all need to
travel between threads on a tokio-style executor, and runtime `Value`s need to
survive being parked. If a non-`Send` type sneaks in — `Rc`, `RefCell`, raw
pointers, or anything wrapping them — the async pass turns from additive into
a rewrite of every type that touches it.

The frame-stack constraint is the other half: cooperative suspension needs a
stack that can be paused and resumed. The tree-walking interpreter recurses on
the Rust call stack (it can't), but the M3 BytecodeVM holds frames in a heap
`Vec` (it can). M3 already pays this cost for unrelated reasons; making sure
nothing here assumes the call chain is uninterruptible keeps the cost from
spreading.

## Where each guarantee lives

- Compile-time `Send + Sync` check for `ferric_common`: `crates/ferric_common/src/lib.rs`
- Compile-time `Send` check for `Value`:                `crates/ferric_vm/src/lib.rs`
- `NativeRegistry` upgrade comment:                     `crates/ferric_stdlib/src/lib.rs`
- TreeWalker frame-stack comment:                       `crates/ferric_vm/src/lib.rs`
