//! The Await reaction (ECMA-262 Await, steps 3-9).
//!
//! `await v` on a pending promise performs `PerformPromiseThen(promise,
//! onFulfilled, onRejected)` where both handlers are built-in closures over the
//! suspended async activation. No program code can reach those two functions,
//! so the reaction does not need them: the awaited promise's reaction slot
//! names the activation itself.
//!
//! - `on_fulfilled` and `on_rejected` both hold the activation's step closure
//!   (`__step(value, is_error)`),
//! - `next` holds the activation's result promise (the async function's
//!   promise; the step settles it, so the reaction has no capability of its
//!   own),
//! - the pending promise's fulfillment word, unused while pending, holds the
//!   activation token (`AsyncBoxActivation`) as a plain number. The token
//!   lives as long as the activation is suspended: its lifecycle reference is
//!   released only when the step finishes, and a suspended step is not
//!   running.
//!
//! Settlement turns the reaction into the job a settled await already
//! enqueues, `Task::AsyncStep(step, value, result, is_error)`, in the slot
//! reaction's place in the queue. No closure is born and none is called.
//! While promise hooks observe promises (`async_hooks` promise hooks or
//! `v8.promiseHooks`), or when the slot is taken, the await keeps the
//! closure-pair reaction, whose hook events those observers already see.

use super::then::{
    store_promise_closure_slot, store_promise_jsvalue_slot, store_promise_next_slot,
};
use super::{
    clear_promise_context, set_promise_callback_context, ClosurePtr, Promise, PromiseState, Task,
};
use crate::r#box::AsyncBoxActivation;

/// Token word of a reaction with no activation token. Tokens are 8-byte
/// aligned addresses, so bit 0 marks the word as a token and keeps it
/// non-zero.
const TOKEN_BIT: u64 = 1;

#[inline]
fn token_word(token: *mut AsyncBoxActivation) -> f64 {
    ((token as u64) | TOKEN_BIT) as f64
}

#[inline]
fn word_token(word: f64) -> *mut AsyncBoxActivation {
    ((word as u64) & !TOKEN_BIT) as *mut AsyncBoxActivation
}

#[inline]
fn pointer_bits<T>(pointer: *const T) -> u64 {
    if pointer.is_null() {
        crate::value::TAG_UNDEFINED
    } else {
        crate::value::JSValue::pointer(pointer.cast()).bits()
    }
}

/// The awaited promise can carry the activation in its reaction slot.
#[inline]
pub(super) fn can_attach(promise: *mut Promise) -> bool {
    if promise.is_null()
        || crate::async_hook_slots::promise_hooks_active()
        || crate::v8::promise_hooks_active()
    {
        return false;
    }
    unsafe {
        (*promise).state == PromiseState::Pending
            && (*promise).on_fulfilled.is_null()
            && (*promise).on_rejected.is_null()
            && (*promise).next.is_null()
            && (*promise).value.to_bits() == 0
    }
}

/// Suspend the activation `step` on `awaited` (which `can_attach` accepted).
/// Returns the activation's result promise: `trap_next` on a later await, a
/// fresh promise on the activation's first await (entered through
/// `js_async_first_call`, which leaves `trap_next` null).
pub(super) fn suspend(
    step: ClosurePtr,
    trap_next: *mut Promise,
    token: *mut AsyncBoxActivation,
    awaited: *mut Promise,
) -> *mut Promise {
    let scope = crate::gc::RuntimeHandleScope::new();
    let step_h = scope.root_raw_const_ptr(step);
    let awaited_h = scope.root_raw_mut_ptr(awaited);
    // `js_promise_new_with_parent` roots its parent across the allocation.
    let result = if trap_next.is_null() {
        awaited_h.with_mut_ptr::<Promise, _>(super::then::js_promise_new_with_parent)
    } else {
        trap_next
    };
    let result_h = scope.root_raw_mut_ptr(result);
    // Both can allocate (the operation-owner record); `across_mut` re-reads
    // the awaited promise after them.
    let ((), awaited) = awaited_h.across_mut::<Promise, _>(|| {
        awaited_h.with_mut_ptr::<Promise, _>(super::mark_rejection_handled);
        awaited_h.with_mut_ptr::<Promise, _>(super::operation_owner::observe);
    });
    step_h.with_const_ptr::<crate::closure::ClosureHeader, _>(|step| {
        result_h.with_mut_ptr::<Promise, _>(|result| unsafe {
            store_promise_closure_slot(
                awaited,
                std::ptr::addr_of_mut!((*awaited).on_fulfilled),
                step,
            );
            store_promise_closure_slot(
                awaited,
                std::ptr::addr_of_mut!((*awaited).on_rejected),
                step,
            );
            store_promise_next_slot(awaited, std::ptr::addr_of_mut!((*awaited).next), result);
            store_promise_jsvalue_slot(
                awaited,
                std::ptr::addr_of_mut!((*awaited).value),
                token_word(token),
            );
        })
    });
    // The context snapshot can allocate; re-read the result after it.
    let ((), result) = result_h.across_mut::<Promise, _>(|| {
        awaited_h.with_mut_ptr::<Promise, _>(set_promise_callback_context);
    });
    result
}

/// Called by settlement while `promise` is still pending, before the result
/// word is written: take the activation token of an Await reaction in the
/// slot, clearing the word. `None` when the slot holds no Await reaction.
#[inline]
pub(super) fn take_token(promise: *mut Promise) -> Option<*mut AsyncBoxActivation> {
    unsafe {
        let word = (*promise).value;
        if word.to_bits() == 0
            || (*promise).on_fulfilled.is_null()
            || (*promise).on_fulfilled != (*promise).on_rejected
            || (*promise).next.is_null()
        {
            return None;
        }
        store_promise_jsvalue_slot(promise, std::ptr::addr_of_mut!((*promise).value), 0.0);
        Some(word_token(word))
    }
}

/// Queue the activation's resumption for a settled `promise` whose slot held
/// an Await reaction, and empty the slot. `value` is the fulfillment value or
/// the rejection reason.
pub(super) fn enqueue_resume(
    queue: &mut std::collections::VecDeque<Task>,
    promise: *mut Promise,
    value: f64,
    fulfilled: bool,
    context: crate::async_context::AsyncContextSnapshot,
    token: *mut AsyncBoxActivation,
) {
    use super::job_queue::EnqueuePromiseJob;
    unsafe {
        let step = (*promise).on_fulfilled;
        let next = (*promise).next;
        // The queue is a scanned root; the slot stops naming step/next below.
        crate::gc::runtime_write_barrier_root_nanbox(pointer_bits(step));
        crate::gc::runtime_write_barrier_root_nanbox(value.to_bits());
        crate::gc::runtime_write_barrier_root_nanbox(pointer_bits(next));
        // The queued job owns one token reference until its dispatch releases it.
        crate::r#box::retain_async_box_activation(token);
        queue.enqueue_promise_job(Task::AsyncStep(
            step, value, next, !fulfilled, context, token, 0, 0,
        ));
        store_promise_closure_slot(
            promise,
            std::ptr::addr_of_mut!((*promise).on_fulfilled),
            std::ptr::null(),
        );
        store_promise_closure_slot(
            promise,
            std::ptr::addr_of_mut!((*promise).on_rejected),
            std::ptr::null(),
        );
        store_promise_next_slot(
            promise,
            std::ptr::addr_of_mut!((*promise).next),
            std::ptr::null_mut(),
        );
    }
    clear_promise_context(promise);
}
