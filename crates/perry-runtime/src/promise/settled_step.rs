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
            if (*inner).state != PromiseState::Fulfilled || !crate::object::cell_meta::promise_cell_meta(inner).is_null() {
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

/// Settled self-next continuation (SNC). The running job J is this
/// activation's own step dispatch and the await it reaches is already
/// determined, so the FIFO round trip (publish T, return to J's tail, pop T,
/// dispatch T) is unobservable when all six conditions hold:
///  1. J is the activation's own AsyncStep job (`CURRENT_MICROTASK_CALLBACK`
///     is this step; a thunk/reaction job or a re-entrant drain declines);
///  2. the result promise is reusable (`INLINE_TRAP` names this step);
///  3. no async_hooks / V8 promise hooks (elided before/after/init unseen);
///  4. the awaited value is a primitive or a native fulfilled promise with
///     null metadata (the `try_enqueue` admission);
///  5. the microtask queue is empty, so T would be the next job popped;
///  6. the job domain is not cancelled (eligible by construction).
/// On admission the awaited result is returned and the step loops in place
/// with it as the next resume value. Otherwise nothing changed, `TAG_TDZ` (a
/// value no await can produce) is returned, and the caller takes the ordinary
/// FIFO path. Never restarts. Leaf: no JS, no allocation, no collection.
#[no_mangle]
pub extern "C" fn js_async_step_resume_now(value: f64, step: ClosurePtr) -> f64 {
    resume_now(value, step).unwrap_or_else(|| f64::from_bits(crate::value::TAG_TDZ))
}

#[inline]
fn resume_now(value: f64, step: ClosurePtr) -> Option<f64> {
    if step.is_null() {
        return None;
    }
    // P1
    if super::microtasks::CURRENT_MICROTASK_CALLBACK.with(|c| c.get()) != step {
        return None;
    }
    // P2
    let trap = INLINE_TRAP.with(|slot| slot.get());
    if trap.trap_next.is_null() || trap.current_step != step as usize {
        return None;
    }
    // P3
    if crate::async_hook_slots::promise_hooks_active() || crate::v8::promise_hooks_active() {
        return None;
    }
    // P4
    let primitive = is_definitely_primitive(value);
    let payload = if primitive {
        value
    } else {
        if js_value_is_promise(value) == 0 {
            return None;
        }
        let inner = crate::value::js_nanbox_get_pointer(value) as *const Promise;
        unsafe {
            if (*inner).state != PromiseState::Fulfilled || !crate::object::cell_meta::promise_cell_meta(inner).is_null() {
                return None;
            }
            (*inner).value
        }
    };
    // P5
    if TASK_QUEUE.with(|queue| !queue.borrow().is_empty()) {
        return None;
    }
    // P6 (last: the only predicate with effects, and only when admitted)
    if !crate::async_context::job_domain::admit_inline_self_next_job() {
        return None;
    }
    bump(&MT_RUN_COUNT);
    if primitive {
        bump(&MT_FAST_PATH_HIT);
    }
    bump(&MT_STEP_CHAIN_REUSE_HIT);
    crate::closure::reset_throw_not_callable_counter();
    Some(payload)
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
    publish(
        step, value, next, is_error, context, activation, async_id, trigger_id,
    );
    notify_published(next)
}

/// No raw address crosses a collection point before queue publication.
/// Once published, a host wake may re-enter; protect and reload the result.
fn notify_published(next: *mut Promise) -> *mut Promise {
    if super::microtasks::microtask_job_drain_active() {
        crate::event_pump::js_notify_promise_progress();
        return next;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let result = scope.root_raw_mut_ptr(next);
    crate::event_pump::js_notify_promise_progress();
    result.get_raw_mut_ptr()
}

/// The observed and unobserved routes publish the identical scanned Task.
#[inline(never)]
#[allow(clippy::too_many_arguments)]
pub(super) fn publish(
    step: ClosurePtr,
    value: f64,
    next: *mut Promise,
    is_error: bool,
    context: crate::async_context::AsyncContextSnapshot,
    activation: *mut crate::r#box::AsyncBoxActivation,
    async_id: u64,
    trigger_id: u64,
) {
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
}

#[cfg(test)]
#[path = "settled_step_tests.rs"]
mod tests;
