//! Producer completion runs inside settlement, after queuing public reactions.
use super::{
    mark_rejection_handled, then::store_promise_jsvalue_slot, ClosurePtr, Promise, PromiseState,
};

/// Install the single completion of an internal pending step. Unlike an
/// Await reaction, completion belongs to the producer's settlement operation.
/// The pending state's unused reason word holds it; no Promise grows and no
/// side table or reaction job is needed. Public reactions retain their slots.
pub(crate) fn attach_step_completion(promise: *mut Promise, completion: ClosurePtr) {
    mark_rejection_handled(promise);
    unsafe {
        assert_eq!((*promise).state, PromiseState::Pending);
        assert_eq!((*promise).reason.to_bits(), 0);
        store_promise_jsvalue_slot(
            promise,
            std::ptr::addr_of_mut!((*promise).reason),
            crate::value::js_nanbox_pointer(completion as i64),
        );
    }
}

/// After reactions have been enqueued, run the producer's completion in this
/// same job. Settlement transfers the pending completion to the inactive
/// result word (reason on fulfillment, value on rejection). Both words are
/// already traced and rewritten JSValue slots.
#[inline]
pub(super) unsafe fn complete_step(promise: *mut Promise, fulfilled: bool) {
    let completion = if fulfilled {
        (*promise).reason
    } else {
        (*promise).value
    };
    if completion.to_bits() != 0 {
        run_completion(promise, fulfilled);
    }
}

// Keep the rooted invocation out of ordinary settlements' register-save
// prologue. This is the same completion operation, entered only when present.
#[inline(never)]
unsafe fn run_completion(promise: *mut Promise, fulfilled: bool) {
    let slot = if fulfilled {
        std::ptr::addr_of_mut!((*promise).reason)
    } else {
        std::ptr::addr_of_mut!((*promise).value)
    };
    let scope = crate::gc::RuntimeHandleScope::new();
    let completion = scope.root_nanbox_f64(*slot);
    let value = scope.root_nanbox_f64(if fulfilled {
        (*promise).value
    } else {
        (*promise).reason
    });
    store_promise_jsvalue_slot(promise, slot, 0.0);
    crate::closure::js_closure_call2(
        crate::value::js_nanbox_get_pointer(completion.get_nanbox_f64()) as ClosurePtr,
        crate::closure::plain_call_receiver(),
        value.get_nanbox_f64(),
        f64::from_bits(if fulfilled {
            crate::value::TAG_TRUE
        } else {
            crate::value::TAG_FALSE
        }),
    );
}

#[cfg(test)]
#[path = "step_completion_tests.rs"]
mod tests;
