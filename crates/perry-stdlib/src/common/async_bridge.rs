//! Async bridge: connects Rust async (tokio) with the perry Promise system.
//!
//! The perry runtime has a Promise implementation that expects synchronous
//! resolution callbacks. We need to bridge this with tokio's async runtime
//! for database operations.
//!
//! IMPORTANT: perry-runtime uses thread-local arenas for memory allocation.
//! This means JSValue objects created on tokio worker threads will be allocated
//! from a different arena than the main thread, causing memory corruption.
//!
//! To avoid this, async operations should:
//! 1. NOT create JSValue objects (arrays, strings, objects) in async blocks
//! 2. Store raw Rust data and use deferred conversion callbacks
//! 3. The conversion callbacks run on the main thread during js_stdlib_process_pending

use std::future::Future;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;

use once_cell::sync::Lazy;
use tokio::runtime::Runtime;

/// Issue #859: pin a Promise so the GC can't sweep it while a tokio
/// worker is computing its eventual resolution.
///
/// Without pinning, the await chain has no path back to the Promise:
/// `P.next = N` is a forward edge, and after the user code yields, all
/// JS-side roots reach only `N`. The tokio future holds `promise_ptr`
/// as `usize`, invisible to the GC. So `js_promise_new()` in a native
/// binding + `spawn_for_promise(...)` opens a window where `P` is
/// unreachable; if GC fires during that window, `P` is swept, and
/// when the worker finally calls `js_promise_resolve(P, ...)` it
/// dereferences freed (and possibly OS-reclaimed) memory → SIGBUS.
///
/// Pin/unpin must run on the main thread. The bit is set here (right
/// before crossing the worker boundary) and cleared in
/// [`js_stdlib_process_pending`] after the queued resolution drains.
///
/// # Safety
/// `promise_ptr` must point to a live Promise allocated by
/// `js_promise_new()` — i.e. an `8-byte GcHeader`-prefixed allocation
/// in the GC arena. Callers in `spawn_for_promise[_deferred]` satisfy
/// this trivially; direct callers of [`queue_promise_resolution`] /
/// [`queue_deferred_resolution`] (fetch, zlib, etc.) must also pin
/// before handing the pointer to a worker future.
#[inline]
pub unsafe fn pin_promise_for_native_resolution(promise_ptr: usize) {
    if promise_ptr == 0 {
        return;
    }
    let header = (promise_ptr as *mut u8).sub(perry_runtime::gc::GC_HEADER_SIZE)
        as *mut perry_runtime::gc::GcHeader;
    (*header).gc_flags |= perry_runtime::gc::GC_FLAG_PINNED;
}

/// Inverse of [`pin_promise_for_native_resolution`]; called from
/// `js_stdlib_process_pending` immediately before the queued
/// resolve/reject so the next GC cycle can reclaim the (now-settled)
/// promise on its normal schedule.
#[inline]
unsafe fn unpin_promise_after_native_resolution(promise_ptr: usize) {
    if promise_ptr == 0 {
        return;
    }
    let header = (promise_ptr as *mut u8).sub(perry_runtime::gc::GC_HEADER_SIZE)
        as *mut perry_runtime::gc::GcHeader;
    (*header).gc_flags &= !perry_runtime::gc::GC_FLAG_PINNED;
}

/// Allocate a fresh Promise and pin it for cross-thread resolution.
/// Convenience wrapper for direct callers of [`queue_promise_resolution`]
/// / [`queue_deferred_resolution`] (fetch, zlib, bcrypt, ioredis, ws,
/// etc.) — modules that bypass `spawn_for_promise[_deferred]` because
/// their own future setup is custom. Equivalent to
/// `js_promise_new()` followed by [`pin_promise_for_native_resolution`].
///
/// # Safety
/// Same as `js_promise_new()`; the pinning has no preconditions of
/// its own. The matching unpin runs automatically in
/// `js_stdlib_process_pending`.
#[inline]
pub unsafe fn js_promise_new_for_native_resolution() -> *mut perry_runtime::Promise {
    ensure_gc_scanner_registered();
    let p = perry_runtime::js_promise_new();
    pin_promise_for_native_resolution(p as usize);
    p
}

/// Count of in-flight `perry_ffi_spawn_blocking[_with_reactor]` tasks
/// dispatched by external native bindings (perry-ext-argon2 /
/// -bcrypt / etc. via perry-ffi). Each spawn `fetch_add(1)`s before
/// the closure runs; the closure-trampoline `fetch_sub(1)`s after it
/// returns. `js_stdlib_has_active_handles` returns 1 while this
/// counter is nonzero so the runtime's event loop keeps draining
/// PENDING_RESOLUTIONS / PENDING_DEFERRED until the closure has
/// queued its result.
///
/// Issue #591: without this counter, `await argon2.hash(pw)` returns
/// a Promise whose resolution is queued from a tokio worker AFTER
/// `main()` returns. The runtime saw zero active handles (no WS,
/// net, readline) and exited before the resolution drained, so the
/// `.then` / `await` never fired and the program ran past the await
/// returning undefined.
pub static EXT_BLOCKING_TASKS_INFLIGHT: AtomicUsize = AtomicUsize::new(0);

/// Global tokio runtime for all async stdlib operations.
/// Falls back to current-thread runtime if multi-thread fails (e.g. on iOS).
pub static RUNTIME: Lazy<Runtime> = Lazy::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .unwrap_or_else(|_| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("Failed to create tokio runtime")
        })
});

/// Pending promise resolutions
/// Format: (promise_ptr, is_success, result_value)
static PENDING_RESOLUTIONS: Lazy<Mutex<Vec<PendingResolution>>> =
    Lazy::new(|| Mutex::new(Vec::new()));

/// Pending deferred resolutions - these store raw data and a conversion function
/// that runs on the main thread to create JSValues safely
static PENDING_DEFERRED: Lazy<Mutex<Vec<DeferredResolution>>> =
    Lazy::new(|| Mutex::new(Vec::new()));

thread_local! {
    static GC_SCANNER_REGISTERED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn ensure_gc_scanner_registered() {
    GC_SCANNER_REGISTERED.with(|registered| {
        if registered.get() {
            return;
        }
        perry_runtime::gc::gc_register_mutable_root_scanner_named(
            "stdlib:async_bridge",
            scan_pending_native_async_resolution_roots_mut,
        );
        registered.set(true);
    });
}

/// A pending promise resolution (for simple values that don't need conversion)
struct PendingResolution {
    /// Pointer to the Promise object (as usize for Send)
    promise_ptr: usize,
    /// True if resolved successfully, false if rejected
    is_success: bool,
    /// The result value (as u64 bits for JSValue)
    result_bits: u64,
}

/// A deferred promise resolution with a conversion callback
/// The converter function runs on the main thread to safely create JSValues
struct DeferredResolution {
    /// Pointer to the Promise object (as usize for Send)
    promise_ptr: usize,
    /// True if resolved successfully, false if rejected
    is_success: bool,
    /// Boxed converter function that creates the JSValue on the main thread
    /// Returns the JSValue bits
    converter: Box<dyn FnOnce() -> u64 + Send>,
}

/// Mutable GC scanner for native async completions waiting in stdlib's
/// main-thread pump. Promise pointers are raw heap pointers; simple
/// result bits may be NaN-boxed heap values.
pub fn scan_pending_native_async_resolution_roots_mut(
    visitor: &mut perry_runtime::gc::RuntimeRootVisitor<'_>,
) {
    {
        let mut pending = PENDING_RESOLUTIONS.lock().unwrap();
        for resolution in pending.iter_mut() {
            visitor.visit_usize_slot(&mut resolution.promise_ptr);
            visitor.visit_nanbox_u64_slot(&mut resolution.result_bits);
        }
    }
    {
        let mut pending = PENDING_DEFERRED.lock().unwrap();
        for resolution in pending.iter_mut() {
            visitor.visit_usize_slot(&mut resolution.promise_ptr);
        }
    }
}

/// Get a reference to the global runtime
pub fn runtime() -> &'static Runtime {
    &RUNTIME
}

/// Spawn an async task on the global runtime.
///
/// Issue #921: bump `EXT_BLOCKING_TASKS_INFLIGHT` for the lifetime of
/// the future so `js_stdlib_has_active_handles()` keeps the codegen-
/// emitted event loop alive while the task is running.
///
/// Without the bump, the race window is:
///
/// 1. `main()` is async, calls `await fetch(...)` (or any other
///    `spawn(...)`-backed binding) — `js_fetch_*` returns a fresh
///    Promise and `spawn(future)` schedules the network roundtrip
///    on a tokio worker.
/// 2. Codegen's async lowering returns from the current step,
///    yielding control back to the entry-module init.
/// 3. The entry-module init finishes (top-level `main()` was
///    fire-and-forget), so codegen drops into its event-loop
///    bootstrap.
/// 4. The event loop's `js_stdlib_has_active_handles()` check sees
///    `PENDING_RESOLUTIONS` empty, no WS / NET / HTTP / readline,
///    no `EXT_BLOCKING_TASKS_INFLIGHT` increment from `spawn(...)`,
///    so it returns 0.
/// 5. The loop exits cleanly (exit code 0). The tokio worker
///    eventually queues its resolution, but no one is listening
///    anymore.
///
/// User-visible symptom: `await fetch(...)` silently exits the
/// process with no JS error and no stderr from the network
/// callback. Production hosts (PM2, systemd) interpret the clean
/// exit as a crash and restart the binary.
///
/// Bumping INFLIGHT around the spawned future fixes this by making
/// the event-loop active-handle check pessimistically wait for the
/// future to finish (or queue its resolution and decrement INFLIGHT).
/// Same mechanism `perry_ffi_spawn_blocking` already uses for
/// external wrapper crates (#591); fetch / ioredis / zlib / etc.
/// just hadn't been wired through it yet.
pub fn spawn<F>(future: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    ensure_pump_registered();
    EXT_BLOCKING_TASKS_INFLIGHT.fetch_add(1, Ordering::AcqRel);
    RUNTIME.spawn(async move {
        future.await;
        EXT_BLOCKING_TASKS_INFLIGHT.fetch_sub(1, Ordering::AcqRel);
        // Notify in case the future resolved without going through
        // `queue_promise_resolution` — flip the active-handle gate
        // so the loop re-evaluates.
        perry_runtime::event_pump::js_notify_main_thread();
    });
}

/// Block on an async task (use sparingly, mainly for initialization)
pub fn block_on<F, T>(future: F) -> T
where
    F: Future<Output = T>,
{
    RUNTIME.block_on(future)
}

/// #5779 idle-kick liveness counters. `ISSUED` bumps on the main thread each
/// time a kick fires; `RAN` bumps inside the spawned task once a worker has
/// actually polled it. Exposed for the regression test below.
static IDLE_KICKS_ISSUED: AtomicU64 = AtomicU64::new(0);
static IDLE_KICKS_RAN: AtomicU64 = AtomicU64::new(0);

/// #5779 — idle-kick callback registered with perry-runtime's event pump.
///
/// Invoked on the main thread immediately before it blocks on the idle
/// condvar (see `event_pump::js_register_idle_kick`). When async work is in
/// flight, schedule a task on the shared runtime so a worker is roused to
/// drain anything that was enqueued from the main thread but whose
/// worker-unpark was lost while every worker was parked — the canonical case
/// being an in-process HTTP server's per-request response `oneshot::send`,
/// fired from `res.end()` inside the main-thread request pump. Without this,
/// that serve task strands forever, its response is never written, and the
/// matching in-process `fetch()` never settles (#5779).
///
/// The spawned task does **real work** (a release atomic store): an empty
/// `spawn(async {})` was measured to *not* reliably rouse a parked worker on
/// the multi-thread runtime (sequential in-process server+fetch deadlocked
/// 8/8 runs), whereas a task with a genuine side effect forces a real
/// scheduler round that wakes a worker and drains the stranded task (0
/// deadlocks across no-load + fully-loaded stress). The store also serves as
/// a memory barrier publishing the main thread's prior enqueue.
///
/// Gated on `EXT_BLOCKING_TASKS_INFLIGHT` so a fully idle process (no fetch,
/// no in-flight client request) lets the runtime sleep undisturbed and the
/// kick costs a single atomic load per idle tick. We deliberately do **not**
/// bump the inflight gate for the kick task itself — it must not keep the
/// event loop alive on its own.
extern "C" fn stdlib_idle_kick() {
    if EXT_BLOCKING_TASKS_INFLIGHT.load(Ordering::Acquire) == 0 {
        return;
    }
    IDLE_KICKS_ISSUED.fetch_add(1, Ordering::Relaxed);
    RUNTIME.spawn(async {
        IDLE_KICKS_RAN.fetch_add(1, Ordering::Release);
    });
}

/// Queue a promise resolution to be processed later
/// NOTE: Only use this for simple values (numbers, booleans, undefined, null)
/// that don't involve pointer allocations. For complex values like arrays,
/// objects, or strings, use queue_deferred_resolution instead.
pub fn queue_promise_resolution(promise_ptr: usize, is_success: bool, result_bits: u64) {
    ensure_gc_scanner_registered();
    // The await busy-pump only drains PENDING_RESOLUTIONS via the registered
    // STDLIB_PUMP_FN. Callers that queue a resolution *without* a preceding
    // `spawn` (e.g. `js_fetch_with_options`'s early "Invalid URL" reject when
    // `fetch()` is handed a non-string first arg such as a `Request` object)
    // would otherwise leave the pump unregistered: `js_stdlib_has_active_handles`
    // reports the pending entry so the awaiter keeps waiting, but nothing ever
    // drains it → the awaited Promise stays pending forever (deadlock). Register
    // the pump here so every queued resolution is guaranteed to be processed.
    ensure_pump_registered();
    {
        let mut pending = PENDING_RESOLUTIONS.lock().unwrap();
        pending.push(PendingResolution {
            promise_ptr,
            is_success,
            result_bits,
        });
    }
    // Issue #84: wake the main-thread event loop / await busy-wait the
    // instant we enqueue, instead of waiting up to ~10 ms for the next
    // poll. Drop the queue lock first so the consumer doesn't briefly
    // block re-acquiring it. Covers all queue_promise_resolution callers
    // — fetch, ioredis, bcrypt, zlib, spawn_for_promise, etc.
    perry_runtime::event_pump::js_notify_main_thread();
}

/// Queue a deferred promise resolution with a conversion callback
/// The converter function will run on the main thread to safely create JSValues
/// using the main thread's arena allocator.
pub fn queue_deferred_resolution<F>(promise_ptr: usize, is_success: bool, converter: F)
where
    F: FnOnce() -> u64 + Send + 'static,
{
    ensure_gc_scanner_registered();
    // Same pump-registration guarantee as `queue_promise_resolution` (above):
    // a deferred resolution queued without a preceding `spawn` must still be
    // drained by the await busy-pump.
    ensure_pump_registered();
    {
        let mut pending = PENDING_DEFERRED.lock().unwrap();
        pending.push(DeferredResolution {
            promise_ptr,
            is_success,
            converter: Box::new(converter),
        });
    }
    // Issue #84: same as queue_promise_resolution — wake the main thread
    // immediately so the awaiter doesn't pay the old hard-sleep latency.
    perry_runtime::event_pump::js_notify_main_thread();
}

/// Register js_stdlib_process_pending with perry-runtime's pump so that
/// perry-ui-macos can call it without a hard link dependency on perry-stdlib.
///
/// Public because non-await modules that nonetheless need the event loop
/// to keep ticking — readline (#347), and any future TUI-shaped module
/// that uses thread-local pending queues without ever calling
/// `spawn_for_promise` — must register the pump explicitly the first time
/// they're touched. Otherwise the runtime exits immediately when `main`
/// returns and the close/line callbacks never fire.
pub fn ensure_pump_registered() {
    use std::sync::Once;
    static REGISTER: Once = Once::new();
    REGISTER.call_once(|| {
        extern "C" {
            fn js_register_stdlib_pump(f: extern "C" fn() -> i32);
            fn js_register_stdlib_has_active(f: extern "C" fn() -> i32);
            fn js_stdlib_init_dispatch();
        }
        ensure_gc_scanner_registered();
        // #5779 — recover from lost tokio worker-unparks while the main
        // thread is parked (in-process HTTP server + fetch deadlock).
        perry_runtime::event_pump::js_register_idle_kick(Some(stdlib_idle_kick));
        unsafe {
            js_register_stdlib_pump(js_stdlib_process_pending);
            js_register_stdlib_has_active(js_stdlib_has_active_handles);
            // Wire up the runtime-level HANDLE_METHOD_DISPATCH so that
            // generic `jsObject.method(args)` calls on stdlib handle types
            // (net.Socket, Fastify, ioredis) fall back to the right FFI
            // even when codegen lost static type info — e.g. accessing the
            // socket through a struct field (`state.sock.write(...)`).
            // Until this was hooked in, HANDLE_METHOD_DISPATCH stayed None
            // and those calls silently returned undefined.
            js_stdlib_init_dispatch();
        }
    });
}

/// Process all pending promise resolutions
///
/// This should be called from the main event loop to process async completions.
/// Returns the number of resolutions processed.
#[no_mangle]
pub extern "C" fn js_stdlib_process_pending() -> i32 {
    let mut count = 0i32;

    // Process simple resolutions first
    let simple_resolutions: Vec<PendingResolution> = {
        let mut pending = PENDING_RESOLUTIONS.lock().unwrap();
        let n = pending.len();
        count += n as i32;
        pending.drain(..).collect()
    };

    for resolution in simple_resolutions {
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        let promise_ptr_usize = resolution.promise_ptr;
        let promise_handle =
            scope.root_raw_mut_ptr(promise_ptr_usize as *mut perry_runtime::Promise);
        let result_handle = scope.root_nanbox_u64(resolution.result_bits);
        // Issue #859: unpin BEFORE resolve so the just-settled promise
        // can be reclaimed by the next GC. Resolve doesn't trigger GC
        // mid-call, so ordering here is purely about leaving a clean
        // GC state after the loop.
        unsafe {
            unpin_promise_after_native_resolution(
                promise_handle.get_raw_mut_ptr::<perry_runtime::Promise>() as usize,
            )
        };
        if resolution.is_success {
            perry_runtime::js_promise_resolve(
                promise_handle.get_raw_mut_ptr(),
                f64::from_bits(result_handle.get_nanbox_u64()),
            );
        } else {
            perry_runtime::js_promise_reject(
                promise_handle.get_raw_mut_ptr(),
                f64::from_bits(result_handle.get_nanbox_u64()),
            );
        }
    }

    // Process deferred resolutions - these run converter functions on the main thread
    let deferred_resolutions: Vec<DeferredResolution> = {
        let mut pending = PENDING_DEFERRED.lock().unwrap();
        let n = pending.len();
        count += n as i32;
        pending.drain(..).collect()
    };

    for resolution in deferred_resolutions {
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        let promise_ptr_usize = resolution.promise_ptr;
        let promise_handle =
            scope.root_raw_mut_ptr(promise_ptr_usize as *mut perry_runtime::Promise);
        // Run the converter on the main thread to create JSValues safely
        let result_bits = (resolution.converter)();
        let result_handle = scope.root_nanbox_u64(result_bits);

        // Issue #859: unpin BEFORE resolve. The converter ran first
        // and may itself have allocated (creating the result string,
        // etc.), but the promise stayed pinned across that work — so
        // even if the converter triggered GC, the promise survived.
        unsafe {
            unpin_promise_after_native_resolution(
                promise_handle.get_raw_mut_ptr::<perry_runtime::Promise>() as usize,
            )
        };
        if resolution.is_success {
            perry_runtime::js_promise_resolve(
                promise_handle.get_raw_mut_ptr(),
                f64::from_bits(result_handle.get_nanbox_u64()),
            );
        } else {
            perry_runtime::js_promise_reject(
                promise_handle.get_raw_mut_ptr(),
                f64::from_bits(result_handle.get_nanbox_u64()),
            );
        }
    }

    // Process pending WebSocket events (server/client listener callbacks).
    // Gate fires for either `bundled-ws` (perry-stdlib's own impl) or
    // `external-ws-pump` (well-known flip → perry-ext-ws provides the
    // symbol). Mirrors net's gate above. Closes #606 follow-up.
    #[cfg(any(feature = "websocket", feature = "external-ws-pump"))]
    {
        extern "C" {
            fn js_ws_process_pending() -> i32;
        }
        let ws_count = unsafe { js_ws_process_pending() };
        count += ws_count;
    }

    // Process pending HTTP events (http/https client callbacks)
    #[cfg(feature = "http-client")]
    {
        let http_count = unsafe { crate::http::js_http_process_pending() };
        count += http_count;
    }

    // Process pending raw TCP socket events (net.Socket).
    // v0.5.579 — gate now fires for `bundled-net` (perry-stdlib's
    // own implementation) AND `external-net-pump` (which the
    // well-known flip in `optimized_libs.rs` enables when routing
    // `import 'net'` to perry-ext-net). The fallback no-op stub
    // pattern (e.g. cron's) doesn't work for net because the
    // perry-ext-net wrapper's symbol can't be reliably preferred
    // over perry-stdlib's stub on Mach-O.
    // v0.5.579: gate on `bundled-net` (perry-stdlib has its own net
    // module compiled in) OR `external-net-pump` (well-known flip
    // activated → perry-ext-net is linked, provides the symbol).
    // Without this gate, the cfg `feature = "net"` from v0.5.572's
    // umbrella renaming was always FALSE under the well-known flip,
    // and tokio events queued by perry-ext-net never got drained.
    #[cfg(all(
        any(feature = "bundled-net", feature = "external-net-pump"),
        not(target_os = "ios"),
        not(target_os = "android")
    ))]
    {
        extern "C" {
            fn js_net_process_pending() -> i32;
        }
        let net_count = unsafe { js_net_process_pending() };
        count += net_count;
    }

    #[cfg(all(feature = "tls", not(target_os = "ios"), not(target_os = "android")))]
    {
        count += unsafe { crate::tls::js_tls_process_pending() };
    }

    // Process pending HTTP server requests + WS upgrades (perry-ext-http-server).
    // Closes #604 — pre-fix `js_node_http_server_listen` blocked the
    // main TS thread inside an inner event_loop, so axios.get/etc.
    // after a `server.listen(port, () => resolve())` callback never
    // ran. Now `listen()` returns immediately and pending requests
    // are drained from the unified pump on every tick. Mirrors the
    // `external-net-pump` / `external-ws-pump` patterns above.
    #[cfg(feature = "external-http-server-pump")]
    {
        extern "C" {
            fn js_node_http_server_process_pending() -> i32;
        }
        let n = unsafe { js_node_http_server_process_pending() };
        count += n;
    }

    // Issue #769 — when the well-known flip routes `node:http` /
    // `node:https` client (`http.request` / `http.get`) to
    // perry-ext-http, drain its response/error queue on every tick.
    // Mirrors the server-side `external-http-server-pump` arm above.
    #[cfg(feature = "external-http-client-pump")]
    {
        extern "C" {
            fn js_http_process_pending() -> i32;
        }
        let n = unsafe { js_http_process_pending() };
        count += n;
    }

    // Process pending worker_threads messages (stdin reader)
    count += crate::worker_threads::js_worker_threads_process_pending();

    // Drain same-process MessageChannel port inboxes (#3157) — dispatch queued
    // `port.postMessage(v)` payloads to `port.on('message', cb)` listeners and
    // fire `close` events for closed ports.
    count += crate::worker_threads::js_worker_threads_channels_process_pending();

    // Process pending readline lines (#347 Phase 1) — drains the stdin
    // reader's queue and dispatches to question/line/close callbacks.
    count += crate::readline::js_readline_process_pending();

    // Process pending crypto Hash/Hmac stream digest events (#2479).
    #[cfg(feature = "crypto")]
    {
        count += unsafe { crate::crypto::js_crypto_stream_process_pending() };
    }

    // Process pending zlib stream events (#1843) — `createGzip()` etc.
    // buffer input across `.write()` and queue 'data'/'end' on `.end()`;
    // drained + dispatched to listeners (and forwarded to `.pipe()` dests)
    // here on the main thread. Bundled path (perry-stdlib's own zlib mod):
    #[cfg(feature = "compression")]
    {
        count += unsafe { crate::zlib::js_zlib_process_pending() };
    }
    // External path: the well-known flip routed `node:zlib` to perry-ext-zlib
    // and stripped `compression`. Drain perry-ext-zlib's queue via its extern.
    #[cfg(feature = "external-zlib-pump")]
    {
        extern "C" {
            fn js_ext_zlib_process_pending() -> i32;
        }
        count += unsafe { js_ext_zlib_process_pending() };
    }

    // Process pending fastify requests. `listen()` returns immediately and the
    // per-server mpsc is drained here each tick (#604), so an `await
    // app.listen(...)` resumes and subsequent user code (in-process `fetch`,
    // `app.close()`, async route handlers) runs. fastify is served exclusively by
    // the external perry-ext-fastify crate (the in-stdlib adapter was removed);
    // the well-known flip enables `external-fastify-pump` and the symbol is
    // provided by that crate. Mirrors `external-net-pump` / `external-ws-pump`.
    #[cfg(feature = "external-fastify-pump")]
    {
        extern "C" {
            fn js_fastify_process_pending() -> i32;
        }
        count += unsafe { js_fastify_process_pending() };
    }

    count
}

/// Returns 1 if the stdlib has active event sources that need the event
/// loop to keep running (active WS servers, pending events, etc.).
/// Registered with perry-runtime via js_register_stdlib_has_active()
/// so the runtime's trampoline calls this when perry-stdlib is linked.
pub extern "C" fn js_stdlib_has_active_handles() -> i32 {
    // External wrapper crates (perry-ext-argon2, -bcrypt, …) dispatch
    // their CPU-bound work through `perry_ffi_spawn_blocking`. Until
    // the closure has run + queued its result, the awaiter's Promise
    // is pending but invisible to the rest of the gate (no entry in
    // PENDING_RESOLUTIONS yet). Issue #591.
    if EXT_BLOCKING_TASKS_INFLIGHT.load(Ordering::Acquire) != 0 {
        return 1;
    }
    // Check for pending stdlib resolutions
    {
        let pending = PENDING_RESOLUTIONS.lock().unwrap();
        if !pending.is_empty() {
            return 1;
        }
    }
    {
        let pending = PENDING_DEFERRED.lock().unwrap();
        if !pending.is_empty() {
            return 1;
        }
    }
    // Check for active WebSocket servers/connections
    #[cfg(feature = "websocket")]
    {
        // #854: removed an unused `js_ws_process_pending` extern decl here —
        // this block only checks for active handles; the drain path with its
        // own extra decl lives earlier in the pump.
        // If there are pending WS events, keep running
        // (we don't drain here — just check)
        let has_ws = crate::ws::js_ws_has_active_handles();
        if has_ws != 0 {
            return 1;
        }
    }
    // External (perry-ext-ws) path — when the well-known flip strips
    // `bundled-ws` and routes `import 'ws'` to perry-ext-ws, the
    // wrapper's `js_ws_has_pending` reports active servers / open
    // connections / queued events. Without this gate, a TS program
    // running an in-process WebSocketServer would have its event loop
    // exit before the listener task can dispatch any event. Closes
    // #606 follow-up. Mirrors the `external-net-pump` arm above.
    #[cfg(all(feature = "external-ws-pump", not(feature = "websocket")))]
    {
        extern "C" {
            fn js_ws_has_pending() -> i32;
        }
        if unsafe { js_ws_has_pending() } != 0 {
            return 1;
        }
    }
    // Check for active raw TCP sockets (net.Socket / tls.connect / upgrade).
    // Without this, an `await net.connect(...)` returns a Promise that the
    // runtime can't see is pending, so the event loop exits before the
    // socket's 'connect' event ever fires through the pump.
    //
    // Two paths: `bundled-net` (perry-stdlib's own net implementation
    // is compiled in) calls `crate::net::js_net_has_active_handles`
    // directly; `external-net-pump` (the well-known flip routes
    // `import 'net'` to perry-ext-net) calls perry-ext-net's
    // `js_ext_net_has_active_handles` extern. Pre-fix only the
    // bundled-net gate fired, so programs using TS-source drivers
    // like `@perryts/mysql` that route through perry-ext-net saw
    // `await new Promise(r => sock.on('connect', r))` exit early
    // because perry-stdlib's empty NET_SOCKETS map reported no
    // active handles. Issue #536.
    #[cfg(all(
        feature = "bundled-net",
        not(target_os = "ios"),
        not(target_os = "android")
    ))]
    {
        let has_net = crate::net::js_net_has_active_handles();
        if has_net != 0 {
            return 1;
        }
    }
    #[cfg(all(feature = "tls", not(target_os = "ios"), not(target_os = "android")))]
    {
        if crate::tls::js_tls_has_active_handles() != 0 {
            return 1;
        }
    }
    #[cfg(all(
        feature = "external-net-pump",
        not(feature = "bundled-net"),
        not(target_os = "ios"),
        not(target_os = "android")
    ))]
    {
        extern "C" {
            fn js_ext_net_has_active_handles() -> i32;
        }
        if unsafe { js_ext_net_has_active_handles() } != 0 {
            return 1;
        }
    }
    // Active HTTP/HTTPS/HTTP2 servers — keep the event loop alive
    // for the lifetime of any listening server (until the user calls
    // `server.close()`). Without this gate, the codegen-emitted main
    // loop sees no active sources and exits before the first request
    // ever arrives. Closes #604 — paired with the
    // `js_node_http_server_process_pending` arm in
    // `js_stdlib_process_pending` above.
    #[cfg(feature = "external-http-server-pump")]
    {
        extern "C" {
            fn js_node_http_server_has_active() -> i32;
        }
        if unsafe { js_node_http_server_has_active() } != 0 {
            return 1;
        }
    }
    // Issue #769 — keep the event loop alive while an in-flight
    // `http.request` / `http.get` (perry-ext-http) hasn't received its
    // response or error event yet.
    #[cfg(feature = "external-http-client-pump")]
    {
        extern "C" {
            fn js_http_has_pending() -> i32;
        }
        if unsafe { js_http_has_pending() } != 0 {
            return 1;
        }
    }
    // readline (#347 Phase 1) — keep the loop alive while a stdin
    // reader is started and EOF hasn't been observed, so `rl.on('line')`
    // / `rl.question()` programs don't exit before the user types.
    if crate::readline::js_readline_has_active() != 0 {
        return 1;
    }
    // Same-process MessageChannel ports (#3157) — keep the loop alive while a
    // started port still has queued messages or a pending `close` event.
    if crate::worker_threads::js_worker_threads_channels_has_pending() != 0 {
        return 1;
    }
    if crate::worker_threads::js_worker_threads_has_pending() != 0 {
        return 1;
    }
    #[cfg(feature = "crypto")]
    {
        if crate::crypto::js_crypto_stream_has_active_handles() != 0 {
            return 1;
        }
    }
    // External fastify (perry-ext-fastify) — keep the loop alive while any
    // FastifyServerHandle is "listening". Paired with `js_fastify_process_pending`
    // in `js_stdlib_process_pending` above (closes the compat-sweep timeout for
    // `await app.listen(...)` + in-process `fetch`). The in-stdlib adapter was
    // removed; the well-known flip enables `external-fastify-pump` and the symbol
    // is provided by perry-ext-fastify at link time. Mirrors the
    // `external-{net,ws,http-server}-pump` arms above.
    #[cfg(feature = "external-fastify-pump")]
    {
        extern "C" {
            fn js_fastify_has_active() -> i32;
        }
        if unsafe { js_fastify_has_active() } != 0 {
            return 1;
        }
    }
    // zlib streams (#1843) — keep the loop alive while `.end()`-queued
    // 'data'/'end' events are still waiting to be drained, so a purely-
    // synchronous `createGzip().write(x).end()` program doesn't exit before
    // its listeners fire. Bundled path:
    #[cfg(feature = "compression")]
    {
        if crate::zlib::js_zlib_has_active_handles() != 0 {
            return 1;
        }
    }
    // External (perry-ext-zlib) path:
    #[cfg(feature = "external-zlib-pump")]
    {
        extern "C" {
            fn js_ext_zlib_has_active_handles() -> i32;
        }
        if unsafe { js_ext_zlib_has_active_handles() } != 0 {
            return 1;
        }
    }
    0
}

/// Spawn an async operation that will resolve a Promise when complete
///
/// WARNING: This function assumes the returned u64 bits represent a simple value
/// (number, boolean, undefined, null) that doesn't contain heap pointers.
/// For complex values (arrays, objects, strings), use spawn_for_promise_deferred instead.
///
/// # Safety
/// The promise_ptr must be a valid pointer to a Promise object
pub unsafe fn spawn_for_promise<F>(promise_ptr: *mut u8, future: F)
where
    F: Future<Output = Result<u64, String>> + Send + 'static,
{
    ensure_pump_registered();
    ensure_gc_scanner_registered();
    // Convert to usize for Send.
    let ptr = promise_ptr as usize;
    // Issue #859: pin the promise BEFORE crossing the tokio boundary.
    // See `pin_promise_for_native_resolution` for the full rationale.
    pin_promise_for_native_resolution(ptr);

    // Issue #921: same race-window mitigation as the plain
    // `spawn()` above — bump INFLIGHT for the lifetime of the
    // future so the event loop's `js_stdlib_has_active_handles`
    // check stays truthy until the resolution is queued.
    EXT_BLOCKING_TASKS_INFLIGHT.fetch_add(1, Ordering::AcqRel);
    RUNTIME.spawn(async move {
        match future.await {
            Ok(result_bits) => {
                queue_promise_resolution(ptr, true, result_bits);
            }
            Err(error_msg) => {
                // Store the error message and create the string on the main thread
                queue_deferred_resolution(ptr, false, move || {
                    let str_ptr = perry_runtime::js_string_from_bytes(
                        error_msg.as_ptr(),
                        error_msg.len() as u32,
                    );
                    // Use string_ptr for proper type identification (STRING_TAG, not POINTER_TAG)
                    perry_runtime::JSValue::string_ptr(str_ptr).bits()
                });
            }
        }
        EXT_BLOCKING_TASKS_INFLIGHT.fetch_sub(1, Ordering::AcqRel);
        perry_runtime::event_pump::js_notify_main_thread();
    });
}

/// Spawn an async operation with deferred JSValue creation
///
/// This is the safe way to create complex JSValues (arrays, objects, strings)
/// from async operations. The async block returns raw Rust data, and the
/// converter function creates the JSValue on the main thread.
///
/// # Type Parameters
/// - `T`: The raw data type produced by the async operation (must be Send + 'static)
/// - `F`: The async future type
/// - `C`: The converter function type
///
/// # Arguments
/// - `promise_ptr`: Pointer to the Promise object
/// - `future`: Async future that produces Result<T, String>
/// - `converter`: Function that converts T to JSValue bits (runs on main thread)
///
/// # Safety
/// The promise_ptr must be a valid pointer to a Promise object
pub unsafe fn spawn_for_promise_deferred<T, F, C>(promise_ptr: *mut u8, future: F, converter: C)
where
    T: Send + 'static,
    F: Future<Output = Result<T, String>> + Send + 'static,
    C: FnOnce(T) -> u64 + Send + 'static,
{
    ensure_pump_registered();
    ensure_gc_scanner_registered();
    let ptr = promise_ptr as usize;
    // Issue #859: pin the promise BEFORE crossing the tokio boundary.
    pin_promise_for_native_resolution(ptr);

    // Issue #921: same race-window mitigation as `spawn_for_promise`
    // above — bump INFLIGHT for the lifetime of the future.
    EXT_BLOCKING_TASKS_INFLIGHT.fetch_add(1, Ordering::AcqRel);
    RUNTIME.spawn(async move {
        match future.await {
            Ok(data) => {
                // Queue deferred resolution with the converter
                queue_deferred_resolution(ptr, true, move || converter(data));
            }
            Err(error_msg) => {
                // Create error string on main thread
                queue_deferred_resolution(ptr, false, move || {
                    let str_ptr = perry_runtime::js_string_from_bytes(
                        error_msg.as_ptr(),
                        error_msg.len() as u32,
                    );
                    // Use string_ptr for proper type identification (STRING_TAG, not POINTER_TAG)
                    perry_runtime::JSValue::string_ptr(str_ptr).bits()
                });
            }
        }
        EXT_BLOCKING_TASKS_INFLIGHT.fetch_sub(1, Ordering::AcqRel);
        perry_runtime::event_pump::js_notify_main_thread();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clear_pending() {
        PENDING_RESOLUTIONS.lock().unwrap().clear();
        PENDING_DEFERRED.lock().unwrap().clear();
    }

    /// #5779: when async work is in flight the idle kick must schedule a task
    /// that a worker actually runs — an empty `spawn(async {})` was observed
    /// not to reliably rouse a parked worker, leaving the in-process
    /// server+fetch deadlock unrecovered. Bump the in-flight gate (restoring
    /// it after, so a parallel test's count is preserved), fire the kick, and
    /// drive the runtime until the kick task records that it ran.
    #[test]
    fn idle_kick_schedules_a_task_that_runs() {
        let issued_before = IDLE_KICKS_ISSUED.load(Ordering::Relaxed);
        let ran_before = IDLE_KICKS_RAN.load(Ordering::Acquire);

        EXT_BLOCKING_TASKS_INFLIGHT.fetch_add(1, Ordering::AcqRel);
        stdlib_idle_kick();
        EXT_BLOCKING_TASKS_INFLIGHT.fetch_sub(1, Ordering::AcqRel);

        assert!(
            IDLE_KICKS_ISSUED.load(Ordering::Relaxed) > issued_before,
            "kick must fire when async work is in flight"
        );
        // Give the worker pool real wall-clock time to schedule and run the
        // kick task (a tight `yield_now` loop spins the calling thread faster
        // than a worker is scheduled, so use timed sleeps — up to ~2s).
        RUNTIME.block_on(async {
            for _ in 0..200 {
                if IDLE_KICKS_RAN.load(Ordering::Acquire) > ran_before {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        });
        assert!(
            IDLE_KICKS_RAN.load(Ordering::Acquire) > ran_before,
            "the kick's spawned task never executed on a worker"
        );
    }

    #[test]
    fn async_bridge_pending_resolution_scanner_emits_promise_and_result_roots() {
        clear_pending();
        let promise_ptr = 0x1234_5000usize;
        let deferred_promise_ptr = 0x1234_6000usize;
        let result_bits = 0x7FFD_0000_1234_7000u64;
        PENDING_RESOLUTIONS.lock().unwrap().push(PendingResolution {
            promise_ptr,
            is_success: true,
            result_bits,
        });
        PENDING_DEFERRED.lock().unwrap().push(DeferredResolution {
            promise_ptr: deferred_promise_ptr,
            is_success: true,
            converter: Box::new(|| 0),
        });

        let mut emitted = Vec::new();
        {
            let mut mark = |value: f64| emitted.push(value.to_bits());
            let mut visitor = perry_runtime::gc::RuntimeRootVisitor::for_copy(&mut mark);
            scan_pending_native_async_resolution_roots_mut(&mut visitor);
        }

        assert!(emitted.contains(&(0x7FFD_0000_0000_0000 | promise_ptr as u64)));
        assert!(emitted.contains(&result_bits));
        assert!(emitted.contains(&(0x7FFD_0000_0000_0000 | deferred_promise_ptr as u64)));
        clear_pending();
    }
}
