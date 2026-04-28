//! # `sync` — Channels, Mutexes, and Once cells
//!
//! Stub implementation built on top of `std::sync`. The function surface
//! defined here matches the spec in `docs/stdlib.md`. The async runtime
//! arrives post-M3 and will swap the internals (channels become async-aware,
//! mutexes become reentrancy-friendly with the executor, etc.) **without**
//! changing this function surface.
//!
//! All functions live under the `sync_*` native prefix, e.g. Ferric
//! `sync::send` → native `sync_send`.

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

// REQUIRED DEPS:
//   none — std::sync is sufficient for the stub.

use std::rc::Rc;
use std::sync::mpsc::{Receiver, Sender, SyncSender, TryRecvError};
use std::sync::{Mutex, OnceLock};

use ferric_common::Interner;

use crate::{NativeRegistry, NativeValue};

// ============================================================================
// Repr types — used as payloads for the new NativeValue variants declared
// at the top of the file. Integration will lift these into the real enum.
// ============================================================================

/// Wraps either an unbounded `Sender` or a bounded `SyncSender`. The two
/// flavours come out of `std::sync::mpsc::channel` and `sync_channel`
/// respectively; we hide the distinction behind a single value so the
/// Ferric-side type is simply `Sender`.
pub enum SenderRepr {
    Unbounded(Mutex<Option<Sender<NativeValue>>>),
    Bounded(Mutex<Option<SyncSender<NativeValue>>>),
}

impl SenderRepr {
    fn send(&self, val: NativeValue) -> Result<(), &'static str> {
        match self {
            SenderRepr::Unbounded(m) => {
                let guard = m.lock().map_err(|_| "sender mutex poisoned")?;
                match guard.as_ref() {
                    Some(s) => s.send(val).map_err(|_| "disconnected"),
                    None => Err("disconnected"),
                }
            }
            SenderRepr::Bounded(m) => {
                let guard = m.lock().map_err(|_| "sender mutex poisoned")?;
                match guard.as_ref() {
                    Some(s) => s.send(val).map_err(|_| "disconnected"),
                    None => Err("disconnected"),
                }
            }
        }
    }
}

impl std::fmt::Debug for SenderRepr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SenderRepr::Unbounded(_) => f.write_str("SenderRepr::Unbounded(..)"),
            SenderRepr::Bounded(_) => f.write_str("SenderRepr::Bounded(..)"),
        }
    }
}

/// Wraps a receiving end of an mpsc channel. Behind a `Mutex` so that
/// the value itself can be `Clone`/`Rc`'d safely while still allowing
/// blocking `recv` calls — the receiver is not `Sync` on its own.
pub struct ReceiverRepr {
    inner: Mutex<Receiver<NativeValue>>,
}

impl ReceiverRepr {
    fn recv(&self) -> Result<NativeValue, &'static str> {
        let guard = self.inner.lock().map_err(|_| "receiver mutex poisoned")?;
        guard.recv().map_err(|_| "disconnected")
    }

    fn try_recv(&self) -> Option<NativeValue> {
        let guard = self.inner.lock().ok()?;
        match guard.try_recv() {
            Ok(v) => Some(v),
            // Empty and Disconnected both collapse to None for the stub —
            // callers can poll `recv` for definitive disconnection.
            Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => None,
        }
    }
}

impl std::fmt::Debug for ReceiverRepr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ReceiverRepr(..)")
    }
}

/// Wraps a `Mutex<NativeValue>`. The held value is the user's data.
pub struct MutexRepr {
    inner: Mutex<NativeValue>,
}

impl std::fmt::Debug for MutexRepr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MutexRepr(..)")
    }
}

/// Wraps a `OnceLock<NativeValue>` plus the captured init closure.
///
/// The init closure is stored as a Ferric `NativeValue` so we can hand
/// it back to the VM via the as-yet-unimplemented `invoke_closure`
/// helper. Until that helper exists, `get` calls a placeholder error;
/// the init-runs-once contract is still enforced by the underlying
/// `OnceLock`.
pub struct OnceRepr {
    cell: OnceLock<NativeValue>,
    /// The user-supplied init closure, stored opaquely.
    pub init: NativeValue,
}

impl std::fmt::Debug for OnceRepr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OnceRepr(..)")
    }
}

// ============================================================================
// Helpers (private to this module per overview rule 5)
// ============================================================================

fn check_arg_count(args: &[NativeValue], expected: usize) -> Result<(), String> {
    if args.len() != expected {
        Err(format!(
            "expected {} argument(s), got {}",
            expected,
            args.len()
        ))
    } else {
        Ok(())
    }
}

fn expect_int(value: &NativeValue) -> Result<i64, String> {
    match value {
        NativeValue::Int(n) => Ok(*n),
        other => Err(format!("expected int, got {other:?}")),
    }
}

fn expect_sender(value: &NativeValue) -> Result<Rc<SenderRepr>, String> {
    match value {
        NativeValue::SyncSender(s) => Ok(Rc::clone(s)),
        other => Err(format!("expected Sender, got {other:?}")),
    }
}

fn expect_receiver(value: &NativeValue) -> Result<Rc<ReceiverRepr>, String> {
    match value {
        NativeValue::SyncReceiver(r) => Ok(Rc::clone(r)),
        other => Err(format!("expected Receiver, got {other:?}")),
    }
}

fn expect_mutex(value: &NativeValue) -> Result<Rc<MutexRepr>, String> {
    match value {
        NativeValue::SyncMutex(m) => Ok(Rc::clone(m)),
        other => Err(format!("expected Mutex, got {other:?}")),
    }
}

fn expect_once(value: &NativeValue) -> Result<Rc<OnceRepr>, String> {
    match value {
        NativeValue::SyncOnce(o) => Ok(Rc::clone(o)),
        other => Err(format!("expected Once, got {other:?}")),
    }
}

/// Wraps a payload in a Ferric-style `Result::Ok`. Until enum variants
/// are encoded as `NativeValue`, we use a `Tuple2(Int, payload)` where
/// `Int(0) = Ok`, `Int(1) = Err`. This is the same convention used by
/// other stdlib modules that need to surface `Result` from native code
/// today.
fn ok(val: NativeValue) -> NativeValue {
    NativeValue::Tuple2(Box::new(NativeValue::Int(0)), Box::new(val))
}

/// Wraps a payload in a Ferric-style `Result::Err` carrying a `SyncError`
/// tag. The tag is an `Int`:
///   0 → Disconnected
///   1 → Poisoned
fn sync_err(tag: i64) -> NativeValue {
    NativeValue::Tuple2(
        Box::new(NativeValue::Int(1)),
        Box::new(NativeValue::Int(tag)),
    )
}

/// Wraps a payload in a Ferric-style `Option::Some`. `Tuple2(Int(0), v)`.
fn some(val: NativeValue) -> NativeValue {
    NativeValue::Tuple2(Box::new(NativeValue::Int(0)), Box::new(val))
}

/// Wraps in `Option::None`. `Tuple2(Int(1), Unit)`.
fn none() -> NativeValue {
    NativeValue::Tuple2(Box::new(NativeValue::Int(1)), Box::new(NativeValue::Unit))
}

// ============================================================================
// Built-ins
// ============================================================================

/// `sync::channel(capacity: Int) -> (Sender<T>, Receiver<T>)`
///
/// `capacity == 0` → unbounded. `capacity > 0` → bounded (`sync_channel`).
fn builtin_sync_channel(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let cap = expect_int(&args[0])?;
    if cap < 0 {
        return Err(format!("channel capacity must be non-negative, got {cap}"));
    }
    let (sender, receiver) = if cap == 0 {
        let (tx, rx) = std::sync::mpsc::channel::<NativeValue>();
        (SenderRepr::Unbounded(Mutex::new(Some(tx))), rx)
    } else {
        let (tx, rx) = std::sync::mpsc::sync_channel::<NativeValue>(cap as usize);
        (SenderRepr::Bounded(Mutex::new(Some(tx))), rx)
    };
    let s = NativeValue::SyncSender(Rc::new(sender));
    let r = NativeValue::SyncReceiver(Rc::new(ReceiverRepr {
        inner: Mutex::new(receiver),
    }));
    Ok(NativeValue::Tuple2(Box::new(s), Box::new(r)))
}

/// `sync::send(s, val) -> Result<Unit, SyncError>`
fn builtin_sync_send(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let sender = expect_sender(&args[0])?;
    let val = args[1].clone();
    match sender.send(val) {
        Ok(()) => Ok(ok(NativeValue::Unit)),
        Err(_disc) => Ok(sync_err(0)), // Disconnected
    }
}

/// `sync::recv(r) -> Result<T, SyncError>` (blocks)
fn builtin_sync_recv(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let receiver = expect_receiver(&args[0])?;
    match receiver.recv() {
        Ok(v) => Ok(ok(v)),
        Err(_) => Ok(sync_err(0)),
    }
}

/// `sync::try_recv(r) -> Option<T>`
fn builtin_sync_try_recv(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let receiver = expect_receiver(&args[0])?;
    match receiver.try_recv() {
        Some(v) => Ok(some(v)),
        None => Ok(none()),
    }
}

/// `sync::mutex(val) -> Mutex<T>`
fn builtin_sync_mutex(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let val = args[0].clone();
    Ok(NativeValue::SyncMutex(Rc::new(MutexRepr {
        inner: Mutex::new(val),
    })))
}

/// `sync::lock(m, f) -> U`
///
/// **Stub limitation:** The closure `f` cannot be invoked from native
/// code today — there is no `invoke_closure` helper plumbed through the
/// `NativeRegistry` interface. When the runtime grows that hook, this
/// function will:
///
/// 1. lock the mutex (returning `SyncError::Poisoned` on poison),
/// 2. call `f(value)` to get `(new_value, U)`,
/// 3. write `new_value` back into the mutex,
/// 4. release the lock and return `U`.
///
/// For now we expose a degraded form: the closure argument is **ignored**
/// and the function simply returns the current locked value. Tests
/// covering the mutate-via-closure path call `lock_with` (a Rust-only
/// helper, see `tests`) which does the lock/poison handling correctly;
/// the closure-driven path is documented but unimplemented at the
/// native boundary.
fn builtin_sync_lock(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let m = expect_mutex(&args[0])?;
    let _f = &args[1]; // ignored — see doc comment.
    match m.inner.lock() {
        Ok(guard) => Ok(ok(guard.clone())),
        Err(_) => Ok(sync_err(1)), // Poisoned
    }
}

/// `sync::once(init) -> Once<T>`
fn builtin_sync_once(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let init = args[0].clone();
    Ok(NativeValue::SyncOnce(Rc::new(OnceRepr {
        cell: OnceLock::new(),
        init,
    })))
}

/// `sync::get(o) -> T`
///
/// **Stub limitation:** Like `lock`, we cannot invoke the stored init
/// closure from native code today. If the cell is uninitialised we
/// return the captured init value verbatim — which only matches the
/// spec when the user passed a value rather than a closure. The
/// `OnceLock` itself still guarantees init runs at most once. Tests
/// exercise the contract via `get_with` (Rust-only), where we drive
/// initialisation by hand.
fn builtin_sync_get(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let o = expect_once(&args[0])?;
    if let Some(v) = o.cell.get() {
        return Ok(v.clone());
    }
    // Fallback: store the init value as-is, but only on the first call.
    let _ = o.cell.set(o.init.clone());
    Ok(o.cell.get().cloned().unwrap_or(NativeValue::Unit))
}

// ============================================================================
// Rust-side helpers used by tests (and, eventually, by the closure-aware
// runtime). These bypass the native-call boundary and operate directly on
// the repr types, exercising the high-value semantics: poison handling,
// drop-disconnect, and once-init exactly-once.
// ============================================================================

/// Lock the mutex, run `f` on the contained value, write back the new
/// value, return the auxiliary `U`. Mirrors the eventual semantics of
/// `sync::lock(m, f)`.
pub fn lock_with<U>(
    m: &MutexRepr,
    f: impl FnOnce(NativeValue) -> (NativeValue, U),
) -> Result<U, &'static str> {
    let mut guard = m.inner.lock().map_err(|_| "poisoned")?;
    let (new_val, out) = f(guard.clone());
    *guard = new_val;
    Ok(out)
}

/// Get-or-init driver for `OnceRepr`. Runs `init` at most once across
/// all callers, returns the cached value thereafter.
pub fn get_with(o: &OnceRepr, init: impl FnOnce() -> NativeValue) -> NativeValue {
    o.cell.get_or_init(init).clone()
}

// ============================================================================
// Registration
// ============================================================================

/// Registers every `sync_*` native with the registry.
pub fn register(registry: &mut NativeRegistry, interner: &mut Interner) {
    type Entry = (&'static str, &'static [&'static str], crate::NativeFn);
    let entries: &[Entry] = &[
        ("sync_channel", &["capacity"], builtin_sync_channel),
        ("sync_send", &["s", "val"], builtin_sync_send),
        ("sync_recv", &["r"], builtin_sync_recv),
        ("sync_try_recv", &["r"], builtin_sync_try_recv),
        ("sync_mutex", &["val"], builtin_sync_mutex),
        ("sync_lock", &["m", "f"], builtin_sync_lock),
        ("sync_once", &["init"], builtin_sync_once),
        ("sync_get", &["o"], builtin_sync_get),
    ];
    for (name, params, f) in entries {
        registry.register_named(interner, name, params, *f);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
    use std::thread;
    use std::time::Duration;

    /// Helper: extract a sender/receiver pair from the Tuple2 returned by
    /// `sync_channel`.
    fn unpack_channel(v: NativeValue) -> (Rc<SenderRepr>, Rc<ReceiverRepr>) {
        match v {
            NativeValue::Tuple2(a, b) => match (*a, *b) {
                (NativeValue::SyncSender(s), NativeValue::SyncReceiver(r)) => (s, r),
                other => panic!("expected (Sender, Receiver), got {other:?}"),
            },
            other => panic!("expected Tuple2, got {other:?}"),
        }
    }

    fn unwrap_ok(v: NativeValue) -> NativeValue {
        match v {
            NativeValue::Tuple2(tag, payload) => match *tag {
                NativeValue::Int(0) => *payload,
                NativeValue::Int(t) => panic!("expected Ok, got Err({t})"),
                other => panic!("expected Int tag, got {other:?}"),
            },
            other => panic!("expected Tuple2 result, got {other:?}"),
        }
    }

    fn unwrap_err_tag(v: NativeValue) -> i64 {
        match v {
            NativeValue::Tuple2(tag, payload) => match (*tag, *payload) {
                (NativeValue::Int(1), NativeValue::Int(t)) => t,
                other => panic!("expected Err(Int), got {other:?}"),
            },
            other => panic!("expected Tuple2, got {other:?}"),
        }
    }

    // -------------------- channel: unbounded round-trip ----------------------

    #[test]
    fn unbounded_channel_send_recv() {
        let pair = builtin_sync_channel(&[NativeValue::Int(0)]).expect("channel(0) should succeed");
        let (s, r) = unpack_channel(pair);

        let s_val = NativeValue::SyncSender(s);
        let r_val = NativeValue::SyncReceiver(r);

        let send_res = builtin_sync_send(&[s_val, NativeValue::Int(1)]).unwrap();
        assert_eq!(unwrap_ok(send_res), NativeValue::Unit);

        let recv_res = builtin_sync_recv(&[r_val]).unwrap();
        assert_eq!(unwrap_ok(recv_res), NativeValue::Int(1));
    }

    // -------------------- bounded channel blocks on third send --------------

    #[test]
    fn bounded_channel_blocks_when_full() {
        // The blocking semantic belongs to `std::sync::mpsc::sync_channel`
        // and would normally be exercised by sending NativeValues across
        // threads. NativeValue contains `Rc` variants and is therefore
        // `!Send`, so the channel-of-NativeValue isn't `Send` either —
        // which is exactly why the runtime will gain a thread-safe handle
        // type when async lands. Until then we verify the blocking
        // semantic against a plain `SyncSender<i64>`, and verify the
        // NativeValue-typed channel works synchronously on a single thread.
        let pair = builtin_sync_channel(&[NativeValue::Int(2)]).expect("channel(2) should succeed");
        let (s, r) = unpack_channel(pair);

        // Same-thread fill / drain works for a NativeValue channel.
        builtin_sync_send(&[NativeValue::SyncSender(Rc::clone(&s)), NativeValue::Int(1)]).unwrap();
        builtin_sync_send(&[NativeValue::SyncSender(Rc::clone(&s)), NativeValue::Int(2)]).unwrap();
        let recv = builtin_sync_recv(&[NativeValue::SyncReceiver(Rc::clone(&r))]).unwrap();
        assert_eq!(unwrap_ok(recv), NativeValue::Int(1));

        // Cross-thread blocking: int-typed channel proves the same code
        // path inside std::sync::mpsc.
        let (tx, rx) = std::sync::mpsc::sync_channel::<i64>(2);
        tx.send(1).unwrap();
        tx.send(2).unwrap();
        let done = Arc::new(AtomicUsize::new(0));
        let done2 = Arc::clone(&done);
        let handle = thread::spawn(move || {
            tx.send(3).unwrap();
            done2.store(1, AtomicOrdering::SeqCst);
        });
        thread::sleep(Duration::from_millis(50));
        assert_eq!(
            done.load(AtomicOrdering::SeqCst),
            0,
            "third send should be blocked while buffer is full"
        );
        assert_eq!(rx.recv().unwrap(), 1);
        handle.join().expect("thread joined");
        assert_eq!(done.load(AtomicOrdering::SeqCst), 1);
    }

    // -------------------- try_recv on empty / after send --------------------

    #[test]
    fn try_recv_empty_then_some() {
        let pair = builtin_sync_channel(&[NativeValue::Int(0)]).unwrap();
        let (s, r) = unpack_channel(pair);

        // Empty.
        let empty = builtin_sync_try_recv(&[NativeValue::SyncReceiver(Rc::clone(&r))]).unwrap();
        match empty {
            NativeValue::Tuple2(tag, _) => assert_eq!(*tag, NativeValue::Int(1)),
            other => panic!("expected Option::None tuple, got {other:?}"),
        }

        // After a send, try_recv yields Some.
        builtin_sync_send(&[NativeValue::SyncSender(s), NativeValue::Int(7)]).unwrap();
        let got = builtin_sync_try_recv(&[NativeValue::SyncReceiver(r)]).unwrap();
        match got {
            NativeValue::Tuple2(tag, payload) => {
                assert_eq!(*tag, NativeValue::Int(0));
                assert_eq!(*payload, NativeValue::Int(7));
            }
            other => panic!("expected Some, got {other:?}"),
        }
    }

    // -------------------- drop sender → recv disconnected -------------------

    #[test]
    fn drop_sender_then_recv_returns_disconnected() {
        let pair = builtin_sync_channel(&[NativeValue::Int(0)]).unwrap();
        let (s, r) = unpack_channel(pair);

        // Force-drop the underlying sender by clearing the Option in the
        // SenderRepr's inner mutex. This simulates "all senders dropped"
        // without needing to manage Rc lifetimes from outside.
        match &*s {
            SenderRepr::Unbounded(m) => {
                *m.lock().unwrap() = None;
            }
            SenderRepr::Bounded(m) => {
                *m.lock().unwrap() = None;
            }
        }
        // Drop our Rc to the SenderRepr so the actual std Sender is
        // dropped (the Option carries the only one).
        drop(s);

        let recv = builtin_sync_recv(&[NativeValue::SyncReceiver(r)]).unwrap();
        assert_eq!(unwrap_err_tag(recv), 0, "expected Disconnected");
    }

    // -------------------- mutex: lock_with mutates value -------------------

    #[test]
    fn mutex_lock_with_increments_three_times() {
        let m_val = builtin_sync_mutex(&[NativeValue::Int(0)]).unwrap();
        let m = expect_mutex(&m_val).unwrap();

        for _ in 0..3 {
            lock_with(&m, |v| match v {
                NativeValue::Int(n) => (NativeValue::Int(n + 1), ()),
                other => panic!("unexpected value {other:?}"),
            })
            .expect("lock should succeed");
        }

        let final_val = m.inner.lock().unwrap().clone();
        assert_eq!(final_val, NativeValue::Int(3));
    }

    // -------------------- mutex poisoning ----------------------------------

    #[test]
    fn lock_after_panic_returns_poisoned() {
        // Build a Mutex<NativeValue> via the public path so the poison
        // path exercises exactly the same lock the native uses.
        let m_val = builtin_sync_mutex(&[NativeValue::Int(0)]).unwrap();
        let m = expect_mutex(&m_val).unwrap();

        // Poison the lock from another thread.
        let m_clone = Rc::clone(&m);
        // Rc isn't Send, so send the inner mutex through an Arc instead.
        // We construct a parallel Arc<Mutex<...>> for poisoning, then check
        // poisoning via the `builtin_sync_lock` codepath using a fresh
        // mutex value that *is* poisoned.
        //
        // Simpler approach: use std::panic::catch_unwind on the same
        // thread to provoke a poisoned guard on m's inner mutex via a
        // worker that owns the Arc. Since MutexRepr's inner is std Mutex,
        // we can create an Arc out of a separate one for the cross-thread
        // shenanigans, then verify the codepath responds. But that
        // wouldn't exercise our Rc-wrapped repr.
        //
        // Trick: spawn a scoped thread that takes a raw &Mutex via
        // pointer cast through usize — safe because we join before the
        // Rc drops. This keeps the test exercising the real repr.
        let raw = &m_clone.inner as *const Mutex<NativeValue> as usize;
        let handle = thread::spawn(move || {
            // SAFETY: the main thread holds an Rc to MutexRepr for the
            // entire duration of this thread (we join below before Rc
            // drops), so the &Mutex is valid.
            let mtx: &Mutex<NativeValue> = unsafe { &*(raw as *const Mutex<NativeValue>) };
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _g = mtx.lock().unwrap();
                panic!("intentional poison");
            }));
        });
        handle.join().expect("poisoning thread joined");

        // Now native lock should report Poisoned.
        let res = builtin_sync_lock(&[m_val, NativeValue::Unit]).unwrap();
        assert_eq!(unwrap_err_tag(res), 1, "expected Poisoned");
    }

    // -------------------- once: init runs exactly once ---------------------

    #[test]
    fn once_get_with_runs_init_once() {
        let init_calls = Arc::new(AtomicUsize::new(0));
        let init_calls2 = Arc::clone(&init_calls);

        let o_val = builtin_sync_once(&[NativeValue::Int(42)]).unwrap();
        let o = expect_once(&o_val).unwrap();

        let v1 = get_with(&o, || {
            init_calls2.fetch_add(1, AtomicOrdering::SeqCst);
            NativeValue::Int(42)
        });
        let init_calls3 = Arc::clone(&init_calls);
        let v2 = get_with(&o, || {
            init_calls3.fetch_add(1, AtomicOrdering::SeqCst);
            NativeValue::Int(99) // would be wrong if it ran
        });

        assert_eq!(v1, NativeValue::Int(42));
        assert_eq!(v2, NativeValue::Int(42));
        assert_eq!(init_calls.load(AtomicOrdering::SeqCst), 1);
    }

    // -------------------- once: threaded race --------------------------------

    #[test]
    fn once_threaded_init_runs_exactly_once() {
        // `NativeValue` contains `Rc` variants and is therefore neither
        // `Send` nor `Sync`, so we can't put a `OnceLock<NativeValue>`
        // behind an `Arc` directly. The race semantics we want to verify
        // belong to `OnceLock` itself, so test those with a plain `i64`
        // and assert it via a fresh `OnceRepr` afterwards on the main
        // thread.
        let cell = Arc::new(OnceLock::<i64>::new());
        let counter = Arc::new(AtomicUsize::new(0));

        let mut handles = Vec::new();
        for _ in 0..16 {
            let cell = Arc::clone(&cell);
            let counter = Arc::clone(&counter);
            handles.push(thread::spawn(move || {
                let v = cell.get_or_init(|| {
                    counter.fetch_add(1, AtomicOrdering::SeqCst);
                    // Sleep briefly to widen the race window.
                    thread::sleep(Duration::from_millis(5));
                    7i64
                });
                assert_eq!(*v, 7);
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(
            counter.load(AtomicOrdering::SeqCst),
            1,
            "init must run exactly once across all racing threads"
        );

        // Sanity-check that the OnceRepr wrapper still drives init exactly
        // once on the main thread.
        let o_val = builtin_sync_once(&[NativeValue::Int(0)]).unwrap();
        let o = expect_once(&o_val).unwrap();
        let main_counter = std::cell::Cell::new(0u32);
        let v1 = get_with(&o, || {
            main_counter.set(main_counter.get() + 1);
            NativeValue::Int(99)
        });
        let v2 = get_with(&o, || {
            main_counter.set(main_counter.get() + 1);
            NativeValue::Int(123)
        });
        assert_eq!(v1, NativeValue::Int(99));
        assert_eq!(v2, NativeValue::Int(99));
        assert_eq!(main_counter.get(), 1);
    }

    // -------------------- argument-shape errors ------------------------------

    #[test]
    fn channel_negative_capacity_errors() {
        let res = builtin_sync_channel(&[NativeValue::Int(-1)]);
        assert!(res.is_err());
    }

    #[test]
    fn send_wrong_arg_count_errors() {
        let res = builtin_sync_send(&[]);
        assert!(res.is_err());
    }

    #[test]
    fn register_installs_all_eight() {
        let mut registry = NativeRegistry::new();
        let mut interner = Interner::new();
        register(&mut registry, &mut interner);

        for name in [
            "sync_channel",
            "sync_send",
            "sync_recv",
            "sync_try_recv",
            "sync_mutex",
            "sync_lock",
            "sync_once",
            "sync_get",
        ] {
            let sym = interner.intern(name);
            assert!(registry.get(sym).is_some(), "{name} should be registered");
        }
    }
}
