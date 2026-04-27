# Task: Stdlib Build-Out — Overview & Integration

This is the **coordination doc** for the parallel stdlib build-out described in
`docs/stdlib.md`. The spec is being implemented across 18 module tasks
(`stdlib-01-*` through `stdlib-18-*`), each producing one or more standalone
`.rs` files under `crates/ferric_stdlib/src/`.

This task itself has two halves:

1. **Pre-flight** (run before parallel agents start): nothing — agents are
   self-contained.
2. **Integration** (run after parallel agents finish): wire all the modules
   into the crate, extend `NativeValue`, add deps, and make `cargo build` and
   `cargo test -p ferric_stdlib` pass.

## Constraints That Apply To Every Module Task

These rules are repeated in each task file but stated canonically here.

1. **One module per task creates new files only.** Do not modify
   `crates/ferric_stdlib/src/lib.rs` or `crates/ferric_stdlib/Cargo.toml`.
   Those belong to this overview/integration task.
2. **The crate will not compile** during the parallel phase. That is expected.
   Tests are written as specs that will pass once integration runs.
3. **`NativeValue` extensions are declarative, not actual.** When a module
   needs a variant that doesn't exist (e.g., `Bytes`, `Map`, `Time`), it
   declares it in a `// REQUIRED NATIVE VALUE VARIANTS:` comment block at the
   top of its primary file. Inside the file, write Rust as if those variants
   already exist on `NativeValue`. Integration merges them.
4. **External crate deps are declarative, not actual.** Each task lists its
   required `Cargo.toml` deps in a `// REQUIRED DEPS:` comment block at the
   top of its file. Integration adds them.
5. **Helpers are private per file.** Each module file may include its own
   `check_arg_count`, `expect_str`, `expect_int`, etc. Do not try to share
   them across files during the parallel phase. Integration may dedupe.
6. **Tests live in the module file** under `#[cfg(test)] mod tests`. Tests
   should be written against the public function surface of that module.
   Tests that depend on extended `NativeValue` variants must still be
   written — they will compile after integration.
7. **Naming.** Module files are flat: `crates/ferric_stdlib/src/<mod>.rs`.
   Group tasks (e.g., str + bytes) produce one file per module
   (`str.rs`, `bytes.rs`).
8. **Function naming.** The spec uses `module::function` syntax in Ferric.
   The Rust-side native function name is `<module>_<function>` (e.g.,
   `str::trim` → registered as native `str_trim`). The implementation
   function in Rust is `builtin_<module>_<function>`.

## Integration Step (this task)

Run only after every `stdlib-01-*..stdlib-18-*` task is complete.

### 1. Extend `NativeValue`

Collect every `// REQUIRED NATIVE VALUE VARIANTS:` block from the module
files. Merge into the `NativeValue` enum in `lib.rs`. Expected new variants
include (non-exhaustive):

- `Bytes(Vec<u8>)`, `BytesMut(Rc<RefCell<Vec<u8>>>)`
- `List(Vec<NativeValue>)` — already present as `Array`; consider renaming or
  keeping `Array` as the canonical name
- `ListMut(Rc<RefCell<Vec<NativeValue>>>)`
- `Map(BTreeMap<NativeValue, NativeValue>)` and `MapMut`
- `Set(BTreeSet<NativeValue>)`
- `Time(...)`, `Duration(...)`
- `Json(...)` — likely a boxed enum
- `Regex(Rc<...>)`, `Match(...)`
- `Response`, `Request`, `RequestBuilder`, `Server`
- `TcpStream`, `TcpListener`, `UdpSocket`
- `Process`, `ProcOutput`, `Command`
- `Logger`, `Rng`
- `Sender`, `Receiver`, `Mutex`, `Once`
- `Ordering` (for `sort`)

Use `Rc<RefCell<...>>` for any handle that persists across calls (sockets,
file writers, builders, etc.).

### 2. Add `Cargo.toml` deps

Collect every `// REQUIRED DEPS:` block. Expected additions include
(non-exhaustive):

- `regex` (re)
- `serde_json` or hand-rolled (json)
- `argon2`, `chacha20poly1305`, `aes-gcm`, `sha2`, `blake3`, `hmac`,
  `ed25519-dalek` (crypto)
- `base64`, `hex`, `urlencoding` (encode)
- `chrono` or `time` crate (time)
- `rand`, `rand_chacha` (rand)
- `reqwest` blocking (http) — gate behind a feature
- `hyper` or `tiny_http` (serve)

### 3. Wire `mod` declarations and `register_stdlib`

In `lib.rs`:

```rust
mod bytes;
mod crypto;
mod encode;
mod env;
mod fmt;
mod http;
mod io;
mod json;
mod list;
mod log;
mod map;
mod math;
mod path;
mod proc_;       // proc is a reserved word — use proc_ or r#proc
mod rand_;
mod re;
mod serve;
mod set;
mod sock;
mod sort;
mod str_;        // str is a reserved word — use str_ or r#str
mod sync;
mod time_;

pub fn register_stdlib(registry: &mut NativeRegistry, interner: &mut Interner) {
    // existing M1/M2/M2.5/M3/M6 functions stay where they are

    str_::register(registry, interner);
    bytes::register(registry, interner);
    list::register(registry, interner);
    sort::register(registry, interner);
    map::register(registry, interner);
    set::register(registry, interner);
    math::register(registry, interner);
    io::register(registry, interner);
    path::register(registry, interner);
    env::register(registry, interner);
    time_::register(registry, interner);
    rand_::register(registry, interner);
    re::register(registry, interner);
    json::register(registry, interner);
    fmt::register(registry, interner);
    encode::register(registry, interner);
    crypto::register(registry, interner);
    http::register(registry, interner);
    serve::register(registry, interner);
    sock::register(registry, interner);
    proc_::register(registry, interner);
    log::register(registry, interner);
    sync::register(registry, interner);
}
```

Each module exposes one entry point: `pub fn register(registry: &mut
NativeRegistry, interner: &mut Interner)`.

### 4. Make tests pass

Run `cargo test -p ferric_stdlib`. Fix anything broken at the seams (most
likely: `NativeValue` variant names that drifted between files, helper name
collisions, missing `Eq`/`Hash`/`Ord` impls on `NativeValue` for `Map` /
`Set`).

### 5. Update `lib.rs` test module

Existing tests in `lib.rs` continue to live there. Per-module tests are
inside their files.

## Module → Task Map

| Module        | Task File                       |
|---------------|---------------------------------|
| str, bytes    | `stdlib-01-str-bytes.md`        |
| list, sort    | `stdlib-02-list-sort.md`        |
| map, set      | `stdlib-03-map-set.md`          |
| math          | `stdlib-04-math.md`             |
| io, path      | `stdlib-05-io-path.md`          |
| env           | `stdlib-06-env.md`              |
| time          | `stdlib-07-time.md`             |
| rand          | `stdlib-08-rand.md`             |
| re            | `stdlib-09-re.md`               |
| json, fmt     | `stdlib-10-json-fmt.md`         |
| encode        | `stdlib-11-encode.md`           |
| crypto        | `stdlib-12-crypto.md`           |
| http          | `stdlib-13-http.md`             |
| serve         | `stdlib-14-serve.md`            |
| sock          | `stdlib-15-sock.md`             |
| proc          | `stdlib-16-proc.md`             |
| log           | `stdlib-17-log.md`              |
| sync          | `stdlib-18-sync.md`             |

## Acceptance Criteria

- `cargo build` succeeds.
- `cargo test -p ferric_stdlib` passes.
- Every module file is `mod`-declared in `lib.rs`.
- Every module's `register()` is called by `register_stdlib`.
- No regressions in existing examples (`cargo test`).
