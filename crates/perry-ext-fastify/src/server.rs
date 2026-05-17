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
use std::os::raw::c_int;
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
const GC_HEADER_SIZE: usize = 8;
const GC_TYPE_ERROR: u8 = 7;

struct ClosureCallResult {
    value: f64,
    thrown: Option<f64>,
}

enum HookOutcome {
    Continue,
    Sent,
    Error(f64),
}

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

    /// Condvar-based wait for the next event (timer fire, notify from a
    /// tokio worker, or 1 s idle cap). Used by `wait_for_promise` so the
    /// handler dispatcher blocks on real events instead of burning the
    /// CPU in a 100 us-poll loop. Wakes the moment any stdlib worker
    /// calls `js_notify_main_thread`, including the per-promise wake
    /// fired by `js_promise_resolve` / `js_promise_reject`.
    fn js_wait_for_event();

    /// Drive timer callback dispatch (matches the codegen-emitted await
    /// wait body). Without these the `await new Promise(r => setTimeout(r, n))`
    /// shape would never advance inside `wait_for_promise`, only inside
    /// directly-compiled `await` sites.
    fn js_timer_tick() -> i32;
    fn js_callback_timer_tick() -> i32;
    fn js_interval_timer_tick() -> i32;

    fn js_try_push() -> *mut c_int;
    fn js_try_end();
    fn js_get_exception() -> f64;
    fn js_clear_exception();
    fn js_error_get_message(error: *mut ErrorHeader) -> *mut StringHeader;
}

#[cfg(target_vendor = "apple")]
extern "C" {
    #[link_name = "_setjmp"]
    fn setjmp(env: *mut c_int) -> c_int;
}

#[cfg(not(target_vendor = "apple"))]
extern "C" {
    fn setjmp(env: *mut c_int) -> c_int;
}

/// Opaque marker for the runtime's Promise struct. We never read its
/// fields directly — only pass pointers to runtime helpers above.
#[repr(C)]
pub struct Promise {
    _opaque: [u8; 0],
}

#[repr(C)]
pub struct ErrorHeader {
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
                                        // perry#924: hyper surfaces every malformed
                                        // client read as a per-connection error
                                        // (HTTP/2 prefaces, scanner garbage). The
                                        // application never sees these requests, so
                                        // logging them by default just floods PM2
                                        // error logs. Gate behind `PERRY_DEBUG=1`.
                                        if std::env::var_os("PERRY_DEBUG").is_some() {
                                            eprintln!("Connection error: {}", e);
                                        }
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
    let (on_request_hooks, pre_handler_hooks, matched_handler, error_handler): (
        Vec<ClosurePtr>,
        Vec<ClosurePtr>,
        Option<ClosurePtr>,
        Option<ClosurePtr>,
    ) = match get_handle::<FastifyApp>(app_handle) {
        Some(app) => {
            let on_req = app.hooks.on_request.clone();
            let pre = app.hooks.pre_handler.clone();
            let matched = app
                .match_route(&pending.method, &pending.path)
                .map(|(r, _)| r.handler);
            (on_req, pre, matched, app.error_handler)
        }
        None => (Vec::new(), Vec::new(), None, None),
    };

    // NaN-box the context handle — POINTER_TAG so codegen-side
    // method dispatch on `request.*` / `reply.*` Just Works.
    let ctx_f64 = f64::from_bits(POINTER_TAG | (ctx_handle as u64 & PTR_MASK));

    let mut response_sent = false;
    for hook in &on_request_hooks {
        match call_hook_awaiting(*hook, ctx_f64, ctx_handle) {
            HookOutcome::Continue => {}
            HookOutcome::Sent => {
                response_sent = true;
                break;
            }
            HookOutcome::Error(reason) => {
                handle_error_response(error_handler, ctx_f64, ctx_handle, reason);
                response_sent = true;
                break;
            }
        }
    }
    if !response_sent {
        for hook in &pre_handler_hooks {
            match call_hook_awaiting(*hook, ctx_f64, ctx_handle) {
                HookOutcome::Continue => {}
                HookOutcome::Sent => {
                    response_sent = true;
                    break;
                }
                HookOutcome::Error(reason) => {
                    handle_error_response(error_handler, ctx_f64, ctx_handle, reason);
                    response_sent = true;
                    break;
                }
            }
        }
    }

    let mut final_result = f64::from_bits(TAG_UNDEFINED);
    if !response_sent {
        if let Some(handler) = matched_handler {
            let call = unsafe {
                let raw = handler as *const RawClosureHeader;
                let closure = JsClosure::from_raw(raw);
                if closure.is_null() {
                    ClosureCallResult {
                        value: f64::from_bits(TAG_UNDEFINED),
                        thrown: None,
                    }
                } else {
                    call_closure2_catching(closure, ctx_f64, ctx_f64)
                }
            };
            if let Some(reason) = call.thrown {
                handle_error_response(error_handler, ctx_f64, ctx_handle, reason);
                response_sent = true;
            }
            unsafe {
                js_promise_run_microtasks();
            }
            if !response_sent {
                final_result = call.value;
            }

            // If the handler returned a Promise, wait for it.
            let jsv = JsValue::from_bits(call.value.to_bits());
            if !response_sent && jsv.is_pointer() {
                let ptr = jsv.as_pointer::<Promise>();
                if !ptr.is_null() && unsafe { js_is_promise(ptr) } != 0 {
                    wait_for_promise(ptr);
                    // Read state AFTER the wait — `js_promise_value`
                    // returns `(*promise).value` unconditionally, and
                    // that field stays at its initial `0.0` for rejected
                    // promises (which set `reason`, not `value`) and
                    // pending promises (which are never reached after
                    // the unbounded `wait_for_promise` returns, but
                    // defending here keeps us robust against future
                    // changes to `wait_for_promise`'s contract). Without
                    // this branch, an unhandled rejection inside a
                    // route handler would serialize the literal byte
                    // `0` as the response body — exactly the issue
                    // #748 symptom for the cases where the chain
                    // rejected instead of stalling.
                    let st = unsafe { js_promise_state(ptr) };
                    if st == 2 {
                        // Rejected — translate to a 500 response with
                        // the rejection reason rendered to JSON. The
                        // dispatcher's fallback `build_response_body`
                        // already JSON-stringifies pointer values, so
                        // wrap the reason in a `{ error: <reason> }`
                        // envelope to avoid spilling raw stack traces
                        // into the wire. Mirrors `fastify`'s default
                        // error handler shape.
                        let reason = unsafe { js_promise_reason(ptr) };
                        handle_error_response(error_handler, ctx_f64, ctx_handle, reason);
                        final_result = f64::from_bits(TAG_UNDEFINED);
                    } else {
                        final_result = unsafe { js_promise_value(ptr) };
                    }
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

/// Call a hook closure, await any returned Promise, and report whether it
/// sent a response or threw/rejected.
fn call_hook_awaiting(hook: ClosurePtr, ctx_f64: f64, ctx_handle: Handle) -> HookOutcome {
    if hook == 0 {
        return HookOutcome::Continue;
    }
    let call = unsafe {
        let closure = JsClosure::from_raw(hook as *const RawClosureHeader);
        if closure.is_null() {
            return HookOutcome::Continue;
        }
        call_closure2_catching(closure, ctx_f64, ctx_f64)
    };
    if let Some(reason) = call.thrown {
        return HookOutcome::Error(reason);
    }
    unsafe {
        js_promise_run_microtasks();
    }
    let jsv = JsValue::from_bits(call.value.to_bits());
    if jsv.is_pointer() {
        let ptr = jsv.as_pointer::<Promise>();
        if !ptr.is_null() && unsafe { js_is_promise(ptr) } != 0 {
            wait_for_promise(ptr);
            if unsafe { js_promise_state(ptr) } == 2 {
                return HookOutcome::Error(unsafe { js_promise_reason(ptr) });
            }
        }
    }
    if get_handle::<FastifyContext>(ctx_handle)
        .map(|c| c.sent)
        .unwrap_or(false)
    {
        HookOutcome::Sent
    } else {
        HookOutcome::Continue
    }
}

unsafe fn call_closure2_catching(closure: JsClosure, arg0: f64, arg1: f64) -> ClosureCallResult {
    let trap_buf = js_try_push();
    let jumped = setjmp(trap_buf);
    if jumped != 0 {
        let exc = js_get_exception();
        js_clear_exception();
        js_try_end();
        return ClosureCallResult {
            value: f64::from_bits(TAG_UNDEFINED),
            thrown: Some(exc),
        };
    }

    let value = closure.call2(arg0, arg1);
    js_try_end();
    ClosureCallResult {
        value,
        thrown: None,
    }
}

unsafe fn call_closure3_catching(
    closure: JsClosure,
    arg0: f64,
    arg1: f64,
    arg2: f64,
) -> ClosureCallResult {
    let trap_buf = js_try_push();
    let jumped = setjmp(trap_buf);
    if jumped != 0 {
        let exc = js_get_exception();
        js_clear_exception();
        js_try_end();
        return ClosureCallResult {
            value: f64::from_bits(TAG_UNDEFINED),
            thrown: Some(exc),
        };
    }

    let value = closure.call3(arg0, arg1, arg2);
    js_try_end();
    ClosureCallResult {
        value,
        thrown: None,
    }
}

fn handle_error_response(
    error_handler: Option<ClosurePtr>,
    ctx_f64: f64,
    ctx_handle: Handle,
    reason: f64,
) {
    if let Some(ctx) = perry_ffi::get_handle_mut::<FastifyContext>(ctx_handle) {
        ctx.status_code = 500;
    }

    if let Some(handler) = error_handler {
        let call = unsafe {
            let closure = JsClosure::from_raw(handler as *const RawClosureHeader);
            if closure.is_null() {
                ClosureCallResult {
                    value: f64::from_bits(TAG_UNDEFINED),
                    thrown: Some(reason),
                }
            } else {
                call_closure3_catching(closure, reason, ctx_f64, ctx_f64)
            }
        };
        let mut fallback_reason = call.thrown;

        if fallback_reason.is_none() {
            unsafe {
                js_run_stdlib_pump();
                js_promise_run_microtasks();
            }

            let mut final_result = call.value;
            let jsv = JsValue::from_bits(call.value.to_bits());
            if jsv.is_pointer() {
                let ptr = jsv.as_pointer::<Promise>();
                if !ptr.is_null() && unsafe { js_is_promise(ptr) } != 0 {
                    wait_for_promise(ptr);
                    if unsafe { js_promise_state(ptr) } == 2 {
                        fallback_reason = Some(unsafe { js_promise_reason(ptr) });
                    } else {
                        final_result = unsafe { js_promise_value(ptr) };
                    }
                }
            }

            if fallback_reason.is_none() {
                if let Some(ctx) = perry_ffi::get_handle_mut::<FastifyContext>(ctx_handle) {
                    if ctx.response_body.is_none() {
                        ctx.response_body = Some(unsafe { build_response_body(final_result) });
                    }
                }
                return;
            }
        }

        if let Some(reason) = fallback_reason {
            apply_default_error_response(ctx_handle, reason);
        }
    } else {
        apply_default_error_response(ctx_handle, reason);
    }
}

fn apply_default_error_response(ctx_handle: Handle, reason: f64) {
    if let Some(ctx) = perry_ffi::get_handle_mut::<FastifyContext>(ctx_handle) {
        ctx.status_code = 500;
        ctx.response_body = Some(unsafe { render_rejection_body(reason) });
    }
}

/// Wait until a promise settles, driving microtasks, the stdlib pump,
/// and timer ticks every iteration. Blocks on `js_wait_for_event` (a
/// condvar with a 1 s idle cap) instead of `thread::sleep`, so the
/// dispatcher wakes the moment any stdlib worker calls
/// `js_notify_main_thread` — the same wake that `js_promise_resolve`
/// and `js_promise_reject` fire when the awaited chain advances.
///
/// Mirrors the codegen-emitted `await` body in
/// `crates/perry-codegen/src/expr.rs` (the "=== wait ===" block at
/// lines ~9645-9665): no fixed iteration limit, condvar-based wait.
///
/// ### Why the old polling loop is wrong (issue #748)
///
/// The previous implementation looped 10_000 × 100 us = ~1 s and then
/// returned regardless of whether the promise had settled. Callers
/// then read `js_promise_value(ptr)` which returns `(*promise).value`
/// — `0.0` for a still-Pending Promise (it's initialized to zero in
/// `Promise::new` and only overwritten by `js_promise_resolve`). The
/// dispatcher serialized that `0.0` as the response body, yielding the
/// literal ASCII byte `0x30` ("0") with HTTP 200 (default
/// `status_code` — `reply.code(201)` was never reached because the
/// handler chain hadn't returned). The signup-style route in #748
/// runs many awaits (rate-limiter, argon2 hash, multiple `pool.exec`
/// round-trips, JWT signing) which routinely exceeds 1 s on a cold
/// connection pool; every operation after the timeout silently
/// no-op'd because the dispatcher returned and stopped pumping
/// microtasks for the orphaned chain.
fn wait_for_promise(promise_ptr: *mut Promise) {
    // First pump synchronously — handles the already-settled case
    // (e.g. `async () => 42` whose promise is fulfilled before this
    // function is even called) without entering the wait path.
    unsafe {
        js_run_stdlib_pump();
        js_promise_run_microtasks();
    }
    let mut state = unsafe { js_promise_state(promise_ptr) };
    if state != 0 {
        return;
    }
    loop {
        unsafe {
            // Drive timers, the stdlib pump, and microtasks every tick
            // — mirrors the codegen-emitted await body. `js_timer_tick`
            // & friends are no-ops when there's nothing to dispatch.
            let _ = js_timer_tick();
            let _ = js_callback_timer_tick();
            let _ = js_interval_timer_tick();
            js_run_stdlib_pump();
            js_promise_run_microtasks();
            // Condvar wait: blocks until a notify arrives or the 1 s
            // idle cap elapses, whichever is first. `js_promise_resolve`
            // / `js_promise_reject` fire `js_notify_main_thread`, so
            // the wake happens the instant the chain advances.
            js_wait_for_event();
        }
        state = unsafe { js_promise_state(promise_ptr) };
        if state != 0 {
            return;
        }
    }
}

/// Render a Promise rejection reason as a `{ "error": ... }` JSON body
/// for the 500 response surfaced by `process_request`. Falls back to a
/// generic envelope if the reason can't be stringified (e.g. opaque
/// pointer that JSON.stringify rejects).
unsafe fn render_rejection_body(reason: f64) -> Vec<u8> {
    if let Some(message) = error_message(reason) {
        let mut out =
            b"{\"statusCode\":500,\"error\":\"Internal Server Error\",\"message\":".to_vec();
        push_json_string(&mut out, message.as_bytes());
        out.push(b'}');
        return out;
    }

    // Strings: wrap the user's message verbatim.
    let jsv = JsValue::from_bits(reason.to_bits());
    if jsv.is_string() {
        let s = jsvalue_to_response_body(reason);
        // s is the raw string bytes; embed as a JSON string literal.
        let mut out = b"{\"error\":".to_vec();
        out.push(b'"');
        for b in s {
            match b {
                b'"' => out.extend_from_slice(b"\\\""),
                b'\\' => out.extend_from_slice(b"\\\\"),
                b'\n' => out.extend_from_slice(b"\\n"),
                b'\r' => out.extend_from_slice(b"\\r"),
                b'\t' => out.extend_from_slice(b"\\t"),
                0x00..=0x1f => out.extend_from_slice(format!("\\u{:04x}", b).as_bytes()),
                _ => out.push(b),
            }
        }
        out.push(b'"');
        out.push(b'}');
        return out;
    }
    if jsv.is_pointer() {
        let str_ptr = js_json_stringify(reason, 0);
        if !str_ptr.is_null() {
            let len = (*str_ptr).byte_len as usize;
            let data_ptr = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
            let inner = std::slice::from_raw_parts(data_ptr, len).to_vec();
            let mut out = b"{\"error\":".to_vec();
            out.extend_from_slice(&inner);
            out.push(b'}');
            return out;
        }
    }
    // Numbers/bools/null/undefined: best-effort stringification.
    let body = jsvalue_to_response_body(reason);
    let mut out = b"{\"error\":".to_vec();
    if body.is_empty() {
        out.extend_from_slice(b"null");
    } else {
        out.push(b'"');
        for b in body {
            match b {
                b'"' => out.extend_from_slice(b"\\\""),
                b'\\' => out.extend_from_slice(b"\\\\"),
                _ => out.push(b),
            }
        }
        out.push(b'"');
    }
    out.push(b'}');
    out
}

unsafe fn error_message(reason: f64) -> Option<String> {
    let jsv = JsValue::from_bits(reason.to_bits());
    if jsv.is_pointer() {
        let ptr = jsv.as_pointer::<u8>();
        if gc_obj_type(ptr) == GC_TYPE_ERROR {
            let msg = js_error_get_message(ptr as *mut ErrorHeader);
            return string_header_to_string(msg);
        }
    }
    None
}

unsafe fn gc_obj_type(ptr: *const u8) -> u8 {
    if ptr.is_null() || (ptr as usize) < 0x1000 {
        return 0;
    }
    *ptr.sub(GC_HEADER_SIZE)
}

unsafe fn string_header_to_string(ptr: *const StringHeader) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let len = (*ptr).byte_len as usize;
    let data_ptr = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);
    Some(String::from_utf8_lossy(bytes).to_string())
}

fn push_json_string(out: &mut Vec<u8>, bytes: &[u8]) {
    out.push(b'"');
    for &b in bytes {
        match b {
            b'"' => out.extend_from_slice(b"\\\""),
            b'\\' => out.extend_from_slice(b"\\\\"),
            b'\n' => out.extend_from_slice(b"\\n"),
            b'\r' => out.extend_from_slice(b"\\r"),
            b'\t' => out.extend_from_slice(b"\\t"),
            0x00..=0x1f => out.extend_from_slice(format!("\\u{:04x}", b).as_bytes()),
            _ => out.push(b),
        }
    }
    out.push(b'"');
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
