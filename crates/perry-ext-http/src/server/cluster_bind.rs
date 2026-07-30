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
    // Defined in perry-runtime's cluster.rs / cluster_sched.rs. This crate has
    // no Cargo dep on perry-runtime (dev-dep only); the symbols resolve at
    // final link, the same way perry-ffi's runtime helpers do.
    fn perry_cluster_worker_listening(
        addr_ptr: *const u8,
        addr_len: u32,
        port: i32,
        address_type: i32,
    );
    // #4962 — SCHED_RR / shared-port coordination.
    fn perry_cluster_worker_sched_is_rr() -> i32;
    fn perry_cluster_worker_query_listen(
        host_ptr: *const u8,
        host_len: u32,
        port: i32,
        address_type: i32,
        rr: i32,
    ) -> i32;
    fn perry_cluster_worker_recv_fd(key_id: u32) -> i32;
    fn perry_cluster_compute_key_id(
        host_ptr: *const u8,
        host_len: u32,
        port: i32,
        address_type: i32,
    ) -> u32;
}

/// True when this worker uses SCHED_RR (primary-owned socket + fd-passing).
pub(crate) fn worker_sched_is_rr() -> bool {
    unsafe { perry_cluster_worker_sched_is_rr() != 0 }
}

/// Ask the primary for the concrete port to bind `host:port` (#4962). `rr`
/// requests the fd-passing mode (primary owns the socket). Returns the resolved
/// port, or `None` on timeout/failure (caller falls back to a local bind).
pub(crate) fn worker_query_listen(
    host: &str,
    port: i32,
    address_type: i32,
    rr: bool,
) -> Option<u16> {
    let resolved = unsafe {
        perry_cluster_worker_query_listen(
            host.as_ptr(),
            host.len() as u32,
            port,
            address_type,
            if rr { 1 } else { 0 },
        )
    };
    if (0..=u16::MAX as i32).contains(&resolved) {
        Some(resolved as u16)
    } else {
        None
    }
}

/// Routing key id for a resolved address — must match the primary's fd-frame
/// tag for fds to land in this worker's queue.
pub(crate) fn compute_key_id(host: &str, port: u16, address_type: i32) -> u32 {
    unsafe {
        perry_cluster_compute_key_id(host.as_ptr(), host.len() as u32, port as i32, address_type)
    }
}

/// Block for the next SCHED_RR connection fd for `key_id`; -1 on channel close.
#[cfg(unix)]
pub(crate) fn recv_fd(key_id: u32) -> std::os::unix::io::RawFd {
    unsafe { perry_cluster_worker_recv_fd(key_id) }
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
