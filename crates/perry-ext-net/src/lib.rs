//! Native bindings for Node `net.Socket` — TCP plus optional TLS upgrade.
//!
//! Ported from `crates/perry-stdlib/src/net/mod.rs` to perry-ffi v0.5.x's
//! stable surface as part of #466 Phase 5. Architecturally the same as the
//! perry-stdlib copy: one tokio task per socket reads in a `select!` loop
//! and drives an mpsc command channel for writes/end/destroy/upgrade. Read
//! data is queued as a zero-copy `Bytes` view (sliced out of the socket
//! task's reused read buffer) into `NET_PENDING_EVENTS` and converted
//! to `Buffer` on the main thread inside `js_net_process_pending` — the
//! same arena-safety rule as perry-stdlib (JSValue construction MUST run
//! on the main thread, never on a tokio worker).
//!
//! # Differences from the perry-stdlib version
//!
//! - Uses `perry_ffi::spawn_async` to drive each socket reader / server accept
//!   loop cooperatively on Perry's shared multi-thread runtime (the same
//!   reactor `crate::common::async_bridge` drives), rather than spinning a
//!   throwaway current-thread runtime on a blocking-pool thread per socket.
//!   Keepalive comes from `js_ext_net_has_active_handles` (the socket/server is
//!   registered synchronously before the spawn), not the blocking-pool
//!   active-handle counter.
//! - Uses `perry_ffi::JsClosure` instead of raw `js_closure_call*` extern fns.
//! - Uses `perry_ffi::alloc_buffer` / `BufferHeader` instead of
//!   `perry-runtime::buffer::*` directly.
//! - GC root scanner registered via `perry_ffi::gc_register_mutable_root_scanner`.
//!   Listeners stored inside the `NET_LISTENERS` map need this — issue #35
//!   pattern — and the mutable visitor lets copied-minor GC rewrite moved
//!   closure pointers in place.
//!
//! TLS is unconditionally compiled in (no `#[cfg(feature = "tls")]` gates
//! like perry-stdlib has) — keeping the wrapper crate simple, the deps are
//! small. perry-stdlib's umbrella `net = ["async-runtime"]` + separate
//! `tls = ["net", ...]` feature split is preserved on the perry-stdlib side
//! for backwards compat; the well-known flip routes here.

use bytes::{BufMut, Bytes};
use perry_ffi::{
    alloc_buffer, alloc_string, gc_register_mutable_root_scanner_named, GcRootVisitor, JsClosure,
    JsPromise, JsValue, RawClosureHeader, StringHeader,
};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot};

// #1852 — topical sub-modules split out to keep this file under the
// 2000-line size gate. `tls` holds the rustls config + handshake; `ip`
// holds the `net.isIP*` + auto-select-family helpers.
mod ip;
// Process-wide freelist of read buffers, so the socket read loop
// recycles pooled 16 KiB capacity instead of allocating a fresh
// `BytesMut` per read. See `buffer_pool.rs` for the rationale.
mod buffer_pool;
mod tls;
pub use tls::{js_ext_tls_connect, js_tls_connect};
// #2131 — lifecycle / EventEmitter surface for `net.Socket` + `net.Server`
// (once / off / removeAllListeners / listenerCount / eventNames /
// resetAndDestroy, plus `socket.address()`). Re-exports keep the
// `pub unsafe extern "C" fn js_net_*` symbols at the crate root so the
// ext_registry well-known flip + native_table entries link the same as
// the rest of the FFI surface.
mod lifecycle;
pub use lifecycle::*;
mod classes;
pub use classes::*;
mod handle_ids;
pub(crate) use handle_ids::{next_id, next_id_or_throw};
mod dispatch;
mod dispatch_custody;
mod gc_roots;
mod ipc;
pub(crate) use gc_roots::ensure_gc_scanner_registered;
mod socket_emit;
pub use socket_emit::{
    js_ext_net_register_http_agent_socket_event_hook, js_ext_net_set_http_agent_phase,
    js_ext_net_socket_emit, js_ext_net_socket_emit_abort_error,
};
mod task_spawn;
use task_spawn::spawn_socket_runner;
// #2154 — raw-consumer bridge so perry-ext-http can drive an HTTP exchange
// over a socket produced by `agent.createConnection` (split out for the gate).
mod provider_lifecycle;
mod raw_bridge;
use provider_lifecycle::{
    event_provider_id, init_provider, init_provider_with_trigger, prepare_event_provider,
    ProviderScope,
};
use raw_bridge::RawReadState;
// #2013 — chainable option-setter no-ops + Node arg-validation bridge to
// perry-runtime (split out to keep lib.rs under the 2000-line gate). The
// `#[no_mangle]` setter/setTimeout symbols re-export at the crate root; the
// validator `extern` declarations are imported for the listen/connect sites.
mod adopt;
pub use adopt::{adopt_upgraded_tcp_stream, ensure_adopted_socket_dispatch};
mod option_setters;
pub use option_setters::{
    js_net_server_noop_self, js_net_socket_get_type_of_service, js_net_socket_noop_self,
    js_net_socket_ref, js_net_socket_set_encoding, js_net_socket_set_no_delay,
    js_net_socket_set_timeout, js_net_socket_set_type_of_service, js_net_socket_unref,
};
use option_setters::{js_net_validate_connect_port, js_net_validate_listen_port};
mod socket_facade;
pub(crate) use socket_facade::TlsSocketMetadata;
pub use socket_facade::{
    js_ext_net_has_active_handles, js_ext_net_is_socket_handle, js_ext_net_set_tls_metadata,
    js_ext_net_socket_has_ref, js_ext_net_socket_peer_certificate_json, js_ext_net_socket_set_ref,
    js_ext_net_socket_tls_authorized, js_ext_net_socket_tls_encrypted,
    js_ext_net_socket_tls_servername, js_ext_net_socket_tls_session,
    js_ext_net_socket_tls_session_reused,
};

#[cfg(test)]
mod nodelay_tests;
mod server_state;
#[cfg(test)]
mod test_async_shims;
pub use server_state::*;
// NaN-box value-conversion helpers (string/buffer/number/bool extraction +
// the `Error`-shaped object builder) split out for the file-size gate. The
// `crate::<fn>` re-export keeps every existing call site — here and in the
// `tls` / `classes` / `ip` / `lifecycle` / `option_setters` siblings —
// unchanged.
mod jsvalue;
pub(crate) use jsvalue::{
    build_error_object, get_object_bool_field, get_object_number_field, get_object_string_field,
    get_object_value_field, is_nanboxed_pointer, jsvalue_to_socket_bytes, string_from_header_i64,
    unbox_pointer,
};

use crate::tls::{do_tls_handshake, record_tls_handshake, TlsClientConfigData};

// ─── Transport enum (plain or TLS, swappable at runtime) ─────────────────────
//
// Split out to `transport.rs` for the 2000-line file-size gate. The
// `pub(crate)` re-export keeps `crate::Transport` resolving unchanged for
// the `adopt` / `nodelay_tests` siblings and for this file.
mod transport;
pub(crate) use transport::Transport;

// ─── Handle storage ──────────────────────────────────────────────────────────
//
// We keep our own integer-keyed handle map here rather than going through
// perry-ffi's generic registry, because every socket needs *two* parallel
// data structures (state + listeners) keyed by the same id. Splitting them
// across two registry types would force two lookups per FFI entry; bundling
// them into one `SocketHandle` value would make the GC scanner walk noisier.
// The pattern matches perry-stdlib's existing copy exactly.

pub(crate) mod statics {
    use super::*;
    use std::sync::OnceLock;

    pub fn sockets() -> &'static Mutex<HashMap<i64, SocketState>> {
        static S: OnceLock<Mutex<HashMap<i64, SocketState>>> = OnceLock::new();
        S.get_or_init(|| Mutex::new(HashMap::new()))
    }

    pub fn listeners() -> &'static Mutex<HashMap<i64, HashMap<String, Vec<i64>>>> {
        static L: OnceLock<Mutex<HashMap<i64, HashMap<String, Vec<i64>>>>> = OnceLock::new();
        L.get_or_init(|| Mutex::new(HashMap::new()))
    }

    /// Issue #2131 — closure pointers that were registered via
    /// `socket.once(event, cb)` / `server.once(event, cb)`. Keyed by
    /// handle id (socket OR server — they share the listener namespace)
    /// then event name. After the pump fires an event, any callback in
    /// this set is removed from both the regular listener vector AND
    /// this set, giving Node's "fire once and auto-remove" semantics.
    /// Kept as a side table so the flat `Vec<i64>` listener storage
    /// (and the GC scanner that walks it) stays unchanged.
    pub fn once_flags(
    ) -> &'static Mutex<HashMap<i64, HashMap<String, std::collections::HashSet<i64>>>> {
        static O: OnceLock<Mutex<HashMap<i64, HashMap<String, std::collections::HashSet<i64>>>>> =
            OnceLock::new();
        O.get_or_init(|| Mutex::new(HashMap::new()))
    }

    pub fn pending_events() -> &'static Mutex<Vec<PendingNetEvent>> {
        static P: OnceLock<Mutex<Vec<PendingNetEvent>>> = OnceLock::new();
        P.get_or_init(|| Mutex::new(Vec::new()))
    }

    /// HTTP Agent-owned socket handles are transport facades over the HTTP
    /// client's private connection pool. `true` means assigned to a request;
    /// `false` means parked in `agent.freeSockets`. Keeping this tiny bit of
    /// metadata here lets the ordinary net.Socket EventEmitter surface expose
    /// Node's internal listener counts without leaking HTTP internals into the
    /// generic handle dispatcher.
    pub fn http_agent_phases() -> &'static Mutex<HashMap<i64, bool>> {
        static H: OnceLock<Mutex<HashMap<i64, bool>>> = OnceLock::new();
        H.get_or_init(|| Mutex::new(HashMap::new()))
    }

    /// Server registry — `net.createServer(...)` returns a handle here.
    /// Separate from the socket map: server handles host an accept-loop
    /// shutdown channel and a bound port; sockets host a per-connection
    /// command channel + per-connection listener map. Keyed by the same
    /// monotonic id counter as sockets, so handles never collide.
    pub fn servers() -> &'static Mutex<HashMap<i64, ServerState>> {
        static S: OnceLock<Mutex<HashMap<i64, ServerState>>> = OnceLock::new();
        S.get_or_init(|| Mutex::new(HashMap::new()))
    }

    /// #4973 — per-socket read encoding set via `socket.setEncoding(enc)`.
    /// When present, the main-thread pump delivers `'data'` as a decoded
    /// string instead of a Buffer (Node readable-stream semantics). Side
    /// table (not a SocketState field) so the many SocketState literal
    /// constructions stay untouched.
    pub fn encodings() -> &'static Mutex<HashMap<i64, String>> {
        static E: OnceLock<Mutex<HashMap<i64, String>>> = OnceLock::new();
        E.get_or_init(|| Mutex::new(HashMap::new()))
    }

    /// Per-socket EventEmitter warning thresholds set through `events.*`.
    pub fn max_listeners() -> &'static Mutex<HashMap<i64, f64>> {
        static M: OnceLock<Mutex<HashMap<i64, f64>>> = OnceLock::new();
        M.get_or_init(|| Mutex::new(HashMap::new()))
    }
}

/// Backing state for an `net.Server` handle (`net.createServer(...)`).
/// Mirrors `perry-ext-http::HttpServer` in shape but stripped to
/// the raw-TCP surface — no hyper, no request/response channels, just
/// the accept loop's shutdown sender + bound address. Per-server event
/// listeners (`'connection'`, `'listening'`, `'close'`, `'error'`) live
/// in the shared `statics::listeners()` map keyed by the server's id;
/// reusing the socket listener map keeps the GC scanner walk single-
/// pass instead of needing a second per-server scanner.
pub(crate) struct ServerState {
    pub async_id: u64,
    /// Set by `.listen()`, dropped by `.close()`. Send on this channel
    /// to break the accept loop's `tokio::select!`.
    pub shutdown_tx: Option<oneshot::Sender<()>>,
    pub bound_port: u16,
    pub bound_host: String,
    /// Named-pipe / Unix-domain-socket path for an IPC listener. TCP servers
    /// leave this unset and use `bound_host` + `bound_port`.
    pub bound_path: Option<String>,
    pub listening: bool,
    pub active_connections: usize,
    pub pending_connections: usize,
    pub pending_local_connect_events: usize,
    pub max_connections: Option<usize>,
    pub drop_max_connection: Option<bool>,
}

pub(crate) struct SocketState {
    pub(crate) tcp_async_id: u64,
    pub(crate) connect_async_id: u64,
    pub(crate) shutdown_async_id: u64,
    pub(crate) cmd_tx: mpsc::UnboundedSender<SocketCommand>,
    /// `Some` only between `js_net_socket_alloc` and the first
    /// `js_net_socket_method_connect`. Held here so the deferred-connect
    /// path (issue #422: `new net.Socket()` then `sock.connect(port,host)`)
    /// can move it into the spawned task at connect time.
    pub(crate) pending_rx: Option<mpsc::UnboundedReceiver<SocketCommand>>,
    pub(crate) is_open: bool,
    /// Whether pending socket I/O keeps the process event loop alive.
    pub(crate) refed: bool,
    /// Issue #2131 — the kernel-assigned local address, populated after
    /// `TcpStream::connect`/`accept`. Drives `socket.address()` so the
    /// "undefined.address" cluster reports the actual bound port/family.
    pub(crate) local_addr: Option<SocketAddr>,
    /// #2154 — raw-consumer mode (see `raw_bridge`). When `Some`,
    /// `run_socket_task` buffers inbound bytes here for `perry-ext-http` to
    /// drain instead of firing JS `'data'` events.
    raw: Option<Arc<Mutex<RawReadState>>>,
    /// #2549 — Node `net.Socket` lifecycle/counter property surface.
    /// `destroyed` flips true on `.destroy()`/peer close; drives
    /// `socket.destroyed` and the `readyState` string. Byte counters track
    /// `socket.bytesRead`/`socket.bytesWritten`. `timeout` holds the value set
    /// via `setTimeout(ms)` (Node reports `undefined` until one is set).
    pub(crate) destroyed: bool,
    pub(crate) bytes_read: u64,
    pub(crate) bytes_written: u64,
    pub(crate) timeout: Option<u64>,
    pub(crate) type_of_service: u8,
    pub(crate) server_id: Option<i64>,
    pub(crate) server_connection_active: bool,
    pub(crate) tls: TlsSocketMetadata,
}

#[cfg(test)]
impl SocketState {
    /// Minimal open socket state wired to a command channel — for the nodelay
    /// command-path test, which only needs `cmd_tx` to reach `run_socket_task`.
    pub(crate) fn for_test(cmd_tx: mpsc::UnboundedSender<SocketCommand>) -> Self {
        SocketState {
            tcp_async_id: 0,
            connect_async_id: 0,
            shutdown_async_id: 0,
            cmd_tx,
            pending_rx: None,
            is_open: true,
            refed: true,
            local_addr: None,
            raw: None,
            destroyed: false,
            bytes_read: 0,
            bytes_written: 0,
            timeout: None,
            type_of_service: 0,
            server_id: None,
            server_connection_active: false,
            tls: TlsSocketMetadata::default(),
        }
    }
}

pub(crate) enum SocketCommand {
    Write(Vec<u8>),
    End,
    Destroy,
    /// `socket.setNoDelay(enable)` — applies `TCP_NODELAY` to the live socket.
    /// Carried as a command (rather than a flag on `SocketState`) because the
    /// owning `TcpStream`/TLS wrapper lives in `run_socket_task`, not in the
    /// handle map. The channel is unbounded, so a `setNoDelay` issued on a
    /// deferred-connect socket before it connects is buffered and applied once
    /// the task starts — after the connect site has set the Node default ON,
    /// so an explicit opt-out wins.
    SetNoDelay(bool),
    /// The main thread finished dispatching the accepted socket's
    /// `connection` callback. Commands queued by that callback precede this
    /// marker, so a peer FIN may now auto-close without dropping its response.
    ServerConnectionReady,
    /// Test-only: report the live socket's `TCP_NODELAY` state back over a
    /// oneshot, so the command-path test can observe `setNoDelay` taking
    /// effect on the stream the task owns.
    #[cfg(test)]
    QueryNoDelay(oneshot::Sender<bool>),
    UpgradeTls {
        servername: String,
        verify: bool,
        config: TlsClientConfigData,
        reply: oneshot::Sender<Result<(), String>>,
    },
}

#[derive(Debug)]
enum PendingNetEvent {
    /// `.1` identifies a same-process server target and whether its admission
    /// is expected to hit `dropMaxConnection`; external connects use `None`.
    Connect(i64, Option<(i64, bool)>),
    SecureConnect(i64),
    /// One chunk of read data. Carried as a refcounted `Bytes` — a zero-copy
    /// view sliced out of the socket task's reused read buffer (`split_to`) —
    /// so the path from the receive buffer to the main-thread drain handler
    /// (which only borrows it as `&[u8]`) stays alloc-free per read.
    Data(i64, Bytes),
    /// Issue #1852 — peer half-closed (FIN received, `read()` returned 0).
    /// Node fires `'end'` on the readable side *before* `'close'`; lots of
    /// net tests block on `socket.on('end', …)` to learn the peer is done,
    /// so without this the connection lifecycle never completes and the
    /// test hangs.
    End(i64),
    /// Completion of the writable-side shutdown requested by `socket.end()`.
    /// This is distinct from `End`, which is the peer's readable-side FIN and
    /// fires the public `end` event.
    ShutdownComplete(i64),
    Close(i64),
    Error(i64, String),
    AbortError(i64),
    /// Issue #1123 followup — accept-loop on a `net.Server` produced
    /// a new client socket. Fires the server's `'connection'`
    /// listeners with the new socket handle.
    ///   `.0` = server id (for listener lookup)
    ///   `.1` = socket id (passed to listeners as the arg)
    ///   `.2` = loopback client callback has crossed a pump boundary
    ServerConnection(i64, i64, bool),
    /// Issue #1123 followup — `listener.bind()` resolved + accept
    /// loop is running. Fires `'listening'` listeners + the
    /// `.listen(port, cb)` callback. `.0` = server id.
    ServerListening(i64),
    /// Issue #1123 followup — accept-loop exited (after `.close()`
    /// or bind failure). Fires `'close'` listeners on the server.
    ServerClose(i64),
    /// Issue #1123 followup — bind / accept I/O error on the server.
    /// Fires `'error'` listeners with an Error-shaped object.
    ///   `.0` = server id, `.1` = error message.
    ServerError(i64, String),
    ServerDrop(i64, server_state::DropInfo),
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

// Runtime entrypoint provided by perry-runtime (declared as extern so
// perry-ext-net doesn't need to depend on the perry-runtime rlib). The
// NaN-box value-conversion helpers and their runtime externs now live in
// `jsvalue.rs` (split out for the file-size gate); this one stays here
// because `js_net_create_server` below resolves the `connectionListener`
// callback pointer with it.
extern "C" {
    fn js_net_callback_ptr(value: f64) -> i64;
    fn js_get_string_pointer_unified(value: f64) -> i64;
    fn js_async_hooks_provider_init(type_ptr: *const u8, type_len: usize) -> u64;
    fn js_async_hooks_provider_init_with_trigger(
        type_ptr: *const u8,
        type_len: usize,
        trigger_async_id: u64,
    ) -> u64;
    fn js_async_hooks_provider_enter(async_id: u64);
    fn js_async_hooks_provider_leave(async_id: u64);
    fn js_async_hooks_provider_destroy(async_id: u64);
    fn perry_cluster_worker_listening(
        addr_ptr: *const u8,
        addr_len: u32,
        port: i32,
        address_type: i32,
    );
}

fn push_event(ev: PendingNetEvent) {
    if let PendingNetEvent::ServerConnection(server_id, socket_id, false) = &ev {
        if !server_state::queue_server_connection(*server_id, *socket_id) {
            return;
        }
    }
    statics::pending_events().lock().unwrap().push(ev);
    // Wake the main thread so its `js_wait_for_event` returns
    // promptly instead of waiting on the heartbeat cap (#84
    // sub-millisecond responsiveness). perry-ffi shipped this
    // surface in v0.5.567.
    perry_ffi::notify_main_thread();
}

fn mark_closed(id: i64) {
    server_state::mark_socket_closed(id);
}

// ─── FFI: net.createConnection / net.connect ─────────────────────────────────

/// `net.createConnection(...)` / `net.connect(...)` — returns a handle
/// immediately; connection happens in the background and emits
/// `'connect'` or `'error'`. Supports Node's TCP and IPC overloads:
///
/// - Positional: `net.connect(port, host, cb?)`. `arg1_f64` is the
///   port as a regular f64 number, `arg2_f64` carries the host as a
///   NaN-boxed string, `arg3_f64` is the optional `connectListener`.
/// - Options object: `net.connect({ host, port }, cb?)`. `arg1_f64`
///   is a NaN-boxed pointer to a JS object with `host`/`hostname`/
///   `port`; `arg2_f64` is the optional `connectListener`. In this
///   form `arg3_f64` is unused (the dispatch table pads it with
///   `undefined`). Issue #770.
/// - IPC: `net.connect(path, cb?)` or `net.connect({ path }, cb?)`.
///
/// The `connectListener` (whichever slot it ends up in) is
/// auto-registered as a `'connect'` listener on the new socket
/// handle, matching the Node spec.
///
/// # Safety
///
/// All three args must be NaN-boxed Perry-runtime values per the
/// codegen ABI — see `NA_F64` lowering in perry-codegen.
/// Distinct-symbol alias of `js_net_socket_connect` for perry-stdlib's
/// dynamic-dispatch bridge (`js_node_http_native_dispatch`'s net arm). The
/// shared name has a bundled-stdlib twin, and in a build that links BOTH
/// archives the shared symbol can bind to the twin whose socket registry the
/// handle-dispatch never consults — connect then "succeeds" into one registry
/// while `.on('data')` registers in the other and the bytes are silently
/// dropped (mysql2 handshake ETIMEDOUT). Mirrors the
/// `js_ext_net_socket_write`/`_end`/`_destroy` splits (#5010/#5021).
#[no_mangle]
pub unsafe extern "C" fn js_ext_net_socket_connect(
    arg1_f64: f64,
    arg2_f64: f64,
    arg3_f64: f64,
) -> i64 {
    js_net_socket_connect(arg1_f64, arg2_f64, arg3_f64)
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_connect(arg1_f64: f64, arg2_f64: f64, arg3_f64: f64) -> i64 {
    // Path overload: `net.connect(path[, cb])`.
    if let Some(path) = ipc::string_value(arg1_f64) {
        let handle = ipc::spawn_socket(path);
        ipc::register_connect_cb(handle, arg2_f64);
        return handle;
    }

    if is_nanboxed_pointer(arg1_f64) {
        // Options-object overload. A `path` selects local IPC before the TCP
        // host/port fields are considered, matching Node's normalization.
        if let Some(path) = get_object_string_field(arg1_f64, "path") {
            let handle = ipc::spawn_socket(path);
            ipc::register_connect_cb(handle, arg2_f64);
            return handle;
        }
        let host = match get_object_string_field(arg1_f64, "host")
            .or_else(|| get_object_string_field(arg1_f64, "hostname"))
        {
            Some(h) if !h.is_empty() => h,
            _ => "localhost".to_string(),
        };
        let port = match get_object_number_field(arg1_f64, "port") {
            Some(p) => {
                // #2013: validate `options.port` before truncating to u16.
                js_net_validate_connect_port(p);
                p as u16
            }
            None => return 0,
        };
        let handle = spawn_socket_task(host, port, /* direct_tls: */ None);
        // connectListener lives in arg2 for the options form.
        ipc::register_connect_cb(handle, arg2_f64);
        return handle;
    }
    // Positional overload: arg1 is the port number, arg2 is the host
    // string (NaN-boxed), arg3 is the optional connectListener. Reuse
    // the runtime's string-pointer unifier (handles STRING_TAG and
    // POINTER_TAG strings the same way).
    extern "C" {
        fn js_get_string_pointer_unified(value: f64) -> i64;
    }
    let host_ptr = js_get_string_pointer_unified(arg2_f64);
    let (host, listener_f64) = match string_from_header_i64(host_ptr) {
        Some(h) => (h, arg3_f64),
        // #4905: `connect(port)` / `connect(port, connectListener)` —
        // Node defaults the host to localhost when arg2 isn't a string
        // (it may carry the connectListener instead). Pre-fix this
        // returned handle 0, so the socket never connected and no
        // 'connect'/'error' event ever fired.
        None => ("127.0.0.1".to_string(), arg2_f64),
    };
    // #2013: positional `port` must be a valid integer in [0, 65536).
    js_net_validate_connect_port(arg1_f64);
    let port = arg1_f64 as u16;
    let handle = spawn_socket_task(host, port, /* direct_tls: */ None);
    ipc::register_connect_cb(handle, listener_f64);
    handle
}

// ─── FFI: new net.Socket() (alloc-only, deferred connect) ────────────────────

/// `new net.Socket()` — allocates an unconnected socket handle. The TCP
/// connection is deferred until `js_net_socket_method_connect` runs. Issue
/// #422 added this path; pre-#422 only the eager `createConnection` factory
/// existed.
#[no_mangle]
pub unsafe extern "C" fn js_net_socket_alloc() -> i64 {
    ensure_gc_scanner_registered();
    dispatch::ensure_runtime_dispatch_registered();
    let id = next_id_or_throw();
    let (tx, rx) = mpsc::unbounded_channel::<SocketCommand>();
    let tcp_async_id = init_provider(b"TCPWRAP");
    statics::sockets().lock().unwrap().insert(
        id,
        SocketState {
            tcp_async_id,
            connect_async_id: 0,
            shutdown_async_id: 0,
            cmd_tx: tx,
            pending_rx: Some(rx),
            is_open: false,
            refed: true,
            local_addr: None,
            raw: None,
            destroyed: false,
            bytes_read: 0,
            bytes_written: 0,
            timeout: None,
            type_of_service: 0,
            server_id: None,
            server_connection_active: false,
            tls: TlsSocketMetadata::default(),
        },
    );
    statics::listeners()
        .lock()
        .unwrap()
        .insert(id, HashMap::new());
    id
}

// ─── FFI: net.createServer(options?, connectionListener?) ────────────────────

/// `net.createServer(options?, connectionListener?)`.
#[no_mangle]
pub unsafe extern "C" fn js_net_create_server(
    _options_i64: i64,
    connection_listener_i64: i64,
) -> i64 {
    ensure_gc_scanner_registered();
    dispatch::ensure_runtime_dispatch_registered();
    let id = next_id_or_throw();
    statics::listeners()
        .lock()
        .unwrap()
        .insert(id, HashMap::new());
    // Issue #1123 followup — register the server in the dedicated
    // `servers()` map alongside the listener-map entry. The accept
    // loop in `js_net_server_listen` populates `shutdown_tx` + the
    // bound address fields; `js_net_server_close` consumes the
    // shutdown sender to wake the accept loop.
    statics::servers().lock().unwrap().insert(
        id,
        ServerState {
            async_id: 0,
            shutdown_tx: None,
            bound_port: 0,
            bound_host: String::new(),
            bound_path: None,
            listening: false,
            active_connections: 0,
            pending_connections: 0,
            pending_local_connect_events: 0,
            max_connections: None,
            drop_max_connection: None,
        },
    );
    if connection_listener_i64 != 0 {
        if let Ok(mut listeners) = statics::listeners().lock() {
            listeners
                .entry(id)
                .or_default()
                .entry("connection".to_string())
                .or_default()
                .push(connection_listener_i64);
        }
    }
    id
}

/// Collision-proof server factory for generated code. Pulling this distinct
/// symbol from perry-ext-net also ensures the server/socket symbols in this
/// archive win over bundled-stdlib twins with separate handle registries.
#[no_mangle]
pub unsafe extern "C" fn js_ext_net_create_server(
    options_i64: i64,
    connection_listener_i64: i64,
) -> i64 {
    js_net_create_server(options_i64, connection_listener_i64)
}

// ─── FFI: net.Server.listen / .close / .address / .on ────────────────────────

/// `server.listen(port | path, callback?)` — bind TCP, a Windows named pipe,
/// or a Unix-domain socket and spawn an accept loop on the shared runtime.
/// The `callback` (a NaN-boxed closure pointer in the codegen's
/// NA_PTR slot, raw i64 here after unboxing in lower_call.rs) is
/// registered as a one-shot `'listening'` listener; when the bind
/// resolves, the accept-loop task pushes a `ServerListening` event so
/// the main-thread pump invokes both the user's `.on('listening', cb)`
/// listeners and the trailing `.listen(port, cb)` callback.
///
/// Bind failures emit a `ServerError` and a `ServerClose` so the user's
/// `.on('error', err => …)` + `.on('close', () => …)` listeners fire
/// the same way they would on Node.
///
/// # Safety
///
/// `handle` must be a server id returned by `js_net_create_server`.
/// `callback_i64` may be 0 (no callback) or a raw `*const RawClosureHeader`
/// cast to `i64` — the codegen ABI for NA_PTR-unboxed closures.
#[no_mangle]
pub unsafe extern "C" fn js_net_server_listen(handle: i64, port: f64, arg2: f64, arg3: f64) {
    ensure_gc_scanner_registered();
    let callback_i64 = match js_net_callback_ptr(arg3) {
        0 => js_net_callback_ptr(arg2),
        cb => cb,
    };
    let path = ipc::string_value(port)
        .or_else(|| is_nanboxed_pointer(port).then(|| get_object_string_field(port, "path"))?);
    let (port_u16, host) = if path.is_some() {
        (0, String::new())
    } else if is_nanboxed_pointer(port) {
        let option_port = get_object_number_field(port, "port").unwrap_or(0.0);
        js_net_validate_listen_port(option_port);
        let option_host = get_object_string_field(port, "host")
            .filter(|host| !host.is_empty())
            .unwrap_or_else(|| "0.0.0.0".to_string());
        (option_port as u16, option_host)
    } else {
        // #2013: a numeric `port` must be an integer in [0, 65536); Node throws
        // RangeError [ERR_SOCKET_BAD_PORT] otherwise.
        js_net_validate_listen_port(port);
        let host = string_from_header_i64(js_get_string_pointer_unified(arg2))
            .unwrap_or_else(|| "0.0.0.0".to_string());
        (port as u16, host)
    };
    let server_async_id = init_provider(b"TCPSERVERWRAP");

    let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();

    // Mark the server as listening + stash the shutdown sender. If the
    // handle isn't registered, bail before touching tokio.
    {
        let mut servers = match statics::servers().lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        let s = match servers.get_mut(&handle) {
            Some(s) => s,
            None => return,
        };
        s.async_id = server_async_id;
        s.shutdown_tx = Some(shutdown_tx);
        s.bound_port = port_u16;
        s.bound_host = host.clone();
        s.bound_path = path.clone();
        s.listening = true;
    }

    // Stash the listen-callback under `'listening'` so the pump fires
    // it on the first ServerListening event, then drops it (matching
    // Node's "callback runs once on listen" semantics).
    if callback_i64 != 0 {
        if let Ok(mut listeners) = statics::listeners().lock() {
            listeners
                .entry(handle)
                .or_default()
                .entry("listening".to_string())
                .or_default()
                .push(callback_i64);
        }
    }

    let host_for_spawn = host.clone();
    let server_id = handle;

    if let Some(path) = path {
        ipc::spawn_listener(server_id, path, shutdown_rx);
        return;
    }

    // Run the accept loop cooperatively on Perry's shared multi-thread runtime
    // via `spawn_async` — no throwaway current-thread runtime, no blocking-pool
    // thread held for the server's life. The shared runtime owns the I/O
    // reactor, so `TcpListener::bind` / `accept` work without an ambient
    // `Handle`. The server is marked `listening` synchronously above, so
    // `js_ext_net_has_active_handles` keeps the loop alive until `close()`.
    perry_ffi::spawn_async(async move {
        let bind_str = format!("{}:{}", host_for_spawn, port_u16);
        let listener = match TcpListener::bind(&bind_str).await {
            Ok(l) => l,
            Err(e) => {
                push_event(PendingNetEvent::ServerError(
                    server_id,
                    format!("bind {}: {}", bind_str, e),
                ));
                push_event(PendingNetEvent::ServerClose(server_id));
                if let Ok(mut servers) = statics::servers().lock() {
                    if let Some(s) = servers.get_mut(&server_id) {
                        s.listening = false;
                    }
                }
                return;
            }
        };
        // Issue #1852 — record the *actual* bound address. The
        // dominant Node test pattern is `server.listen(0, () =>
        // client.connect(server.address().port))`: port 0 asks the OS
        // for an ephemeral port, so the requested `port_u16` (0) is
        // never what we end up listening on. Read `local_addr()` and
        // overwrite the stashed port/host BEFORE firing `'listening'`,
        // so `server.address()` inside the listen callback reports the
        // real port (pre-fix it returned 0 and every client connected
        // to port 0 → connection refused → hang).
        if let Ok(local) = listener.local_addr() {
            if let Ok(mut servers) = statics::servers().lock() {
                if let Some(s) = servers.get_mut(&server_id) {
                    s.bound_port = local.port();
                    s.bound_host = local.ip().to_string();
                }
            }
            let address = local.ip().to_string();
            perry_cluster_worker_listening(
                address.as_ptr(),
                address.len() as u32,
                local.port() as i32,
                if local.is_ipv6() { 6 } else { 4 },
            );
        }
        // bind succeeded — fire `'listening'`.
        push_event(PendingNetEvent::ServerListening(server_id));

        loop {
            tokio::select! {
                accepted = listener.accept() => {
                    match accepted {
                        Ok((stream, _peer)) => {
                            if let Some(info) =
                                server_state::should_drop_connection(server_id, &stream)
                            {
                                push_event(PendingNetEvent::ServerDrop(server_id, info));
                                continue;
                            }
                            // Node sets TCP_NODELAY on every accepted socket by
                            // default (Nagle off). Match that so small writes
                            // aren't delayed waiting to coalesce; a later
                            // `socket.setNoDelay(false)` can re-enable Nagle.
                            let _ = stream.set_nodelay(true);
                            // Issue #2131 — record the accepted
                            // stream's local address so `sock.address()`
                            // on the server-side socket reports the
                            // bound port/family instead of returning
                            // undefined.
                            let accepted_local = stream.local_addr().ok();
                            ipc::register_accepted_transport(
                                server_id,
                                Transport::Plain(stream),
                                accepted_local,
                            );
                        }
                        Err(e) => {
                            push_event(PendingNetEvent::ServerError(
                                server_id,
                                format!("accept: {}", e),
                            ));
                            // Don't break the loop on a transient
                            // accept error — Node doesn't.
                        }
                    }
                }
                _ = &mut shutdown_rx => {
                    break;
                }
            }
        }
        // Loop exited (close() called or fatal error) — emit
        // a final 'close' event so user code can see the
        // server stopped.
        push_event(PendingNetEvent::ServerClose(server_id));
        if let Ok(mut servers) = statics::servers().lock() {
            if let Some(s) = servers.get_mut(&server_id) {
                s.listening = false;
            }
        }
    });
}

/// `server.close(callback?)` — break the accept loop and fire the
/// optional callback once it exits. The actual `'close'` listener
/// dispatch happens in the main-thread pump when the accept-loop
/// task pushes its terminal `ServerClose` event.
///
/// # Safety
///
/// `handle` must be a server id; `callback_i64` is a raw closure ptr.
#[no_mangle]
pub unsafe extern "C" fn js_net_server_close(handle: i64, callback_i64: i64) {
    // Stash the user's close callback under `'close'` so the pump fires
    // it alongside the registered listeners when the accept loop exits.
    if callback_i64 != 0 {
        if let Ok(mut listeners) = statics::listeners().lock() {
            listeners
                .entry(handle)
                .or_default()
                .entry("close".to_string())
                .or_default()
                .push(callback_i64);
        }
    }
    // Drop the shutdown sender — the accept loop's `tokio::select!`
    // wakes immediately on the receiver side and exits its loop.
    if let Ok(mut servers) = statics::servers().lock() {
        if let Some(s) = servers.get_mut(&handle) {
            s.shutdown_tx.take();
        }
    }
}

/// `server.address()` — returns a JSON string the TS-side wrapper can
/// `JSON.parse` into `{ port, address, family }`. Matches the
/// perry-ext-http contract (`js_node_http_server_address_json`).
///
/// Returns `null` (as a JS string) for an unlistening server.
///
/// # Safety
///
/// `handle` must be a server id. The returned `*mut StringHeader` is
/// allocated in the runtime arena and follows perry-ffi's standard
/// ownership: the caller hands it to user code as a NaN-boxed JS string.
#[no_mangle]
pub unsafe extern "C" fn js_net_server_address(handle: i64) -> *mut StringHeader {
    let json = match statics::servers().lock() {
        Ok(g) => match g.get(&handle) {
            Some(s) if s.listening => {
                if let Some(path) = &s.bound_path {
                    return alloc_string(
                        &serde_json::to_string(path).unwrap_or_else(|_| "null".to_string()),
                    )
                    .as_raw();
                }
                let family = if s.bound_host.contains(':') {
                    "IPv6"
                } else {
                    "IPv4"
                };
                format!(
                    "{{\"port\":{},\"address\":\"{}\",\"family\":\"{}\"}}",
                    s.bound_port, s.bound_host, family
                )
            }
            _ => "null".to_string(),
        },
        Err(_) => "null".to_string(),
    };
    alloc_string(&json).as_raw()
}

// ─── FFI: socket.connect(port, host) (instance method on existing handle) ─────

/// `socket.connect(port, host)` / `socket.connect(path)` — initiates a TCP or
/// IPC connection on a socket previously allocated by `new net.Socket()`. Pulls its receiver out of
/// the `SocketState::pending_rx` slot rather than allocating a fresh
/// channel, so any listener already registered (`sock.on('data', cb)`)
/// sees the same handle id once the connect completes.
///
/// # Safety
///
/// See `js_net_socket_connect`.
#[no_mangle]
pub unsafe extern "C" fn js_ext_net_socket_method_connect(
    handle: i64,
    arg1: f64,
    arg2: f64,
    arg3: f64,
) {
    js_net_socket_method_connect(handle, arg1, arg2, arg3);
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_method_connect(
    handle: i64,
    arg1: f64,
    arg2: f64,
    arg3: f64,
) {
    if let Some(path) = ipc::string_value(arg1) {
        ipc::register_connect_cb(handle, arg2);
        ipc::connect_existing(handle, path);
        return;
    }

    let (host, port, callback) = if is_nanboxed_pointer(arg1) {
        if let Some(path) = get_object_string_field(arg1, "path") {
            ipc::register_connect_cb(handle, arg2);
            ipc::connect_existing(handle, path);
            return;
        }
        let port = match get_object_number_field(arg1, "port") {
            Some(port) => port,
            None => {
                push_event(PendingNetEvent::Error(
                    handle,
                    "socket.connect: options.port or options.path is required".to_string(),
                ));
                return;
            }
        };
        let host = get_object_string_field(arg1, "host")
            .or_else(|| get_object_string_field(arg1, "hostname"))
            .filter(|host| !host.is_empty())
            .unwrap_or_else(|| "localhost".to_string());
        (host, port, arg2)
    } else {
        let host = ipc::string_value(arg2);
        let callback = if host.is_some() { arg3 } else { arg2 };
        (
            host.unwrap_or_else(|| "127.0.0.1".to_string()),
            arg1,
            callback,
        )
    };
    // #2013: validate before truncating, matching Node's synchronous
    // ERR_SOCKET_BAD_PORT behavior for positional and options overloads.
    js_net_validate_connect_port(port);
    let port = port as u16;
    ipc::register_connect_cb(handle, callback);

    let (rx, tcp_async_id) = {
        let mut guard = statics::sockets().lock().unwrap();
        match guard.get_mut(&handle) {
            Some(socket) => match socket.pending_rx.take() {
                Some(rx) => (rx, socket.tcp_async_id),
                None => {
                    push_event(PendingNetEvent::Error(
                        handle,
                        "socket already connected (or unknown handle)".to_string(),
                    ));
                    return;
                }
            },
            None => {
                push_event(PendingNetEvent::Error(
                    handle,
                    "socket already connected (or unknown handle)".to_string(),
                ));
                return;
            }
        }
    };
    let connect_async_id = init_provider_with_trigger(b"TCPCONNECTWRAP", tcp_async_id);
    if let Some(socket) = statics::sockets().lock().unwrap().get_mut(&handle) {
        socket.connect_async_id = connect_async_id;
    }

    let local_server = server_state::begin_local_connect(&host, port);
    spawn_socket_runner(move || {
        Box::pin(async move {
            let mut rx = rx;
            let addr = format!("{}:{}", host, port);
            let tcp = match TcpStream::connect(&addr).await {
                Ok(s) => s,
                Err(e) => {
                    server_state::cancel_local_connect(local_server);
                    push_event(PendingNetEvent::Error(handle, format!("{}", e)));
                    push_event(PendingNetEvent::Close(handle));
                    mark_closed(handle);
                    return;
                }
            };

            // Node default: TCP_NODELAY on for a freshly-connected socket.
            let _ = tcp.set_nodelay(true);
            // Issue #2131 — record the local addr so `socket.address()`
            // returns the bound port/family on the deferred-connect path.
            let local = tcp.local_addr().ok();
            if let Some(s) = statics::sockets().lock().unwrap().get_mut(&handle) {
                s.is_open = true;
                s.local_addr = local;
            }
            tokio::task::yield_now().await;
            push_event(PendingNetEvent::Connect(handle, local_server));

            run_socket_task(handle, Transport::Plain(tcp), &mut rx).await;
        })
    });
}

// ─── FFI: tls.connect ────────────────────────────────────────────────────────
// `js_tls_connect` lives in tls.rs (this file is at the 2000-line gate);
// it resolves Node's connect overloads and reuses `spawn_socket_task`.

/// Internal: allocate the handle, spawn the tokio task.
/// `direct_tls = Some((servername, verify))` runs a TLS handshake before
/// firing 'connect'; None keeps the socket in plain TCP mode.
pub(crate) fn spawn_socket_task(
    host: String,
    port: u16,
    direct_tls: Option<(String, bool, TlsClientConfigData)>,
) -> i64 {
    spawn_socket_task_initialized(host, port, direct_tls, |_| {})
}

/// Allocate a socket and run `initialize` after its registries exist but before
/// the async connect task can complete. TLS uses this boundary to publish its
/// runtime metadata without racing a fast loopback handshake.
pub(crate) fn spawn_socket_task_initialized<F>(
    host: String,
    port: u16,
    direct_tls: Option<(String, bool, TlsClientConfigData)>,
    initialize: F,
) -> i64
where
    F: FnOnce(i64),
{
    ensure_gc_scanner_registered();
    dispatch::ensure_runtime_dispatch_registered();
    let id = next_id_or_throw();
    let (tx, rx) = mpsc::unbounded_channel::<SocketCommand>();
    let local_server = direct_tls
        .is_none()
        .then(|| server_state::begin_local_connect(&host, port))
        .flatten();
    let tcp_async_id = unsafe { init_provider(b"TCPWRAP") };
    let connect_async_id = unsafe { init_provider_with_trigger(b"TCPCONNECTWRAP", tcp_async_id) };

    statics::sockets().lock().unwrap().insert(
        id,
        SocketState {
            tcp_async_id,
            connect_async_id,
            shutdown_async_id: 0,
            cmd_tx: tx,
            pending_rx: None,
            is_open: false,
            refed: true,
            local_addr: None,
            raw: None,
            destroyed: false,
            bytes_read: 0,
            bytes_written: 0,
            timeout: None,
            type_of_service: 0,
            server_id: None,
            server_connection_active: false,
            tls: TlsSocketMetadata::default(),
        },
    );
    statics::listeners()
        .lock()
        .unwrap()
        .insert(id, HashMap::new());
    initialize(id);

    spawn_socket_runner(move || {
        Box::pin(async move {
            let mut rx = rx;
            let addr = format!("{}:{}", host, port);
            let tcp = match TcpStream::connect(&addr).await {
                Ok(s) => s,
                Err(e) => {
                    server_state::cancel_local_connect(local_server);
                    push_event(PendingNetEvent::Error(id, format!("{}", e)));
                    push_event(PendingNetEvent::Close(id));
                    mark_closed(id);
                    return;
                }
            };

            // Node default: TCP_NODELAY on. Set it on the raw TCP socket
            // before any TLS handshake consumes the stream — the option lives
            // on the kernel socket and persists through the rustls wrapper.
            let _ = tcp.set_nodelay(true);
            // Issue #2131 — capture the local addr before we possibly
            // hand the stream to rustls (the TLS path consumes it).
            let local = tcp.local_addr().ok();

            let transport = match direct_tls {
                Some((servername, verify, config)) => {
                    match do_tls_handshake(tcp, &servername, verify, Some(&config)).await {
                        Ok(tls) => {
                            record_tls_handshake(id, &tls, &servername, verify, Some(&config));
                            Transport::Tls(Box::new(tls))
                        }
                        Err(e) => {
                            server_state::cancel_local_connect(local_server);
                            push_event(PendingNetEvent::Error(id, e));
                            push_event(PendingNetEvent::Close(id));
                            mark_closed(id);
                            return;
                        }
                    }
                }
                None => Transport::Plain(tcp),
            };

            if let Some(s) = statics::sockets().lock().unwrap().get_mut(&id) {
                s.is_open = true;
                s.local_addr = local;
            }
            tokio::task::yield_now().await;
            push_event(PendingNetEvent::Connect(id, local_server));

            run_socket_task(id, transport, &mut rx).await;
        })
    });

    id
}

/// The read/write/command loop. Shared by plain-TCP and direct-TLS paths.
pub(crate) async fn run_socket_task(
    id: i64,
    initial_transport: Transport,
    rx: &mut mpsc::UnboundedReceiver<SocketCommand>,
) {
    let mut transport: Option<Transport> = Some(initial_transport);
    let mut writable_ended = false;
    let accepted_socket = statics::sockets()
        .lock()
        .ok()
        .and_then(|sockets| sockets.get(&id).map(|socket| socket.server_id.is_some()))
        .unwrap_or(false);
    let mut server_connection_ready = !accepted_socket;

    loop {
        let t = match transport.as_mut() {
            Some(t) => t,
            None => break,
        };

        // Check a read buffer out of the process-wide freelist instead of
        // allocating one per socket / reallocating one per read. `checkout`
        // hands back an empty `BytesMut` with a ≥ 16 KiB writable window
        // (recycling a pooled allocation in place once its prior chunk has
        // drained; allocating only when none is reusable) — identical to a
        // per-socket `BytesMut::with_capacity(16 KiB)` + per-read `clear()` /
        // `reserve()`, just amortized across reads and sockets. `read_buf`
        // fills the uninitialized tail in place (no
        // per-read zeroing) and `split_to(n)` carves the freshly-read bytes
        // off as a refcounted `Bytes` view for the 'data' event. The per-read
        // 16 KiB ceiling is still enforced by the `BufMut::limit` wrapper at
        // the read site below, so read sizing and 'data' chunk boundaries are
        // unchanged.
        let mut buf = buffer_pool::checkout();

        // Wrap the buffer in `BufMut::limit(16 KiB)` so a single `read_buf`
        // reads the same per-call ceiling the old fixed `[u8; 16 KiB]` scratch
        // did: `checkout` guarantees *at least* 16 KiB of spare capacity, but
        // `BytesMut` may over-allocate and `read_buf` would otherwise fill all
        // of it. The adapter only borrows `buf` for the read future;
        // `read_buf` advances `buf` itself, so `buf.len() == n` afterwards and
        // `split_to(n)` carves off exactly the freshly-read run.
        let mut window = (&mut buf).limit(buffer_pool::READ_BUF_CAP);
        tokio::select! {
            read_result = t.read_buf(&mut window) => {
                // Release the `Limit` borrow of `buf` before touching `buf`
                // again; `read_buf` already advanced `buf` in place.
                drop(window);
                match read_result {
                    Ok(0) => {
                        // Return the untouched buffer to the freelist before
                        // breaking, so a peer FIN doesn't leak its pooled
                        // capacity (the success path checks in below).
                        buffer_pool::checkin(buf);
                        // #2154 raw mode owns its own terminal state. Accepted
                        // sockets may receive a request plus FIN before their
                        // delayed `connection` callback runs. Wait for the
                        // callback-complete marker so its queued writes/end
                        // commands are honored. Outgoing sockets start ready
                        // and therefore retain Node's default auto-close after
                        // readable EOF (#6764).
                        if raw_bridge::mark_terminal(id, None) {
                            // Complete the writable half before dropping a TLS
                            // transport so the peer observes close_notify rather
                            // than an unclean EOF (#8688).
                            let _ = t.shutdown().await;
                            mark_closed(id);
                            break;
                        }
                        push_event(PendingNetEvent::End(id));

                        while accepted_socket && !writable_ended {
                            let command = if server_connection_ready {
                                match tokio::time::timeout(
                                    std::time::Duration::from_millis(25),
                                    rx.recv(),
                                )
                                .await
                                {
                                    Ok(command) => command,
                                    Err(_) => break,
                                }
                            } else {
                                rx.recv().await
                            };
                            match command {
                                Some(SocketCommand::Write(bytes)) => {
                                    if let Err(e) = t.write_all(&bytes).await {
                                        push_event(PendingNetEvent::Error(id, format!("{}", e)));
                                        break;
                                    }
                                }
                                Some(SocketCommand::End) => {
                                    let _ = t.shutdown().await;
                                    writable_ended = true;
                                    push_event(PendingNetEvent::ShutdownComplete(id));
                                }
                                Some(SocketCommand::SetNoDelay(enable)) => {
                                    let _ = t.set_nodelay(enable);
                                }
                                Some(SocketCommand::ServerConnectionReady) => {
                                    server_connection_ready = true;
                                }
                                #[cfg(test)]
                                Some(SocketCommand::QueryNoDelay(reply)) => {
                                    let _ = reply.send(t.nodelay().unwrap_or(false));
                                }
                                Some(SocketCommand::UpgradeTls { reply, .. }) => {
                                    let _ = reply.send(Err(
                                        "cannot upgrade a half-closed socket".to_string(),
                                    ));
                                }
                                Some(SocketCommand::Destroy) | None => break,
                            }
                        }
                        if !writable_ended {
                            let _ = t.shutdown().await;
                            push_event(PendingNetEvent::ShutdownComplete(id));
                        }
                        push_event(PendingNetEvent::Close(id));
                        mark_closed(id);
                        break;
                    }
                    Ok(n) => {
                        // #2154 raw mode buffers for `poll_read`; else 'data'.
                        // `split_to(n)` hands out a zero-copy `Bytes` view of
                        // the bytes just read and leaves `buf` empty (still
                        // backed by the pooled allocation, now shared with the
                        // chunk) to return to the freelist below.
                        let chunk = buf.split_to(n).freeze();
                        if !raw_bridge::route_data(id, &chunk) {
                            push_event(PendingNetEvent::Data(id, chunk));
                        }
                        // Return the buffer to the freelist. Its next checkout
                        // reclaims this allocation in place once `chunk` has
                        // drained + dropped (reallocates otherwise — never
                        // corrupting the in-flight chunk).
                        buffer_pool::checkin(buf);
                    }
                    Err(e) => {
                        // Return the buffer before breaking on a read error,
                        // mirroring the EOF and success paths — a failed read
                        // wrote nothing, so its pooled capacity is reusable.
                        buffer_pool::checkin(buf);
                        let msg = format!("{}", e);
                        if !raw_bridge::mark_terminal(id, Some(msg.clone())) {
                            // rustls reports a peer that closes TCP without a
                            // close_notify alert as UnexpectedEof. Node's TLS
                            // socket treats that terminal read as the readable
                            // side ending, so preserve the normal end→close
                            // event order instead of silently losing `end`.
                            if msg.contains("close_notify") || msg.contains("unexpected end of file") {
                                push_event(PendingNetEvent::End(id));
                            } else {
                                push_event(PendingNetEvent::Error(id, msg));
                            }
                            push_event(PendingNetEvent::Close(id));
                        }
                        mark_closed(id);
                        break;
                    }
                }
            }
            cmd = rx.recv() => {
                // The command arm never wrote to `buf`; return the untouched
                // buffer to the freelist for the next read instead of dropping
                // its capacity.
                drop(window);
                buffer_pool::checkin(buf);
                match cmd {
                    Some(SocketCommand::Write(bytes)) => {
                        if let Err(e) = t.write_all(&bytes).await {
                            let msg = format!("{}", e);
                            if !raw_bridge::mark_terminal(id, Some(msg.clone())) {
                                push_event(PendingNetEvent::Error(id, msg));
                                push_event(PendingNetEvent::Close(id));
                            }
                            mark_closed(id);
                            break;
                        }
                    }
                    Some(SocketCommand::End) => {
                        let _ = t.shutdown().await;
                        writable_ended = true;
                        push_event(PendingNetEvent::ShutdownComplete(id));
                    }
                    Some(SocketCommand::SetNoDelay(enable)) => {
                        // Best-effort, matching Node: a failed setsockopt (e.g.
                        // the peer already closed) does not error the socket.
                        let _ = t.set_nodelay(enable);
                    }
                    Some(SocketCommand::ServerConnectionReady) => {
                        server_connection_ready = true;
                    }
                    #[cfg(test)]
                    Some(SocketCommand::QueryNoDelay(reply)) => {
                        let _ = reply.send(t.nodelay().unwrap_or(false));
                    }
                    Some(SocketCommand::Destroy) | None => {
                        if !raw_bridge::mark_terminal(id, None) {
                            push_event(PendingNetEvent::Close(id));
                        }
                        mark_closed(id);
                        break;
                    }
                    Some(SocketCommand::UpgradeTls { servername, verify, config, reply }) => {
                        let old = transport.take();
                        match old {
                            Some(Transport::Plain(tcp)) => {
                                match do_tls_handshake(tcp, &servername, verify, Some(&config)).await {
                                    Ok(tls) => {
                                        record_tls_handshake(
                                            id,
                                            &tls,
                                            &servername,
                                            verify,
                                            Some(&config),
                                        );
                                        transport = Some(Transport::Tls(Box::new(tls)));
                                        let _ = reply.send(Ok(()));
                                        push_event(PendingNetEvent::SecureConnect(id));
                                    }
                                    Err(e) => {
                                        let _ = reply.send(Err(e.clone()));
                                        push_event(PendingNetEvent::Error(id, e));
                                        push_event(PendingNetEvent::Close(id));
                                        mark_closed(id);
                                        break;
                                    }
                                }
                            }
                            Some(already_tls @ Transport::Tls(_)) => {
                                transport = Some(already_tls);
                                let _ = reply.send(Err("socket is already TLS".to_string()));
                            }
                            Some(ipc @ Transport::Ipc(_)) => {
                                transport = Some(ipc);
                                let _ = reply.send(Err(
                                    "TLS upgrade is unsupported for IPC sockets".to_string(),
                                ));
                            }
                            None => {
                                let _ = reply.send(Err("socket closed".to_string()));
                                break;
                            }
                        }
                    }
                }
            }
        }
    }
}

// ─── FFI: socket.write / end / destroy live in `lifecycle.rs` ────────────────
// (#2549 split — moved there alongside the new state/counter getters to keep
// this file under the 2000-line gate; they mutate the same SocketState.)

// ─── FFI: socket.on(event, callback) ─────────────────────────────────────────

/// `socket.on(event, cb)` — registers a listener. Closures are stored as
/// raw `i64` pointers; the GC root scanner keeps them alive across cycles.
///
/// # Safety
///
/// `event_ptr` must be null or a Perry-runtime `StringHeader`. `cb` is a
/// raw `*const ClosureHeader` cast to `i64` (codegen ABI for NA_PTR).
#[no_mangle]
pub unsafe extern "C" fn js_net_socket_on(handle: i64, event_ptr: i64, cb: i64) {
    ensure_gc_scanner_registered();
    let event = match string_from_header_i64(event_ptr) {
        Some(e) => e,
        None => return,
    };
    {
        let mut listeners = statics::listeners().lock().unwrap();
        let entry = listeners.entry(handle).or_default();
        entry.entry(event.clone()).or_default().push(cb);
    }
    if event == "close" {
        tls::fire_pending_tls_abort(handle);
    }
}

// ─── FFI: socket.upgradeToTLS(servername, verify) -> Promise ─────────────────

/// `socket.upgradeToTLS(servername, verify)` — sends an UpgradeTls command
/// to the socket task and returns a Promise that resolves when the
/// handshake completes (or rejects on failure).
///
/// # Safety
///
/// `servername_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_net_socket_upgrade_tls(
    handle: i64,
    servername_ptr: i64,
    verify: f64,
) -> *mut perry_ffi::Promise {
    let promise = JsPromise::new();
    let promise_raw = promise.as_raw();

    let servername = match string_from_header_i64(servername_ptr) {
        Some(s) => s,
        None => {
            // Reject on the same thread we're called from — works because
            // the resolution is queued and processed on the main thread by
            // the runtime's promise dispatcher.
            promise.reject_string("invalid servername");
            return promise_raw;
        }
    };

    let cmd_tx = {
        let sockets = statics::sockets().lock().unwrap();
        match sockets.get(&handle) {
            Some(s) => s.cmd_tx.clone(),
            None => {
                promise.reject_string(&format!("socket {} not found", handle));
                return promise_raw;
            }
        }
    };

    let (reply_tx, reply_rx) = oneshot::channel::<Result<(), String>>();
    let verify_bool = verify != 0.0;
    if cmd_tx
        .send(SocketCommand::UpgradeTls {
            servername,
            verify: verify_bool,
            config: TlsClientConfigData::default(),
            reply: reply_tx,
        })
        .is_err()
    {
        promise.reject_string("socket task is gone");
        return promise_raw;
    }

    // Hand the JsPromise to a blocking thread that awaits the oneshot reply.
    perry_ffi::spawn_blocking(move || {
        let handle_rt = tokio::runtime::Handle::current();
        handle_rt.block_on(async move {
            match reply_rx.await {
                Ok(Ok(())) => promise.resolve_undefined(),
                Ok(Err(msg)) => promise.reject_string(&msg),
                Err(_) => promise.reject_string("upgrade reply dropped"),
            }
        });
    });

    promise_raw
}

// ─── Main-thread event pump ──────────────────────────────────────────────────

/// Dispatches queued socket events to JS listeners on the main thread.
/// Called from codegen's event-loop tick (via the well-known pending-events
/// pump).
///
/// Per the arena-safety rule: JSValue construction (Buffer, error string)
/// happens HERE on the main thread, never in the tokio read task.
///
/// Returns the number of events fired in this pass.
///
/// #1114 followup (mysql wedge): this pump runs on EVERY iteration of
/// the generated event loop AND every iteration of every inline `await`
/// poll loop. `@perryts/mysql` (pure-TS driver) drives all its bytes
/// through `net.Socket`, so under a `setInterval` + async-query JobLoop
/// this function is the dominant per-tick path. The original
/// `Vec::drain(..).collect()` allocated a fresh Vec every call
/// (mirroring the fastify wedge that e538caa7 fixed) → GC `madvise`
/// page-churn. Reuse a per-thread scratch buffer (moved out across
/// dispatch so a re-entrant pump from inside a user callback is safe;
/// capacity retained → zero steady-state allocation).
#[no_mangle]
pub unsafe extern "C" fn js_net_process_pending() -> i32 {
    js_ext_net_drain_pending()
}

fn socket_receiver(handle: i64) -> f64 {
    f64::from_bits(0x7FFD_0000_0000_0000 | (handle as u64 & 0x0000_FFFF_FFFF_FFFF))
}

unsafe fn emit_socket_no_arg(handle: i64, event: &str) {
    extern "C" {
        fn js_implicit_this_set(value: f64) -> f64;
    }
    let frame = dispatch_custody::DispatchFrame::park(listeners_for(handle, event));
    let previous_this = js_implicit_this_set(socket_receiver(handle));
    for index in 0..frame.len() {
        let callback = frame.cb(index);
        if callback != 0 {
            let _ = JsClosure::from_raw(callback as *const RawClosureHeader).call0();
        }
    }
    js_implicit_this_set(previous_this);
    drop(frame);
    lifecycle::drain_once_listeners(handle, event);
}

unsafe fn emit_tls_secure_connect(handle: i64) {
    extern "C" {
        fn js_tls_client_check_identity_from_metadata(handle: i64) -> f64;
    }
    let identity_error = js_tls_client_check_identity_from_metadata(handle);
    if !JsValue::from_bits(identity_error.to_bits()).is_undefined() {
        let mut frame = dispatch_custody::DispatchFrame::park(listeners_for(handle, "error"));
        frame.set_payload(identity_error.to_bits());
        for index in 0..frame.len() {
            let callback = frame.cb(index);
            if callback != 0 {
                let _ = JsClosure::from_raw(callback as *const RawClosureHeader)
                    .call1(f64::from_bits(frame.payload_bits()));
            }
        }
        drop(frame);
        lifecycle::drain_once_listeners(handle, "error");
        if let Some(socket) = statics::sockets().lock().unwrap().get(&handle) {
            let _ = socket.cmd_tx.send(SocketCommand::Destroy);
        }
        return;
    }
    emit_socket_no_arg(handle, "secureConnect");
}

/// Drain ext-net's own pending-event queue.
///
/// This carries a DISTINCT `#[no_mangle]` symbol (`js_ext_net_drain_pending`),
/// deliberately NOT the `js_net_process_pending` name that the bundled stdlib
/// net ALSO exports. In a workspace/auto-optimize build both crates are
/// linked, so `js_net_process_pending` is a duplicate symbol; the link binds
/// every reference to whichever twin wins (stdlib's). The aux pump
/// (`process_pending_aux`) and the extern wrapper above therefore call THIS
/// uniquely-named entry point instead — a symbol with no twin and nothing to
/// fold against — so the adopted raw-`'upgrade'` socket's `Close` event in
/// ext-net's own queue is actually drained rather than left to pin the event
/// loop forever. Without this the loop hung, and the behavior flipped with
/// unrelated code-size changes (link-order roulette). (#5010)
///
/// # Safety
/// Fires user JS closures (listeners); callers must hold a valid runtime.
#[no_mangle]
pub unsafe extern "C" fn js_ext_net_drain_pending() -> i32 {
    thread_local! {
        static SCRATCH: std::cell::RefCell<Vec<PendingNetEvent>> =
            const { std::cell::RefCell::new(Vec::new()) };
    }
    let mut events = SCRATCH.with(|s| std::mem::take(&mut *s.borrow_mut()));
    events.clear();
    {
        let mut g = statics::pending_events().lock().unwrap();
        events.append(&mut *g);
    }
    let count = events.len() as i32;

    for ev in events.drain(..) {
        prepare_event_provider(&ev);
        let provider_id = event_provider_id(&ev);
        let destroy_ids: Vec<u64> = match &ev {
            PendingNetEvent::Connect(id, _) => statics::sockets()
                .lock()
                .ok()
                .and_then(|sockets| sockets.get(id).map(|socket| vec![socket.connect_async_id]))
                .unwrap_or_default(),
            PendingNetEvent::ShutdownComplete(id) => statics::sockets()
                .lock()
                .ok()
                .and_then(|sockets| sockets.get(id).map(|socket| vec![socket.shutdown_async_id]))
                .unwrap_or_default(),
            PendingNetEvent::Close(id) => statics::sockets()
                .lock()
                .ok()
                .and_then(|sockets| {
                    sockets.get(id).map(|socket| {
                        vec![
                            socket.connect_async_id,
                            socket.shutdown_async_id,
                            socket.tcp_async_id,
                        ]
                    })
                })
                .unwrap_or_default(),
            PendingNetEvent::ServerClose(id) => statics::servers()
                .lock()
                .ok()
                .and_then(|servers| servers.get(id).map(|server| vec![server.async_id]))
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        let provider_scope = ProviderScope::enter(provider_id);
        match ev {
            PendingNetEvent::Connect(id, local_server) => {
                server_state::finish_local_connect(local_server);
                // #8259: park the snapshot so callback N stays rooted (and is
                // rewritten on evacuation) while callback N-1 runs user JS.
                emit_socket_no_arg(id, "connect");
                // TLS sockets additionally fire 'secureConnect' once the
                // handshake completes — the direct-TLS connect path only
                // signals Connect after the handshake, so this is the right
                // tick. Plain sockets simply have no listeners here. #4971.
                extern "C" {
                    fn js_tls_client_is_connected(handle: i64) -> i32;
                }
                if js_tls_client_is_connected(id) != 0 {
                    emit_tls_secure_connect(id);
                }
            }
            PendingNetEvent::SecureConnect(id) => emit_tls_secure_connect(id),
            PendingNetEvent::Data(id, bytes) => {
                let cbs = listeners_for(id, "data");
                if cbs.is_empty() {
                    server_state::buffer_pending_server_data(id, bytes);
                    continue;
                }
                // #8259: park BEFORE the payload allocation below — it can
                // collect, and the evacuating arms then move the closures a
                // bare snapshot would still point at. The payload is parked
                // too: callback 1's JS can move it before callback 2 runs.
                let mut frame = dispatch_custody::DispatchFrame::park(cbs);
                // #4973: `socket.setEncoding(enc)` switches 'data' delivery
                // from Buffers to decoded strings (Node readable-stream
                // semantics). 'hex'/'base64' render their text forms; the
                // remaining text encodings decode as UTF-8 (lossy).
                let encoding = statics::encodings().lock().unwrap().get(&id).cloned();
                let payload_f64 = if let Some(enc) = encoding {
                    let s = match enc.as_str() {
                        "hex" => bytes.iter().map(|b| format!("{b:02x}")).collect::<String>(),
                        "base64" => adopt::base64_encode(&bytes),
                        _ => String::from_utf8_lossy(&bytes).into_owned(),
                    };
                    let hdr = alloc_string(&s);
                    f64::from_bits(
                        0x7FFF_0000_0000_0000 | (hdr.as_raw() as u64 & 0x0000_FFFF_FFFF_FFFF),
                    )
                } else {
                    let buf = alloc_buffer(&bytes);
                    if buf.is_null() {
                        continue;
                    }
                    // POINTER_TAG over the buffer pointer.
                    f64::from_bits(0x7FFD_0000_0000_0000 | (buf as u64 & 0x0000_FFFF_FFFF_FFFF))
                };
                frame.set_payload(payload_f64.to_bits());
                for i in 0..frame.len() {
                    let cb = frame.cb(i);
                    if cb != 0 {
                        let _ = JsClosure::from_raw(cb as *const RawClosureHeader)
                            .call1(f64::from_bits(frame.payload_bits()));
                    }
                }
                drop(frame);
                lifecycle::drain_once_listeners(id, "data");
            }
            PendingNetEvent::Error(id, msg) => {
                let cbs = listeners_for(id, "error");
                if cbs.is_empty() {
                    continue;
                }
                // #8259: park before the allocating build_error_object.
                let mut frame = dispatch_custody::DispatchFrame::park(cbs);
                // Issue #770 — emit an Error-shaped object `{message: msg}`
                // so user code can read `err.message`. Pre-fix this was a
                // raw NaN-boxed string and `err.message` was `undefined`.
                let err_f64 = build_error_object(&msg);
                frame.set_payload(err_f64.to_bits());
                for i in 0..frame.len() {
                    let cb = frame.cb(i);
                    if cb != 0 {
                        let _ = JsClosure::from_raw(cb as *const RawClosureHeader)
                            .call1(f64::from_bits(frame.payload_bits()));
                    }
                }
                drop(frame);
                lifecycle::drain_once_listeners(id, "error");
            }
            PendingNetEvent::AbortError(id) => {
                let cbs = listeners_for(id, "error");
                if cbs.is_empty() {
                    continue;
                }
                let mut frame = dispatch_custody::DispatchFrame::park(cbs);
                extern "C" {
                    fn js_abort_error_value() -> f64;
                }
                frame.set_payload(js_abort_error_value().to_bits());
                for i in 0..frame.len() {
                    let cb = frame.cb(i);
                    if cb != 0 {
                        let _ = JsClosure::from_raw(cb as *const RawClosureHeader)
                            .call1(f64::from_bits(frame.payload_bits()));
                    }
                }
                drop(frame);
                lifecycle::drain_once_listeners(id, "error");
            }
            PendingNetEvent::End(id) => {
                // Issue #1852 — readable side ended (peer FIN). Fire the
                // `'end'` listeners; the trailing `Close` event (pushed
                // right after `End` in `run_socket_task`) does the actual
                // listener-map / socket-map teardown, so don't remove
                // anything here.
                let frame = dispatch_custody::DispatchFrame::park(listeners_for(id, "end"));
                for i in 0..frame.len() {
                    let cb = frame.cb(i);
                    if cb != 0 {
                        let _ = JsClosure::from_raw(cb as *const RawClosureHeader).call0();
                    }
                }
                drop(frame);
                lifecycle::drain_once_listeners(id, "end");
            }
            PendingNetEvent::ShutdownComplete(_) => {}
            PendingNetEvent::Close(id) => {
                extern "C" {
                    fn js_tls_client_record_closed(handle: i64);
                }
                js_tls_client_record_closed(id);
                let had_error = f64::from_bits(JsValue::from_bool(false).bits());
                let frame = dispatch_custody::DispatchFrame::park(listeners_for(id, "close"));
                for i in 0..frame.len() {
                    let cb = frame.cb(i);
                    if cb != 0 {
                        let _ = JsClosure::from_raw(cb as *const RawClosureHeader).call1(had_error);
                    }
                }
                drop(frame);
                statics::listeners().lock().unwrap().remove(&id);
                statics::sockets().lock().unwrap().remove(&id);
                statics::once_flags().lock().unwrap().remove(&id);
                statics::encodings().lock().unwrap().remove(&id);
                statics::http_agent_phases().lock().unwrap().remove(&id);
                statics::max_listeners().lock().unwrap().remove(&id);
                server_state::discard_pending_server_data(id);
            }
            // Issue #1123 followup — server-side events. The
            // accept loop pushes `ServerConnection`/`ServerListening`/
            // `ServerError`/`ServerClose`; the main-thread pump
            // converts them into the appropriate JS dispatch.
            PendingNetEvent::ServerConnection(server_id, socket_id, released) => {
                if !released && server_state::defer_server_connection(server_id, socket_id) {
                    continue;
                }
                server_state::activate_connection(server_id, socket_id);
                let cbs = listeners_for(server_id, "connection");
                if cbs.is_empty() {
                    // Drain any `server.once('connection', cb)` flagged
                    // here too — listeners_for returned empty but the
                    // once-set may still be holding stale entries.
                    lifecycle::drain_once_listeners(server_id, "connection");
                    server_state::release_pending_server_data(socket_id);
                    server_state::release_connection_callback(socket_id);
                    continue;
                }
                // Sockets returned by the codegen's `net.connect`
                // path (`js_net_socket_connect` → NR_PTR ret kind in
                // lower_call.rs) are NaN-boxed with POINTER_TAG over
                // the raw socket id. Match that here so user code
                // sees the same value shape regardless of which side
                // produced the socket: `sock.on(...)` then dispatches
                // through the `("net", true, "on", Some("Socket"))`
                // NATIVE_MODULE_TABLE row (which `unbox_to_i64`s the
                // receiver back to the raw id). Bare-number sockets
                // skipped the dispatch and hit the generic property
                // path → `(number).on is not a function`.
                let sock_f64 = f64::from_bits(
                    0x7FFD_0000_0000_0000 | (socket_id as u64 & 0x0000_FFFF_FFFF_FFFF),
                );
                // #8259: sock_f64 is a handle id (not a heap address), so
                // only the callbacks need custody.
                let frame = dispatch_custody::DispatchFrame::park(cbs);
                for i in 0..frame.len() {
                    let cb = frame.cb(i);
                    if cb != 0 {
                        let _ = JsClosure::from_raw(cb as *const RawClosureHeader).call1(sock_f64);
                    }
                }
                drop(frame);
                lifecycle::drain_once_listeners(server_id, "connection");
                server_state::release_pending_server_data(socket_id);
                server_state::release_connection_callback(socket_id);
            }
            PendingNetEvent::ServerListening(server_id) => {
                // Take + drain the 'listening' listeners so the
                // optional `listen(port, cb)` callback fires exactly
                // once (Node's semantics). Subsequent
                // `.on('listening', ...)` registrations would have
                // to wait for another `.listen(...)` cycle — fine,
                // re-binding without close() in between would error
                // on bind anyway.
                let cbs = {
                    let mut listeners = statics::listeners().lock().unwrap();
                    if let Some(per_server) = listeners.get_mut(&server_id) {
                        per_server.remove("listening").unwrap_or_default()
                    } else {
                        Vec::new()
                    }
                };
                // #8259: these were REMOVED from the table above (one-shot),
                // so this frame is their ONLY root during dispatch — without
                // it a collection here can free, not just move, them.
                let frame = dispatch_custody::DispatchFrame::park(cbs);
                for i in 0..frame.len() {
                    let cb = frame.cb(i);
                    if cb != 0 {
                        let _ = JsClosure::from_raw(cb as *const RawClosureHeader).call0();
                    }
                }
                drop(frame);
            }
            PendingNetEvent::ServerClose(server_id) => {
                // Drain close listeners (one-shot, like Node).
                let cbs = {
                    let mut listeners = statics::listeners().lock().unwrap();
                    if let Some(per_server) = listeners.get_mut(&server_id) {
                        per_server.remove("close").unwrap_or_default()
                    } else {
                        Vec::new()
                    }
                };
                // #8259: removed from the table above — custody is the only
                // root; see the ServerListening arm.
                let frame = dispatch_custody::DispatchFrame::park(cbs);
                for i in 0..frame.len() {
                    let cb = frame.cb(i);
                    if cb != 0 {
                        let _ = JsClosure::from_raw(cb as *const RawClosureHeader).call0();
                    }
                }
                drop(frame);
                // Tear down the server entry so the keepalive gate
                // (`js_ext_net_has_active_handles`) lets the runtime
                // exit cleanly after the user's close() resolves.
                server_state::remove_server(server_id);
                statics::servers().lock().unwrap().remove(&server_id);
                statics::listeners().lock().unwrap().remove(&server_id);
                statics::once_flags().lock().unwrap().remove(&server_id);
            }
            PendingNetEvent::ServerError(server_id, msg) => {
                let cbs = listeners_for(server_id, "error");
                if cbs.is_empty() {
                    // Node prints to stderr if there's no handler and
                    // crashes the process; we just log and continue —
                    // less hostile to test harnesses that haven't
                    // wired an error listener yet.
                    eprintln!("[perry-ext-net] server {} error: {}", server_id, msg);
                    continue;
                }
                // #8259: park before the allocating build_error_object.
                let mut frame = dispatch_custody::DispatchFrame::park(cbs);
                let err_f64 = build_error_object(&msg);
                frame.set_payload(err_f64.to_bits());
                for i in 0..frame.len() {
                    let cb = frame.cb(i);
                    if cb != 0 {
                        let _ = JsClosure::from_raw(cb as *const RawClosureHeader)
                            .call1(f64::from_bits(frame.payload_bits()));
                    }
                }
                drop(frame);
                lifecycle::drain_once_listeners(server_id, "error");
            }
            PendingNetEvent::ServerDrop(server_id, info) => {
                let cbs = listeners_for(server_id, "drop");
                if cbs.is_empty() {
                    lifecycle::drain_once_listeners(server_id, "drop");
                    continue;
                }
                // #8259: park before the allocating build_drop_object.
                let mut frame = dispatch_custody::DispatchFrame::park(cbs);
                let info = server_state::build_drop_object(&info);
                frame.set_payload(info.to_bits());
                for i in 0..frame.len() {
                    let cb = frame.cb(i);
                    if cb != 0 {
                        let _ = JsClosure::from_raw(cb as *const RawClosureHeader)
                            .call1(f64::from_bits(frame.payload_bits()));
                    }
                }
                drop(frame);
                lifecycle::drain_once_listeners(server_id, "drop");
            }
        }
        drop(provider_scope);
        for async_id in destroy_ids {
            if async_id != 0 {
                js_async_hooks_provider_destroy(async_id);
            }
        }
    }

    // Restore the (capacity-retaining) buffer to the thread-local so the
    // next tick reuses it. A re-entrant pump call during dispatch may
    // have left a grown buffer in the slot — keep whichever is larger.
    SCRATCH.with(|s| {
        let mut slot = s.borrow_mut();
        if events.capacity() >= slot.capacity() {
            *slot = events;
        }
    });

    count
}

mod handle_exports;
use handle_exports::listeners_for;
pub use handle_exports::{
    is_net_server_handle, is_net_socket_handle, js_ext_net_is_server_handle, js_ext_net_socket_on,
    js_ext_net_socket_once, js_ext_net_socket_remove_all_listeners,
    js_ext_net_socket_remove_listener, js_net_has_pending, js_net_server_listening,
    js_net_server_on,
};

#[cfg(test)]
mod tests;
