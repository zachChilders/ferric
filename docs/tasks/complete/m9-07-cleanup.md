# M9 — Task 7: Cleanup

> Removes the dead M8 async scaffolding now that `ferric_effects` has fully
> taken over. Deletes the `ferric_async` crate, removes the M8-era types from
> `ferric_common`, and updates the workspace replacement log.
>
> **Prerequisite:** [Task 6](m9-06-stdlib.md) must be complete — every async
> path in stdlib and the VM must already route through the effects system
> before any of these symbols can be deleted.
>
> See [m9-00-overview.md](m9-00-overview.md) for milestone-level design
> decisions.

---

## What this task does

Pure deletion. The `ferric_async` crate is removed from the workspace, the M8
async-specific AST/`Ty`/error/result types are removed from `ferric_common`,
the now-unused `Op::Await` opcode is dropped, and the replacement log is
updated. After this task, no symbol in the codebase mentions `ferric_async` or
`async`-as-special-case — async is just an effect.

This task is mechanical: every removal here corresponds to something that was
already replaced in Tasks 2–6 and is no longer constructed or referenced. If
something in this list still has a live reference, the corresponding earlier
task is incomplete — fix that first rather than working around it here.

---

## Remove from `ferric_common`

- `AsyncFnItem` → delete; `Item::AsyncFn` variant removed.
- `AwaitExpr` → delete; `Expr::Await` variant removed.
- `AsyncBlockExpr` → delete; `Expr::AsyncBlock` variant removed.
- `Ty::Async`, `Ty::Handle` → delete.
- `Ty::Poll` → delete (the `Poll` enum was M8-internal and is no longer needed).
- `AsyncResult`, `AsyncLowerError` → delete.
- `AsyncWarning`, `AsyncWarningKind` → delete.
- `Op::Await` → delete.
- `TypeError::AwaitOutsideAsync`, `TypeError::AwaitOnNonAsync`,
  `TypeError::AsyncBlockInSync`, `TypeError::SpawnNonAsync` → delete.
- `ParseError::AwaitOutsideAsync` → delete.
- `Ty::Display` arms for `Async`, `Handle`, `Poll` → delete.
- `Send + Sync` compile-time assertions for the deleted types → delete.

Verify: `rg -i "AsyncFnItem|AwaitExpr|AsyncBlockExpr|Ty::Async|Ty::Handle|Ty::Poll|AsyncResult|AsyncLowerError|AsyncWarning|Op::Await"`
returns nothing in the workspace.

---

## Remove crate

- Delete the `ferric_async/` directory.
- Remove `ferric_async` from the workspace `Cargo.toml`.
- Verify that `cargo build` succeeds and the workspace is clean.
- Verify there is no remaining `use ferric_async::...` anywhere.

---

## Update the replacement log

| Milestone | Stage replaced       | New implementation        | main.rs delta                  |
|-----------|----------------------|---------------------------|--------------------------------|
| M9        | `ferric_async`       | `ferric_effects`          | 1 import swap + 1 arg change   |
| M9        | VM async scheduler   | Effects handler stack     | 0 — contained in `ferric_vm`  |
| M9        | `AsyncVal` native type | plain `Value` return    | 0 — contained in `ferric_stdlib` |

(The exact location of the replacement log lives in the workspace docs — find
it via `rg "Stage replaced"` and update in place.)

---

## Done when

- [ ] `AsyncFnItem`, `AwaitExpr`, `AsyncBlockExpr` are deleted from `ferric_common`
- [ ] `Item::AsyncFn`, `Expr::Await`, `Expr::AsyncBlock` variants are deleted
- [ ] `Ty::Async`, `Ty::Handle`, `Ty::Poll` variants are deleted
- [ ] `AsyncResult`, `AsyncLowerError`, `AsyncWarning`, `AsyncWarningKind` are deleted
- [ ] `Op::Await` is deleted
- [ ] All M8-era async `TypeError` and `ParseError` variants are deleted
- [ ] `Ty::Display` arms for the deleted variants are removed
- [ ] `Send + Sync` compile-time assertions for the deleted types are removed
- [ ] `ferric_async/` directory is deleted
- [ ] `ferric_async` is removed from the workspace `Cargo.toml`
- [ ] `cargo build` succeeds with no warnings
- [ ] `rg "ferric_async"` returns nothing in the workspace
- [ ] Replacement log is updated with M9 entries
- [ ] `main.rs` matches the spec exactly: 1 import swap + 1 type rename + 1 argument change relative to the M8-end state
- [ ] All M1–M8 fixtures still produce identical output
- [ ] All M9 done-when checklist items in [m9-00-overview.md](m9-00-overview.md) pass
