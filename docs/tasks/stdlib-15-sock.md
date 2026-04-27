# Task: Stdlib — `sock`

> Read `docs/tasks/stdlib-00-overview.md` first for the global rules.

## Scope

Implement the `sock` module from `docs/stdlib.md` (section "`sock` —
Sockets"). Low-level TCP and UDP.

## Output File

- `crates/ferric_stdlib/src/sock.rs`

Exposes `pub fn register(registry, interner)`.

## Required `NativeValue` Variants

```rust
// REQUIRED NATIVE VALUE VARIANTS:
//   Bytes(Vec<u8>)                              // shared
//   BytesMut(Rc<RefCell<Vec<u8>>>)              // shared
//   TcpStream(Rc<RefCell<std::net::TcpStream>>)
//   TcpListener(Rc<std::net::TcpListener>)
//   UdpSocket(Rc<std::net::UdpSocket>)
```

## Required Deps

None — `std::net` is sufficient. (Async sockets land with the runtime
in M3+.)

## Function Surface (must register)

TCP client:

`tcp_connect`, `tcp_connect_timeout`.

TCP stream ops:

`read`, `read_exact`, `read_line`, `write`, `write_all`, `write_str`,
`flush`, `close`, `set_timeout`, `local_addr`, `peer_addr`.

TCP server:

`tcp_listen`, `tcp_listen_addr`, `accept`, `accept_loop`.

UDP:

`udp_bind`, `udp_bind_addr`, `udp_send`, `udp_recv`.

## Implementation Notes

- `tcp_connect("host", port)` uses `TcpStream::connect((host, port))`.
- `tcp_connect_timeout` calls `connect_timeout` after resolving the
  address.
- `read(s, buf)` reads up to `buf.capacity()` bytes — but we represent
  `BytesMut` as a growable buffer. Spec the contract: pass a `BytesMut`
  with desired length pre-allocated; we read up to its current length.
  Returns bytes actually read.
- `read_exact(s, n)` blocks until exactly `n` bytes are read, or returns
  `SockError::Disconnected` on EOF.
- `read_line(s)` reads bytes until `\n` (consumed), returns the line
  without the trailing newline. UTF-8 decode at the end; on invalid UTF-8
  returns `SockError::Other`.
- `write_all(s, data)` keeps writing until the buffer is exhausted.
- `set_timeout(s, ms)` sets both read and write timeouts.
- Errors map:
  - `ConnectionRefused` → `SockError::ConnectionRefused`
  - `ConnectionReset` → `ConnectionReset`
  - `WouldBlock`/timeout → `Timeout`
  - `AddrInUse` → `AddressInUse`
- `accept_loop(l, handler)` blocks forever, invoking `handler(stream)` for
  each accept. Handler errors are logged (eprintln) but don't stop the
  loop.

## Tests

Tests need a port. Bind to `0` to let the OS assign one and report it via
`local_addr()`.

- `tcp_listen(0)` returns a listener; `local_addr` reports the port.
- Echo server smoke test:
  - Spawn a thread: `accept_loop(listener, |s| { read line; write line back })`
  - Client thread: `tcp_connect`, `write_str("hi\n")`, `read_line` →
    `Ok("hi")`.
- `tcp_connect_timeout("127.0.0.1", unused_port, 100)` → `ConnectionRefused`
  (or `Timeout` depending on platform; assert it's an error).
- `read_exact(s, 5)` on a stream with 3 bytes available then EOF →
  `Disconnected`.
- `set_timeout(s, 50)` then `read_line` on a silent connection → `Timeout`.
- UDP roundtrip:
  - `udp_bind(0)` two sockets;
  - one sends to the other;
  - `udp_recv` returns `(data, sender_addr)`.
- `local_addr` / `peer_addr` return non-empty strings.

Aim for ~15 tests. Each test must clean up its sockets.

## Constraints

- Do **not** modify `lib.rs` or `Cargo.toml`.
- All sockets in tests must use `127.0.0.1` and ephemeral (port 0)
  binding.

## Acceptance

- One file with the listed register coverage.
- Tests cover happy + timeout + connection-refused + EOF paths.
- No edits outside the new file.
