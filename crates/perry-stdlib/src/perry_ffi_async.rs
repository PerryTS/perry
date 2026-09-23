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
/// to a worker future (via `perry_ffi_spawn_blocking*` /
/// `perry_ffi_spawn_async`) and later resolves it from the worker, so
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
/// JSValues (objects/arrays/strings) for the result: doing that on a tokio
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

/// `perry_ffi_spawn_blocking(ctx, invoke)` — run `invoke(ctx)` on
/// the global tokio runtime's blocking pool. The caller is expected
/// to box a closure into `ctx` before calling, and write a thin
/// trampoline that decodes the closure inside `invoke`. perry-ffi's
/// safe `spawn_blocking` does exactly that.
///
/// `invoke` must take ownership of `ctx` (drop the box inside) —
/// this function does not free `ctx` itself.
///
/// Why blocking-pool: most native bindings that need async (bcrypt,
/// argon2, fs, http) are CPU-bound or make synchronous I/O calls
/// that would stall a tokio worker. The blocking pool is the
/// recommended pattern; pure-async tasks can use this same shim
/// (the closure can run an `async {}` block via
/// `tokio::runtime::Handle::current().block_on`).
#[cfg(feature = "async-runtime")]
#[no_mangle]
pub extern "C" fn perry_ffi_spawn_blocking(ctx: *mut c_void, invoke: extern "C" fn(*mut c_void)) {
    // v0.5.579: ensure perry-stdlib's pump is registered with the
    // runtime so events queued by external wrappers (perry-ext-*)
    // get drained on the main thread. Without this, the runtime's
    // `js_run_stdlib_pump` sees a null function pointer and never
    // calls `js_stdlib_process_pending` → tokio events queued by
    // perry-ext-net / -ws / -http stay forever in their pending
    // queues and listener callbacks never fire.
    async_bridge::ensure_pump_registered();
    // SAFETY of the raw `ctx` pointer is the caller's; we only
    // forward it across the spawn boundary to `invoke`. Wrapping
    // pointers in a `usize` lets us cross the closure boundary
    // because raw pointers are not `Send`.
    let ctx_addr = ctx as usize;
    // #591: keep the event loop alive until the spawned closure has
    // queued its Promise resolution. See `EXT_BLOCKING_TASKS_INFLIGHT`.
    let inflight = async_bridge::InflightGuard::new();
    async_bridge::runtime().spawn_blocking(move || {
        invoke(ctx_addr as *mut c_void);
        // Dropping the guard wakes the main thread: well-formed wrappers
        // will have queued a Promise resolution from inside `invoke`,
        // which already notified — but a wrapper that resolves without
        // going through queue_* still needs the active-handle gate
        // to flip and re-evaluate.
        drop(inflight);
    });
    perry_runtime::event_pump::js_native_work_submitted();
}

/// `perry_ffi_spawn_blocking(ctx, invoke)` in a build WITHOUT tokio (turnloop
/// P8 lane L) — same contract, no tokio blocking pool to run on.
///
/// Every caller that reaches this arm is a CPU-only wrapper (perry-ads,
/// perry-ext-sharp, …): the auto-optimize driver selects `async-runtime` for
/// every binding whose closure enters tokio (`Handle::current()` from inside
/// the closure is only legal on tokio's own blocking pool), so this arm never
/// sees one. It runs `invoke(ctx)` on turnloop's `Occupancy::Long` worker set
/// (PerryTS/turnloop#42) — not the bounded set, because nothing in this ABI
/// says the closure is short, and a closure that holds its thread must not
/// starve bcrypt/argon2/zlib's bounded jobs. A thread with no event loop (a
/// `worker_threads` agent before its loop exists), or a refusal at the long
/// set's ceiling, falls back to one plain OS thread, which is what tokio's
/// blocking pool would have spawned in the same position.
///
/// The in-flight counter is held for exactly the closure's run, as in the
/// tokio arm, and released even when the job is cancelled or dropped unrun.
#[cfg(not(feature = "async-runtime"))]
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

/// The no-loop fallback of the tokio-free `perry_ffi_spawn_blocking`: one OS
/// thread with the same stack reservation tokio's blocking pool used (#10399).
/// If even the thread cannot be created the closure runs inline — the one
/// outcome that must not happen is an awaited promise never settling.
#[cfg(all(not(feature = "async-runtime"), not(target_arch = "wasm32")))]
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

/// `perry_ffi_spawn_blocking_with_reactor(ctx, invoke)` — like
/// `perry_ffi_spawn_blocking` but the wrapped closure is dispatched
/// through `RUNTIME.spawn(async { spawn_blocking(closure).await })`
/// instead of straight `RUNTIME.spawn_blocking(closure)`.
///
/// **Why both exist:** the plain `spawn_blocking` shim runs the
/// closure on a tokio blocking-pool thread that does NOT carry the
/// runtime's I/O reactor context. `tokio::runtime::Handle::current()
/// .block_on(fut)` from inside the closure spins up a fresh
/// current_thread runtime to drive the future, and that runtime has
/// no reactor — so any `TcpStream::connect` / `TcpListener::bind` /
/// `tokio::time::sleep` inside `fut` panics with "there is no
/// reactor running, must be called from the context of a Tokio 1.x
/// runtime".
///
/// This shim wraps the call so the blocking task inherits the
/// runtime context properly (reactor + handle accessible). The
/// wrapper detaches — the caller should not assume the closure
/// runs synchronously.
///
/// Used by perry-ext-net / perry-ext-ws / perry-ext-http (any
/// wrapper whose async work is pure I/O — TcpStream / hyper /
/// tokio-tungstenite). Closes the v0.5.571 net regression batch
/// (test_issue_422_socket_connect / test_net_min / test_net_socket /
/// test_net_upgrade_tls / test_sock_write_map all panicked with
/// "there is no reactor running" without this shim).
///
/// Compiled only with `async-runtime`: a closure that needs tokio's reactor
/// has nothing to run on without it, and a link error names the missing
/// feature where a stub would abort at runtime.
#[cfg(feature = "async-runtime")]
#[no_mangle]
pub extern "C" fn perry_ffi_spawn_blocking_with_reactor(
    ctx: *mut c_void,
    invoke: extern "C" fn(*mut c_void),
) {
    // v0.5.579: see `perry_ffi_spawn_blocking` above for why this
    // call is mandatory.
    async_bridge::ensure_pump_registered();
    let ctx_addr = ctx as usize;
    // #591: same active-handle gate as the plain variant.
    let inflight = async_bridge::InflightGuard::new();
    // Spawn directly on the multi-thread runtime so the closure
    // body runs on a worker thread that has full I/O reactor +
    // handle access. Inside the spawned task, `tokio::spawn(fut)`
    // and `Handle::current().spawn(fut)` both work for fan-out
    // I/O work.
    async_bridge::spawn_native(async move {
        invoke(ctx_addr as *mut c_void);
        drop(inflight);
    });
}

/// `perry_ffi_spawn_async(ctx)` — drive a future cooperatively on the
/// shared multi-thread runtime's worker pool. Used by wrappers whose
/// work is pure async I/O (TcpStream / WebSocket / hyper) and
/// shouldn't tie up a blocking-pool thread for the whole
/// connection's lifetime, unlike `perry_ffi_spawn_blocking*`.
///
/// `ctx` is a thin pointer to the `Pin<Box<dyn Future<Output = ()> +
/// Send>>` that perry-ffi's safe `spawn_async` boxed (the same
/// double-box trick `spawn_blocking` uses for `FnOnce`). perry-ffi has
/// no tokio dependency, so the spawn happens here on the perry-stdlib
/// side. The shared runtime carries the I/O reactor, so the future's
/// `TcpStream` / TLS / hyper work needs no ambient `Handle`.
///
/// Unlike the blocking shims, this does NOT bump
/// `EXT_BLOCKING_TASKS_INFLIGHT`: a cooperative task can outlive any
/// single resolution, so its caller must own an active-handle gate
/// (e.g. perry-ext-net's `js_ext_net_has_active_handles`) to keep the
/// event loop alive. The pump registration is preserved so events the
/// future queues still drain on the main thread.
///
/// # Safety
/// `ctx` must be a pointer produced by perry-ffi's `spawn_async` (i.e.
/// `Box::into_raw` of a `Box<Pin<Box<dyn Future<Output = ()> +
/// Send>>>`) and not already consumed.
///
/// Compiled only with `async-runtime`, for the reason
/// `perry_ffi_spawn_blocking_with_reactor` is: the future is a tokio future.
#[cfg(feature = "async-runtime")]
#[no_mangle]
pub unsafe extern "C" fn perry_ffi_spawn_async(ctx: *mut c_void) {
    // Register the main-thread pump exactly like the blocking variants
    // do (see `perry_ffi_spawn_blocking`), so resolutions the spawned
    // future queues get drained.
    async_bridge::ensure_pump_registered();
    type BoxFuture = std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>;
    // SAFETY: `ctx` came from perry-ffi's `spawn_async` (Box::into_raw
    // of `Box<BoxFuture>`); reconstruct + own it once.
    let future: BoxFuture = *unsafe { Box::from_raw(ctx as *mut BoxFuture) };
    async_bridge::spawn_native(future);
}

/// `perry_ffi_run_pending(budget_ms)` — drive the shared current-thread runtime
/// for one bounded tick (see `perry-ffi::run_pending`). A synchronous native API
/// that blocks the main thread waiting for data a spawned task delivers (e.g.
/// `js_ws_wait_for_message`) calls this in its poll loop so the delivering task
/// actually runs; in the unified single-thread model the runtime only advances
/// while the main thread drives it. Safe on the main thread between ticks; must
/// not be called from inside a spawned runtime task.
///
/// turnloop P4 (DESIGN §9, "`run_pending` becomes a bounded `turn`"): this is
/// now a **v1 shim over v2**. It takes a turnloop turn *first*, so a caller
/// polling for a blocking-pool result actually collects it — a turn is the only
/// thing that does — and then drives whatever tokio work is left. The tokio
/// half goes away with tokio in P8; the signature does not change.
///
/// The turn is deliberately **non-blocking** rather than given the caller's
/// budget: this shim's callers are waiting for something *tokio* will deliver
/// (`js_ws_wait_for_message`), and spending their budget parked in turnloop
/// would add a poll's worth of latency to every one of them. A caller that is
/// waiting for a pool result specifically asks for a blocking turn through the
/// v2 [`perry_ffi_pool_turn`].
///
/// Without tokio (turnloop P8 lane L) there is no tokio half to drive, so the
/// whole budget goes to the turn: whatever a caller is waiting for is either a
/// turnloop completion or a pump entry a pool/OS thread queues, and a bounded
/// turn is what observes both.
#[no_mangle]
pub extern "C" fn perry_ffi_run_pending(budget_ms: u64) {
    async_bridge::ensure_pump_registered();
    #[cfg(feature = "async-runtime")]
    {
        pool_turn(0);
        async_bridge::drive_pending(budget_ms);
    }
    #[cfg(not(feature = "async-runtime"))]
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

/// The tokio-free `perry_ffi_spawn_blocking` arm only compiles without
/// `async-runtime`, which the default (`full`) test build always has, so these
/// run under e.g. `cargo test -p perry-stdlib --no-default-features --features
/// crypto`. The unit-test thread has no agent loop, so this exercises the
/// plain-thread fallback; the `Occupancy::Long` path is
/// `turnloop_pool::tests::a_long_job_runs_while_every_bounded_worker_is_held`.
#[cfg(all(test, not(feature = "async-runtime"), not(target_arch = "wasm32")))]
mod tokio_free_tests {
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
    fn spawn_blocking_without_tokio_runs_off_thread_and_releases_its_inflight() {
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
