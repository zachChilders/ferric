//! Auto-extracted metadata for the `sock` stdlib module.
//!
//! Names + param names mirror `crates/ferric_stdlib/src/sock.rs`.
//! Type signatures are `Signature::Unknown` unless filled in by hand.

use crate::{FunctionMeta, Signature};

pub const SOCK_FNS: &[FunctionMeta] = &[
    FunctionMeta { name: "sock_tcp_connect", params: &["host", "port"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "sock_tcp_connect_timeout", params: &["host", "port", "ms"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "sock_read", params: &["s", "buf"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "sock_read_exact", params: &["s", "n"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "sock_read_line", params: &["s"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "sock_write", params: &["s", "data"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "sock_write_all", params: &["s", "data"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "sock_write_str", params: &["s", "data"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "sock_flush", params: &["s"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "sock_close", params: &["s"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "sock_set_timeout", params: &["s", "ms"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "sock_local_addr", params: &["s"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "sock_peer_addr", params: &["s"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "sock_tcp_listen", params: &["port"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "sock_tcp_listen_addr", params: &["addr", "port"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "sock_accept", params: &["l"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "sock_accept_loop", params: &["l", "handler"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "sock_udp_bind", params: &["port"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "sock_udp_bind_addr", params: &["addr", "port"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "sock_udp_send", params: &["s", "to", "port", "data"], signature: Signature::Unknown, display: "fn(...)" },
    FunctionMeta { name: "sock_udp_recv", params: &["s"], signature: Signature::Unknown, display: "fn(...)" },
];
