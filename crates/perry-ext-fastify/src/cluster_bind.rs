//! `node:cluster` worker port sharing for the Fastify listen site.
//!
//! When this process is a `cluster.fork()`ed worker, the TCP bind goes through
//! SO_REUSEPORT so N workers can share one port — the kernel load-balances
//! accepts across them without a primary-accept hop. The bound address is
//! reported to the primary over the
//! cluster IPC so `cluster.on('listening')` fires Node-style.
//!
//! This wires Fastify into the cluster machinery that already exists in
//! perry-runtime (`worker_reuseport_bind`, `perry_cluster_worker_listening`)
//! and is used by `net` and perry-ext-http. It mirrors the SO_REUSEPORT
//! (`SCHED_NONE`) path of perry-ext-http's HTTP/2 & HTTPS listen sites.
//! Round-robin fd-passing (`SCHED_RR`) and the shared ephemeral port for
//! `listen(0)` (#4962) are a follow-up here, exactly as for those sites today.

/// True when this process is a `cluster.fork()`ed worker. The runtime caches
/// this before consuming Node's bootstrap-only `NODE_UNIQUE_ID` variable.
pub(crate) fn is_cluster_worker() -> bool {
    unsafe { perry_cluster_is_worker() != 0 }
}

// The SO_REUSEPORT bind itself is no longer here. It existed for the hyper
// accept loop, which owned a `std::net::TcpListener`; `perry_http_server::listen`
// takes `reuse_port` as an argument and sets the option on the socket turnloop
// binds, so there is one binder rather than two. What stays is
// `is_cluster_worker`, which decides whether to pass it, and
// `notify_listening`, which reports the bound address to the cluster primary.

extern "C" {
    // Defined in perry-runtime's cluster module. This crate has no Cargo dep on
    // perry-runtime (dev-dep only); the symbol resolves at final link, the same
    // way perry-ffi's runtime helpers do — matching perry-ext-http's
    // `cluster_bind`.
    fn perry_cluster_is_worker() -> i32;
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

// The SO_REUSEPORT tests went with the binder. They asserted that two
// `reuseport_bind`/`bind_listener` calls could share one live port while a
// plain `TcpListener::bind` on it was refused — a property of a socket this
// crate no longer opens. `perry_http_server::listen` takes `reuse_port` and
// sets the option on the socket turnloop binds, so the behaviour and its
// coverage belong there rather than to a second binder kept alive by its own
// test.
