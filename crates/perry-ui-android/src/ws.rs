//! WebSocket implementation for Android using tungstenite + rustls.
//!
//! Provides the `js_ws_*` functions that the Perry runtime needs for WebSocket
//! support. Uses a background thread per connection for async I/O, with
//! message queues for the poll-based API used by TypeScript (sync-transport.ts).

use std::collections::VecDeque;
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use perry_runtime::promise::Promise;
use perry_runtime::string::{js_string_from_bytes, StringHeader};

use tungstenite::stream::MaybeTlsStream;
use tungstenite::{connect, Message, WebSocket};

extern "C" {
    fn __android_log_print(prio: i32, tag: *const u8, fmt: *const u8, ...) -> i32;
}

fn ws_log(msg: &str) {
    let c_msg = std::ffi::CString::new(msg).unwrap_or_default();
    unsafe {
        __android_log_print(3, b"PerryWS\0".as_ptr(), b"%s\0".as_ptr(), c_msg.as_ptr());
    }
}

/// A WebSocket connection managed by a background thread.
struct WsConnection {
    /// Messages received from the server, waiting to be polled by TypeScript.
    messages: Mutex<VecDeque<String>>,
    /// Pending messages to send (queued by TypeScript, drained by the IO thread).
    pending_sends: Mutex<Vec<String>>,
    /// Whether the connection is open and ready.
    is_open: AtomicBool,
    /// Whether a close has been requested.
    close_requested: AtomicBool,
}

/// Global connection storage. Index = ws_id - 1.
static CONNECTIONS: Mutex<Vec<Option<std::sync::Arc<WsConnection>>>> = Mutex::new(Vec::new());
static NEXT_WS_ID: AtomicUsize = AtomicUsize::new(1);

/// A Send wrapper for *mut Promise (raw pointer is not Send by default).
/// SAFETY: Promise pointers are created on the event loop thread and resolved on the
/// same thread. The background thread only stores the pointer; it never dereferences it.
struct SendPromise(*mut Promise);
unsafe impl Send for SendPromise {}

/// Pending promise resolutions from background threads.
/// The pump tick drains this and calls js_promise_resolve on the main thread.
static PENDING_RESOLVES: Mutex<Vec<(SendPromise, f64)>> = Mutex::new(Vec::new());

/// Extract a Rust &str from a Perry StringHeader pointer.
fn str_from_header(ptr: *const StringHeader) -> Option<&'static str> {
    if ptr.is_null() {
        return None;
    }
    unsafe {
        let header = ptr as *const perry_runtime::string::StringHeader;
        let len = (*header).byte_len as usize;
        let data =
            (ptr as *const u8).add(std::mem::size_of::<perry_runtime::string::StringHeader>());
        Some(std::str::from_utf8_unchecked(std::slice::from_raw_parts(
            data, len,
        )))
    }
}

/// Set the read timeout on the underlying TcpStream (works for both plain and TLS).
fn set_read_timeout(ws: &WebSocket<MaybeTlsStream<TcpStream>>, timeout: Option<Duration>) {
    match ws.get_ref() {
        MaybeTlsStream::Plain(tcp) => {
            let _ = tcp.set_read_timeout(timeout);
        }
        MaybeTlsStream::Rustls(tls_stream) => {
            let _ = tls_stream.get_ref().set_read_timeout(timeout);
        }
        _ => {}
    }
}

/// Background IO thread: reads messages and processes pending sends.
fn io_thread(mut ws: WebSocket<MaybeTlsStream<TcpStream>>, conn: std::sync::Arc<WsConnection>) {
    // Set a short read timeout so we can periodically check for pending sends
    set_read_timeout(&ws, Some(Duration::from_millis(50)));

    loop {
        // Check for close request
        if conn.close_requested.load(Ordering::Relaxed) {
            let _ = ws.close(None);
            // Drain any remaining close frames
            loop {
                match ws.read() {
                    Ok(Message::Close(_)) | Err(_) => break,
                    _ => {}
                }
            }
            conn.is_open.store(false, Ordering::Relaxed);
            ws_log("io_thread: closed by request");
            return;
        }

        // Process pending sends
        {
            let mut sends = conn.pending_sends.lock().unwrap();
            for msg in sends.drain(..) {
                if let Err(e) = ws.send(Message::Text(msg)) {
                    ws_log(&format!("io_thread: send error: {}", e));
                    conn.is_open.store(false, Ordering::Relaxed);
                    return;
                }
            }
        }

        // Try to read a message (with 50ms timeout)
        match ws.read() {
            Ok(Message::Text(text)) => {
                conn.messages.lock().unwrap().push_back(text);
            }
            Ok(Message::Close(_)) => {
                ws_log("io_thread: server closed connection");
                conn.is_open.store(false, Ordering::Relaxed);
                return;
            }
            Ok(Message::Ping(_)) | Ok(Message::Pong(_)) | Ok(Message::Frame(_)) => {
                // Handled internally by tungstenite
            }
            Ok(Message::Binary(_)) => {
                // Ignore binary messages
            }
            Err(tungstenite::Error::Io(ref e))
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                // Timeout — normal, loop back to check sends
            }
            Err(e) => {
                ws_log(&format!("io_thread: read error: {}", e));
                conn.is_open.store(false, Ordering::Relaxed);
                return;
            }
        }
    }
}

/// Core connection logic shared by js_ws_connect and js_ws_connect_start.
/// Returns the ws_id. If `promise` is Some, it will be resolved with the ws_id
/// once the connection is established (or rejected on failure).
fn start_connection(url: String, promise: Option<SendPromise>) -> usize {
    let ws_id = NEXT_WS_ID.fetch_add(1, Ordering::Relaxed);

    let conn = std::sync::Arc::new(WsConnection {
        messages: Mutex::new(VecDeque::new()),
        pending_sends: Mutex::new(Vec::new()),
        is_open: AtomicBool::new(false),
        close_requested: AtomicBool::new(false),
    });

    // Store in global map
    {
        let mut conns = CONNECTIONS.lock().unwrap();
        let idx = ws_id - 1;
        while conns.len() <= idx {
            conns.push(None);
        }
        conns[idx] = Some(conn.clone());
    }

    // Spawn connection thread
    let conn_clone = conn.clone();
    std::thread::Builder::new()
        .name(format!("ws-{}", ws_id))
        .spawn(move || {
            ws_log(&format!("ws-{}: connecting to {}", ws_id, &url));
            match connect(&url) {
                Ok((ws_stream, _response)) => {
                    ws_log(&format!("ws-{}: connected!", ws_id));
                    conn_clone.is_open.store(true, Ordering::Relaxed);

                    // Resolve promise if provided
                    if let Some(SendPromise(p)) = promise {
                        PENDING_RESOLVES
                            .lock()
                            .unwrap()
                            .push((SendPromise(p), ws_id as f64));
                    }

                    io_thread(ws_stream, conn_clone);
                }
                Err(e) => {
                    ws_log(&format!("ws-{}: connect error: {}", ws_id, e));
                    // Resolve promise with 0 (failure) so .then() can detect
                    if let Some(SendPromise(p)) = promise {
                        PENDING_RESOLVES.lock().unwrap().push((SendPromise(p), 0.0));
                    }
                }
            }
        })
        .ok();

    ws_id
}

// =============================================================================
// Public API (js_ws_* functions called from compiled TypeScript)
// =============================================================================

/// Create a WebSocket connection (Promise-based API).
/// Called by `new WebSocket(url)` in TypeScript.
/// Returns a Promise that resolves with the ws_id when connected.
#[no_mangle]
pub extern "C" fn js_ws_connect(url_ptr: *const StringHeader) -> *mut Promise {
    ws_log("js_ws_connect called");

    let url = match str_from_header(url_ptr) {
        Some(u) => u.to_string(),
        None => {
            ws_log("js_ws_connect: null URL");
            return std::ptr::null_mut();
        }
    };

    ws_log(&format!("js_ws_connect: url={}", &url));

    // Create a Promise to resolve once connected
    let promise = perry_runtime::js_promise_new();

    start_connection(url, Some(SendPromise(promise)));

    promise
}

/// Create a WebSocket connection (handle-based API).
/// Returns a handle (ws_id) immediately.
/// The connection happens in a background thread. Use `js_ws_is_open` to check.
#[no_mangle]
pub unsafe extern "C" fn js_ws_connect_start(url_nanboxed: f64) -> f64 {
    ws_log("js_ws_connect_start called");

    let url_ptr = perry_runtime::js_get_string_pointer_unified(url_nanboxed) as *const StringHeader;
    let url = match str_from_header(url_ptr) {
        Some(u) => u.to_string(),
        None => {
            ws_log("js_ws_connect_start: null URL");
            return 0.0;
        }
    };

    ws_log(&format!("js_ws_connect_start: url={}", &url));

    start_connection(url, None) as f64
}

/// Check if a WebSocket connection is open.
#[no_mangle]
pub extern "C" fn js_ws_is_open(handle: i64) -> f64 {
    let idx = handle as usize;
    if idx < 1 {
        return 0.0;
    }
    let conns = CONNECTIONS.lock().unwrap();
    if let Some(Some(conn)) = conns.get(idx - 1) {
        if conn.is_open.load(Ordering::Relaxed) {
            1.0
        } else {
            0.0
        }
    } else {
        0.0
    }
}

/// Send a text message on a WebSocket connection.
#[no_mangle]
pub extern "C" fn js_ws_send(handle: i64, message_ptr: *const StringHeader) {
    let idx = handle as usize;
    if idx < 1 || message_ptr.is_null() {
        return;
    }
    let msg = match str_from_header(message_ptr) {
        Some(s) => s.to_string(),
        None => return,
    };

    let conns = CONNECTIONS.lock().unwrap();
    if let Some(Some(conn)) = conns.get(idx - 1) {
        conn.pending_sends.lock().unwrap().push(msg);
    }
}

/// Close a WebSocket connection.
#[no_mangle]
pub extern "C" fn js_ws_close(handle: i64) {
    let idx = handle as usize;
    if idx < 1 {
        return;
    }
    ws_log(&format!("js_ws_close: handle={}", handle));
    let conns = CONNECTIONS.lock().unwrap();
    if let Some(Some(conn)) = conns.get(idx - 1) {
        conn.close_requested.store(true, Ordering::Relaxed);
    }
}

/// Get the number of queued received messages.
#[no_mangle]
pub extern "C" fn js_ws_message_count(handle: i64) -> f64 {
    let idx = handle as usize;
    if idx < 1 {
        return 0.0;
    }
    let conns = CONNECTIONS.lock().unwrap();
    if let Some(Some(conn)) = conns.get(idx - 1) {
        conn.messages.lock().unwrap().len() as f64
    } else {
        0.0
    }
}

/// Dequeue one received message. Returns a Perry StringHeader pointer.
#[no_mangle]
pub extern "C" fn js_ws_receive(handle: i64) -> *mut StringHeader {
    let idx = handle as usize;
    if idx < 1 {
        return std::ptr::null_mut();
    }
    let conns = CONNECTIONS.lock().unwrap();
    if let Some(Some(conn)) = conns.get(idx - 1) {
        if let Some(msg) = conn.messages.lock().unwrap().pop_front() {
            js_string_from_bytes(msg.as_ptr(), msg.len() as u32)
        } else {
            std::ptr::null_mut()
        }
    } else {
        std::ptr::null_mut()
    }
}

/// Wait for a message (not used by sync-transport.ts polling model).
#[no_mangle]
pub extern "C" fn js_ws_wait_for_message(_handle: i64, _timeout_ms: f64) -> *mut Promise {
    std::ptr::null_mut()
}

/// Register an event listener (not used by sync-transport.ts polling model).
#[no_mangle]
pub extern "C" fn js_ws_on(
    _handle: i64,
    _event_name_ptr: *const StringHeader,
    _callback_ptr: i64,
) -> i64 {
    0
}

/// Create a WebSocket server (not needed on Android).
#[no_mangle]
pub extern "C" fn js_ws_server_new(_opts_f64: f64) -> i64 {
    0
}

/// Close a WebSocket server (not needed on Android).
#[no_mangle]
pub extern "C" fn js_ws_server_close(_handle: i64) {}

/// Process pending WebSocket events.
/// Resolves promises for completed connections.
#[no_mangle]
pub extern "C" fn js_ws_process_pending() -> i32 {
    let pending: Vec<(SendPromise, f64)> = {
        let mut q = PENDING_RESOLVES.lock().unwrap();
        q.drain(..).collect()
    };

    let count = pending.len() as i32;
    for (SendPromise(promise), value) in pending {
        perry_runtime::js_promise_resolve(promise, value);
    }
    count
}

/// Called from the pump timer on every tick.
/// Processes pending WebSocket promise resolves.
#[no_mangle]
pub extern "C" fn js_stdlib_process_pending() -> i32 {
    js_ws_process_pending()
}

/// Init dispatch (no-op on Android).
#[no_mangle]
pub extern "C" fn js_stdlib_init_dispatch() {}
