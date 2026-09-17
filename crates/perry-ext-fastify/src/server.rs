//! HTTP server loop and request dispatch.
//!
//! # Two transports, and which one a server gets
//!
//! The transport is **turnloop**, through [`perry_http_server`]: one multishot
//! accept, one multishot read, a sans-I/O `turnloop_http::http1` codec, and no
//! task, no thread hop and no cross-thread notify anywhere on the request
//! path. `js_fastify_listen` binds synchronously (so the `(err, address)`
//! callback reports the real port), installs a [`FastifyHost`], and returns;
//! `js_fastify_process_pending` drains the decoded requests on the main thread
//! each tick, exactly where the hyper path's `mpsc` delivered them.
//!
//! There is no second transport. The hyper accept loop survived for one case —
//! an app with `app.server.on("upgrade", …)` handlers, whose handshake ended in
//! `perry_ext_ws::register_external_ws_stream` and needed an owned
//! `AsyncRead + AsyncWrite` stream a turnloop connection cannot produce — and
//! that blocker is gone: `perry_ext_ws::accept_http_upgrade` answers the
//! handshake over bytes on the connection the core already owns, through
//! `Host::on_upgrade`. An agent with no `turnloop::Loop` (a `worker_threads`
//! agent, the `tokio-wait-driver` A/B arm) therefore has no fastify server at
//! all, and `listen()` reports that through its `(err, address)` callback
//! rather than falling back. See `docs/turnloop/fastify-report.md`.

use std::collections::HashMap;
use std::os::raw::c_int;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use perry_ffi::{
    alloc_string, get_handle, get_handle_mut, iter_handle_ids_of, read_bytes, register_handle,
    Handle, JsClosure, JsString, JsValue, RawClosureHeader, StringHeader,
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
    /// off the main thread get dispatched on it. See #747.
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

    /// Condvar-based wait for the next event (timer fire, a notify from
    /// whichever thread produced one, or 1 s idle cap). Used by
    /// `wait_for_promise` so the
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

extern "C" {
    /// The runtime's C setjmp trampoline (#9305, `perry_sjlj.c`, bundled in
    /// `libperry_runtime.a`). rustc cannot express `returns_twice`, so a raw
    /// `setjmp` call in a Rust frame is miscompiled under LLVM's one-return
    /// assumption (stack-slot coloring across the call); every jmp_buf arm
    /// goes through this C frame instead. Mirrors
    /// `perry_runtime::exception::arm_trap_and_run`.
    fn perry_sjlj_try(
        env: *mut core::ffi::c_void,
        body: unsafe extern "C" fn(*mut core::ffi::c_void),
        ctx: *mut core::ffi::c_void,
    ) -> c_int;
}

/// Arm `env` (from `js_try_push`) inside the C trampoline and run `f` under
/// it. `None` = a JS throw longjmp-landed (exception state set, trap still
/// pushed). Local mirror of `perry_runtime::exception::arm_trap_and_run` —
/// this crate deliberately has no Cargo dep on perry-runtime.
fn arm_trap_and_run<R, F: FnOnce() -> R>(env: *mut c_int, f: F) -> Option<R> {
    struct Ctx<F, R> {
        f: Option<F>,
        ret: Option<R>,
    }
    unsafe extern "C" fn invoke<F: FnOnce() -> R, R>(raw: *mut core::ffi::c_void) {
        let ctx = unsafe { &mut *(raw as *mut Ctx<F, R>) };
        let f = ctx.f.take().expect("sjlj trampoline invoked body twice");
        ctx.ret = Some(f());
    }
    let mut ctx: Ctx<_, R> = Ctx {
        f: Some(f),
        ret: None,
    };
    let rc = unsafe {
        perry_sjlj_try(
            env as *mut core::ffi::c_void,
            invoke::<F, R>,
            &mut ctx as *mut Ctx<_, R> as *mut core::ffi::c_void,
        )
    };
    if rc == 0 {
        Some(
            ctx.ret
                .take()
                .expect("sjlj trampoline returned 0 without a body result"),
        )
    } else {
        None
    }
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
/// mirrors what perry-ext-http did in #604: `listen()` returns
/// immediately after spawning the accept loop, and a new
/// `js_fastify_process_pending` extern wired into perry-stdlib's main
/// pump drains the per-server mpsc each tick. The receiver lives
/// inside the handle so the pump can find it after `listen()` returns.
pub struct FastifyServerHandle {
    pub port: u16,
    pub app_handle: Handle,
    /// The `perry_http_server` listener. `js_fastify_close` stops accepting
    /// through `perry_http_server::close_listener`; there is no second
    /// shutdown channel now that there is no second accept loop.
    pub listener_id: i64,
    /// Drained by `js_fastify_process_pending` from the main TS thread each
    /// tick. The producer is the completion sink on *this* thread, so the
    /// channel is a same-thread hand-off rather than a thread hop. Bounded at
    /// [`REQUEST_QUEUE_DEPTH`], which is the backpressure: a full queue is
    /// answered `503` at once instead of growing without limit.
    ///
    /// `Mutex` because the handle registry hands out `&'static` references but
    /// the pump needs `&mut` access to `try_recv`.
    pub request_rx: Mutex<Option<mpsc::Receiver<FastifyPendingRequest>>>,
    /// #1113 — accepted `app.server.on('upgrade', …)` handshakes, queued by
    /// the completion sink for the main-thread pump to fire.
    pub upgrade_rx: Mutex<Option<mpsc::Receiver<FastifyPendingUpgrade>>>,
    /// How many upgrades have been queued and not yet drained. `std`'s
    /// `Receiver` has no `is_empty`, and the runtime keepalive has to know
    /// whether a queued upgrade is still waiting for the pump.
    pub upgrade_depth: Arc<AtomicUsize>,
    /// True between `listen()` and `close()`. The `js_fastify_has_active`
    /// extern returns 1 while any server has this set, keeping the runtime's
    /// main event loop alive until the user explicitly closes the server.
    pub listening: Arc<AtomicBool>,
}

/// The per-server request queue depth. A request that does not fit is refused
/// with `503` rather than queued, which is what the hyper path got from
/// `mpsc::Sender::send(...).await` resolving `Err` on a closed channel.
const REQUEST_QUEUE_DEPTH: usize = 1024;

/// The per-server upgrade queue depth. The same 256 the hyper path used: an
/// upgrade is a handshake per *connection*, not per request, so it does not
/// need the request queue's headroom.
const UPGRADE_QUEUE_DEPTH: usize = 256;

/// #1113 — pending WebSocket upgrade ready to fire the fastify
/// `app.server.on("upgrade", …)` handlers. Queued by the completion
/// sink once the `101` has been written and the upgraded connection
/// has been adopted by `perry_ext_ws::accept_http_upgrade`.
pub struct FastifyPendingUpgrade {
    pub app_handle: Handle,
    pub method: String,
    pub path: String,
    pub headers: HashMap<String, String>,
    pub ws_id: i64,
}

/// Where a dispatched request's response goes.
///
/// This is what replaced the `oneshot::Sender<FastifyResponse>` the hyper
/// service fn awaited. The handler now runs on the thread that owns the
/// connection, so there is nothing to wake: the response encodes and submits
/// its own write.
pub enum Reply {
    /// A live `perry_http_server` exchange: the connection id and the sequence
    /// number that names the request on it. A `seq` that is no longer the
    /// connection's active request is ignored by the core rather than
    /// mis-delivered, which is what makes a late response from an abandoned
    /// handler harmless.
    Turnloop { conn_id: i64, seq: u64 },
    /// Nothing is waiting — a reply already sent, or a test that only cares
    /// that the dispatcher ran.
    None,
    /// A test's capture of what the dispatcher produced.
    ///
    /// `#[cfg(test)]` on purpose. This used to be `Hyper(oneshot::Sender<…>)`,
    /// a production variant two unit tests borrowed; with the hyper path gone
    /// the honest replacement is a variant that does not exist in a shipped
    /// build, rather than a second live reply mode nothing reaches.
    #[cfg(test)]
    Captured(std::sync::mpsc::SyncSender<FastifyResponse>),
}

impl Reply {
    /// Send the response, consuming the reply. Returns false when the peer is
    /// already gone.
    fn send(&mut self, response: FastifyResponse) -> bool {
        match std::mem::replace(self, Reply::None) {
            Reply::Turnloop { conn_id, seq } => {
                perry_http_server::respond(conn_id, seq, into_core_response(response));
                true
            }
            Reply::None => false,
            #[cfg(test)]
            Reply::Captured(tx) => tx.send(response).is_ok(),
        }
    }

    /// Refuse the request: the queue was full, or the pending was dropped
    /// before a handler ever ran. Answering here is what keeps the client from
    /// hanging — the hyper path got the same effect from `response_tx`'s Drop
    /// resolving the awaiting service fn `Err`.
    fn refuse(&mut self) {
        if matches!(self, Reply::None) {
            return;
        }
        self.send(FastifyResponse {
            status: 503,
            headers: vec![("content-type".to_string(), "text/plain".to_string())],
            body: b"Server unavailable".to_vec(),
        });
    }
}

/// Pending request waiting for the TS handler to produce a response.
pub struct FastifyPendingRequest {
    pub method: String,
    pub path: String,
    pub headers: HashMap<String, String>,
    pub body: Option<Vec<u8>>,
    pub params: HashMap<String, String>,
    pub reply: Reply,
}

impl Drop for FastifyPendingRequest {
    /// A pending that is dropped without being answered — the deferred queue
    /// hit its cap, or the server closed under it — must still close the
    /// exchange, or the client waits forever. This is the explicit form of the
    /// hyper path's "dropping `response_tx` resolves the service fn `Err`".
    fn drop(&mut self) {
        self.reply.refuse();
    }
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

// The listen path — the bind and the turnloop `Host` — lives in `listen.rs`,
// declared as a `#[path]` child
// module so `use super::*` there resolves exactly as it did inline. Split out
// only to keep this file under the repository's 2000-line-per-file lint cap
// (`scripts/check_file_size.sh`).
#[path = "listen.rs"]
mod listen;
pub use listen::*;

/// Close one specific server by its `FastifyServerHandle` id. Marks
/// the server as no-longer-listening (so `js_fastify_has_active`
/// stops reporting it as active), drops the request and upgrade
/// receivers, and stops the listener accepting. Idempotent — safe to
/// call multiple times.
#[no_mangle]
pub unsafe extern "C" fn js_fastify_close(server_handle: Handle) -> bool {
    if let Some(server) = get_handle_mut::<FastifyServerHandle>(server_handle) {
        server.listening.store(false, Ordering::Release);
        // Dropping the receiver drops every queued pending, and a pending's
        // Drop answers its exchange 503 rather than leaving the client to hang.
        *server.request_rx.lock().unwrap() = None;
        *server.upgrade_rx.lock().unwrap() = None;
        if server.listener_id != 0 {
            // Stop accepting. In-flight connections finish, which is Node's
            // `server.close()` contract.
            perry_http_server::close_listener(server.listener_id);
            server.listener_id = 0;
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
/// exceeds this drops the newest pending — and `FastifyPendingRequest::drop`
/// answers that exchange `503` — so memory is bounded (backpressure) rather
/// than growing without limit, and no client is left hanging.
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
/// answers its exchange `503`.
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
        // else: `pending` is dropped here → its Drop answers the exchange 503.
    }
    moved
}

/// Drain & fire every queued WebSocket upgrade for one server (#1113). Upgrades
/// are handled before request traffic — including *between* deferred-request
/// dispatches in the pump's tail loop — so a busy / replenishing request stream
/// can't starve them. Returns the number fired.
fn drain_server_upgrades(server_handle: Handle) -> i32 {
    let mut count = 0i32;
    let depth = get_handle::<FastifyServerHandle>(server_handle).map(|s| s.upgrade_depth.clone());
    while let Some(up) = try_recv_fastify_upgrade(server_handle) {
        if let Some(d) = depth.as_ref() {
            d.fetch_sub(1, Ordering::AcqRel);
        }
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
/// Registered with perry-runtime when the first Fastify app initializes, so
/// the runtime's outer event loop drives it without a named reference from
/// stdlib.
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
        // stream can't starve them (mirror of perry-ext-http's
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
/// Mirror of perry-ext-http's `try_recv_upgrade`.
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
/// "listening" state OR has a non-empty upgrade queue. Registered as a
/// runtime keepalive contributor so the main event loop keeps running until
/// the user explicitly closes every server and every queued upgrade has been
/// drained.
#[no_mangle]
pub extern "C" fn js_fastify_has_active() -> i32 {
    let mut active = 0i32;
    iter_handle_ids_of::<FastifyServerHandle, _>(|id| {
        if let Some(s) = get_handle::<FastifyServerHandle>(id) {
            if s.listening.load(Ordering::Acquire) {
                active = 1;
            }
            // Even after close(), the upgrade queue may still hold items the
            // pump needs to drain on a later tick before the program can exit
            // cleanly (mirror of perry-ext-http's `server_is_active`).
            // `std::sync::mpsc::Receiver` has no `is_empty`, so the depth is
            // counted explicitly as items are queued and drained.
            if s.upgrade_depth.load(Ordering::Acquire) > 0 {
                active = 1;
            }
        }
    });
    active
}

// ============================================================================
// Request dispatch
// ============================================================================

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
        pending.reply.send(response);
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
    let outcome = arm_trap_and_run(trap_buf, || unsafe { closure.call2(arg0, arg1) });
    match outcome {
        Some(value) => {
            js_try_end();
            ClosureCallResult {
                value,
                thrown: None,
            }
        }
        None => {
            let exc = js_get_exception();
            js_clear_exception();
            js_try_end();
            ClosureCallResult {
                value: f64::from_bits(TAG_UNDEFINED),
                thrown: Some(exc),
            }
        }
    }
}

unsafe fn call_closure3_catching(
    closure: JsClosure,
    arg0: f64,
    arg1: f64,
    arg2: f64,
) -> ClosureCallResult {
    let trap_buf = js_try_push();
    let outcome = arm_trap_and_run(trap_buf, || unsafe { closure.call3(arg0, arg1, arg2) });
    match outcome {
        Some(value) => {
            js_try_end();
            ClosureCallResult {
                value,
                thrown: None,
            }
        }
        None => {
            let exc = js_get_exception();
            js_clear_exception();
            js_try_end();
            ClosureCallResult {
                value: f64::from_bits(TAG_UNDEFINED),
                thrown: Some(exc),
            }
        }
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
    let handle = JsString::from_raw(ptr as *mut StringHeader);
    read_bytes(handle).map(|bytes| String::from_utf8_lossy(bytes).into_owned())
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

/// Read `reusePort: true` from a `{ port, reusePort }` listen-options object.
/// `reusePort` is a real Node (`net` / `http` `listen`) and Bun (`Bun.serve`)
/// option that sets SO_REUSEPORT so multiple processes can share one port;
/// honoring it lets a non-cluster program opt into port sharing directly.
/// Defaults to false for a bare-number `listen(port)` or a missing option.
unsafe fn extract_reuse_port(opts: f64) -> bool {
    let v = JsValue::from_bits(opts.to_bits());
    if v.is_pointer() {
        if let Some(json) = perry_ffi::json_stringify(v) {
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&json) {
                return parsed
                    .get("reusePort")
                    .and_then(|p| p.as_bool())
                    .unwrap_or(false);
            }
        }
    }
    false
}

/// Hand a failed bind to the `(err, address)` listen callback as a Node-style
/// system `Error` (`.code` / `.syscall` / `.errno`), so user code branching on
/// `err.code === 'EADDRINUSE'` sees the failure instead of being told the
/// server is listening. Called before any server registration or success
/// callback, so a port clash can no longer masquerade as a successful listen.
unsafe fn fire_listen_error(callback: i64, e: &std::io::Error, port: u16) {
    eprintln!("[fastify] listen on 0.0.0.0:{} failed: {}", port, e);
    if callback == 0 {
        return;
    }
    let raw = if (callback as u64 & 0xFFFF_0000_0000_0000) == POINTER_TAG {
        (callback as u64 & PTR_MASK) as *const RawClosureHeader
    } else {
        callback as *const RawClosureHeader
    };
    let closure = JsClosure::from_raw(raw);
    if closure.is_null() {
        return;
    }
    let code: std::borrow::Cow<'static, str> = match e.kind() {
        std::io::ErrorKind::AddrInUse => "EADDRINUSE".into(),
        std::io::ErrorKind::PermissionDenied => "EACCES".into(),
        std::io::ErrorKind::AddrNotAvailable => "EADDRNOTAVAIL".into(),
        // Any other bind/setup errno: carry it through as `E<errno>` so the
        // error stays truthy and inspectable rather than being mislabeled.
        _ => match e.raw_os_error() {
            Some(n) => format!("E{}", n).into(),
            None => "EUNKNOWN".into(),
        },
    };
    // Node/libuv reports the negated OS errno.
    let errno = e.raw_os_error().map(|n| -(n as i64)).unwrap_or(0);
    let msg = format!("listen {} 0.0.0.0:{}", code, port);
    let err_val = perry_ffi::system_error_value(&msg, &code, "listen", errno);
    let _ = closure.call2(
        f64::from_bits(err_val.bits()),
        f64::from_bits(TAG_UNDEFINED),
    );
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
    /// receiver so a test can observe the reply's fate — still pending, or
    /// refused because the pending was dropped over the cap.
    fn make_pending(
        path: &str,
    ) -> (
        FastifyPendingRequest,
        std::sync::mpsc::Receiver<FastifyResponse>,
    ) {
        // Capacity 1: every assertion below is "exactly one reply, or none",
        // and a bounded channel makes a second send fail loudly rather than
        // queue behind the first.
        let (response_tx, response_rx) = std::sync::mpsc::sync_channel::<FastifyResponse>(1);
        let pending = FastifyPendingRequest {
            method: "GET".to_string(),
            path: path.to_string(),
            headers: HashMap::new(),
            body: None,
            params: HashMap::new(),
            reply: Reply::Captured(response_tx),
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
            listener_id: 0,
            request_rx: Mutex::new(Some(rx)),
            upgrade_rx: Mutex::new(None),
            upgrade_depth: Arc::new(AtomicUsize::new(0)),
            listening: Arc::new(AtomicBool::new(true)),
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
        let (tx, rx) = mpsc::sync_channel::<FastifyPendingRequest>(n + 1);
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
        let (tx, rx) = mpsc::sync_channel::<FastifyPendingRequest>(n + 1);
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
        // Nothing dropped: no request has been refused.
        for r in &mut rxs {
            assert!(
                matches!(r.try_recv(), Err(std::sync::mpsc::TryRecvError::Empty)),
                "a kept request must not have been answered yet"
            );
        }

        drop_handle(server_h);
        drop_handle(app_h);
    }

    /// Backpressure contract: when the cap is hit, the OVER-cap requests are
    /// dropped, and dropping a pending answers its exchange `503`. This proves
    /// the client is signalled (no hang / no leak) rather than the request
    /// silently vanishing.
    #[test]
    fn deferred_drain_over_cap_signals_dropped_clients() {
        let n = 8usize;
        let cap = 3usize;
        let (tx, rx) = mpsc::sync_channel::<FastifyPendingRequest>(n + 1);
        let (server_h, app_h) = register_test_server(rx);
        let mut rxs = Vec::new();
        for i in 0..n {
            let (p, r) = make_pending(&format!("q{i}"));
            tx.try_send(p).expect("send");
            rxs.push(r);
        }

        let mut deferred: VecDeque<(Handle, FastifyPendingRequest)> = VecDeque::new();
        drain_server_requests_into_deferred(server_h, app_h, &mut deferred, cap);

        // Kept entries: nothing sent yet, the handler has not run.
        for r in rxs.iter_mut().take(cap) {
            assert!(
                matches!(r.try_recv(), Err(std::sync::mpsc::TryRecvError::Empty)),
                "kept entries stay open"
            );
        }
        // Over-cap entries: dropped → `FastifyPendingRequest::drop` refuses the
        // exchange with a 503, so the client is answered rather than hung. The
        // hyper path got this from `response_tx`'s Drop resolving the awaiting
        // service fn `Err`; making it an explicit response is what lets the
        // turnloop path — which has no channel to close — do the same thing.
        for r in rxs.iter_mut().skip(cap) {
            match r.try_recv() {
                Ok(response) => assert_eq!(
                    response.status, 503,
                    "over-cap entries must be answered, not hung"
                ),
                Err(e) => panic!("expected a 503 for a dropped pending, got {e:?}"),
            }
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
            mpsc::SyncSender<FastifyPendingRequest>,
            usize,
            usize,
        )> = Vec::new();
        let mut expected_total = 0usize;
        for (s, depth) in [(0usize, 1usize), (1, 5), (2, 0), (3, 12)] {
            let (tx, rx) = mpsc::sync_channel::<FastifyPendingRequest>(depth + 1);
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
