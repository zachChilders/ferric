//! # `sock` — TCP/UDP Sockets
//!
//! Low-level synchronous sockets backed by `std::net`.
//! Async variants land with the runtime in M3+.
//!
//! All functions in this module are registered with the `sock_*` prefix on
//! the [`NativeRegistry`]; from Ferric they are exposed as `sock::tcp_connect`,
//! `sock::read_line`, etc., per the spec in `docs/stdlib.md`.

// REQUIRED NATIVE VALUE VARIANTS:
//   Bytes(Vec<u8>)                              // shared
//   BytesMut(Rc<RefCell<Vec<u8>>>)              // shared
//   TcpStream(Rc<RefCell<std::net::TcpStream>>)
//   TcpListener(Rc<std::net::TcpListener>)
//   UdpSocket(Rc<std::net::UdpSocket>)

// REQUIRED DEPS:
//   (none — std::net is sufficient)

use std::cell::RefCell;
use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs, UdpSocket};
use std::rc::Rc;
use std::time::Duration;

use ferric_common::Interner;

use crate::{NativeRegistry, NativeValue};

// ============================================================================
// Local helpers
// ============================================================================
//
// Per the integration overview (rule 5: "Helpers are private per file."),
// these duplicate the helpers in `lib.rs`. Integration may dedupe later.

fn check_arg_count(args: &[NativeValue], expected: usize) -> Result<(), String> {
    if args.len() != expected {
        Err(format!("expected {} argument(s), got {}", expected, args.len()))
    } else {
        Ok(())
    }
}

fn expect_str(value: &NativeValue) -> Result<&String, String> {
    match value {
        NativeValue::Str(s) => Ok(s),
        other => Err(format!("expected string, got {:?}", other)),
    }
}

fn expect_int(value: &NativeValue) -> Result<i64, String> {
    match value {
        NativeValue::Int(n) => Ok(*n),
        other => Err(format!("expected int, got {:?}", other)),
    }
}

fn expect_bytes(value: &NativeValue) -> Result<Vec<u8>, String> {
    match value {
        NativeValue::Bytes(b) => Ok(b.clone()),
        NativeValue::BytesMut(buf) => Ok(buf.borrow().clone()),
        other => Err(format!("expected bytes, got {:?}", other)),
    }
}

fn expect_bytes_mut(value: &NativeValue) -> Result<Rc<RefCell<Vec<u8>>>, String> {
    match value {
        NativeValue::BytesMut(buf) => Ok(buf.clone()),
        other => Err(format!("expected mutable bytes buffer, got {:?}", other)),
    }
}

fn expect_tcp_stream(value: &NativeValue) -> Result<Rc<RefCell<TcpStream>>, String> {
    match value {
        NativeValue::TcpStream(s) => Ok(s.clone()),
        other => Err(format!("expected TcpStream, got {:?}", other)),
    }
}

fn expect_tcp_listener(value: &NativeValue) -> Result<Rc<TcpListener>, String> {
    match value {
        NativeValue::TcpListener(l) => Ok(l.clone()),
        other => Err(format!("expected TcpListener, got {:?}", other)),
    }
}

fn expect_udp_socket(value: &NativeValue) -> Result<Rc<UdpSocket>, String> {
    match value {
        NativeValue::UdpSocket(s) => Ok(s.clone()),
        other => Err(format!("expected UdpSocket, got {:?}", other)),
    }
}

/// Map a `std::io::Error` to one of the spec's `SockError` variants.
///
/// We surface the variant as a leading tag in the error string so that the VM
/// can later wrap each into a typed `SockError` once the algebraic-types
/// plumbing handles native errors. Until then, the tag is informational but
/// stable across calls — tests in this file assert on the prefix.
fn map_io_err(e: &std::io::Error, host: &str, port: i64) -> String {
    match e.kind() {
        ErrorKind::ConnectionRefused => {
            format!("ConnectionRefused: host={} port={}", host, port)
        }
        ErrorKind::ConnectionReset | ErrorKind::ConnectionAborted => {
            format!("ConnectionReset: {}", e)
        }
        ErrorKind::WouldBlock | ErrorKind::TimedOut => {
            format!("Timeout: {}", e)
        }
        ErrorKind::AddrInUse => {
            format!("AddressInUse: port={}", port)
        }
        ErrorKind::AddrNotAvailable | ErrorKind::InvalidInput => {
            format!("InvalidAddress: {}={}", host, e)
        }
        ErrorKind::UnexpectedEof => {
            format!("Disconnected: {}", e)
        }
        _ => format!("Other: {}", e),
    }
}

fn parse_port(n: i64) -> Result<u16, String> {
    if !(0..=u16::MAX as i64).contains(&n) {
        Err(format!("InvalidAddress: port {} out of range 0..=65535", n))
    } else {
        Ok(n as u16)
    }
}

// ============================================================================
// TCP client
// ============================================================================

fn builtin_sock_tcp_connect(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let host = expect_str(&args[0])?.clone();
    let port_i = expect_int(&args[1])?;
    let port = parse_port(port_i)?;

    let stream = TcpStream::connect((host.as_str(), port))
        .map_err(|e| map_io_err(&e, &host, port_i))?;
    Ok(NativeValue::TcpStream(Rc::new(RefCell::new(stream))))
}

fn builtin_sock_tcp_connect_timeout(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 3)?;
    let host = expect_str(&args[0])?.clone();
    let port_i = expect_int(&args[1])?;
    let port = parse_port(port_i)?;
    let ms = expect_int(&args[2])?;
    if ms < 0 {
        return Err(format!("Other: negative timeout {}", ms));
    }
    let dur = Duration::from_millis(ms as u64);

    // Resolve to a single SocketAddr so we can use connect_timeout.
    let mut addrs = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|e| map_io_err(&e, &host, port_i))?;
    let addr = addrs
        .next()
        .ok_or_else(|| format!("InvalidAddress: {}:{} resolved to no addresses", host, port))?;

    let stream = TcpStream::connect_timeout(&addr, dur)
        .map_err(|e| map_io_err(&e, &host, port_i))?;
    Ok(NativeValue::TcpStream(Rc::new(RefCell::new(stream))))
}

// ============================================================================
// TcpStream operations
// ============================================================================

/// Reads up to `buf.len()` bytes into the given `BytesMut` buffer.
///
/// The buffer must be pre-sized: we read up to its current length and overwrite
/// its contents in place. Returns the number of bytes actually read. The
/// caller can then call `bytes::slice` (once integrated) to trim the buffer to
/// the read length.
fn builtin_sock_read(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let stream_rc = expect_tcp_stream(&args[0])?;
    let buf_rc = expect_bytes_mut(&args[1])?;

    let mut buf = buf_rc.borrow_mut();
    if buf.is_empty() {
        return Ok(NativeValue::Int(0));
    }
    let n = stream_rc
        .borrow_mut()
        .read(&mut buf[..])
        .map_err(|e| map_io_err(&e, "", 0))?;
    Ok(NativeValue::Int(n as i64))
}

fn builtin_sock_read_exact(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let stream_rc = expect_tcp_stream(&args[0])?;
    let n = expect_int(&args[1])?;
    if n < 0 {
        return Err(format!("Other: negative byte count {}", n));
    }
    let mut buf = vec![0u8; n as usize];

    let mut stream = stream_rc.borrow_mut();
    let mut filled = 0usize;
    while filled < buf.len() {
        match stream.read(&mut buf[filled..]) {
            Ok(0) => {
                return Err(format!(
                    "Disconnected: EOF after {} of {} bytes",
                    filled,
                    buf.len()
                ));
            }
            Ok(k) => filled += k,
            Err(ref e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(map_io_err(&e, "", 0)),
        }
    }
    Ok(NativeValue::Bytes(buf))
}

/// Reads bytes from the stream until a `\n` is consumed, returning the line
/// without the trailing newline (and without a preceding `\r`, if present).
fn builtin_sock_read_line(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let stream_rc = expect_tcp_stream(&args[0])?;

    let mut buf: Vec<u8> = Vec::new();
    let mut byte = [0u8; 1];
    let mut stream = stream_rc.borrow_mut();
    loop {
        match stream.read(&mut byte) {
            Ok(0) => {
                return Err(format!(
                    "Disconnected: EOF after {} bytes without newline",
                    buf.len()
                ));
            }
            Ok(_) => {
                if byte[0] == b'\n' {
                    if buf.last().copied() == Some(b'\r') {
                        buf.pop();
                    }
                    break;
                } else {
                    buf.push(byte[0]);
                }
            }
            Err(ref e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(map_io_err(&e, "", 0)),
        }
    }
    let s = String::from_utf8(buf).map_err(|e| format!("Other: invalid UTF-8 in line: {}", e))?;
    Ok(NativeValue::Str(s))
}

fn builtin_sock_write(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let stream_rc = expect_tcp_stream(&args[0])?;
    let data = expect_bytes(&args[1])?;
    let n = stream_rc
        .borrow_mut()
        .write(&data)
        .map_err(|e| map_io_err(&e, "", 0))?;
    Ok(NativeValue::Int(n as i64))
}

fn builtin_sock_write_all(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let stream_rc = expect_tcp_stream(&args[0])?;
    let data = expect_bytes(&args[1])?;
    stream_rc
        .borrow_mut()
        .write_all(&data)
        .map_err(|e| map_io_err(&e, "", 0))?;
    Ok(NativeValue::Unit)
}

fn builtin_sock_write_str(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let stream_rc = expect_tcp_stream(&args[0])?;
    let s = expect_str(&args[1])?;
    stream_rc
        .borrow_mut()
        .write_all(s.as_bytes())
        .map_err(|e| map_io_err(&e, "", 0))?;
    Ok(NativeValue::Unit)
}

fn builtin_sock_flush(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let stream_rc = expect_tcp_stream(&args[0])?;
    stream_rc
        .borrow_mut()
        .flush()
        .map_err(|e| map_io_err(&e, "", 0))?;
    Ok(NativeValue::Unit)
}

fn builtin_sock_close(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let stream_rc = expect_tcp_stream(&args[0])?;
    // `TcpStream::shutdown(Both)` is the closest std::net offers without
    // dropping the value (the Rc may still be referenced elsewhere).
    use std::net::Shutdown;
    stream_rc
        .borrow()
        .shutdown(Shutdown::Both)
        .map_err(|e| map_io_err(&e, "", 0))?;
    Ok(NativeValue::Unit)
}

fn builtin_sock_set_timeout(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let stream_rc = expect_tcp_stream(&args[0])?;
    let ms = expect_int(&args[1])?;
    if ms < 0 {
        return Err(format!("Other: negative timeout {}", ms));
    }
    // A timeout of 0 is interpreted as "no timeout" (None), matching the
    // documented contract of std::net::TcpStream::set_read_timeout.
    let dur = if ms == 0 {
        None
    } else {
        Some(Duration::from_millis(ms as u64))
    };
    let stream = stream_rc.borrow();
    stream
        .set_read_timeout(dur)
        .map_err(|e| map_io_err(&e, "", 0))?;
    stream
        .set_write_timeout(dur)
        .map_err(|e| map_io_err(&e, "", 0))?;
    Ok(NativeValue::Unit)
}

fn builtin_sock_local_addr(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let stream_rc = expect_tcp_stream(&args[0])?;
    let addr = stream_rc
        .borrow()
        .local_addr()
        .map_err(|e| map_io_err(&e, "", 0))?;
    Ok(NativeValue::Str(addr.to_string()))
}

fn builtin_sock_peer_addr(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let stream_rc = expect_tcp_stream(&args[0])?;
    let addr = stream_rc
        .borrow()
        .peer_addr()
        .map_err(|e| map_io_err(&e, "", 0))?;
    Ok(NativeValue::Str(addr.to_string()))
}

// ============================================================================
// TCP server
// ============================================================================

fn builtin_sock_tcp_listen(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let port_i = expect_int(&args[0])?;
    let port = parse_port(port_i)?;
    let listener = TcpListener::bind(("127.0.0.1", port))
        .map_err(|e| map_io_err(&e, "127.0.0.1", port_i))?;
    Ok(NativeValue::TcpListener(Rc::new(listener)))
}

fn builtin_sock_tcp_listen_addr(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let addr = expect_str(&args[0])?.clone();
    let port_i = expect_int(&args[1])?;
    let port = parse_port(port_i)?;
    let listener = TcpListener::bind((addr.as_str(), port))
        .map_err(|e| map_io_err(&e, &addr, port_i))?;
    Ok(NativeValue::TcpListener(Rc::new(listener)))
}

fn builtin_sock_accept(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let listener_rc = expect_tcp_listener(&args[0])?;
    let (stream, _peer) = listener_rc.accept().map_err(|e| map_io_err(&e, "", 0))?;
    Ok(NativeValue::TcpStream(Rc::new(RefCell::new(stream))))
}

/// `accept_loop` accepts connections forever, invoking `handler(stream)` for
/// each. This native implementation cannot dispatch a Ferric closure on its
/// own — the VM owns closures — so we register a synchronous loop that returns
/// `Unit` only on a fatal listener error. Once the closure-callback ABI lands
/// (post-M3 alongside async), the VM will route this through itself instead.
///
/// For now we accept exactly one argument: the listener. A handler argument
/// is rejected with a stable error so callers see a clear surface; tests only
/// exercise the single-arg path indirectly via `accept` in a worker thread.
fn builtin_sock_accept_loop(args: &[NativeValue]) -> Result<NativeValue, String> {
    if args.len() != 2 {
        return Err(format!(
            "Other: sock_accept_loop expects (listener, handler); got {} arg(s). Handler dispatch is wired up by the VM closure ABI, not this native.",
            args.len()
        ));
    }
    // We *recognise* the call shape so the registration is non-trivial, but
    // refuse to actually loop without a VM-supplied dispatcher. Returning an
    // error is preferable to silently blocking the interpreter forever.
    Err("Other: sock_accept_loop requires VM closure dispatch (post-M3)".into())
}

// ============================================================================
// UDP
// ============================================================================

fn builtin_sock_udp_bind(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let port_i = expect_int(&args[0])?;
    let port = parse_port(port_i)?;
    let socket = UdpSocket::bind(("127.0.0.1", port))
        .map_err(|e| map_io_err(&e, "127.0.0.1", port_i))?;
    Ok(NativeValue::UdpSocket(Rc::new(socket)))
}

fn builtin_sock_udp_bind_addr(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 2)?;
    let addr = expect_str(&args[0])?.clone();
    let port_i = expect_int(&args[1])?;
    let port = parse_port(port_i)?;
    let socket = UdpSocket::bind((addr.as_str(), port))
        .map_err(|e| map_io_err(&e, &addr, port_i))?;
    Ok(NativeValue::UdpSocket(Rc::new(socket)))
}

fn builtin_sock_udp_send(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 4)?;
    let socket = expect_udp_socket(&args[0])?;
    let to = expect_str(&args[1])?.clone();
    let port_i = expect_int(&args[2])?;
    let port = parse_port(port_i)?;
    let data = expect_bytes(&args[3])?;
    let n = socket
        .send_to(&data, (to.as_str(), port))
        .map_err(|e| map_io_err(&e, &to, port_i))?;
    Ok(NativeValue::Int(n as i64))
}

/// Receives one datagram, returning `(Bytes, sender_addr_string)`. The buffer
/// is sized to 65_507 bytes — the maximum payload of a single UDP datagram on
/// IPv4 — so a recv never silently truncates a well-formed packet.
fn builtin_sock_udp_recv(args: &[NativeValue]) -> Result<NativeValue, String> {
    check_arg_count(args, 1)?;
    let socket = expect_udp_socket(&args[0])?;
    let mut buf = vec![0u8; 65_507];
    let (n, peer) = socket
        .recv_from(&mut buf)
        .map_err(|e| map_io_err(&e, "", 0))?;
    buf.truncate(n);
    Ok(NativeValue::Array(vec![
        NativeValue::Bytes(buf),
        NativeValue::Str(peer.to_string()),
    ]))
}

// ============================================================================
// Registration
// ============================================================================

/// Registers every `sock::*` native in the registry. Names use the
/// canonical `sock_<function>` convention from the stdlib overview.
pub fn register(registry: &mut NativeRegistry, interner: &mut Interner) {
    // TCP client
    registry.register(interner.intern("sock_tcp_connect"),         builtin_sock_tcp_connect);
    registry.register(interner.intern("sock_tcp_connect_timeout"), builtin_sock_tcp_connect_timeout);

    // TCP stream operations
    registry.register(interner.intern("sock_read"),        builtin_sock_read);
    registry.register(interner.intern("sock_read_exact"),  builtin_sock_read_exact);
    registry.register(interner.intern("sock_read_line"),   builtin_sock_read_line);
    registry.register(interner.intern("sock_write"),       builtin_sock_write);
    registry.register(interner.intern("sock_write_all"),   builtin_sock_write_all);
    registry.register(interner.intern("sock_write_str"),   builtin_sock_write_str);
    registry.register(interner.intern("sock_flush"),       builtin_sock_flush);
    registry.register(interner.intern("sock_close"),       builtin_sock_close);
    registry.register(interner.intern("sock_set_timeout"), builtin_sock_set_timeout);
    registry.register(interner.intern("sock_local_addr"),  builtin_sock_local_addr);
    registry.register(interner.intern("sock_peer_addr"),   builtin_sock_peer_addr);

    // TCP server
    registry.register(interner.intern("sock_tcp_listen"),      builtin_sock_tcp_listen);
    registry.register(interner.intern("sock_tcp_listen_addr"), builtin_sock_tcp_listen_addr);
    registry.register(interner.intern("sock_accept"),          builtin_sock_accept);
    registry.register(interner.intern("sock_accept_loop"),     builtin_sock_accept_loop);

    // UDP
    registry.register(interner.intern("sock_udp_bind"),      builtin_sock_udp_bind);
    registry.register(interner.intern("sock_udp_bind_addr"), builtin_sock_udp_bind_addr);
    registry.register(interner.intern("sock_udp_send"),      builtin_sock_udp_send);
    registry.register(interner.intern("sock_udp_recv"),      builtin_sock_udp_recv);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpStream as StdTcpStream;
    use std::thread;

    /// Bind a TCP listener on `127.0.0.1:0` and return both the
    /// `NativeValue::TcpListener` and the OS-assigned port.
    fn bind_ephemeral_tcp() -> (NativeValue, u16) {
        let v = builtin_sock_tcp_listen(&[NativeValue::Int(0)]).expect("listen");
        let listener = match &v {
            NativeValue::TcpListener(l) => l.clone(),
            other => panic!("expected TcpListener, got {:?}", other),
        };
        let port = listener.local_addr().expect("local_addr").port();
        (v, port)
    }

    fn bind_ephemeral_udp() -> (NativeValue, u16) {
        let v = builtin_sock_udp_bind(&[NativeValue::Int(0)]).expect("udp bind");
        let sock = match &v {
            NativeValue::UdpSocket(s) => s.clone(),
            other => panic!("expected UdpSocket, got {:?}", other),
        };
        let port = sock.local_addr().expect("local_addr").port();
        (v, port)
    }

    #[test]
    fn tcp_listen_assigns_port_and_local_addr_reports_it() {
        let (listener, port) = bind_ephemeral_tcp();
        assert!(port > 0);
        // Listener doesn't expose local_addr through our surface (only TcpStream does),
        // but a paired connect will let us check peer_addr resolves to that port.
        let join = thread::spawn(move || {
            let l = match listener {
                NativeValue::TcpListener(l) => l,
                _ => unreachable!(),
            };
            l.accept().expect("accept").0
        });
        let stream = StdTcpStream::connect(("127.0.0.1", port)).expect("connect");
        let _ = join.join().expect("join");
        let peer = stream.peer_addr().unwrap().to_string();
        assert!(peer.contains(&port.to_string()), "peer addr {peer} missing port {port}");
    }

    #[test]
    fn echo_server_smoke_test() {
        // Server side: accept once, read a line, write it back with newline.
        let (listener_v, port) = bind_ephemeral_tcp();
        let server = thread::spawn(move || {
            let l = match listener_v {
                NativeValue::TcpListener(l) => l,
                _ => unreachable!(),
            };
            let (stream, _peer) = l.accept().expect("server accept");
            let stream_v = NativeValue::TcpStream(Rc::new(RefCell::new(stream)));
            let line = builtin_sock_read_line(&[stream_v.clone()]).expect("read_line");
            let s = match line {
                NativeValue::Str(s) => s,
                other => panic!("expected str, got {:?}", other),
            };
            builtin_sock_write_str(&[
                stream_v.clone(),
                NativeValue::Str(format!("{}\n", s)),
            ])
            .expect("write_str");
        });

        let client = builtin_sock_tcp_connect(&[
            NativeValue::Str("127.0.0.1".into()),
            NativeValue::Int(port as i64),
        ])
        .expect("connect");
        builtin_sock_write_str(&[
            client.clone(),
            NativeValue::Str("hi\n".into()),
        ])
        .expect("client write");
        let resp = builtin_sock_read_line(&[client.clone()]).expect("client read");
        assert_eq!(resp, NativeValue::Str("hi".to_string()));
        let _ = builtin_sock_close(&[client]);
        server.join().expect("server thread");
    }

    #[test]
    fn tcp_connect_to_unused_port_is_an_error() {
        // Bind then drop a listener to obtain a port the OS just had open and
        // is unlikely to reassign during this brief window.
        let (l_v, port) = bind_ephemeral_tcp();
        drop(l_v);
        let r = builtin_sock_tcp_connect_timeout(&[
            NativeValue::Str("127.0.0.1".into()),
            NativeValue::Int(port as i64),
            NativeValue::Int(200),
        ]);
        assert!(r.is_err(), "expected error connecting to closed port, got {:?}", r);
        let msg = r.unwrap_err();
        assert!(
            msg.starts_with("ConnectionRefused")
                || msg.starts_with("Timeout")
                || msg.starts_with("Other"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn read_exact_returns_disconnected_on_eof() {
        let (listener_v, port) = bind_ephemeral_tcp();
        let server = thread::spawn(move || {
            let l = match listener_v {
                NativeValue::TcpListener(l) => l,
                _ => unreachable!(),
            };
            let (mut stream, _) = l.accept().expect("server accept");
            // Send only 3 bytes then close.
            stream.write_all(b"abc").expect("write");
            // dropping `stream` here closes the connection
        });

        let client = builtin_sock_tcp_connect(&[
            NativeValue::Str("127.0.0.1".into()),
            NativeValue::Int(port as i64),
        ])
        .expect("connect");
        let r = builtin_sock_read_exact(&[client.clone(), NativeValue::Int(5)]);
        assert!(r.is_err(), "expected EOF error, got {:?}", r);
        assert!(r.unwrap_err().starts_with("Disconnected"));
        let _ = builtin_sock_close(&[client]);
        server.join().expect("server thread");
    }

    #[test]
    fn set_timeout_on_silent_connection_yields_timeout() {
        let (listener_v, port) = bind_ephemeral_tcp();
        let server = thread::spawn(move || {
            let l = match listener_v {
                NativeValue::TcpListener(l) => l,
                _ => unreachable!(),
            };
            let (stream, _) = l.accept().expect("server accept");
            // Hold the connection open without sending anything until the
            // client times out and signals us by closing.
            // We just keep stream alive in this thread.
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let mut tmp = [0u8; 1];
            let _ = (&stream).read(&mut tmp); // returns once client closes
        });

        let client = builtin_sock_tcp_connect(&[
            NativeValue::Str("127.0.0.1".into()),
            NativeValue::Int(port as i64),
        ])
        .expect("connect");
        builtin_sock_set_timeout(&[client.clone(), NativeValue::Int(50)]).expect("set_timeout");
        let r = builtin_sock_read_line(&[client.clone()]);
        assert!(r.is_err(), "expected timeout, got {:?}", r);
        assert!(
            r.unwrap_err().starts_with("Timeout"),
            "expected Timeout prefix"
        );
        let _ = builtin_sock_close(&[client]);
        server.join().expect("server thread");
    }

    #[test]
    fn udp_roundtrip() {
        let (a, port_a) = bind_ephemeral_udp();
        let (b, _port_b) = bind_ephemeral_udp();

        let payload = NativeValue::Bytes(b"ping".to_vec());
        let sent = builtin_sock_udp_send(&[
            b.clone(),
            NativeValue::Str("127.0.0.1".into()),
            NativeValue::Int(port_a as i64),
            payload,
        ])
        .expect("udp send");
        assert_eq!(sent, NativeValue::Int(4));

        // Configure a generous read timeout via std::net so the test never hangs.
        if let NativeValue::UdpSocket(s) = &a {
            s.set_read_timeout(Some(Duration::from_secs(2))).expect("set_read_timeout");
        }
        let r = builtin_sock_udp_recv(&[a]).expect("udp recv");
        match r {
            NativeValue::Array(items) => {
                assert_eq!(items.len(), 2);
                assert_eq!(items[0], NativeValue::Bytes(b"ping".to_vec()));
                match &items[1] {
                    NativeValue::Str(s) => assert!(s.contains("127.0.0.1")),
                    other => panic!("expected sender addr str, got {:?}", other),
                }
            }
            other => panic!("expected Array tuple, got {:?}", other),
        }
    }

    #[test]
    fn local_and_peer_addr_strings() {
        let (listener_v, port) = bind_ephemeral_tcp();
        let server = thread::spawn(move || {
            let l = match listener_v {
                NativeValue::TcpListener(l) => l,
                _ => unreachable!(),
            };
            let (s, _) = l.accept().expect("accept");
            // Hold open until client closes.
            let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
            let mut tmp = [0u8; 1];
            let _ = (&s).read(&mut tmp);
        });
        let client = builtin_sock_tcp_connect(&[
            NativeValue::Str("127.0.0.1".into()),
            NativeValue::Int(port as i64),
        ])
        .expect("connect");
        let local = builtin_sock_local_addr(&[client.clone()]).expect("local_addr");
        let peer = builtin_sock_peer_addr(&[client.clone()]).expect("peer_addr");
        match (local, peer) {
            (NativeValue::Str(l), NativeValue::Str(p)) => {
                assert!(!l.is_empty());
                assert!(!p.is_empty());
                assert!(p.contains(&port.to_string()));
            }
            other => panic!("unexpected addr shape {:?}", other),
        }
        let _ = builtin_sock_close(&[client]);
        server.join().expect("server thread");
    }

    #[test]
    fn write_all_and_read_exact_matched_pair() {
        let (listener_v, port) = bind_ephemeral_tcp();
        let server = thread::spawn(move || {
            let l = match listener_v {
                NativeValue::TcpListener(l) => l,
                _ => unreachable!(),
            };
            let (stream, _) = l.accept().expect("accept");
            let stream_v = NativeValue::TcpStream(Rc::new(RefCell::new(stream)));
            // Read exactly 8 bytes, send them back.
            let r = builtin_sock_read_exact(&[stream_v.clone(), NativeValue::Int(8)])
                .expect("server read_exact");
            let bytes = match r {
                NativeValue::Bytes(b) => b,
                other => panic!("expected Bytes, got {:?}", other),
            };
            builtin_sock_write_all(&[stream_v.clone(), NativeValue::Bytes(bytes)])
                .expect("server write_all");
        });

        let client = builtin_sock_tcp_connect(&[
            NativeValue::Str("127.0.0.1".into()),
            NativeValue::Int(port as i64),
        ])
        .expect("connect");
        builtin_sock_write_all(&[client.clone(), NativeValue::Bytes(b"abcdefgh".to_vec())])
            .expect("client write_all");
        let r = builtin_sock_read_exact(&[client.clone(), NativeValue::Int(8)])
            .expect("client read_exact");
        assert_eq!(r, NativeValue::Bytes(b"abcdefgh".to_vec()));
        let _ = builtin_sock_close(&[client]);
        server.join().expect("server thread");
    }

    #[test]
    fn read_into_bytesmut_buffer_returns_byte_count() {
        let (listener_v, port) = bind_ephemeral_tcp();
        let server = thread::spawn(move || {
            let l = match listener_v {
                NativeValue::TcpListener(l) => l,
                _ => unreachable!(),
            };
            let (mut s, _) = l.accept().expect("accept");
            s.write_all(b"xyz").expect("write");
        });
        let client = builtin_sock_tcp_connect(&[
            NativeValue::Str("127.0.0.1".into()),
            NativeValue::Int(port as i64),
        ])
        .expect("connect");
        let buf = NativeValue::BytesMut(Rc::new(RefCell::new(vec![0u8; 16])));
        let n = builtin_sock_read(&[client.clone(), buf.clone()]).expect("read");
        // Either 3 bytes in one read, or possibly fewer if the kernel splits;
        // we assert we got *something* in (0, 16].
        match n {
            NativeValue::Int(k) => assert!(k > 0 && k <= 16, "unexpected read count {k}"),
            other => panic!("expected Int, got {:?}", other),
        }
        if let NativeValue::BytesMut(b) = &buf {
            // The first three bytes should match.
            let inner = b.borrow();
            assert_eq!(&inner[..3], b"xyz");
        }
        let _ = builtin_sock_close(&[client]);
        server.join().expect("server thread");
    }

    #[test]
    fn invalid_port_is_rejected() {
        let r = builtin_sock_tcp_listen(&[NativeValue::Int(70_000)]);
        assert!(r.is_err());
        assert!(r.unwrap_err().starts_with("InvalidAddress"));

        let r2 = builtin_sock_tcp_connect(&[
            NativeValue::Str("127.0.0.1".into()),
            NativeValue::Int(-1),
        ]);
        assert!(r2.is_err());
    }

    #[test]
    fn write_returns_byte_count() {
        let (listener_v, port) = bind_ephemeral_tcp();
        let server = thread::spawn(move || {
            let l = match listener_v {
                NativeValue::TcpListener(l) => l,
                _ => unreachable!(),
            };
            let (mut s, _) = l.accept().expect("accept");
            // Drain whatever the client sends.
            let mut buf = [0u8; 64];
            let _ = s.read(&mut buf);
        });
        let client = builtin_sock_tcp_connect(&[
            NativeValue::Str("127.0.0.1".into()),
            NativeValue::Int(port as i64),
        ])
        .expect("connect");
        let n = builtin_sock_write(&[
            client.clone(),
            NativeValue::Bytes(b"hello".to_vec()),
        ])
        .expect("write");
        assert_eq!(n, NativeValue::Int(5));
        let _ = builtin_sock_close(&[client]);
        server.join().expect("server thread");
    }

    #[test]
    fn flush_on_open_stream_is_ok() {
        let (listener_v, port) = bind_ephemeral_tcp();
        let server = thread::spawn(move || {
            let l = match listener_v {
                NativeValue::TcpListener(l) => l,
                _ => unreachable!(),
            };
            let (s, _) = l.accept().expect("accept");
            let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
            let mut tmp = [0u8; 1];
            let _ = (&s).read(&mut tmp);
        });
        let client = builtin_sock_tcp_connect(&[
            NativeValue::Str("127.0.0.1".into()),
            NativeValue::Int(port as i64),
        ])
        .expect("connect");
        let r = builtin_sock_flush(&[client.clone()]).expect("flush");
        assert_eq!(r, NativeValue::Unit);
        let _ = builtin_sock_close(&[client]);
        server.join().expect("server thread");
    }

    #[test]
    fn accept_returns_a_stream() {
        let (listener_v, port) = bind_ephemeral_tcp();
        let listener_clone = listener_v.clone();
        let acceptor = thread::spawn(move || {
            let r = builtin_sock_accept(&[listener_clone]).expect("accept");
            assert!(matches!(r, NativeValue::TcpStream(_)));
        });
        let _client = StdTcpStream::connect(("127.0.0.1", port)).expect("client connect");
        acceptor.join().expect("acceptor thread");
    }

    #[test]
    fn tcp_listen_addr_binds_to_explicit_loopback() {
        let r = builtin_sock_tcp_listen_addr(&[
            NativeValue::Str("127.0.0.1".into()),
            NativeValue::Int(0),
        ])
        .expect("listen_addr");
        let l = match r {
            NativeValue::TcpListener(l) => l,
            other => panic!("expected TcpListener, got {:?}", other),
        };
        let addr = l.local_addr().expect("local_addr").to_string();
        assert!(addr.starts_with("127.0.0.1:"));
    }

    #[test]
    fn udp_bind_addr_binds_to_explicit_loopback() {
        let r = builtin_sock_udp_bind_addr(&[
            NativeValue::Str("127.0.0.1".into()),
            NativeValue::Int(0),
        ])
        .expect("udp_bind_addr");
        let s = match r {
            NativeValue::UdpSocket(s) => s,
            other => panic!("expected UdpSocket, got {:?}", other),
        };
        let addr = s.local_addr().expect("local_addr").to_string();
        assert!(addr.starts_with("127.0.0.1:"));
    }

    #[test]
    fn accept_loop_rejects_call_without_handler_dispatch() {
        // The native cannot run a Ferric closure directly — until VM closure
        // dispatch lands, calling it is a clean error rather than a hang.
        let (listener_v, _port) = bind_ephemeral_tcp();
        let r = builtin_sock_accept_loop(&[
            listener_v,
            NativeValue::Int(0), // stand-in for a handler
        ]);
        assert!(r.is_err());
        assert!(r.unwrap_err().starts_with("Other:"));
    }

    #[test]
    fn arg_count_errors_are_descriptive() {
        let r = builtin_sock_tcp_connect(&[NativeValue::Str("x".into())]);
        assert!(r.is_err());
        assert!(r.unwrap_err().contains("expected 2 argument(s)"));
    }

    #[test]
    fn register_installs_every_sock_function() {
        let mut registry = NativeRegistry::new();
        let mut interner = ferric_common::Interner::new();
        register(&mut registry, &mut interner);
        for name in [
            "sock_tcp_connect",
            "sock_tcp_connect_timeout",
            "sock_read",
            "sock_read_exact",
            "sock_read_line",
            "sock_write",
            "sock_write_all",
            "sock_write_str",
            "sock_flush",
            "sock_close",
            "sock_set_timeout",
            "sock_local_addr",
            "sock_peer_addr",
            "sock_tcp_listen",
            "sock_tcp_listen_addr",
            "sock_accept",
            "sock_accept_loop",
            "sock_udp_bind",
            "sock_udp_bind_addr",
            "sock_udp_send",
            "sock_udp_recv",
        ] {
            let sym = interner.intern(name);
            assert!(
                registry.get(sym).is_some(),
                "expected {} to be registered",
                name
            );
        }
    }
}
