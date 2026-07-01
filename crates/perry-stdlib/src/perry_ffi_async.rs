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
/// Issue #859: the promise is also PINNED before returning, because
/// every documented use of `perry_ffi_promise_new` ships the pointer
/// to a worker future (via `perry_ffi_spawn_blocking*` /
/// `perry_ffi_spawn_async`) and later resolves it from the worker.
/// Without pinning, the await chain has no path back to the promise
/// — `P.next = N` is a forward edge — and a GC cycle during the
/// worker's run sweeps `P` mid-flight, turning the eventual
/// `js_promise_resolve(P, ...)` into a use-after-free SIGBUS. The
/// matching unpin lives in `js_stdlib_process_pending` (see
/// `unpin_promise_after_native_resolution` in `async_bridge`).
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
    use std::sync::atomic::Ordering;
    async_bridge::EXT_BLOCKING_TASKS_INFLIGHT.fetch_add(1, Ordering::AcqRel);
    async_bridge::runtime().spawn_blocking(move || {
        invoke(ctx_addr as *mut c_void);
        async_bridge::EXT_BLOCKING_TASKS_INFLIGHT.fetch_sub(1, Ordering::AcqRel);
        // Wake the main thread: well-formed wrappers will have
        // queued a Promise resolution from inside `invoke`, which
        // already notified — but a wrapper that resolves without
        // going through queue_* still needs the active-handle gate
        // to flip and re-evaluate.
        perry_runtime::event_pump::js_notify_main_thread();
    });
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
    use std::sync::atomic::Ordering;
    async_bridge::EXT_BLOCKING_TASKS_INFLIGHT.fetch_add(1, Ordering::AcqRel);
    // Spawn directly on the multi-thread runtime so the closure
    // body runs on a worker thread that has full I/O reactor +
    // handle access. Inside the spawned task, `tokio::spawn(fut)`
    // and `Handle::current().spawn(fut)` both work for fan-out
    // I/O work.
    async_bridge::runtime().spawn(async move {
        invoke(ctx_addr as *mut c_void);
        async_bridge::EXT_BLOCKING_TASKS_INFLIGHT.fetch_sub(1, Ordering::AcqRel);
        perry_runtime::event_pump::js_notify_main_thread();
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
    async_bridge::runtime().spawn(future);
}

/// `perry_ffi_run_pending(budget_ms)` — drive the shared current-thread runtime
/// for one bounded tick (see `perry-ffi::run_pending`). A synchronous native API
/// that blocks the main thread waiting for data a spawned task delivers (e.g.
/// `js_ws_wait_for_message`) calls this in its poll loop so the delivering task
/// actually runs; in the unified single-thread model the runtime only advances
/// while the main thread drives it. Safe on the main thread between ticks; must
/// not be called from inside a spawned runtime task.
#[no_mangle]
pub extern "C" fn perry_ffi_run_pending(budget_ms: u64) {
    async_bridge::ensure_pump_registered();
    async_bridge::drive_pending(budget_ms);
}
