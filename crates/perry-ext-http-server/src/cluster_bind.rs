//! #4914 — `node:cluster` worker port sharing for the HTTP/HTTPS/HTTP2
//! listen sites.
//!
//! When this process is a `cluster.fork()`ed worker (Node's convention:
//! non-empty `NODE_UNIQUE_ID` in the environment), every TCP bind goes
//! through SO_REUSEPORT so N workers can share one port, and the bound
//! address is reported to the primary over the fork IPC channel so
//! `cluster.on('listening')` fires Node-style. Kernel SO_REUSEPORT
//! balancing is effectively `SCHED_NONE`; round-robin fd-passing
//! (`SCHED_RR`) and the shared ephemeral port for `listen(0)` are #4962.

use std::net::{SocketAddr, TcpListener};

pub(crate) fn is_cluster_worker() -> bool {
    std::env::var("NODE_UNIQUE_ID")
        .map(|s| !s.is_empty())
        .unwrap_or(false)
}

/// Bind `addr`, with SO_REUSEPORT (+SO_REUSEADDR) when running as a cluster
/// worker. Non-worker binds keep the plain `TcpListener::bind` path.
pub(crate) fn bind_listener(addr: SocketAddr) -> std::io::Result<TcpListener> {
    #[cfg(unix)]
    if is_cluster_worker() {
        use socket2::{Domain, Protocol, Socket, Type};
        let socket = Socket::new(Domain::for_address(addr), Type::STREAM, Some(Protocol::TCP))?;
        socket.set_reuse_address(true)?;
        socket.set_reuse_port(true)?;
        socket.bind(&addr.into())?;
        // Node's default listen backlog.
        socket.listen(511)?;
        return Ok(socket.into());
    }
    TcpListener::bind(addr)
}

extern "C" {
    // Defined in perry-runtime's cluster.rs. This crate has no Cargo dep on
    // perry-runtime (dev-dep only); the symbol resolves at final link, the
    // same way perry-ffi's runtime helpers do.
    fn perry_cluster_worker_listening(
        addr_ptr: *const u8,
        addr_len: u32,
        port: i32,
        address_type: i32,
    );
}

/// Worker→primary `'listening'` report; no-op unless this is a cluster
/// worker (checked again on the runtime side).
pub(crate) fn notify_listening(host: &str, port: u16) {
    if !is_cluster_worker() {
        return;
    }
    let address_type = if host.contains(':') { 6 } else { 4 };
    unsafe {
        perry_cluster_worker_listening(host.as_ptr(), host.len() as u32, port as i32, address_type)
    }
}
