//! HTTP server loop and request dispatch (hyper-based).
//!
//! `js_fastify_listen` is a blocking call: TS code calls
//! `app.listen({ port: 3000 })` and the wrapper enters an event loop
//! that doesn't return until the program exits. The actual TCP
//! accept loop lives on a perry-ffi-spawned blocking task; the main
//! thread receives `FastifyPendingRequest`s through an `mpsc`
//! channel and invokes the user's TS handler synchronously, then
//! sends the response back via a oneshot channel.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{body::Incoming, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot};

use perry_ffi::{
    alloc_string, get_handle, register_handle, Handle, JsClosure, JsValue, RawClosureHeader,
    StringHeader,
};

use crate::app::{ClosurePtr, FastifyApp, Route};
use crate::context::{jsvalue_to_response_body, FastifyContext};

const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
const PTR_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;

// Runtime symbols not yet wrapped by perry-ffi — we declare them
// locally as `extern "C"`. Same pattern perry-ext-{net,http,ws}
// follow for the small set of stable runtime exports outside
// perry-ffi v0.5's surface.
extern "C" {
    /// Drain all queued microtasks. The fastify event loop calls
    /// this between recv'ing a request and waiting for the next one
    /// so promise chains the user's handler kicked off get a
    /// chance to advance.
    fn js_promise_run_microtasks() -> i32;

    /// Dispatch the registered stdlib pump (perry-stdlib registers
    /// `js_stdlib_process_pending` here at startup, which fans out to
    /// `js_ws_process_pending`, `js_net_process_pending`,
    /// `js_http_process_pending`, etc.). Called from the fastify
    /// event loop so perry-ext-{ws,net,http,fetch} events accumulated
    /// on tokio workers get dispatched on the JS main thread. See #747.
    fn js_run_stdlib_pump();

    /// True if `ptr` is a Promise (NaN-boxed pointer to a runtime
    /// `Promise` struct).
    fn js_is_promise(ptr: *mut Promise) -> i32;

    /// Promise state — 0 = pending, 1 = fulfilled, 2 = rejected.
    fn js_promise_state(ptr: *mut Promise) -> i32;

    /// Read the resolved value of a settled promise.
    fn js_promise_value(ptr: *mut Promise) -> f64;

    /// Read the rejection reason of a rejected promise.
    fn js_promise_reason(ptr: *mut Promise) -> f64;

    /// JSON.stringify with type hint — used for non-string handler
    /// returns when no explicit response body was set.
    fn js_json_stringify(value: f64, type_hint: u32) -> *mut StringHeader;

    /// Toggle the GC's "unsafe zone" — stops gc() calls from worker
    /// threads from collecting objects that may be referenced from
    /// tokio worker stacks. Same call perry-stdlib's fastify makes
    /// to dodge issue #31.
    fn js_gc_enter_unsafe_zone();
}

/// Opaque marker for the runtime's Promise struct. We never read its
/// fields directly — only pass pointers to runtime helpers above.
#[repr(C)]
pub struct Promise {
    _opaque: [u8; 0],
}

/// Server handle returned by `js_fastify_listen`.
pub struct FastifyServerHandle {
    pub port: u16,
    pub app_handle: Handle,
    pub shutdown_tx: Option<oneshot::Sender<()>>,
}

/// Pending request waiting for the TS handler to produce a response.
pub struct FastifyPendingRequest {
    pub method: String,
    pub path: String,
    pub headers: HashMap<String, String>,
    pub body: Option<Vec<u8>>,
    pub params: HashMap<String, String>,
    pub response_tx: oneshot::Sender<FastifyResponse>,
}

/// Response built by the TS handler, sent back to hyper's worker.
pub struct FastifyResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

// ============================================================================
// FFI: listen + close
// ============================================================================

/// `app.listen({ port }, callback?)` — start the server. Blocks the
/// caller indefinitely (the TS-visible API is "kick off the server,
/// then live in the event loop"; main thread returns to the event
/// loop after this call returns).
///
/// # Safety
///
/// `app_handle` must be a registered `FastifyApp` handle. `callback`
/// is an optional `*const ClosureHeader` (NaN-boxed or raw); pass `0`
/// for "no callback".
#[no_mangle]
pub unsafe extern "C" fn js_fastify_listen(app_handle: Handle, opts: f64, callback: i64) {
    // Extract port — accepts `{ port: 3000 }`, a bare number, or
    // falls back to 3000.
    let port = extract_port(opts);

    let (request_tx, request_rx) = mpsc::channel::<FastifyPendingRequest>(1024);
    let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();
    let request_tx = Arc::new(request_tx);

    // Snapshot the current routes for the server task — routes added
    // after `listen()` returns are not picked up by this snapshot
    // (matches perry-stdlib's existing semantics).
    let routes_arc = Arc::new(
        get_handle::<FastifyApp>(app_handle)
            .map(|app| app.routes.clone())
            .unwrap_or_default(),
    );

    // Mark GC-unsafe — request callbacks dispatch on tokio worker
    // threads whose stacks the main-thread GC can't scan. Without
    // this, a user-level `gc()` mid-request could collect objects
    // still referenced from worker stacks (issue #31).
    js_gc_enter_unsafe_zone();

    let request_tx_for_spawn = request_tx.clone();
    let routes_for_spawn = routes_arc.clone();

    perry_ffi::spawn_blocking(move || {
        let handle = tokio::runtime::Handle::current();
        handle.block_on(async move {
            let addr = SocketAddr::from(([0, 0, 0, 0], port));
            let listener = match TcpListener::bind(addr).await {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("Failed to bind to port {}: {}", port, e);
                    return;
                }
            };
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        match accepted {
                            Ok((stream, _)) => {
                                let io = TokioIo::new(stream);
                                let request_tx = request_tx_for_spawn.clone();
                                let routes = routes_for_spawn.clone();
                                tokio::spawn(async move {
                                    let service = service_fn(move |req: Request<Incoming>| {
                                        let request_tx = request_tx.clone();
                                        let routes = routes.clone();
                                        async move {
                                            handle_request(req, request_tx, routes).await
                                        }
                                    });
                                    if let Err(e) = http1::Builder::new()
                                        .serve_connection(io, service)
                                        .await
                                    {
                                        eprintln!("Connection error: {}", e);
                                    }
                                });
                            }
                            Err(e) => eprintln!("Accept error: {}", e),
                        }
                    }
                    _ = &mut shutdown_rx => {
                        break;
                    }
                }
            }
        });
    });

    // Register the server handle so `js_fastify_close` can find it.
    let _server_handle = register_handle(FastifyServerHandle {
        port,
        app_handle,
        shutdown_tx: Some(shutdown_tx),
    });

    // Fire the user's `(err, address) => { ... }` callback — null
    // err, address as a string.
    if callback != 0 {
        let raw = if (callback as u64 & 0xFFFF_0000_0000_0000) == POINTER_TAG {
            (callback as u64 & PTR_MASK) as *const RawClosureHeader
        } else {
            callback as *const RawClosureHeader
        };
        let address = format!("http://0.0.0.0:{}", port);
        let addr_str = alloc_string(&address);
        let addr_val = JsValue::from_string_ptr(addr_str.as_raw());
        let null_val = f64::from_bits(TAG_NULL);
        let closure = JsClosure::from_raw(raw);
        if !closure.is_null() {
            let _ = closure.call2(null_val, f64::from_bits(addr_val.bits()));
        }
    }

    println!("Server listening on http://0.0.0.0:{}", port);

    // Enter the main event loop — drain pending requests + dispatch
    // to user handlers until the process exits.
    let mut request_rx = request_rx;
    event_loop(app_handle, &mut request_rx);
}

/// Close the server by dropping the registered handle.
#[no_mangle]
pub unsafe extern "C" fn js_fastify_close(server_handle: Handle) -> bool {
    if let Some(_server) = get_handle::<FastifyServerHandle>(server_handle) {
        // Shutdown sender drops when the handle is dropped — the
        // accept loop's `tokio::select!` picks up the channel close
        // and terminates. Simpler than threading the sender out.
        return true;
    }
    false
}

// ============================================================================
// Request dispatch
// ============================================================================

/// Hyper service function — match the route, hand the request to the
/// main thread via mpsc, await the response.
async fn handle_request(
    req: Request<Incoming>,
    request_tx: Arc<mpsc::Sender<FastifyPendingRequest>>,
    routes: Arc<Vec<Route>>,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    let method = req.method().to_string();
    let uri = req.uri();
    let path = match uri.query() {
        Some(q) => format!("{}?{}", uri.path(), q),
        None => uri.path().to_string(),
    };

    let mut headers = HashMap::new();
    for (name, value) in req.headers() {
        if let Ok(v) = value.to_str() {
            headers.insert(name.to_string().to_lowercase(), v.to_string());
        }
    }

    let body = match req.collect().await {
        Ok(collected) => {
            let bytes = collected.to_bytes();
            if bytes.is_empty() {
                None
            } else {
                Some(bytes.to_vec())
            }
        }
        Err(_) => None,
    };

    // Match
    let mut matched_params = HashMap::new();
    let mut found_route = false;
    for route in routes.iter() {
        if route.method == method {
            if let Some(params) = route.pattern.match_path(&path) {
                matched_params = params;
                found_route = true;
                break;
            }
        }
    }

    if !found_route {
        return Ok(Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header("content-type", "application/json")
            .body(Full::new(Bytes::from(r#"{"error":"Not Found"}"#)))
            .unwrap());
    }

    let (response_tx, response_rx) = oneshot::channel::<FastifyResponse>();
    let pending = FastifyPendingRequest {
        method,
        path,
        headers,
        body,
        params: matched_params,
        response_tx,
    };

    if request_tx.send(pending).await.is_err() {
        return Ok(Response::builder()
            .status(StatusCode::SERVICE_UNAVAILABLE)
            .body(Full::new(Bytes::from("Server unavailable")))
            .unwrap());
    }

    // Wake the main thread so it doesn't wait on its 10ms timeout.
    perry_ffi::notify_main_thread();

    match response_rx.await {
        Ok(fr) => {
            let mut builder = Response::builder()
                .status(StatusCode::from_u16(fr.status).unwrap_or(StatusCode::OK));
            for (name, value) in fr.headers {
                builder = builder.header(name, value);
            }
            Ok(builder.body(Full::new(Bytes::from(fr.body))).unwrap())
        }
        Err(_) => Ok(Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Full::new(Bytes::from("Handler error")))
            .unwrap()),
    }
}

/// Main thread event loop — drains pending requests and runs user
/// handlers (and lifecycle hooks) synchronously.
fn event_loop(app_handle: Handle, request_rx: &mut mpsc::Receiver<FastifyPendingRequest>) {
    loop {
        // Pump stdlib so perry-ext-{ws,net,http,fetch} events that
        // accumulated on tokio workers get dispatched on the JS main
        // thread. Without this, listeners registered before
        // `app.listen()` (e.g. `wss.on('connection', ...)` against a
        // separate WebSocketServer) never see incoming traffic. See
        // #747 — and #746, whose symptom is the same in any program
        // that combines `fastify` with `ws` / `net` / `http`.
        unsafe {
            js_run_stdlib_pump();
        }

        // Drain microtasks queued by previous handler runs.
        unsafe {
            js_promise_run_microtasks();
        }

        // Try to receive a request with a 10ms timeout. Keeps the
        // event loop responsive without busy-spinning.
        let result = match try_recv_with_timeout(request_rx) {
            Some(p) => p,
            None => continue,
        };

        process_request(app_handle, result);
    }
}

/// Receive a pending request, blocking up to 10ms. We can't easily
/// re-enter perry-stdlib's tokio runtime from this thread (we're
/// outside any tokio context here), so we use `try_recv` in a tight
/// loop with a small `thread::sleep`.
fn try_recv_with_timeout(
    request_rx: &mut mpsc::Receiver<FastifyPendingRequest>,
) -> Option<FastifyPendingRequest> {
    use std::time::{Duration, Instant};
    let deadline = Instant::now() + Duration::from_millis(10);
    loop {
        match request_rx.try_recv() {
            Ok(p) => return Some(p),
            Err(mpsc::error::TryRecvError::Disconnected) => return None,
            Err(mpsc::error::TryRecvError::Empty) => {
                if Instant::now() >= deadline {
                    return None;
                }
                std::thread::sleep(Duration::from_micros(200));
            }
        }
    }
}

/// Process one request — fire hooks, call route handler, send the
/// response back through the oneshot channel.
fn process_request(app_handle: Handle, pending: FastifyPendingRequest) {
    let ctx = FastifyContext::new(
        0,
        pending.method.clone(),
        pending.path.clone(),
        pending.headers.clone(),
        pending.body.clone(),
        pending.params.clone(),
    );
    let ctx_handle = register_handle(ctx);

    // Snapshot hooks + matched route (need to drop the borrow before
    // invoking user closures, which may mutate the app).
    let (on_request_hooks, pre_handler_hooks, matched_handler): (
        Vec<ClosurePtr>,
        Vec<ClosurePtr>,
        Option<ClosurePtr>,
    ) = match get_handle::<FastifyApp>(app_handle) {
        Some(app) => {
            let on_req = app.hooks.on_request.clone();
            let pre = app.hooks.pre_handler.clone();
            let matched = app
                .match_route(&pending.method, &pending.path)
                .map(|(r, _)| r.handler);
            (on_req, pre, matched)
        }
        None => (Vec::new(), Vec::new(), None),
    };

    // NaN-box the context handle — POINTER_TAG so codegen-side
    // method dispatch on `request.*` / `reply.*` Just Works.
    let ctx_f64 = f64::from_bits(POINTER_TAG | (ctx_handle as u64 & PTR_MASK));

    let mut response_sent = false;
    for hook in &on_request_hooks {
        if call_hook_awaiting(*hook, ctx_f64, ctx_handle) {
            response_sent = true;
            break;
        }
    }
    if !response_sent {
        for hook in &pre_handler_hooks {
            if call_hook_awaiting(*hook, ctx_f64, ctx_handle) {
                response_sent = true;
                break;
            }
        }
    }

    let mut final_result = f64::from_bits(TAG_UNDEFINED);
    if !response_sent {
        if let Some(handler) = matched_handler {
            let result = unsafe {
                let raw = handler as *const RawClosureHeader;
                let closure = JsClosure::from_raw(raw);
                if closure.is_null() {
                    f64::from_bits(TAG_UNDEFINED)
                } else {
                    closure.call2(ctx_f64, ctx_f64)
                }
            };
            unsafe {
                js_promise_run_microtasks();
            }
            final_result = result;

            // If the handler returned a Promise, wait for it.
            let jsv = JsValue::from_bits(result.to_bits());
            if jsv.is_pointer() {
                let ptr = jsv.as_pointer::<Promise>();
                if !ptr.is_null() && unsafe { js_is_promise(ptr) } != 0 {
                    wait_for_promise(ptr);
                    final_result = unsafe { js_promise_value(ptr) };
                }
            }
        }
    }

    // Build + send the response.
    if let Some(ctx) = get_handle::<FastifyContext>(ctx_handle) {
        let mut response = FastifyResponse {
            status: ctx.status_code,
            headers: ctx.response_headers.clone(),
            body: ctx
                .response_body
                .clone()
                .unwrap_or_else(|| unsafe { build_response_body(final_result) }),
        };
        if !response
            .headers
            .iter()
            .any(|(k, _)| k.to_lowercase() == "content-type")
        {
            response
                .headers
                .push(("content-type".to_string(), "application/json".to_string()));
        }
        let _ = pending.response_tx.send(response);
    }

    // Free the context handle so it doesn't leak.
    perry_ffi::drop_handle(ctx_handle);
}

/// Call a hook closure, await any returned Promise, and return whether
/// `ctx.sent` is true (i.e. the hook produced a response, e.g. an
/// auth-middleware 401).
fn call_hook_awaiting(hook: ClosurePtr, ctx_f64: f64, ctx_handle: Handle) -> bool {
    if hook == 0 {
        return false;
    }
    let result = unsafe {
        let closure = JsClosure::from_raw(hook as *const RawClosureHeader);
        if closure.is_null() {
            return false;
        }
        closure.call2(ctx_f64, ctx_f64)
    };
    unsafe {
        js_promise_run_microtasks();
    }
    let jsv = JsValue::from_bits(result.to_bits());
    if jsv.is_pointer() {
        let ptr = jsv.as_pointer::<Promise>();
        if !ptr.is_null() && unsafe { js_is_promise(ptr) } != 0 {
            wait_for_promise(ptr);
        }
    }
    get_handle::<FastifyContext>(ctx_handle)
        .map(|c| c.sent)
        .unwrap_or(false)
}

/// Spin until a promise resolves — bounded to avoid infinite loops if
/// the handler chain stalls. Polls microtasks and the stdlib pump every
/// iteration so awaited values get a chance to settle. The codegen-
/// emitted `await` body already pumps stdlib inside compiled functions
/// (`perry-codegen/src/expr.rs:9500`), so the common case is covered
/// without this — but a route handler that returns a Promise resolved
/// directly by an external mechanism (no inner `await` loop pumping
/// stdlib) would otherwise stall here. Matches `perry-stdlib/src/
/// fastify/server.rs:432`, which has had this call all along; the
/// perry-ext-fastify port (v0.5.572) dropped it.
fn wait_for_promise(promise_ptr: *mut Promise) {
    use std::time::Duration;
    for _ in 0..10000 {
        unsafe {
            js_run_stdlib_pump();
            js_promise_run_microtasks();
        }
        let state = unsafe { js_promise_state(promise_ptr) };
        if state != 0 {
            return;
        }
        std::thread::sleep(Duration::from_micros(100));
    }
}

/// Render the handler return value as response bytes. Handlers can
/// return strings (used as-is), objects/arrays (JSON-stringified),
/// numbers/bools (toString), or `undefined` (empty `{}`).
unsafe fn build_response_body(value: f64) -> Vec<u8> {
    let jsv = JsValue::from_bits(value.to_bits());
    if jsv.is_undefined() || jsv.is_null() {
        return b"{}".to_vec();
    }
    if jsv.is_string() {
        return jsvalue_to_response_body(value);
    }
    if jsv.is_pointer() {
        let str_ptr = js_json_stringify(value, 0);
        if !str_ptr.is_null() {
            let len = (*str_ptr).byte_len as usize;
            let data_ptr = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
            return std::slice::from_raw_parts(data_ptr, len).to_vec();
        }
    }
    // Fallback through the unified path.
    jsvalue_to_response_body(value)
}

// ============================================================================
// Helpers
// ============================================================================

unsafe fn extract_port(opts: f64) -> u16 {
    let v = JsValue::from_bits(opts.to_bits());
    if v.is_pointer() {
        if let Some(json) = perry_ffi::json_stringify(v) {
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&json) {
                if let Some(p) = parsed.get("port").and_then(|p| {
                    p.as_u64()
                        .or_else(|| p.as_i64().map(|n| n.max(0) as u64))
                        .or_else(|| p.as_f64().map(|n| n.max(0.0) as u64))
                }) {
                    return p as u16;
                }
            }
        }
        return 3000;
    }
    if v.is_number() {
        let n = v.to_number();
        if n > 0.0 {
            return n as u16;
        }
    }
    3000
}

// `js_promise_reason` is declared so wrappers that want to surface
// rejected-promise errors can use it; not consumed by the v0 port,
// but kept in the extern block so signature drift causes a link
// error rather than UB.
#[allow(dead_code)]
unsafe fn _force_promise_reason_link(p: *mut Promise) -> f64 {
    js_promise_reason(p)
}
