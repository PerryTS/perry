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

/// Owns exactly one `EXT_BLOCKING_TASKS_INFLIGHT` increment, released on drop.
///
/// Create it BEFORE spawning and move it into the task: the decrement then runs
/// on completion, on error, when the task panics (tokio drops the future while
/// unwinding) and when the task is dropped before its first poll (runtime
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

/// Spawn onto the shared runtime, then tell the primary agent that tokio-owned
/// native work now exists (turnloop P0-transitional). A primary agent parked in
/// a turnloop turn re-selects its wait so the tokio tick runs the new task;
/// otherwise the hint is one atomic load. Needed for a spawn from a thread that
/// is not the primary agent's (a `worker_threads` Worker, a blocking-pool
/// closure), which tokio's own unpark cannot deliver to a turnloop wait.
pub(crate) fn spawn_native<F>(future: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    RUNTIME.spawn(future);
    perry_runtime::event_pump::js_native_work_submitted();
}

/// turnloop P0-transitional predicate registered with
/// `js_register_native_inflight`: nonzero while tokio owns native work that only
/// advances inside its own tick. O(1): the in-flight counter above plus tokio's
/// alive-task count (an atomic read). Every task on the shared current-thread
/// runtime counts — fetch/net/ws/db connections, server accept loops — so the
/// primary agent drives the legacy tick exactly while any of it exists and
/// parks in its turnloop loop otherwise. P8 deletes this with tokio.
extern "C" fn native_work_inflight() -> i32 {
    let tasks = Lazy::get(&RUNTIME).is_some_and(|rt| rt.metrics().num_alive_tasks() != 0);
    i32::from(tasks || EXT_BLOCKING_TASKS_INFLIGHT.load(Ordering::Acquire) != 0)
}

/// Global tokio runtime for all async stdlib operations.
///
/// Unified single-thread async model: a CURRENT-THREAD runtime, driven one
/// bounded tick at a time by the main JS event loop (see `stdlib_wait_driver`
/// and `js_register_wait_driver`). The I/O reactor, timer wheel, and all
/// spawned native tasks (reqwest / net / ws) run on the main thread interleaved
/// with JS — Node's model — so a native completion is observed in-thread and
/// queued with no cross-thread wake to lose. `spawn_blocking` still offloads
/// genuinely blocking / CPU-bound work to the blocking-thread pool; its result
/// is delivered back and ends the next tick.
pub static RUNTIME: Lazy<Runtime> = Lazy::new(|| {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        // #10399: glibc carves a thread's static TLS block out of the same
        // mapping as its stack, so a compiled program's TLS comes off the top
        // of whatever we ask for here. OpenCode's binary carries 5.79 MB of
        // PT_TLS once module state is per-thread; against tokio's 2 MB default
        // the blocking threads were left with almost no usable stack and
        // SIGSEGV'd deep inside reqwest's connector on first use, while the
        // main thread (whose TLS is allocated separately) was fine.
        //
        // The stack is reserved address space, committed lazily, so a generous
        // reservation costs no RSS. `PERRY_THREAD_STACK_SIZE` overrides it.
        .thread_stack_size(blocking_thread_stack_size())
        .build()
        .expect("Failed to create tokio current-thread runtime")
});

/// #10399: stack reservation for perry-spawned threads. See `RUNTIME`.
pub fn blocking_thread_stack_size() -> usize {
    const DEFAULT: usize = 32 * 1024 * 1024;
    std::env::var("PERRY_THREAD_STACK_SIZE")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|v| *v >= 1024 * 1024)
        .unwrap_or(DEFAULT)
}

/// Fired whenever a producer has queued main-thread-visible work (any
/// `js_notify_main_thread`, via the wait-driver wake). Ends the current bounded
/// tick in `stdlib_wait_driver`. `notify_one` coalesces and leaves a permit if
/// no tick is in progress, so a notify between ticks is not lost.
static EVENT_READY: tokio::sync::Notify = tokio::sync::Notify::const_new();

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
    let inflight = InflightGuard::new();
    spawn_native(async move {
        future.await;
        // Dropping the guard notifies in case the future resolved without
        // going through `queue_promise_resolution` — flip the active-handle
        // gate so the loop re-evaluates.
        drop(inflight);
    });
}

/// Block on an async task (use sparingly, mainly for initialization)
pub fn block_on<F, T>(future: F) -> T
where
    F: Future<Output = T>,
{
    RUNTIME.block_on(future)
}

/// Wait-driver SLEEP side — one bounded tick of the current-thread runtime.
///
/// Installed via `js_register_wait_driver` and called by the main loop's
/// `js_wait_for_event` in place of a condvar park. `block_on` drives the I/O
/// reactor, the timer wheel, and every spawned native task (reqwest / net / ws)
/// on THIS (main) thread until a producer fires `EVENT_READY` — i.e. a native
/// task queued a resolution / pushed an event — or `budget_ms` elapses,
/// whichever first. Because the producing task ran in this same tick, its
/// completion is observed in-thread and `perry_poll` drains it on the next loop
/// turn; there is no cross-thread wake to lose. `budget_ms` is the loop's
/// computed sleep budget (min of the next Perry timer/native deadline and the
/// 1 s idle cap); a zero budget is floored to 1 ms so native work still gets
/// one poll cycle under a hot timer.
extern "C" fn stdlib_wait_driver(budget_ms: u64) {
    run_one_tick(budget_ms);
}

/// One bounded tick of the current-thread runtime: drive the reactor + timers +
/// spawned native tasks until `EVENT_READY` fires or `budget_ms` (floored to
/// 1 ms) elapses. Shared by the main-loop wait-driver and `perry_ffi_run_pending`
/// (a synchronous native API that must let a delivering task run — see
/// `perry-ffi::run_pending`). Must NOT be called from inside a spawned runtime
/// task (no nested `block_on`); only from the main thread between ticks.
pub fn run_one_tick(budget_ms: u64) {
    extern "C" {
        fn js_main_thread_notified() -> i32;
    }
    let budget = std::time::Duration::from_millis(budget_ms.max(1));
    RUNTIME.block_on(async {
        let notified = EVENT_READY.notified();
        tokio::pin!(notified);
        // Register as a waiter BEFORE checking the condition: a `notify_waiters`
        // that lands between the check and the await still wakes us (it wakes only
        // registered waiters). `enable()` returns false here because the wake side
        // stores no permit.
        notified.as_mut().enable();
        // End immediately if a native result was already queued during this tick
        // (the durable `NOTIFIED` flag — checked instead of a notify permit so a
        // stale wake can't make us skip parking on the reactor). The main loop
        // cleared `NOTIFIED` before this tick, so a set flag is fresh work.
        if unsafe { js_main_thread_notified() } != 0 {
            return;
        }
        // Otherwise park: `block_on` drives every spawned native task (reqwest /
        // net / ws) and parks on the I/O reactor on this thread until a producer
        // queues a result (`notify_waiters` wakes us; we re-check NOTIFIED) or the
        // budget elapses.
        let _ = tokio::time::timeout(budget, notified).await;
    });
}

/// Drive the runtime for the full `budget_ms` (floored to 1 ms), parking on the
/// I/O reactor so spawned native tasks make progress. Unlike `run_one_tick` this
/// does NOT end early on the `NOTIFIED` flag — it is for a *synchronous* native
/// API (`perry_ffi_run_pending`, e.g. `js_ws_wait_for_message`) that is called
/// mid-`perry_poll` (where `NOTIFIED` may already be set for unrelated reasons)
/// and just needs the delivering task to run for a slice before it re-checks its
/// own condition.
pub fn drive_pending(budget_ms: u64) {
    let budget = std::time::Duration::from_millis(budget_ms.max(1));
    RUNTIME.block_on(async {
        tokio::time::sleep(budget).await;
    });
}

/// Wait-driver WAKE side — ends the current bounded tick. Fired from
/// `js_notify_main_thread` (via `js_register_wait_driver`) by any producer: the
/// in-thread native task during a tick, or a blocking-pool thread cross-thread.
/// Uses `notify_waiters` (NOT `notify_one`) so it stores NO permit: a notify
/// outside a tick is intentionally dropped (the corresponding `NOTIFIED` flag is
/// the durable signal the tick re-checks), which is what keeps stale permits from
/// making every tick return instantly without parking on the reactor.
extern "C" fn stdlib_wait_wake() {
    EVENT_READY.notify_waiters();
}

/// Wait-driver FAST side — a brief native drive invoked by `js_wait_for_event`
/// when JS work is pending (a notify or queued microtasks). On the single-thread
/// runtime, in-flight native tasks (a fetch's reqwest `send`, its h2 connection
/// driver, sibling fetches, or a server accept loop) run ONLY inside a tick;
/// under constant JS promise churn the fast-path is taken every iteration, so
/// without this they are starved forever (the bundle hang). When something
/// native IS in flight, drive one short (1 ms) tick: `block_on` drains the run
/// queue (starts freshly-spawned tasks) and parks briefly on the I/O reactor
/// (advancing TLS/h2 round-trips and accepting server connections), ending early
/// if a native result is queued. No-op when nothing native is in flight, so
/// pure-JS-async pays only atomic loads.
extern "C" fn stdlib_fast_drive() {
    extern "C" {
        fn js_aux_has_active() -> i32;
    }
    let n = EXT_BLOCKING_TASKS_INFLIGHT.load(Ordering::Acquire);
    let registered_extension_active = unsafe { js_aux_has_active() != 0 };
    let native = native_fast_drive_needed(n, registered_extension_active);
    if !native {
        return;
    }
    // PERRY_LOOP_STATS: a fast drive that actually ran tokio.
    let stats = perry_runtime::event_pump::loop_stats::begin_fast_drive();
    RUNTIME.block_on(async {
        let notified = EVENT_READY.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        let _ = tokio::time::timeout(std::time::Duration::from_millis(1), notified).await;
    });
    perry_runtime::event_pump::loop_stats::end_fast_drive(stats);
}

#[inline]
fn native_fast_drive_needed(blocking_tasks_inflight: usize, extension_active: bool) -> bool {
    blocking_tasks_inflight > 0 || extension_active
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
        // Unified single-thread async model: install the wait-driver so the main
        // JS loop drives the current-thread tokio runtime one bounded tick per
        // `js_wait_for_event` (see `stdlib_wait_driver`). Registered here, before
        // any async work spawns, so the first `js_wait_for_event` after a spawn
        // already drives the runtime. Forcing RUNTIME now also constructs it on
        // the main thread up front.
        perry_runtime::event_pump::js_register_wait_driver(
            Some(stdlib_wait_driver),
            Some(stdlib_fast_drive),
            Some(stdlib_wait_wake),
        );
        Lazy::force(&RUNTIME);
        // turnloop P0: the primary agent parks in its own loop and drives the
        // tick above only while tokio owns native work (P0-transitional).
        perry_runtime::event_pump::js_register_native_inflight(Some(native_work_inflight));
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

    // Process pending WebSocket events (server/client listener callbacks).
    // External WebSocket implementations register their own pump with runtime.
    #[cfg(feature = "websocket")]
    {
        count += unsafe { crate::ws::js_ws_process_pending() };
    }

    // Process pending bundled raw TCP socket events (net.Socket).
    // External net implementations register their own pump with runtime.
    #[cfg(all(
        feature = "bundled-net",
        not(target_os = "ios"),
        not(target_os = "android")
    ))]
    {
        count += unsafe { crate::net::js_net_process_pending() };
    }

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
    // Check bundled raw TCP sockets. External net implementations register
    // their own keepalive contributor with runtime and remain invisible here.
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
    let inflight = InflightGuard::new();
    spawn_native(async move {
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
        drop(inflight);
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
    let inflight = InflightGuard::new();
    spawn_native(async move {
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
        drop(inflight);
    });
}

/// turnloop P4: run `work` on turnloop's shared blocking pool and settle
/// `promise_ptr` from its result, on the thread that owns the JS heap.
///
/// This is [`spawn_for_promise_deferred`]'s contract with the tokio runtime
/// taken out of the middle. The two halves are the same as before — owned Rust
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
fn queue_rejection_string(promise: usize, message: String) {
    queue_deferred_resolution(promise, false, move || {
        let str_ptr = perry_runtime::js_string_from_bytes(message.as_ptr(), message.len() as u32);
        perry_runtime::JSValue::string_ptr(str_ptr).bits()
    });
}

/// Spawn an async operation whose success and error values both need to be
/// materialized on the main thread.
///
/// Database adapters use this variant to reject with a real JavaScript Error
/// (including driver-specific fields such as `code` and `errno`) instead of a
/// bare string. As with [`spawn_for_promise_deferred`], neither converter runs
/// on the async executor, where allocating Perry heap values would be unsafe.
///
/// # Safety
/// `promise_ptr` must point to a live Perry Promise.
pub unsafe fn spawn_for_promise_deferred_with_error<T, E, F, C, R>(
    promise_ptr: *mut u8,
    future: F,
    converter: C,
    reject_converter: R,
) where
    T: Send + 'static,
    E: Send + 'static,
    F: Future<Output = Result<T, E>> + Send + 'static,
    C: FnOnce(T) -> u64 + Send + 'static,
    R: FnOnce(E) -> u64 + Send + 'static,
{
    ensure_pump_registered();
    ensure_gc_scanner_registered();
    let ptr = promise_ptr as usize;
    pin_promise_for_native_resolution(ptr);

    let inflight = InflightGuard::new();
    spawn_native(async move {
        match future.await {
            Ok(data) => queue_deferred_resolution(ptr, true, move || converter(data)),
            Err(error) => queue_deferred_resolution(ptr, false, move || reject_converter(error)),
        }
        drop(inflight);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// turnloop P0: one in-flight reference per spawned native task, released
    /// on success, error, panic and cancellation before first poll. Uses a
    /// private runtime so the shared one's other tasks cannot interfere; each
    /// step proves its task ran (or was cancelled) before checking the count.
    #[test]
    fn inflight_guard_balances_success_error_panic_and_cancel() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let count = || EXT_BLOCKING_TASKS_INFLIGHT.load(Ordering::Acquire);
        let baseline = count();
        for fail in [false, true] {
            let guard = InflightGuard::new();
            assert_eq!(count(), baseline + 1);
            let task = runtime.spawn(async move {
                let _guard = guard;
                if fail {
                    Err(())
                } else {
                    Ok(())
                }
            });
            assert_eq!(runtime.block_on(task).unwrap().is_err(), fail);
            assert_eq!(count(), baseline, "fail={fail} leaked a reference");
        }
        let guard = InflightGuard::new();
        let panicked = runtime.spawn(async move {
            let _guard = guard;
            panic!("native task panicked");
        });
        assert!(runtime.block_on(panicked).unwrap_err().is_panic());
        assert_eq!(count(), baseline, "a panicking task leaked a reference");
        let guard = InflightGuard::new();
        let cancelled = runtime.spawn(async move {
            let _guard = guard;
            std::future::pending::<()>().await;
        });
        assert_eq!(count(), baseline + 1);
        cancelled.abort();
        assert!(runtime.block_on(cancelled).unwrap_err().is_cancelled());
        assert_eq!(count(), baseline, "a cancelled task leaked a reference");
        let guard = InflightGuard::new();
        let never_polled = runtime.spawn(async move {
            let _guard = guard;
        });
        drop(runtime);
        drop(never_polled);
        assert_eq!(
            count(),
            baseline,
            "a task dropped unpolled leaked a reference"
        );
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
        clear_pending();
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

    /// PERRY_LOOP_STATS, real wiring: a live tokio task makes the primary
    /// agent's park a TOKIO TICK, and the notify that ends it is one
    /// wake-latency sample — `native_work_inflight()` hands the wait back to
    /// the tick.
    #[test]
    fn a_live_tokio_task_parks_the_main_loop_in_a_counted_tokio_tick() {
        use perry_runtime::event_pump::loop_stats;
        loop_stats::enable_for_tests();
        ensure_pump_registered();
        // Drain a notify left by an earlier test: `js_wait_for_event` would
        // take its fast path and never reach a wait.
        while perry_runtime::event_pump::js_main_thread_notified() != 0 {
            perry_runtime::event_pump::js_wait_for_event();
        }

        let (ran_tx, ran_rx) = std::sync::mpsc::channel();
        spawn_native(async move {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            perry_runtime::event_pump::js_notify_main_thread();
            let _ = ran_tx.send(());
        });
        assert!(
            RUNTIME.metrics().num_alive_tasks() >= 1,
            "the spawned task is the subject and tokio does not own it"
        );

        let before = loop_stats::snapshot();
        let started = std::time::Instant::now();
        // Bounded: the idle-reclaim hook can consume a park by doing GC work.
        for _ in 0..5 {
            perry_runtime::event_pump::js_wait_for_event();
            if loop_stats::snapshot().tokio_tick.count > before.tokio_tick.count {
                break;
            }
        }
        let after = loop_stats::snapshot();
        ran_rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("the spawned task never ran");

        assert_eq!(
            after.tokio_tick.count - before.tokio_tick.count,
            1,
            "a live tokio task did not produce exactly one tokio tick"
        );
        assert!(
            after.tokio_tick.total_ns > before.tokio_tick.total_ns,
            "the tick was counted with no time in it"
        );
        assert_eq!(
            after.turnloop.count - before.turnloop.count,
            0,
            "tokio-owned work was miscounted as a turnloop turn"
        );
        assert_eq!(
            after.wake_samples() - before.wake_samples(),
            1,
            "the notify that ended the tick produced no wake-latency sample"
        );
        assert!(
            started.elapsed() >= std::time::Duration::from_millis(15),
            "the park returned before the task's notify: it never parked"
        );
        while perry_runtime::event_pump::js_main_thread_notified() != 0 {
            perry_runtime::event_pump::js_wait_for_event();
        }
    }

    #[test]
    fn active_extension_keeps_the_fast_wait_path_driving_native_tasks() {
        assert!(!native_fast_drive_needed(0, false));
        assert!(native_fast_drive_needed(0, true));
        assert!(native_fast_drive_needed(1, false));
        assert!(native_fast_drive_needed(1, true));
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
