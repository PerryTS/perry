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
    let p = perry_runtime::js_promise_new();
    unsafe { async_bridge::pin_promise_for_native_resolution(p as usize) };
    p
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
    async_bridge::queue_promise_resolution(promise as usize, true, bits);
}

/// `perry_ffi_promise_reject_bits(promise, bits)` — reject with a
/// JSValue. Same encoding contract as resolve.
#[no_mangle]
pub extern "C" fn perry_ffi_promise_reject_bits(promise: *mut perry_runtime::Promise, bits: u64) {
    async_bridge::queue_promise_resolution(promise as usize, false, bits);
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

/// `perry_ffi_spawn_async(ctx, invoke)` — schedule `invoke(ctx)` to
/// run on the multi-thread runtime's worker pool. Used by wrappers
/// whose work is pure async I/O (TcpStream / WebSocket / hyper) and
/// shouldn't tie up a blocking-pool thread for the whole
/// connection's lifetime. Closes #421 / regression batch 2 from the
/// v0.5.571 net + ws + http port.
///
/// The trampoline pattern matches `spawn_blocking` — perry-ffi
/// boxes the user's `Pin<Box<dyn Future>>` into `ctx` and writes a
/// trampoline that decodes + spawns it.
///
/// `invoke` must take ownership of `ctx`.
#[no_mangle]
pub extern "C" fn perry_ffi_spawn_async(ctx: *mut c_void, invoke: extern "C" fn(*mut c_void)) {
    let ctx_addr = ctx as usize;
    async_bridge::runtime().spawn(async move {
        // The user-supplied trampoline receives the raw `ctx`,
        // reconstructs the boxed future, and `.await`s it inline.
        // This block runs on a worker thread with full reactor
        // access (TcpStream::connect / TLS handshakes / etc.).
        invoke(ctx_addr as *mut c_void);
    });
}
