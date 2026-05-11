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
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use once_cell::sync::Lazy;
use tokio::runtime::Runtime;

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

/// Get a reference to the global runtime
pub fn runtime() -> &'static Runtime {
    &RUNTIME
}

/// Spawn an async task on the global runtime
pub fn spawn<F>(future: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    ensure_pump_registered();
    RUNTIME.spawn(future);
}

/// Block on an async task (use sparingly, mainly for initialization)
pub fn block_on<F, T>(future: F) -> T
where
    F: Future<Output = T>,
{
    RUNTIME.block_on(future)
}

/// Queue a promise resolution to be processed later
/// NOTE: Only use this for simple values (numbers, booleans, undefined, null)
/// that don't involve pointer allocations. For complex values like arrays,
/// objects, or strings, use queue_deferred_resolution instead.
pub fn queue_promise_resolution(promise_ptr: usize, is_success: bool, result_bits: u64) {
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
    {
        let mut pending = PENDING_RESOLUTIONS.lock().unwrap();
        let n = pending.len();
        count += n as i32;

        for resolution in pending.drain(..) {
            let promise_ptr = resolution.promise_ptr as *mut perry_runtime::Promise;
            if resolution.is_success {
                perry_runtime::js_promise_resolve(
                    promise_ptr,
                    f64::from_bits(resolution.result_bits),
                );
            } else {
                perry_runtime::js_promise_reject(
                    promise_ptr,
                    f64::from_bits(resolution.result_bits),
                );
            }
        }
    }

    // Process deferred resolutions - these run converter functions on the main thread
    {
        let mut pending = PENDING_DEFERRED.lock().unwrap();
        let n = pending.len();
        count += n as i32;

        for resolution in pending.drain(..) {
            let promise_ptr = resolution.promise_ptr as *mut perry_runtime::Promise;
            // Run the converter on the main thread to create JSValues safely
            let result_bits = (resolution.converter)();

            if resolution.is_success {
                perry_runtime::js_promise_resolve(promise_ptr, f64::from_bits(result_bits));
            } else {
                perry_runtime::js_promise_reject(promise_ptr, f64::from_bits(result_bits));
            }
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

    // Process pending worker_threads messages (stdin reader)
    count += crate::worker_threads::js_worker_threads_process_pending();

    // Process pending readline lines (#347 Phase 1) — drains the stdin
    // reader's queue and dispatches to question/line/close callbacks.
    count += crate::readline::js_readline_process_pending();

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
        extern "C" {
            fn js_ws_process_pending() -> i32;
        }
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
    // readline (#347 Phase 1) — keep the loop alive while a stdin
    // reader is started and EOF hasn't been observed, so `rl.on('line')`
    // / `rl.question()` programs don't exit before the user types.
    if crate::readline::js_readline_has_active() != 0 {
        return 1;
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
    let ptr = promise_ptr as usize; // Convert to usize for Send

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
    let ptr = promise_ptr as usize;

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
    });
}
