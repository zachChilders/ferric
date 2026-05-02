# M9 — Task 6: Stdlib — Built-in Effect Handlers

> Registers the built-in `Async` and `Gen` effects, ships their default
> handlers, and retires the M8-era async-specific stdlib infrastructure.
>
> **Prerequisite:** [Task 5](m9-05-vm.md) must be complete — this task installs
> handlers that rely on the new opcodes and continuation slab.
>
> See [m9-00-overview.md](m9-00-overview.md) for milestone-level design
> decisions.

---

## What this task does

Wires the algebraic effects machinery into something users can actually run.
Registers `Async` and `Gen` as pre-declared effects (visible without import),
ships the default handlers that make `async fn` / `gen fn` programs behave the
way M8 programs did, and removes the now-redundant async-specific scaffolding
from `ferric_vm` and `ferric_stdlib`.

After this task, the M8 user-visible behaviour is fully preserved — every
async/await/spawn/join/gen/yield program produces identical output — but the
implementation is entirely in terms of effects and handlers.

---

## Built-in effect declarations

Register these in `ferric_stdlib` at startup as pre-declared effects visible to
all programs without an import:

```rust
effect Async {
    Suspend(future: Async<A>) -> A,
}

effect Gen<T> {
    Yield(value: T) -> Unit,
}
```

`Config` and other application-defined effects require explicit `effect`
declarations in user code.

---

## Built-in handlers

### `Async` handler

The async handler is the runtime backing of all `async fn` / `.await` programs.
It is installed automatically around `main` if `main` has an `Eff<[Async], _>`
type (detected by the compiler). Users may also install it explicitly for
sub-computations.

Semantics: identical to the M8 scheduler. `Async::Suspend(future)` captures the
continuation, enqueues `(continuation, future)` in a `VecDeque`, and polls the
queue until `future` is ready, then resumes the continuation with the value.

`spawn` and `join` are handler operations on `Async`:

- `spawn(task)` — submits `task` to the same scheduler queue, returns a handle
  that the same handler resolves on `.await`.
- `join(a, b)` — awaits two handles and returns a tuple.

Their public signatures and semantics are unchanged from M8; only the
implementation moves into the handler.

### `Gen<T>` handler

Turns a `gen fn` into an iterator-compatible value:

```rust
// In Ferric — built-in handler available as gen::collect, gen::next, gen::for_each
let values = handle { range(start: 0, end: 5) } with {
    Gen::Yield(value: v) => {
        // append v to accumulator, resume with Unit
        resume k with unit
    }
}
```

`gen::collect(f: gen_fn)` is a stdlib function that installs this handler and
returns `[T]`. `for v in range(0, 5) { }` desugars to `gen::for_each` at parse
time.

---

## Retire `ferric_async` scheduler infrastructure

Within this task (the actual crate deletion happens in Task 7):

- Remove `Value::Async`, `Value::Handle`, `AsyncState` from `ferric_vm`.
- Remove `AsyncVal`; the `NativeRegistry` fn type returns `Value` again — the
  async wrapping is now handled by the effects system.
- All existing stdlib functions that were updated to return
  `AsyncVal::Ready(v)` in M8 revert to returning `Value` directly.

This is a breaking change to the native ABI but it is internal — no Ferric
program is affected.

---

## `$` shell expressions

The M8 special-case for `$ expr` inside async contexts becomes a `Shell` effect:

```rust
effect Shell {
    Run(cmd: Str) -> Str,
}
```

`$ cmd` desugars to `perform Shell::Run(cmd: cmd)` at parse time (this lives in
Task 2 once the effect is declared). The default `Shell` handler is installed
alongside `Async` for `main` — same blocking-vs-non-blocking semantics as M8,
now derived from whether a non-blocking handler is in scope.

> If shell handling is too large for this task, it can be deferred — flag it
> explicitly and leave the M8 special case in place until a follow-up task
> retires it.

---

## Done when

- [ ] `Async` and `Gen<T>` are pre-declared at VM startup and resolvable without an import
- [ ] The default `Async` handler is installed automatically around `main` when `main` has an `Eff<[Async], _>` type
- [ ] `async fn` programs from M8 produce identical output under the new handler
- [ ] `spawn` and `join` are handler operations on `Async`, with M8-identical signatures and semantics
- [ ] `gen::collect`, `gen::next`, `gen::for_each` are exposed in stdlib
- [ ] `gen fn range(...) { yield ... }` collected via `gen::collect` returns the expected list
- [ ] `for v in gen_fn() { }` desugars and runs end-to-end
- [ ] `Value::Async`, `Value::Handle`, `AsyncState` are removed from `ferric_vm`
- [ ] `AsyncVal` is removed; `NativeRegistry` fn type returns `Value`
- [ ] All M8 stdlib natives that returned `AsyncVal::Ready(v)` revert to returning `Value`
- [ ] All M1–M8 programs still pass — no behaviour change visible at the source level
- [ ] If shell handling is deferred, that decision is documented in this file and a follow-up task is created
