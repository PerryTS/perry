//! `node:cluster` worker port sharing for the Fastify listen site.
//!
//! When this process is a `cluster.fork()`ed worker (Node's convention: a
//! non-empty `NODE_UNIQUE_ID` in the environment, set by the runtime's
//! `cluster.fork`), the TCP bind goes through SO_REUSEPORT so N workers can
//! share one port — the kernel load-balances accepts across them, with no
//! primary-accept hop. The bound address is reported to the primary over the
//! cluster IPC so `cluster.on('listening')` fires Node-style.
//!
//! This wires Fastify into the cluster machinery that already exists in
//! perry-runtime (`worker_reuseport_bind`, `perry_cluster_worker_listening`)
//! and is used by `net` and perry-ext-http-server. It mirrors the SO_REUSEPORT
//! (`SCHED_NONE`) path of perry-ext-http-server's HTTP/2 & HTTPS listen sites.
//! Round-robin fd-passing (`SCHED_RR`) and the shared ephemeral port for
//! `listen(0)` (#4962) are a follow-up here, exactly as for those sites today.

use std::net::{SocketAddr, TcpListener};

/// True when this process is a `cluster.fork()`ed worker (non-empty
/// `NODE_UNIQUE_ID` in the environment — the same check the runtime and
/// perry-ext-http-server use).
pub(crate) fn is_cluster_worker() -> bool {
    std::env::var("NODE_UNIQUE_ID")
        .map(|s| !s.is_empty())
        .unwrap_or(false)
}

/// Bind `addr` with SO_REUSEPORT (+SO_REUSEADDR) so multiple cluster workers can
/// share the port (kernel-balanced accepts). Unix-only; the caller falls back
/// to the plain path elsewhere.
#[cfg(unix)]
fn reuseport_bind(addr: SocketAddr) -> std::io::Result<TcpListener> {
    use socket2::{Domain, Protocol, Socket, Type};
    let socket = Socket::new(Domain::for_address(addr), Type::STREAM, Some(Protocol::TCP))?;
    socket.set_reuse_address(true)?;
    socket.set_reuse_port(true)?;
    socket.bind(&addr.into())?;
    // Node's default listen backlog.
    socket.listen(511)?;
    Ok(socket.into())
}

/// Bind `addr`, enabling SO_REUSEPORT when this process is a cluster worker
/// **or** when the caller explicitly requested it via the `reusePort: true`
/// listen option, so multiple processes can share the port (kernel-balanced
/// accepts). `reusePort` is a real Node (`net`/`http` `listen`) and Bun
/// (`Bun.serve`) option; honoring it lets a non-cluster program opt into port
/// sharing directly. Non-worker, non-`reusePort` (or non-unix) binds keep the
/// plain `TcpListener::bind` path.
pub(crate) fn bind_listener(addr: SocketAddr, reuse_port: bool) -> std::io::Result<TcpListener> {
    #[cfg(unix)]
    if reuse_port || is_cluster_worker() {
        return reuseport_bind(addr);
    }
    // On non-unix targets SO_REUSEPORT isn't wired (matching the HTTP listen
    // sites); the explicit request is a no-op there rather than an error.
    #[cfg(not(unix))]
    let _ = reuse_port;
    TcpListener::bind(addr)
}

extern "C" {
    // Defined in perry-runtime's cluster module. This crate has no Cargo dep on
    // perry-runtime (dev-dep only); the symbol resolves at final link, the same
    // way perry-ffi's runtime helpers do — matching perry-ext-http-server's
    // `cluster_bind`.
    fn perry_cluster_worker_listening(
        addr_ptr: *const u8,
        addr_len: u32,
        port: i32,
        address_type: i32,
    );
}

/// Report this worker's bound `host:port` to the primary so
/// `cluster.on('listening')` fires Node-style. No-op unless this is a cluster
/// worker (re-checked on the runtime side as well).
pub(crate) fn notify_listening(host: &str, port: u16) {
    if !is_cluster_worker() {
        return;
    }
    let address_type = if host.contains(':') { 6 } else { 4 };
    unsafe {
        perry_cluster_worker_listening(host.as_ptr(), host.len() as u32, port as i32, address_type);
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// SO_REUSEPORT lets two listeners share one live port — the mechanism that
    /// makes `cluster.fork()` workers able to each `listen()` on the same port.
    #[test]
    fn reuseport_bind_lets_workers_share_a_port() {
        let any: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let first = reuseport_bind(any).expect("first reuseport bind");
        let port = first.local_addr().unwrap().port();
        let shared: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();

        // A second SO_REUSEPORT bind on the SAME live port succeeds.
        let second = reuseport_bind(shared).expect("second reuseport bind on the same port");
        assert_eq!(second.local_addr().unwrap().port(), port);

        // ...whereas a PLAIN bind on that live port is refused — proving the
        // shared bind above is SO_REUSEPORT doing the work, not an accident.
        assert!(
            TcpListener::bind(shared).is_err(),
            "a plain bind must be refused on a port already in use"
        );
    }

    /// `reusePort: true` enables SO_REUSEPORT even when this process is NOT a
    /// cluster worker — the explicit-option path (a real Node/Bun listen
    /// option). Two `bind_listener(_, true)` calls share one live port, while
    /// the non-worker, no-option path keeps the plain bind and is refused.
    #[test]
    fn explicit_reuse_port_option_shares_a_port_without_cluster() {
        // Sharing here comes purely from the explicit `reusePort = true`
        // argument, not from worker auto-detection — guard that invariant so a
        // stray NODE_UNIQUE_ID can't make this pass for the wrong reason.
        assert!(
            !is_cluster_worker(),
            "this test must not run as a cluster worker"
        );
        let any: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let first = bind_listener(any, true).expect("first reusePort bind");
        let port = first.local_addr().unwrap().port();
        let shared: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();

        let _second =
            bind_listener(shared, true).expect("second reusePort bind shares the same port");

        // Without the option (and not a worker), `bind_listener` takes the
        // plain path, which is refused on the SO_REUSEPORT-held port.
        assert!(
            bind_listener(shared, false).is_err(),
            "bind_listener(_, false) off-cluster must not share a reusePort-held port"
        );
    }
}
