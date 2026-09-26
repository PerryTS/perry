//! Stable `extern "C"` shims that perry-ffi declares for use by
//! external native binding crates (#466 Phase 1 + 5).
//!
//! perry-ffi can't depend on perry-stdlib's internal Rust modules
//! (`crate::common::async_bridge::*`) because that would force every
//! external wrapper to take a workspace dep on perry-stdlib —
//! defeating the whole point of an ABI-stable surface. Instead, the
//! contract goes through C ABI:
//!
//! 1. perry-stdlib (this file) defines `#[no_mangle] extern "C"`
//!    shims wrapping `async_bridge`'s public Rust functions.
//! 2. perry-ffi declares those symbols as `extern "C"` and exposes
//!    safe Rust wrappers (`JsPromise`, `spawn_blocking`).
//! 3. External wrappers depend only on perry-ffi. At final link
//!    time, the `perry_ffi_*` undefined references they carry get
//!    resolved by perry-stdlib's archive — same mechanism as every
//!    other `_js_*` symbol that perry-stdlib exports today.
//!
//! Symbol naming uses the `perry_ffi_` prefix (vs. perry-stdlib's
//! existing `js_*`) so the contract is unambiguously bound to
//! perry-ffi's semver — not perry-stdlib's. A breaking change to
//! one of these signatures bumps perry-ffi major.

use std::ffi::c_void;

use crate::common::async_bridge;

extern "C" {
    fn js_native_async_completion_new(flags: u32) -> *mut c_void;
    fn js_native_async_completion_promise(token: *mut c_void) -> *mut perry_runtime::Promise;
    fn js_native_async_completion_resolve_bits(token: *mut c_void, bits: u64) -> i32;
    fn js_native_async_completion_reject_bits(token: *mut c_void, bits: u64) -> i32;
    fn js_native_async_completion_reject_string(
        token: *mut c_void,
        data: *const u8,
        len: usize,
    ) -> i32;
    fn js_native_async_completion_cancel(token: *mut c_void) -> i32;
    fn js_native_async_completion_attach_handle(
        token: *mut c_void,
        handle_bits: u64,
        cleanup_flags: u32,
    ) -> i32;
    fn js_native_async_completion_resolve_promise_bits(
        promise: *mut perry_runtime::Promise,
        bits: u64,
    ) -> i32;
    fn js_native_async_completion_reject_promise_bits(
        promise: *mut perry_runtime::Promise,
        bits: u64,
    ) -> i32;
}

/// `perry_ffi_promise_new()` — allocate a fresh Promise.
///
/// Thin pass-through to perry-runtime's allocator. Returned pointer
/// is owned by the runtime arena; resolution / rejection is what
/// transfers it to the awaiter.
///
/// Every documented use of `perry_ffi_promise_new` ships the pointer
/// to a worker (via `perry_ffi_spawn_blocking` / the v2 pool) and later resolves it from the worker, so
/// the promise must survive — and keep its address — for as long as
/// that raw pointer is out there. The await chain has no path back to
/// the promise (`P.next = N` is a forward edge; #859), so the native
/// async token registered here is what roots it. And because the
/// worker's copy of the address is invisible to the collector, the
/// promise is allocated in non-moving malloc space (see
/// `js_native_async_completion_new`, #9356): a nursery-resident
/// promise was relocated by the copying minor and the worker then
/// settled its retired from-space copy, leaving the awaited promise
/// pending forever. The token is released when the stdlib pump settles
/// the promise (`js_stdlib_process_pending`).
#[no_mangle]
pub extern "C" fn perry_ffi_promise_new() -> *mut perry_runtime::Promise {
    async_bridge::ensure_pump_registered();
    unsafe { js_native_async_completion_promise(js_native_async_completion_new(0)) }
}

/// `perry_ffi_promise_resolve_bits(promise, bits)` — resolve the
/// promise with a NaN-boxed JSValue, supplied as raw bits.
///
/// Caller is responsible for the bits being a valid encoded value
/// (e.g. a `STRING_TAG`-tagged pointer for strings, the bit pattern
/// of `1.0` / `0.0` for booleans). perry-ffi's safe wrappers handle
/// the encoding so external authors don't write the tag values
/// directly.
#[no_mangle]
pub extern "C" fn perry_ffi_promise_resolve_bits(promise: *mut perry_runtime::Promise, bits: u64) {
    async_bridge::ensure_pump_registered();
    unsafe {
        let _ = js_native_async_completion_resolve_promise_bits(promise, bits);
    }
}

/// `perry_ffi_promise_reject_bits(promise, bits)` — reject with a
/// JSValue. Same encoding contract as resolve.
#[no_mangle]
pub extern "C" fn perry_ffi_promise_reject_bits(promise: *mut perry_runtime::Promise, bits: u64) {
    async_bridge::ensure_pump_registered();
    unsafe {
        let _ = js_native_async_completion_reject_promise_bits(promise, bits);
    }
}

/// `perry_ffi_native_async_new(flags)` — allocate a runtime-owned native
/// async completion token and its JS-visible Promise.
#[no_mangle]
pub extern "C" fn perry_ffi_native_async_new(flags: u32) -> *mut c_void {
    async_bridge::ensure_pump_registered();
    unsafe { js_native_async_completion_new(flags) }
}

/// Return the JS Promise associated with a native async completion token.
#[no_mangle]
pub extern "C" fn perry_ffi_native_async_promise(
    token: *mut c_void,
) -> *mut perry_runtime::Promise {
    unsafe { js_native_async_completion_promise(token) }
}

/// Resolve a native async completion token with encoded JSValue bits.
#[no_mangle]
pub extern "C" fn perry_ffi_native_async_resolve_bits(token: *mut c_void, bits: u64) -> i32 {
    async_bridge::ensure_pump_registered();
    unsafe { js_native_async_completion_resolve_bits(token, bits) }
}

/// Reject a native async completion token with encoded JSValue bits.
#[no_mangle]
pub extern "C" fn perry_ffi_native_async_reject_bits(token: *mut c_void, bits: u64) -> i32 {
    async_bridge::ensure_pump_registered();
    unsafe { js_native_async_completion_reject_bits(token, bits) }
}

/// Reject a native async completion token with copied UTF-8 message bytes.
#[no_mangle]
pub extern "C" fn perry_ffi_native_async_reject_string(
    token: *mut c_void,
    data: *const u8,
    len: usize,
) -> i32 {
    async_bridge::ensure_pump_registered();
    unsafe { js_native_async_completion_reject_string(token, data, len) }
}

/// Cancel a native async completion token with Perry's default cancellation
/// reason.
#[no_mangle]
pub extern "C" fn perry_ffi_native_async_cancel(token: *mut c_void) -> i32 {
    async_bridge::ensure_pump_registered();
    unsafe { js_native_async_completion_cancel(token) }
}

/// Attach a JS native-handle value for cleanup according to cleanup flags.
#[no_mangle]
pub extern "C" fn perry_ffi_native_async_attach_handle(
    token: *mut c_void,
    handle_bits: u64,
    cleanup_flags: u32,
) -> i32 {
    unsafe { js_native_async_completion_attach_handle(token, handle_bits, cleanup_flags) }
}

/// `perry_ffi_promise_resolve_deferred(promise, ctx, invoke)` — resolve the
/// promise by running `invoke(ctx)` on the MAIN thread during the resolution
/// pump (`js_stdlib_process_pending`), using its return value as the result
/// bits. This is the safe path for native bindings that need to *construct*
/// JSValues (objects/arrays/strings) for the result: doing that on a
/// blocking-pool thread allocates from a worker thread-local arena that is
/// freed when the pooled thread idles out, leaving dangling objects on the
/// main thread (issue #1824). The worker keeps the JS-building closure boxed
/// (carrying only `Send` Rust data) and this defers its execution to the main
/// thread via the existing deferred-resolution queue. perry-ffi's
/// `JsPromise::resolve_with` builds the `ctx`/`invoke` pair.
#[no_mangle]
pub extern "C" fn perry_ffi_promise_resolve_deferred(
    promise: *mut perry_runtime::Promise,
    ctx: *mut std::ffi::c_void,
    invoke: extern "C" fn(*mut std::ffi::c_void) -> u64,
) {
    // `ctx` is a raw pointer (not Send); carry it as usize into the converter
    // closure, which runs on the main thread where `invoke` decodes + runs the
    // boxed JS-builder and returns the NaN-boxed result bits.
    let ctx_addr = ctx as usize;
    async_bridge::queue_deferred_resolution(promise as usize, true, move || {
        invoke(ctx_addr as *mut std::ffi::c_void)
    });
}

/// `perry_ffi_promise_reject_deferred(promise, ctx, invoke)` — rejection-side
/// twin of [`perry_ffi_promise_resolve_deferred`]. `invoke(ctx)` runs once on
/// the main thread so external bindings can safely allocate Error objects and
/// other structured rejection values after worker-thread work completes.
#[no_mangle]
pub extern "C" fn perry_ffi_promise_reject_deferred(
    promise: *mut perry_runtime::Promise,
    ctx: *mut std::ffi::c_void,
    invoke: extern "C" fn(*mut std::ffi::c_void) -> u64,
) {
    let ctx_addr = ctx as usize;
    async_bridge::queue_deferred_resolution(promise as usize, false, move || {
        invoke(ctx_addr as *mut std::ffi::c_void)
    });
}

/// `perry_ffi_spawn_blocking(ctx, invoke)` — run `invoke(ctx)` off the
/// calling thread. The caller boxes a closure into `ctx` and writes a thin
/// trampoline that decodes it inside `invoke`; perry-ffi's safe
/// `spawn_blocking` does exactly that. `invoke` takes ownership of `ctx`
/// (drops the box inside) — this function does not free it.
///
/// It runs on turnloop's `Occupancy::Long` worker set (PerryTS/turnloop#42) —
/// not the bounded set, because nothing in this ABI says the closure is short,
/// and a closure that holds its thread must not starve bcrypt/argon2/zlib's
/// bounded jobs. A thread with no event loop (a `worker_threads` agent before
/// its loop exists), or a refusal at the long set's ceiling, falls back to one
/// plain OS thread. (Until the final tokio lane this had a second arm on
/// tokio's blocking pool, compiled under the retired `async-runtime` feature;
/// this was the tokio-free arm since turnloop P8 lane L and is now the only
/// one.)
///
/// The in-flight counter (#591) is held for exactly the closure's run and
/// released even when the job is cancelled or dropped unrun, so the event loop
/// stays alive until the closure has queued its Promise resolution.
#[no_mangle]
pub extern "C" fn perry_ffi_spawn_blocking(ctx: *mut c_void, invoke: extern "C" fn(*mut c_void)) {
    async_bridge::ensure_pump_registered();
    let ctx_addr = ctx as usize;
    let inflight = async_bridge::InflightGuard::new();
    let run = move || {
        invoke(ctx_addr as *mut c_void);
        drop(inflight);
    };
    #[cfg(not(target_arch = "wasm32"))]
    {
        // `submit_long` consumes `run` only when it accepts the job, so a
        // refusal hands it back through the slot for the thread fallback.
        let slot = std::sync::Arc::new(std::sync::Mutex::new(Some(run)));
        let pool_slot = slot.clone();
        let accepted = perry_runtime::turnloop_pool::submit_long(
            move || {
                if let Some(run) = pool_slot
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .take()
                {
                    run();
                }
            },
            |_delivery| {},
        )
        .is_ok();
        if accepted {
            return;
        }
        let run = slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(run) = run {
            spawn_blocking_thread(run);
        }
    }
    #[cfg(target_arch = "wasm32")]
    run();
}

/// The no-loop fallback of `perry_ffi_spawn_blocking`: one OS thread with the
/// same stack reservation tokio's blocking pool used to give it (#10399).
/// If even the thread cannot be created the closure runs inline — the one
/// outcome that must not happen is an awaited promise never settling.
#[cfg(not(target_arch = "wasm32"))]
fn spawn_blocking_thread<F: FnOnce() + Send + 'static>(run: F) {
    let slot = std::sync::Arc::new(std::sync::Mutex::new(Some(run)));
    let thread_slot = slot.clone();
    let spawned = std::thread::Builder::new()
        .name("perry-blocking".to_string())
        .stack_size(async_bridge::blocking_thread_stack_size())
        .spawn(move || {
            let run = thread_slot
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            if let Some(run) = run {
                run();
            }
        });
    if spawned.is_err() {
        let run = slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(run) = run {
            run();
        }
    }
    perry_runtime::event_pump::js_native_work_submitted();
}

// `perry_ffi_spawn_blocking_with_reactor` and `perry_ffi_spawn_async` are
// RETIRED (final tokio lane). Both existed to hand work to tokio: the first ran
// a closure on a tokio blocking thread that carried the runtime's I/O reactor,
// the second drove a boxed tokio future on the shared runtime. Their contract
// is "tokio's reactor is ambient", which nothing can honour once tokio is gone,
// so they are deleted rather than stubbed — a wrapper still calling them fails
// at link time naming the symbol, instead of aborting at its first socket.
// No in-tree caller was left (perry-ext-net / -ws / -http moved to turnloop in
// lanes #11105 / #11265). Socket work belongs on turnloop
// (`perry_ffi::turnloop_net`, `perry_ffi::agent_post`); CPU work on
// `perry_ffi::spawn_blocking` or the v2 pool (`perry_ffi::pool`).

/// `perry_ffi_run_pending(budget_ms)` — one bounded event-loop turn (see
/// `perry-ffi::run_pending`). A synchronous native API that blocks the main
/// thread waiting for data another thread delivers (e.g.
/// `js_ws_wait_for_message`) calls this in its poll loop so the delivery is
/// actually collected: whatever such a caller waits for is either a turnloop
/// completion or a pump entry a pool/OS thread queues, and a bounded turn is
/// what observes both. Safe on the main thread between ticks.
///
/// turnloop P4 made this a v1 shim over the v2 [`perry_ffi_pool_turn`]; until
/// the final tokio lane it also drove a tokio half after the turn. The
/// signature never changed.
#[no_mangle]
pub extern "C" fn perry_ffi_run_pending(budget_ms: u64) {
    async_bridge::ensure_pump_registered();
    pool_turn(budget_ms);
}

// ── perry-ffi async ABI v2: the shared blocking pool ────────────────────────
//
// The v2 surface is three symbols, and the split between them is the contract
// (see `perry-ffi::pool`): `run_on_pool` runs on a turnloop pool thread with
// nothing but owned Rust data, `deliver_on_owner` runs on the thread that
// submitted, where JSValues are legal. perry-ffi owns both trampolines and the
// `ctx` box; this side only routes.

/// Outcome codes, matching `perry-ffi::pool`'s. Plain integers so the boundary
/// carries no Rust layout.
const POOL_OUTCOME_DONE: i32 = 0;
const POOL_OUTCOME_CANCELLED: i32 = 1;
const POOL_OUTCOME_FAILED: i32 = 2;

/// `perry_ffi_pool_submit(ctx, run_on_pool, deliver_on_owner)` — run
/// `run_on_pool(ctx)` on turnloop's shared bounded pool and
/// `deliver_on_owner(ctx, outcome)` on the calling thread once it finishes.
///
/// Returns the job id, or **0** when the submission was refused — this thread
/// has no event loop (a `worker_threads` agent), or the pool queue is full.
/// A refused submission delivers nothing and the caller still owns `ctx`.
///
/// Exactly one delivery per accepted job (turnloop DESIGN D4), including when
/// the job is cancelled or panics, so `ctx` is freed exactly once.
///
/// Unlike `perry_ffi_spawn_blocking` this needs no in-flight counter: an
/// accepted job is an outstanding turnloop operation, and
/// `js_stdlib_has_active_handles` already reports it through
/// `turnloop_pool::has_pending_jobs`.
#[no_mangle]
pub extern "C" fn perry_ffi_pool_submit(
    ctx: *mut c_void,
    run_on_pool: extern "C" fn(*mut c_void),
    deliver_on_owner: extern "C" fn(*mut c_void, i32),
) -> u64 {
    // Resolutions a delivery queues drain on the main thread through the
    // stdlib pump, exactly as for the v1 shims.
    async_bridge::ensure_pump_registered();
    #[cfg(not(target_arch = "wasm32"))]
    {
        // Raw pointers are not `Send`; the address is, and only the pool-side
        // trampoline dereferences it there (see `perry-ffi::pool::Ctx`).
        let ctx_addr = ctx as usize;
        let submitted = perry_runtime::turnloop_pool::submit(
            move || {
                run_on_pool(ctx_addr as *mut c_void);
            },
            move |delivery| {
                let outcome = match delivery {
                    perry_runtime::turnloop_pool::Delivery::Done(()) => POOL_OUTCOME_DONE,
                    perry_runtime::turnloop_pool::Delivery::Cancelled => POOL_OUTCOME_CANCELLED,
                    perry_runtime::turnloop_pool::Delivery::Failed(_) => POOL_OUTCOME_FAILED,
                };
                deliver_on_owner(ctx_addr as *mut c_void, outcome);
            },
        );
        match submitted {
            Ok(job) => job.raw(),
            Err(_) => 0,
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (ctx, run_on_pool, deliver_on_owner);
        0
    }
}

/// `perry_ffi_pool_cancel(job)` — ask the runtime to cancel an accepted job.
/// Best effort (turnloop DESIGN D8): a job the pool already started runs to its
/// end, and either way exactly one delivery still happens. Returns 0 when the
/// job is already delivered or unknown.
#[no_mangle]
pub extern "C" fn perry_ffi_pool_cancel(job: u64) -> i32 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let id = perry_runtime::turnloop_pool::JobId::from_raw(job);
        i32::from(perry_runtime::turnloop_pool::cancel(id))
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = job;
        0
    }
}

/// `perry_ffi_pool_turn(budget_ms)` — one bounded event-loop turn, so a
/// synchronous binding polling for a pool result actually collects it.
#[no_mangle]
pub extern "C" fn perry_ffi_pool_turn(budget_ms: u64) {
    pool_turn(budget_ms);
}

#[inline]
fn pool_turn(budget_ms: u64) {
    #[cfg(not(target_arch = "wasm32"))]
    perry_runtime::turnloop_pool::turn(budget_ms);
    #[cfg(target_arch = "wasm32")]
    let _ = budget_ms;
}

/// `perry_ffi_spawn_blocking` has one arm since the final tokio lane, so these
/// run in the default (`full`) test build. The unit-test thread has no agent
/// loop, so this exercises the plain-thread fallback; the `Occupancy::Long`
/// path is
/// `turnloop_pool::tests::a_long_job_runs_while_every_bounded_worker_is_held`.
#[cfg(all(test, not(target_arch = "wasm32")))]
mod spawn_blocking_tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, Instant};

    static RAN: AtomicU64 = AtomicU64::new(0);

    extern "C" fn record_run(ctx: *mut c_void) {
        // Take ownership of the ctx box exactly as perry-ffi's trampoline does.
        let caller = unsafe { Box::from_raw(ctx as *mut std::thread::ThreadId) };
        assert_ne!(
            *caller,
            std::thread::current().id(),
            "the closure must not run on the thread that called the shim"
        );
        RAN.fetch_add(1, Ordering::AcqRel);
    }

    #[test]
    fn spawn_blocking_runs_off_thread_and_releases_its_inflight() {
        let baseline = async_bridge::EXT_BLOCKING_TASKS_INFLIGHT.load(Ordering::Acquire);
        let ran_before = RAN.load(Ordering::Acquire);
        let ctx = Box::into_raw(Box::new(std::thread::current().id())) as *mut c_void;
        perry_ffi_spawn_blocking(ctx, record_run);
        let deadline = Instant::now() + Duration::from_secs(10);
        while RAN.load(Ordering::Acquire) == ran_before
            || async_bridge::EXT_BLOCKING_TASKS_INFLIGHT.load(Ordering::Acquire) != baseline
        {
            assert!(
                Instant::now() < deadline,
                "the closure never ran, or its in-flight reference leaked"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(
            RAN.load(Ordering::Acquire),
            ran_before + 1,
            "ran exactly once"
        );
    }
}
