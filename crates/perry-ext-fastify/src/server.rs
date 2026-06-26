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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{body::Incoming, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot};

use perry_ffi::{
    alloc_string, get_handle, get_handle_mut, iter_handle_ids_of, register_handle, Handle,
    JsClosure, JsValue, RawClosureHeader, StringHeader,
};

use crate::app::{ClosurePtr, FastifyApp};
use crate::context::{extract_buffer_bytes, jsvalue_to_response_body, BodyKind, FastifyContext};
use crate::router::RoutePattern;

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

#[derive(Clone)]
struct RouteMatcher {
    method: String,
    pattern: RoutePattern,
}

impl RouteMatcher {
    fn from_route(route: &crate::app::Route) -> Self {
        Self {
            method: route.method.clone(),
            pattern: route.pattern.clone(),
        }
    }
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
///
/// Pre-fix, `listen()` blocked the main TS thread inside an inner
/// `event_loop` that never returned, so `await app.listen(...)` in
/// user code never resumed and any subsequent code (an in-process
/// `fetch` against the same process, `app.close()`, etc.) never ran —
/// the compat-sweep fixture timed out at gtimeout(30s). The fix
/// mirrors what perry-ext-http-server did in #604: `listen()` returns
/// immediately after spawning the accept loop, and a new
/// `js_fastify_process_pending` extern wired into perry-stdlib's main
/// pump drains the per-server mpsc each tick. The receiver lives
/// inside the handle so the pump can find it after `listen()` returns.
pub struct FastifyServerHandle {
    pub port: u16,
    pub app_handle: Handle,
    pub shutdown_tx: Option<oneshot::Sender<()>>,
    /// Drained by `js_fastify_process_pending` from the main TS thread
    /// each tick. `Mutex` because the handle registry hands out `&'static`
    /// references but the pump needs `&mut` access to `try_recv` and
    /// we can't statically prove the pump is the only mutator.
    pub request_rx: Mutex<Option<mpsc::Receiver<FastifyPendingRequest>>>,
    /// #1113 — WebSocket upgrade events queued from the hyper accept
    /// task once `hyper::upgrade::on` resolves and the upgraded stream
    /// has been registered with `perry_ext_ws::register_external_ws_stream`.
    /// Drained alongside `request_rx` in `js_fastify_process_pending`.
    pub upgrade_rx: Mutex<Option<mpsc::Receiver<FastifyPendingUpgrade>>>,
    /// True between `listen()` and `close()`. The
    /// `js_fastify_has_active` extern returns 1 while any server has
    /// this set, keeping the runtime's main event loop alive until the
    /// user explicitly closes the server.
    pub listening: AtomicBool,
}

/// #1113 — pending WebSocket upgrade ready to fire the fastify
/// `app.server.on("upgrade", …)` handlers. Sent by the hyper accept
/// task after `hyper::upgrade::on` resolves and the upgraded stream
/// has been registered with `perry_ext_ws::register_external_ws_stream`.
/// Mirror of perry-ext-http-server's `HttpPendingUpgrade`.
pub struct FastifyPendingUpgrade {
    pub app_handle: Handle,
    pub method: String,
    pub path: String,
    pub headers: HashMap<String, String>,
    pub ws_id: i64,
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
    // #1113 — separate channel for WebSocket upgrade events so a busy
    // request stream can't starve them (mirror of perry-ext-http-server).
    let (upgrade_tx, upgrade_rx) = mpsc::channel::<FastifyPendingUpgrade>(256);
    let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();
    let request_tx = Arc::new(request_tx);
    let upgrade_tx = Arc::new(upgrade_tx);

    // Snapshot only route-matching metadata for the server task. Handler
    // closure pointers stay in the FastifyApp handle and are read by the
    // main-thread pump during dispatch.
    let routes_arc = Arc::new(
        get_handle::<FastifyApp>(app_handle)
            .map(|app| {
                app.routes
                    .iter()
                    .map(RouteMatcher::from_route)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default(),
    );

    // Tokio workers only match routes and queue raw request/upgrade data.
    // User JS dispatch runs from `js_fastify_process_pending` on the main
    // thread, so a server lifetime must not suppress GC.

    let request_tx_for_spawn = request_tx.clone();
    let upgrade_tx_for_spawn = upgrade_tx.clone();
    let routes_for_spawn = routes_arc.clone();

    // The accept loop must run as a cooperative task on the shared
    // multi-thread runtime. A plain `spawn_blocking` thread does not
    // reliably carry the runtime's reactor/worker context: with
    // `Handle::current().block_on(accept_loop)` the listener bound and
    // accepted connections, but the per-connection
    // `tokio::spawn(serve_connection)` tasks below were never driven — the
    // request bytes sat unread and every response hung (the "in release the
    // IO loop silently failed to start" brittleness the net/ws adapters
    // hit). `spawn_blocking_with_reactor` runs the closure inside a worker
    // task (`runtime().spawn(async { … })`), so `tokio::spawn`-ing the
    // accept loop drives it and its fan-out serve tasks on the worker pool —
    // mirroring perry-ext-http-server / -net / -ws. (A bare
    // `Handle::current().block_on` here would panic "Cannot start a runtime
    // from within a runtime" inside the worker task; spawn instead.)
    perry_ffi::spawn_blocking_with_reactor(move || {
        tokio::spawn(async move {
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
                                let upgrade_tx = upgrade_tx_for_spawn.clone();
                                let routes = routes_for_spawn.clone();
                                tokio::spawn(async move {
                                    let service = service_fn(move |req: Request<Incoming>| {
                                        let request_tx = request_tx.clone();
                                        let upgrade_tx = upgrade_tx.clone();
                                        let routes = routes.clone();
                                        async move {
                                            handle_request(app_handle, req, request_tx, upgrade_tx, routes).await
                                        }
                                    });
                                    // #1113: `.with_upgrades()` is REQUIRED for
                                    // `hyper::upgrade::on(&mut req)` to resolve.
                                    // Without it the upgrade future never
                                    // completes and the WS handshake stalls.
                                    if let Err(e) = http1::Builder::new()
                                        .serve_connection(io, service)
                                        .with_upgrades()
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

    // Register the server handle so `js_fastify_close` and the
    // process_pending pump can find it. The receiver lives inside the
    // handle so the pump (driven from perry-stdlib's main loop) can
    // drain it after `listen()` returns.
    let _server_handle = register_handle(FastifyServerHandle {
        port,
        app_handle,
        shutdown_tx: Some(shutdown_tx),
        request_rx: Mutex::new(Some(request_rx)),
        upgrade_rx: Mutex::new(Some(upgrade_rx)),
        listening: AtomicBool::new(true),
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

    // `listen()` is now non-blocking — the accept loop is already
    // spawned above, and `js_fastify_process_pending` drains pending
    // requests from the registered handle on every tick of
    // perry-stdlib's main pump. Pre-fix this function entered the
    // blocking `event_loop(...)` and never returned, so
    // `await app.listen(...)` in user code never resumed — every
    // subsequent line (the in-process `fetch` against itself,
    // `app.close()`, etc.) was unreachable.
}

/// Close one specific server by its `FastifyServerHandle` id. Marks
/// the server as no-longer-listening (so `js_fastify_has_active`
/// stops reporting it as active), drops the request receiver, and
/// fires the shutdown oneshot so the accept loop exits. Idempotent —
/// safe to call multiple times.
#[no_mangle]
pub unsafe extern "C" fn js_fastify_close(server_handle: Handle) -> bool {
    if let Some(server) = get_handle_mut::<FastifyServerHandle>(server_handle) {
        server.listening.store(false, Ordering::Release);
        *server.request_rx.lock().unwrap() = None;
        *server.upgrade_rx.lock().unwrap() = None;
        if let Some(tx) = server.shutdown_tx.take() {
            let _ = tx.send(());
        }
        return true;
    }
    false
}

/// `app.close()` — close every server bound to `app_handle`. Walks the
/// handle registry for matching `FastifyServerHandle` rows and marks
/// each as no-longer listening so `js_fastify_has_active` lets the
/// runtime's event loop exit. Returns void — TS-side dispatch arm
/// just discards the result.
#[no_mangle]
pub unsafe extern "C" fn js_fastify_app_close(app_handle: Handle) {
    let mut server_ids: Vec<Handle> = Vec::new();
    iter_handle_ids_of::<FastifyServerHandle, _>(|id| {
        if let Some(s) = get_handle::<FastifyServerHandle>(id) {
            if s.app_handle == app_handle {
                server_ids.push(id);
            }
        }
    });
    for id in server_ids {
        let _ = js_fastify_close(id);
    }
}

/// Cap on the per-thread deferred-request queue (see
/// `js_fastify_process_pending`). A pathological awaited-handler storm that
/// exceeds this drops the newest pending — its `response_tx` Drop makes the
/// awaiting hyper service-fn resolve `Err` (→ an error response), so memory is
/// bounded (backpressure) rather than growing without limit.
const DEFERRED_QUEUE_CAP: usize = 4096;

/// Non-blocking `try_recv` of one pending request from a server's channel.
/// Holds the `request_rx` mutex only for the `try_recv` itself, never across
/// dispatch. Returns `None` when the channel is empty / the handle is gone.
fn try_recv_pending_request(server_handle: Handle) -> Option<FastifyPendingRequest> {
    let s = get_handle::<FastifyServerHandle>(server_handle)?;
    let mut guard = s.request_rx.lock().unwrap();
    guard.as_mut()?.try_recv().ok()
}

/// Move every currently-queued request out of one server's channel into
/// `deferred` — the nested-pump path (see `js_fastify_process_pending`). Does
/// NOT dispatch (dispatching in a nested frame would deepen the try-frame
/// nesting toward `MAX_TRY_DEPTH`). Respects `cap`: once `deferred` holds `cap`
/// entries, further pending are dropped, and dropping a `FastifyPendingRequest`
/// drops its `response_tx`, signalling the awaiting service-fn with `Err`.
/// Returns the number actually moved into `deferred`.
fn drain_server_requests_into_deferred(
    server_handle: Handle,
    app_handle: Handle,
    deferred: &mut std::collections::VecDeque<(Handle, FastifyPendingRequest)>,
    cap: usize,
) -> usize {
    let mut moved = 0;
    while let Some(pending) = try_recv_pending_request(server_handle) {
        if deferred.len() < cap {
            deferred.push_back((app_handle, pending));
            moved += 1;
        }
        // else: `pending` is dropped here → response_tx Drop → service-fn Err.
    }
    moved
}

/// Drain & fire every queued WebSocket upgrade for one server (#1113). Upgrades
/// are handled before request traffic — including *between* deferred-request
/// dispatches in the pump's tail loop — so a busy / replenishing request stream
/// can't starve them. Returns the number fired.
fn drain_server_upgrades(server_handle: Handle) -> i32 {
    let mut count = 0i32;
    while let Some(up) = try_recv_fastify_upgrade(server_handle) {
        let req_bits =
            unsafe { crate::upgrade::build_request_object(&up.method, &up.path, &up.headers) }
                .to_bits() as i64;
        crate::upgrade::fire_fastify_upgrade_listeners(
            up.app_handle,
            req_bits,
            up.ws_id,
            Vec::new(),
        );
        count += 1;
    }
    count
}

/// Pump entrypoint — drain pending requests from every registered
/// `FastifyServerHandle` and dispatch each on the main TS thread.
/// Wired into perry-stdlib's `js_stdlib_process_pending` via the
/// `external-fastify-pump` feature so the runtime's outer event loop
/// drives us each tick.
///
/// Re-entrancy: dispatching a request runs user TS, which may `await`;
/// `wait_for_promise` then drives `js_run_stdlib_pump`, which calls back into
/// this pump. Each nested dispatch would push another `call_closure2_catching`
/// try-frame (setjmp), so under sustained awaited-handler load the recursion
/// could blow `MAX_TRY_DEPTH` (128 — "Try block nesting too deep"). An
/// `IN_PROGRESS` thread-local guard makes a nested entry capture pending
/// requests into a thread-local `DEFERRED` queue WITHOUT dispatching, and the
/// outer frame drains `DEFERRED` at its tail — so every dispatch happens at the
/// outer (bounded) try-depth.
///
/// Returns the number of requests processed (matches the convention
/// other pump arms follow — `js_ws_process_pending`,
/// `js_node_http_server_process_pending`, etc.).
#[no_mangle]
pub extern "C" fn js_fastify_process_pending() -> i32 {
    // #1114: called every iteration of the generated event loop AND
    // every inline `await` poll loop. A fresh `Vec<Handle>` per call is
    // a high-frequency alloc/free that shows up as GC `madvise`
    // page-churn under sustained async load (the wedge signature).
    // Reuse a per-thread scratch buffer; move it out (not borrow it)
    // across `process_request` since that dispatches user TS which can
    // re-enter this pump. Mirror of the bundled `perry-stdlib::fastify`
    // fix — either crate can be live depending on the well-known flip.
    thread_local! {
        static SCRATCH: std::cell::RefCell<Vec<Handle>> =
            const { std::cell::RefCell::new(Vec::new()) };
        // Re-entrancy guard + deferred queue (see the fn doc-comment). Both are
        // main-thread-only (the pump runs only on the main TS thread).
        static IN_PROGRESS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
        static DEFERRED: std::cell::RefCell<std::collections::VecDeque<(Handle, FastifyPendingRequest)>> =
            const { std::cell::RefCell::new(std::collections::VecDeque::new()) };
    }

    if IN_PROGRESS.with(|f| f.replace(true)) {
        // Nested entry (a dispatched handler's await re-drove the pump): capture
        // every server's pending into DEFERRED for the outer frame to dispatch;
        // do NOT dispatch here — that would deepen the try-frame nesting. Collect
        // ids first (don't call `get_handle` inside `iter_handle_ids_of`).
        let mut ids: Vec<Handle> = Vec::new();
        iter_handle_ids_of::<FastifyServerHandle, _>(|id| ids.push(id));
        DEFERRED.with(|q| {
            let mut q = q.borrow_mut();
            for &h in &ids {
                if let Some(app_handle) = get_handle::<FastifyServerHandle>(h).map(|s| s.app_handle)
                {
                    drain_server_requests_into_deferred(h, app_handle, &mut q, DEFERRED_QUEUE_CAP);
                }
            }
        });
        return 0;
    }
    // Reset the guard on EVERY exit path (incl. an early return / unwind).
    struct ResetReentryGuard;
    impl Drop for ResetReentryGuard {
        fn drop(&mut self) {
            IN_PROGRESS.with(|f| f.set(false));
        }
    }
    let _reset = ResetReentryGuard;

    let mut server_handles = SCRATCH.with(|s| std::mem::take(&mut *s.borrow_mut()));
    server_handles.clear();
    iter_handle_ids_of::<FastifyServerHandle, _>(|id| {
        server_handles.push(id);
    });
    let mut count = 0i32;
    for &h in server_handles.iter() {
        let app_handle = match get_handle::<FastifyServerHandle>(h) {
            Some(s) => s.app_handle,
            None => continue,
        };
        // #1113 — drain WebSocket upgrades FIRST so a busy request
        // stream can't starve them (mirror of perry-ext-http-server's
        // `js_node_http_server_process_pending`).
        count += drain_server_upgrades(h);
        while let Some(pending) = try_recv_pending_request(h) {
            process_request(app_handle, pending);
            count += 1;
        }
    }

    // Tail: dispatch anything a nested pump call captured into DEFERRED. These
    // were already `try_recv`'d out of the per-server channels, so we dispatch
    // the captured structs here in the OUTER frame — the try-frame depth stays
    // at the outer (bounded) level. A dispatch here may itself await and re-enter
    // the pump; that nested call hits the guard above and appends to DEFERRED,
    // which this same loop then drains — still all at the outer try-depth.
    //
    // Re-drain upgrades before each deferred dispatch: a long (nested-await
    // replenished) deferred drain must not starve WebSocket upgrades that arrive
    // after the initial pass. Nested entries defer only REQUESTS, so the outer
    // frame is where late upgrades get picked up — keep them ahead of requests
    // here too (#1113).
    loop {
        for &h in server_handles.iter() {
            count += drain_server_upgrades(h);
        }
        let next = DEFERRED.with(|q| q.borrow_mut().pop_front());
        let (app_handle, pending) = match next {
            Some(t) => t,
            None => break,
        };
        process_request(app_handle, pending);
        count += 1;
    }

    server_handles.clear();
    SCRATCH.with(|s| {
        let mut slot = s.borrow_mut();
        if server_handles.capacity() >= slot.capacity() {
            *slot = server_handles;
        }
    });
    count
}

/// #1113 — non-blocking try_recv for a pending WebSocket upgrade.
/// Mirror of perry-ext-http-server's `try_recv_upgrade`.
fn try_recv_fastify_upgrade(server_handle: Handle) -> Option<FastifyPendingUpgrade> {
    if let Some(s) = get_handle::<FastifyServerHandle>(server_handle) {
        let mut guard = s.upgrade_rx.lock().unwrap();
        if let Some(rx) = guard.as_mut() {
            return rx.try_recv().ok();
        }
    }
    None
}

/// Reports whether any registered fastify server is currently in the
/// "listening" state OR has a non-empty upgrade queue. Wired into
/// perry-stdlib's `js_stdlib_has_active_handles` so the runtime's main
/// event loop keeps running until the user explicitly closes every
/// server and every queued upgrade has been drained.
#[no_mangle]
pub extern "C" fn js_fastify_has_active() -> i32 {
    let mut active = 0i32;
    iter_handle_ids_of::<FastifyServerHandle, _>(|id| {
        if let Some(s) = get_handle::<FastifyServerHandle>(id) {
            if s.listening.load(Ordering::Acquire) {
                active = 1;
            }
            // Even after close(), the upgrade channel may still hold
            // queued items the pump needs to drain on a later tick
            // before the program can exit cleanly (mirror of
            // perry-ext-http-server's `server_is_active`).
            if let Ok(guard) = s.upgrade_rx.lock() {
                if let Some(rx) = guard.as_ref() {
                    if !rx.is_closed() && !rx.is_empty() {
                        active = 1;
                    }
                }
            }
        }
    });
    active
}

// ============================================================================
// Request dispatch
// ============================================================================

/// Hyper service function — match the route, hand the request to the
/// main thread via mpsc, await the response.
async fn handle_request(
    app_handle: Handle,
    req: Request<Incoming>,
    request_tx: Arc<mpsc::Sender<FastifyPendingRequest>>,
    upgrade_tx: Arc<mpsc::Sender<FastifyPendingUpgrade>>,
    routes: Arc<Vec<RouteMatcher>>,
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

    // #1113: detect WebSocket upgrade requests. The user's pattern
    //
    //   import { WebSocketServer } from "ws";
    //   const wss = new WebSocketServer({ noServer: true });
    //   app.server.on("upgrade", (req, socket, head) => {
    //     wss.handleUpgrade(req, socket, head, (sock) => { ... });
    //   });
    //
    // expects the fastify accept loop to surface upgrade requests via
    // `app.server`'s registered `"upgrade"` handler. Branch into the
    // handshake path: build the 101 response synchronously and spawn
    // a task that awaits hyper's upgraded stream, completes the
    // tungstenite server handshake, registers the WebSocketStream
    // with perry-ext-ws, and queues a `FastifyPendingUpgrade` for the
    // main-thread pump to fire the registered handlers. Mirror of
    // perry-ext-http-server's #577 Phase 4 path.
    if crate::upgrade::is_websocket_upgrade(&req) {
        return handle_fastify_websocket_upgrade(
            app_handle, req, method, path, headers, upgrade_tx,
        )
        .await;
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

    // Match: first try the exact method, then — for HEAD — fall back to
    // a GET route with the same path. Node fastify auto-handles HEAD
    // against any registered GET (via `app.head` shadowing) by running
    // the GET handler and dropping the body before sending. We do the
    // same: rewrite the method to GET so the handler sees a vanilla
    // request, then strip the body on the way out (see `head_for_get`
    // below). #1120 part 2.
    let mut matched_params = HashMap::new();
    let mut found_route = false;
    let mut head_for_get = false;
    for route in routes.iter() {
        if route.method == method {
            if let Some(params) = route.pattern.match_path(&path) {
                matched_params = params;
                found_route = true;
                break;
            }
        }
    }
    if !found_route && method == "HEAD" {
        for route in routes.iter() {
            if route.method == "GET" {
                if let Some(params) = route.pattern.match_path(&path) {
                    matched_params = params;
                    found_route = true;
                    head_for_get = true;
                    break;
                }
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
    // When fronting a GET handler for an inbound HEAD, surface the
    // method as `GET` to the handler — Node fastify's shadowing
    // semantics. The body-drop happens below in the hyper response
    // assembly.
    let dispatch_method = if head_for_get {
        "GET".to_string()
    } else {
        method.clone()
    };
    let pending = FastifyPendingRequest {
        method: dispatch_method,
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
            let body_len = fr.body.len();
            let mut builder = Response::builder()
                .status(StatusCode::from_u16(fr.status).unwrap_or(StatusCode::OK));
            let mut had_content_length = false;
            for (name, value) in fr.headers {
                if name.eq_ignore_ascii_case("content-length") {
                    had_content_length = true;
                }
                builder = builder.header(name, value);
            }
            let body_bytes = if head_for_get {
                // HEAD response: no body on the wire, but expose the
                // would-have-been size via Content-Length so clients
                // (curl -I, browsers, monitoring) see what GET would
                // produce. Mirror of Node fastify's HEAD-on-GET path.
                if !had_content_length {
                    builder = builder.header("content-length", body_len.to_string());
                }
                Bytes::new()
            } else {
                Bytes::from(fr.body)
            };
            Ok(builder.body(Full::new(body_bytes)).unwrap())
        }
        Err(_) => Ok(Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Full::new(Bytes::from("Handler error")))
            .unwrap()),
    }
}

/// #1113 — WebSocket upgrade dispatch (mirror of perry-ext-http-server's
/// `handle_websocket_upgrade`, issue #577 Phase 4).
///
/// Synchronously builds the 101 response (so hyper drives the protocol
/// switch) and spawns a tokio task that awaits the upgraded stream,
/// finishes the handshake server-side via
/// `tokio_tungstenite::WebSocketStream::from_raw_socket`, registers
/// the stream with perry-ext-ws, and queues a `FastifyPendingUpgrade`
/// on the per-server channel; the main-thread pump fires the
/// `app.server.on("upgrade", …)` handlers with `(req, ws_id, head)`.
async fn handle_fastify_websocket_upgrade(
    app_handle: Handle,
    mut req: Request<Incoming>,
    method: String,
    path: String,
    headers: HashMap<String, String>,
    upgrade_tx: Arc<mpsc::Sender<FastifyPendingUpgrade>>,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    // Compute the Sec-WebSocket-Accept value before consuming req.
    let accept_value = req
        .headers()
        .get("sec-websocket-key")
        .and_then(|v| v.to_str().ok())
        .map(|k| tokio_tungstenite::tungstenite::handshake::derive_accept_key(k.as_bytes()))
        .unwrap_or_default();

    // Spawn a task that waits for hyper to perform the protocol
    // switch, completes the tungstenite handshake, and hands the
    // resulting stream to perry-ext-ws.
    tokio::spawn(async move {
        let upgraded = match hyper::upgrade::on(&mut req).await {
            Ok(u) => u,
            Err(_) => return,
        };
        let io = TokioIo::new(upgraded);
        let ws = tokio_tungstenite::WebSocketStream::from_raw_socket(
            io,
            tokio_tungstenite::tungstenite::protocol::Role::Server,
            None,
        )
        .await;
        let ws_id = perry_ext_ws::register_external_ws_stream(ws);
        let pending = FastifyPendingUpgrade {
            app_handle,
            method,
            path,
            headers,
            ws_id,
        };
        let _ = upgrade_tx.send(pending).await;
        perry_ffi::notify_main_thread();
    });

    Ok(Response::builder()
        .status(101)
        .header("upgrade", "websocket")
        .header("connection", "Upgrade")
        .header("sec-websocket-accept", accept_value)
        .body(Full::new(Bytes::new()))
        .unwrap())
}

/// Build the per-request [`FastifyContext`], MOVING the pending request's
/// headers/body/params out of `pending` (via `mem::take` / `Option::take`)
/// rather than cloning them. Each is consumed exactly once, so cloning would
/// fire three redundant per-request allocations (two `HashMap`s + one `Vec`,
/// dominated by the O(headers) header-map clone) on the hot dispatch path.
/// `method`/`path` stay cloned: `process_request` reuses them for the route
/// match (`app.match_route(&pending.method, &pending.path)`) after the context
/// is built. This is the single construction site `process_request` uses, so a
/// regression test can drive it directly.
pub(crate) fn build_context_from_pending(
    request_id: u64,
    pending: &mut FastifyPendingRequest,
) -> FastifyContext {
    FastifyContext::new(
        request_id,
        pending.method.clone(),
        pending.path.clone(),
        std::mem::take(&mut pending.headers),
        pending.body.take(),
        std::mem::take(&mut pending.params),
    )
}

/// Process one request — fire hooks, call route handler, send the
/// response back through the oneshot channel.
pub(crate) fn process_request(app_handle: Handle, mut pending: FastifyPendingRequest) -> Handle {
    let ctx = build_context_from_pending(0, &mut pending);
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
        // Track whether the body came back as binary (Buffer / Uint8Array)
        // so the default content-type below picks octet-stream over JSON
        // when the handler didn't pin one via `reply.type(...)` (#1120).
        let (body, body_kind) = if let Some(b) = ctx.response_body.clone() {
            // The body was set explicitly by `reply.send(...)`, which
            // already pushed an `application/octet-stream` default if
            // the payload was binary and no content-type was pinned.
            // Treat it as text/json here so we don't override.
            (b, BodyKind::TextOrJson)
        } else {
            unsafe { build_response_body(final_result) }
        };
        let mut response = FastifyResponse {
            status: ctx.status_code,
            headers: ctx.response_headers.clone(),
            body,
        };
        if !response
            .headers
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("content-type"))
        {
            let ct = match body_kind {
                BodyKind::Binary => "application/octet-stream",
                BodyKind::TextOrJson => "application/json",
            };
            response
                .headers
                .push(("content-type".to_string(), ct.to_string()));
        }
        let _ = pending.response_tx.send(response);
    }

    // Free the context handle so it doesn't leak.
    perry_ffi::drop_handle(ctx_handle);

    // Return the (now-freed) context handle so the register → drop lifecycle
    // is observable: the `context_handle_dropped_after_dispatch` regression
    // test drives a real request through this function and asserts the handle
    // is gone, which a missing `drop_handle` above would fail. The live
    // dispatch loop calls this as a statement and discards the value.
    ctx_handle
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
                        let (bytes, kind) = unsafe { build_response_body(final_result) };
                        // #1120: when the hook chain returned a Buffer /
                        // Uint8Array and no content-type was pinned, default
                        // to octet-stream so the request finalization step
                        // doesn't paint over it with `application/json`.
                        if kind == BodyKind::Binary
                            && !ctx
                                .response_headers
                                .iter()
                                .any(|(k, _)| k.eq_ignore_ascii_case("content-type"))
                        {
                            ctx.response_headers.push((
                                "content-type".to_string(),
                                "application/octet-stream".to_string(),
                            ));
                        }
                        ctx.response_body = Some(bytes);
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
        let (s, _) = jsvalue_to_response_body(reason);
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
    let (body, _) = jsvalue_to_response_body(reason);
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
/// return strings (used as-is), `Buffer` / `Uint8Array` (raw bytes,
/// see #1120), objects/arrays (JSON-stringified), numbers/bools
/// (toString), or `undefined` (empty `{}`). The returned `BodyKind`
/// signals whether the caller should default `content-type` to
/// `application/octet-stream` (binary) instead of `application/json`.
unsafe fn build_response_body(value: f64) -> (Vec<u8>, BodyKind) {
    let jsv = JsValue::from_bits(value.to_bits());
    if jsv.is_undefined() || jsv.is_null() {
        return (b"{}".to_vec(), BodyKind::TextOrJson);
    }
    if jsv.is_string() {
        return jsvalue_to_response_body(value);
    }
    // Issue #1120 — Buffer / Uint8Array must ship their raw bytes,
    // not the Buffer.toJSON `{"type":"Buffer","data":[...]}` form
    // that `js_json_stringify` produces. Probe BUFFER_REGISTRY
    // first; only fall through to JSON for non-buffer pointers.
    if let Some(bytes) = extract_buffer_bytes(value) {
        return (bytes, BodyKind::Binary);
    }
    if jsv.is_pointer() {
        let str_ptr = js_json_stringify(value, 0);
        if !str_ptr.is_null() {
            let len = (*str_ptr).byte_len as usize;
            let data_ptr = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
            return (
                std::slice::from_raw_parts(data_ptr, len).to_vec(),
                BodyKind::TextOrJson,
            );
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

#[cfg(test)]
mod tests {
    //! Tests for the re-entrancy deferred-queue path of
    //! `js_fastify_process_pending`. These exercise the real
    //! `drain_server_requests_into_deferred` / `try_recv_pending_request`
    //! helpers against a live `FastifyServerHandle` + mpsc channel — no JS
    //! runtime needed (the drain path is pure Rust). The full pump re-entry
    //! (guard → defer → outer-tail dispatch) is exercised end-to-end by
    //! `scripts/run_fastify_tests.sh`, which runs awaited handlers.
    use super::*;
    use perry_ffi::drop_handle;
    use std::collections::{HashMap, VecDeque};

    /// Build a pending request tagged by `path`; return it plus its response
    /// receiver so a test can observe `response_tx`'s fate (kept open vs dropped).
    fn make_pending(path: &str) -> (FastifyPendingRequest, oneshot::Receiver<FastifyResponse>) {
        let (response_tx, response_rx) = oneshot::channel::<FastifyResponse>();
        let pending = FastifyPendingRequest {
            method: "GET".to_string(),
            path: path.to_string(),
            headers: HashMap::new(),
            body: None,
            params: HashMap::new(),
            response_tx,
        };
        (pending, response_rx)
    }

    /// Register a `FastifyServerHandle` wrapping `rx`. Returns (server, app)
    /// handles; the caller drops both.
    fn register_test_server(rx: mpsc::Receiver<FastifyPendingRequest>) -> (Handle, Handle) {
        let app_handle = register_handle(FastifyApp::new());
        let server = FastifyServerHandle {
            port: 0,
            app_handle,
            shutdown_tx: None,
            request_rx: Mutex::new(Some(rx)),
            upgrade_rx: Mutex::new(None),
            listening: AtomicBool::new(true),
        };
        (register_handle(server), app_handle)
    }

    /// Cap + FIFO + full-drain: over-cap entries are dropped, the kept ones stay
    /// in arrival order, and — critically — the channel is fully drained even
    /// past the cap (no request is left stranded in the mpsc).
    #[test]
    fn deferred_drain_respects_cap_and_drains_channel_fully() {
        let n = 10usize;
        let cap = 4usize;
        let (tx, rx) = mpsc::channel::<FastifyPendingRequest>(n + 1);
        let (server_h, app_h) = register_test_server(rx);
        let mut rxs = Vec::new();
        for i in 0..n {
            let (p, r) = make_pending(&format!("req-{i}"));
            tx.try_send(p).expect("send");
            rxs.push(r);
        }

        let mut deferred: VecDeque<(Handle, FastifyPendingRequest)> = VecDeque::new();
        let moved = drain_server_requests_into_deferred(server_h, app_h, &mut deferred, cap);

        assert_eq!(moved, cap, "exactly `cap` entries are moved");
        assert_eq!(deferred.len(), cap);
        for (i, (h, p)) in deferred.iter().enumerate() {
            assert_eq!(*h, app_h, "captured under the right app handle");
            assert_eq!(p.path, format!("req-{i}"), "FIFO arrival order preserved");
        }
        assert!(
            try_recv_pending_request(server_h).is_none(),
            "channel must be fully drained even past the cap"
        );

        drop_handle(server_h);
        drop_handle(app_h);
        drop(rxs);
    }

    /// Under cap: every request is captured, in arrival order, and none is
    /// dropped — every kept request's response channel stays open (the client
    /// will get a real response, not an error).
    #[test]
    fn deferred_drain_under_cap_moves_all_without_dropping() {
        let n = 5usize;
        let (tx, rx) = mpsc::channel::<FastifyPendingRequest>(n + 1);
        let (server_h, app_h) = register_test_server(rx);
        let mut rxs = Vec::new();
        for i in 0..n {
            let (p, r) = make_pending(&format!("r{i}"));
            tx.try_send(p).expect("send");
            rxs.push(r);
        }

        let mut deferred: VecDeque<(Handle, FastifyPendingRequest)> = VecDeque::new();
        let moved =
            drain_server_requests_into_deferred(server_h, app_h, &mut deferred, DEFERRED_QUEUE_CAP);

        assert_eq!(moved, n, "all moved when under cap");
        for (i, (_, p)) in deferred.iter().enumerate() {
            assert_eq!(p.path, format!("r{i}"));
        }
        // Nothing dropped: every response channel is still open (Empty, not Closed).
        for r in &mut rxs {
            assert!(
                matches!(r.try_recv(), Err(oneshot::error::TryRecvError::Empty)),
                "kept request's response_tx must remain alive"
            );
        }

        drop_handle(server_h);
        drop_handle(app_h);
    }

    /// Backpressure contract: when the cap is hit, the OVER-cap requests are
    /// dropped, and dropping a pending drops its `response_tx`, which makes the
    /// awaiting hyper service-fn resolve `Closed` → an error response. This
    /// proves the client is signalled (no hang / no leak) rather than the
    /// request silently vanishing.
    #[test]
    fn deferred_drain_over_cap_signals_dropped_clients() {
        let n = 8usize;
        let cap = 3usize;
        let (tx, rx) = mpsc::channel::<FastifyPendingRequest>(n + 1);
        let (server_h, app_h) = register_test_server(rx);
        let mut rxs = Vec::new();
        for i in 0..n {
            let (p, r) = make_pending(&format!("q{i}"));
            tx.try_send(p).expect("send");
            rxs.push(r);
        }

        let mut deferred: VecDeque<(Handle, FastifyPendingRequest)> = VecDeque::new();
        drain_server_requests_into_deferred(server_h, app_h, &mut deferred, cap);

        // Kept entries: response channel still open.
        for r in rxs.iter_mut().take(cap) {
            assert!(
                matches!(r.try_recv(), Err(oneshot::error::TryRecvError::Empty)),
                "kept entries stay open"
            );
        }
        // Over-cap entries: dropped → response_tx Drop → client observes Closed.
        for r in rxs.iter_mut().skip(cap) {
            assert!(
                matches!(r.try_recv(), Err(oneshot::error::TryRecvError::Closed)),
                "over-cap entries must signal the client (error response, not a hang)"
            );
        }

        drop_handle(server_h);
        drop_handle(app_h);
    }

    /// No-loss / no-duplication property across multiple servers + repeated
    /// drain passes (simulating repeated nested pump entries): every queued
    /// request lands in DEFERRED exactly once, in per-server FIFO order, and a
    /// second pass captures nothing (channels already drained).
    #[test]
    fn deferred_drain_no_loss_across_servers_and_repeated_passes() {
        let mut deferred: VecDeque<(Handle, FastifyPendingRequest)> = VecDeque::new();
        let mut servers: Vec<(
            Handle,
            Handle,
            mpsc::Sender<FastifyPendingRequest>,
            usize,
            usize,
        )> = Vec::new();
        let mut expected_total = 0usize;
        for (s, depth) in [(0usize, 1usize), (1, 5), (2, 0), (3, 12)] {
            let (tx, rx) = mpsc::channel::<FastifyPendingRequest>(depth + 1);
            let (server_h, app_h) = register_test_server(rx);
            for i in 0..depth {
                let (p, _r) = make_pending(&format!("s{s}-r{i}"));
                tx.try_send(p).expect("send");
                // `_r` dropped here is harmless: it never affects the moved sender.
            }
            expected_total += depth;
            servers.push((server_h, app_h, tx, s, depth));
        }

        // First pass: capture everything exactly once.
        for (server_h, app_h, _tx, _s, _d) in &servers {
            drain_server_requests_into_deferred(
                *server_h,
                *app_h,
                &mut deferred,
                DEFERRED_QUEUE_CAP,
            );
        }
        assert_eq!(
            deferred.len(),
            expected_total,
            "every request captured exactly once"
        );

        // Second pass: nothing new (no loss, no double-capture).
        let before = deferred.len();
        for (server_h, app_h, _tx, _s, _d) in &servers {
            drain_server_requests_into_deferred(
                *server_h,
                *app_h,
                &mut deferred,
                DEFERRED_QUEUE_CAP,
            );
        }
        assert_eq!(deferred.len(), before, "repeated drain captures nothing");

        // Per-server FIFO order preserved.
        for (_, app_h, _tx, s, depth) in &servers {
            let seq: Vec<String> = deferred
                .iter()
                .filter(|(h, _)| h == app_h)
                .map(|(_, p)| p.path.clone())
                .collect();
            let want: Vec<String> = (0..*depth).map(|i| format!("s{s}-r{i}")).collect();
            assert_eq!(seq, want, "per-server FIFO order");
        }

        for (server_h, app_h, _tx, _s, _d) in servers {
            drop_handle(server_h);
            drop_handle(app_h);
        }
    }
}
