//! Native bindings for Node `net.Socket` — TCP plus optional TLS upgrade.
//!
//! Ported from `crates/perry-stdlib/src/net/mod.rs` to perry-ffi v0.5.x's
//! stable surface as part of #466 Phase 5. Every socket and listener lives on
//! the agent's turnloop loop (`turnloop_io`): one multishot read per socket,
//! submissions made where the FFI call happens, and completions turned into
//! `PendingNetEvent`s by this crate's sink. Read data is queued as a zero-copy
//! `Bytes` view into `NET_PENDING_EVENTS` and converted to `Buffer` on the JS
//! thread inside `js_net_process_pending` — the arena-safety rule (JSValue
//! construction MUST run on the agent's own thread, never in the I/O path).
//!
//! TLS runs as a sans-I/O rustls session *above* the turnloop handle
//! (`turnloop_tls_io`), for both `tls.connect` and `socket.upgradeToTLS`.
//!
//! This crate has no tokio. The tokio socket task that used to back a thread
//! with no loop of its own is replaced by posting to the thread that owns the
//! agent's loop (`turnloop_io::on_loop`); see that module's header.
//!
//! # Differences from the perry-stdlib version
//!
//! - Uses perry-ffi closures, buffers, and mutable GC root scanning; the latter
//!   rewrites listener pointers after a copying minor collection.
//!
//! TLS is unconditionally compiled in (no `#[cfg(feature = "tls")]` gates
//! like perry-stdlib has) — keeping the wrapper crate simple, the deps are
//! small. perry-stdlib's umbrella `net = ["async-runtime"]` + separate
//! `tls = ["net", ...]` feature split is preserved on the perry-stdlib side
//! for backwards compat; the well-known flip routes here.

use bytes::Bytes;
use perry_ffi::{
    alloc_buffer, alloc_string, gc_register_mutable_root_scanner_named, GcRootVisitor, JsClosure,
    JsPromise, JsValue, RawClosureHeader, StringHeader, TransientRootScope,
};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

// #1852 — topical sub-modules split out to keep this file under the
// 2000-line size gate. `tls` holds the rustls config + handshake; `ip`
// holds the `net.isIP*` + auto-select-family helpers.
mod ip;
// Process-wide freelist of read buffers, so the read sink recycles pooled
// 16 KiB capacity instead of allocating a fresh `BytesMut` per read. See
// `buffer_pool.rs` for the rationale.
mod buffer_pool;
mod bun_tcp;
// #10429: runtime callback for `net` exports used as values.
mod native_dispatch;
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
// #10444 — `net.Socket.prototype.pipe()`/`.unpipe()` (split out; see the
// module doc for why it doesn't reuse node:stream's own pipe machinery).
mod pipe;
pub(crate) use gc_roots::ensure_gc_scanner_registered;
mod socket_emit;
pub use socket_emit::{
    js_ext_net_register_http_agent_socket_event_hook, js_ext_net_set_http_agent_phase,
    js_ext_net_socket_emit, js_ext_net_socket_emit_abort_error,
};
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
pub use adopt::{
    adopt_turnloop_upgrade, adopt_upgraded_tcp_stream, ensure_adopted_socket_dispatch,
};

/// This crate's slot in the runtime's turnloop completion-sink registry.
///
/// Published so perry-ext-http can `turnloop_net::transfer` an upgraded
/// connection here by name rather than by a duplicated literal.
pub const TURNLOOP_SUBSYSTEM: u8 = turnloop_io::SUBSYSTEM;
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
    get_object_value_field, is_nanboxed_pointer, jsvalue_to_owned_string, jsvalue_to_socket_bytes,
    string_from_header_i64, unbox_pointer,
};

use crate::tls::TlsClientConfigData;

/// `node:net` on turnloop handles — the only transport (`turnloop_io.rs`).
mod turnloop_io;
pub mod turnloop_tls;
pub mod turnloop_tls_io;

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
/// the listen state + bound address. Per-server event
/// listeners (`'connection'`, `'listening'`, `'close'`, `'error'`) live
/// in the shared `statics::listeners()` map keyed by the server's id;
/// reusing the socket listener map keeps the GC scanner walk single-
/// pass instead of needing a second per-server scanner.
pub(crate) struct ServerState {
    pub async_id: u64,
    /// Set by `.listen()`. Keeps `has_active_handles` answering yes from the
    /// `listen()` call until the server's registry entry is removed by its
    /// `'close'`, including the window before the bind has been published.
    /// It replaced the tokio accept loop's shutdown sender, whose `is_some()`
    /// was the same question.
    pub listen_armed: bool,
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
    /// True only between `js_net_socket_alloc` and the first
    /// `js_net_socket_method_connect` (issue #422: `new net.Socket()` then
    /// `sock.connect(port, host)`). A second `connect()` finds it false and
    /// reports "already connected"; `has_active_handles` does not count a
    /// socket that has never been asked to connect.
    ///
    /// It is the whole of what the old per-socket command *receiver* parked
    /// here meant. Commands issued while it is true are accepted and counted
    /// but not delivered — exactly what happened to the ones that receiver
    /// buffered, which the turnloop connect path dropped with it.
    pub(crate) awaiting_connect: bool,
    pub(crate) is_open: bool,
    /// Borrowed OS descriptor for Node's private `socket._handle.fd` shape.
    /// turnloop owns its descriptors without exposing them, so this is unset
    /// on every socket today; the field keeps the getter's shape.
    pub(crate) raw_fd: Option<i32>,
    /// Whether pending socket I/O keeps the process event loop alive.
    pub(crate) refed: bool,
    /// Issue #2131 — the kernel-assigned local address, populated after
    /// `TcpStream::connect`/`accept`. Drives `socket.address()` so the
    /// "undefined.address" cluster reports the actual bound port/family.
    pub(crate) local_addr: Option<SocketAddr>,
    pub(crate) remote_addr: Option<SocketAddr>,
    /// #2154 — raw-consumer mode (see `raw_bridge`). When `Some`, the read
    /// sink buffers inbound bytes here for `perry-ext-http` to drain instead
    /// of firing JS `'data'` events.
    raw: Option<Arc<Mutex<RawReadState>>>,
    /// #2549 — Node `net.Socket` lifecycle/counter property surface.
    /// `destroyed` flips true on `.destroy()`/peer close; drives
    /// `socket.destroyed` and the `readyState` string. Byte counters track
    /// `socket.bytesRead`/`socket.bytesWritten`. `timeout` holds the value set
    /// via `setTimeout(ms)` (Node reports `undefined` until one is set).
    pub(crate) destroyed: bool,
    /// #10465 — true from `net.connect()`/`socket.connect()` until the
    /// attempt resolves (open, error, or destroy). `false` both before any
    /// connect attempt (`new net.Socket()`) and once resolved — matches
    /// Node's `socket.connecting`, which Perry previously hardcoded to
    /// `false` unconditionally.
    pub(crate) connecting: bool,
    /// #10465 — true from the moment the socket FIRST became open (set
    /// alongside every `is_open = true` transition, and at construction for
    /// a socket that starts already open, e.g. a server-accepted
    /// connection), never reset back to `false`. `socket.pending`'s real
    /// Node formula is "no live handle", which — once a socket has ever
    /// connected — reduces to `destroyed`, NOT `!is_open`: `is_open` itself
    /// flips false earlier than `destroyed` does (via
    /// `server_state::mark_socket_closed`, called from `mark_closed` in the
    /// completion sink as soon as teardown starts, well before the main
    /// thread has processed the `'end'`/`'close'` events those pushed). A
    /// `pending` getter keyed on `is_open` directly read `true` from inside
    /// the `'end'` listener, where Node still reports `false`. `destroyed`
    /// doesn't have that problem — see `socket_events.rs`'s `Close` arm —
    /// so `has_opened` lets the getter pick the RIGHT signal for each phase:
    /// `!has_opened` (never connected / still connecting) or `destroyed`
    /// (has connected at least once).
    pub(crate) has_opened: bool,
    /// #10465 — true as soon as `.end()`/`.destroy()` is called, independent
    /// of whether the FIN has actually flushed. Drives `socket.writable` and
    /// `socket.writableEnded`.
    pub(crate) writable_ended: bool,
    /// #10465 — true once the readable side has seen EOF (peer FIN) and the
    /// `'end'` event has fired. Drives `socket.readable` and
    /// `socket.readableEnded`.
    pub(crate) readable_ended: bool,
    pub(crate) bytes_read: u64,
    pub(crate) bytes_written: u64,
    pub(crate) bytes_queued: u64,
    pub(crate) timeout: Option<u64>,
    pub(crate) type_of_service: u8,
    pub(crate) server_id: Option<i64>,
    pub(crate) server_connection_active: bool,
    pub(crate) tls: TlsSocketMetadata,
    /// A connect, accept or adoption has been submitted to the agent's
    /// turnloop loop for this socket, so its commands are submissions. False
    /// only for a socket that never got that far — allocated and not yet
    /// connected, or whose connect was refused before the driver took it.
    pub(crate) turnloop: bool,
}

impl SocketState {
    /// Deliver one socket command to the loop that owns this socket.
    ///
    /// The single choke point: every `socket.write` / `.end()` / `.destroy()`
    /// / `.setNoDelay()` call site goes through here.
    ///
    /// Callers hold the socket registry lock, so this updates `bytes_queued`
    /// itself and returns the failure message instead of emitting it — the
    /// caller drops the lock first and then reports through
    /// [`turnloop_io::submission_failed`] or its own path.
    pub(crate) fn command(&mut self, id: i64, cmd: SocketCommand) -> Result<(), String> {
        let bytes = match &cmd {
            SocketCommand::Write(bytes, _) => bytes.len() as u64,
            _ => 0,
        };
        if !self.turnloop {
            // Never connected. Accepted and counted, then dropped: see
            // `awaiting_connect`. A socket that is not even waiting for a
            // connect any more (its connect was refused) has nothing to take
            // the command, which is the "write failed" the old closed channel
            // reported.
            if self.awaiting_connect {
                self.bytes_queued = self.bytes_queued.saturating_add(bytes);
                return Ok(());
            }
            return Err("Socket write failed".to_string());
        }
        let submits = matches!(
            cmd,
            SocketCommand::Write(..) | SocketCommand::End(_) | SocketCommand::Destroy
        );
        if submits && !turnloop_io::enabled() {
            // A second thread acting for this agent: the socket lives on the
            // owner's loop, so the command is carried there. The queue length
            // is provisional until the owner reports the driver's own count.
            turnloop_io::command_on_owner(id, cmd)?;
            self.bytes_queued = self.bytes_queued.saturating_add(bytes);
            return Ok(());
        }
        let mut queued = None;
        let result = turnloop_io::command(id, cmd, &mut queued);
        if let Some(queued) = queued {
            self.bytes_queued = queued;
        }
        result
    }
}

/// Publish a connection turnloop accepted as a normal `net.Socket`.
///
/// The turnloop twin of `ipc::register_accepted_transport`: same registries,
/// same `'connection'` event, no task and no command channel.
pub(crate) fn register_turnloop_socket(
    server_id: i64,
    socket_id: i64,
    local_addr: Option<SocketAddr>,
    remote_addr: Option<SocketAddr>,
) {
    ensure_gc_scanner_registered();
    dispatch::ensure_runtime_dispatch_registered();
    statics::sockets().lock().unwrap().insert(
        socket_id,
        SocketState {
            tcp_async_id: 0,
            connect_async_id: 0,
            shutdown_async_id: 0,
            awaiting_connect: false,
            is_open: true,
            raw_fd: None,
            refed: true,
            local_addr,
            remote_addr,
            raw: None,
            destroyed: false,
            // #10465: accepted-socket values, as in `adopt.rs`.
            connecting: false,
            has_opened: true,
            writable_ended: false,
            readable_ended: false,
            bytes_read: 0,
            bytes_written: 0,
            bytes_queued: 0,
            timeout: None,
            type_of_service: 0,
            server_id: Some(server_id),
            server_connection_active: false,
            tls: TlsSocketMetadata::default(),
            turnloop: true,
        },
    );
    statics::listeners()
        .lock()
        .unwrap()
        .insert(socket_id, Default::default());
}

#[cfg(test)]
impl SocketState {
    /// Minimal socket state that has not reached the loop, for the byte
    /// accounting tests. `awaiting_connect` picks whether a command is
    /// accepted (a socket still waiting for `connect()`) or refused (one whose
    /// connect never reached the driver).
    pub(crate) fn for_test(awaiting_connect: bool) -> Self {
        SocketState {
            tcp_async_id: 0,
            connect_async_id: 0,
            shutdown_async_id: 0,
            awaiting_connect,
            is_open: true,
            raw_fd: None,
            refed: true,
            local_addr: None,
            remote_addr: None,
            raw: None,
            destroyed: false,
            connecting: false,
            has_opened: true,
            writable_ended: false,
            readable_ended: false,
            bytes_read: 0,
            bytes_written: 0,
            bytes_queued: 0,
            timeout: None,
            type_of_service: 0,
            server_id: None,
            server_connection_active: false,
            tls: TlsSocketMetadata::default(),
            turnloop: false,
        }
    }
}

pub(crate) enum SocketCommand {
    Write(Vec<u8>, u64),
    End(u64),
    Destroy,
    /// The main thread finished dispatching the accepted socket's
    /// `connection` callback. Commands queued by that callback precede this
    /// marker, so a peer FIN may now auto-close without dropping its response.
    ServerConnectionReady,
}

#[derive(Debug)]
enum PendingNetEvent {
    /// `.1` identifies a same-process server target and whether its admission
    /// is expected to hit `dropMaxConnection`; external connects use `None`.
    Connect(i64, Option<(i64, bool)>),
    SecureConnect(i64),
    /// One chunk of read data. Carried as a refcounted `Bytes` — a zero-copy
    /// view sliced out of the read sink's reused buffer (`split_to`) —
    /// so the path from the receive buffer to the main-thread drain handler
    /// (which only borrows it as `&[u8]`) stays alloc-free per read.
    Data(i64, Bytes),
    /// Peer half-closed (FIN received); public readable-side `end` event.
    End(i64),
    /// A queued `socket.write` finished with a completion token and optional error.
    WriteComplete(i64, u64, Option<String>),
    /// `socket.end()` writable shutdown with a completion token and optional error.
    ShutdownComplete(i64, u64, Option<String>),
    Close(i64),
    Error(i64, String),
    AbortError(i64),
    /// Accept-loop produced a socket for the server's `connection` listeners.
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
    if let Some(socket) = statics::sockets().lock().unwrap().get_mut(&id) {
        socket.raw_fd = None;
    }
    server_state::mark_socket_closed(id);
    // #10465 — `destroyed`/`is_open`/`connecting` are NOT flipped here on
    // purpose. `mark_closed` runs in the completion sink immediately after
    // queuing the `Close` (and, on this path, `End`) pending events — well
    // before the main thread's `js_ext_net_drain_pending` has processed
    // either. Flipping the fields here (an earlier version of this fix did)
    // made them ALREADY read "destroyed" from inside the `'end'` listener,
    // which fires first and, in real Node, still observes `destroyed:
    // false`. `socket_events::js_ext_net_drain_pending`'s `Close` arm sets
    // them instead, synchronously with firing `'close'` — the one point
    // where Node's own timing and this runtime's actually agree.
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
/// Distinct-symbol alias of `js_net_socket_connect` for generated code and the
/// value-form dispatcher (`native_dispatch.rs`). The
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
    // string (NaN-boxed), arg3 is the optional connectListener. Accept only
    // actual JS string tags: arg2 may instead be the connectListener closure,
    // whose POINTER_TAG storage must never be read as a StringHeader (#8909).
    let (host, listener_f64) = match jsvalue_to_owned_string(arg2_f64) {
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
    let tcp_async_id = init_provider(b"TCPWRAP");
    statics::sockets().lock().unwrap().insert(
        id,
        SocketState {
            tcp_async_id,
            connect_async_id: 0,
            shutdown_async_id: 0,
            awaiting_connect: true,
            is_open: false,
            raw_fd: None,
            refed: true,
            local_addr: None,
            remote_addr: None,
            raw: None,
            destroyed: false,
            connecting: false,
            has_opened: false,
            writable_ended: false,
            readable_ended: false,
            bytes_read: 0,
            bytes_written: 0,
            bytes_queued: 0,
            timeout: None,
            type_of_service: 0,
            server_id: None,
            server_connection_active: false,
            tls: TlsSocketMetadata::default(),
            turnloop: false,
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
    // `servers()` map alongside the listener-map entry.
    // `js_net_server_listen` populates the listen state + bound address
    // fields; `js_net_server_close` closes the listener on the loop.
    statics::servers().lock().unwrap().insert(
        id,
        ServerState {
            async_id: 0,
            listen_armed: false,
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
/// or a Unix-domain socket and start a multishot accept on the agent's loop.
/// The `callback` (a NaN-boxed closure pointer in the codegen's
/// NA_PTR slot, raw i64 here after unboxing in lower_call.rs) is
/// registered as a one-shot `'listening'` listener; when the bind
/// resolves, a `ServerListening` event is pushed so
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
    let roots = TransientRootScope::enter();
    let arg2 = roots.root_nanbox(arg2);
    let arg3 = roots.root_nanbox(arg3);
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
        // `listen(port, callback)` places the callback in arg2. Keep the host
        // boundary strict so closure/object storage cannot become a hostname;
        // real heap and SSO strings are copied at their logical byte length.
        let host = jsvalue_to_owned_string(arg2.get()).unwrap_or_else(|| "0.0.0.0".to_string());
        (port as u16, host)
    };
    let server_async_id = init_provider(b"TCPSERVERWRAP");
    let callback_i64 = match js_net_callback_ptr(arg3.get()) {
        0 => js_net_callback_ptr(arg2.get()),
        cb => cb,
    };

    // Mark the server as listening. If the handle isn't registered, bail
    // before touching the loop.
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
        s.listen_armed = true;
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

    let server_id = handle;
    let no_loop_target = match &path {
        Some(path) => path.clone(),
        None => format!("{}:{}", host, port_u16),
    };
    // Bind and accept on the agent's turnloop loop. `tcp_listen` binds
    // synchronously, so an EADDRINUSE is known inside the submission — but it
    // still reaches JS through the pending-event queue, so `'error'` stays
    // asynchronous exactly as Node's is.
    let submitted = turnloop_io::on_loop(move || match path {
        Some(path) => match turnloop_io::listen_pipe(server_id, &path, 511) {
            Ok(()) => push_event(PendingNetEvent::ServerListening(server_id)),
            Err(err) => fail_listen(server_id, format!("bind {}: {}", path, err.message())),
        },
        None => match turnloop_io::listen_tcp(server_id, &host, port_u16, 511) {
            Ok(()) => {
                publish_bound_address(server_id);
                push_event(PendingNetEvent::ServerListening(server_id));
            }
            Err(err) => fail_listen(
                server_id,
                format!("bind {}:{}: {}", host, port_u16, err.message()),
            ),
        },
    });
    if !submitted {
        fail_listen(
            server_id,
            format!("bind {}: {}", no_loop_target, turnloop_io::NO_LOOP_CODE),
        );
    }
}

/// Record the address the kernel actually bound, before `'listening'` fires.
///
/// `server.listen(0, () => client.connect(server.address().port))` is the
/// dominant pattern in Node's own net tests; reporting the requested port (0)
/// instead of the real one makes every such test connect to port 0.
fn publish_bound_address(server_id: i64) {
    let Some(local) = turnloop_io::local_endpoint(server_id) else {
        return;
    };
    if let Ok(mut servers) = statics::servers().lock() {
        if let Some(s) = servers.get_mut(&server_id) {
            s.bound_port = local.port;
            s.bound_host = local.address.clone();
        }
    }
    unsafe {
        perry_cluster_worker_listening(
            local.address.as_ptr(),
            local.address.len() as u32,
            local.port as i32,
            local.family,
        );
    }
}

/// A bind that failed: Node emits `'error'` then `'close'`, and the server is
/// no longer listening.
fn fail_listen(server_id: i64, message: String) {
    push_event(PendingNetEvent::ServerError(server_id, message));
    push_event(PendingNetEvent::ServerClose(server_id));
    if let Ok(mut servers) = statics::servers().lock() {
        if let Some(s) = servers.get_mut(&server_id) {
            s.listening = false;
        }
    }
}

/// `server.close(callback?)` — close the listener and fire the optional
/// callback once it is gone. The actual `'close'` listener dispatch happens
/// in the main-thread pump when the listener's terminal `Closed` completion
/// pushes `ServerClose`.
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
    // Closing the listener cancels its multishot accept and delivers a
    // terminal `Closed`, which is what pushes `'close'`. Decided on the loop's
    // owner, because only that thread can see whether the listener is live.
    // A server that never bound (its `listen()` failed, or never ran) has no
    // listener to close; it only stops counting as a live handle, which is
    // what dropping the old accept loop's shutdown sender did.
    let forget = move || {
        if let Ok(mut servers) = statics::servers().lock() {
            if let Some(s) = servers.get_mut(&handle) {
                s.listen_armed = false;
            }
        }
    };
    let submitted = turnloop_io::on_loop(move || {
        if turnloop_io::owns(handle) {
            turnloop_io::close_server(handle);
        } else {
            forget();
        }
    });
    if !submitted {
        forget();
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
/// IPC connection on a socket previously allocated by `new net.Socket()`,
/// under that socket's own id, so any listener already registered
/// (`sock.on('data', cb)`) sees the same handle once the connect completes.
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

    let tcp_async_id = {
        let mut guard = statics::sockets().lock().unwrap();
        match guard.get_mut(&handle) {
            Some(socket) if socket.awaiting_connect => {
                socket.awaiting_connect = false;
                socket.tcp_async_id
            }
            _ => {
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
        // #10465 — `socket.connect(...)` on a `new net.Socket()` starts
        // connecting synchronously from the caller's point of view, same as
        // the eager `net.connect()` factory.
        socket.connecting = true;
    }

    let local_server = server_state::begin_local_connect(&host, port);
    // The deferred-connect shape (`new net.Socket()` then `socket.connect()`,
    // which is also what `Bun.connect` and `net.Socket.prototype.connect`
    // lower to) takes the same route `net.connect()` does.
    submit_tcp_connect(handle, host, port, local_server, None);
}

/// Submit an outbound TCP connect for socket `id` on the agent's loop, and
/// report a refusal the way a failed connect is reported.
///
/// `direct_tls` is `tls.connect`'s request for TLS from byte zero: the session
/// is installed the instant the connect completes, before `'connect'` is
/// pushed.
fn submit_tcp_connect(
    id: i64,
    host: String,
    port: u16,
    local_server: Option<(i64, bool)>,
    direct_tls: Option<(String, bool, TlsClientConfigData)>,
) {
    // Published before the submission, not after it: on a thread that posts
    // to the loop's owner, a `socket.write()` issued before the connect has
    // run must already be routed to the loop rather than parked.
    set_turnloop(id, true);
    let target = host.clone();
    let submitted = turnloop_io::on_loop(move || {
        match turnloop_io::connect_tcp(id, &host, port, true) {
            Ok(()) => {
                turnloop_io::note_local_connect(id, local_server);
                if let Some((servername, verify, config)) = direct_tls {
                    turnloop_io::note_direct_tls(id, servername, verify, config);
                }
            }
            Err(err) => refuse_connect(id, &err.code, &host, port, local_server),
        }
    });
    if !submitted {
        refuse_connect(id, turnloop_io::NO_LOOP_CODE, &target, port, local_server);
    }
}

/// A connect the driver never took: `'error'` then `'close'`, and the socket
/// never reached the loop, so its later commands are refused rather than
/// submitted.
fn refuse_connect(id: i64, code: &str, host: &str, port: u16, local_server: Option<(i64, bool)>) {
    set_turnloop(id, false);
    server_state::cancel_local_connect(local_server);
    // libuv's shape, which is Node's `err.message` and the only place
    // `err.code` / `errno` / `syscall` come from (`build_error_object` parses
    // it).
    push_event(PendingNetEvent::Error(
        id,
        format!("connect {} {}:{}", code, host, port),
    ));
    push_event(PendingNetEvent::Close(id));
    mark_closed(id);
}

fn set_turnloop(id: i64, on_loop: bool) {
    if let Some(s) = statics::sockets().lock().unwrap().get_mut(&id) {
        s.turnloop = on_loop;
    }
}

// ─── FFI: tls.connect ────────────────────────────────────────────────────────
// `js_tls_connect` lives in tls.rs (this file is at the 2000-line gate);
// it resolves Node's connect overloads and reuses `spawn_socket_task`.

/// Internal: allocate the handle and submit its connect.
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
/// its connect can complete. TLS uses this boundary to publish its runtime
/// metadata without racing a fast loopback handshake.
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
            awaiting_connect: false,
            is_open: false,
            raw_fd: None,
            refed: true,
            local_addr: None,
            remote_addr: None,
            raw: None,
            destroyed: false,
            connecting: true,
            has_opened: false,
            writable_ended: false,
            readable_ended: false,
            bytes_read: 0,
            bytes_written: 0,
            bytes_queued: 0,
            timeout: None,
            type_of_service: 0,
            server_id: None,
            server_connection_active: false,
            tls: TlsSocketMetadata::default(),
            turnloop: false,
        },
    );
    statics::listeners()
        .lock()
        .unwrap()
        .insert(id, HashMap::new());
    initialize(id);

    // TLS runs above the turnloop handle (`turnloop_tls_io`), so a client that
    // may later be upgraded needs no descriptor handover and lives on the loop
    // from the start. `tls.connect` (`direct_tls`) installs its session the
    // moment the connect completes, before `'connect'` is pushed.
    submit_tcp_connect(id, host, port, local_server, direct_tls);
    id
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

/// `socket.upgradeToTLS(servername, verify)` — installs a client TLS session
/// above the socket's turnloop handle and returns a Promise that resolves when
/// the handshake completes (or rejects on failure).
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

    let turnloop = {
        let sockets = statics::sockets().lock().unwrap();
        match sockets.get(&handle) {
            Some(s) => s.turnloop,
            None => {
                promise.reject_string(&format!("socket {} not found", handle));
                return promise_raw;
            }
        }
    };
    if !turnloop {
        // Never connected, or its connect was refused: there is no stream to
        // put a session on.
        promise.reject_string("socket is not connected");
        return promise_raw;
    }

    // The upgrade happens in place. No descriptor changes hands — the rustls
    // session is installed *above* the same handle. The promise is held by a
    // native-async token rather than a bare `*mut Promise` in a side table, so
    // the runtime pins and root-scans it across the collections that happen
    // while the handshake is in flight (#9552); the token settles on the loop
    // thread, inside the same dispatch that sees the handshake finish.
    let token =
        perry_ffi::JsNativeAsyncCompletion::with_flags(perry_ffi::PERRY_NATIVE_ASYNC_THREAD_MAIN);
    let token_promise = token.promise();
    // The promise minted above is unused on this path; settle it so the
    // runtime never carries a permanently pending one.
    promise.resolve_undefined();
    let verify = verify != 0.0;
    // `begin_client_upgrade` settles the token on every failure path, so the
    // caller does not have to get it back to reject it. It runs on the loop's
    // owner, which is this thread unless a second thread is acting for the
    // agent.
    let token = Arc::new(Mutex::new(Some(token)));
    let job_token = token.clone();
    let submitted = turnloop_io::on_loop(move || {
        let token = job_token.lock().unwrap_or_else(|e| e.into_inner()).take();
        let _ = turnloop_tls_io::begin_client_upgrade(
            handle,
            servername,
            verify,
            TlsClientConfigData::default(),
            token,
        );
    });
    if !submitted {
        if let Some(token) = token.lock().unwrap_or_else(|e| e.into_inner()).take() {
            token.reject_string(turnloop_io::NO_LOOP_CODE);
        }
    }
    token_promise
}

// ─── Main-thread event pump ──────────────────────────────────────────────────

/// Dispatches queued socket events to JS listeners on the main thread.
/// Called from codegen's event-loop tick (via the well-known pending-events
/// pump).
///
/// Per the arena-safety rule: JSValue construction (Buffer, error string)
/// happens HERE on the main thread, never in the completion sink.
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

mod socket_events;
pub use socket_events::js_ext_net_drain_pending;

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
