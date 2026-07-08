//! WebSocket module (ws compatible)
//!
//! Native implementation of the 'ws' npm package using tokio-tungstenite.
//! Provides WebSocket client and server functionality.

#[cfg(not(target_os = "ios"))]
use futures_util::{SinkExt, StreamExt};
use perry_runtime::{
    js_closure_call0, js_closure_call1, js_closure_call2, js_string_from_bytes, ClosureHeader,
    JSValue, StringHeader,
};
use std::collections::HashMap;
use std::sync::Mutex;
#[cfg(not(target_os = "ios"))]
use tokio::sync::mpsc;
#[cfg(not(target_os = "ios"))]
use tokio_tungstenite::{connect_async, tungstenite::Message};

#[cfg(not(target_os = "ios"))]
use crate::common::async_bridge::{queue_promise_resolution, spawn};
use crate::common::{for_each_handle_mut_of, get_handle_mut, register_handle, Handle};

/// #6117 — rustls panics resolving the process-level CryptoProvider on the
/// first `wss://` handshake when both `ring` and `aws-lc-rs` end up
/// feature-unified into the final link (perry-ext-http-server brings ring;
/// net/tls bring aws-lc-rs). Install one explicitly before connecting.
/// Idempotent — `install_default` errors (ignored) if a provider is already
/// set. Mirrors `net::mod` / `tls` (#4971) and `perry-ext-net`.
#[cfg(not(target_os = "ios"))]
fn ensure_tls_crypto_provider() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
}

fn ws_file_log(msg: &str) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/hone-ws-macos.log")
    {
        let _ = writeln!(f, "{}", msg);
    }
}

// On iOS, delegate to native NSURLSessionWebSocketTask implementation (provided by perry-ui-ios)
#[cfg(target_os = "ios")]
extern "C" {
    fn perry_native_ws_connect(url_ptr: *const u8) -> f64;
    fn perry_native_ws_is_open(handle: f64) -> f64;
    fn perry_native_ws_send(handle: f64, msg_ptr: *const u8);
    fn perry_native_ws_receive(handle: f64) -> f64;
    fn perry_native_ws_message_count(handle: f64) -> f64;
    fn perry_native_ws_close(handle: f64);
}

// WebSocket handle storage
#[cfg(not(target_os = "ios"))]
lazy_static::lazy_static! {
    static ref WS_CONNECTIONS: Mutex<HashMap<usize, WsConnection>> = Mutex::new(HashMap::new());
    /// Map from client ws_id to parent server handle (for server-connected clients)
    static ref WS_CLIENT_PARENT_SERVER: Mutex<HashMap<usize, Handle>> = Mutex::new(HashMap::new());
}

lazy_static::lazy_static! {
    static ref NEXT_WS_ID: Mutex<usize> = Mutex::new(1);
    /// Per-client event listeners (for .on('message', cb) etc.)
    static ref WS_CLIENT_LISTENERS: Mutex<HashMap<usize, WsClientListeners>> = Mutex::new(HashMap::new());
    /// Pending WebSocket events to be processed on the main thread
    static ref WS_PENDING_EVENTS: Mutex<Vec<PendingWsEvent>> = Mutex::new(Vec::new());
}

#[cfg(not(target_os = "ios"))]
static WS_GC_REGISTERED: std::sync::Once = std::sync::Once::new();

/// Register the ws GC root scanner exactly once. Safe to call from any
/// ws FFI entry point on the main thread. Mirrors `net::ensure_gc_scanner_registered`
/// (issue #35) — user closures passed to `.on(event, cb)` are stored in
/// WS_CLIENT_LISTENERS (for client sockets) or inside a WsServerHandle
/// (for servers); neither is visible to the GC mark phase without this
/// scanner, so a malloc-triggered sweep between registration and
/// dispatch would free the closure and the next event would call freed
/// memory.
#[cfg(not(target_os = "ios"))]
fn ensure_gc_scanner_registered() {
    WS_GC_REGISTERED.call_once(|| {
        perry_runtime::gc::gc_register_mutable_root_scanner_named("stdlib:ws", scan_ws_roots_mut);
    });
}

/// GC root scanner for WebSocket event listener closures. Covers both
/// the global `WS_CLIENT_LISTENERS` map (for `WebSocket` clients) and
/// every `WsServerHandle` currently in the handle registry (for
/// `WebSocketServer` instances).
#[cfg(not(target_os = "ios"))]
#[allow(dead_code)]
fn scan_ws_roots(mark: &mut dyn FnMut(f64)) {
    let mut visitor = perry_runtime::gc::RuntimeRootVisitor::for_copy(mark);
    scan_ws_roots_mut(&mut visitor);
}

#[cfg(not(target_os = "ios"))]
fn scan_ws_roots_mut(visitor: &mut perry_runtime::gc::RuntimeRootVisitor<'_>) {
    if let Ok(mut per_client) = WS_CLIENT_LISTENERS.lock() {
        for client in per_client.values_mut() {
            for cb_vec in client.listeners.values_mut() {
                for cb in cb_vec.iter_mut() {
                    visitor.visit_i64_slot(cb);
                }
            }
        }
    }

    for_each_handle_mut_of::<WsServerHandle, _>(|server| {
        for cb_vec in server.listeners.values_mut() {
            for cb in cb_vec.iter_mut() {
                visitor.visit_i64_slot(cb);
            }
        }
    });
}

/// Number of active WS servers — keeps the event loop alive.
static WS_ACTIVE_SERVERS: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

#[cfg(not(target_os = "ios"))]
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

#[cfg(not(target_os = "ios"))]
enum WsCommand {
    Send(String),
    Close,
}

/// Per-client event listeners
struct WsClientListeners {
    listeners: HashMap<String, Vec<i64>>,
}

/// WebSocketServer handle
#[cfg(not(target_os = "ios"))]
pub struct WsServerHandle {
    /// Event name -> list of closure pointers (stored as i64 for Send + Sync)
    pub listeners: HashMap<String, Vec<i64>>,
    pub port: u16,
    pub is_listening: bool,
    /// Track connected client IDs for cleanup
    pub client_ids: Vec<usize>,
    /// Shutdown signal sender
    pub shutdown_tx: Option<mpsc::UnboundedSender<()>>,
}

/// Pending WebSocket event to be dispatched on the main thread
enum PendingWsEvent {
    /// Server received a new connection: (server_handle, client_ws_id)
    Connection(Handle, usize),
    /// Client received a message: (client_ws_id, message)
    Message(usize, String),
    /// Client connection closed: (client_ws_id, code, reason)
    Close(usize, u16, String),
    /// Error on client: (client_ws_id, error_message)
    Error(usize, String),
    /// Server error: (server_handle, error_message)
    ServerError(Handle, String),
    /// Server started listening: (server_handle)
    Listening(Handle),
}

/// Push a WS event and wake the main-thread pump (issue #84).
///
/// Every producer in this file runs inside a tokio-spawned task or
/// upgrade handler, so direct `.push()` without a notify would leave the
/// event invisible to the main thread until the next `js_wait_for_event`
/// timeout (old code: 10 ms). Wrapping here covers all 18 call sites at
/// once.
#[cfg(not(target_os = "ios"))]
fn push_ws_event(ev: PendingWsEvent) {
    WS_PENDING_EVENTS.lock().unwrap().push(ev);
    perry_runtime::event_pump::js_notify_main_thread();
}

#[cfg(not(target_os = "ios"))]
fn mark_ws_connection_closed(ws_id: usize) -> bool {
    WS_CONNECTIONS
        .lock()
        .unwrap()
        .get_mut(&ws_id)
        .map(|conn| {
            let was_open = conn.is_open;
            conn.is_open = false;
            conn.is_closed = true;
            was_open
        })
        .unwrap_or(false)
}

#[cfg(not(target_os = "ios"))]
fn cleanup_ws_client(ws_id: usize) {
    WS_CONNECTIONS.lock().unwrap().remove(&ws_id);
    WS_CLIENT_LISTENERS.lock().unwrap().remove(&ws_id);

    let parent = WS_CLIENT_PARENT_SERVER.lock().unwrap().remove(&ws_id);
    if let Some(server_handle) = parent {
        if let Some(server) = get_handle_mut::<WsServerHandle>(server_handle) {
            server.client_ids.retain(|client_id| *client_id != ws_id);
        }
    }
}

/// Helper to extract string from StringHeader pointer
unsafe fn string_from_header(ptr: *const StringHeader) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let len = (*ptr).byte_len as usize;
    let data_ptr = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);
    std::str::from_utf8(bytes).ok().map(|s| s.to_string())
}

/// Create a new WebSocket connection
/// new WebSocket(url) -> Promise<WebSocket>
#[cfg(not(target_os = "ios"))]
#[no_mangle]
pub unsafe extern "C" fn js_ws_connect(
    url_ptr: *const StringHeader,
) -> *mut perry_runtime::Promise {
    ensure_gc_scanner_registered();
    ensure_tls_crypto_provider();
    #[cfg(target_os = "android")]
    {
        extern "C" {
            fn __android_log_print(prio: i32, tag: *const u8, fmt: *const u8, ...) -> i32;
        }
        __android_log_print(3, b"PerryWS\0".as_ptr(), b"js_ws_connect called\0".as_ptr());
    }
    let promise = perry_runtime::js_promise_new();
    let promise_ptr = promise as usize;

    let url = match string_from_header(url_ptr) {
        Some(u) => u,
        None => {
            let err_msg = "Invalid URL";
            let err_str = js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
            let err_bits = JSValue::pointer(err_str as *const u8).bits();
            queue_promise_resolution(promise_ptr, false, err_bits);
            return promise;
        }
    };

    #[cfg(target_os = "android")]
    {
        extern "C" {
            fn __android_log_print(prio: i32, tag: *const u8, fmt: *const u8, ...) -> i32;
        }
        __android_log_print(
            3,
            b"PerryWS\0".as_ptr(),
            b"ws_connect: spawning async for URL\0".as_ptr(),
        );
    }

    let url_for_log = url.clone();
    spawn(async move {
        #[cfg(target_os = "android")]
        {
            extern "C" {
                fn __android_log_print(prio: i32, tag: *const u8, fmt: *const u8, ...) -> i32;
            }
            unsafe {
                __android_log_print(
                    3,
                    b"PerryWS\0".as_ptr(),
                    b"ws_connect: connect_async starting\0".as_ptr(),
                );
            }
        }
        match connect_async(&url_for_log).await {
            Ok((ws_stream, _response)) => {
                #[cfg(target_os = "android")]
                {
                    extern "C" {
                        fn __android_log_print(
                            prio: i32,
                            tag: *const u8,
                            fmt: *const u8,
                            ...
                        ) -> i32;
                    }
                    unsafe {
                        __android_log_print(
                            3,
                            b"PerryWS\0".as_ptr(),
                            b"ws_connect: SUCCESS connected\0".as_ptr(),
                        );
                    }
                }
                // Create command channel
                let (tx, mut rx) = mpsc::unbounded_channel::<WsCommand>();

                // Allocate connection ID
                let mut id_guard = NEXT_WS_ID.lock().unwrap();
                let ws_id = *id_guard;
                *id_guard += 1;
                drop(id_guard);

                // Store connection
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

                // Initialize client listeners
                WS_CLIENT_LISTENERS.lock().unwrap().insert(
                    ws_id,
                    WsClientListeners {
                        listeners: HashMap::new(),
                    },
                );

                // Single task handles both read and write (avoids BiLock split issue)
                let ws_id_io = ws_id;
                tokio::spawn(async move {
                    ws_file_log(&format!("[WS-io] started for id={}", ws_id_io));
                    let (mut write, mut read) = ws_stream.split();
                    loop {
                        tokio::select! {
                            msg_result = read.next() => {
                                match msg_result {
                                    Some(Ok(Message::Text(text))) => {
                                        let text = text.to_string();
                                        let has_listeners = WS_CLIENT_LISTENERS.lock().unwrap()
                                            .get(&ws_id_io)
                                            .map(|l| l.listeners.get("message").map(|v| !v.is_empty()).unwrap_or(false))
                                            .unwrap_or(false);
                                        if has_listeners {
                                            push_ws_event(
                                                PendingWsEvent::Message(ws_id_io, text)
                                            );
                                        } else {
                                            if let Some(conn) = WS_CONNECTIONS.lock().unwrap().get_mut(&ws_id_io) {
                                                conn.messages.push(text);
                                            }
                                        }
                                    }
                                    Some(Ok(Message::Close(frame))) => {
                                        let (code, reason) = frame
                                            .map(|f| (f.code.into(), f.reason.to_string()))
                                            .unwrap_or((1000u16, String::new()));
                                        mark_ws_connection_closed(ws_id_io);
                                        push_ws_event(
                                            PendingWsEvent::Close(ws_id_io, code, reason)
                                        );
                                        break;
                                    }
                                    Some(Err(e)) => {
                                        mark_ws_connection_closed(ws_id_io);
                                        push_ws_event(
                                            PendingWsEvent::Error(ws_id_io, format!("{}", e))
                                        );
                                        push_ws_event(
                                            PendingWsEvent::Close(ws_id_io, 1006, String::new())
                                        );
                                        break;
                                    }
                                    Some(Ok(_)) => {} // binary, ping, pong — ignore
                                    None => {
                                        // Stream ended
                                        if mark_ws_connection_closed(ws_id_io) {
                                            push_ws_event(
                                                PendingWsEvent::Close(ws_id_io, 1000, String::new())
                                            );
                                        }
                                        break;
                                    }
                                }
                            }
                            cmd = rx.recv() => {
                                match cmd {
                                    Some(WsCommand::Send(msg)) => {
                                        ws_file_log(&format!("[WS-io] sending len={}", msg.len()));
                                        if let Err(e) = write.send(Message::Text(msg.into())).await {
                                            ws_file_log(&format!("[WS-io] send ERR: {}", e));
                                            if mark_ws_connection_closed(ws_id_io) {
                                                push_ws_event(
                                                    PendingWsEvent::Error(ws_id_io, format!("{}", e))
                                                );
                                                push_ws_event(
                                                    PendingWsEvent::Close(ws_id_io, 1006, String::new())
                                                );
                                            }
                                            break;
                                        }
                                        ws_file_log("[WS-io] send OK");
                                    }
                                    Some(WsCommand::Close) => {
                                        let _ = write.send(Message::Close(None)).await;
                                        if mark_ws_connection_closed(ws_id_io) {
                                            push_ws_event(
                                                PendingWsEvent::Close(ws_id_io, 1000, String::new())
                                            );
                                        }
                                        break;
                                    }
                                    None => {
                                        if mark_ws_connection_closed(ws_id_io) {
                                            push_ws_event(
                                                PendingWsEvent::Close(ws_id_io, 1000, String::new())
                                            );
                                        }
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    // Mark as closed
                    mark_ws_connection_closed(ws_id_io);
                    ws_file_log(&format!("[WS-io] task ended for id={}", ws_id_io));
                });

                // Return WebSocket handle
                let result_bits = (ws_id as f64).to_bits();
                queue_promise_resolution(promise_ptr, true, result_bits);
            }
            Err(e) => {
                #[cfg(target_os = "android")]
                {
                    extern "C" {
                        fn __android_log_print(
                            prio: i32,
                            tag: *const u8,
                            fmt: *const u8,
                            ...
                        ) -> i32;
                    }
                    let msg = format!("ws_connect: FAILED: {}\0", e);
                    unsafe {
                        __android_log_print(
                            6,
                            b"PerryWS\0".as_ptr(),
                            b"%s\0".as_ptr(),
                            msg.as_ptr(),
                        );
                    }
                }
                let err_msg = format!("WebSocket connection error: {}", e);
                let err_str = js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
                let err_bits = JSValue::pointer(err_str as *const u8).bits();
                queue_promise_resolution(promise_ptr, false, err_bits);
            }
        }
    });

    promise
}

/// Create a new WebSocket connection (synchronous — returns handle immediately).
/// Connection happens in background. isOpen() returns 0 until connected.
/// connectStart(url) -> handle (number)
/// Accepts f64 NaN-boxed string (extracts pointer internally).
#[cfg(not(target_os = "ios"))]
#[no_mangle]
pub unsafe extern "C" fn js_ws_connect_start(url_nanboxed: f64) -> f64 {
    ensure_gc_scanner_registered();
    ensure_tls_crypto_provider();
    #[cfg(target_os = "android")]
    {
        extern "C" {
            fn __android_log_print(prio: i32, tag: *const u8, fmt: *const u8, ...) -> i32;
        }
        __android_log_print(
            3,
            b"PerryWS\0".as_ptr(),
            b"js_ws_connect_start called\0".as_ptr(),
        );
    }
    // Extract string pointer from NaN-boxed value
    let url_ptr = perry_runtime::js_get_string_pointer_unified(url_nanboxed) as *const StringHeader;
    let url = match string_from_header(url_ptr) {
        Some(u) => u,
        None => return 0.0,
    };

    // Allocate ws_id immediately (before async connection)
    let mut id_guard = NEXT_WS_ID.lock().unwrap();
    let ws_id = *id_guard;
    *id_guard += 1;
    drop(id_guard);

    // Create command channel
    let (tx, mut rx) = mpsc::unbounded_channel::<WsCommand>();

    // Store connection (initially NOT open)
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

    // Initialize client listeners
    WS_CLIENT_LISTENERS.lock().unwrap().insert(
        ws_id,
        WsClientListeners {
            listeners: HashMap::new(),
        },
    );

    // Connect in background
    spawn(async move {
        match connect_async(&url).await {
            Ok((ws_stream, _response)) => {
                // Mark as open
                if let Some(conn) = WS_CONNECTIONS.lock().unwrap().get_mut(&ws_id) {
                    conn.is_open = true;
                }

                // Single task handles both read and write (avoids BiLock split issue)
                let ws_id_io = ws_id;
                tokio::spawn(async move {
                    let (mut write, mut read) = ws_stream.split();
                    loop {
                        tokio::select! {
                            msg_result = read.next() => {
                                match msg_result {
                                    Some(Ok(Message::Text(text))) => {
                                        let text = text.to_string();
                                        let has_listeners = WS_CLIENT_LISTENERS.lock().unwrap()
                                            .get(&ws_id_io)
                                            .map(|l| l.listeners.get("message").map(|v| !v.is_empty()).unwrap_or(false))
                                            .unwrap_or(false);
                                        if has_listeners {
                                            push_ws_event(
                                                PendingWsEvent::Message(ws_id_io, text)
                                            );
                                        } else {
                                            if let Some(conn) = WS_CONNECTIONS.lock().unwrap().get_mut(&ws_id_io) {
                                                conn.messages.push(text);
                                            }
                                        }
                                    }
                                    Some(Ok(Message::Close(frame))) => {
                                        let (code, reason) = frame
                                            .map(|f| (f.code.into(), f.reason.to_string()))
                                            .unwrap_or((1000u16, String::new()));
                                        mark_ws_connection_closed(ws_id_io);
                                        push_ws_event(
                                            PendingWsEvent::Close(ws_id_io, code, reason)
                                        );
                                        break;
                                    }
                                    Some(Err(e)) => {
                                        mark_ws_connection_closed(ws_id_io);
                                        push_ws_event(
                                            PendingWsEvent::Error(ws_id_io, format!("{}", e))
                                        );
                                        push_ws_event(
                                            PendingWsEvent::Close(ws_id_io, 1006, String::new())
                                        );
                                        break;
                                    }
                                    Some(Ok(_)) => {}
                                    None => {
                                        if mark_ws_connection_closed(ws_id_io) {
                                            push_ws_event(
                                                PendingWsEvent::Close(ws_id_io, 1000, String::new())
                                            );
                                        }
                                        break;
                                    }
                                }
                            }
                            cmd = rx.recv() => {
                                match cmd {
                                    Some(WsCommand::Send(msg)) => {
                                        if let Err(e) = write.send(Message::Text(msg.into())).await {
                                            if mark_ws_connection_closed(ws_id_io) {
                                                push_ws_event(
                                                    PendingWsEvent::Error(ws_id_io, format!("{}", e))
                                                );
                                                push_ws_event(
                                                    PendingWsEvent::Close(ws_id_io, 1006, String::new())
                                                );
                                            }
                                            break;
                                        }
                                    }
                                    Some(WsCommand::Close) => {
                                        let _ = write.send(Message::Close(None)).await;
                                        if mark_ws_connection_closed(ws_id_io) {
                                            push_ws_event(
                                                PendingWsEvent::Close(ws_id_io, 1000, String::new())
                                            );
                                        }
                                        break;
                                    }
                                    None => {
                                        if mark_ws_connection_closed(ws_id_io) {
                                            push_ws_event(
                                                PendingWsEvent::Close(ws_id_io, 1000, String::new())
                                            );
                                        }
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    mark_ws_connection_closed(ws_id_io);
                });
            }
            Err(e) => {
                // #6117 — readyState must report CLOSED (3), not
                // CONNECTING (0), once the connect has failed.
                mark_ws_connection_closed(ws_id);
                push_ws_event(PendingWsEvent::Error(
                    ws_id,
                    format!("WebSocket connection error: {}", e),
                ));
                push_ws_event(PendingWsEvent::Close(ws_id, 1006, String::new()));
            }
        }
    });

    ws_id as f64
}

/// iOS: delegate to native NSURLSessionWebSocketTask
#[cfg(target_os = "ios")]
#[no_mangle]
pub unsafe extern "C" fn js_ws_connect_start(url_nanboxed: f64) -> f64 {
    let url_ptr = perry_runtime::js_get_string_pointer_unified(url_nanboxed) as *const u8;
    perry_native_ws_connect(url_ptr)
}

/// iOS: delegate to native
#[cfg(target_os = "ios")]
#[no_mangle]
pub unsafe extern "C" fn js_ws_connect(
    url_ptr: *const StringHeader,
) -> *mut perry_runtime::Promise {
    let promise = perry_runtime::js_promise_new();
    let handle = perry_native_ws_connect(url_ptr as *const u8);
    let result_bits = handle.to_bits();
    // Resolve immediately with the handle (connection happens async in native)
    crate::common::async_bridge::queue_promise_resolution(promise as usize, true, result_bits);
    promise
}

/// Send a message through the WebSocket
/// ws.send(message) -> void
#[cfg(not(target_os = "ios"))]
#[no_mangle]
pub unsafe extern "C" fn js_ws_send(handle: i64, message_ptr: *const StringHeader) {
    let ws_id = handle as usize;
    let message = match string_from_header(message_ptr) {
        Some(m) => {
            ws_file_log(&format!("[WS-send] id={} len={}", ws_id, m.len()));
            m
        }
        None => {
            ws_file_log(&format!("[WS-send] id={} string_from_header=None", ws_id));
            return;
        }
    };

    let guard = WS_CONNECTIONS.lock().unwrap();
    if let Some(conn) = guard.get(&ws_id) {
        match conn.sender.send(WsCommand::Send(message)) {
            Ok(()) => ws_file_log("[WS-send] channel send OK"),
            Err(e) => ws_file_log(&format!("[WS-send] channel send ERR: {}", e)),
        }
    } else {
        ws_file_log(&format!("[WS-send] no connection for id={}", ws_id));
    }
}

#[cfg(target_os = "ios")]
#[no_mangle]
pub unsafe extern "C" fn js_ws_send(handle: i64, message_ptr: *const StringHeader) {
    perry_native_ws_send(handle as f64, message_ptr as *const u8);
}

/// Close the WebSocket connection or server
/// ws.close() / wss.close() -> void
/// Checks if handle is a server first, then falls back to client close
#[cfg(not(target_os = "ios"))]
#[no_mangle]
pub unsafe extern "C" fn js_ws_close(handle: i64) {
    // Check if this is a server handle
    if get_handle_mut::<WsServerHandle>(handle).is_some() {
        js_ws_server_close(handle);
        return;
    }

    // Otherwise close client connection
    let ws_id = handle as usize;
    let mut guard = WS_CONNECTIONS.lock().unwrap();
    if let Some(conn) = guard.get_mut(&ws_id) {
        // #6117 — readyState reports CLOSING (2) until the close completes.
        conn.is_closing = true;
        let _ = conn.sender.send(WsCommand::Close);
    }
}

#[cfg(target_os = "ios")]
#[no_mangle]
pub unsafe extern "C" fn js_ws_close(handle: i64) {
    unsafe {
        perry_native_ws_close(handle as f64);
    }
}

/// Server-side bridges: `sendToClient(handle, msg)` / `closeClient(handle)`.
/// `ws.on('connection', cb)` delivers the client handle as a plain f64
/// number (see `PendingWsEvent::Connection` dispatch — `client_ws_id as f64`,
/// not NaN-boxed), so the codegen passes f64 here rather than the i64 form
/// `js_ws_send`/`js_ws_close` use for receiver-style `ws.send(...)` calls.
#[no_mangle]
pub unsafe extern "C" fn js_ws_send_to_client(handle_f64: f64, message_ptr: *const StringHeader) {
    js_ws_send(handle_f64 as i64, message_ptr);
}

#[no_mangle]
pub unsafe extern "C" fn js_ws_close_client(handle_f64: f64) {
    js_ws_close(handle_f64 as i64);
}

/// Check if WebSocket is open
/// ws.readyState === WebSocket.OPEN
#[cfg(not(target_os = "ios"))]
#[no_mangle]
pub extern "C" fn js_ws_is_open(handle: i64) -> f64 {
    let ws_id = handle as usize;

    let guard = WS_CONNECTIONS.lock().unwrap();
    match guard.get(&ws_id) {
        Some(conn) => {
            if conn.is_open {
                1.0
            } else {
                0.0
            }
        }
        None => 0.0,
    }
}

#[cfg(target_os = "ios")]
#[no_mangle]
pub extern "C" fn js_ws_is_open(handle: i64) -> f64 {
    unsafe { perry_native_ws_is_open(handle as f64) }
}

/// #6117 — `ws.readyState` per npm-ws semantics: CONNECTING=0, OPEN=1,
/// CLOSING=2, CLOSED=3. An id with no map entry is CLOSED — either the
/// entry was cleaned up after close, or the promise-path connect failed
/// before an entry was ever created.
#[cfg(not(target_os = "ios"))]
#[no_mangle]
pub extern "C" fn js_ws_ready_state(handle: i64) -> f64 {
    let ws_id = handle as usize;
    match WS_CONNECTIONS.lock().unwrap().get(&ws_id) {
        Some(conn) if conn.is_closed => 3.0,
        Some(conn) if conn.is_closing => 2.0,
        Some(conn) if conn.is_open => 1.0,
        Some(_) => 0.0,
        None => 3.0,
    }
}

/// iOS: NSURLSessionWebSocketTask exposes no CONNECTING/CLOSING signal
/// through the existing native bridge — approximate with open/closed.
#[cfg(target_os = "ios")]
#[no_mangle]
pub extern "C" fn js_ws_ready_state(handle: i64) -> f64 {
    if unsafe { perry_native_ws_is_open(handle as f64) } == 1.0 {
        1.0
    } else {
        3.0
    }
}

/// Get the number of pending messages
/// Returns the count of received messages waiting to be read
#[cfg(not(target_os = "ios"))]
#[no_mangle]
pub extern "C" fn js_ws_message_count(handle: i64) -> f64 {
    let ws_id = handle as usize;

    let guard = WS_CONNECTIONS.lock().unwrap();
    match guard.get(&ws_id) {
        Some(conn) => conn.messages.len() as f64,
        None => 0.0,
    }
}

#[cfg(target_os = "ios")]
#[no_mangle]
pub extern "C" fn js_ws_message_count(handle: i64) -> f64 {
    unsafe { perry_native_ws_message_count(handle as f64) }
}

/// Get the next message from the queue
/// Returns null if no messages available
#[cfg(not(target_os = "ios"))]
#[no_mangle]
pub extern "C" fn js_ws_receive(handle: i64) -> *mut StringHeader {
    let ws_id = handle as usize;

    let mut guard = WS_CONNECTIONS.lock().unwrap();
    match guard.get_mut(&ws_id) {
        Some(conn) => {
            if conn.messages.is_empty() {
                std::ptr::null_mut()
            } else {
                let msg = conn.messages.remove(0);
                js_string_from_bytes(msg.as_ptr(), msg.len() as u32)
            }
        }
        None => std::ptr::null_mut(),
    }
}

#[cfg(target_os = "ios")]
#[no_mangle]
pub extern "C" fn js_ws_receive(handle: i64) -> *mut StringHeader {
    // perry_native_ws_receive returns a NaN-boxed string (f64).
    // We need to return *mut StringHeader. Extract pointer from the f64.
    let val = unsafe { perry_native_ws_receive(handle as f64) };
    let ptr = perry_runtime::js_get_string_pointer_unified(val);
    ptr as *mut StringHeader
}

/// Wait for a message (blocking with timeout)
/// ws.waitForMessage(timeoutMs) -> Promise<string | null>
#[cfg(not(target_os = "ios"))]
#[no_mangle]
pub unsafe extern "C" fn js_ws_wait_for_message(
    handle: i64,
    timeout_ms: f64,
) -> *mut perry_runtime::Promise {
    let promise = perry_runtime::js_promise_new();
    let promise_ptr = promise as usize;
    let ws_id = handle as usize;
    let timeout = std::time::Duration::from_millis(timeout_ms as u64);

    spawn(async move {
        let start = std::time::Instant::now();

        loop {
            // Check for messages
            {
                let mut guard = WS_CONNECTIONS.lock().unwrap();
                if let Some(conn) = guard.get_mut(&ws_id) {
                    if !conn.messages.is_empty() {
                        let msg = conn.messages.remove(0);
                        let result_str = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
                        let result_bits = JSValue::pointer(result_str as *const u8).bits();
                        queue_promise_resolution(promise_ptr, true, result_bits);
                        return;
                    }

                    if !conn.is_open {
                        // Connection closed
                        let result_bits = JSValue::null().bits();
                        queue_promise_resolution(promise_ptr, true, result_bits);
                        return;
                    }
                } else {
                    // Invalid handle
                    let result_bits = JSValue::null().bits();
                    queue_promise_resolution(promise_ptr, true, result_bits);
                    return;
                }
            }

            // Check timeout
            if start.elapsed() >= timeout {
                let result_bits = JSValue::null().bits();
                queue_promise_resolution(promise_ptr, true, result_bits);
                return;
            }

            // Wait a bit before checking again
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    });

    promise
}

// ============================================================================
// WebSocketServer (wss) implementation
// ============================================================================

/// Convert a WS value (f64 bits as i64) to the correct i64 handle.
/// Server handles are NaN-boxed pointers (tag 0x7FFD); client handles are plain f64 numbers.
#[cfg(not(target_os = "ios"))]
#[no_mangle]
pub unsafe extern "C" fn js_ws_handle_to_i64(val_f64: f64) -> i64 {
    let bits = val_f64.to_bits();
    let ptr_tag: u64 = 0x7FFD_0000_0000_0000;
    let mask: u64 = 0xFFFF_0000_0000_0000;
    if (bits & mask) == ptr_tag {
        // NaN-boxed pointer (server handle) — extract raw pointer
        (bits & 0x0000_FFFF_FFFF_FFFF) as i64
    } else {
        // Plain f64 number (client ws_id) — convert to integer
        val_f64 as i64
    }
}

/// Register an event listener on a WebSocket handle (server or client).
/// Unified function: checks handle type at runtime.
///
/// js_ws_on(handle, event_name_ptr, callback_ptr) -> handle
#[cfg(not(target_os = "ios"))]
#[no_mangle]
pub unsafe extern "C" fn js_ws_on(
    handle: i64,
    event_name_ptr: *const StringHeader,
    callback_ptr: i64,
) -> i64 {
    ensure_gc_scanner_registered();
    let event_name = match string_from_header(event_name_ptr) {
        Some(name) => name,
        None => {
            eprintln!(
                "[ws_on] Failed to extract event name from handle={}",
                handle
            );
            return handle;
        }
    };

    if callback_ptr == 0 {
        return handle;
    }

    // Try server handle first
    if let Some(server) = get_handle_mut::<WsServerHandle>(handle) {
        server
            .listeners
            .entry(event_name)
            .or_insert_with(Vec::new)
            .push(callback_ptr);
        return handle;
    }

    // Otherwise treat as client ws_id
    let ws_id = handle as usize;
    let mut guard = WS_CLIENT_LISTENERS.lock().unwrap();
    let entry = guard.entry(ws_id).or_insert_with(|| WsClientListeners {
        listeners: HashMap::new(),
    });
    entry
        .listeners
        .entry(event_name)
        .or_default()
        .push(callback_ptr);

    handle
}

/// Create a new WebSocketServer
/// new WebSocketServer({ port }) -> handle (synchronous, starts listening immediately)
#[cfg(not(target_os = "ios"))]
#[no_mangle]
pub unsafe extern "C" fn js_ws_server_new(opts_f64: f64) -> Handle {
    ensure_gc_scanner_registered();
    // Extract port from options object
    let port = {
        let opts_bits = opts_f64.to_bits();
        // Check if it's a NaN-boxed pointer (object)
        let ptr_tag: u64 = 0x7FFD_0000_0000_0000;
        let mask: u64 = 0xFFFF_0000_0000_0000;
        if (opts_bits & mask) == ptr_tag {
            // Extract raw pointer
            let ptr = (opts_bits & 0x0000_FFFF_FFFF_FFFF) as *const perry_runtime::ObjectHeader;
            if !ptr.is_null() {
                // Get 'port' field
                let key = "port";
                let key_str = js_string_from_bytes(key.as_ptr(), key.len() as u32);
                let val = perry_runtime::js_object_get_field_by_name(ptr, key_str);
                let val_f64 = f64::from_bits(val.bits());
                if val_f64.is_finite() && val_f64 > 0.0 {
                    val_f64 as u16
                } else {
                    0
                }
            } else {
                0
            }
        } else if opts_f64.is_finite() && opts_f64 > 0.0 {
            // Maybe port was passed directly as a number
            opts_f64 as u16
        } else {
            0
        }
    };

    let (shutdown_tx, mut shutdown_rx) = mpsc::unbounded_channel::<()>();

    let server_handle = register_handle(WsServerHandle {
        listeners: HashMap::new(),
        port,
        is_listening: false,
        client_ids: Vec::new(),
        shutdown_tx: Some(shutdown_tx),
    });
    WS_ACTIVE_SERVERS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    // Tokio workers only enqueue raw Rust events here. JS closure dispatch
    // happens later in `js_ws_process_pending` on the main thread, and the
    // listener slots are covered by the ws mutable root scanner.
    // Spawn the accept loop
    let handle_id = server_handle;
    spawn(async move {
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

        // Queue 'listening' event
        push_ws_event(PendingWsEvent::Listening(handle_id));

        // Mark as listening
        if let Some(server) = get_handle_mut::<WsServerHandle>(handle_id) {
            server.is_listening = true;
        }

        loop {
            tokio::select! {
                accept_result = listener.accept() => {
                    match accept_result {
                        Ok((tcp_stream, _addr)) => {
                            // Upgrade to WebSocket
                            match tokio_tungstenite::accept_async(tcp_stream).await {
                                Ok(ws_stream) => {
                                    let (mut write, mut read) = ws_stream.split();
                                    let (tx, mut rx) = mpsc::unbounded_channel::<WsCommand>();

                                    // Allocate client ID
                                    let mut id_guard = NEXT_WS_ID.lock().unwrap();
                                    let ws_id = *id_guard;
                                    *id_guard += 1;
                                    drop(id_guard);

                                    // Store connection
                                    WS_CONNECTIONS.lock().unwrap().insert(ws_id, WsConnection {
                                        sender: tx,
                                        messages: Vec::new(),
                                        is_open: true,
                                        is_closing: false,
                                        is_closed: false,
                                    });

                                    // Initialize client listeners
                                    WS_CLIENT_LISTENERS.lock().unwrap().insert(ws_id, WsClientListeners {
                                        listeners: HashMap::new(),
                                    });

                                    // Track client on server and record parent relationship
                                    if let Some(server) = get_handle_mut::<WsServerHandle>(handle_id) {
                                        server.client_ids.push(ws_id);
                                    }
                                    WS_CLIENT_PARENT_SERVER.lock().unwrap().insert(ws_id, handle_id);

                                    // Queue 'connection' event
                                    push_ws_event(
                                        PendingWsEvent::Connection(handle_id, ws_id)
                                    );

                                    // Single task handles both read and write (avoids BiLock split issue)
                                    let ws_id_io = ws_id;
                                    ws_file_log(&format!("[WS-srv] spawning io task for id={}", ws_id_io));
                                    tokio::spawn(async move {
                                        loop {
                                            tokio::select! {
                                                msg_result = read.next() => {
                                                    match msg_result {
                                                        Some(Ok(Message::Text(text))) => {
                                                            let text = text.to_string();
                                                            ws_file_log(&format!("[WS-srv-io] id={} recv len={}", ws_id_io, text.len()));
                                                            push_ws_event(
                                                                PendingWsEvent::Message(ws_id_io, text)
                                                            );
                                                        }
                                                        Some(Ok(Message::Binary(data))) => {
                                                            let text = String::from_utf8_lossy(&data).to_string();
                                                            push_ws_event(
                                                                PendingWsEvent::Message(ws_id_io, text)
                                                            );
                                                        }
                                                        Some(Ok(Message::Close(frame))) => {
                                                            let (code, reason) = frame
                                                                .map(|f| (f.code.into(), f.reason.to_string()))
                                                                .unwrap_or((1000u16, String::new()));
                                                            mark_ws_connection_closed(ws_id_io);
                                                            push_ws_event(
                                                                PendingWsEvent::Close(ws_id_io, code, reason)
                                                            );
                                                            break;
                                                        }
                                                        Some(Err(e)) => {
                                                            mark_ws_connection_closed(ws_id_io);
                                                            push_ws_event(
                                                                PendingWsEvent::Error(ws_id_io, format!("{}", e))
                                                            );
                                                            push_ws_event(
                                                                PendingWsEvent::Close(ws_id_io, 1006, String::new())
                                                            );
                                                            break;
                                                        }
                                                        Some(Ok(_)) => {}
                                                        None => {
                                                            if mark_ws_connection_closed(ws_id_io) {
                                                                push_ws_event(
                                                                    PendingWsEvent::Close(ws_id_io, 1000, String::new())
                                                                );
                                                            }
                                                            break;
                                                        }
                                                    }
                                                }
                                                cmd = rx.recv() => {
                                                    match cmd {
                                                        Some(WsCommand::Send(msg)) => {
                                                            ws_file_log(&format!("[WS-srv-io] id={} sending len={}", ws_id_io, msg.len()));
                                                            match write.send(Message::Text(msg.into())).await {
                                                                Ok(_) => {
                                                                    ws_file_log(&format!("[WS-srv-io] id={} send OK", ws_id_io));
                                                                }
                                                                Err(e) => {
                                                                    ws_file_log(&format!("[WS-srv-io] id={} send ERR: {}", ws_id_io, e));
                                                                    if mark_ws_connection_closed(ws_id_io) {
                                                                        push_ws_event(
                                                                            PendingWsEvent::Error(ws_id_io, format!("{}", e))
                                                                        );
                                                                        push_ws_event(
                                                                            PendingWsEvent::Close(ws_id_io, 1006, String::new())
                                                                        );
                                                                    }
                                                                    break;
                                                                }
                                                            }
                                                        }
                                                        Some(WsCommand::Close) => {
                                                            ws_file_log(&format!("[WS-srv-io] id={} closing", ws_id_io));
                                                            let _ = write.send(Message::Close(None)).await;
                                                            if mark_ws_connection_closed(ws_id_io) {
                                                                push_ws_event(
                                                                    PendingWsEvent::Close(ws_id_io, 1000, String::new())
                                                                );
                                                            }
                                                            break;
                                                        }
                                                        None => {
                                                            if mark_ws_connection_closed(ws_id_io) {
                                                                push_ws_event(
                                                                    PendingWsEvent::Close(ws_id_io, 1000, String::new())
                                                                );
                                                            }
                                                            break;
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                        mark_ws_connection_closed(ws_id_io);
                                    });
                                }
                                Err(e) => {
                                    push_ws_event(
                                        PendingWsEvent::ServerError(handle_id, format!("WebSocket accept error: {}", e))
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            push_ws_event(
                                PendingWsEvent::ServerError(handle_id, format!("TCP accept error: {}", e))
                            );
                        }
                    }
                }
                _ = shutdown_rx.recv() => {
                    // Shutdown signal received
                    break;
                }
            }
        }
    });

    server_handle
}

/// Close the WebSocketServer and all its client connections
/// wss.close(callback?) -> void
#[cfg(not(target_os = "ios"))]
#[no_mangle]
pub unsafe extern "C" fn js_ws_server_close(handle: i64) {
    WS_ACTIVE_SERVERS.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
    if let Some(server) = get_handle_mut::<WsServerHandle>(handle) {
        server.is_listening = false;

        // Send shutdown signal
        if let Some(tx) = server.shutdown_tx.take() {
            let _ = tx.send(());
        }

        // Close all client connections
        let client_ids: Vec<usize> = server.client_ids.clone();
        for ws_id in client_ids {
            let guard = WS_CONNECTIONS.lock().unwrap();
            if let Some(conn) = guard.get(&ws_id) {
                let _ = conn.sender.send(WsCommand::Close);
            }
        }
    }
}

/// Returns 1 if there are active WS servers or connections that need
/// the event loop to keep running.
#[cfg(not(target_os = "ios"))]
pub fn js_ws_has_active_handles() -> i32 {
    // Check the active-server counter (set in js_ws_server_new)
    if WS_ACTIVE_SERVERS.load(std::sync::atomic::Ordering::Relaxed) > 0 {
        return 1;
    }
    // Check for active connections
    let conns = WS_CONNECTIONS.lock().unwrap();
    if !conns.is_empty() {
        return 1;
    }
    // Check for pending events
    let pending = WS_PENDING_EVENTS.lock().unwrap();
    if !pending.is_empty() {
        return 1;
    }
    0
}

#[cfg(target_os = "ios")]
pub fn js_ws_has_active_handles() -> i32 {
    0
}

/// Process pending WebSocket events (called from js_stdlib_process_pending)
/// Drains the event queue and invokes closures on the main thread.
/// Returns number of events processed.
///
/// #1114 followup: same per-tick scratch-Vec discipline as the fastify
/// (e538caa7) and net (this PR) pumps. Called every event-loop iteration
/// + every inline `await` poll iteration; the original
/// `Vec::drain(..).collect()` was a per-call heap alloc that contributed
/// to the GC `madvise` churn observed under shop-admin's realtime WS
/// broker + JobLoop combo. Reuse a per-thread scratch buffer (moved out
/// across dispatch so a re-entrant pump from inside a user callback is
/// safe).
#[cfg(not(target_os = "ios"))]
#[no_mangle]
pub unsafe extern "C" fn js_ws_process_pending() -> i32 {
    thread_local! {
        static SCRATCH: std::cell::RefCell<Vec<PendingWsEvent>> =
            const { std::cell::RefCell::new(Vec::new()) };
    }
    let mut events = SCRATCH.with(|s| std::mem::take(&mut *s.borrow_mut()));
    events.clear();
    {
        let mut guard = WS_PENDING_EVENTS.lock().unwrap();
        events.append(&mut *guard);
    }

    let count = events.len() as i32;

    for event in events.drain(..) {
        match event {
            PendingWsEvent::Connection(server_handle, client_ws_id) => {
                // Get 'connection' listeners from server
                let listeners: Vec<i64> = get_handle_mut::<WsServerHandle>(server_handle)
                    .and_then(|s| s.listeners.get("connection").cloned())
                    .unwrap_or_default();

                // Pass ws_id as a regular f64 number (not NaN-boxed) so === comparison works
                let client_handle_f64 = client_ws_id as f64;

                for cb in listeners {
                    if cb != 0 {
                        let closure = cb as *const ClosureHeader;
                        js_closure_call1(closure, client_handle_f64);
                    }
                }
            }
            PendingWsEvent::Message(ws_id, message) => {
                // Get 'message' listeners from client
                let listeners: Vec<i64> = {
                    let guard = WS_CLIENT_LISTENERS.lock().unwrap();
                    guard
                        .get(&ws_id)
                        .and_then(|l| l.listeners.get("message").cloned())
                        .unwrap_or_default()
                };

                // Create string on main thread and NaN-box with STRING_TAG
                let msg_str = js_string_from_bytes(message.as_ptr(), message.len() as u32);
                let msg_f64 = f64::from_bits(
                    0x7FFF_0000_0000_0000u64 | (msg_str as u64 & 0x0000_FFFF_FFFF_FFFF),
                );

                if !listeners.is_empty() {
                    for cb in listeners {
                        if cb != 0 {
                            let closure = cb as *const ClosureHeader;
                            js_closure_call1(closure, msg_f64);
                        }
                    }
                } else {
                    // Fall through to parent server's 'message' listeners (ws, data)
                    let parent = WS_CLIENT_PARENT_SERVER.lock().unwrap().get(&ws_id).copied();
                    if let Some(server_handle) = parent {
                        let server_listeners: Vec<i64> =
                            get_handle_mut::<WsServerHandle>(server_handle)
                                .and_then(|s| s.listeners.get("message").cloned())
                                .unwrap_or_default();
                        // Pass ws_id as regular f64 number (not NaN-boxed) so === comparison works
                        let client_handle_f64 = ws_id as f64;
                        for cb in server_listeners {
                            if cb != 0 {
                                let closure = cb as *const ClosureHeader;
                                js_closure_call2(closure, client_handle_f64, msg_f64);
                            }
                        }
                    }
                }
            }
            PendingWsEvent::Close(ws_id, _code, _reason) => {
                let listeners: Vec<i64> = {
                    let guard = WS_CLIENT_LISTENERS.lock().unwrap();
                    guard
                        .get(&ws_id)
                        .and_then(|l| l.listeners.get("close").cloned())
                        .unwrap_or_default()
                };

                if !listeners.is_empty() {
                    for cb in listeners {
                        if cb != 0 {
                            let closure = cb as *const ClosureHeader;
                            js_closure_call0(closure);
                        }
                    }
                } else {
                    // Fall through to parent server's 'close' listeners (ws)
                    let parent = WS_CLIENT_PARENT_SERVER.lock().unwrap().get(&ws_id).copied();
                    if let Some(server_handle) = parent {
                        let server_listeners: Vec<i64> =
                            get_handle_mut::<WsServerHandle>(server_handle)
                                .and_then(|s| s.listeners.get("close").cloned())
                                .unwrap_or_default();
                        let client_handle_f64 = ws_id as f64;
                        for cb in server_listeners {
                            if cb != 0 {
                                let closure = cb as *const ClosureHeader;
                                js_closure_call1(closure, client_handle_f64);
                            }
                        }
                    }
                }

                cleanup_ws_client(ws_id);
            }
            PendingWsEvent::Error(ws_id, error_msg) => {
                let listeners: Vec<i64> = {
                    let guard = WS_CLIENT_LISTENERS.lock().unwrap();
                    guard
                        .get(&ws_id)
                        .and_then(|l| l.listeners.get("error").cloned())
                        .unwrap_or_default()
                };

                let err_str = js_string_from_bytes(error_msg.as_ptr(), error_msg.len() as u32);
                let err_f64 = f64::from_bits(
                    0x7FFF_0000_0000_0000u64 | (err_str as u64 & 0x0000_FFFF_FFFF_FFFF),
                );

                if !listeners.is_empty() {
                    for cb in listeners {
                        if cb != 0 {
                            let closure = cb as *const ClosureHeader;
                            js_closure_call1(closure, err_f64);
                        }
                    }
                } else {
                    // Fall through to parent server's 'error' listeners (ws, error)
                    let parent = WS_CLIENT_PARENT_SERVER.lock().unwrap().get(&ws_id).copied();
                    if let Some(server_handle) = parent {
                        let server_listeners: Vec<i64> =
                            get_handle_mut::<WsServerHandle>(server_handle)
                                .and_then(|s| s.listeners.get("client_error").cloned())
                                .unwrap_or_default();
                        let client_handle_f64 = ws_id as f64;
                        for cb in server_listeners {
                            if cb != 0 {
                                let closure = cb as *const ClosureHeader;
                                js_closure_call2(closure, client_handle_f64, err_f64);
                            }
                        }
                    }
                }
            }
            PendingWsEvent::ServerError(server_handle, error_msg) => {
                let listeners: Vec<i64> = get_handle_mut::<WsServerHandle>(server_handle)
                    .and_then(|s| s.listeners.get("error").cloned())
                    .unwrap_or_default();

                let err_str = js_string_from_bytes(error_msg.as_ptr(), error_msg.len() as u32);
                let err_f64 = f64::from_bits(
                    0x7FFF_0000_0000_0000u64 | (err_str as u64 & 0x0000_FFFF_FFFF_FFFF),
                );

                for cb in listeners {
                    if cb != 0 {
                        let closure = cb as *const ClosureHeader;
                        js_closure_call1(closure, err_f64);
                    }
                }
            }
            PendingWsEvent::Listening(server_handle) => {
                let listeners: Vec<i64> = get_handle_mut::<WsServerHandle>(server_handle)
                    .and_then(|s| s.listeners.get("listening").cloned())
                    .unwrap_or_default();

                for cb in listeners {
                    if cb != 0 {
                        let closure = cb as *const ClosureHeader;
                        js_closure_call0(closure);
                    }
                }
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

/// iOS: no-op since native WebSocket handles events via NSURLSession callbacks
#[cfg(target_os = "ios")]
#[no_mangle]
pub unsafe extern "C" fn js_ws_process_pending() -> i32 {
    0
}

#[cfg(all(test, not(target_os = "ios")))]
mod tests {
    use super::*;

    lazy_static::lazy_static! {
        static ref TEST_LOCK: Mutex<()> = Mutex::new(());
    }

    fn clear_test_state() {
        WS_CONNECTIONS.lock().unwrap().clear();
        WS_CLIENT_LISTENERS.lock().unwrap().clear();
        WS_CLIENT_PARENT_SERVER.lock().unwrap().clear();
        WS_PENDING_EVENTS.lock().unwrap().clear();
        WS_ACTIVE_SERVERS.store(0, std::sync::atomic::Ordering::Relaxed);
    }

    #[test]
    fn root_scanner_emits_client_and_server_listeners() {
        let _guard = TEST_LOCK.lock().unwrap();
        clear_test_state();

        {
            let mut clients = WS_CLIENT_LISTENERS.lock().unwrap();
            clients.insert(
                42,
                WsClientListeners {
                    listeners: HashMap::from([("message".to_string(), vec![0x1234_5678])]),
                },
            );
        }
        let server_handle = register_handle(WsServerHandle {
            listeners: HashMap::from([("connection".to_string(), vec![0x2345_6780])]),
            port: 0,
            is_listening: false,
            client_ids: Vec::new(),
            shutdown_tx: None,
        });

        let mut emitted = Vec::new();
        scan_ws_roots(&mut |value| emitted.push(value.to_bits()));

        assert!(emitted.contains(&(0x7FFD_0000_0000_0000 | 0x1234_5678)));
        assert!(emitted.contains(&(0x7FFD_0000_0000_0000 | 0x2345_6780)));
        crate::common::drop_handle(server_handle);
        clear_test_state();
    }

    #[test]
    fn close_event_releases_server_client_bookkeeping() {
        let _guard = TEST_LOCK.lock().unwrap();
        clear_test_state();

        let client_id = 77usize;
        let (tx, _rx) = mpsc::unbounded_channel::<WsCommand>();
        let server_handle = register_handle(WsServerHandle {
            listeners: HashMap::new(),
            port: 0,
            is_listening: false,
            client_ids: vec![client_id],
            shutdown_tx: None,
        });

        WS_CONNECTIONS.lock().unwrap().insert(
            client_id,
            WsConnection {
                sender: tx,
                messages: Vec::new(),
                is_open: false,
                is_closing: false,
                is_closed: false,
            },
        );
        WS_CLIENT_LISTENERS.lock().unwrap().insert(
            client_id,
            WsClientListeners {
                listeners: HashMap::new(),
            },
        );
        WS_CLIENT_PARENT_SERVER
            .lock()
            .unwrap()
            .insert(client_id, server_handle);
        WS_PENDING_EVENTS
            .lock()
            .unwrap()
            .push(PendingWsEvent::Close(client_id, 1000, String::new()));

        assert_eq!(js_ws_has_active_handles(), 1);
        let processed = unsafe { js_ws_process_pending() };
        assert_eq!(processed, 1);

        assert!(!WS_CONNECTIONS.lock().unwrap().contains_key(&client_id));
        assert!(!WS_CLIENT_LISTENERS.lock().unwrap().contains_key(&client_id));
        assert!(!WS_CLIENT_PARENT_SERVER
            .lock()
            .unwrap()
            .contains_key(&client_id));
        assert!(get_handle_mut::<WsServerHandle>(server_handle)
            .unwrap()
            .client_ids
            .is_empty());
        assert_eq!(js_ws_has_active_handles(), 0);

        crate::common::drop_handle(server_handle);
        clear_test_state();
    }

    /// #6117 — `readyState` walks the npm-ws lifecycle: CONNECTING (0)
    /// pre-open, OPEN (1), CLOSING (2) after `close()` is requested,
    /// CLOSED (3) once the IO loop marks the connection dead, and CLOSED
    /// for ids with no entry (cleaned up, or promise-path connect failed).
    #[test]
    fn ready_state_reports_npm_ws_lifecycle() {
        let _guard = TEST_LOCK.lock().unwrap();
        clear_test_state();

        let ws_id = 91usize;
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
        unsafe { js_ws_close(ws_id as i64) };
        assert_eq!(js_ws_ready_state(ws_id as i64), 2.0);
        mark_ws_connection_closed(ws_id);
        assert_eq!(js_ws_ready_state(ws_id as i64), 3.0);
        cleanup_ws_client(ws_id);
        assert_eq!(js_ws_ready_state(ws_id as i64), 3.0);

        clear_test_state();
    }
}
