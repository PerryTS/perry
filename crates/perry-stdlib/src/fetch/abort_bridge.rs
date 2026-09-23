//! Bridges a JS `AbortSignal` to an in-flight `fetch` so the request rejects
//! when the signal aborts — either `controller.abort()` or an
//! `AbortSignal.timeout(ms)` deadline elapsing.
//!
//! The signal reaches the native fetch via the runtime's pending-signal stash
//! (`js_fetch_set_pending_signal` / `js_fetch_take_pending_signal`), which keeps
//! the 4-arg `js_fetch_with_options` ABI unchanged. At the start of
//! `js_fetch_with_options` (main thread) the signal's object address becomes
//! the request's cancellation key in the turnloop client engine.
//!
//! The signal's abort reaches us through the runtime: `fire_abort_listeners`
//! (run by `controller.abort()` and the `AbortSignal.timeout` deadline) calls
//! `js_fetch_notify_signal_aborted`, which cancels every engine request bound
//! to that key. There is deliberately no per-fetch JS `abort` listener, so
//! reused signals never accumulate stale listener closures.
//!
//! Before the reqwest fallback was removed this module also kept a registry of
//! `tokio::sync::Notify`s that a reqwest future `select!`ed against. Every
//! fetch now goes through the engine, which owns cancellation itself, so the
//! registry had no reader left and went with it. Refs the AbortSignal runtime
//! in `perry_runtime::url::abort`.

use crate::common::async_bridge::queue_deferred_resolution;

unsafe extern "C" {
    // `js_fetch_take_pending_signal` lives in perry-runtime's private
    // `object::global_fetch` module, so it is reached as a `#[no_mangle]`
    // extern rather than by Rust path.
    fn js_fetch_take_pending_signal() -> f64;
}

/// Cancel every in-flight fetch bound to `signal_ptr`. Called on the main
/// thread by the runtime's `fire_abort_listeners` (`perry_runtime::url::abort`)
/// when the signal aborts — `controller.abort()` or an `AbortSignal.timeout`
/// deadline. Cancelling a request closes its socket, which cancels the
/// in-flight operation on the loop exactly once, and delivers the request as
/// aborted. A miss (no in-flight fetch for the signal) is a no-op.
#[no_mangle]
pub extern "C" fn js_fetch_notify_signal_aborted(signal_ptr: i64) {
    crate::turnloop_client::abort_signal(signal_ptr as usize);
}

/// A live (not yet aborted) signal bound to one request.
pub(crate) struct FetchAbortWatch {
    signal_ptr: usize,
}

impl FetchAbortWatch {
    /// The signal's object address, used by the turnloop engine as its own
    /// cancellation key. Never dereferenced there.
    pub(crate) fn signal_ptr(&self) -> usize {
        self.signal_ptr
    }
}

/// Outcome of consuming the pending fetch signal on the main thread.
pub(crate) enum SignalState {
    /// No signal, or the value was not an AbortSignal — dispatch normally.
    None,
    /// The signal was already aborted before the request started — the caller
    /// should reject immediately without dispatching.
    AlreadyAborted,
    /// A live signal the request is bound to.
    Watch(FetchAbortWatch),
}

/// Consume the runtime's pending fetch signal. An already-aborted signal
/// short-circuits to a reject. MUST run on the main thread (it reads the signal
/// object).
pub(crate) fn take_pending_signal_watch() -> SignalState {
    let signal_value = unsafe { js_fetch_take_pending_signal() };
    let signal = perry_runtime::url::js_abort_signal_resolve_ptr(signal_value);
    if signal.is_null() {
        return SignalState::None;
    }
    if perry_runtime::url::js_abort_signal_is_aborted(signal) != 0 {
        return SignalState::AlreadyAborted;
    }
    SignalState::Watch(FetchAbortWatch {
        signal_ptr: signal as usize,
    })
}

/// Resolve a freshly-taken `SignalState` against the request's promise: reject
/// up front when the signal was already aborted (returns `None`, telling the
/// caller to return the promise immediately), otherwise yield the optional
/// watch the request is bound to.
pub(crate) fn watch_or_reject(
    state: SignalState,
    promise_ptr: usize,
) -> Option<Option<FetchAbortWatch>> {
    match state {
        SignalState::AlreadyAborted => {
            queue_deferred_resolution(promise_ptr, false, abort_error_bits);
            None
        }
        SignalState::Watch(watch) => Some(Some(watch)),
        SignalState::None => Some(None),
    }
}

/// NaN-boxed bits of a fresh `AbortError` for rejecting an aborted fetch. Built
/// inside a `queue_deferred_resolution` converter so the error object is
/// allocated on the main thread (never on a worker).
pub(crate) fn abort_error_bits() -> u64 {
    perry_runtime::url::js_abort_error_value().to_bits()
}
