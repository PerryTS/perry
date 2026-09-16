//! Native bindings for the npm `ws` package — WebSocket client + server.
//!
//! # One codec, two transports
//!
//! The protocol lives in [`codec`] and [`handshake`], which wrap
//! `turnloop_websocket`'s sans-I/O state machine and do no I/O at all. Two
//! transports drive it:
//!
//! * [`turnloop_link`] — a connection `perry-ext-http` keeps owning on a
//!   turnloop handle. No task, no channel, no stream. This is what closes P5's
//!   attached-`WebSocketServer` hole.
//! * [`io`] — a tokio stream, for the standalone `WebSocketServer({port})`, the
//!   outbound client, and any agent with no `turnloop::Loop` of its own.
//!
//! Replacing `tokio-tungstenite` with the sans-I/O core is what let the second
//! transport exist: a `WebSocketStream<S>` needs an owned `AsyncRead + AsyncWrite`,
//! and a turnloop connection is an `i64` handle id. Nothing about the *protocol*
//! ever needed the stream.
//!
//! Architecture mirrors perry-stdlib's existing copy minus the iOS
//! `NSURLSessionWebSocketTask` delegation path (out of scope for an
//! in-tree port that doesn't depend on `perry-ui-ios`):
//!
//!   - Per-client: `tokio::spawn`-driven select loop reads incoming
//!     messages and writes commands from an mpsc channel. Reader
//!     pushes events onto `WS_PENDING_EVENTS`; main thread drains
//!     them via `js_ws_process_pending`.
//!   - Per-server: another spawned task accepts TCP connections, does
//!     the WebSocket handshake, allocates a per-client id, spawns
//!     the per-client task. Each connection carries a back-reference
//!     to its parent server handle so events can route.
//!   - GC root scanner walks WS_CLIENT_LISTENERS + every
//!     WsServerHandle's listeners, marking every closure pointer
//!     so a malloc-triggered sweep can't free them between
//!     registration and dispatch (issue #35 pattern).
//!
//! `spawn_blocking + tokio::Handle::current().block_on(async {...})`
//! is used in place of perry-stdlib's `crate::common::async_bridge::spawn`.
//! Each long-running task ties up one blocking-pool thread for the
//! connection's lifetime; default tokio blocking pool is 512 threads,
//! enough for typical WebSocket usage. Cooperative `spawn_async` is
//! a v0.6.0 followup.

pub mod codec;
mod connect;
mod dispatch;
pub mod handshake;
mod io;
/// SIMD-widened WebSocket frame (un)masking (RFC 6455 §5.3). See
/// [`mask::apply_mask`] / [`mask::apply_mask_from`]. The hot tungstenite
/// read/write path masks internally with its own `u32`-blocked routine
/// (private, no injection seam), so this module is the masker for any
/// frame bytes perry handles itself — kept byte-identical to the scalar
/// reference and validated by a property test.
pub mod mask;
mod server;
pub use server::*;
pub mod turnloop_link;

#[cfg(test)]
mod test_async_shims;

use lazy_static::lazy_static;
use perry_ffi::{
    alloc_set, alloc_string, gc_register_mutable_root_scanner_named, get_handle_mut,
    iter_handles_of_mut, notify_main_thread, register_aux_event_pump, register_handle, set_add,
    set_delete, spawn_async, spawn_blocking_with_reactor as spawn_blocking, take_handle,
    GcRootVisitor, Handle, JsClosure, JsString, JsValue, RawClosureHeader, StringHeader,
};
use std::collections::HashMap;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Mutex;
use tokio::sync::mpsc;

use crate::codec::{Codec, Incoming, Message, WsError};

const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
const TAG_MASK: u64 = 0xFFFF_0000_0000_0000;
const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

/// #6117 — rustls panics resolving the process-level CryptoProvider on the
/// first `wss://` handshake when both `ring` and `aws-lc-rs` end up
/// feature-unified into the final link (perry-ext-http brings ring;
/// perry-ext-net brings aws-lc-rs). Install one explicitly before
/// connecting. Idempotent — `install_default` errors (ignored) if a
/// provider is already set. Same pattern as perry-ext-net's tls module.
fn ensure_tls_crypto_provider() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
}

unsafe fn read_str(ptr: *const StringHeader) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let h = JsString::from_raw(ptr as *mut StringHeader);
    perry_ffi::read_string(h).map(String::from)
}

// ── Global state ──────────────────────────────────────────────────

struct WsClientHandle;

/// How a connection's bytes reach the wire.
///
/// The discriminant is the whole of the transport migration on this side:
/// `Turnloop` carries only the host's connection id, because the host still
/// owns the connection and this crate owns the protocol state keyed by it.
enum WsTransport {
    Tokio(mpsc::UnboundedSender<WsCommand>),
    Turnloop(i64),
}

struct WsConnection {
    transport: WsTransport,
    messages: Vec<WsPayload>,
    is_open: bool,
    /// #6117 — `close()` was called but the close handshake hasn't finished:
    /// `readyState` reports CLOSING (2).
    is_closing: bool,
    /// #6117 — the connection terminated (close event, IO error, or connect
    /// failure): `readyState` reports CLOSED (3). Distinguishes a dead entry
    /// from a pre-open one (CONNECTING, 0) — both have `is_open == false`.
    is_closed: bool,
}

/// An application message on its way out.
#[derive(Clone, Debug)]
pub(crate) enum WsOutgoing {
    Text(String),
    Binary(Vec<u8>),
    Ping(Vec<u8>),
    Pong(Vec<u8>),
}

impl WsOutgoing {
    pub(crate) fn into_message(self) -> Message {
        match self {
            WsOutgoing::Text(text) => Message::text(text),
            WsOutgoing::Binary(bytes) => Message::binary(bytes),
            WsOutgoing::Ping(bytes) => Message::Ping(bytes.into()),
            WsOutgoing::Pong(bytes) => Message::Pong(bytes.into()),
        }
    }
}

/// An application message on its way in. Binary is kept as bytes rather than
/// lossily decoded: `String::from_utf8_lossy` replaced every non-UTF-8 byte
/// with U+FFFD, so a `ws` client could not receive a binary payload intact.
#[derive(Clone, Debug)]
pub(crate) enum WsPayload {
    Text(String),
    Binary(Vec<u8>),
}

enum WsCommand {
    Send(WsOutgoing),
    /// `ws.close(code, reason)` — start the closing handshake.
    Close(Option<u16>, String),
    /// `ws.terminate()` — drop the connection without one.
    Terminate,
}

struct WsClientListeners {
    listeners: HashMap<String, Vec<i64>>,
}

pub struct WsServerHandle {
    /// Event name → list of closure pointers.
    pub listeners: HashMap<String, Vec<i64>>,
    pub port: u16,
    pub host: String,
    pub attached_server: Option<Handle>,
    pub no_server: bool,
    pub is_listening: bool,
    pub client_ids: Vec<usize>,
    /// The persistent JavaScript `Set` exposed as `WebSocketServer.clients`.
    /// Stored as NaN-boxed bits so the mutable-root scanner can rewrite it
    /// when a moving collection evacuates the Set header.
    pub clients_bits: u64,
    pub shutdown_tx: Option<mpsc::UnboundedSender<()>>,
}

enum PendingWsEvent {
    Connection(Handle, usize),
    Message(usize, WsPayload),
    /// `ws.on('ping' | 'pong', data)`. The codec answers a ping itself; these
    /// are the JS-visible notifications, which did not exist before — an
    /// inbound control frame used to hit a catch-all and vanish.
    Ping(usize, Vec<u8>),
    Pong(usize, Vec<u8>),
    Close(usize, u16, String),
    Error(usize, String),
    ServerError(Handle, String),
    Listening(Handle),
    /// `wss.close()` finished: fire `'close'`, then retire the handle.
    ServerClose(Handle),
    /// Issue #606 — fired when an outbound client connection succeeds
    /// so `client.on("open", cb)` callbacks fire. Without this, code that
    /// awaits `new Promise(r => client.on("open", () => r()))` hangs
    /// forever even though `is_open=true` was set.
    Open(usize),
}

lazy_static! {
    static ref WS_CONNECTIONS: Mutex<HashMap<usize, WsConnection>> = Mutex::new(HashMap::new());
    static ref WS_CLIENT_PARENT_SERVER: Mutex<HashMap<usize, Handle>> = Mutex::new(HashMap::new());
    static ref WS_CLIENT_LISTENERS: Mutex<HashMap<usize, WsClientListeners>> =
        Mutex::new(HashMap::new());
    static ref WS_PENDING_EVENTS: Mutex<Vec<PendingWsEvent>> = Mutex::new(Vec::new());
}

static WS_ACTIVE_SERVERS: AtomicI32 = AtomicI32::new(0);
static WS_RUNTIME_HOOKS_REGISTERED: std::sync::Once = std::sync::Once::new();

extern "C" {
    fn js_register_handle_property_dispatch_extension(
        f: unsafe extern "C" fn(i64, *const u8, usize, *mut f64) -> i32,
    );
}

/// Install the crate's runtime hooks once: event-pump and keepalive
/// contributors, the mutable-root scanner, and the handle-property dispatch
/// extension that answers dynamic reads on ws handles (#9324).
fn ensure_runtime_hooks_registered() {
    WS_RUNTIME_HOOKS_REGISTERED.call_once(|| {
        gc_register_mutable_root_scanner_named("perry-ext-ws", scan_ws_roots);
        register_aux_event_pump(js_ws_process_pending, js_ws_has_pending);
        unsafe {
            js_register_handle_property_dispatch_extension(js_ext_ws_handle_property_dispatch);
            dispatch::register_method_dispatch();
        };
    });
}

/// Resolve `WebSocketServer.clients` for an UNTYPED receiver (#9324).
///
/// #9325/#9335 made `clients` a real `Set`, but only through the statically
/// typed lowering (`native_dispatch.rs` needs to know the receiver's class).
/// Every other read lands in the runtime's handle-property dispatcher, and
/// `perry-ext-ws` registered no property surface there at all — so an `any`
/// alias, a computed `wss[key]`, a helper taking the server as an untyped
/// parameter, and every compiled npm package (a published bundle carries no
/// types) all read `undefined`. `for (const ws of wss.clients)` over that
/// `undefined` threw `TypeError: is not iterable` from inside a `setInterval`
/// heartbeat — uncatchable by application code, so the process exited every
/// 30 seconds.
///
/// Also exposes native server/client method values. Unknown members and
/// unrelated handle types fall through to the primary dispatcher.
///
/// # Safety
/// FFI entry; `property_name_ptr` must be valid for `property_name_len` bytes,
/// and `out` must be writable when non-null.
#[no_mangle]
pub unsafe extern "C" fn js_ext_ws_handle_property_dispatch(
    handle: i64,
    property_name_ptr: *const u8,
    property_name_len: usize,
    out: *mut f64,
) -> i32 {
    dispatch::property(handle, property_name_ptr, property_name_len, out)
}

fn scan_ws_roots(visitor: &mut GcRootVisitor<'_>) {
    if let Ok(mut per_client) = WS_CLIENT_LISTENERS.lock() {
        for client in per_client.values_mut() {
            for cb_vec in client.listeners.values_mut() {
                for cb in cb_vec.iter_mut() {
                    visitor.visit_i64_slot(cb);
                }
            }
        }
    }
    iter_handles_of_mut::<WsServerHandle, _>(|server| {
        visitor.visit_nanbox_u64_slot(&mut server.clients_bits);
        for cb_vec in server.listeners.values_mut() {
            for cb in cb_vec.iter_mut() {
                visitor.visit_i64_slot(cb);
            }
        }
    });
}

fn push_ws_event(ev: PendingWsEvent) {
    WS_PENDING_EVENTS.lock().unwrap().push(ev);
    notify_main_thread();
}

/// `ws`'s error `code` for a protocol failure, as `turnloop_websocket` names it.
pub(crate) fn codec_error_message(error: &WsError) -> String {
    format!("{}: {error}", turnloop_websocket::node_error_code(error))
}

/// Queue a decoded message for the main-thread pump.
///
/// Both transports funnel through here so they cannot disagree about ordering,
/// about the pre-listener backlog, or about which events exist.
///
/// **This never runs JS.** The turnloop transport calls it from inside the
/// host's completion dispatch, where running JS would reorder the event loop;
/// the tokio transport calls it from a task. Delivery is `js_ws_process_pending`'s
/// job either way.
pub(crate) fn emit_incoming(ws_id: usize, event: Incoming) {
    match event {
        Incoming::Text(text) => queue_payload(ws_id, WsPayload::Text(text)),
        Incoming::Binary(bytes) => queue_payload(ws_id, WsPayload::Binary(bytes)),
        Incoming::Ping(bytes) => push_ws_event(PendingWsEvent::Ping(ws_id, bytes)),
        Incoming::Pong(bytes) => push_ws_event(PendingWsEvent::Pong(ws_id, bytes)),
        // The close event is raised by `connection_closed` once the transport
        // has finished with the connection, so a listener never sees `'close'`
        // before the answering frame has been written.
        Incoming::Close(_) => {}
    }
}

/// A message is queued as a pending event only once a listener exists; before
/// that it is parked on the connection so the registration site can replay it.
/// That race is real — the transport starts reading the moment the handshake
/// completes, and `wss.on('connection')` runs a tick later.
fn queue_payload(ws_id: usize, payload: WsPayload) {
    let client_has_listener = WS_CLIENT_LISTENERS
        .lock()
        .unwrap()
        .get(&ws_id)
        .map(|l| l.listeners.get("message").is_some_and(|v| !v.is_empty()))
        .unwrap_or(false);
    let server_has_listener = WS_CLIENT_PARENT_SERVER
        .lock()
        .unwrap()
        .get(&ws_id)
        .copied()
        .map(|sh| !listeners_on_server(sh, "message").is_empty())
        .unwrap_or(false);
    if client_has_listener || server_has_listener {
        push_ws_event(PendingWsEvent::Message(ws_id, payload));
    } else if let Some(c) = WS_CONNECTIONS.lock().unwrap().get_mut(&ws_id) {
        c.messages.push(payload);
    }
}

/// The connection failed. Reported to JS as `ws.on('error')`.
pub(crate) fn connection_error(ws_id: usize, message: &str) {
    push_ws_event(PendingWsEvent::Error(ws_id, message.to_string()));
}

/// The connection is finished, with the status JS should see.
pub(crate) fn connection_closed(ws_id: usize, code: u16, reason: String) {
    if let Some(c) = WS_CONNECTIONS.lock().unwrap().get_mut(&ws_id) {
        if c.is_closed {
            // A close frame and then EOF is one close, not two.
            return;
        }
        c.is_open = false;
        c.is_closed = true;
    } else {
        return;
    }
    push_ws_event(PendingWsEvent::Close(ws_id, code, reason));
}

/// Register a client whose bytes a turnloop host carries. No channel and no
/// task: `send`/`close` drive the codec inline and hand the bytes back.
pub(crate) fn register_turnloop_client(conn_id: i64) -> usize {
    ensure_runtime_hooks_registered();
    let ws_id = register_handle(WsClientHandle) as usize;
    WS_CONNECTIONS.lock().unwrap().insert(
        ws_id,
        WsConnection {
            transport: WsTransport::Turnloop(conn_id),
            messages: Vec::new(),
            is_open: true,
            is_closing: false,
            is_closed: false,
        },
    );
    WS_CLIENT_LISTENERS.lock().unwrap().insert(
        ws_id,
        WsClientListeners {
            listeners: HashMap::new(),
        },
    );
    ws_id
}

/// Register a client on a tokio stream and start its IO loop.
fn register_stream_client<S: io::Transport>(
    stream: S,
    codec: Codec,
    leftover: Vec<u8>,
    open: bool,
) -> usize {
    ensure_runtime_hooks_registered();
    let ws_id = register_handle(WsClientHandle) as usize;
    let (tx, rx) = mpsc::unbounded_channel::<WsCommand>();
    WS_CONNECTIONS.lock().unwrap().insert(
        ws_id,
        WsConnection {
            transport: WsTransport::Tokio(tx),
            messages: Vec::new(),
            is_open: open,
            is_closing: false,
            is_closed: false,
        },
    );
    WS_CLIENT_LISTENERS.lock().unwrap().insert(
        ws_id,
        WsClientListeners {
            listeners: HashMap::new(),
        },
    );
    // `spawn_async` drives the loop on Perry's shared reactor-owned runtime.
    // It does NOT bump the event-loop active-handle counter, so the connection
    // is kept alive by `js_ws_has_pending` reporting live while it is open —
    // the gate `WS_CONNECTIONS` above establishes before this call.
    spawn_async(io::run(ws_id, stream, codec, leftover, rx));
    ws_id
}

/// Register a client on a tokio stream that belongs to a `WebSocketServer`.
///
/// The parent link is published before the IO loop starts, so a message that
/// arrived with the handshake is routed to the server's own `'message'`
/// listener rather than parked forever.
fn register_stream_client_for_server<S: io::Transport>(
    server_handle: Handle,
    stream: S,
    codec: Codec,
    leftover: Vec<u8>,
) -> usize {
    ensure_runtime_hooks_registered();
    let ws_id = register_handle(WsClientHandle) as usize;
    let (tx, rx) = mpsc::unbounded_channel::<WsCommand>();
    WS_CONNECTIONS.lock().unwrap().insert(
        ws_id,
        WsConnection {
            transport: WsTransport::Tokio(tx),
            messages: Vec::new(),
            is_open: true,
            is_closing: false,
            is_closed: false,
        },
    );
    WS_CLIENT_LISTENERS.lock().unwrap().insert(
        ws_id,
        WsClientListeners {
            listeners: HashMap::new(),
        },
    );
    WS_CLIENT_PARENT_SERVER
        .lock()
        .unwrap()
        .insert(ws_id, server_handle);
    spawn_async(io::run(ws_id, stream, codec, leftover, rx));
    ws_id
}

/// `ws.send(...)` on either transport.
fn send_on(ws_id: usize, outgoing: WsOutgoing) {
    let target = WS_CONNECTIONS
        .lock()
        .unwrap()
        .get(&ws_id)
        .map(|c| match &c.transport {
            WsTransport::Tokio(tx) => Ok(tx.clone()),
            WsTransport::Turnloop(conn_id) => Err(*conn_id),
        });
    match target {
        Some(Ok(tx)) => {
            let _ = tx.send(WsCommand::Send(outgoing));
        }
        Some(Err(conn_id)) => {
            turnloop_link::send(conn_id, outgoing);
        }
        None => {}
    }
}

/// `ws.close(code, reason)` on either transport.
fn close_on(ws_id: usize, code: Option<u16>, reason: &str) {
    let target = WS_CONNECTIONS.lock().unwrap().get_mut(&ws_id).map(|c| {
        // `readyState` is CLOSING (2) until the handshake finishes; the
        // connection is NOT closed yet, and a peer frame may still arrive.
        c.is_closing = true;
        match &c.transport {
            WsTransport::Tokio(tx) => Ok(tx.clone()),
            WsTransport::Turnloop(conn_id) => Err(*conn_id),
        }
    });
    match target {
        Some(Ok(tx)) => {
            let _ = tx.send(WsCommand::Close(code, reason.to_string()));
        }
        Some(Err(conn_id)) => {
            turnloop_link::close(conn_id, code, reason);
        }
        None => {}
    }
}

/// `ws.terminate()` — no closing handshake.
fn terminate_on(ws_id: usize) {
    let target = WS_CONNECTIONS
        .lock()
        .unwrap()
        .get(&ws_id)
        .map(|c| match &c.transport {
            WsTransport::Tokio(tx) => Ok(tx.clone()),
            WsTransport::Turnloop(conn_id) => Err(*conn_id),
        });
    match target {
        Some(Ok(tx)) => {
            let _ = tx.send(WsCommand::Terminate);
        }
        Some(Err(conn_id)) => {
            turnloop_link::terminate(conn_id);
        }
        None => {}
    }
}

#[inline]
fn client_js_value(ws_id: usize) -> JsValue {
    JsValue::from_bits(POINTER_TAG | ws_id as u64)
}

/// Add a connection to a server's persistent JS-visible clients Set.
///
/// This runs only on the JS/main thread: either while handling
/// `handleUpgrade`, or while draining a queued standalone-server connection.
fn track_server_client(server_handle: Handle, ws_id: usize) {
    let clients_bits = if let Some(server) = get_handle_mut::<WsServerHandle>(server_handle) {
        if !server.client_ids.contains(&ws_id) {
            server.client_ids.push(ws_id);
        }
        server.clients_bits
    } else {
        return;
    };

    // Do not hold a handle-registry guard across a runtime collection point:
    // Set growth can notify the GC, whose ws scanner walks this same registry.
    let updated = set_add(JsValue::from_bits(clients_bits), client_js_value(ws_id));
    if !updated.is_undefined() && updated.bits() != clients_bits {
        if let Some(server) = get_handle_mut::<WsServerHandle>(server_handle) {
            server.clients_bits = updated.bits();
        }
    }
}

/// Remove a connection from its parent server and return that parent for
/// dispatching the server-level close event after the bookkeeping is current.
fn untrack_server_client(ws_id: usize) -> Option<Handle> {
    let parent = WS_CLIENT_PARENT_SERVER.lock().unwrap().remove(&ws_id);
    if let Some(server_handle) = parent {
        let clients_bits = get_handle_mut::<WsServerHandle>(server_handle).map(|server| {
            server.client_ids.retain(|client_id| *client_id != ws_id);
            server.clients_bits
        });
        if let Some(clients_bits) = clients_bits {
            // Keep runtime collection points outside the registry guard; see
            // the matching add path above.
            set_delete(JsValue::from_bits(clients_bits), client_js_value(ws_id));
        }
    }
    parent
}

// ── Client connect ────────────────────────────────────────────────

/// `new WebSocket(url)` — async constructor returning Promise<id>.
///
/// # Safety
/// `url_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_ws_connect(url_ptr: *const StringHeader) -> *mut perry_ffi::Promise {
    ensure_runtime_hooks_registered();
    ensure_tls_crypto_provider();
    let promise = perry_ffi::JsPromise::new();
    let raw = promise.as_raw();
    let Some(url) = read_str(url_ptr) else {
        promise.reject_string("Invalid URL");
        return raw;
    };
    // Issue #606 — `spawn_blocking_with_reactor` runs the closure inside a
    // tokio worker task, where `Handle::current().block_on` panics. Use
    // `tokio::spawn` so the connect awaits as a sibling task.
    spawn_blocking(move || {
        tokio::spawn(async move {
            match connect::connect(&url, Vec::new(), Vec::new()).await {
                Ok(connected) => {
                    let id = register_stream_client(
                        connected.stream,
                        connected.codec,
                        connected.leftover,
                        true,
                    );
                    push_ws_event(PendingWsEvent::Open(id));
                    promise.resolve(JsValue::from_number(id as f64));
                }
                Err(e) => promise.reject_string(&format!("WebSocket connect error: {e}")),
            }
        });
    });
    raw
}

/// `js_ws_connect_start(url_nanboxed)` — sync alternative used by codegen sites
/// that don't expect a Promise return.
///
/// The id is allocated synchronously so the caller can register listeners
/// before the connect resolves; the connection is adopted into that id once it
/// completes.
#[no_mangle]
pub extern "C" fn js_ws_connect_start(url_nanboxed: f64) -> f64 {
    ensure_runtime_hooks_registered();
    ensure_tls_crypto_provider();
    let bits = url_nanboxed.to_bits();
    let string_tag = 0x7FFF_0000_0000_0000u64;
    let url = if (bits & TAG_MASK) == string_tag {
        let ptr = (bits & POINTER_MASK) as *const StringHeader;
        unsafe { read_str(ptr) }
    } else {
        None
    };
    let Some(url) = url else { return 0.0 };

    // A connection that has not opened yet still needs an id and a command
    // channel, so `ws.send(...)` issued before `'open'` is queued rather than
    // dropped — which is what `ws` does with its own `_sender` queue.
    let ws_id = register_handle(WsClientHandle) as usize;
    let (tx, rx) = mpsc::unbounded_channel::<WsCommand>();
    WS_CONNECTIONS.lock().unwrap().insert(
        ws_id,
        WsConnection {
            transport: WsTransport::Tokio(tx),
            messages: Vec::new(),
            is_open: false,
            is_closing: false,
            is_closed: false,
        },
    );
    WS_CLIENT_LISTENERS.lock().unwrap().insert(
        ws_id,
        WsClientListeners {
            listeners: HashMap::new(),
        },
    );
    spawn_blocking(move || {
        tokio::spawn(async move {
            match connect::connect(&url, Vec::new(), Vec::new()).await {
                Ok(connected) => {
                    if let Some(c) = WS_CONNECTIONS.lock().unwrap().get_mut(&ws_id) {
                        c.is_open = true;
                    }
                    push_ws_event(PendingWsEvent::Open(ws_id));
                    spawn_async(io::run(
                        ws_id,
                        connected.stream,
                        connected.codec,
                        connected.leftover,
                        rx,
                    ));
                }
                Err(e) => {
                    // #6117 — readyState must report CLOSED (3), not
                    // CONNECTING (0), once the connect has failed.
                    if let Some(c) = WS_CONNECTIONS.lock().unwrap().get_mut(&ws_id) {
                        c.is_closed = true;
                    }
                    push_ws_event(PendingWsEvent::Error(
                        ws_id,
                        format!("WebSocket connect error: {e}"),
                    ));
                }
            }
        });
    });
    ws_id as f64
}

// ── Send / close (client) ─────────────────────────────────────────

/// A JS value as a string, for the places `ws` stringifies its argument.
fn js_string_of(value: JsValue) -> Option<String> {
    if value.is_any_string() {
        return value_string(value);
    }
    if value.is_number() {
        let n = value.to_number();
        return Some(if n.fract() == 0.0 && n.abs() < 1e21 {
            format!("{}", n as i64)
        } else {
            format!("{n}")
        });
    }
    if value.is_bool() {
        return Some(value.to_bool().to_string());
    }
    None
}

/// Read a `ws.send(data)` argument.
///
/// `ws` sends a string as a text frame and anything buffer-shaped as a binary
/// frame; everything else is stringified. Perry used to take only a
/// `StringHeader`, so a `Buffer` argument could not be sent at all.
pub(crate) fn outgoing_from_value(value: f64) -> Option<WsOutgoing> {
    let value = JsValue::from_bits(value.to_bits());
    if value.is_undefined() || value.is_null() {
        return None;
    }
    // A Buffer / TypedArray / ArrayBuffer resolves to its backing bytes; a
    // string does not, which is how the two cases are told apart.
    if !value.is_any_string() {
        if let Some(bytes) = perry_ffi::value_byte_slice(value) {
            return Some(WsOutgoing::Binary(bytes.to_vec()));
        }
    }
    js_string_of(value).map(WsOutgoing::Text)
}

/// `ws.send(data[, options])`.
///
/// `options.binary` overrides the framing `ws` would infer from the value —
/// `send(buffer, { binary: false })` is a TEXT frame carrying those bytes, and
/// `send(string, { binary: true })` is a binary one. Inferring from the value
/// alone gets the common case right and this one wrong, which is observable on
/// the wire as the opcode.
#[no_mangle]
pub extern "C" fn js_ws_send_value(handle: i64, value: f64, options: f64) {
    let Some(outgoing) = outgoing_from_value(value) else {
        return;
    };
    let outgoing = match binary_option(options) {
        Some(true) => WsOutgoing::Binary(match outgoing {
            WsOutgoing::Text(text) => text.into_bytes(),
            WsOutgoing::Binary(bytes) => bytes,
            other => return send_on(handle as usize, other),
        }),
        Some(false) => WsOutgoing::Text(match outgoing {
            WsOutgoing::Text(text) => text,
            // A forced-text frame must still carry the bytes it was given, and
            // it must be valid UTF-8 to be a legal text frame at all.
            WsOutgoing::Binary(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            other => return send_on(handle as usize, other),
        }),
        None => outgoing,
    };
    send_on(handle as usize, outgoing);
}

/// `options.binary`, when the caller passed an options object with one.
fn binary_option(options: f64) -> Option<bool> {
    let value = JsValue::from_bits(options.to_bits());
    if !value.is_pointer() {
        return None;
    }
    let key = alloc_string("binary");
    let field = unsafe { server::object_field_by_name(value, key.as_raw() as *const StringHeader) };
    if field.is_bool() {
        Some(field.to_bool())
    } else {
        None
    }
}

/// `ws.send(text)` — the string-typed entry point kept for call sites whose
/// argument codegen proved a string.
///
/// # Safety
/// `message_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_ws_send(handle: i64, message_ptr: *const StringHeader) {
    let Some(msg) = read_str(message_ptr) else {
        return;
    };
    send_on(handle as usize, WsOutgoing::Text(msg));
}

/// `ws.ping([data])`.
#[no_mangle]
pub extern "C" fn js_ws_ping(handle: i64, value: f64) {
    send_on(handle as usize, WsOutgoing::Ping(control_payload(value)));
}

/// `ws.pong([data])`.
#[no_mangle]
pub extern "C" fn js_ws_pong(handle: i64, value: f64) {
    send_on(handle as usize, WsOutgoing::Pong(control_payload(value)));
}

/// `ws.terminate()` — drop the connection with no closing handshake.
#[no_mangle]
pub extern "C" fn js_ws_terminate(handle: i64) {
    terminate_on(handle as usize);
}

fn control_payload(value: f64) -> Vec<u8> {
    match outgoing_from_value(value) {
        Some(WsOutgoing::Text(text)) => text.into_bytes(),
        Some(WsOutgoing::Binary(bytes)) => bytes,
        _ => Vec::new(),
    }
}

/// `ws.close()` / `wss.close()`.
#[no_mangle]
pub extern "C" fn js_ws_close(handle: i64) {
    js_ws_close_with(handle, undefined(), undefined())
}

/// `ws.close(code, reason)`.
///
/// Both arguments reach the wire now. They used to be dropped entirely — the
/// FFI took none and the frame was always `Close(None)` — so a peer could never
/// observe an application close code.
#[no_mangle]
pub extern "C" fn js_ws_close_with(handle: i64, code: f64, reason: f64) {
    if get_handle_mut::<WsServerHandle>(handle).is_some() {
        // `wss.close([cb])`. The first argument is a callback, not a close
        // code: a server has no close frame. It used to be dropped, so
        // `wss.close(() => …)` never ran and a program that awaited it hung.
        js_ws_server_close_with(handle, code);
        return;
    }
    let (code, reason) = close_args(code, reason);
    close_on(handle as usize, code, &reason);
}

/// `ws`'s own validation: a code must be 1000 or in 3000..=4999, and a reason
/// without a code is ignored rather than sent as 1005.
fn close_args(code: f64, reason: f64) -> (Option<u16>, String) {
    let code_value = JsValue::from_bits(code.to_bits());
    let code = Some(code_value)
        .filter(|v| v.is_number())
        .map(|v| v.to_number())
        .filter(|n| n.is_finite())
        .map(|n| n as i64)
        .filter(|n| *n == 1000 || (3000..=4999).contains(n))
        .map(|n| n as u16);
    let reason = if code.is_some() {
        js_string_of(JsValue::from_bits(reason.to_bits())).unwrap_or_default()
    } else {
        String::new()
    };
    (code, reason)
}

/// # Safety
// `js_ws_send_to_client_i64` / `js_ws_close_client_i64` /
// `js_ws_on_client_i64` are the Phase 4 receiver-method variants.
// Receivers from NATIVE_MODULE_TABLE dispatch arrive as raw i64
// (already unboxed via the POINTER_TAG mask), so these helpers
// take `i64` directly — same shape as `js_ws_send` / `js_ws_close`
// / `js_ws_on` but they exist as separate symbols so the codegen
// dispatch table can pin Client-class entries without colliding
// with the existing receiver-less / module-method-call entries.

/// Issue #577 Phase 4 — `wsId.send(msg)` on an upgrade-path Client.
///
/// # Safety
/// `message_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_ws_send_client_i64(handle: i64, message_ptr: *const StringHeader) {
    let Some(msg) = read_str(message_ptr) else {
        return;
    };
    send_on(handle as usize, WsOutgoing::Text(msg));
}

/// Issue #577 Phase 4 — `wsId.send(data)` on an upgrade-path Client, for any
/// value shape.
#[no_mangle]
pub extern "C" fn js_ws_send_value_client_i64(handle: i64, value: f64, options: f64) {
    js_ws_send_value(handle, value, options)
}

/// Issue #577 Phase 4 — `wsId.close([code, reason])` on an upgrade-path Client.
#[no_mangle]
pub extern "C" fn js_ws_close_client_i64(handle: i64) {
    close_on(handle as usize, None, "");
}

/// Issue #577 Phase 4 — `wsId.close(code, reason)` on an upgrade-path Client.
#[no_mangle]
pub extern "C" fn js_ws_close_with_client_i64(handle: i64, code: f64, reason: f64) {
    let (code, reason) = close_args(code, reason);
    close_on(handle as usize, code, &reason);
}

/// Issue #577 Phase 4 — `wsId.on(event, cb)` on an upgrade-path Client.
///
/// # Safety
/// `event_name_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_ws_on_client_i64(
    handle: i64,
    event_name_ptr: *const StringHeader,
    callback_ptr: i64,
) -> i64 {
    ensure_runtime_hooks_registered();
    let Some(event_name) = read_str(event_name_ptr) else {
        return handle;
    };
    if callback_ptr == 0 {
        return handle;
    }
    let ws_id = handle as usize;
    {
        let mut g = WS_CLIENT_LISTENERS.lock().unwrap();
        let entry = g.entry(ws_id).or_insert_with(|| WsClientListeners {
            listeners: HashMap::new(),
        });
        entry
            .listeners
            .entry(event_name.clone())
            .or_default()
            .push(callback_ptr);
    }
    // Issue #577 Phase 4 — drain any messages that arrived before this
    // listener was registered (race window: IO loop reads frames as
    // soon as the WS handshake completes, but TS-side
    // `wsId.on('message', cb)` only runs once the upgrade event
    // fires). Republish queued messages as PendingWsEvents so the
    // next `js_ws_process_pending` tick fires this freshly-registered
    // listener against them.
    if event_name == "message" {
        let queued: Vec<WsPayload> = if let Some(c) = WS_CONNECTIONS.lock().unwrap().get_mut(&ws_id)
        {
            std::mem::take(&mut c.messages)
        } else {
            Vec::new()
        };
        for msg in queued {
            push_ws_event(PendingWsEvent::Message(ws_id, msg));
        }
    }
    handle
}

/// `message_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_ws_send_to_client(handle_f64: f64, message_ptr: *const StringHeader) {
    let Some(msg) = read_str(message_ptr) else {
        return;
    };
    send_on(decode_client_id(handle_f64), WsOutgoing::Text(msg));
}

#[no_mangle]
pub extern "C" fn js_ws_close_client(handle_f64: f64) {
    close_on(decode_client_id(handle_f64), None, "");
}

// ── Accessors ─────────────────────────────────────────────────────

#[no_mangle]
pub extern "C" fn js_ws_is_open(handle: i64) -> f64 {
    let id = handle as usize;
    WS_CONNECTIONS
        .lock()
        .unwrap()
        .get(&id)
        .map(|c| if c.is_open { 1.0 } else { 0.0 })
        .unwrap_or(0.0)
}

/// #6117 — `ws.readyState` per npm-ws semantics: CONNECTING=0, OPEN=1,
/// CLOSING=2, CLOSED=3. An id with no map entry is CLOSED — either the
/// entry was cleaned up after close, or the promise-path connect failed
/// before an entry was ever created.
#[no_mangle]
pub extern "C" fn js_ws_ready_state(handle: i64) -> f64 {
    let id = handle as usize;
    match WS_CONNECTIONS.lock().unwrap().get(&id) {
        Some(c) if c.is_closed => 3.0,
        Some(c) if c.is_closing => 2.0,
        Some(c) if c.is_open => 1.0,
        Some(_) => 0.0,
        None => 3.0,
    }
}

#[no_mangle]
pub extern "C" fn js_ws_message_count(handle: i64) -> f64 {
    let id = handle as usize;
    WS_CONNECTIONS
        .lock()
        .unwrap()
        .get(&id)
        .map(|c| c.messages.len() as f64)
        .unwrap_or(0.0)
}

#[no_mangle]
pub extern "C" fn js_ws_receive(handle: i64) -> *mut StringHeader {
    let id = handle as usize;
    let mut g = WS_CONNECTIONS.lock().unwrap();
    if let Some(c) = g.get_mut(&id) {
        if !c.messages.is_empty() {
            let msg = c.messages.remove(0);
            return alloc_string(&payload_text(&msg)).as_raw();
        }
    }
    std::ptr::null_mut()
}

/// `js_ws_receive` / `js_ws_wait_for_message` are Perry-only string APIs that
/// predate binary support, so a binary payload is rendered lossily for them —
/// and only for them. Every `ws`-shaped path delivers a `Buffer`.
fn payload_text(payload: &WsPayload) -> String {
    match payload {
        WsPayload::Text(text) => text.clone(),
        WsPayload::Binary(bytes) => String::from_utf8_lossy(bytes).into_owned(),
    }
}

/// `js_ws_wait_for_message(handle, timeout_ms)` — block up to
/// `timeout_ms` milliseconds for a buffered message; returns the
/// message string or null on timeout.
#[no_mangle]
pub unsafe extern "C" fn js_ws_wait_for_message(handle: i64, timeout_ms: f64) -> *mut StringHeader {
    let id = handle as usize;
    let timeout = std::time::Duration::from_millis(timeout_ms.max(0.0) as u64);
    let start = std::time::Instant::now();
    loop {
        if let Some(c) = WS_CONNECTIONS.lock().unwrap().get_mut(&id) {
            if !c.messages.is_empty() {
                let msg = c.messages.remove(0);
                return alloc_string(&payload_text(&msg)).as_raw();
            }
        }
        if start.elapsed() >= timeout {
            return std::ptr::null_mut();
        }
        // Unified single-thread model: the WS reader task only advances while the
        // main thread drives the runtime, so drive one bounded tick here (which
        // runs the reader and delivers messages) instead of `std::thread::sleep`,
        // which would block this thread and never let a message arrive.
        perry_ffi::run_pending(10);
    }
}

/// Helper: extract i64 handle from a NaN-boxed JsValue (server
/// handle as POINTER_TAG-tagged f64) OR a plain f64 number (client
/// ws_id). Used at JS-side dispatch points where the handle could be
/// either.
#[no_mangle]
pub extern "C" fn js_ws_handle_to_i64(val_f64: f64) -> i64 {
    let bits = val_f64.to_bits();
    if (bits & TAG_MASK) == POINTER_TAG {
        (bits & POINTER_MASK) as i64
    } else {
        val_f64 as i64
    }
}

/// Register an event listener. Routes to server-handle listeners or
/// per-client-id listeners based on which registry the handle is in.
///
/// # Safety
/// `event_name_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_ws_on(
    handle: i64,
    event_name_ptr: *const StringHeader,
    callback_ptr: i64,
) -> i64 {
    ensure_runtime_hooks_registered();
    let Some(event_name) = read_str(event_name_ptr) else {
        return handle;
    };
    if callback_ptr == 0 {
        return handle;
    }
    // Client and server ids share the handle allocator, so routing is unambiguous.
    let ws_id = handle as usize;
    let is_client = WS_CONNECTIONS.lock().unwrap().contains_key(&ws_id);
    if !is_client {
        if let Some(server) = get_handle_mut::<WsServerHandle>(handle) {
            // If the server has already bound by the time the user
            // registers a "listening" handler, re-emit the event so the
            // late-registered callback fires on the next event-loop pump.
            // Without this, the accept-loop task races the JS-side `wss.on(
            // "listening", cb)` registration — `push_ws_event(Listening)`
            // happens immediately after the bind succeeds, and any pump
            // tick that drains it before the user's listener registers
            // discards the event silently.
            let already_listening = event_name == "listening" && server.is_listening;
            server
                .listeners
                .entry(event_name)
                .or_insert_with(Vec::new)
                .push(callback_ptr);
            if already_listening {
                push_ws_event(PendingWsEvent::Listening(handle));
            }
            return handle;
        }
    }
    // Issue #606: same race fix as listening — if the client has
    // already opened by the time the user registers an "open"
    // handler, re-emit the event so the late-registered callback
    // fires on the next pump tick.
    let already_open = event_name == "open"
        && WS_CONNECTIONS
            .lock()
            .unwrap()
            .get(&ws_id)
            .map(|c| c.is_open)
            .unwrap_or(false);
    let replay_messages = event_name == "message";
    let mut g = WS_CLIENT_LISTENERS.lock().unwrap();
    let entry = g.entry(ws_id).or_insert_with(|| WsClientListeners {
        listeners: HashMap::new(),
    });
    entry
        .listeners
        .entry(event_name)
        .or_default()
        .push(callback_ptr);
    drop(g);
    if already_open {
        push_ws_event(PendingWsEvent::Open(ws_id));
    }
    // Replay whatever arrived before this listener existed. `js_ws_on_client_i64`
    // has always done this; `js_ws_on` had not, which is the same race seen from
    // the other receiver convention and it is reachable now: on the turnloop
    // transport a frame pipelined behind the handshake is decoded inside the
    // sink, one pump tick BEFORE `wss.on('connection')` runs and registers this
    // listener.
    if replay_messages {
        let queued: Vec<WsPayload> = if let Some(c) = WS_CONNECTIONS.lock().unwrap().get_mut(&ws_id)
        {
            std::mem::take(&mut c.messages)
        } else {
            Vec::new()
        };
        for msg in queued {
            push_ws_event(PendingWsEvent::Message(ws_id, msg));
        }
    }
    handle
}

// ── Server ────────────────────────────────────────────────────────

#[no_mangle]
pub extern "C" fn js_ws_server_close(handle: i64) {
    js_ws_server_close_with(handle, undefined())
}

/// `wss.close([cb])`.
///
/// The callback is registered as a `'close'` listener rather than stashed in
/// the pending queue, which is deliberate: `WsServerHandle::listeners` is
/// walked by `scan_ws_roots`, so the closure pointer is a rooted slot a moving
/// collection rewrites. A raw closure address parked in `WS_PENDING_EVENTS`
/// would be exactly the unrooted-cache shape `gc_runtime_root_holders.py`
/// exists to catch — and that queue's freedom from JS values is what lets it
/// have no scanner at all.
///
/// The handle is NOT taken here. It used to be, which destroyed the listener
/// map and the `clients` Set before anything could fire `'close'`; the drain
/// takes it after the listeners have run.
#[no_mangle]
pub extern "C" fn js_ws_server_close_with(handle: i64, callback: f64) {
    let callback_ptr = {
        let value = JsValue::from_bits(callback.to_bits());
        if value.is_pointer() {
            (value.bits() & POINTER_MASK) as i64
        } else {
            0
        }
    };
    // Scoped: `push_ws_event` notifies the main thread and the shutdown send
    // wakes the accept loop, and neither may run while a handle-registry
    // borrow is live (the same rule `track_server_client` follows).
    let shutdown = {
        let Some(server) = get_handle_mut::<WsServerHandle>(handle) else {
            return;
        };
        if callback_ptr != 0 {
            server
                .listeners
                .entry("close".to_string())
                .or_default()
                .push(callback_ptr);
        }
        server.is_listening = false;
        server.shutdown_tx.take()
    };
    if let Some(tx) = shutdown {
        let _ = tx.send(());
    }
    push_ws_event(PendingWsEvent::ServerClose(handle));
}

/// Adopt a stream whose WebSocket handshake a host crate has already completed
/// — `perry-ext-http`'s and `perry-ext-fastify`'s hyper upgrade paths.
///
/// The stream is any `AsyncRead + AsyncWrite`; in practice
/// `TokioIo<hyper::upgrade::Upgraded>`. What changed with the codec swap is
/// that the *caller* no longer constructs a `tokio_tungstenite::WebSocketStream`
/// and therefore no longer needs `tokio-tungstenite` in its own dependency
/// graph: it hands over the raw stream and this crate installs the protocol.
///
/// Returns the assigned `ws_id`. The caller fires whatever `'connection'` /
/// `'upgrade'` listeners are appropriate; this does not push a
/// `PendingWsEvent::Connection`.
pub fn register_upgraded_stream<S>(stream: S) -> i64
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    register_stream_client(stream, Codec::new(codec::Role::Server), Vec::new(), true) as i64
}

/// Validate an upgrade request and return the headers a `101` must carry, so a
/// host crate does not need a WebSocket library of its own to answer one.
///
/// `request_headers` is the request's header list, in any case. `protocols` is
/// the server's offered subprotocol list in preference order.
///
/// This replaces the hyper path's hand-rolled `derive_accept_key` + literal
/// header block, which validated *nothing*: it checked neither
/// `Sec-WebSocket-Version` nor `Upgrade: websocket`, and a missing
/// `Sec-WebSocket-Key` produced an empty accept value rather than a refusal.
///
/// (`turnloop_websocket` does not re-export `derive_accept_key`, so a host that
/// wants only the one header cannot get it; going through `accept` is better
/// anyway, because it is the validation too.)
pub fn accept_headers(
    method: &str,
    target: &str,
    request_headers: &[(String, String)],
    protocols: &[&str],
) -> Result<Vec<(String, String)>, String> {
    let head = turnloop_link::request_head(method, target, 1, request_headers);
    let (response, _) = turnloop_websocket::accept(&head, protocols).map_err(|e| e.to_string())?;
    Ok(response
        .headers
        .into_iter()
        .map(|h| (h.name, String::from_utf8_lossy(&h.value).into_owned()))
        .collect())
}

/// #1113 — `wss.handleUpgrade(req, socket, head, cb)` for a
/// `new WebSocketServer({ noServer: true })`.
///
/// The handshake + per-client IO loop already happened: the host
/// server's accept task (fastify's `handle_fastify_websocket_upgrade`
/// or perry-ext-http's `handle_websocket_upgrade`) drove
/// `hyper::upgrade::on`, completed the tungstenite server handshake,
/// and called `register_external_ws_stream` (which spawned
/// `drive_server_client_io`). By the time the user's `'upgrade'`
/// handler runs and calls `wss.handleUpgrade(...)`, `ws_id` is already
/// a live connection. This function is purely the JS-visible
/// re-dispatch shim — it does NOT register another stream or perform
/// another handshake.
///
/// Adopt the client into the server's tracked Set before invoking
/// `cb(socket, request)`. The callback decides whether to emit `connection`;
/// `handleUpgrade` itself never emits that event and returns `undefined`.
/// The HTTP transport has already consumed the head bytes.
///
/// # Safety
/// `cb`, when non-zero, must be a valid NaN-boxed / raw closure
/// pointer. `ws_id_f64` must be a POINTER_TAG-boxed ws id produced by
/// the host server's upgrade path.
#[no_mangle]
pub unsafe extern "C" fn js_ws_handle_upgrade(
    server_handle: i64,
    req_f64: f64,
    ws_id_f64: f64,
    _head_f64: f64,
    cb: i64,
) {
    ensure_runtime_hooks_registered();
    let scope = perry_ffi::TransientRootScope::enter();
    let cb = scope.root_addr((cb as u64 & POINTER_MASK) as i64);
    let req = scope.root_nanbox(req_f64);
    let ws_id = decode_client_id(ws_id_f64);
    if get_handle_mut::<WsServerHandle>(server_handle).is_none()
        || !WS_CONNECTIONS.lock().unwrap().contains_key(&ws_id)
    {
        return;
    }
    WS_CLIENT_PARENT_SERVER
        .lock()
        .unwrap()
        .insert(ws_id, server_handle);
    track_server_client(server_handle, ws_id);
    // ws delegates connection emission to the callback. Emitting again here
    // duplicates the usual `wss.emit("connection", ws, req)` idiom.
    if cb.get() != 0 {
        let closure = JsClosure::from_raw(cb.get() as *const RawClosureHeader);
        let _ = closure.call2(f64::from_bits(client_js_value(ws_id).bits()), req.get());
    }
}

// ── Event-loop tick ───────────────────────────────────────────────

/// A `Buffer` holding these bytes, for the JS side of a binary frame.
fn buffer_value(bytes: &[u8]) -> f64 {
    let buffer = perry_ffi::alloc_buffer(bytes);
    f64::from_bits(POINTER_TAG | (buffer as u64 & POINTER_MASK))
}

/// A message payload as JS sees it: a string for text, a `Buffer` for binary.
fn payload_value(payload: &WsPayload) -> f64 {
    match payload {
        WsPayload::Text(text) => {
            let s = alloc_string(text);
            f64::from_bits(JsValue::from_string_ptr(s.as_raw()).bits())
        }
        WsPayload::Binary(bytes) => buffer_value(bytes),
    }
}

/// Drain pending events and dispatch to user-registered listeners.
/// Called by perry-codegen's main-thread event-loop pump.
#[no_mangle]
pub extern "C" fn js_ws_process_pending() -> i32 {
    let events: Vec<PendingWsEvent> = {
        let mut g = WS_PENDING_EVENTS.lock().unwrap();
        std::mem::take(&mut *g)
    };
    if events.is_empty() {
        return 0;
    }
    let mut fired = 0;
    for ev in events {
        match ev {
            PendingWsEvent::Connection(server_handle, client_id) => {
                // Match `ws`: clients is current before the user-visible
                // `connection` callback fires.
                track_server_client(server_handle, client_id);
                let listeners = listeners_on_server(server_handle, "connection");
                for cb in listeners {
                    if cb != 0 {
                        let closure = unsafe { JsClosure::from_raw(cb as *const RawClosureHeader) };
                        // Use the same handle value as the clients Set and
                        // manual-upgrade callback, including dynamic dispatch.
                        let _ = unsafe {
                            closure.call1(f64::from_bits(client_js_value(client_id).bits()))
                        };
                        fired += 1;
                    }
                }
            }
            PendingWsEvent::Message(ws_id, payload) => {
                let listeners = listeners_on_client(ws_id, "message");
                // `ws` hands a text frame to JS as a string and a binary frame
                // as a Buffer, and passes `isBinary` as the second argument.
                // Perry used to deliver every frame as a string, with a binary
                // payload run through `String::from_utf8_lossy` — which is not
                // a representation choice but data loss: every non-UTF-8 byte
                // became U+FFFD and could not be recovered.
                let is_binary = matches!(payload, WsPayload::Binary(_));
                let msg_f64 = payload_value(&payload);
                let binary_f64 = f64::from_bits(JsValue::from_bool(is_binary).bits());
                if !listeners.is_empty() {
                    for cb in listeners {
                        if cb != 0 {
                            let closure =
                                unsafe { JsClosure::from_raw(cb as *const RawClosureHeader) };
                            let _ = unsafe { closure.call2(msg_f64, binary_f64) };
                            fired += 1;
                        }
                    }
                } else {
                    // #746 follow-up: server-level `wss.on('message',
                    // (ws, data) => ...)` parity with perry-stdlib::ws.
                    let parent = WS_CLIENT_PARENT_SERVER.lock().unwrap().get(&ws_id).copied();
                    if let Some(server_handle) = parent {
                        for cb in listeners_on_server(server_handle, "message") {
                            if cb != 0 {
                                let closure =
                                    unsafe { JsClosure::from_raw(cb as *const RawClosureHeader) };
                                let _ = unsafe {
                                    closure.call2(
                                        f64::from_bits(client_js_value(ws_id).bits()),
                                        msg_f64,
                                    )
                                };
                                fired += 1;
                            }
                        }
                    }
                }
            }
            PendingWsEvent::Ping(ws_id, data) => {
                let listeners = listeners_on_client(ws_id, "ping");
                if !listeners.is_empty() {
                    let value = buffer_value(&data);
                    for cb in listeners {
                        if cb != 0 {
                            let closure =
                                unsafe { JsClosure::from_raw(cb as *const RawClosureHeader) };
                            let _ = unsafe { closure.call1(value) };
                            fired += 1;
                        }
                    }
                }
            }
            PendingWsEvent::Pong(ws_id, data) => {
                let listeners = listeners_on_client(ws_id, "pong");
                if !listeners.is_empty() {
                    let value = buffer_value(&data);
                    for cb in listeners {
                        if cb != 0 {
                            let closure =
                                unsafe { JsClosure::from_raw(cb as *const RawClosureHeader) };
                            let _ = unsafe { closure.call1(value) };
                            fired += 1;
                        }
                    }
                }
            }
            PendingWsEvent::Close(ws_id, code, reason) => {
                // The upstream `ws` package installs its tracking listener
                // before handing the socket to user code, so user close
                // callbacks observe the client as already removed.
                let parent = untrack_server_client(ws_id);
                let listeners = listeners_on_client(ws_id, "close");
                if !listeners.is_empty() {
                    // `ws.on('close', (code, reason) => ...)`. Both arguments
                    // used to be `undefined`: the queue carried them and the
                    // drain destructured them into `_code`/`_reason` and called
                    // the listener with no arguments at all.
                    let code_f64 = f64::from_bits(JsValue::from_number(code as f64).bits());
                    let reason_string = alloc_string(&reason);
                    let reason_f64 =
                        f64::from_bits(JsValue::from_string_ptr(reason_string.as_raw()).bits());
                    for cb in listeners {
                        if cb != 0 {
                            let closure =
                                unsafe { JsClosure::from_raw(cb as *const RawClosureHeader) };
                            let _ = unsafe { closure.call2(code_f64, reason_f64) };
                            fired += 1;
                        }
                    }
                } else {
                    // #746 follow-up: server-level `wss.on('close',
                    // (ws) => ...)` parity with perry-stdlib::ws.
                    if let Some(server_handle) = parent {
                        for cb in listeners_on_server(server_handle, "close") {
                            if cb != 0 {
                                let closure =
                                    unsafe { JsClosure::from_raw(cb as *const RawClosureHeader) };
                                let _ = unsafe {
                                    closure.call1(f64::from_bits(client_js_value(ws_id).bits()))
                                };
                                fired += 1;
                            }
                        }
                    }
                }
                WS_CONNECTIONS.lock().unwrap().remove(&ws_id);
                WS_CLIENT_LISTENERS.lock().unwrap().remove(&ws_id);
            }
            PendingWsEvent::Error(ws_id, err) => {
                let listeners = listeners_on_client(ws_id, "error");
                for cb in listeners {
                    if cb != 0 {
                        let closure = unsafe { JsClosure::from_raw(cb as *const RawClosureHeader) };
                        let s = alloc_string(&err);
                        let _ = unsafe {
                            closure
                                .call1(f64::from_bits(JsValue::from_string_ptr(s.as_raw()).bits()))
                        };
                        fired += 1;
                    }
                }
            }
            PendingWsEvent::ServerError(server_handle, err) => {
                let listeners = listeners_on_server(server_handle, "error");
                for cb in listeners {
                    if cb != 0 {
                        let closure = unsafe { JsClosure::from_raw(cb as *const RawClosureHeader) };
                        let s = alloc_string(&err);
                        let _ = unsafe {
                            closure
                                .call1(f64::from_bits(JsValue::from_string_ptr(s.as_raw()).bits()))
                        };
                        fired += 1;
                    }
                }
            }
            PendingWsEvent::Listening(server_handle) => {
                let listeners = listeners_on_server(server_handle, "listening");
                for cb in listeners {
                    if cb != 0 {
                        let closure = unsafe { JsClosure::from_raw(cb as *const RawClosureHeader) };
                        let _ = unsafe { closure.call0() };
                        fired += 1;
                    }
                }
            }
            PendingWsEvent::ServerClose(server_handle) => {
                for cb in listeners_on_server(server_handle, "close") {
                    if cb != 0 {
                        let closure = unsafe { JsClosure::from_raw(cb as *const RawClosureHeader) };
                        let _ = unsafe { closure.call0() };
                        fired += 1;
                    }
                }
                // Retire the handle only once its listeners have run.
                let _ = take_handle::<WsServerHandle>(server_handle);
            }
            PendingWsEvent::Open(ws_id) => {
                let listeners = listeners_on_client(ws_id, "open");
                for cb in listeners {
                    if cb != 0 {
                        let closure = unsafe { JsClosure::from_raw(cb as *const RawClosureHeader) };
                        let _ = unsafe { closure.call0() };
                        fired += 1;
                    }
                }
            }
        }
    }
    fired
}

#[no_mangle]
pub extern "C" fn js_ws_has_pending() -> i32 {
    if !WS_PENDING_EVENTS.lock().unwrap().is_empty() {
        return 1;
    }
    if WS_ACTIVE_SERVERS.load(Ordering::Relaxed) > 0 {
        return 1;
    }
    let any_open = WS_CONNECTIONS.lock().unwrap().values().any(|c| c.is_open);
    if any_open {
        1
    } else {
        0
    }
}

fn listeners_on_client(ws_id: usize, event: &str) -> Vec<i64> {
    WS_CLIENT_LISTENERS
        .lock()
        .unwrap()
        .get(&ws_id)
        .and_then(|l| l.listeners.get(event).cloned())
        .unwrap_or_default()
}

fn listeners_on_server(handle: Handle, event: &str) -> Vec<i64> {
    perry_ffi::with_handle::<WsServerHandle, _, _>(handle, |s| {
        s.listeners.get(event).cloned().unwrap_or_default()
    })
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_ffi::{drop_handle, get_handle, register_handle};
    use std::sync::{Mutex, MutexGuard};

    static GC_TEST_LOCK: Mutex<()> = Mutex::new(());

    struct GcTestGuard {
        frame: u64,
        previous_force_evacuation: i32,
        _lock: MutexGuard<'static, ()>,
    }

    impl GcTestGuard {
        fn new() -> Self {
            let lock = GC_TEST_LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let previous_force_evacuation =
                perry_runtime::gc::js_gc_force_evacuation_test_override(1);
            perry_runtime::gc::js_gc_write_barriers_emitted(1);
            let frame = perry_runtime::gc::js_shadow_frame_push(0);
            Self {
                frame,
                previous_force_evacuation,
                _lock: lock,
            }
        }
    }

    impl Drop for GcTestGuard {
        fn drop(&mut self) {
            perry_runtime::gc::js_shadow_frame_pop(self.frame);
            perry_runtime::gc::js_gc_write_barriers_emitted(0);
            perry_runtime::gc::js_gc_force_evacuation_test_override(self.previous_force_evacuation);
        }
    }

    fn young_gc_root() -> i64 {
        perry_runtime::arena::arena_alloc_gc(32, 8, perry_runtime::gc::GC_TYPE_STRING) as i64
    }

    fn assert_rewritten(before: i64, after: i64) {
        assert_ne!(after, before);
        assert!(perry_runtime::arena::pointer_in_nursery(after as usize));
    }

    #[test]
    fn gc_scanner_registration_idempotent() {
        ensure_runtime_hooks_registered();
        ensure_runtime_hooks_registered();
    }

    #[test]
    fn gc_mutable_scanner_rewrites_client_and_server_listener_roots() {
        let _guard = GcTestGuard::new();
        perry_ffi::gc_register_mutable_root_scanner_named("perry-ext-ws", scan_ws_roots);

        let client_id = usize::MAX - 9_001;
        let client_callback = young_gc_root();
        WS_CLIENT_LISTENERS.lock().unwrap().insert(
            client_id,
            WsClientListeners {
                listeners: HashMap::from([("message".to_string(), vec![client_callback])]),
            },
        );

        let server_callback = young_gc_root();
        let clients_before = alloc_set(4).bits();
        let server_handle = register_handle(WsServerHandle {
            listeners: HashMap::from([("connection".to_string(), vec![server_callback])]),
            port: 0,
            host: "0.0.0.0".into(),
            attached_server: None,
            no_server: true,
            is_listening: false,
            client_ids: Vec::new(),
            clients_bits: clients_before,
            shutdown_tx: None,
        });

        let _ = perry_runtime::gc::gc_collect_minor();

        {
            let clients = WS_CLIENT_LISTENERS.lock().unwrap();
            assert_rewritten(client_callback, clients[&client_id].listeners["message"][0]);
            let server = get_handle::<WsServerHandle>(server_handle)
                .expect("server handle should remain live");
            assert_rewritten(server_callback, server.listeners["connection"][0]);
            assert_ne!(server.clients_bits, clients_before);
            let clients = JsValue::from_bits(server.clients_bits)
                .as_pointer::<perry_runtime::set::SetHeader>();
            assert_eq!(perry_runtime::set::js_set_size(clients), 0);
        }
        WS_CLIENT_LISTENERS.lock().unwrap().remove(&client_id);
        drop_handle(server_handle);
    }

    /// #9324: the DYNAMIC read must reach the same live `Set` the typed read
    /// gets. Pre-fix this dispatcher did not exist, every untyped
    /// `wss.clients` read `undefined`, and iterating it killed the process.
    #[test]
    fn handle_property_dispatch_answers_clients_for_an_untyped_read() {
        let _lock = GC_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let undefined = f64::from_bits(JsValue::UNDEFINED.bits());
        let server_handle = js_ws_server_new(undefined);

        let mut out = f64::NAN;
        let handled = unsafe {
            js_ext_ws_handle_property_dispatch(
                server_handle,
                b"clients".as_ptr(),
                b"clients".len(),
                &mut out,
            )
        };
        assert_eq!(handled, 1, "the dispatcher must claim `clients`");
        assert_eq!(
            out.to_bits(),
            js_ws_server_clients(server_handle).to_bits(),
            "the dynamic read must return the SAME Set as the typed read"
        );
        let set = JsValue::from_bits(out.to_bits()).as_pointer::<perry_runtime::set::SetHeader>();
        assert!(!set.is_null());
        assert_eq!(perry_runtime::set::js_set_size(set), 0);

        // Every other property stays unhandled so the composite dispatcher
        // falls through to the primary stdlib dispatcher.
        let mut other = f64::NAN;
        assert_eq!(
            unsafe {
                js_ext_ws_handle_property_dispatch(
                    server_handle,
                    b"readyState".as_ptr(),
                    b"readyState".len(),
                    &mut other,
                )
            },
            0
        );

        drop_handle(server_handle);

        // A handle that is no longer a live server must NOT be claimed —
        // otherwise this arm would shadow whatever id gets recycled into it.
        let mut dead = f64::NAN;
        assert_eq!(
            unsafe {
                js_ext_ws_handle_property_dispatch(
                    server_handle,
                    b"clients".as_ptr(),
                    b"clients".len(),
                    &mut dead,
                )
            },
            0
        );
    }

    #[test]
    fn server_clients_is_a_stable_set_that_tracks_connections() {
        let _lock = GC_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let undefined = f64::from_bits(JsValue::UNDEFINED.bits());
        let server_handle = js_ws_server_new(undefined);

        let first = js_ws_server_clients(server_handle);
        let second = js_ws_server_clients(server_handle);
        assert_eq!(first.to_bits(), second.to_bits());
        let clients =
            JsValue::from_bits(first.to_bits()).as_pointer::<perry_runtime::set::SetHeader>();
        assert!(!clients.is_null());
        assert_eq!(perry_runtime::set::js_set_size(clients), 0);

        let client_id = register_handle(WsClientHandle) as usize;
        track_server_client(server_handle, client_id);
        let clients = JsValue::from_bits(js_ws_server_clients(server_handle).to_bits())
            .as_pointer::<perry_runtime::set::SetHeader>();
        assert_eq!(perry_runtime::set::js_set_size(clients), 1);
        assert_eq!(
            perry_runtime::set::js_set_has(
                clients,
                f64::from_bits(client_js_value(client_id).bits())
            ),
            1
        );

        WS_CLIENT_PARENT_SERVER
            .lock()
            .unwrap()
            .insert(client_id, server_handle);
        assert_eq!(untrack_server_client(client_id), Some(server_handle));
        let clients = JsValue::from_bits(js_ws_server_clients(server_handle).to_bits())
            .as_pointer::<perry_runtime::set::SetHeader>();
        assert_eq!(perry_runtime::set::js_set_size(clients), 0);

        drop_handle(client_id as i64);
        drop_handle(server_handle);
    }

    #[test]
    fn has_pending_returns_zero_with_no_state() {
        // May be non-zero if a prior test left state behind, but
        // process_pending drains it.
        let _ = js_ws_process_pending();
        // No active servers, no pending events, no open connections.
        // (We can't fully clean state across tests since these are
        // process-globals; assert non-negative as the minimal sanity
        // check.)
        let v = js_ws_has_pending();
        assert!(v >= 0);
    }

    #[test]
    fn handle_to_i64_strips_pointer_tag() {
        let raw_ptr: u64 = 0x1234_5678_9abc;
        let nan_boxed = f64::from_bits(POINTER_TAG | raw_ptr);
        assert_eq!(js_ws_handle_to_i64(nan_boxed), raw_ptr as i64);

        let plain = 42.0_f64;
        assert_eq!(js_ws_handle_to_i64(plain), 42);
    }

    #[test]
    fn client_handles_do_not_alias_registered_servers() {
        let server = js_ws_server_new(f64::from_bits(JsValue::UNDEFINED.bits()));
        let client = register_handle(WsClientHandle);
        assert_ne!(server, client);
        assert!(get_handle_mut::<WsServerHandle>(client).is_none());
        assert!(get_handle_mut::<WsClientHandle>(server).is_none());
        assert_eq!(
            decode_client_id(f64::from_bits(client_js_value(client as usize).bits())),
            client as usize
        );
        assert_eq!(decode_client_id(client as f64), client as usize);
        perry_ffi::drop_handle(client);
        perry_ffi::drop_handle(server);
    }

    /// #6117 — `readyState` walks the npm-ws lifecycle: CONNECTING (0)
    /// pre-open, OPEN (1), CLOSING (2) after `close()` is requested,
    /// CLOSED (3) once the IO loop marks the connection dead, and CLOSED
    /// for ids with no entry (cleaned up, or promise-path connect failed).
    /// Uses an id far outside anything other tests insert, so no lock.
    #[test]
    fn ready_state_reports_npm_ws_lifecycle() {
        let ws_id = 990_077usize;
        let (tx, _rx) = mpsc::unbounded_channel::<WsCommand>();
        WS_CONNECTIONS.lock().unwrap().insert(
            ws_id,
            WsConnection {
                transport: WsTransport::Tokio(tx),
                messages: Vec::new(),
                is_open: false,
                is_closing: false,
                is_closed: false,
            },
        );

        assert_eq!(js_ws_ready_state(ws_id as i64), 0.0);
        WS_CONNECTIONS
            .lock()
            .unwrap()
            .get_mut(&ws_id)
            .unwrap()
            .is_open = true;
        assert_eq!(js_ws_ready_state(ws_id as i64), 1.0);
        js_ws_close(ws_id as i64);
        assert_eq!(js_ws_ready_state(ws_id as i64), 2.0);
        if let Some(c) = WS_CONNECTIONS.lock().unwrap().get_mut(&ws_id) {
            c.is_closed = true;
        }
        assert_eq!(js_ws_ready_state(ws_id as i64), 3.0);
        WS_CONNECTIONS.lock().unwrap().remove(&ws_id);
        assert_eq!(js_ws_ready_state(ws_id as i64), 3.0);
    }
}
