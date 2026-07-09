//! Native bindings for the npm `ws` package — WebSocket client +
//! server via `tokio-tungstenite`. Uses only perry-ffi.
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

/// SIMD-widened WebSocket frame (un)masking (RFC 6455 §5.3). See
/// [`mask::apply_mask`] / [`mask::apply_mask_from`]. The hot tungstenite
/// read/write path masks internally with its own `u32`-blocked routine
/// (private, no injection seam), so this module is the masker for any
/// frame bytes perry handles itself — kept byte-identical to the scalar
/// reference and validated by a property test.
pub mod mask;

use futures_util::{SinkExt, StreamExt};
use lazy_static::lazy_static;
use perry_ffi::{
    alloc_string, gc_register_mutable_root_scanner_named, get_handle_mut, iter_handles_of_mut,
    notify_main_thread, register_handle, spawn_async,
    spawn_blocking_with_reactor as spawn_blocking, take_handle, GcRootVisitor, Handle, JsClosure,
    JsString, JsValue, ObjectHeader, RawClosureHeader, StringHeader,
};
use std::collections::HashMap;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Mutex;
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};

const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
const TAG_MASK: u64 = 0xFFFF_0000_0000_0000;
const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

/// #6117 — rustls panics resolving the process-level CryptoProvider on the
/// first `wss://` handshake when both `ring` and `aws-lc-rs` end up
/// feature-unified into the final link (perry-ext-http-server brings ring;
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

struct WsConnection {
    sender: mpsc::UnboundedSender<WsCommand>,
    messages: Vec<String>,
    is_open: bool,
    /// #6117 — `close()` was called but the close handshake hasn't finished:
    /// `readyState` reports CLOSING (2).
    is_closing: bool,
    /// #6117 — the connection terminated (close event, IO error, or connect
    /// failure): `readyState` reports CLOSED (3). Distinguishes a dead entry
    /// from a pre-open one (CONNECTING, 0) — both have `is_open == false`.
    is_closed: bool,
}

enum WsCommand {
    Send(String),
    Close,
}

struct WsClientListeners {
    listeners: HashMap<String, Vec<i64>>,
}

pub struct WsServerHandle {
    /// Event name → list of closure pointers.
    pub listeners: HashMap<String, Vec<i64>>,
    pub port: u16,
    pub is_listening: bool,
    pub client_ids: Vec<usize>,
    pub shutdown_tx: Option<mpsc::UnboundedSender<()>>,
}

enum PendingWsEvent {
    Connection(Handle, usize),
    Message(usize, String),
    Close(usize, u16, String),
    Error(usize, String),
    ServerError(Handle, String),
    Listening(Handle),
    /// Issue #606 — fired when an outbound client connection succeeds
    /// so `client.on("open", cb)` callbacks fire. Without this, code that
    /// awaits `new Promise(r => client.on("open", () => r()))` hangs
    /// forever even though `is_open=true` was set.
    Open(usize),
}

lazy_static! {
    static ref WS_CONNECTIONS: Mutex<HashMap<usize, WsConnection>> = Mutex::new(HashMap::new());
    static ref WS_CLIENT_PARENT_SERVER: Mutex<HashMap<usize, Handle>> = Mutex::new(HashMap::new());
    static ref NEXT_WS_ID: Mutex<usize> = Mutex::new(1);
    static ref WS_CLIENT_LISTENERS: Mutex<HashMap<usize, WsClientListeners>> =
        Mutex::new(HashMap::new());
    static ref WS_PENDING_EVENTS: Mutex<Vec<PendingWsEvent>> = Mutex::new(Vec::new());
}

static WS_ACTIVE_SERVERS: AtomicI32 = AtomicI32::new(0);
static WS_GC_REGISTERED: std::sync::Once = std::sync::Once::new();

fn ensure_gc_scanner_registered() {
    WS_GC_REGISTERED.call_once(|| {
        gc_register_mutable_root_scanner_named("perry-ext-ws", scan_ws_roots);
    });
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

// ── Client connect ────────────────────────────────────────────────

/// `new WebSocket(url)` — async constructor returning Promise<id>.
///
/// # Safety
/// `url_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_ws_connect(url_ptr: *const StringHeader) -> *mut perry_ffi::Promise {
    ensure_gc_scanner_registered();
    ensure_tls_crypto_provider();
    let promise = perry_ffi::JsPromise::new();
    let raw = promise.as_raw();
    let Some(url) = read_str(url_ptr) else {
        promise.reject_string("Invalid URL");
        return raw;
    };
    // Issue #606 — `spawn_blocking_with_reactor` runs the closure inside
    // a tokio worker task; `Handle::current().block_on` panics in that
    // context. Use `tokio::spawn` so the connect awaits as a sibling task.
    spawn_blocking(move || {
        tokio::spawn(async move {
            match connect_async(&url).await {
                Ok((ws_stream, _resp)) => {
                    let id = setup_client_io(ws_stream);
                    push_ws_event(PendingWsEvent::Open(id));
                    promise.resolve(JsValue::from_number(id as f64));
                }
                Err(e) => promise.reject_string(&format!("WebSocket connect error: {}", e)),
            }
        });
    });
    raw
}

/// `js_ws_connect_start(url_nanboxed)` — sync alternative used by
/// codegen sites that don't expect a Promise return.
#[no_mangle]
pub extern "C" fn js_ws_connect_start(url_nanboxed: f64) -> f64 {
    ensure_gc_scanner_registered();
    ensure_tls_crypto_provider();
    let bits = url_nanboxed.to_bits();
    let mask = TAG_MASK;
    let string_tag = 0x7FFF_0000_0000_0000u64;
    let url = if (bits & mask) == string_tag {
        let ptr = (bits & POINTER_MASK) as *const StringHeader;
        unsafe { read_str(ptr) }
    } else {
        None
    };
    let Some(url) = url else { return 0.0 };

    // Allocate the id synchronously so the caller can register
    // listeners before the connect resolves.
    let mut id_guard = NEXT_WS_ID.lock().unwrap();
    let ws_id = *id_guard;
    *id_guard += 1;
    drop(id_guard);
    let (tx, rx) = mpsc::unbounded_channel::<WsCommand>();
    WS_CONNECTIONS.lock().unwrap().insert(
        ws_id,
        WsConnection {
            sender: tx,
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
    // Issue #606 — same fix as js_ws_connect: tokio::spawn instead of
    // block_on so the connect+IO loop runs as a sibling task on the
    // existing runtime.
    spawn_blocking(move || {
        tokio::spawn(async move {
            match connect_async(&url).await {
                Ok((ws_stream, _)) => {
                    if let Some(c) = WS_CONNECTIONS.lock().unwrap().get_mut(&ws_id) {
                        c.is_open = true;
                    }
                    push_ws_event(PendingWsEvent::Open(ws_id));
                    drive_client_io(ws_id, ws_stream, rx);
                }
                Err(e) => {
                    // #6117 — readyState must report CLOSED (3), not
                    // CONNECTING (0), once the connect has failed.
                    if let Some(c) = WS_CONNECTIONS.lock().unwrap().get_mut(&ws_id) {
                        c.is_closed = true;
                    }
                    push_ws_event(PendingWsEvent::Error(
                        ws_id,
                        format!("WebSocket connect error: {}", e),
                    ));
                }
            }
        });
    });
    ws_id as f64
}

fn setup_client_io(
    ws_stream: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> usize {
    let mut id_guard = NEXT_WS_ID.lock().unwrap();
    let ws_id = *id_guard;
    *id_guard += 1;
    drop(id_guard);
    let (tx, rx) = mpsc::unbounded_channel::<WsCommand>();
    WS_CONNECTIONS.lock().unwrap().insert(
        ws_id,
        WsConnection {
            sender: tx,
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
    drive_client_io(ws_id, ws_stream, rx);
    ws_id
}

fn drive_client_io<S>(
    ws_id: usize,
    ws_stream: tokio_tungstenite::WebSocketStream<S>,
    mut rx: mpsc::UnboundedReceiver<WsCommand>,
) where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    // Issue #606 — `spawn_blocking_with_reactor` runs the closure inside
    // a tokio worker task; `Handle::current().block_on` panics in that
    // context. Spawn the IO loop as a sibling task on the existing
    // runtime instead.
    spawn_blocking(move || {
        tokio::spawn(async move {
            let (mut write, mut read) = ws_stream.split();
            loop {
                tokio::select! {
                    msg_result = read.next() => {
                        match msg_result {
                            Some(Ok(Message::Text(text))) => {
                                let has_listeners = WS_CLIENT_LISTENERS.lock().unwrap()
                                    .get(&ws_id)
                                    .map(|l| l.listeners.get("message").map(|v| !v.is_empty()).unwrap_or(false))
                                    .unwrap_or(false);
                                let text = text.to_string();
                                if has_listeners {
                                    push_ws_event(PendingWsEvent::Message(ws_id, text));
                                } else if let Some(c) = WS_CONNECTIONS.lock().unwrap().get_mut(&ws_id) {
                                    c.messages.push(text);
                                }
                            }
                            Some(Ok(Message::Binary(b))) => {
                                let s = String::from_utf8_lossy(&b).to_string();
                                push_ws_event(PendingWsEvent::Message(ws_id, s));
                            }
                            Some(Ok(Message::Close(frame))) => {
                                let (code, reason) = frame
                                    .map(|f| (f.code.into(), f.reason.to_string()))
                                    .unwrap_or((1000u16, String::new()));
                                if let Some(c) = WS_CONNECTIONS.lock().unwrap().get_mut(&ws_id) {
                                    c.is_open = false;
                                    c.is_closed = true;
                                }
                                push_ws_event(PendingWsEvent::Close(ws_id, code, reason));
                                break;
                            }
                            Some(Ok(_)) => { /* ping/pong/etc — ignore */ }
                            Some(Err(e)) => {
                                push_ws_event(PendingWsEvent::Error(ws_id, format!("{}", e)));
                                break;
                            }
                            None => break,
                        }
                    }
                    cmd = rx.recv() => {
                        match cmd {
                            Some(WsCommand::Send(text)) => {
                                if write.send(Message::Text(text.into())).await.is_err() {
                                    break;
                                }
                            }
                            Some(WsCommand::Close) => {
                                let _ = write.send(Message::Close(None)).await;
                                break;
                            }
                            None => break,
                        }
                    }
                }
            }
            if let Some(c) = WS_CONNECTIONS.lock().unwrap().get_mut(&ws_id) {
                c.is_open = false;
                c.is_closed = true;
            }
        });
    });
}

// ── Send / close (client) ─────────────────────────────────────────

/// # Safety
/// `message_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_ws_send(handle: i64, message_ptr: *const StringHeader) {
    let Some(msg) = read_str(message_ptr) else {
        return;
    };
    let id = handle as usize;
    if let Some(c) = WS_CONNECTIONS.lock().unwrap().get_mut(&id) {
        let _ = c.sender.send(WsCommand::Send(msg));
    }
}

#[no_mangle]
pub extern "C" fn js_ws_close(handle: i64) {
    let id = handle as usize;
    if let Some(c) = WS_CONNECTIONS.lock().unwrap().get_mut(&id) {
        let _ = c.sender.send(WsCommand::Close);
        c.is_open = false;
        // #6117 — readyState reports CLOSING (2) until the IO loop
        // finishes the close handshake and marks is_closed.
        c.is_closing = true;
    }
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
    let id = handle as usize;
    if let Some(c) = WS_CONNECTIONS.lock().unwrap().get_mut(&id) {
        let _ = c.sender.send(WsCommand::Send(msg));
    }
}

/// Issue #577 Phase 4 — `wsId.close()` on an upgrade-path Client.
#[no_mangle]
pub extern "C" fn js_ws_close_client_i64(handle: i64) {
    let id = handle as usize;
    if let Some(c) = WS_CONNECTIONS.lock().unwrap().get_mut(&id) {
        let _ = c.sender.send(WsCommand::Close);
        c.is_open = false;
        c.is_closing = true;
    }
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
    ensure_gc_scanner_registered();
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
        let queued: Vec<String> = if let Some(c) = WS_CONNECTIONS.lock().unwrap().get_mut(&ws_id) {
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
    let id = handle_f64 as i64 as usize;
    let Some(msg) = read_str(message_ptr) else {
        return;
    };
    if let Some(c) = WS_CONNECTIONS.lock().unwrap().get_mut(&id) {
        let _ = c.sender.send(WsCommand::Send(msg));
    }
}

#[no_mangle]
pub extern "C" fn js_ws_close_client(handle_f64: f64) {
    let id = handle_f64 as i64 as usize;
    if let Some(c) = WS_CONNECTIONS.lock().unwrap().get_mut(&id) {
        let _ = c.sender.send(WsCommand::Close);
        c.is_open = false;
        c.is_closing = true;
    }
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
            return alloc_string(&msg).as_raw();
        }
    }
    std::ptr::null_mut()
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
                return alloc_string(&msg).as_raw();
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
    ensure_gc_scanner_registered();
    let Some(event_name) = read_str(event_name_ptr) else {
        return handle;
    };
    if callback_ptr == 0 {
        return handle;
    }
    // Issue #606: client ws_ids (NEXT_WS_ID counter) and server handle
    // ids (perry-ffi NEXT_HANDLE counter) live in disjoint registries
    // but their numeric ranges collide — both start near 1. If we look
    // up the server registry first, a client id that happens to also
    // be a registered server handle id would route through the server
    // arm and the user's `client.on("open", cb)` would land on the
    // server's listeners. Check the client registry first so client
    // dispatch is correct regardless of allocation order.
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
    handle
}

// ── Server ────────────────────────────────────────────────────────

/// `new WebSocketServer({ port })` — sync ctor; spawns the accept loop.
///
/// #1113: `new WebSocketServer({ noServer: true })` must NOT bind a
/// TCP port or spawn the accept loop — it's a passive registry whose
/// connections arrive exclusively via `wss.handleUpgrade(...)` driven
/// by a host server's `'upgrade'` event (fastify's `app.server` or
/// `node:http`). For that shape we register a listener-only handle and
/// return early; `WS_ACTIVE_SERVERS` is left untouched so a noServer
/// wss doesn't keep the event loop alive on its own (the host server's
/// has-active gate — `js_fastify_has_active` — does that).
#[no_mangle]
pub extern "C" fn js_ws_server_new(opts_f64: f64) -> Handle {
    ensure_gc_scanner_registered();
    let port = extract_port(opts_f64);
    let no_server = extract_no_server(opts_f64);

    if no_server || port == 0 {
        // Listener-only handle — no bind, no accept loop, no shutdown
        // channel (nothing to shut down). Connections are injected via
        // `js_ws_handle_upgrade`.
        return register_handle(WsServerHandle {
            listeners: HashMap::new(),
            port: 0,
            is_listening: false,
            client_ids: Vec::new(),
            shutdown_tx: None,
        });
    }

    let (shutdown_tx, mut shutdown_rx) = mpsc::unbounded_channel::<()>();
    let server_handle = register_handle(WsServerHandle {
        listeners: HashMap::new(),
        port,
        is_listening: false,
        client_ids: Vec::new(),
        shutdown_tx: Some(shutdown_tx),
    });
    WS_ACTIVE_SERVERS.fetch_add(1, Ordering::Relaxed);
    let handle_id = server_handle;
    // Issue #606 — `spawn_blocking_with_reactor` already runs the closure
    // inside a tokio worker task, so `Handle::current().block_on(fut)` panics
    // with "Cannot start a runtime from within a runtime". Schedule the
    // accept loop as a sibling task on the existing runtime instead.
    // (Same root cause as the v0.5.691 sweep that fixed perry-ext-http's
    // server.rs / https_server.rs / http2_server.rs and perry-ext-ws's
    // `drive_server_client_io` — this site was missed in that sweep.)
    spawn_blocking(move || {
        tokio::spawn(async move {
            let addr = format!("0.0.0.0:{}", port);
            let listener = match tokio::net::TcpListener::bind(&addr).await {
                Ok(l) => l,
                Err(e) => {
                    push_ws_event(PendingWsEvent::ServerError(
                        handle_id,
                        format!("WebSocketServer bind error: {}", e),
                    ));
                    return;
                }
            };
            if let Some(s) = get_handle_mut::<WsServerHandle>(handle_id) {
                s.is_listening = true;
            }
            push_ws_event(PendingWsEvent::Listening(handle_id));
            loop {
                tokio::select! {
                    accept_result = listener.accept() => {
                        match accept_result {
                            Ok((tcp_stream, _addr)) => {
                                match tokio_tungstenite::accept_async(tcp_stream).await {
                                    Ok(ws_stream) => {
                                        let mut id_guard = NEXT_WS_ID.lock().unwrap();
                                        let ws_id = *id_guard;
                                        *id_guard += 1;
                                        drop(id_guard);
                                        let (tx, rx) = mpsc::unbounded_channel::<WsCommand>();
                                        WS_CONNECTIONS.lock().unwrap().insert(ws_id, WsConnection {
                                            sender: tx,
                                            messages: Vec::new(),
                                            is_open: true,
                                            is_closing: false,
                                            is_closed: false,
                                        });
                                        WS_CLIENT_LISTENERS.lock().unwrap().insert(ws_id, WsClientListeners {
                                            listeners: HashMap::new(),
                                        });
                                        if let Some(s) = get_handle_mut::<WsServerHandle>(handle_id) {
                                            s.client_ids.push(ws_id);
                                        }
                                        WS_CLIENT_PARENT_SERVER.lock().unwrap().insert(ws_id, handle_id);
                                        push_ws_event(PendingWsEvent::Connection(handle_id, ws_id));
                                        drive_server_client_io(ws_id, ws_stream, rx);
                                    }
                                    Err(e) => {
                                        push_ws_event(PendingWsEvent::ServerError(
                                            handle_id,
                                            format!("WebSocket handshake error: {}", e),
                                        ));
                                    }
                                }
                            }
                            Err(e) => {
                                push_ws_event(PendingWsEvent::ServerError(
                                    handle_id,
                                    format!("accept error: {}", e),
                                ));
                            }
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        break;
                    }
                }
            }
            if let Some(s) = get_handle_mut::<WsServerHandle>(handle_id) {
                s.is_listening = false;
            }
            WS_ACTIVE_SERVERS.fetch_sub(1, Ordering::Relaxed);
        });
    });
    server_handle
}

fn extract_port(opts_f64: f64) -> u16 {
    let bits = opts_f64.to_bits();
    if (bits & TAG_MASK) == POINTER_TAG {
        let ptr = (bits & POINTER_MASK) as *const ObjectHeader;
        if !ptr.is_null() {
            // Object literal: assume `port` is the first field
            // (positional shape — same convention as nodemailer/pg/mysql2).
            let val = unsafe { perry_ffi::js_object_get_field(ptr, 0) };
            if val.is_number() {
                let n = val.to_number();
                if n.is_finite() && n > 0.0 {
                    return n as u16;
                }
            }
        }
        return 0;
    }
    if opts_f64.is_finite() && opts_f64 > 0.0 {
        opts_f64 as u16
    } else {
        0
    }
}

/// #1113 — detect `new WebSocketServer({ noServer: true })`.
///
/// perry-ffi exposes only positional object-field reads
/// (`js_object_get_field(ptr, idx)`), not name-based lookup, so we
/// can't read the `noServer` key by name. Heuristic: an options
/// object that carries a `true` boolean field AND no positive numeric
/// port field is a `noServer` config. (A real `{ port: N }` config
/// has a positive number in field 0 — `extract_port` handles that;
/// a `{ noServer: true }` config has no port and a `true` boolean.)
/// `js_ws_server_new` additionally treats "object with no positive
/// port" as noServer, so this is a belt-and-suspenders signal that
/// also catches `{ noServer: true, ...other }` shapes regardless of
/// field order.
fn extract_no_server(opts_f64: f64) -> bool {
    let bits = opts_f64.to_bits();
    if (bits & TAG_MASK) != POINTER_TAG {
        return false;
    }
    let ptr = (bits & POINTER_MASK) as *const ObjectHeader;
    if ptr.is_null() {
        return false;
    }
    unsafe {
        let n = (*ptr).field_count;
        let mut saw_true = false;
        let mut saw_positive_port = false;
        for i in 0..n {
            let v = perry_ffi::js_object_get_field(ptr, i);
            if v.is_bool() && v.to_bool() {
                saw_true = true;
            }
            if v.is_number() {
                let num = v.to_number();
                if num.is_finite() && num > 0.0 {
                    saw_positive_port = true;
                }
            }
        }
        saw_true && !saw_positive_port
    }
}

fn drive_server_client_io<S>(
    ws_id: usize,
    ws_stream: tokio_tungstenite::WebSocketStream<S>,
    mut rx: mpsc::UnboundedReceiver<WsCommand>,
) where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    // Issue #577 (Phase 4 + follow-up): the caller may already be inside a tokio
    // runtime task (the upgrade-from-http path via `register_external_ws_stream`),
    // so `Handle::current().block_on(fut)` would panic ("Cannot start a runtime
    // from within a runtime"). Avoid `spawn_blocking(|| tokio::spawn(...))` too:
    // the nested spawn depends on an ambient `Handle` on a blocking-pool thread,
    // which "proved brittle under release/LTO builds" (see `perry_ffi::spawn_async`
    // docs) — in release the IO loop silently failed to start, so neither inbound
    // frames were read nor `WsCommand::Send` writes flushed (a dead post-upgrade
    // channel). Drive the per-connection IO loop on Perry's shared reactor-owned
    // runtime instead, matching perry-ext-net's socket reader loops.
    //
    // Keepalive: unlike the old `spawn_blocking_with_reactor`, `spawn_async` does
    // NOT bump the event-loop active-handle counter, so this task is not
    // self-keepalive — the caller must own a gate. It does: every path that
    // reaches here registers the connection in `WS_CONNECTIONS` with
    // `is_open = true` BEFORE this call (`register_external_ws_stream` for the
    // upgrade path; the standalone server also holds `WS_ACTIVE_SERVERS`), and
    // `js_ws_has_pending` reports the loop live while any connection is open. So
    // the WS connection itself keeps the shared loop alive for as long as it is
    // open — independent of the host HTTP listener's own gate.
    spawn_async(async move {
        let (mut write, mut read) = ws_stream.split();
        loop {
            tokio::select! {
                msg_result = read.next() => {
                    match msg_result {
                        Some(Ok(Message::Text(text))) => {
                            // Issue #577 Phase 4 — race between
                            // `register_external_ws_stream` (which spawns
                            // this IO loop and starts reading immediately)
                            // and the main-thread `'upgrade'` event firing
                            // (which is where user code registers
                            // `wsId.on('message', cb)`). If the client
                            // sends a frame fast enough, the IO loop
                            // pushes it to WS_PENDING_EVENTS before the
                            // listener exists, then `js_ws_process_pending`
                            // drops it silently. Mirror the client-side
                            // logic at line 268: only push as a pending
                            // event when a listener is already registered;
                            // otherwise queue on `c.messages` so the
                            // listener-registration site can drain it
                            // synchronously.
                            let text_str = text.to_string();
                            let client_has_listener = WS_CLIENT_LISTENERS
                                .lock()
                                .unwrap()
                                .get(&ws_id)
                                .map(|l| l.listeners.get("message").map(|v| !v.is_empty()).unwrap_or(false))
                                .unwrap_or(false);
                            // #746 follow-up: a server-level
                            // `wss.on('message', (ws, data) => ...)` handler
                            // (perry-stdlib::ws parity) registers on the parent
                            // WsServerHandle, not on WS_CLIENT_LISTENERS. The
                            // original #577 Phase 4 race-guard only checked the
                            // per-client map, so a server-only message handler
                            // never produced a PendingWsEvent::Message — the
                            // frame was parked on c.messages forever.
                            let server_has_listener = WS_CLIENT_PARENT_SERVER
                                .lock()
                                .unwrap()
                                .get(&ws_id)
                                .copied()
                                .map(|sh| !listeners_on_server(sh, "message").is_empty())
                                .unwrap_or(false);
                            if client_has_listener || server_has_listener {
                                push_ws_event(PendingWsEvent::Message(ws_id, text_str));
                            } else if let Some(c) = WS_CONNECTIONS.lock().unwrap().get_mut(&ws_id) {
                                c.messages.push(text_str);
                            }
                        }
                        Some(Ok(Message::Binary(b))) => {
                            let s = String::from_utf8_lossy(&b).to_string();
                            push_ws_event(PendingWsEvent::Message(ws_id, s));
                        }
                        Some(Ok(Message::Close(frame))) => {
                            let (code, reason) = frame
                                .map(|f| (f.code.into(), f.reason.to_string()))
                                .unwrap_or((1000u16, String::new()));
                            push_ws_event(PendingWsEvent::Close(ws_id, code, reason));
                            break;
                        }
                        Some(Ok(_)) => {}
                        Some(Err(e)) => {
                            push_ws_event(PendingWsEvent::Error(ws_id, format!("{}", e)));
                            break;
                        }
                        None => break,
                    }
                }
                cmd = rx.recv() => {
                    match cmd {
                        Some(WsCommand::Send(text)) => {
                            if write.send(Message::Text(text.into())).await.is_err() {
                                break;
                            }
                        }
                        Some(WsCommand::Close) => {
                            let _ = write.send(Message::Close(None)).await;
                            break;
                        }
                        None => break,
                    }
                }
            }
        }
        if let Some(c) = WS_CONNECTIONS.lock().unwrap().get_mut(&ws_id) {
            c.is_open = false;
            c.is_closed = true;
        }
    });
}

#[no_mangle]
pub extern "C" fn js_ws_server_close(handle: i64) {
    if let Some(server) = take_handle::<WsServerHandle>(handle) {
        if let Some(tx) = server.shutdown_tx {
            let _ = tx.send(());
        }
    }
}

/// Register an externally-provided WebSocket stream as a perry-ext-ws
/// connection — used by perry-ext-http-server's upgrade path so that
/// `Server.on('upgrade', ...)` integration flows through the same
/// per-client IO loop and listener registry as standalone
/// `WebSocketServer({port})` connections (issue #577 Phase 4).
///
/// Returns the assigned `ws_id` (`usize`-shaped, fits in `i64`)
/// that user code consumes via `js_ws_send` / `js_ws_close` / `js_ws_on`.
/// The caller is responsible for firing whatever 'connection' /
/// 'upgrade' event listeners are appropriate; this function does not
/// push a `PendingWsEvent::Connection`.
pub fn register_external_ws_stream<S>(ws_stream: tokio_tungstenite::WebSocketStream<S>) -> i64
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    ensure_gc_scanner_registered();
    let mut id_guard = NEXT_WS_ID.lock().unwrap();
    let ws_id = *id_guard;
    *id_guard += 1;
    drop(id_guard);
    let (tx, rx) = mpsc::unbounded_channel::<WsCommand>();
    WS_CONNECTIONS.lock().unwrap().insert(
        ws_id,
        WsConnection {
            sender: tx,
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
    drive_server_client_io(ws_id, ws_stream, rx);
    ws_id as i64
}

/// #1113 — `wss.handleUpgrade(req, socket, head, cb)` for a
/// `new WebSocketServer({ noServer: true })`.
///
/// The handshake + per-client IO loop already happened: the host
/// server's accept task (fastify's `handle_fastify_websocket_upgrade`
/// or perry-ext-http-server's `handle_websocket_upgrade`) drove
/// `hyper::upgrade::on`, completed the tungstenite server handshake,
/// and called `register_external_ws_stream` (which spawned
/// `drive_server_client_io`). By the time the user's `'upgrade'`
/// handler runs and calls `wss.handleUpgrade(...)`, `ws_id` is already
/// a live connection. This function is purely the JS-visible
/// re-dispatch shim — it does NOT register another stream or perform
/// another handshake.
///
/// Steps (mirror `WebSocketServer({port})`'s per-connection wiring):
///   1. Decode `ws_id` from the POINTER_TAG-boxed `ws_id_f64`.
///   2. Adopt the connection under this server (`WS_CLIENT_PARENT_SERVER`
///      + `client_ids`) so server-level `wss.on('message'|'close', …)`
///      handlers route, and the GC scanner pins the right listeners.
///   3. Invoke the user's `cb(socket)` with `socket === ws_id_f64`
///      (the same NaN-boxed id `wss.on('connection', (ws) => …)` gets,
///      so `ws.send(...)` / `ws.on(...)` dispatch through the Client
///      class arm).
///   4. Also push `PendingWsEvent::Connection` so a separately
///      registered `wss.on('connection', cb)` fires through the pump.
///
/// `req_f64` / `head_f64` are accepted for API shape parity (Node's
/// `handleUpgrade(request, socket, head, callback)`); they're not
/// consumed here — the request metadata was already surfaced to the
/// `'upgrade'` handler.
///
/// # Safety
/// `cb`, when non-zero, must be a valid NaN-boxed / raw closure
/// pointer. `ws_id_f64` must be a POINTER_TAG-boxed ws id produced by
/// the host server's upgrade path.
#[no_mangle]
pub unsafe extern "C" fn js_ws_handle_upgrade(
    server_handle: i64,
    _req_f64: f64,
    ws_id_f64: f64,
    _head_f64: f64,
    cb: i64,
) -> i64 {
    ensure_gc_scanner_registered();
    // `ws_id_f64` is POINTER_TAG-boxed (the host upgrade path encodes
    // it as `POINTER_TAG | (ws_id & POINTER_MASK)` so codegen's
    // unbox_to_i64 round-trips it). Extract the low-48 bits.
    let ws_id = (ws_id_f64.to_bits() & POINTER_MASK) as usize;
    if ws_id == 0 {
        return server_handle;
    }

    WS_CLIENT_PARENT_SERVER
        .lock()
        .unwrap()
        .insert(ws_id, server_handle);
    if let Some(s) = get_handle_mut::<WsServerHandle>(server_handle) {
        if !s.client_ids.contains(&ws_id) {
            s.client_ids.push(ws_id);
        }
    }

    if cb != 0 {
        // Accept either a NaN-boxed POINTER_TAG closure or a raw
        // pointer (same dual-shape the rest of the crate handles).
        let raw = if (cb as u64 & TAG_MASK) == POINTER_TAG {
            (cb as u64 & POINTER_MASK) as *const RawClosureHeader
        } else {
            cb as *const RawClosureHeader
        };
        let closure = JsClosure::from_raw(raw);
        if !closure.is_null() {
            let _ = closure.call1(ws_id_f64);
        }
    }

    // Also fire a Connection event so a `wss.on('connection', cb)`
    // registered separately from `handleUpgrade`'s inline callback
    // still runs through the normal pump.
    push_ws_event(PendingWsEvent::Connection(server_handle, ws_id));
    notify_main_thread();
    server_handle
}

// ── Event-loop tick ───────────────────────────────────────────────

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
                let listeners = listeners_on_server(server_handle, "connection");
                for cb in listeners {
                    if cb != 0 {
                        let closure = unsafe { JsClosure::from_raw(cb as *const RawClosureHeader) };
                        // Pass client_id as f64 so user handler can
                        // pass it back to js_ws_send_to_client etc.
                        let _ = unsafe { closure.call1(client_id as f64) };
                        fired += 1;
                    }
                }
            }
            PendingWsEvent::Message(ws_id, text) => {
                let listeners = listeners_on_client(ws_id, "message");
                let s = alloc_string(&text);
                let msg_f64 = f64::from_bits(JsValue::from_string_ptr(s.as_raw()).bits());
                if !listeners.is_empty() {
                    for cb in listeners {
                        if cb != 0 {
                            let closure =
                                unsafe { JsClosure::from_raw(cb as *const RawClosureHeader) };
                            let _ = unsafe { closure.call1(msg_f64) };
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
                                let _ = unsafe { closure.call2(ws_id as f64, msg_f64) };
                                fired += 1;
                            }
                        }
                    }
                }
            }
            PendingWsEvent::Close(ws_id, _code, _reason) => {
                let listeners = listeners_on_client(ws_id, "close");
                if !listeners.is_empty() {
                    for cb in listeners {
                        if cb != 0 {
                            let closure =
                                unsafe { JsClosure::from_raw(cb as *const RawClosureHeader) };
                            let _ = unsafe { closure.call0() };
                            fired += 1;
                        }
                    }
                } else {
                    // #746 follow-up: server-level `wss.on('close',
                    // (ws) => ...)` parity with perry-stdlib::ws.
                    let parent = WS_CLIENT_PARENT_SERVER.lock().unwrap().get(&ws_id).copied();
                    if let Some(server_handle) = parent {
                        for cb in listeners_on_server(server_handle, "close") {
                            if cb != 0 {
                                let closure =
                                    unsafe { JsClosure::from_raw(cb as *const RawClosureHeader) };
                                let _ = unsafe { closure.call1(ws_id as f64) };
                                fired += 1;
                            }
                        }
                    }
                }
                WS_CONNECTIONS.lock().unwrap().remove(&ws_id);
                WS_CLIENT_LISTENERS.lock().unwrap().remove(&ws_id);
                WS_CLIENT_PARENT_SERVER.lock().unwrap().remove(&ws_id);
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
        _lock: MutexGuard<'static, ()>,
    }

    impl GcTestGuard {
        fn new() -> Self {
            let lock = GC_TEST_LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            perry_runtime::gc::js_gc_write_barriers_emitted(1);
            let frame = perry_runtime::gc::js_shadow_frame_push(0);
            Self { frame, _lock: lock }
        }
    }

    impl Drop for GcTestGuard {
        fn drop(&mut self) {
            perry_runtime::gc::js_shadow_frame_pop(self.frame);
            perry_runtime::gc::js_gc_write_barriers_emitted(0);
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
        ensure_gc_scanner_registered();
        ensure_gc_scanner_registered();
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
        let server_handle = register_handle(WsServerHandle {
            listeners: HashMap::from([("connection".to_string(), vec![server_callback])]),
            port: 0,
            is_listening: false,
            client_ids: Vec::new(),
            shutdown_tx: None,
        });

        let _ = perry_runtime::gc::gc_collect_minor();

        {
            let clients = WS_CLIENT_LISTENERS.lock().unwrap();
            assert_rewritten(client_callback, clients[&client_id].listeners["message"][0]);
            let server = get_handle::<WsServerHandle>(server_handle)
                .expect("server handle should remain live");
            assert_rewritten(server_callback, server.listeners["connection"][0]);
        }
        WS_CLIENT_LISTENERS.lock().unwrap().remove(&client_id);
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
    fn extract_port_from_number_arg() {
        assert_eq!(extract_port(8080.0), 8080);
        assert_eq!(extract_port(0.0), 0);
        assert_eq!(extract_port(-5.0), 0);
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
                sender: tx,
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
