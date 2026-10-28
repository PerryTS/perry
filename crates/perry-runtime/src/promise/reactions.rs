//! Promise-owned reaction lists: settle listeners (internal
//! "run-this-when-settled" callbacks attached by the async machinery) and
//! overflow reactions (2nd+ `.then`/`.catch`/`.finally` reactions on the same
//! promise — the `Promise` struct holds only ONE inline reaction triple).
//! Extracted from `then.rs` to keep that file under the 2000-line cap;
//! Each private GC list is reachable only through its owning Promise.

use super::*;

pub(crate) struct PromiseSettleListener {
    pub(crate) on_fulfilled: ClosurePtr,
    pub(crate) on_rejected: ClosurePtr,
    pub(crate) context: AsyncContextSnapshot,
}

pub(crate) fn js_promise_attach_settle_listener(
    promise: *mut Promise,
    on_fulfilled: ClosurePtr,
    on_rejected: ClosurePtr,
) {
    if promise.is_null() {
        return;
    }
    mark_rejection_handled(promise);

    let context = capture_context();
    unsafe {
        match (*promise).state {
            PromiseState::Pending => {
                park_settle_listener(promise, on_fulfilled, on_rejected, context);
            }
            PromiseState::Fulfilled => {
                enqueue_settle_listener_task(on_fulfilled, (*promise).value, true, context);
            }
            PromiseState::Rejected => {
                enqueue_settle_listener_task(on_rejected, (*promise).reason, false, context);
            }
        }
    }
}

pub(crate) fn promise_take_settle_listeners(promise: *mut Promise) -> Vec<PromiseSettleListener> {
    if promise.is_null() {
        return Vec::new();
    }
    unsafe {
        let list = owned_state::take_list(promise, owned_state::LISTENERS);
        if list.is_null() {
            return Vec::new();
        }
        (0..(*list).length / 3)
            .map(|i| PromiseSettleListener {
                on_fulfilled: (crate::array::js_array_get(list, i * 3).bits()
                    & crate::value::POINTER_MASK) as ClosurePtr,
                on_rejected: (crate::array::js_array_get(list, i * 3 + 1).bits()
                    & crate::value::POINTER_MASK) as ClosurePtr,
                context: owned_state::context_at(list, i * 3 + 2),
            })
            .collect()
    }
}

fn enqueue_settle_listener_task(
    callback: ClosurePtr,
    value: f64,
    is_fulfilled: bool,
    context: AsyncContextSnapshot,
) {
    if callback.is_null() {
        return;
    }
    TASK_QUEUE.with(|q| {
        q.borrow_mut().enqueue_promise_job(Task::Inline(
            callback,
            value,
            ptr::null_mut(),
            is_fulfilled,
            context,
        ));
    });
}

// ---------------------------------------------------------------------------
// Multiple reactions per promise (PerformPromiseThen's [[PromiseFulfillReactions]]
// / [[PromiseRejectReactions]] lists).
//
// The `Promise` struct holds ONE `on_fulfilled`/`on_rejected`/`next` triple, so
// the FIRST `.then`/`.catch`/`.finally` reaction uses those inline slots (the
// common, hot, zero-overhead case). A SECOND+ reaction on the same promise —
// `p.then(a); p.then(b)`, or a user `.then` plus a combinator's per-element
// `.then` when `Promise.resolve(p) === p` — would clobber the slot. Those
// overflow reactions live in an owned GC list and are replayed in
// FIFO registration order (after the slot reaction) when the promise settles.
//
// Each overflow reaction carries its OWN chained `next` promise and async
// context, so the chained promise settles and runs in the correct realm —
// dispatched via `Task::Inline`, which already models "invoke one handler (or
// pass the value through when null) and resolve `next` with the result".
// ---------------------------------------------------------------------------

pub(crate) struct OverflowReaction {
    pub(crate) on_fulfilled: ClosurePtr,
    pub(crate) on_rejected: ClosurePtr,
    pub(crate) next: *mut Promise,
    pub(crate) context: AsyncContextSnapshot,
}

/// Park a 2nd+ reaction on a still-pending `promise`.
pub(crate) fn push_overflow_reaction(
    promise: *mut Promise,
    on_fulfilled: ClosurePtr,
    on_rejected: ClosurePtr,
    next: *mut Promise,
    context: AsyncContextSnapshot,
) {
    let _no_move = crate::gc::GcSuppressScope::new();
    let context = owned_state::context_word(context);
    unsafe {
        owned_state::push_record(
            promise,
            owned_state::OVERFLOW,
            &[
                owned_state::pointer_word(on_fulfilled),
                owned_state::pointer_word(on_rejected),
                owned_state::pointer_word(next),
                context,
            ],
        );
    }
}

/// Drain (in registration order) every overflow reaction registered against
/// `promise`. Returns `Vec::new()` for the overwhelmingly common no-overflow
/// case without allocating a list.
pub(crate) fn promise_take_overflow_reactions(promise: *mut Promise) -> Vec<OverflowReaction> {
    if promise.is_null() {
        return Vec::new();
    }
    unsafe {
        let list = owned_state::take_list(promise, owned_state::OVERFLOW);
        if list.is_null() {
            return Vec::new();
        }
        (0..(*list).length / 4)
            .map(|i| OverflowReaction {
                on_fulfilled: (crate::array::js_array_get(list, i * 4).bits()
                    & crate::value::POINTER_MASK) as ClosurePtr,
                on_rejected: (crate::array::js_array_get(list, i * 4 + 1).bits()
                    & crate::value::POINTER_MASK) as ClosurePtr,
                next: (crate::array::js_array_get(list, i * 4 + 2).bits()
                    & crate::value::POINTER_MASK) as *mut Promise,
                context: owned_state::context_at(list, i * 4 + 3),
            })
            .collect()
    }
}

/// Push the `Task::Inline` jobs for a settled promise's drained overflow
/// reactions. `value` is the fulfilled value or rejection reason.
pub(super) fn enqueue_overflow_reactions(
    reactions: Vec<OverflowReaction>,
    value: f64,
    is_fulfilled: bool,
    q: &mut std::collections::VecDeque<Task>,
) {
    for r in reactions {
        let cb = if is_fulfilled {
            r.on_fulfilled
        } else {
            r.on_rejected
        };
        // A null `cb` with a non-null `next` is a pass-through (the
        // `Task::Inline` arm resolves/rejects `next` with `value`) — exactly the
        // `.then(onFulfilled)` rejected-side / `.catch` fulfilled-side behavior.
        q.enqueue_promise_job(Task::Inline(cb, value, r.next, is_fulfilled, r.context));
    }
}

pub(crate) fn park_settle_listener(
    promise: *mut Promise,
    on_fulfilled: ClosurePtr,
    on_rejected: ClosurePtr,
    context: AsyncContextSnapshot,
) {
    let _no_move = crate::gc::GcSuppressScope::new();
    let context = owned_state::context_word(context);
    unsafe {
        owned_state::push_record(
            promise,
            owned_state::LISTENERS,
            &[
                owned_state::pointer_word(on_fulfilled),
                owned_state::pointer_word(on_rejected),
                context,
            ],
        );
    }
}
