//! Bridges a JS `AbortSignal` to an in-flight `fetch` so the request rejects
//! when the signal aborts — either `controller.abort()` or an
//! `AbortSignal.timeout(ms)` deadline elapsing.
//!
//! The signal reaches the native fetch via the runtime's pending-signal stash
//! (`js_fetch_set_pending_signal` / `js_fetch_take_pending_signal`), alongside
//! the equivalent `RequestInit.redirect` bridge, which keeps the 4-arg
//! `js_fetch_with_options` ABI unchanged. At the start of
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

use crate::common::async_bridge::{queue_deferred_resolution, queue_promise_resolution};
use std::cell::RefCell;

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
///
/// A request rejects with the signal's reason (node), read here while the
/// signal is live and kept rooted (`visit_abort_reasons`) until each request
/// it aborted here has settled. A request another thread runs for this agent
/// (posted) settles there and rejects with an `AbortError`.
#[no_mangle]
pub extern "C" fn js_fetch_notify_signal_aborted(signal_ptr: i64) {
    let signal = signal_ptr as *mut perry_runtime::ObjectHeader;
    let bits = perry_runtime::event_target_hook_slots::js_abort_signal_reason(signal).to_bits();
    let handle = begin_abort(bits);
    let n = crate::turnloop_client::abort_signal(signal_ptr as usize, handle);
    end_abort(handle, n);
}

/// Root `bits`, the reason of an abort that is about to cancel requests.
fn begin_abort(bits: u64) -> usize {
    REASONS.with(|r| {
        let mut r = r.borrow_mut();
        r.next = r.next.wrapping_add(1).max(1);
        let handle = r.next;
        r.live.push(AbortReason {
            handle,
            bits,
            taken: 0,
            expected: None,
        });
        handle
    })
}

/// The abort `handle` cancelled `n` requests here: its reason stays rooted
/// until each of them has taken it (`take_abort_reason`).
fn end_abort(handle: usize, n: usize) {
    REASONS.with(|r| {
        let mut r = r.borrow_mut();
        if let Some(i) = r.live.iter().position(|a| a.handle == handle) {
            r.live[i].expected = Some(n);
            if r.live[i].taken >= n {
                r.live.swap_remove(i);
            }
        }
    });
}

/// One abort's reason, rooted until the requests it aborted have settled.
struct AbortReason {
    handle: usize,
    bits: u64,
    taken: usize,
    /// How many requests the abort cancelled; `None` while it runs.
    expected: Option<usize>,
}

#[derive(Default)]
struct Reasons {
    next: usize,
    live: Vec<AbortReason>,
}

thread_local! {
    static REASONS: RefCell<Reasons> = RefCell::new(Reasons::default());
}

/// The reasons of aborts whose requests have not all settled: GC roots.
pub(super) fn visit_abort_reasons<V: super::gc::FetchRootVisitor>(visitor: &mut V) {
    REASONS.with(|r| {
        for a in r.borrow_mut().live.iter_mut() {
            visitor.visit_nanbox_u64_slot(&mut a.bits);
        }
    });
}

/// The rejection of a request delivered as aborted with reason handle
/// `handle`: the signal's reason, taken once per request. `None`: no reason
/// (handle 0, a posted cancellation), an `AbortError`.
fn take_abort_reason(handle: usize) -> Option<u64> {
    if handle == 0 {
        return None;
    }
    REASONS.with(|r| {
        let mut r = r.borrow_mut();
        let i = r.live.iter().position(|a| a.handle == handle)?;
        let a = &mut r.live[i];
        a.taken += 1;
        let bits = a.bits;
        if a.expected.is_some_and(|n| a.taken >= n) {
            r.live.swap_remove(i);
        }
        Some(bits)
    })
}

/// Reject `promise_ptr` for an abort with reason handle `handle`.
pub(crate) fn reject_aborted(promise_ptr: usize, handle: usize) {
    match take_abort_reason(handle) {
        Some(bits) => queue_promise_resolution(promise_ptr, false, bits),
        None => queue_deferred_resolution(promise_ptr, false, abort_error_bits),
    }
}

/// The error a body stream of a request aborted with reason handle
/// `handle` errors with (main thread: an `AbortError` is built here).
pub(crate) fn aborted_bits(handle: usize) -> u64 {
    take_abort_reason(handle).unwrap_or_else(abort_error_bits)
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
    /// should reject immediately, with the signal's reason, without
    /// dispatching.
    AlreadyAborted(u64),
    /// A live signal the request is bound to.
    Watch(FetchAbortWatch),
}

/// Consume the runtime's pending fetch signal. An already-aborted signal
/// short-circuits to a reject. MUST run on the main thread (it reads the signal
/// object).
pub(crate) fn take_pending_signal_watch() -> SignalState {
    let signal_value = unsafe { js_fetch_take_pending_signal() };
    let signal = perry_runtime::event_target_hook_slots::js_abort_signal_resolve_ptr(signal_value);
    if signal.is_null() {
        return SignalState::None;
    }
    if perry_runtime::event_target_hook_slots::js_abort_signal_is_aborted(signal) != 0 {
        let reason = perry_runtime::event_target_hook_slots::js_abort_signal_reason(signal);
        return SignalState::AlreadyAborted(reason.to_bits());
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
        // Queued as it is: the resolution queue roots the reason.
        SignalState::AlreadyAborted(bits) if bits != super::TAG_UNDEFINED => {
            queue_promise_resolution(promise_ptr, false, bits);
            None
        }
        SignalState::AlreadyAborted(_) => {
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
    perry_runtime::event_target_hook_slots::js_abort_error_value().to_bits()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rooted() -> Vec<u64> {
        let mut seen = Vec::new();
        super::super::gc::scan_fetch_roots(&mut |v| seen.push(v.to_bits()));
        seen
    }

    /// A fetch an abort cancelled rejects with the signal's reason (node),
    /// not a fresh AbortError: each cancelled request takes the reason once,
    /// a request that settles inside the abort's own drain included, and it
    /// stays a GC root until the last one has; a cancellation posted from
    /// another thread (handle 0) has no reason here.
    #[test]
    fn abort_reason_is_each_cancelled_requests_rejection() {
        let reason = 1.5f64.to_bits();
        let h = begin_abort(reason);
        // One request settles while the abort drains, before the count is known.
        assert_eq!(take_abort_reason(h), Some(reason));
        end_abort(h, 2);
        assert!(
            rooted().contains(&reason),
            "the reason must stay rooted while a request is pending"
        );
        assert_eq!(take_abort_reason(h), Some(reason));
        assert!(
            !rooted().contains(&reason),
            "the reason must be released once every request settled"
        );
        assert_eq!(take_abort_reason(h), None);
        assert_eq!(take_abort_reason(0), None);

        // An abort that cancelled nothing here keeps nothing.
        let other = 2.5f64.to_bits();
        let h2 = begin_abort(other);
        end_abort(h2, 0);
        assert!(!rooted().contains(&other));
        assert_eq!(take_abort_reason(h2), None);
    }
}
