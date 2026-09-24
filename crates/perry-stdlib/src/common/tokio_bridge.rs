//! The tokio half of the async bridge (turnloop P8 lane L).
//!
//! Compiled only under `async-runtime`. Everything here exists to drive tokio
//! futures that perry-stdlib's own tokio-socket modules (bundled `net`, `tls`,
//! `ws`), the reqwest fetch fallback and the container engine build, plus the
//! ones perry-ext-net / perry-ext-http hand over through `perry_ffi_spawn_async`
//! and `perry_ffi_spawn_blocking_with_reactor`. It settles promises only
//! through the tokio-free queue in `super::async_bridge`, which re-exports this
//! module's public names so their callers' paths are unchanged.
//!
//! When the last of those clients is off tokio this file is deleted whole, and
//! with it perry-stdlib's `tokio` edge; nothing in `async_bridge` has to move.

use std::future::Future;
use std::sync::atomic::Ordering;

use std::sync::LazyLock as Lazy;
use tokio::runtime::Runtime;

use super::async_bridge::{
    blocking_thread_stack_size, ensure_gc_scanner_registered, ensure_pump_registered,
    pin_promise_for_native_resolution, queue_deferred_resolution, queue_promise_resolution,
    InflightGuard, EXT_BLOCKING_TASKS_INFLIGHT,
};

/// Install the tokio half of the unified single-thread loop. Called once, from
/// `ensure_pump_registered`, before any async work spawns, so the first
/// `js_wait_for_event` after a spawn already drives the runtime.
///
/// The main JS loop drives the current-thread runtime one bounded tick per
/// `js_wait_for_event` (see `stdlib_wait_driver`). Forcing `RUNTIME` here also
/// constructs it on the main thread up front. turnloop P0: the primary agent
/// parks in its own loop and drives the tick only while tokio owns native work
/// (P0-transitional), which `native_work_inflight` reports.
pub(super) fn install_wait_driver() {
    perry_runtime::event_pump::js_register_wait_driver(
        Some(stdlib_wait_driver),
        Some(stdlib_fast_drive),
        Some(stdlib_wait_wake),
    );
    Lazy::force(&RUNTIME);
    perry_runtime::event_pump::js_register_native_inflight(Some(native_work_inflight));
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
    let tasks =
        RUNTIME_INITIALIZED.load(Ordering::Acquire) && RUNTIME.metrics().num_alive_tasks() != 0;
    i32::from(tasks || EXT_BLOCKING_TASKS_INFLIGHT.load(Ordering::Acquire) != 0)
}

// Stable LazyLock has no non-initializing get on our supported interface.
// Publish only after construction, so the activity probe never starts Tokio.
static RUNTIME_INITIALIZED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

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
    let runtime = tokio::runtime::Builder::new_current_thread()
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
        .expect("Failed to create tokio current-thread runtime");
    RUNTIME_INITIALIZED.store(true, Ordering::Release);
    runtime
});

/// Fired whenever a producer has queued main-thread-visible work (any
/// `js_notify_main_thread`, via the wait-driver wake). Ends the current bounded
/// tick in `stdlib_wait_driver`. `notify_one` coalesces and leaves a permit if
/// no tick is in progress, so a notify between ticks is not lost.
static EVENT_READY: tokio::sync::Notify = tokio::sync::Notify::const_new();

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
}
