# M8 — Async/Await

This milestone has been broken into actionable tasks under
[`docs/tasks/`](tasks/). Start with the overview, then work through the five
tasks in order.

| File | Contents |
|------|----------|
| [m8-00-overview.md](tasks/m8-00-overview.md) | Goal, settled design decisions, pipeline shape, milestone-done checklist |
| [m8-01-common-types.md](tasks/m8-01-common-types.md) | New AST nodes, `Ty` variants, `AsyncResult`, error/warning types in `ferric_common` |
| [m8-02-lexer-parser.md](tasks/m8-02-lexer-parser.md) | `async`/`await` keywords, `async fn`, `.await` postfix, `async { }`, `Async<T>`/`Handle<T>` type syntax |
| [m8-03-typecheck.md](tasks/m8-03-typecheck.md) | `Async<T>`/`Handle<T>` checking, await context tracking, `spawn`/`join` signatures |
| [m8-04-lowering.md](tasks/m8-04-lowering.md) | `ferric_async` crate: state-machine transform, shell lowering, output invariants |
| [m8-05-vm-stdlib.md](tasks/m8-05-vm-stdlib.md) | `Value::Async`/`Value::Handle`, scheduler, `NativeRegistry` upgrade, `spawn`/`join`/`sleep`/`shell_run_async` |

## Dependency order

```
Task 1  ──►  Task 2  ──┐
        │              ├──►  Task 4  ──►  Task 5
        └──►  Task 3  ──┘
```

Task 1 must land first. Tasks 2 and 3 are independent of each other and may run
concurrently. Task 4 needs all three. Task 5 needs Tasks 1, 2, and 4.
