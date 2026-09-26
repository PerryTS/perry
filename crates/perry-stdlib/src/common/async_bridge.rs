//! Async bridge: settles Perry Promises from native work, on the thread that
//! owns the JS heap.
//!
//! Native work — a turnloop pool job or a plain OS thread — never builds a
//! JSValue. It queues either finished bits (`queue_promise_resolution`) or a
//! converter (`queue_deferred_resolution`), and `js_stdlib_process_pending`
//! settles the promise on the main thread.
//!
//! IMPORTANT: perry-runtime uses thread-local arenas for memory allocation.
//! This means JSValue objects created on worker threads will be allocated
//! from a different arena than the main thread, causing memory corruption.
//!
//! To avoid this, async operations should:
//! 1. NOT create JSValue objects (arrays, strings, objects) off the main thread
//! 2. Store raw Rust data and use deferred conversion callbacks
//! 3. The conversion callbacks run on the main thread during js_stdlib_process_pending
//!
//! # No tokio
//!
//! This is the only async bridge perry-stdlib has. Turnloop P8 lane L split
//! the tokio current-thread runtime out of this module into a sibling
//! `tokio_bridge` behind an `async-runtime` feature; the final tokio lane
//! deleted both, together with the `perry_ffi_spawn_async` /
//! `_with_reactor` C ABI that was their last user. Every program links no
//! tokio: CPU work goes to turnloop's pool (`pool_for_promise_deferred`),
//! I/O to the owning agent's turnloop loop.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use std::sync::LazyLock as Lazy;

/// Issue #859: pin a Promise so the GC can't sweep it while a pool
/// job or native thread is computing its eventual resolution.
///
/// Without pinning, the await chain has no path back to the Promise:
/// `P.next = N` is a forward edge, and after the user code yields, all
/// JS-side roots reach only `N`. The native job holds `promise_ptr`
/// as `usize`, invisible to the GC. So `js_promise_new()` in a native
/// binding + a pool submission opens a window where `P` is
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
    // `js_promise_new()` allocates in the arena (Eden) unless promise hooks are
    // active, so this pin DOES arm the copying minor's young-pin latch (#7645).
    perry_runtime::gc::pin_object(header);
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
    perry_runtime::gc::unpin_object(header);
}

/// Release the native async completion token a promise minted through
/// `perry_ffi_promise_new` still owns (#9356).
///
/// `perry_ffi_promise_new` allocates through `js_native_async_completion_new`,
/// which registers a token keyed by the promise address so the token API can
/// settle it later. perry-ffi's `JsPromise::resolve_with` / `reject_with` and
/// the legacy `resolve_*` shims do not go through the token API — they queue
/// straight into the stdlib pump and settle the promise here — so nothing ever
/// removed the token. Every native call (each mysql2 query, every bcrypt hash,
/// …) leaked one registry entry: the settled promise stayed rooted and was
/// rewritten on every minor collection, and `js_native_async_has_active`
/// reported work forever. Dropping the token here mirrors the cleanup
/// `js_native_async_process_pending` performs for token-API settlements; it
/// is a no-op for promises that never had a token.
fn release_native_async_token(promise: *mut perry_runtime::Promise) {
    perry_runtime::promise::js_native_async_drop_promise_token(promise);
}

/// Allocate a fresh Promise for cross-thread resolution. Convenience wrapper
/// for direct callers of [`queue_promise_resolution`] /
/// [`queue_deferred_resolution`] — modules that bypass
/// `spawn_for_promise[_deferred]` because their own future setup is custom.
///
/// #9552: the pin is taken by `js_promise_new_cross_thread` itself and
/// released when the promise settles, so this is now exactly that
/// constructor. Callers that reach for the bare constructor get the same
/// guarantee; this name survives for the modules that spell the intent.
///
/// # Safety
/// Same as `js_promise_new()`.
#[inline]
pub unsafe fn js_promise_new_for_native_resolution() -> *mut perry_runtime::Promise {
    ensure_gc_scanner_registered();
    perry_runtime::js_promise_new_cross_thread()
}

/// Count of in-flight `perry_ffi_spawn_blocking` tasks
/// dispatched by external native bindings (perry-ext-argon2 /
/// -bcrypt / etc. via perry-ffi). Each spawn `fetch_add(1)`s before
/// the closure runs; the closure-trampoline `fetch_sub(1)`s after it
/// returns. `js_stdlib_has_active_handles` returns 1 while this
/// counter is nonzero so the runtime's event loop keeps draining
/// PENDING_RESOLUTIONS / PENDING_DEFERRED until the closure has
/// queued its result.
///
/// Issue #591: without this counter, `await argon2.hash(pw)` returns
/// a Promise whose resolution is queued from a worker thread AFTER
/// `main()` returns. The runtime saw zero active handles (no WS,
/// net, readline) and exited before the resolution drained, so the
/// `.then` / `await` never fired and the program ran past the await
/// returning undefined.
pub static EXT_BLOCKING_TASKS_INFLIGHT: AtomicUsize = AtomicUsize::new(0);

/// Owns exactly one `EXT_BLOCKING_TASKS_INFLIGHT` increment, released on drop.
///
/// Create it BEFORE spawning and move it into the task: the decrement then runs
/// on completion, on error, when the task panics (the job is dropped while
/// unwinding) and when the task is dropped before it runs (pool
/// shutdown). The hand-written `fetch_add` / `fetch_sub` pairs it replaces
/// leaked an increment on the last two paths, which pinned the event loop alive
/// forever. Drop also notifies the main thread so the loop re-evaluates its
/// keep-alive predicate.
pub(crate) struct InflightGuard(());

impl InflightGuard {
    pub(crate) fn new() -> Self {
        EXT_BLOCKING_TASKS_INFLIGHT.fetch_add(1, Ordering::AcqRel);
        Self(())
    }
}

impl Drop for InflightGuard {
    fn drop(&mut self) {
        let previous = EXT_BLOCKING_TASKS_INFLIGHT.fetch_sub(1, Ordering::AcqRel);
        debug_assert!(previous > 0, "EXT_BLOCKING_TASKS_INFLIGHT underflow");
        perry_runtime::event_pump::js_notify_main_thread();
    }
}

pub use super::thread_config::blocking_thread_stack_size;

/// Pending promise resolutions
/// Format: (promise_ptr, is_success, result_value)
static PENDING_RESOLUTIONS: Lazy<Mutex<Vec<PendingResolution>>> =
    Lazy::new(|| Mutex::new(Vec::new()));

/// Pending deferred resolutions - these store raw data and a conversion function
/// that runs on the main thread to create JSValues safely
static PENDING_DEFERRED: Lazy<Mutex<Vec<DeferredResolution>>> =
    Lazy::new(|| Mutex::new(Vec::new()));

/// turnloop P0: the two queues' lengths, republished under their locks after
/// every push and drain, so `js_stdlib_has_active_handles` — asked on every
/// event-loop turn — reads two atomics instead of taking both queue locks.
static PENDING_RESOLUTIONS_LEN: AtomicUsize = AtomicUsize::new(0);
static PENDING_DEFERRED_LEN: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    static GC_SCANNER_REGISTERED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub(crate) fn ensure_gc_scanner_registered() {
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
        PENDING_RESOLUTIONS_LEN.store(pending.len(), Ordering::Release);
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
        PENDING_DEFERRED_LEN.store(pending.len(), Ordering::Release);
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
            fn js_register_stdlib_next_wake(f: extern "C" fn() -> f64);
            fn js_stdlib_init_dispatch();
        }
        ensure_gc_scanner_registered();
        // No wait-driver is installed: the primary agent parks in its own
        // turnloop loop, which `js_notify_main_thread` wakes.
        unsafe {
            js_register_stdlib_pump(js_stdlib_process_pending);
            js_register_stdlib_has_active(js_stdlib_has_active_handles);
            js_register_stdlib_next_wake(crate::readline::js_readline_next_wake_ms);
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
        PENDING_RESOLUTIONS_LEN.store(0, Ordering::Release);
        pending.drain(..).collect()
    };
    for resolution in simple_resolutions {
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        let promise_ptr_usize = resolution.promise_ptr;
        // #9552: the address spent its in-flight window as a bare usize in a
        // worker future; verify it still names a promise before touching it.
        let promise_handle = scope.root_raw_mut_ptr(
            perry_runtime::promise::native_promise_from_raw(promise_ptr_usize, "stdlib pump"),
        );
        let result_handle = scope.root_nanbox_u64(resolution.result_bits);
        // Issue #859: unpin BEFORE resolve so the just-settled promise
        // can be reclaimed by the next GC. Resolve doesn't trigger GC
        // mid-call, so ordering here is purely about leaving a clean
        // GC state after the loop.
        let promise_ptr = promise_handle.get_raw_mut_ptr::<perry_runtime::Promise>();
        unsafe { unpin_promise_after_native_resolution(promise_ptr as usize) };
        release_native_async_token(promise_ptr);
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
        PENDING_DEFERRED_LEN.store(0, Ordering::Release);
        pending.drain(..).collect()
    };

    for resolution in deferred_resolutions {
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        let promise_ptr_usize = resolution.promise_ptr;
        // #9552: the address spent its in-flight window as a bare usize in a
        // worker future; verify it still names a promise before touching it.
        let promise_handle = scope.root_raw_mut_ptr(
            perry_runtime::promise::native_promise_from_raw(promise_ptr_usize, "stdlib pump"),
        );
        // Run the converter on the main thread to create JSValues safely
        let result_bits = (resolution.converter)();
        let result_handle = scope.root_nanbox_u64(result_bits);

        // Issue #859: unpin BEFORE resolve. The converter ran first
        // and may itself have allocated (creating the result string,
        // etc.), but the promise stayed pinned across that work — so
        // even if the converter triggered GC, the promise survived.
        let promise_ptr = promise_handle.get_raw_mut_ptr::<perry_runtime::Promise>();
        unsafe { unpin_promise_after_native_resolution(promise_ptr as usize) };
        release_native_async_token(promise_ptr);
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

    // WebSocket and raw TCP socket events are pumped by perry-ext-ws /
    // perry-ext-net, which register their own pumps with the runtime (the
    // bundled copies that drained here were deleted in tokio lane L4).

    #[cfg(all(
        feature = "tls-runtime",
        not(target_os = "ios"),
        not(target_os = "android")
    ))]
    {
        count += unsafe { crate::tls::js_tls_process_pending() };
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
    #[cfg(feature = "compression-gzip")]
    {
        count += unsafe { crate::zlib::js_zlib_process_pending() };
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
    // Check for pending stdlib resolutions (turnloop P0: O(1) length mirrors).
    if PENDING_RESOLUTIONS_LEN.load(Ordering::Acquire) != 0
        || PENDING_DEFERRED_LEN.load(Ordering::Acquire) != 0
    {
        return 1;
    }
    // turnloop P6: an outbound request the client engine accepted is work the
    // process owes an answer for, and it holds NO `InflightGuard` on purpose.
    // The guard also feeds `native_work_inflight`, which makes the park choose
    // the legacy tokio tick over a turnloop turn (P4's note 2) — so a fetch
    // that took the turnloop path would have driven tokio to wait for work
    // tokio was not carrying. A separate predicate is the whole point.
    #[cfg(feature = "turnloop-http-client")]
    if crate::turnloop_client::has_pending_requests() {
        return 1;
    }
    #[cfg(feature = "turnloop-smtp-client")]
    if crate::turnloop_smtp::has_pending() {
        return 1;
    }
    // Active WebSocket / raw TCP handles keep the loop alive through the
    // keepalive contributors perry-ext-ws / perry-ext-net register with the
    // runtime (the bundled copies checked here were deleted in tokio lane L4).
    #[cfg(all(
        feature = "tls-runtime",
        not(target_os = "ios"),
        not(target_os = "android")
    ))]
    {
        if crate::tls::js_tls_has_active_handles() != 0 {
            return 1;
        }
    }
    // readline (#347 Phase 1) — keep the loop alive while a stdin
    // reader is started and EOF hasn't been observed, so `rl.on('line')`
    // / `rl.question()` programs don't exit before the user types.
    if crate::readline::js_readline_has_active() != 0 {
        return 1;
    }
    // #9399: the sibling registry. `process.stdin.on(...)` reached as an OBJECT
    // method — an alias, a parameter, or a field like claude-code's
    // `this._stdin.on("data", this._ondata)` — lands in perry-runtime's own
    // stdin listener lists, not readline's, and nothing here reported them. The
    // loop then exited 0 while the pipe was still open and the request unread.
    if perry_runtime::os::stdin_listeners_keep_loop_alive() {
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
    // zlib streams (#1843) — keep the loop alive while `.end()`-queued
    // 'data'/'end' events are still waiting to be drained, so a purely-
    // synchronous `createGzip().write(x).end()` program doesn't exit before
    // its listeners fire. Bundled path:
    #[cfg(feature = "compression-gzip")]
    {
        if crate::zlib::js_zlib_has_active_handles() != 0 {
            return 1;
        }
    }
    0
}

/// turnloop P4: run `work` on turnloop's shared blocking pool and settle
/// `promise_ptr` from its result, on the thread that owns the JS heap.
///
/// This is the retired tokio `spawn_for_promise_deferred`'s contract with the
/// tokio runtime taken out of the middle. The two halves are the same as before — owned Rust
/// data on the worker, JSValue construction on the main thread — but now the
/// split is a trait bound rather than a convention: `work` is `Send` and
/// returns `Result<T, String>`, and `converter` runs inside the completion
/// dispatch on the submitting thread.
///
/// Returns **false** when the pool refused the job, in which case the work has
/// already run inline on the calling thread and the promise is already
/// settled. A refusal happens on a thread with no event loop (a
/// `worker_threads` agent) or under pool backpressure, and it is visible:
/// `refused=` on the `PERRY_LOOP_STATS` line counts exactly those jobs.
///
/// # Safety
/// `promise_ptr` must point to a live Perry Promise, as for
/// [`spawn_for_promise_deferred`].
#[cfg(not(target_arch = "wasm32"))]
pub unsafe fn pool_for_promise_deferred<T, W, C>(
    promise_ptr: *mut u8,
    work: W,
    converter: C,
) -> bool
where
    T: Send + 'static,
    W: FnOnce() -> Result<T, String> + Send + 'static,
    // `Send` because the deferred-resolution queue is process-global and its
    // entries are `Send`; the closure still only ever RUNS on the main thread.
    C: FnOnce(T) -> u64 + Send + 'static,
{
    ensure_pump_registered();
    ensure_gc_scanner_registered();
    let ptr = promise_ptr as usize;
    // Issue #859, unchanged: the job holds the promise only as an address,
    // which no root scanner visits, so it is pinned across the crossing. The
    // pin is a flag bit and `js_promise_new_cross_thread` has already set it;
    // taking it again is idempotent and the single settlement releases it.
    pin_promise_for_native_resolution(ptr);
    perry_runtime::turnloop_pool::submit_or_run_inline(work, move |delivery| {
        use perry_runtime::turnloop_pool::Delivery;
        match delivery {
            Delivery::Done(Ok(data)) => {
                queue_deferred_resolution(ptr, true, move || converter(data));
            }
            Delivery::Done(Err(message)) => queue_rejection_string(ptr, message),
            // A cancelled or panicking job still owes the awaiter an answer;
            // leaving the promise pending is the one outcome a caller cannot
            // recover from (DESIGN D4).
            Delivery::Cancelled => {
                queue_rejection_string(ptr, "operation was cancelled".to_string())
            }
            Delivery::Failed(_) => {
                queue_rejection_string(ptr, "native operation failed".to_string())
            }
        }
    })
}

/// Reject `promise` with a JS string built on the main thread.
///
/// `queue_deferred_resolution`'s converter runs on the main thread, which is
/// the only place `js_string_from_bytes` is legal; every rejection path that
/// carries a message goes through here so none of them can forget.
pub(super) fn queue_rejection_string(promise: usize, message: String) {
    queue_deferred_resolution(promise, false, move || {
        let str_ptr = perry_runtime::js_string_from_bytes(message.as_ptr(), message.len() as u32);
        perry_runtime::JSValue::string_ptr(str_ptr).bits()
    });
}

/// Reject `promise_ptr` with `message` on the next pump.
///
/// The tokio-free spelling of `spawn_for_promise(p, async { Err(message) })`,
/// which bindings used for an argument error found before any work started:
/// the promise is pinned across the window exactly as the spawn pinned it, and
/// it settles in the same place, `js_stdlib_process_pending`. What is gone is
/// the task hop — the rejection no longer waits for a tokio tick to queue it.
///
/// # Safety
/// `promise_ptr` must point to a live Perry Promise.
pub unsafe fn reject_promise_later(promise_ptr: *mut u8, message: String) {
    ensure_pump_registered();
    ensure_gc_scanner_registered();
    let ptr = promise_ptr as usize;
    pin_promise_for_native_resolution(ptr);
    queue_rejection_string(ptr, message);
}

/// Serializes the unit tests that populate or drain `PENDING_RESOLUTIONS` /
/// `PENDING_DEFERRED` (#11417). Both queues are process-global and every pump
/// drains all of them, so a concurrent test's `clear_pending()` or pump takes
/// entries another test just queued — the resolution then settles (or is
/// scanned) on the wrong thread, or not at all.
#[cfg(test)]
static PENDING_QUEUE_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
fn pending_queue_test_lock() -> std::sync::MutexGuard<'static, ()> {
    PENDING_QUEUE_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Take the queues for this test and start it from empty.
    fn own_pending_queues() -> std::sync::MutexGuard<'static, ()> {
        let guard = pending_queue_test_lock();
        clear_pending();
        guard
    }

    fn clear_pending() {
        let mut resolutions = PENDING_RESOLUTIONS.lock().unwrap();
        resolutions.clear();
        PENDING_RESOLUTIONS_LEN.store(0, Ordering::Release);
        drop(resolutions);
        let mut deferred = PENDING_DEFERRED.lock().unwrap();
        deferred.clear();
        PENDING_DEFERRED_LEN.store(0, Ordering::Release);
    }

    #[test]
    fn stdlib_pump_releases_the_native_async_token_of_a_deferred_resolution() {
        let _queues = own_pending_queues();
        // perry-ffi's `JsPromise::new()` mints the promise through this shim,
        // which registers a native async token keyed by the promise address.
        let promise = crate::perry_ffi_async::perry_ffi_promise_new();
        assert!(!promise.is_null());
        assert!(perry_runtime::promise::native_async_promise_has_token(
            promise
        ));
        // `JsPromise::resolve_with` settles through the deferred queue, never
        // through the token API — the pump must drop the token itself or it
        // leaks one registry entry per native call (#9356).
        queue_deferred_resolution(promise as usize, true, || 7.0f64.to_bits());
        assert!(js_stdlib_process_pending() >= 1);
        assert_eq!(perry_runtime::promise::js_promise_state(promise), 1);
        assert_eq!(perry_runtime::promise::js_promise_value(promise), 7.0);
        assert!(!perry_runtime::promise::native_async_promise_has_token(
            promise
        ));
    }

    /// lane L: an argument error found before any work starts rejects through
    /// the tokio-free queue — pinned across the window, settled by the pump
    /// with the message as a JS string — instead of a spawned tokio task.
    #[test]
    fn reject_promise_later_rejects_with_the_message_on_the_next_pump() {
        let _queues = own_pending_queues();
        let promise = perry_runtime::js_promise_new_cross_thread();
        unsafe { reject_promise_later(promise as *mut u8, "Invalid password".to_string()) };
        assert_eq!(
            perry_runtime::promise::js_promise_state(promise),
            0,
            "nothing settles before the pump runs"
        );
        assert!(js_stdlib_process_pending() >= 1);
        assert_eq!(perry_runtime::promise::js_promise_state(promise), 2);
        let reason = perry_runtime::promise::js_promise_reason(promise);
        let ptr = perry_runtime::value::js_get_string_pointer_unified(reason)
            as *const perry_runtime::StringHeader;
        assert!(!ptr.is_null(), "the rejection reason is a JS string");
        let text = unsafe { crate::common::string_from_header_lossy(ptr) };
        assert_eq!(text.as_deref(), Some("Invalid password"));
    }

    #[test]
    fn stdlib_bridge_does_not_hard_reference_extension_pumps() {
        let source = include_str!("async_bridge.rs");
        let extension_symbols = [
            "js_ws_process_pending",
            "js_ws_has_pending",
            "js_net_process_pending",
            "js_ext_net_has_active_handles",
            "js_node_http_server_process_pending",
            "js_node_http_server_has_active",
            "js_http_process_pending",
            "js_http_has_pending",
            "js_ext_http_client_inflight",
            "js_ext_zlib_process_pending",
            "js_ext_zlib_has_active_handles",
        ];

        for symbol in extension_symbols {
            assert!(
                !source.contains(&format!("fn {symbol}(")),
                "stdlib must discover {symbol} through the runtime registry, not an extern declaration"
            );
        }
    }

    #[test]
    fn async_bridge_pending_resolution_scanner_emits_promise_and_result_roots() {
        let _queues = own_pending_queues();
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
