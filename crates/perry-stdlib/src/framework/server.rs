//! The bundled HTTP server of Perry's native framework.
//!
//! # Transport
//!
//! turnloop, through [`perry_http_server`] — the same HTTP/1.1 core
//! `perry-ext-fastify` serves on. It replaced a hyper service: a
//! `tokio::spawn`ed accept loop, one `tokio::spawn` per connection, an `mpsc`
//! carrying requests to the main thread and a `oneshot` carrying each response
//! back. None of that remains; the codec runs on the thread that owns the
//! loop, and a response encodes and submits its own write.
//!
//! This is the bundled implementation — the copy the well-known flip compiles
//! *out* when `import 'http'` routes to `perry-ext-http`. It cannot depend on
//! that wrapper (it exists to be its fallback), which is why the server core
//! lives in a crate below both. See `perry_http_server`'s module header.
//!
//! # The API is blocking, and that is the whole shape
//!
//! `js_http_server_accept_v2` blocks until a request arrives, because that is
//! what the documented surface promises (`docs/native-libraries.md`, "HTTP
//! Server (Low-Level API)"): a `while (true)` loop that accepts, reads and
//! responds. Under tokio the block was `RUNTIME.block_on`, with hyper's accept
//! loop running on the worker pool. Under turnloop nothing else can drive the
//! completions, so the block *is* the event loop: it pumps and parks through
//! `js_wait_for_event`, which turns the agent's loop and dispatches
//! completions. A request therefore reaches the queue inside the same call
//! that is waiting for it.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use perry_runtime::{js_string_from_bytes, StringHeader};

extern "C" {
    /// perry-runtime's `stdlib_pump::js_run_stdlib_pump` — `pub(crate)` there,
    /// exported under `#[no_mangle]`, so it is reached by symbol.
    fn js_run_stdlib_pump();
}

use crate::common::{
    get_handle, register_handle, string_from_header_lossy as string_from_header, Handle,
};

/// This crate's second completion-sink slot. `perry-ext-net` owns 0,
/// `perry-ext-http` 1, this crate's turnloop HTTP client 2 and its SMTP client
/// 3, `perry-ext-fastify` 4.
const SUBSYSTEM: u8 = 5;

/// Request ID counter.
static REQUEST_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Pending request waiting for a response.
pub struct PendingRequest {
    pub id: u64,
    pub method: String,
    pub path: String,
    pub headers: HashMap<String, String>,
    pub body: Option<Vec<u8>>,
    /// The exchange this answers, on the connection that carried it.
    pub conn_id: i64,
    pub seq: u64,
}

/// HTTP response to send back.
pub struct HttpResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

/// Where one request's response goes: a live `perry_http_server` exchange.
///
/// This replaced a `oneshot::Sender<HttpResponse>`, and keeps its shape
/// (`send` consuming self, handing the response back when the peer is gone) so
/// every `js_http_respond_*` in `response.rs` reads the same.
pub struct ResponseSlot {
    conn_id: i64,
    seq: u64,
}

impl ResponseSlot {
    pub fn send(self, response: HttpResponse) -> Result<(), HttpResponse> {
        perry_http_server::respond(
            self.conn_id,
            self.seq,
            perry_http_server::Response {
                status: response.status,
                status_message: None,
                headers: with_content_length(&response.headers, response.body.len()),
                body: response.body,
                trailers: Vec::new(),
                auto_content_length: true,
            },
        );
        Ok(())
    }
}

/// The response headers plus the `Content-Length` this API never set itself.
///
/// hyper's `Full<Bytes>` supplied one; the core supplies none, because a
/// caller that streams must not have one invented. `auto_content_length: true`
/// beside this is what tells the core the length is *ours* — so a 204, a 304,
/// a 1xx or a HEAD response drops it exactly where Node sends none, while a
/// length the caller set survives.
fn with_content_length(
    headers: &HashMap<String, String>,
    body_len: usize,
) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = headers
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    if !out
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("content-length"))
    {
        out.push(("content-length".to_string(), body_len.to_string()));
    }
    // Node's HTTP server sends `Date` on every response; hyper supplied a
    // lowercase `date` here. After the length and before the core appends
    // `Connection`/`Keep-Alive`, which is the order Node emits.
    if !out.iter().any(|(k, _)| k.eq_ignore_ascii_case("date")) {
        out.push(("Date".to_string(), perry_http_server::wire::http_date_now()));
    }
    out
}

/// HTTP Server handle.
pub struct HttpServerHandle {
    pub port: u16,
    pub listener_id: i64,
}

/// Request handle for TypeScript access.
pub struct RequestHandle {
    pub id: u64,
    pub method: String,
    pub path: String,
    pub query: String,
    pub headers: HashMap<String, String>,
    pub body: Option<Vec<u8>>,
}

/// Decoded requests waiting for `js_http_server_accept*`, per listener.
///
/// A plain queue rather than a channel: the sink that fills it and the accept
/// that drains it are the same thread.
fn queues() -> &'static Mutex<HashMap<i64, VecDeque<PendingRequest>>> {
    static QUEUES: OnceLock<Mutex<HashMap<i64, VecDeque<PendingRequest>>>> = OnceLock::new();
    QUEUES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The [`perry_http_server::Host`] this server installs. It runs inside the
/// completion sink, so it queues and does nothing else — in particular it runs
/// no JS, which is the rule that keeps the event-loop phase order intact.
struct FrameworkHost {
    listener_id: Arc<Mutex<i64>>,
}

impl perry_http_server::Host for FrameworkHost {
    fn on_request(&self, request: perry_http_server::Request) {
        let listener_id = *self.listener_id.lock().unwrap_or_else(|e| e.into_inner());
        if listener_id == 0 {
            // The listener id is written the instant `listen` returns, and the
            // first completion cannot arrive before then — a connection is
            // only accepted on a later turn. Refusing here rather than
            // silently dropping keeps that assumption checkable.
            perry_http_server::respond(
                request.conn_id,
                request.seq,
                perry_http_server::Response {
                    status: 503,
                    headers: vec![("content-length".to_string(), "0".to_string())],
                    ..Default::default()
                },
            );
            return;
        }
        let id = REQUEST_ID_COUNTER.fetch_add(1, Ordering::SeqCst);
        let headers = request.headers.iter().cloned().collect();
        let body = if request.body.is_empty() {
            None
        } else {
            Some(request.body)
        };
        queues()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(listener_id)
            .or_default()
            .push_back(PendingRequest {
                id,
                method: request.method,
                path: request.target,
                headers,
                body,
                conn_id: request.conn_id,
                seq: request.seq,
            });
    }
}

/// Create a new HTTP server.
///
/// Returns a server handle that can accept connections, or -1 when the bind
/// failed or this thread has no loop.
#[no_mangle]
pub unsafe extern "C" fn js_http_server_create(port: f64) -> Handle {
    let port = port as u16;
    if !perry_http_server::available(SUBSYSTEM) {
        eprintln!(
            "Failed to bind to port {}: no event loop on this thread",
            port
        );
        return -1;
    }
    let listener_id = Arc::new(Mutex::new(0i64));
    let host = Arc::new(FrameworkHost {
        listener_id: listener_id.clone(),
    });
    let bound = match perry_http_server::listen(
        SUBSYSTEM, host, "0.0.0.0", port, 511,
        // No SO_REUSEPORT: a second `listen()` on a live port must fail, the
        // way Node answers EADDRINUSE. `no_delay` is the next argument, not
        // this one — they were transposed on the `perry-ext-http` listen path
        // for its whole life, which is why they are written out here.
        false, true,
        // Node's idle close for a keep-alive connection: keepAliveTimeout +
        // keepAliveTimeoutBuffer, 5000 + 1000 by default.
        6_000,
    ) {
        Ok(bound) => bound,
        Err(e) => {
            eprintln!("Failed to bind to port {}: {}", port, e.message());
            return -1;
        }
    };
    *listener_id.lock().unwrap_or_else(|e| e.into_inner()) = bound.listener_id;

    println!("Server listening on http://0.0.0.0:{}", bound.port);

    register_handle(HttpServerHandle {
        port: bound.port,
        listener_id: bound.listener_id,
    })
}

/// Take the next decoded request, driving the event loop until one arrives.
///
/// Returns `None` when the server handle is gone — the caller's `while (true)`
/// loop then ends rather than parking forever.
fn accept_blocking(server_handle: Handle) -> Option<PendingRequest> {
    loop {
        let listener_id = get_handle::<HttpServerHandle>(server_handle)?.listener_id;
        if let Some(pending) = queues()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_mut(&listener_id)
            .and_then(|q| q.pop_front())
        {
            return Some(pending);
        }
        // Nothing queued: turn the loop. `js_wait_for_event` parks on the
        // agent's turnloop deadline and dispatches the completions that wake
        // it, so the accept below sees a request the very turn it decodes.
        // `js_wait_for_event` is reached through perry-runtime directly
        // rather than through an `extern "C"` block, because a hand-written
        // declaration is not checked against its definition — the block this
        // replaced declared `js_run_stdlib_pump` as returning `i32` where it
        // returns `()`. The pump itself lives in a `pub(crate)` module, so it
        // keeps its extern declaration, now with the right signature.
        // SAFETY: a no-argument runtime entry point, safe to call on any
        // thread; it no-ops when perry-stdlib registered no pump.
        unsafe { js_run_stdlib_pump() };
        perry_runtime::event_pump::js_wait_for_event();
    }
}

fn register_request(pending: PendingRequest) -> Handle {
    let (path, query) = match pending.path.split_once('?') {
        Some((p, q)) => (p.to_string(), q.to_string()),
        None => (pending.path.clone(), String::new()),
    };
    // The response slot is keyed by request id rather than carried in the
    // handle: `get_handle` hands out a shared reference, so a response cannot
    // take anything out of the handle it is answering.
    PENDING_RESPONSES.insert(
        pending.id,
        ResponseSlot {
            conn_id: pending.conn_id,
            seq: pending.seq,
        },
    );
    register_handle(RequestHandle {
        id: pending.id,
        method: pending.method,
        path,
        query,
        headers: pending.headers,
        body: pending.body,
    })
}

/// Accept the next request (blocking).
///
/// Returns a request handle, or -1 if the server is gone.
#[no_mangle]
pub unsafe extern "C" fn js_http_server_accept(server_handle: Handle) -> Handle {
    match accept_blocking(server_handle) {
        Some(pending) => register_request(pending),
        None => -1,
    }
}

/// Accept the next request (blocking). Identical to
/// [`js_http_server_accept`]; both are kept because both are documented.
#[no_mangle]
pub unsafe extern "C" fn js_http_server_accept_v2(server_handle: Handle) -> Handle {
    js_http_server_accept(server_handle)
}

/// Get request method.
#[no_mangle]
pub unsafe extern "C" fn js_http_request_method(req_handle: Handle) -> *mut StringHeader {
    if let Some(req) = get_handle::<RequestHandle>(req_handle) {
        return js_string_from_bytes(req.method.as_ptr(), req.method.len() as u32);
    }
    std::ptr::null_mut()
}

/// Get request path.
#[no_mangle]
pub unsafe extern "C" fn js_http_request_path(req_handle: Handle) -> *mut StringHeader {
    if let Some(req) = get_handle::<RequestHandle>(req_handle) {
        return js_string_from_bytes(req.path.as_ptr(), req.path.len() as u32);
    }
    std::ptr::null_mut()
}

/// Get request query string.
#[no_mangle]
pub unsafe extern "C" fn js_http_request_query(req_handle: Handle) -> *mut StringHeader {
    if let Some(req) = get_handle::<RequestHandle>(req_handle) {
        return js_string_from_bytes(req.query.as_ptr(), req.query.len() as u32);
    }
    std::ptr::null_mut()
}

/// Get request header by name.
#[no_mangle]
pub unsafe extern "C" fn js_http_request_header(
    req_handle: Handle,
    name_ptr: *const StringHeader,
) -> *mut StringHeader {
    let name = match string_from_header(name_ptr) {
        Some(n) => n.to_lowercase(),
        None => return std::ptr::null_mut(),
    };

    if let Some(req) = get_handle::<RequestHandle>(req_handle) {
        if let Some(value) = req.headers.get(&name) {
            return js_string_from_bytes(value.as_ptr(), value.len() as u32);
        }
    }
    std::ptr::null_mut()
}

/// Get request body as string.
#[no_mangle]
pub unsafe extern "C" fn js_http_request_body(req_handle: Handle) -> *mut StringHeader {
    if let Some(req) = get_handle::<RequestHandle>(req_handle) {
        if let Some(ref body) = req.body {
            return js_string_from_bytes(body.as_ptr(), body.len() as u32);
        }
    }
    std::ptr::null_mut()
}

/// Send response to a request.
#[no_mangle]
pub unsafe extern "C" fn js_http_respond(
    req_handle: Handle,
    status: f64,
    body_ptr: *const StringHeader,
    content_type_ptr: *const StringHeader,
) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;

    let body = string_from_header(body_ptr).unwrap_or_default();
    let content_type =
        string_from_header(content_type_ptr).unwrap_or_else(|| "text/plain".to_string());

    if let Some(req) = get_handle::<RequestHandle>(req_handle) {
        let mut headers = HashMap::new();
        headers.insert("content-type".to_string(), content_type);

        let response = HttpResponse {
            status: status as u16,
            headers,
            body: body.into_bytes(),
        };

        if let Some((_, slot)) = PENDING_RESPONSES.remove(&req.id) {
            let _: Result<(), HttpResponse> = slot.send(response);
            return f64::from_bits(TAG_TRUE);
        }
    }
    f64::from_bits(TAG_FALSE)
}

// Global map of pending responses. Keyed by request id because the handle
// registry hands out shared references: a response cannot move anything out of
// the request handle it answers.
use dashmap::DashMap;
use once_cell::sync::Lazy;

pub static PENDING_RESPONSES: Lazy<DashMap<u64, ResponseSlot>> = Lazy::new(DashMap::new);

/// Shutdown the server: stop accepting, and drop anything still queued.
///
/// In-flight connections finish, which is Node's `server.close()` contract.
#[no_mangle]
pub unsafe extern "C" fn js_http_server_close(server_handle: Handle) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;

    if let Some(server) = get_handle::<HttpServerHandle>(server_handle) {
        let listener_id = server.listener_id;
        perry_http_server::close_listener(listener_id);
        // Anything decoded but never accepted can no longer be answered
        // through this API, so answer it here rather than leaving the client
        // to hang on a connection that stays open until its idle deadline.
        if let Some(queue) = queues()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&listener_id)
        {
            for pending in queue {
                PENDING_RESPONSES.remove(&pending.id);
                perry_http_server::respond(
                    pending.conn_id,
                    pending.seq,
                    perry_http_server::Response {
                        status: 503,
                        headers: vec![("content-length".to_string(), "0".to_string())],
                        ..Default::default()
                    },
                );
            }
        }
        return f64::from_bits(TAG_TRUE);
    }
    f64::from_bits(TAG_FALSE)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value_of<'a>(out: &'a [(String, String)], name: &str) -> Option<&'a str> {
        out.iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    #[test]
    fn a_response_with_no_length_gets_one_from_its_body() {
        let mut headers = HashMap::new();
        headers.insert("content-type".to_string(), "text/plain".to_string());
        let out = with_content_length(&headers, 5);
        assert_eq!(value_of(&out, "content-length"), Some("5"));
    }

    #[test]
    fn a_caller_set_length_survives_whatever_its_case() {
        let mut headers = HashMap::new();
        headers.insert("Content-Length".to_string(), "3".to_string());
        let out = with_content_length(&headers, 5);
        assert_eq!(
            out.iter()
                .filter(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                .count(),
            1,
            "no second length header"
        );
        assert_eq!(
            value_of(&out, "content-length"),
            Some("3"),
            "the caller's length wins over the body's"
        );
    }

    #[test]
    fn an_empty_body_still_declares_zero() {
        let out = with_content_length(&HashMap::new(), 0);
        assert_eq!(value_of(&out, "content-length"), Some("0"));
    }

    /// Node sends `Date` on every response, capitalised, and emits it after
    /// the length — the core then appends `Connection`/`Keep-Alive` after
    /// that, which is the order Node's own server writes.
    #[test]
    fn a_date_header_is_added_after_the_length_in_nodes_spelling() {
        let out = with_content_length(&HashMap::new(), 0);
        let names: Vec<&str> = out.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(names, vec!["content-length", "Date"]);
        assert!(
            value_of(&out, "date").is_some_and(|v| v.ends_with(" GMT")),
            "{out:?}"
        );
    }

    /// A caller that set its own `Date` keeps it.
    #[test]
    fn a_caller_set_date_is_not_duplicated() {
        let mut headers = HashMap::new();
        headers.insert(
            "date".to_string(),
            "Thu, 01 Jan 1970 00:00:00 GMT".to_string(),
        );
        let out = with_content_length(&headers, 0);
        assert_eq!(
            out.iter()
                .filter(|(k, _)| k.eq_ignore_ascii_case("date"))
                .count(),
            1
        );
        assert_eq!(
            value_of(&out, "date"),
            Some("Thu, 01 Jan 1970 00:00:00 GMT")
        );
    }
}
