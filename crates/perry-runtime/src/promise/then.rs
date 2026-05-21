//! Promise allocation, settlement (resolve/reject), and chaining
//! (`then`/`catch`/`finally`). See `super` for the shared task queue
//! and Promise type.

use super::*;
/// Allocate a new Promise
#[no_mangle]
pub extern "C" fn js_promise_new() -> *mut Promise {
    bump(&MT_PROMISE_NEW_COUNT);
    let raw = crate::gc::gc_malloc(std::mem::size_of::<Promise>(), crate::gc::GC_TYPE_PROMISE);
    let promise = raw as *mut Promise;
    unsafe {
        ptr::write(promise, Promise::new());
        if crate::async_hooks::hooks_active() {
            let resource =
                f64::from_bits(0x7FFD_0000_0000_0000 | (promise as u64 & 0x0000_FFFF_FFFF_FFFF));
            let ids = crate::async_hooks::init_resource("PROMISE", resource, false);
            (*promise).async_id = ids.async_id;
            (*promise).trigger_async_id = ids.trigger_async_id;
        }
    }
    promise
}

/// Free a Promise (no-op — GC handles deallocation)
#[no_mangle]
pub extern "C" fn js_promise_free(_promise: *mut Promise) {
    // GC handles deallocation now
}

/// Get promise state (0=pending, 1=fulfilled, 2=rejected)
#[no_mangle]
pub extern "C" fn js_promise_state(promise: *mut Promise) -> i32 {
    if promise.is_null() {
        return -1;
    }
    unsafe { (*promise).state as i32 }
}

/// Get promise value (if fulfilled)
#[no_mangle]
pub extern "C" fn js_promise_value(promise: *mut Promise) -> f64 {
    if promise.is_null() {
        return 0.0;
    }

    unsafe { (*promise).value }
}

/// Get promise reason (if rejected)
#[no_mangle]
pub extern "C" fn js_promise_reason(promise: *mut Promise) -> f64 {
    if promise.is_null() {
        return 0.0;
    }
    unsafe { (*promise).reason }
}

/// Get promise result (value if fulfilled, reason if rejected)
/// This is what await should use to get the result of a promise.
/// For fulfilled promises, returns the resolved value.
/// For rejected promises, returns the rejection reason.
/// For pending promises (should not happen in normal use), returns 0.0.
#[no_mangle]
pub extern "C" fn js_promise_result(promise: *mut Promise) -> f64 {
    if promise.is_null() {
        return 0.0;
    }
    unsafe {
        match (*promise).state {
            PromiseState::Fulfilled => (*promise).value,
            PromiseState::Rejected => (*promise).reason,
            PromiseState::Pending => 0.0,
        }
    }
}

/// Resolve a promise with a value
#[no_mangle]
pub extern "C" fn js_promise_resolve(promise: *mut Promise, value: f64) {
    if promise.is_null() {
        return;
    }
    unsafe {
        if (*promise).state != PromiseState::Pending {
            return; // Already settled
        }
        (*promise).state = PromiseState::Fulfilled;
        (*promise).value = value;
        crate::async_hooks::promise_resolve((*promise).async_id);

        // Schedule callbacks. Push to TASK_QUEUE whenever there's anything
        // for the microtask runner to do — either invoke the user callback,
        // or propagate the value to the chained `next` promise. Issue #236:
        // pre-fix the queue push only fired when `on_fulfilled` was non-null,
        // so `.then(console.log)` (where `console.log`-as-value lowers to
        // a NULL ClosurePtr sentinel — see expr.rs:GlobalGet→PropertyGet
        // value path) skipped the queue entirely; the chained promise then
        // never settled and `await chained` busy-waited forever.
        let promise_all_states = combinators::promise_all_take_all_handlers(promise);
        let has_normal_handler = !(*promise).on_fulfilled.is_null() || !(*promise).next.is_null();
        if !promise_all_states.is_empty() || has_normal_handler {
            let task_context = context_for_promise(promise);
            TASK_QUEUE.with(|q| {
                let mut q = q.borrow_mut();
                for all_state in promise_all_states {
                    q.push_back(Task::PromiseAll(
                        all_state,
                        value,
                        true,
                        task_context.clone(),
                    ));
                }
                if has_normal_handler {
                    q.push_back(Task::Promise(promise, value, true, task_context));
                } else {
                    clear_promise_context(promise);
                }
            });
        }
    }
    // Issue #84: an `await` busy-wait that called `js_timer_tick` (or any
    // tick fn) which then resolved this promise needs to skip the
    // following `js_wait_for_event` sleep — otherwise it blocks for the
    // 1 s idle cap before the loop re-checks promise state. The notify
    // sets the flag so the immediately-following wait returns at once.
    crate::event_pump::js_notify_main_thread();
    unsafe {
        crate::async_hooks::destroy((*promise).async_id);
    }
}

/// Resolve a promise with another promise (Promise chaining/unwrapping)
/// When the inner promise resolves, the outer promise adopts its value
#[no_mangle]
pub extern "C" fn js_promise_resolve_with_promise(outer: *mut Promise, inner: *mut Promise) {
    if outer.is_null() || inner.is_null() {
        return;
    }

    unsafe {
        if (*outer).state != PromiseState::Pending {
            return; // Already settled
        }

        // Check inner promise state
        match (*inner).state {
            PromiseState::Fulfilled => {
                // Inner already resolved - resolve outer with inner's value
                js_promise_resolve(outer, (*inner).value);
            }
            PromiseState::Rejected => {
                // Inner already rejected - reject outer with inner's reason
                js_promise_reject(outer, (*inner).reason);
            }
            PromiseState::Pending => {
                // Inner is pending.
                //
                // Perf fast path: if inner has no callbacks AND no
                // chained `next` already, we can simply chain outer
                // as inner's next. When inner settles, the microtask
                // runner's "callback null but next non-null" arm at
                // `js_promise_run_microtasks` will propagate the
                // value/reason to outer directly — same observable
                // semantics as forward_resolve/forward_reject but
                // skips two closure allocations AND a microtask hop.
                //
                // This is the steady-state shape inside the async-
                // step driver: each await's `step()` returns a fresh
                // promise from `Promise.resolve(v).then(...)` whose
                // `next` is null and whose callbacks were just set
                // on the inner source — the returned outer wrapper
                // itself is callback-less. Eliminating that hop is
                // the largest single win in the per-await steady
                // state.
                if (*inner).on_fulfilled.is_null()
                    && (*inner).on_rejected.is_null()
                    && (*inner).next.is_null()
                {
                    (*inner).next = outer;
                    return;
                }

                // Slow path: inner already has callbacks or a chained
                // `next`. Fall back to the forwarding-closure shape
                // so we don't clobber existing wiring.
                let outer_i64 = outer as i64;

                // Create a resolve forwarding closure
                let resolve_closure =
                    crate::closure::js_closure_alloc(promise_forward_resolve as *const u8, 1);
                crate::closure::js_closure_set_capture_ptr(resolve_closure, 0, outer_i64);

                // Create a reject forwarding closure
                let reject_closure =
                    crate::closure::js_closure_alloc(promise_forward_reject as *const u8, 1);
                crate::closure::js_closure_set_capture_ptr(reject_closure, 0, outer_i64);

                // Register the forwarding callbacks on the inner promise
                (*inner).on_fulfilled = resolve_closure;
                (*inner).on_rejected = reject_closure;
                (*inner).next = ptr::null_mut(); // Don't chain, we handle resolution ourselves
            }
        }
    }
}

/// Internal callback for forwarding resolve from inner to outer promise
extern "C" fn promise_forward_resolve(
    closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    let outer_ptr = crate::closure::js_closure_get_capture_ptr(closure, 0) as *mut Promise;
    js_promise_resolve(outer_ptr, value);
    0.0
}

/// Internal callback for forwarding reject from inner to outer promise
extern "C" fn promise_forward_reject(
    closure: *const crate::closure::ClosureHeader,
    reason: f64,
) -> f64 {
    let outer_ptr = crate::closure::js_closure_get_capture_ptr(closure, 0) as *mut Promise;
    js_promise_reject(outer_ptr, reason);
    0.0
}

/// Reject a promise with a reason
#[no_mangle]
pub extern "C" fn js_promise_reject(promise: *mut Promise, reason: f64) {
    if promise.is_null() {
        return;
    }
    unsafe {
        if (*promise).state != PromiseState::Pending {
            return; // Already settled
        }
        (*promise).state = PromiseState::Rejected;
        (*promise).reason = reason;
        crate::async_hooks::promise_resolve((*promise).async_id);

        // Schedule callbacks. Same propagation rule as `js_promise_resolve`
        // (#236): push to the queue whenever there's a callback to invoke
        // OR a chained `next` promise to forward to.
        let promise_all_states = combinators::promise_all_take_all_handlers(promise);
        let has_normal_handler = !(*promise).on_rejected.is_null() || !(*promise).next.is_null();
        if !promise_all_states.is_empty() || has_normal_handler {
            let task_context = context_for_promise(promise);
            TASK_QUEUE.with(|q| {
                let mut q = q.borrow_mut();
                for all_state in promise_all_states {
                    q.push_back(Task::PromiseAll(
                        all_state,
                        reason,
                        false,
                        task_context.clone(),
                    ));
                }
                if has_normal_handler {
                    q.push_back(Task::Promise(promise, reason, false, task_context));
                } else {
                    clear_promise_context(promise);
                }
            });
        }
    }
    // Issue #84: see js_promise_resolve — same wake reasoning.
    crate::event_pump::js_notify_main_thread();
    unsafe {
        crate::async_hooks::destroy((*promise).async_id);
    }
}

/// Register fulfillment callback, returns a new promise for chaining
#[no_mangle]
pub extern "C" fn js_promise_then(
    promise: *mut Promise,
    on_fulfilled: ClosurePtr,
    on_rejected: ClosurePtr,
) -> *mut Promise {
    bump(&MT_PROMISE_THEN_COUNT);
    if promise.is_null() {
        return ptr::null_mut();
    }

    let next = js_promise_new();

    unsafe {
        (*promise).on_fulfilled = on_fulfilled;
        (*promise).on_rejected = on_rejected;
        (*promise).next = next;
        set_promise_callback_context(promise);

        // If already settled, schedule callback immediately. Same propagation
        // rule as `js_promise_resolve`/`js_promise_reject` (#236): push to the
        // queue when there's either a callback to invoke OR a chained `next`
        // promise to forward to. `next` is always non-null here (we just
        // created it), so this is effectively unconditional — the explicit
        // checks document the intent.
        match (*promise).state {
            PromiseState::Fulfilled => {
                if !on_fulfilled.is_null() || !next.is_null() {
                    TASK_QUEUE.with(|q| {
                        q.borrow_mut().push_back(Task::Promise(
                            promise,
                            (*promise).value,
                            true,
                            context_for_promise(promise),
                        ));
                    });
                }
            }
            PromiseState::Rejected => {
                if !on_rejected.is_null() || !next.is_null() {
                    TASK_QUEUE.with(|q| {
                        q.borrow_mut().push_back(Task::Promise(
                            promise,
                            (*promise).reason,
                            false,
                            context_for_promise(promise),
                        ));
                    });
                }
            }
            PromiseState::Pending => {}
        }
    }

    next
}

/// Like `js_promise_then` but skips the allocation of a chained `next`
/// promise. Used by callers that only need the side effect of running
/// the handler (Promise.all, Promise.race, internal forwarders), not
/// the chained promise. Saves one Promise alloc per call — material on
/// Promise.all of N inputs which today allocates N never-used `next`
/// promises.
pub(crate) fn js_promise_attach_handlers(
    promise: *mut Promise,
    on_fulfilled: ClosurePtr,
    on_rejected: ClosurePtr,
) {
    if promise.is_null() {
        return;
    }
    unsafe {
        (*promise).on_fulfilled = on_fulfilled;
        (*promise).on_rejected = on_rejected;
        set_promise_callback_context(promise);
        // No next — caller doesn't want a chained promise.

        match (*promise).state {
            PromiseState::Fulfilled => {
                if !on_fulfilled.is_null() {
                    TASK_QUEUE.with(|q| {
                        q.borrow_mut().push_back(Task::Promise(
                            promise,
                            (*promise).value,
                            true,
                            context_for_promise(promise),
                        ));
                    });
                }
            }
            PromiseState::Rejected => {
                if !on_rejected.is_null() {
                    TASK_QUEUE.with(|q| {
                        q.borrow_mut().push_back(Task::Promise(
                            promise,
                            (*promise).reason,
                            false,
                            context_for_promise(promise),
                        ));
                    });
                }
            }
            PromiseState::Pending => {}
        }
    }
}

/// Register rejection callback, returns a new promise for chaining
/// This is equivalent to .catch(onRejected) in JavaScript
#[no_mangle]
pub extern "C" fn js_promise_catch(promise: *mut Promise, on_rejected: ClosurePtr) -> *mut Promise {
    js_promise_then(promise, ptr::null(), on_rejected)
}

/// Register finally callback, returns a new promise for chaining.
/// This is equivalent to .finally(onFinally) in JavaScript.
///
/// Per spec, `.finally(cb)` must:
///   - Call `cb()` (ignoring its return value)
///   - Propagate the upstream fulfilled VALUE (not cb's return) to `next`
///   - Re-reject with the upstream rejection REASON if the upstream rejected
///
/// The spec (and Node.js) requires `.finally(cb)` to take ONE more microtask
/// tick than a plain `.then(cb)`.  We achieve this by setting `promise.next =
/// null` so the microtask runner does NOT resolve `next` after invoking the
/// wrapper callback — the wrappers resolve `next` themselves, via an extra
/// `js_promise_then(resolved_promise, passthrough)` hop that adds one queue
/// entry before `next` settles.
///
/// Capture layout for each wrapper: [on_finally, next_promise_ptr]
/// Capture layout for passthrough closures: [next_promise_ptr, value_or_reason]
#[no_mangle]
pub extern "C" fn js_promise_finally(
    promise: *mut Promise,
    on_finally: ClosurePtr,
) -> *mut Promise {
    use crate::closure::{js_closure_alloc, js_closure_set_capture_ptr};

    // Create the `next` promise that callers chain off.
    let next = js_promise_new();
    let next_i64 = next as i64;

    // Build the fulfilled wrapper: captures [on_finally, next].
    let fulfill_wrap = js_closure_alloc(finally_fulfill_wrapper as *const u8, 2);
    js_closure_set_capture_ptr(fulfill_wrap, 0, on_finally as i64);
    js_closure_set_capture_ptr(fulfill_wrap, 1, next_i64);

    // Build the rejected wrapper: captures [on_finally, next].
    let reject_wrap = js_closure_alloc(finally_reject_wrapper as *const u8, 2);
    js_closure_set_capture_ptr(reject_wrap, 0, on_finally as i64);
    js_closure_set_capture_ptr(reject_wrap, 1, next_i64);

    // Register wrappers on `promise`.  Crucially, set `promise.next = null`
    // so the microtask runner does NOT attempt to resolve `next` after calling
    // the wrapper — each wrapper handles `next` settlement itself via the
    // extra-tick passthrough pattern.
    unsafe {
        (*promise).on_fulfilled = fulfill_wrap;
        (*promise).on_rejected = reject_wrap;
        (*promise).next = ptr::null_mut(); // wrappers own next; runner must not touch it
        set_promise_callback_context(promise);

        // If the promise is already settled, push its task now.
        match (*promise).state {
            PromiseState::Fulfilled => {
                TASK_QUEUE.with(|q| {
                    q.borrow_mut().push_back(Task::Promise(
                        promise,
                        (*promise).value,
                        true,
                        context_for_promise(promise),
                    ));
                });
            }
            PromiseState::Rejected => {
                TASK_QUEUE.with(|q| {
                    q.borrow_mut().push_back(Task::Promise(
                        promise,
                        (*promise).reason,
                        false,
                        context_for_promise(promise),
                    ));
                });
            }
            PromiseState::Pending => {}
        }
    }

    next
}

/// Fulfilled-path wrapper for `.finally()`.
/// Captures [on_finally, next_promise].
/// Called with the upstream fulfilled `value`.
/// Runs `on_finally()`, then resolves `next` with `value` via ONE extra
/// microtask hop (matching Node.js `.finally()` microtask depth).
extern "C" fn finally_fulfill_wrapper(
    closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    use crate::closure::{
        js_closure_alloc, js_closure_get_capture_ptr, js_closure_set_capture_ptr,
    };

    let on_finally = js_closure_get_capture_ptr(closure, 0) as *const crate::closure::ClosureHeader;
    let next = js_closure_get_capture_ptr(closure, 1) as *mut Promise;

    // Call the user's finally callback (ignoring its return value).
    if !on_finally.is_null() {
        let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
        crate::closure::js_closure_call1(on_finally, undef);
    }

    // Add one extra microtask tick before settling `next` by registering a
    // passthrough closure on an already-resolved promise.  The runner will
    // enqueue it, call it next iteration, and THEN `next` gets resolved.
    if !next.is_null() {
        let pass = js_closure_alloc(finally_passthrough_fulfill as *const u8, 2);
        js_closure_set_capture_ptr(pass, 0, next as i64);
        crate::closure::js_closure_set_capture_f64(pass, 1, value);

        let undef_promise = js_promise_resolved(f64::from_bits(crate::value::TAG_UNDEFINED));
        // js_promise_then returns a new (discarded) promise; the side-effect
        // is enqueuing `pass` to run in the next microtask iteration.
        js_promise_then(undef_promise, pass, ptr::null());
    }

    // Return undefined.  Since promise.next is null (set in js_promise_finally),
    // the runner will not try to resolve anything with this return value.
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// Passthrough closure for the extra hop in finally_fulfill_wrapper.
/// Captures [next_promise_ptr (i64), value (f64)].
/// Resolves `next` with `value`.
extern "C" fn finally_passthrough_fulfill(
    closure: *const crate::closure::ClosureHeader,
    _: f64,
) -> f64 {
    use crate::closure::{js_closure_get_capture_f64, js_closure_get_capture_ptr};
    let next = js_closure_get_capture_ptr(closure, 0) as *mut Promise;
    let value = js_closure_get_capture_f64(closure, 1);
    if !next.is_null() {
        js_promise_resolve(next, value);
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// Rejected-path wrapper for `.finally()`.
/// Captures [on_finally, next_promise].
/// Called with the upstream rejection `reason`.
/// Runs `on_finally()`, then rejects `next` with `reason` via ONE extra
/// microtask hop.
extern "C" fn finally_reject_wrapper(
    closure: *const crate::closure::ClosureHeader,
    reason: f64,
) -> f64 {
    use crate::closure::{
        js_closure_alloc, js_closure_get_capture_ptr, js_closure_set_capture_ptr,
    };

    let on_finally = js_closure_get_capture_ptr(closure, 0) as *const crate::closure::ClosureHeader;
    let next = js_closure_get_capture_ptr(closure, 1) as *mut Promise;

    // Call the user's finally callback (ignoring its return value).
    if !on_finally.is_null() {
        let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
        crate::closure::js_closure_call1(on_finally, undef);
    }

    // Add one extra microtask tick before rejecting `next`.
    if !next.is_null() {
        let pass = js_closure_alloc(finally_passthrough_reject as *const u8, 2);
        js_closure_set_capture_ptr(pass, 0, next as i64);
        crate::closure::js_closure_set_capture_f64(pass, 1, reason);

        let undef_promise = js_promise_resolved(f64::from_bits(crate::value::TAG_UNDEFINED));
        js_promise_then(undef_promise, pass, ptr::null());
    }

    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// Passthrough closure for the extra hop in finally_reject_wrapper.
/// Captures [next_promise_ptr (i64), reason (f64)].
/// Rejects `next` with `reason`.
extern "C" fn finally_passthrough_reject(
    closure: *const crate::closure::ClosureHeader,
    _: f64,
) -> f64 {
    use crate::closure::{js_closure_get_capture_f64, js_closure_get_capture_ptr};
    let next = js_closure_get_capture_ptr(closure, 0) as *mut Promise;
    let reason = js_closure_get_capture_f64(closure, 1);
    if !next.is_null() {
        js_promise_reject(next, reason);
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}
