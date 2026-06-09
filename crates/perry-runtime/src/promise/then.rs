//! Promise allocation, settlement (resolve/reject), and chaining
//! (`then`/`catch`/`finally`). See `super` for the shared task queue
//! and Promise type.

use super::*;

#[inline]
unsafe fn store_promise_jsvalue_slot(promise: *mut Promise, slot: *mut f64, value: f64) {
    crate::gc::runtime_store_gc_jsvalue_slot(promise as usize, slot as usize, value.to_bits());
}

#[inline]
unsafe fn store_promise_closure_slot(
    promise: *mut Promise,
    slot: *mut ClosurePtr,
    value: ClosurePtr,
) {
    // GC_STORE_AUDIT(BARRIERED): Promise raw closure fields are GC heap words.
    crate::gc::runtime_store_gc_heap_word_slot(promise as usize, slot as usize, value as u64);
}

#[inline]
unsafe fn store_promise_next_slot(
    promise: *mut Promise,
    slot: *mut *mut Promise,
    value: *mut Promise,
) {
    // GC_STORE_AUDIT(BARRIERED): Promise raw next fields are GC heap words.
    crate::gc::runtime_store_gc_heap_word_slot(promise as usize, slot as usize, value as u64);
}

// ---------------------------------------------------------------------------
// Unhandled-rejection tracking (HostPromiseRejectionTracker, simplified).
//
// A promise that rejects with NO reaction attached at rejection time is
// "currently unhandled". If, by the time the program's event loop drains, it
// still has no handler, Node reports an unhandled rejection and exits non-zero
// (the v15+ `--unhandled-rejections=throw` default). test262 leans on this:
// `Promise.all` over a throwing iterator, a bare `Promise.reject(x)`, etc. all
// leave an unhandled rejection and the oracle exits non-zero.
//
// We track only the promise POINTER (no reason value — the harness judges on
// exit code, not the stderr text, and not storing a `f64` reason avoids rooting
// a heap value across the whole program). A handler attached LATER
// (`then`/`catch`/`finally`/`await`/settle-listener) removes the promise from
// the set via `mark_rejection_handled`.
// ---------------------------------------------------------------------------

thread_local! {
    static UNHANDLED_REJECTIONS: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
}

/// Record a rejection that has no reaction attached yet.
pub(crate) fn track_unhandled_rejection(promise: *mut Promise) {
    if promise.is_null() {
        return;
    }
    UNHANDLED_REJECTIONS.with(|m| m.borrow_mut().push(promise as usize));
}

/// A handler was attached to `promise` — it is no longer an unhandled rejection.
/// Cheap no-op for the common case (the set is empty on the hot async path).
pub(crate) fn mark_rejection_handled(promise: *mut Promise) {
    if promise.is_null() {
        return;
    }
    let key = promise as usize;
    UNHANDLED_REJECTIONS.with(|m| {
        let mut v = m.borrow_mut();
        if !v.is_empty() {
            v.retain(|p| *p != key);
        }
    });
}

/// Program-end hook (emitted by codegen's event-loop exit block, after the
/// final microtask/timer drain). If any rejection went unhandled, mirror Node:
/// print to stderr and exit with a non-zero code.
#[no_mangle]
pub extern "C" fn js_promise_report_unhandled_rejections() {
    // Backstop re-check: a tracked promise is only *still* unhandled if, at
    // program end, it is rejected AND no reaction was ever wired onto it. Any
    // consumer — `then`/`catch`/`finally`, chaining (`resolve_with_promise`),
    // or `attach_handlers` — sets `on_rejected` or `next` on the promise, so
    // re-reading those fields catches handlers attached through direct-field
    // paths we don't explicitly hook. (Settle-listener consumers don't touch
    // these fields, so `attach_settle_listener` removes them from the set at
    // attach time.) This makes the detector robust to internal machinery
    // (async generators, async-from-sync iterators) adopting a rejection.
    let unhandled_reasons: Vec<f64> = UNHANDLED_REJECTIONS.with(|m| {
        m.borrow()
            .iter()
            .filter_map(|&p| {
                let pr = p as *const Promise;
                unsafe {
                    if (*pr).state == PromiseState::Rejected
                        && (*pr).on_rejected.is_null()
                        && (*pr).next.is_null()
                    {
                        Some((*pr).reason)
                    } else {
                        None
                    }
                }
            })
            .collect()
    });
    if unhandled_reasons.is_empty() {
        return;
    }
    // Surface the rejection reason instead of the bare, opaque
    // "Uncaught (in promise)" line (#4841): an unhandled rejection that
    // carried a `TypeError: ...` previously printed nothing useful, forcing
    // users to wrap every call in `.catch` just to learn what failed.
    // `js_jsvalue_to_string` renders Error values as `<Name>: <message>` and
    // everything else via ordinary ToString, matching the synchronous
    // uncaught-throw header.
    let reason = unhandled_reasons[0];
    let reason_str_ptr = crate::value::js_jsvalue_to_string(reason);
    let reason_str = unsafe { crate::exception::string_header_to_string(reason_str_ptr) };
    if reason_str.is_empty() {
        eprintln!("Uncaught (in promise)");
    } else {
        eprintln!("Uncaught (in promise) {reason_str}");
    }
    // Match Node's unhandled-rejection exit code (1). The event loop has
    // already drained, so there is no pending work to lose.
    std::process::exit(1);
}

pub(super) struct PromiseSettleListener {
    pub(super) on_fulfilled: ClosurePtr,
    pub(super) on_rejected: ClosurePtr,
    pub(super) context: AsyncContextSnapshot,
}

thread_local! {
    pub(super) static PROMISE_SETTLE_LISTENERS: RefCell<Vec<(usize, PromiseSettleListener)>> =
        const { RefCell::new(Vec::new()) };
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
                crate::gc::runtime_write_barrier_root_raw_ptr(promise);
                crate::gc::runtime_write_barrier_root_raw_ptr(on_fulfilled);
                crate::gc::runtime_write_barrier_root_raw_ptr(on_rejected);
                PROMISE_SETTLE_LISTENERS.with(|listeners| {
                    listeners.borrow_mut().push((
                        promise as usize,
                        PromiseSettleListener {
                            on_fulfilled,
                            on_rejected,
                            context,
                        },
                    ));
                });
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

fn promise_take_settle_listeners(promise: *mut Promise) -> Vec<PromiseSettleListener> {
    if promise.is_null() {
        return Vec::new();
    }
    PROMISE_SETTLE_LISTENERS.with(|listeners| {
        let mut listeners = listeners.borrow_mut();
        let key = promise as usize;
        let mut drained = Vec::new();
        let mut i = 0;
        while i < listeners.len() {
            if listeners[i].0 == key {
                drained.push(listeners.swap_remove(i).1);
            } else {
                i += 1;
            }
        }
        drained
    })
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
        q.borrow_mut().push_back(Task::Inline(
            callback,
            value,
            ptr::null_mut(),
            is_fulfilled,
            context,
        ));
    });
}

pub(super) fn scan_promise_settle_listeners_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    PROMISE_SETTLE_LISTENERS.with(|listeners| {
        for (key, listener) in listeners.borrow_mut().iter_mut() {
            visitor.visit_metadata_usize_slot(key);
            visitor.visit_raw_const_ptr_slot(&mut listener.on_fulfilled);
            visitor.visit_raw_const_ptr_slot(&mut listener.on_rejected);
            scan_snapshot_roots_mut(&mut listener.context, visitor);
        }
    });
}

/// Allocate a new Promise
#[no_mangle]
pub extern "C" fn js_promise_new() -> *mut Promise {
    js_promise_new_with_parent(ptr::null_mut())
}

/// Allocate a new Promise, recording `parent` so `v8.promiseHooks` `init`
/// callbacks (#3139) receive the parent promise.
pub(crate) fn js_promise_new_with_parent(parent: *mut Promise) -> *mut Promise {
    bump(&MT_PROMISE_NEW_COUNT);
    let async_hooks_active = crate::async_hooks::hooks_active();
    let lifecycle_hooks_active = async_hooks_active || crate::v8::promise_hooks_active();
    let raw = if lifecycle_hooks_active {
        crate::gc::gc_malloc(std::mem::size_of::<Promise>(), crate::gc::GC_TYPE_PROMISE)
    } else {
        crate::arena::arena_alloc_gc(
            std::mem::size_of::<Promise>(),
            std::mem::align_of::<Promise>(),
            crate::gc::GC_TYPE_PROMISE,
        )
    };
    let promise = raw as *mut Promise;
    let scope = crate::gc::RuntimeHandleScope::new();
    let promise_handle = scope.root_raw_mut_ptr(promise);
    let parent_handle = scope.root_raw_mut_ptr(parent);
    unsafe {
        // GC_STORE_AUDIT(INIT): initializes freshly allocated Promise storage before the promise is published.
        ptr::write(promise, Promise::new());
        if async_hooks_active {
            let promise = promise_handle.get_raw_mut_ptr::<Promise>();
            let resource =
                f64::from_bits(0x7FFD_0000_0000_0000 | (promise as u64 & 0x0000_FFFF_FFFF_FFFF));
            let ids = crate::async_hooks::init_resource("PROMISE", resource, false);
            let promise = promise_handle.get_raw_mut_ptr::<Promise>();
            (*promise).async_id = ids.async_id;
            (*promise).trigger_async_id = ids.trigger_async_id;
        }
    }
    crate::v8::promise_hook_init(
        promise_handle.get_raw_mut_ptr::<Promise>(),
        parent_handle.get_raw_mut_ptr::<Promise>(),
    );
    promise_handle.get_raw_mut_ptr::<Promise>()
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
    // Reading a promise's rejection reason to consume it (the codegen `await`
    // lowering does exactly this on an already-settled promise) counts as
    // handling the rejection. Marking here is the robust catch-all for the
    // await consume path, which reads the reason directly instead of attaching
    // a reject reaction. Eager marking is safe: it can only suppress a report,
    // never produce a spurious one.
    mark_rejection_handled(promise);
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
    // `await`/async-step consumes the settled result here — observing a
    // rejection's reason counts as handling it (see `js_promise_reason`).
    mark_rejection_handled(promise);
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
        store_promise_jsvalue_slot(promise, std::ptr::addr_of_mut!((*promise).value), value);
        crate::async_hooks::promise_resolve((*promise).async_id);
        crate::v8::promise_hook_settled(promise);

        // Schedule callbacks. Push to TASK_QUEUE whenever there's anything
        // for the microtask runner to do — either invoke the user callback,
        // or propagate the value to the chained `next` promise. Issue #236:
        // pre-fix the queue push only fired when `on_fulfilled` was non-null,
        // so `.then(console.log)` (where `console.log`-as-value lowers to
        // a NULL ClosurePtr sentinel — see expr.rs:GlobalGet→PropertyGet
        // value path) skipped the queue entirely; the chained promise then
        // never settled and `await chained` busy-waited forever.
        let settle_listeners = promise_take_settle_listeners(promise);
        let has_settle_listeners = !settle_listeners.is_empty();
        let promise_all_states = combinators::promise_all_take_all_handlers(promise);
        let has_normal_handler = !(*promise).on_fulfilled.is_null() || !(*promise).next.is_null();
        if has_settle_listeners || !promise_all_states.is_empty() || has_normal_handler {
            let task_context = context_for_promise(promise);
            TASK_QUEUE.with(|q| {
                let mut q = q.borrow_mut();
                for listener in settle_listeners {
                    if !listener.on_fulfilled.is_null() {
                        q.push_back(Task::Inline(
                            listener.on_fulfilled,
                            value,
                            ptr::null_mut(),
                            true,
                            listener.context,
                        ));
                    }
                }
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
    // `outer` is adopting `inner`'s eventual state — `inner`'s rejection (now or
    // later) is consumed by `outer`, so `inner` is no longer an unhandled
    // rejection (HostPromiseRejectionTracker "handle"). Without this, a thenable
    // assimilated synchronously (`assimilate_via_then_property` rejects its
    // wrapper before chaining) would leave that wrapper flagged unhandled even
    // though its rejection flows into `outer`. Tracking moves to `outer`.
    mark_rejection_handled(inner);

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
                    store_promise_next_slot(inner, std::ptr::addr_of_mut!((*inner).next), outer);
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
                store_promise_closure_slot(
                    inner,
                    std::ptr::addr_of_mut!((*inner).on_fulfilled),
                    resolve_closure,
                );
                store_promise_closure_slot(
                    inner,
                    std::ptr::addr_of_mut!((*inner).on_rejected),
                    reject_closure,
                );
                // Don't chain; the forwarding callbacks handle resolution.
                store_promise_next_slot(
                    inner,
                    std::ptr::addr_of_mut!((*inner).next),
                    ptr::null_mut(),
                );
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
        store_promise_jsvalue_slot(promise, std::ptr::addr_of_mut!((*promise).reason), reason);
        crate::async_hooks::promise_resolve((*promise).async_id);
        crate::v8::promise_hook_settled(promise);

        // Schedule callbacks. Same propagation rule as `js_promise_resolve`
        // (#236): push to the queue whenever there's a callback to invoke
        // OR a chained `next` promise to forward to.
        let settle_listeners = promise_take_settle_listeners(promise);
        let has_settle_listeners = !settle_listeners.is_empty();
        let promise_all_states = combinators::promise_all_take_all_handlers(promise);
        let has_normal_handler = !(*promise).on_rejected.is_null() || !(*promise).next.is_null();
        if !has_settle_listeners && promise_all_states.is_empty() && !has_normal_handler {
            // No reaction attached at rejection time → currently unhandled.
            // A later `then`/`catch`/`await`/settle-listener removes it. If
            // a `.then(onFulfilled)` (no reject handler) is attached, `next`
            // is non-null so `has_normal_handler` is true here and the
            // rejection propagates to `next`, whose own settlement re-runs
            // this check — the leaf unhandled promise is the one tracked.
            track_unhandled_rejection(promise);
        }
        if has_settle_listeners || !promise_all_states.is_empty() || has_normal_handler {
            let task_context = context_for_promise(promise);
            TASK_QUEUE.with(|q| {
                let mut q = q.borrow_mut();
                for listener in settle_listeners {
                    if !listener.on_rejected.is_null() {
                        q.push_back(Task::Inline(
                            listener.on_rejected,
                            reason,
                            ptr::null_mut(),
                            false,
                            listener.context,
                        ));
                    }
                }
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
    // Attaching a reaction (`then`/`catch`/`finally`/`await`) marks any prior
    // rejection on `promise` as handled, per HostPromiseRejectionTracker.
    mark_rejection_handled(promise);

    // `js_promise_new_with_parent` can allocate via the GC and fire
    // `v8.promiseHooks` `init` callbacks (running JS), so root the inputs across
    // it before threading them into the chained promise.
    let scope = crate::gc::RuntimeHandleScope::new();
    let promise_handle = scope.root_raw_mut_ptr(promise);
    let on_fulfilled_handle = scope.root_raw_const_ptr(on_fulfilled);
    let on_rejected_handle = scope.root_raw_const_ptr(on_rejected);
    let next = js_promise_new_with_parent(promise);
    let promise = promise_handle.get_raw_mut_ptr::<Promise>();
    let on_fulfilled = on_fulfilled_handle.get_raw_const_ptr::<crate::closure::ClosureHeader>();
    let on_rejected = on_rejected_handle.get_raw_const_ptr::<crate::closure::ClosureHeader>();

    unsafe {
        store_promise_closure_slot(
            promise,
            std::ptr::addr_of_mut!((*promise).on_fulfilled),
            on_fulfilled,
        );
        store_promise_closure_slot(
            promise,
            std::ptr::addr_of_mut!((*promise).on_rejected),
            on_rejected,
        );
        store_promise_next_slot(promise, std::ptr::addr_of_mut!((*promise).next), next);
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
    mark_rejection_handled(promise);
    unsafe {
        store_promise_closure_slot(
            promise,
            std::ptr::addr_of_mut!((*promise).on_fulfilled),
            on_fulfilled,
        );
        store_promise_closure_slot(
            promise,
            std::ptr::addr_of_mut!((*promise).on_rejected),
            on_rejected,
        );
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

    // `.finally()` is a reaction — it marks any prior rejection on `promise`
    // handled, even though it wires the wrapper handlers via direct field
    // stores below (not `js_promise_then`). Without this, `Promise.reject(x)
    // .finally(cb)` leaves the source promise flagged as an unhandled
    // rejection. Done before the GC alloc; promise objects are stable
    // malloc-GC payloads so the address used as the tracking key is unchanged.
    mark_rejection_handled(promise);

    // Create the `next` promise that callers chain off. Root inputs across the
    // allocation since `v8.promiseHooks` `init` may run JS (#3139).
    let scope = crate::gc::RuntimeHandleScope::new();
    let promise_handle = scope.root_raw_mut_ptr(promise);
    let on_finally_handle = scope.root_raw_const_ptr(on_finally);
    let next = js_promise_new_with_parent(promise);
    let promise = promise_handle.get_raw_mut_ptr::<Promise>();
    let on_finally = on_finally_handle.get_raw_const_ptr::<crate::closure::ClosureHeader>();
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
        store_promise_closure_slot(
            promise,
            std::ptr::addr_of_mut!((*promise).on_fulfilled),
            fulfill_wrap,
        );
        store_promise_closure_slot(
            promise,
            std::ptr::addr_of_mut!((*promise).on_rejected),
            reject_wrap,
        );
        // Wrappers own next; runner must not touch it.
        store_promise_next_slot(
            promise,
            std::ptr::addr_of_mut!((*promise).next),
            ptr::null_mut(),
        );
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

// ── #1545: bound then/catch/finally for promise *value-reads* ────────────────
//
// `promise.then(cb)` as a call is lowered specially by codegen
// (lower_call/property_get.rs), but a *value-read* — `typeof p.then`,
// `const f = p.then`, passing `p.then` as a callback — fell through to the
// generic property getter and returned `undefined`. These thunks let the
// generic getter (`js_dynamic_object_get_property`) hand back a real bound
// function that forwards to `js_promise_then` / `catch` / `finally`, so
// `typeof p.then === "function"` and a deferred `p.then(cb)` both behave.

#[inline]
fn arg_to_closure(v: f64) -> ClosurePtr {
    let bits = v.to_bits();
    let top = bits >> 48;
    // Pointer- or string-tagged values carry a heap pointer in the low 48 bits;
    // closures are pointer-tagged. Anything else (undefined/null/number) → null.
    if top == 0x7FFD || top == 0x7FFF {
        (bits & 0x0000_FFFF_FFFF_FFFF) as *const crate::closure::ClosureHeader
    } else {
        ptr::null()
    }
}

#[inline]
fn box_promise_ptr(p: *mut Promise) -> f64 {
    f64::from_bits(crate::value::JSValue::pointer(p as *const u8).bits())
}

fn throw_promise_prototype_incompatible_receiver(method: &str, receiver: f64) -> ! {
    let jsval = crate::value::JSValue::from_bits(receiver.to_bits());
    let label = if jsval.is_undefined() {
        "undefined".to_string()
    } else if jsval.is_null() {
        "null".to_string()
    } else if jsval.is_pointer() {
        "#<Object>".to_string()
    } else {
        crate::string::string_as_str(crate::value::js_jsvalue_to_string(receiver)).to_string()
    };
    let msg = format!("Method Promise.prototype.{method} called on incompatible receiver {label}");
    let msg_str = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err_ptr = crate::error::js_typeerror_new(msg_str);
    let err_value = crate::value::JSValue::pointer(err_ptr as *const u8).bits();
    crate::exception::js_throw(f64::from_bits(err_value))
}

fn promise_prototype_receiver(method: &str) -> *mut Promise {
    let receiver = crate::object::js_implicit_this_get();
    if js_value_is_promise(receiver) != 0 {
        return crate::value::js_nanbox_get_pointer(receiver) as *mut Promise;
    }
    throw_promise_prototype_incompatible_receiver(method, receiver)
}

fn call_receiver_then(receiver: f64, args: &[f64]) -> f64 {
    unsafe {
        crate::object::js_native_call_method(
            receiver,
            b"then".as_ptr() as *const i8,
            b"then".len(),
            args.as_ptr(),
            args.len(),
        )
    }
}

fn throw_promise_finally_non_object() -> ! {
    let msg = b"Promise.prototype.finally called on non-object";
    let msg_str = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err_ptr = crate::error::js_typeerror_new(msg_str);
    let err_value = crate::value::JSValue::pointer(err_ptr as *const u8).bits();
    crate::exception::js_throw(f64::from_bits(err_value))
}

pub(crate) extern "C" fn promise_prototype_then_thunk(
    _closure: *const crate::closure::ClosureHeader,
    on_fulfilled: f64,
    on_rejected: f64,
) -> f64 {
    let promise = promise_prototype_receiver("then");
    box_promise_ptr(js_promise_then(
        promise,
        arg_to_closure(on_fulfilled),
        arg_to_closure(on_rejected),
    ))
}

pub(crate) extern "C" fn promise_prototype_catch_thunk(
    _closure: *const crate::closure::ClosureHeader,
    on_rejected: f64,
) -> f64 {
    let receiver = crate::object::js_implicit_this_get();
    let args = [f64::from_bits(crate::value::TAG_UNDEFINED), on_rejected];
    call_receiver_then(receiver, &args)
}

pub(crate) extern "C" fn promise_prototype_finally_thunk(
    _closure: *const crate::closure::ClosureHeader,
    on_finally: f64,
) -> f64 {
    let receiver = crate::object::js_implicit_this_get();
    if js_value_is_promise(receiver) != 0 {
        let promise = crate::value::js_nanbox_get_pointer(receiver) as *mut Promise;
        return box_promise_ptr(js_promise_finally(promise, arg_to_closure(on_finally)));
    }
    let jsval = crate::value::JSValue::from_bits(receiver.to_bits());
    if !jsval.is_pointer() {
        throw_promise_finally_non_object();
    }
    let args = [on_finally, on_finally];
    call_receiver_then(receiver, &args)
}

extern "C" fn promise_then_bound(
    closure: *const crate::closure::ClosureHeader,
    on_fulfilled: f64,
    on_rejected: f64,
) -> f64 {
    let p = crate::closure::js_closure_get_capture_ptr(closure, 0) as *mut Promise;
    box_promise_ptr(js_promise_then(
        p,
        arg_to_closure(on_fulfilled),
        arg_to_closure(on_rejected),
    ))
}

extern "C" fn promise_catch_bound(
    closure: *const crate::closure::ClosureHeader,
    on_rejected: f64,
) -> f64 {
    let p = crate::closure::js_closure_get_capture_ptr(closure, 0) as *mut Promise;
    box_promise_ptr(js_promise_catch(p, arg_to_closure(on_rejected)))
}

extern "C" fn promise_finally_bound(
    closure: *const crate::closure::ClosureHeader,
    on_finally: f64,
) -> f64 {
    let p = crate::closure::js_closure_get_capture_ptr(closure, 0) as *mut Promise;
    box_promise_ptr(js_promise_finally(p, arg_to_closure(on_finally)))
}

/// Return a NaN-boxed bound function for a promise's `then`/`catch`/`finally`
/// value-read, or `None` for any other property. The returned closure captures
/// the promise (slot 0) so the GC keeps it alive and updates the pointer on
/// move, exactly like the existing forward wrappers above.
pub unsafe fn js_promise_bound_method(promise: *mut Promise, property: &str) -> Option<f64> {
    use crate::closure::{js_closure_alloc, js_closure_set_capture_ptr, js_register_closure_arity};
    let (thunk, arity): (*const u8, u32) = match property {
        "then" => (promise_then_bound as *const u8, 2),
        "catch" => (promise_catch_bound as *const u8, 1),
        "finally" => (promise_finally_bound as *const u8, 1),
        _ => return None,
    };
    js_register_closure_arity(thunk, arity);
    let closure = js_closure_alloc(thunk, 1);
    js_closure_set_capture_ptr(closure, 0, promise as i64);
    Some(f64::from_bits(
        crate::value::JSValue::pointer(closure as *const u8).bits(),
    ))
}

/// Fulfilled-path wrapper for `.finally()`.
/// Captures [on_finally, next_promise].
/// Called with the upstream fulfilled `value`.
extern "C" fn finally_fulfill_wrapper(
    closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    finally_wrapper_common(closure, value, true)
}

/// Rejected-path wrapper for `.finally()`.
/// Captures [on_finally, next_promise].
/// Called with the upstream rejection `reason`.
extern "C" fn finally_reject_wrapper(
    closure: *const crate::closure::ClosureHeader,
    reason: f64,
) -> f64 {
    finally_wrapper_common(closure, reason, false)
}

/// Shared body for both `.finally()` wrappers (issue #2825).
///
/// Node's `Promise.prototype.finally(onFinally)` semantics:
///   - `onFinally()` is called with no arguments.
///   - If it **throws**, the chained promise (`next`) rejects with the thrown
///     value, OVERRIDING the upstream outcome.
///   - If it returns a **Promise/thenable**, the chained promise waits for it:
///       * cleanup fulfilled → settle `next` with the ORIGINAL outcome
///         (the cleanup's fulfillment value is ignored).
///       * cleanup rejected → reject `next` with the CLEANUP reason
///         (overriding the upstream outcome).
///   - Otherwise (non-thenable return / non-callable onFinally), settle `next`
///     with the original outcome after one extra microtask hop (matching
///     Node's `.finally()` microtask depth).
///
/// `orig` is the upstream value (fulfilled) or reason (rejected); `is_fulfilled`
/// selects which.
fn finally_wrapper_common(
    closure: *const crate::closure::ClosureHeader,
    orig: f64,
    is_fulfilled: bool,
) -> f64 {
    use crate::closure::{
        js_closure_alloc, js_closure_get_capture_ptr, js_closure_set_capture_f64,
        js_closure_set_capture_ptr,
    };

    let on_finally = js_closure_get_capture_ptr(closure, 0) as *const crate::closure::ClosureHeader;
    let next = js_closure_get_capture_ptr(closure, 1) as *mut Promise;

    // Non-callable onFinally (e.g. `.finally(1)`): act like `.then(undefined,
    // undefined)` — pass the original outcome through after one extra hop.
    if on_finally.is_null() {
        finally_settle_next_with_extra_hop(next, orig, is_fulfilled);
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }

    // Call `onFinally()` under a dedicated try-frame so a throw from the
    // callback rejects `next` (overriding the upstream outcome) instead of
    // unwinding past us to the microtask runner's trap — whose `(*cur).next`
    // is null here (js_promise_finally clears it), so a throw would otherwise
    // be swallowed.
    let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
    let trap_buf = crate::exception::js_try_push();
    let jumped = unsafe { crate::ffi::setjmp::setjmp(trap_buf as *mut std::os::raw::c_int) };
    if jumped != 0 {
        // onFinally threw — reject `next` with the thrown value.
        let exc = crate::exception::js_get_exception();
        crate::exception::js_clear_exception();
        crate::exception::js_try_end();
        if !next.is_null() {
            js_promise_reject(next, exc);
        }
        return undef;
    }
    let ret = crate::closure::js_closure_call1(on_finally, undef);
    crate::exception::js_try_end();

    // If onFinally returned a Promise/thenable, adopt it: wait for it before
    // settling `next`. `js_assimilate_thenable` returns a native Promise for
    // both real Promises and user thenables, or the value unchanged otherwise.
    let cleanup = js_assimilate_thenable(ret);
    if js_value_is_promise(cleanup) != 0 {
        let inner = crate::value::js_nanbox_get_pointer(cleanup) as *mut Promise;
        if !inner.is_null() {
            // On cleanup fulfillment: settle `next` with the original outcome.
            let on_ok = js_closure_alloc(finally_cleanup_fulfill as *const u8, 3);
            js_closure_set_capture_ptr(on_ok, 0, next as i64);
            js_closure_set_capture_f64(on_ok, 1, orig);
            js_closure_set_capture_f64(
                on_ok,
                2,
                f64::from_bits(if is_fulfilled {
                    crate::value::TAG_TRUE
                } else {
                    crate::value::TAG_FALSE
                }),
            );
            // On cleanup rejection: reject `next` with the cleanup reason.
            let on_err = js_closure_alloc(finally_cleanup_reject as *const u8, 1);
            js_closure_set_capture_ptr(on_err, 0, next as i64);
            js_promise_then(inner, on_ok, on_err);
            return undef;
        }
    }

    // Plain (non-thenable) return value: pass the original outcome through
    // after one extra microtask hop.
    finally_settle_next_with_extra_hop(next, orig, is_fulfilled);
    undef
}

/// Settle `next` with the original outcome after one extra microtask hop,
/// matching Node's `.finally()` microtask depth.
fn finally_settle_next_with_extra_hop(next: *mut Promise, orig: f64, is_fulfilled: bool) {
    use crate::closure::{
        js_closure_alloc, js_closure_set_capture_f64, js_closure_set_capture_ptr,
    };
    if next.is_null() {
        return;
    }
    let pass_fn = if is_fulfilled {
        finally_passthrough_fulfill as *const u8
    } else {
        finally_passthrough_reject as *const u8
    };
    let pass = js_closure_alloc(pass_fn, 2);
    js_closure_set_capture_ptr(pass, 0, next as i64);
    js_closure_set_capture_f64(pass, 1, orig);
    let undef_promise = js_promise_resolved(f64::from_bits(crate::value::TAG_UNDEFINED));
    // js_promise_then's side-effect is enqueuing `pass` to run next iteration.
    js_promise_then(undef_promise, pass, ptr::null());
}

/// Cleanup-fulfilled handler: settle `next` with the ORIGINAL outcome.
/// Captures [next_promise_ptr (i64), orig (f64), is_fulfilled (TAG_TRUE/FALSE)].
extern "C" fn finally_cleanup_fulfill(
    closure: *const crate::closure::ClosureHeader,
    _cleanup_value: f64,
) -> f64 {
    use crate::closure::{js_closure_get_capture_f64, js_closure_get_capture_ptr};
    let next = js_closure_get_capture_ptr(closure, 0) as *mut Promise;
    let orig = js_closure_get_capture_f64(closure, 1);
    let is_fulfilled = js_closure_get_capture_f64(closure, 2).to_bits() == crate::value::TAG_TRUE;
    if !next.is_null() {
        if is_fulfilled {
            js_promise_resolve(next, orig);
        } else {
            js_promise_reject(next, orig);
        }
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// Cleanup-rejected handler: reject `next` with the CLEANUP reason (overrides
/// the upstream outcome). Captures [next_promise_ptr (i64)].
extern "C" fn finally_cleanup_reject(
    closure: *const crate::closure::ClosureHeader,
    cleanup_reason: f64,
) -> f64 {
    use crate::closure::js_closure_get_capture_ptr;
    let next = js_closure_get_capture_ptr(closure, 0) as *mut Promise;
    if !next.is_null() {
        js_promise_reject(next, cleanup_reason);
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// Passthrough closure for the extra hop in the fulfilled path.
/// Captures [next_promise_ptr (i64), value (f64)].
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

/// Passthrough closure for the extra hop in the rejected path.
/// Captures [next_promise_ptr (i64), reason (f64)].
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
