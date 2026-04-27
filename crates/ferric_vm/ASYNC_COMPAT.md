# ferric_vm — Async Compatibility Notes

These constraints must be maintained across all milestones so that adding
`async`/`await` is an additive pass on the compiler and VM, not a rewrite.

| Constraint                                | Status                                       | Action needed at async milestone                                  |
| ----------------------------------------- | -------------------------------------------- | ----------------------------------------------------------------- |
| `ferric_common` types are `Send + Sync`   | **Resolved** — compile-time check in `ferric_common/src/lib.rs` (M2/M8 maintained) | None |
| `Value` is `Send`                          | **Resolved** — compile-time check in `ferric_vm/src/lib.rs` (covers `Value::Async`/`Value::Handle` added in M8 Task 5) | None |
| `NativeRegistry` fn type is sync           | **Resolved in M8 Task 5** — see note below   | None                                                              |
| Frame stack is heap-allocated (M3+)        | **Resolved** — `BytecodeVM` uses a heap `Vec<CallFrame>`     | None                                                              |

## NativeRegistry — what actually shipped in M8 Task 5

The original async-prep design called for changing the `NativeFn` type from
`Fn(&[NativeValue]) -> Result<NativeValue, String>` to a future-returning
shape. M8 Task 5 (Path B) chose a simpler architecture: regular sync stdlib
functions keep the existing signature, and the async-aware stdlib intrinsics
(`spawn`, `join`, `sleep`, `shell_run_async`) are dispatched directly inside
the VM's `Op::Call` handler with access to scheduler state. The dual-shape
"AsyncVal" enum the spec sketched was never needed because eager-evaluation
async (`Value::Async` wrapping a resolved value) lets all sync natives
remain sync.

A future migration to truly non-blocking I/O (true cooperative scheduling
mid-await) would re-open this design — at that point the per-native fn type
would change so `shell_run_async` and `sleep` could yield without occupying
a real OS thread.

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
