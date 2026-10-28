//! Publish an already determined await through the ordinary scanned FIFO.
//!
//! Snapshot cloning, activation retains, queue growth and enqueue preparation
//! allocate native Vec/Arc storage only. They neither enter JS nor collect.
//! The brand probe is Leaf. No raw heap address crosses a collection point.
//! Keep precise root insertion barriers even without transient handle slots:
//! the incremental root walk may have passed the queue before this publication.
use super::*;

#[inline]
fn owner_bits<T>(owner: *const T) -> u64 {
    if owner.is_null() {
        crate::value::TAG_UNDEFINED
    } else {
        crate::value::JSValue::pointer(owner.cast()).bits()
    }
}

/// The result promise belongs to the currently running step. An initial or
/// different activation cannot borrow it, even when the code pointer matches.
#[inline]
pub(super) fn try_enqueue(value: f64, step: ClosurePtr) -> Option<*mut Promise> {
    let trap = INLINE_TRAP.with(|slot| slot.get());
    if trap.trap_next.is_null() || trap.current_step != step as usize {
        return None;
    }
    // Per-await async_hooks resource initialization can enter JS and collect.
    // Ordinary V8 promise before/after hooks remain at FIFO dispatch, unchanged.
    if crate::async_hook_slots::promise_hooks_active() {
        return None;
    }
    let payload = if is_definitely_primitive(value) {
        bump(&MT_FAST_PATH_HIT);
        value
    } else {
        if js_value_is_promise(value) == 0 {
            return None;
        }
        let inner = crate::value::js_nanbox_get_pointer(value) as *const Promise;
        unsafe {
            // A null metadata owner proves the existing recorded-prototype
            // probe returns None. No property lookup or getter is skipped.
            if (*inner).state != PromiseState::Fulfilled || !(*inner).meta.is_null() {
                return None;
            }
            (*inner).value
        }
    };
    bump(&MT_STEP_CHAIN_REUSE_HIT);
    Some(enqueue(
        step,
        payload,
        trap.trap_next,
        false,
        trap.box_activation,
    ))
}

/// Called only after the no-async-hook effect check. Native context/token
/// operations cannot move any of these values. The existing queue owns every
/// published edge before the producer returns; no interior pointer is retained.
// Both admission routes share the same barrier/context/queue operation.
// Keep one body rather than duplicating its TLS and publication scaffold.
#[inline(never)]
pub(super) fn enqueue(
    step: ClosurePtr,
    value: f64,
    next: *mut Promise,
    is_error: bool,
    activation: *mut crate::r#box::AsyncBoxActivation,
) -> *mut Promise {
    let context = capture_context();
    let (async_id, trigger_id) = if next.is_null() {
        (0, 0)
    } else {
        unsafe { ((*next).async_id, (*next).trigger_async_id) }
    };
    crate::gc::runtime_write_barrier_root_nanbox(owner_bits(step));
    crate::gc::runtime_write_barrier_root_nanbox(value.to_bits());
    crate::gc::runtime_write_barrier_root_nanbox(owner_bits(next));
    crate::r#box::retain_async_box_activation(activation);
    // GC_STORE_AUDIT(ROOT): existing Task scanner owns step/value/next;
    // precise insertion barriers precede the native-only queue publication.
    TASK_QUEUE.with(|queue| {
        queue.borrow_mut().enqueue_promise_job(Task::AsyncStep(
            step, value, next, is_error, context, activation, async_id, trigger_id,
        ));
    });
    next
}

#[cfg(test)]
#[path = "settled_step_tests.rs"]
mod tests;
