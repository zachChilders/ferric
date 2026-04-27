# Task: Stdlib — `sync`

> Read `docs/tasks/stdlib-00-overview.md` first for the global rules.

## Scope

Implement the `sync` module from `docs/stdlib.md` (section "`sync` —
Channels and Mutexes *(async milestone, post-M3)*"). This module is a
**stub**: it defines the value types and registers the function surface,
but the channel/mutex semantics align with the future async runtime and
are not optimized.

The minimum bar: every function in the spec is registered, has a working
synchronous std-based implementation (so unit tests can pass), and has a
clear comment that real async semantics arrive with the runtime.

## Output File

- `crates/ferric_stdlib/src/sync.rs`

Exposes `pub fn register(registry, interner)`.

## Required `NativeValue` Variants

```rust
// REQUIRED NATIVE VALUE VARIANTS:
//   Tuple2(Box<NativeValue>, Box<NativeValue>)        // shared
//   SyncSender(Rc<SenderRepr>)
//   SyncReceiver(Rc<ReceiverRepr>)
//   SyncMutex(Rc<MutexRepr>)
//   SyncOnce(Rc<OnceRepr>)
//
// SenderRepr / ReceiverRepr wrap std::sync::mpsc::{SyncSender, Receiver}
// MutexRepr wraps std::sync::Mutex<NativeValue>
// OnceRepr wraps std::sync::OnceLock<NativeValue> + an init closure
```

## Required Deps

None — `std::sync` is sufficient for the stub.

## Function Surface (must register)

Channels:

`channel`, `send`, `recv`, `try_recv`.

Mutex:

`mutex`, `lock`.

Once:

`once`, `get`.

## Implementation Notes

- `channel(capacity)`:
  - `capacity == 0` → `std::sync::mpsc::channel()` (unbounded).
  - `capacity > 0` → `std::sync::mpsc::sync_channel(capacity)`.
  - Returns `(Sender, Receiver)` as a `Tuple2`.
- `send(s, val)` — `SendError` → `SyncError::Disconnected`.
- `recv(r)` blocks; `RecvError` → `Disconnected`.
- `try_recv(r)` returns `Some(val)` or `None`. `Disconnected` collapses
  to `None` for the stub (callers can poll `recv` for truth).
- `mutex(val)` wraps a value behind `Arc<Mutex<NativeValue>>`.
- `lock(m, f)` — locks, applies the closure (which returns `(NewValue, U)`),
  writes new value back, returns `U`. On poison, return
  `SyncError::Poisoned`.
- `once(init)` stores the init closure and an `OnceLock<NativeValue>`.
- `get(o)` runs init at most once, returns the cached value.

## Tests

- `(s, r) = channel(0)`; `send(s, Int(1))`; `recv(r)` → `Ok(Int(1))`.
- Bounded channel: `channel(2)`; send 2 values; third send (in another
  thread) blocks until a recv occurs.
- `try_recv` on empty → `None`; after send → `Some`.
- Drop sender, then `recv` → `Disconnected`.
- `mutex(Int(0))` then `lock(_, |v| (v+1, ()))` ×3 produces final value 3.
- `lock` while a poisoned thread released the guard returns `Poisoned`
  (use `panic` inside `catch_unwind` to provoke).
- `once(|| Int(42))` then `get` ×2 → both `Int(42)`, init called once.
- Threaded once: spawn N threads racing `get`; init runs exactly once.

Aim for ~10 tests.

## Constraints

- Do **not** modify `lib.rs` or `Cargo.toml`.
- This module is a stub — performance is irrelevant. Correctness on the
  documented surface is what matters. The async runtime will replace the
  internals later without changing the function surface.

## Acceptance

- One file with the listed register coverage.
- Tests cover send/recv, bounded blocking, drop-disconnect, mutex,
  poisoning, once init.
