# M8 — Task 5: VM Scheduler + NativeRegistry Upgrade + Stdlib

> **Prerequisite:** Task 4 ([m8-04-lowering.md](m8-04-lowering.md)) must be
> complete before starting this task. The VM changes here depend on the
> `Async<T>` and `Poll<T>` value types produced by the lowering pass. Task 2
> (parser) must also be complete so that `async fn` syntax is valid in test
> programs.
>
> See [m8-00-overview.md](m8-00-overview.md) for the milestone-level design
> decisions.

---

## What this task does

Three tightly coupled additions that must land together because they share the
`Value` representation and the `NativeRegistry` fn type:

**A — `Value::Async` and `Value::Handle`** — new value variants in `ferric_vm`
for the runtime representation of async computations.

**B — The cooperative scheduler** — a task queue inside `BytecodeVM` that drives
`poll` calls until all spawned tasks complete.

**C — NativeRegistry upgrade** — changes the native function signature from
synchronous to `AsyncVal`-returning, as documented in M2.5 Task 4. Updates all
existing stdlib functions to the new signature. Adds async-specific stdlib
functions: `spawn`, `join`, `sleep`, `shell_run_async`.

---

## Part A — New value variants

### `Value::Async`

The runtime representation of an `Async<T>` value. Wraps a state value and a
reference to its poll function.

```rust
// Inside ferric_vm — not visible outside
pub(crate) enum AsyncState {
    Pending {
        state: Box<Value>,           // the current state enum value
        poll:  DefId,                // DefId of the generated __foo_poll function
    },
    Ready(Box<Value>),               // resolved value — cached after first Poll::Ready
}
```

Construction follows Rule 7 — `Value::Async` is never constructed directly
outside `ferric_vm`:

```rust
// ILLEGAL outside ferric_vm
let v = Value::Async(...);

// LEGAL everywhere — called by the Async::new stdlib function
let v = Value::new_async(state: Value, poll_def_id: DefId) -> Value;
```

### `Value::Handle`

A handle to a spawned task. Wraps a task ID in the scheduler's task table.

```rust
// LEGAL everywhere
let v = Value::new_handle(task_id: u64) -> Value;
```

Handles are `Copy`-like in the value system — duplicating a `Value::Handle` does
not duplicate the task, only the reference to it.

### `Value::new_module` update

`Value::Module` was added in M7. It contains `HashMap<Symbol, Value>`. Since
`Value` now gains `Async` and `Handle` variants, verify that the `Send` assertion
still passes — it will, because `DefId` is `u32` (Copy) and `u64` is `Copy`.

---

## Part B — Cooperative scheduler

### Design

The scheduler is a `VecDeque<Task>` owned by `BytecodeVM`. Each `Task` wraps
a `Value::Async` and a task ID. The scheduler runs after the entry point function
completes — it drives all spawned tasks to completion before `Executor::run`
returns.

```rust
struct Task {
    id:    u64,
    value: Value,   // always Value::Async
}

pub struct BytecodeVM {
    stack:      Vec<Value>,
    call_stack: Vec<Frame>,
    natives:    NativeRegistry,
    scheduler:  VecDeque<Task>,     // new
    task_seq:   u64,                // new — monotonically increasing task ID
    handles:    HashMap<u64, Value>, // new — task_id → resolved Value::Ready once done
}
```

### Scheduler loop

After the entry point function returns, `BytecodeVM` runs the scheduler loop:

```rust
while let Some(task) = self.scheduler.pop_front() {
    match self.poll_async(&mut task.value) {
        Poll::Ready(v) => {
            self.handles.insert(task.id, v);
            // Task is done — do not re-enqueue
        }
        Poll::Pending => {
            self.scheduler.push_back(task);   // round-robin
        }
    }
}
```

`poll_async` calls the task's poll function (looked up by `DefId`) with the
current state value and returns the `Poll<T>` result. The scheduler makes progress
as long as at least one task returns `Poll::Ready` per full pass. If every task
returns `Poll::Pending` in a full pass (no forward progress), this is a deadlock
— emit `RuntimeError::AsyncDeadlock { span: Span }` and halt.

```rust
// Add to RuntimeError (in ferric_common):
RuntimeError::AsyncDeadlock { span: Span }
```

Carries `Span` (Rule 5). The `span` is the span of the last `.await` expression
that failed to make progress.

### `handle.await` in the VM

When the bytecode VM executes an `Expr::Await` that was **not** eliminated by
the lowering pass — i.e., a `.await` on a `Handle<T>` — the VM checks whether
the handle's task has completed:

- If `self.handles.contains_key(task_id)` → return the resolved value immediately.
- If not → the current task is `Poll::Pending`. Return `Poll::Pending` to the
  scheduler, which will re-enqueue the current task and process others first.

This is cooperative — a task that is waiting on a handle yields the CPU to other
tasks automatically.

**Note on `.await` elimination:** The lowering pass (Task 4) eliminates `.await`
on `Async<T>` values by compiling them into state machine transitions. `.await`
on `Handle<T>` is different — it cannot be eliminated at compile time because the
handle's task may complete at any point during the scheduler loop. The compiler
emits an `Await` opcode for handle awaits; the VM handles it as described above.

### `Op::Await` — new bytecode instruction

```rust
// Add to the Op enum in ferric_common:
Op::Await,   // pops Handle<T> from stack, pushes T (or suspends if not ready)
```

The compiler emits `Op::Await` for `handle.await` expressions. For `async.await`
expressions, the lowering pass has already converted them to `match poll(...)`
calls — no `Op::Await` is emitted.

---

## Part C — NativeRegistry upgrade

### The breaking change

M2.5 Task 4 documented this change. It lands now.

The native function type changes from:

```rust
// Before — synchronous
Box<dyn Fn(Vec<Value>) -> Value>
```

to:

```rust
// After — may return immediately or signal pending
Box<dyn Fn(Vec<Value>) -> AsyncVal>
```

where:

```rust
// In ferric_vm — the return type of all native functions
pub enum AsyncVal {
    Ready(Value),              // synchronous result — available immediately
    Pending(Value),            // async result — Value is an Async<T> to be scheduled
}
```

`AsyncVal::Ready(v)` is the normal case — all existing stdlib functions return
this. `AsyncVal::Pending(v)` is used only by `shell_run_async` and `sleep` in
this milestone.

### Migration of existing stdlib functions

Every existing stdlib function in `ferric_stdlib` must be updated to return
`AsyncVal::Ready(result)` instead of just `result`. This is mechanical:

```rust
// Before
Box::new(|args| {
    let s = args[0].as_str();
    println!("{}", s);
    Value::new_unit()
})

// After
Box::new(|args| {
    let s = args[0].as_str();
    println!("{}", s);
    AsyncVal::Ready(Value::new_unit())
})
```

There are no semantic changes — only the wrapping. All tests must pass after this
migration.

### New stdlib functions

#### `spawn`

```rust
// Signature in Ferric: fn spawn(task: Async<T>) -> Handle<T>
// Native implementation:
Box::new(|args| {
    let async_val = args[0].clone();   // Value::Async
    let task_id   = self.scheduler.push(async_val);
    AsyncVal::Ready(Value::new_handle(task_id))
})
```

`spawn` submits a task to the scheduler and returns a handle immediately.
The task begins running on the next scheduler loop iteration.

#### `join` — two-handle form

```rust
// Signature: fn join(a: Handle<A>, b: Handle<B>) -> (A, B)
Box::new(|args| {
    let id_a = args[0].as_handle_id();
    let id_b = args[1].as_handle_id();
    // If both are resolved, return the tuple immediately.
    // If either is pending, return Pending — the scheduler will retry.
    match (self.handles.get(id_a), self.handles.get(id_b)) {
        (Some(a), Some(b)) => AsyncVal::Ready(Value::new_tuple(vec![a.clone(), b.clone()])),
        _                   => AsyncVal::Pending(/* a synthetic Async that re-polls join */),
    }
})
```

The three- and four-handle forms follow the same pattern.

#### `sleep`

```rust
// Signature: fn sleep(ms: Int) -> Async<Unit>
// Returns an Async<Unit> that becomes ready after `ms` milliseconds.
// Uses std::time::Instant for deadline tracking.
```

`sleep` is the only stdlib function that introduces wall-clock time into the
scheduler. It returns an `AsyncVal::Pending(Value::Async(...))` where the inner
async value's poll function checks `Instant::now() >= deadline`.

#### `shell_run_async`

```rust
// Signature: fn shell_run_async(cmd: Str) -> Async<ShellOutput>
// Non-blocking — spawns a subprocess and returns immediately.
// Uses std::process::Child::try_wait() to poll for completion.
```

`shell_run_async` spawns a subprocess using `std::process::Command` and returns
an `AsyncVal::Pending(Value::Async(...))`. The poll function calls `child.try_wait()`
— if the process has exited, returns `Poll::Ready(ShellOutput { ... })`. If not,
returns `Poll::Pending`.

The `ShellExpr` lowering in Task 4 emits a call to `shell_run_async` for shell
expressions inside async contexts. The stdlib function here is what that call
resolves to.

Rule 7 compliance — `Value::Async` wrapping a `shell_run_async` result is
constructed only through `Value::new_async(...)` inside `ferric_vm`.

---

## `ferric_vm/ASYNC_COMPAT.md` update

Update the compatibility doc to mark all constraints as resolved:

```markdown
| Constraint | Status | Action needed at async milestone |
|---|---|---|
| ferric_common types are Send + Sync | Enforced by compile-time check | None |
| NativeRegistry fn type is sync | **Resolved in M8 Task 5** — now returns AsyncVal | Done |
| Value variants are Send | Enforced by compile-time check | None |
| Frame stack is heap-allocated (M3+) | BytecodeVM uses heap Vec — done in M3 | None |
```

---

## Done when

**Value variants:**
- [ ] `Value::Async` exists and is constructed only via `Value::new_async(...)`
- [ ] `Value::Handle` exists and is constructed only via `Value::new_handle(...)`
- [ ] Both new variants pass the `Value: Send` compile-time assertion

**Scheduler:**
- [ ] `BytecodeVM` contains a `VecDeque<Task>` scheduler
- [ ] Spawned tasks run to completion before `Executor::run` returns
- [ ] Tasks that return `Poll::Pending` are re-enqueued and retried
- [ ] `RuntimeError::AsyncDeadlock` fires when no task makes progress in a full pass
- [ ] `Op::Await` is handled: pops `Handle<T>`, returns `T` if ready or suspends if pending
- [ ] `Executor` trait signature is **unchanged** — this is a test of the architecture

**NativeRegistry upgrade:**
- [ ] All existing stdlib functions return `AsyncVal::Ready(value)` — no semantic change
- [ ] All M1–M7 tests pass after the migration
- [ ] `spawn(task: async_val)` submits a task and returns a `Handle<T>`
- [ ] `join(a: h1, b: h2)` awaits both handles and returns `(A, B)`
- [ ] `join(a: h1, b: h2, c: h3)` and the four-handle form work correctly
- [ ] `sleep(ms: 100)` suspends for approximately 100 milliseconds
- [ ] `shell_run_async(cmd: "...")` returns `Async<ShellOutput>` without blocking

**Integration:**
- [ ] `async fn` with a single `.await` runs correctly end-to-end through the full pipeline
- [ ] `async fn` with multiple sequential `.await` points runs correctly
- [ ] `spawn` + `handle.await` demonstrates concurrent execution of two tasks
- [ ] `join` completes only after both handles resolve
- [ ] `$ cmd` inside `async fn` does not block (uses `shell_run_async` path)
- [ ] `$ cmd` outside `async fn` still works (blocking, with `AsyncWarning::BlockingShell`)
- [ ] `ferric_vm/ASYNC_COMPAT.md` updated — all four constraints marked resolved
- [ ] All new error types carry `Span` (Rule 5)
- [ ] All M1–M7 tests still pass
