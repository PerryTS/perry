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
    /// Promises the runtime owns and observes through internal channels — a
    /// WHATWG reader/writer `closed` promise, a `[[closeRequest]]`, etc. Node
    /// marks these `markPromiseAsHandled` at creation so that an abort / error
    /// / cancel that later rejects them is never surfaced as an unhandled
    /// rejection. We mirror that with a persistent membership set consulted at
    /// rejection-track time. Stays empty for non-stream programs, so the hot
    /// reject path pays nothing (#1545).
    static INTERNALLY_HANDLED: RefCell<std::collections::HashSet<usize>> =
        RefCell::new(std::collections::HashSet::new());
}

/// Mark a promise as internally handled (Node's `markPromiseAsHandled`): a
/// later rejection of it is never reported as unhandled. Used by the WHATWG
/// stream implementation for the internal `closed` / `closeRequest` promises it
/// settles on abort/error/cancel without a user-attached reaction (#1545).
#[no_mangle]
pub extern "C" fn js_promise_mark_internally_handled(promise: *mut Promise) {
    if promise.is_null() {
        return;
    }
    INTERNALLY_HANDLED.with(|s| {
        s.borrow_mut().insert(promise as usize);
    });
    // If it already rejected before being marked, drop it from the set now.
    mark_rejection_handled(promise);
}

/// Keep the stdlib-facing marker alive through the dead-strip pass on the
/// PERRY_NO_AUTO_OPTIMIZE prebuilt-lib link (same pattern as the program-end
/// hook anchor below).
#[used]
static KEEP_PROMISE_MARK_INTERNALLY_HANDLED: extern "C" fn(*mut Promise) =
    js_promise_mark_internally_handled;

fn is_internally_handled(promise: *mut Promise) -> bool {
    INTERNALLY_HANDLED.with(|s| {
        let s = s.borrow();
        !s.is_empty() && s.contains(&(promise as usize))
    })
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
    // Prefer the rejection Error's `stack` (it begins with `<Name>: <message>`
    // and carries `at <frame>` file:line lines) so the throw site is visible —
    // mirrors the synchronous uncaught-throw handler (`exception::print_uncaught`).
    // Falls back to ToString for non-Error reasons / empty stacks.
    let mut printed = false;
    let bits = reason.to_bits();
    if (bits >> 48) == 0x7FFD {
        let ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as usize;
        // Band+plausibility gate (2026-07-02 audit): a POINTER-tagged
        // rejection reason can be a registry HANDLE (a fetch Response id in
        // the 0x40000 band — `fetch().then(r => { throw r })` uncaught) —
        // the old bare `>= 0x10000` deref'd the id as memory instead of
        // printing the fallback line.
        if crate::value::addr_class::is_plausible_heap_addr(ptr)
            && unsafe { *(ptr as *const u32) } == crate::error::OBJECT_TYPE_ERROR
        {
            let eh = ptr as *const crate::error::ErrorHeader;
            let stack_str = unsafe { crate::exception::string_header_to_string((*eh).stack) };
            if !stack_str.is_empty() {
                eprintln!("Uncaught (in promise) {stack_str}");
                printed = true;
            }
        }
    }
    if !printed {
        let reason_str_ptr = crate::value::js_jsvalue_to_string(reason);
        let reason_str = unsafe { crate::exception::string_header_to_string(reason_str_ptr) };
        if reason_str.is_empty() {
            eprintln!("Uncaught (in promise)");
        } else {
            eprintln!("Uncaught (in promise) {reason_str}");
        }
    }
    // Match Node's unhandled-rejection exit code (1). The event loop has
    // already drained, so there is no pending work to lose.
    std::process::exit(1);
}

// #4876: keep the codegen-emitted program-end hook alive through the
// auto-optimize whole-program-bitcode link. `js_promise_report_unhandled_rejections`
// is emitted unconditionally into `_main` but is reachable only from generated
// `.o`; without a `#[used]` anchor the internalize+dead-strip pass drops it and
// every native link fails with "undefined symbol" (see the error.rs/combinators.rs
// anchors for the same pattern).
#[used]
static KEEP_PROMISE_REPORT_UNHANDLED_REJECTIONS: extern "C" fn() =
    js_promise_report_unhandled_rejections;

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

// ---------------------------------------------------------------------------
// Multiple reactions per promise (PerformPromiseThen's [[PromiseFulfillReactions]]
// / [[PromiseRejectReactions]] lists).
//
// The `Promise` struct holds ONE `on_fulfilled`/`on_rejected`/`next` triple, so
// the FIRST `.then`/`.catch`/`.finally` reaction uses those inline slots (the
// common, hot, zero-overhead case). A SECOND+ reaction on the same promise —
// `p.then(a); p.then(b)`, or a user `.then` plus a combinator's per-element
// `.then` when `Promise.resolve(p) === p` — would clobber the slot. Those
// overflow reactions are parked here, keyed by promise pointer, and replayed in
// FIFO registration order (after the slot reaction) when the promise settles.
//
// Each overflow reaction carries its OWN chained `next` promise and async
// context, so the chained promise settles and runs in the correct realm —
// dispatched via `Task::Inline`, which already models "invoke one handler (or
// pass the value through when null) and resolve `next` with the result".
// ---------------------------------------------------------------------------

pub(super) struct OverflowReaction {
    pub(super) on_fulfilled: ClosurePtr,
    pub(super) on_rejected: ClosurePtr,
    pub(super) next: *mut Promise,
    pub(super) context: AsyncContextSnapshot,
}

thread_local! {
    pub(super) static PROMISE_OVERFLOW_REACTIONS: RefCell<Vec<(usize, OverflowReaction)>> =
        const { RefCell::new(Vec::new()) };
}

/// Park a 2nd+ reaction on a still-pending `promise`.
fn push_overflow_reaction(
    promise: *mut Promise,
    on_fulfilled: ClosurePtr,
    on_rejected: ClosurePtr,
    next: *mut Promise,
    context: AsyncContextSnapshot,
) {
    crate::gc::runtime_write_barrier_root_raw_ptr(promise);
    crate::gc::runtime_write_barrier_root_raw_ptr(on_fulfilled);
    crate::gc::runtime_write_barrier_root_raw_ptr(on_rejected);
    crate::gc::runtime_write_barrier_root_raw_ptr(next);
    PROMISE_OVERFLOW_REACTIONS.with(|r| {
        r.borrow_mut().push((
            promise as usize,
            OverflowReaction {
                on_fulfilled,
                on_rejected,
                next,
                context,
            },
        ));
    });
}

/// Drain (in registration order) every overflow reaction registered against
/// `promise`. Returns `Vec::new()` for the overwhelmingly common no-overflow
/// case without touching the table's allocation.
fn promise_take_overflow_reactions(promise: *mut Promise) -> Vec<OverflowReaction> {
    PROMISE_OVERFLOW_REACTIONS.with(|r| {
        let mut r = r.borrow_mut();
        if r.is_empty() {
            return Vec::new();
        }
        let key = promise as usize;
        let mut drained = Vec::new();
        // Preserve FIFO order (a plain filter keeps relative order; swap_remove
        // would not — reaction ordering is observable, see resolved-sequence).
        r.retain(|(k, reaction)| {
            if *k == key {
                drained.push(OverflowReaction {
                    on_fulfilled: reaction.on_fulfilled,
                    on_rejected: reaction.on_rejected,
                    next: reaction.next,
                    context: reaction.context.clone(),
                });
                false
            } else {
                true
            }
        });
        drained
    })
}

/// Push the `Task::Inline` jobs for a settled promise's drained overflow
/// reactions. `value` is the fulfilled value or rejection reason.
fn enqueue_overflow_reactions(
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
        q.push_back(Task::Inline(cb, value, r.next, is_fulfilled, r.context));
    }
}

pub(super) fn scan_promise_overflow_reactions_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    PROMISE_OVERFLOW_REACTIONS.with(|reactions| {
        for (key, reaction) in reactions.borrow_mut().iter_mut() {
            visitor.visit_metadata_usize_slot(key);
            visitor.visit_raw_const_ptr_slot(&mut reaction.on_fulfilled);
            visitor.visit_raw_const_ptr_slot(&mut reaction.on_rejected);
            visitor.visit_raw_mut_ptr_slot(&mut reaction.next);
            scan_snapshot_roots_mut(&mut reaction.context, visitor);
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
    let promise = promise_handle.get_raw_mut_ptr::<Promise>();
    // #5142: a recycled address may carry expando properties (`p.status = …`)
    // left by a previously-collected promise; a fresh promise must start clean.
    crate::object::exotic_expando::expando_clear_on_alloc(promise as usize);
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
        let overflow_reactions = promise_take_overflow_reactions(promise);
        let has_overflow = !overflow_reactions.is_empty();
        let has_normal_handler = !(*promise).on_fulfilled.is_null() || !(*promise).next.is_null();
        if has_settle_listeners
            || !promise_all_states.is_empty()
            || has_normal_handler
            || has_overflow
        {
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
                // Replay 2nd+ reactions in registration order, after the slot.
                enqueue_overflow_reactions(overflow_reactions, value, true, &mut q);
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

                // #5437 bug#2: add the forwarding callbacks as an OVERFLOW
                // reaction rather than OVERWRITING inner's inline
                // on_fulfilled/on_rejected. The previous `store_promise_closure_slot`
                // writes clobbered any existing reaction — e.g. an async-step
                // awaiter's resume thunk registered by an earlier `await inner` —
                // so when `inner` settled it fired only the forwarder and the
                // awaiter hung forever (Next.js SSR render deadlock). Both the
                // existing inline reaction AND this forwarder now fire when inner
                // settles: the awaiter resumes AND `outer` adopts inner's state.
                push_overflow_reaction(
                    inner,
                    resolve_closure,
                    reject_closure,
                    ptr::null_mut(),
                    capture_context(),
                );
                // Keep the original next-nulling: the forwarder handles adoption,
                // and leaving a stale `inner.next` chained to `outer` can form a
                // resolution cycle ("Chaining cycle detected"). The awaiter's
                // reaction lives in on_fulfilled (untouched above), not in `next`.
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
        let overflow_reactions = promise_take_overflow_reactions(promise);
        let has_overflow = !overflow_reactions.is_empty();
        let has_normal_handler = !(*promise).on_rejected.is_null() || !(*promise).next.is_null();
        if !has_settle_listeners
            && promise_all_states.is_empty()
            && !has_normal_handler
            && !has_overflow
        {
            // No reaction attached at rejection time → currently unhandled.
            // A later `then`/`catch`/`await`/settle-listener removes it. If
            // a `.then(onFulfilled)` (no reject handler) is attached, `next`
            // is non-null so `has_normal_handler` is true here and the
            // rejection propagates to `next`, whose own settlement re-runs
            // this check — the leaf unhandled promise is the one tracked.
            // Promises the runtime marked internally handled (stream `closed`
            // promises, etc.) are never reported (#1545).
            if !is_internally_handled(promise) {
                track_unhandled_rejection(promise);
            }
        }
        if has_settle_listeners
            || !promise_all_states.is_empty()
            || has_normal_handler
            || has_overflow
        {
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
                // Replay 2nd+ reactions in registration order, after the slot.
                enqueue_overflow_reactions(overflow_reactions, reason, false, &mut q);
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
        // A promise may carry MULTIPLE reactions (`p.then(a); p.then(b)`, or a
        // user `.then` racing a combinator's per-element `.then` when
        // `Promise.resolve(p) === p`). The inline slot holds the FIRST reaction;
        // 2nd+ reactions overflow into a side table and replay in registration
        // order. Occupancy = any of the two reaction closures OR `next`: a
        // degenerate no-arg `then()` parks with BOTH closures null and only
        // `next` set, and overwriting that `next` here stranded its chained
        // promise (the spec combinators attach per-element reactions through
        // this very path via `invoke_then`, so `const c = p.then();
        // Promise.all([p])` lost `c`). `.finally` deliberately nulls `next`
        // but always sets both wrapper closures, so it is still detected.
        let slot_occupied = !(*promise).on_fulfilled.is_null()
            || !(*promise).on_rejected.is_null()
            || !(*promise).next.is_null();

        if !slot_occupied {
            // Fast path: first reaction uses the inline slot (unchanged behavior).
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

            // If already settled, schedule callback immediately. Same
            // propagation rule as `js_promise_resolve`/`js_promise_reject`
            // (#236): push to the queue when there's either a callback to invoke
            // OR a chained `next` promise to forward to. `next` is always
            // non-null here (we just created it), so this is effectively
            // unconditional — the explicit checks document the intent.
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
        } else {
            // Overflow path: 2nd+ reaction. Carries its own `next` + context and
            // dispatches via `Task::Inline` (which handles the null-callback
            // pass-through), so the chained promise still settles correctly.
            let context = capture_context();
            match (*promise).state {
                PromiseState::Pending => {
                    push_overflow_reaction(promise, on_fulfilled, on_rejected, next, context);
                }
                PromiseState::Fulfilled => {
                    let value = (*promise).value;
                    TASK_QUEUE.with(|q| {
                        q.borrow_mut().push_back(Task::Inline(
                            on_fulfilled,
                            value,
                            next,
                            true,
                            context,
                        ));
                    });
                }
                PromiseState::Rejected => {
                    let reason = (*promise).reason;
                    TASK_QUEUE.with(|q| {
                        q.borrow_mut().push_back(Task::Inline(
                            on_rejected,
                            reason,
                            next,
                            false,
                            context,
                        ));
                    });
                }
            }
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
        // The inline handler slots are single-valued. Combinators reach here
        // with USER promises (`promise_resolve_for_combinator` returns the
        // input promise identity), so an unconditional store clobbered any
        // reaction attached earlier: `p.then(cb); Promise.all([p])` lost
        // `cb`, and two combinators sharing one pending input destroyed each
        // other's forwarder — the loser's remaining-count never reached zero,
        // a permanent hang on shared chunk promises (#5437 family). Mirror
        // `js_promise_then`: the first reaction takes the slot, later ones
        // divert to the overflow table (pending) or dispatch as their own
        // inline task (settled). `next` stays null throughout — these
        // callers own their settlement.
        //
        // `next` counts as occupancy too: a degenerate no-arg `p.then()`
        // parks with BOTH handler slots null and only `next` set, and its
        // chained promise must still resolve with the pass-through value —
        // storing handlers next to it would resolve that chain with the
        // handler's return instead.
        let slot_occupied = !(*promise).on_fulfilled.is_null()
            || !(*promise).on_rejected.is_null()
            || !(*promise).next.is_null();
        if slot_occupied {
            let context = capture_context();
            match (*promise).state {
                PromiseState::Pending => {
                    push_overflow_reaction(
                        promise,
                        on_fulfilled,
                        on_rejected,
                        ptr::null_mut(),
                        context,
                    );
                }
                PromiseState::Fulfilled => {
                    if !on_fulfilled.is_null() {
                        let value = (*promise).value;
                        TASK_QUEUE.with(|q| {
                            q.borrow_mut().push_back(Task::Inline(
                                on_fulfilled,
                                value,
                                ptr::null_mut(),
                                true,
                                context,
                            ));
                        });
                    }
                }
                PromiseState::Rejected => {
                    if !on_rejected.is_null() {
                        let reason = (*promise).reason;
                        TASK_QUEUE.with(|q| {
                            q.borrow_mut().push_back(Task::Inline(
                                on_rejected,
                                reason,
                                ptr::null_mut(),
                                false,
                                context,
                            ));
                        });
                    }
                }
            }
            return;
        }

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
    //
    // ALREADY-SETTLED promises must NOT go through the slot stores: the
    // handler slots are single-valued, and a queued `Task::Promise` re-reads
    // them at drain time. Attaching `.finally()` to a settled promise that
    // already queued another reaction (`p.catch(cb); p.finally(end)`) — or
    // attaching several `.finally()`s to one settled promise (Turbopack's
    // `loadChunkAsync` returns a shared pre-fulfilled `loadedChunk` constant,
    // and Next's CacheSignal does `chunkPromise.finally(endRead)` per load) —
    // overwrote the slot, so WHICHEVER wrapper was stored last ran once per
    // queued task: one callback fired twice (or N times) while the others
    // never ran. Next.js's cache-read count went negative, `cacheReady()`
    // never resolved, the prerender was never aborted, and the dynamic-SSR
    // routes hung forever (#5437). Dispatch each settled attach as a
    // `Task::Inline` carrying ITS OWN wrapper instead; the slots stay
    // untouched.
    unsafe {
        match (*promise).state {
            PromiseState::Pending => {
                // The slots are single-valued: if a reaction is already
                // registered (`p.then(cb); p.finally(end)` while pending, or
                // two pending `.finally()`s), the old unconditional store
                // overwrote its wrapper AND nulled `promise.next`, so the
                // earlier callback never ran and its chained promise never
                // settled. Divert later reactions to the overflow table; the
                // wrapper owns its `next` (null here), matching the settled
                // arms below.
                //
                // A non-null `next` also counts as occupancy: a degenerate
                // no-arg `p.then()` parks with both handler slots null and
                // only `next` set — nulling that `next` here would strand its
                // chained promise forever.
                let slot_occupied = !(*promise).on_fulfilled.is_null()
                    || !(*promise).on_rejected.is_null()
                    || !(*promise).next.is_null();
                if slot_occupied {
                    push_overflow_reaction(
                        promise,
                        fulfill_wrap,
                        reject_wrap,
                        ptr::null_mut(),
                        capture_context(),
                    );
                } else {
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
                }
            }
            PromiseState::Fulfilled => {
                // Capture the attach-time async context: these arms no longer
                // store into the promise's context slot, so
                // `context_for_promise` could hand back a context stored by
                // an EARLIER reaction on the same promise.
                let context = capture_context();
                TASK_QUEUE.with(|q| {
                    q.borrow_mut().push_back(Task::Inline(
                        fulfill_wrap,
                        (*promise).value,
                        ptr::null_mut(),
                        true,
                        context,
                    ));
                });
            }
            PromiseState::Rejected => {
                let context = capture_context();
                TASK_QUEUE.with(|q| {
                    q.borrow_mut().push_back(Task::Inline(
                        reject_wrap,
                        (*promise).reason,
                        ptr::null_mut(),
                        false,
                        context,
                    ));
                });
            }
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
pub(crate) fn arg_to_closure(v: f64) -> ClosurePtr {
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
pub(crate) fn box_promise_ptr(p: *mut Promise) -> f64 {
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

/// ECMA-262 Invoke(receiver, "then", args): read the `then` property and call it.
/// Unlike `js_native_call_method`, this reads the own/inherited `then` property
/// first (invoking accessor getters) and throws TypeError if it is not callable —
/// correctly propagating overridden `then` properties on Promise instances.
fn call_receiver_then(receiver: f64, args: &[f64]) -> f64 {
    let then_fn = unsafe {
        crate::value::js_dynamic_object_get_property(receiver, b"then".as_ptr() as *const i8, 4)
    };
    if !super::spec_combinators::is_callable_value(then_fn) {
        let msg = b"Promise method called on non-callable 'then'";
        let msg_str = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
        let err_ptr = crate::error::js_typeerror_new(msg_str);
        let err_val = crate::value::JSValue::pointer(err_ptr as *const u8).bits();
        crate::exception::js_throw(f64::from_bits(err_val));
    }
    let prev_this = crate::object::js_implicit_this_set(receiver);
    let result =
        unsafe { crate::closure::js_native_call_value(then_fn, args.as_ptr(), args.len()) };
    crate::object::js_implicit_this_set(prev_this);
    result
}

fn throw_promise_finally_non_object() -> ! {
    let msg = b"Promise.prototype.finally called on non-object";
    let msg_str = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err_ptr = crate::error::js_typeerror_new(msg_str);
    let err_value = crate::value::JSValue::pointer(err_ptr as *const u8).bits();
    crate::exception::js_throw(f64::from_bits(err_value))
}

fn throw_type_error_thunk(msg: &str) -> ! {
    let s = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_typeerror_new(s);
    let v = f64::from_bits(crate::value::JSValue::pointer(err as *const u8).bits());
    crate::exception::js_throw(v)
}

// True iff `value` is a JavaScript Object (heap-pointer tagged, not a Symbol or handle).
fn is_promise_species_object(value: f64) -> bool {
    let bits = value.to_bits();
    if (bits & crate::value::TAG_MASK) != crate::value::POINTER_TAG {
        return false;
    }
    let raw = (bits & crate::value::POINTER_MASK) as usize;
    if crate::value::addr_class::is_handle_band(raw) {
        // Proxies are encoded as handle-band values but are valid objects.
        return crate::proxy::js_proxy_is_proxy(value) != 0;
    }
    !crate::symbol::is_registered_symbol(raw)
}

fn get_intrinsic_promise() -> f64 {
    crate::object::js_get_global_this_builtin_value(b"Promise".as_ptr(), b"Promise".len())
}

/// Return the `Promise.prototype[name]` closure (then/catch/finally), or `None`.
/// Used by the dynamic dispatch path to route spec-path calls when a promise has
/// own properties that modify observable behavior.
pub(crate) fn promise_proto_method(name: &str) -> Option<f64> {
    unsafe { js_promise_bound_method(std::ptr::null_mut(), name) }
}

/// True iff the given promise has an own "constructor" expando property.
/// Used by primitive_methods.rs to decide whether SpeciesConstructor must run.
pub(crate) fn promise_has_own_constructor(promise_addr: usize) -> bool {
    crate::object::exotic_expando::exotic_has_own_property(
        crate::object::exotic_expando::ExoticKind::Promise,
        promise_addr,
        "constructor",
    )
}

/// True iff the given promise has an own property with the given name.
/// Used by primitive_methods.rs to decide whether to bypass the fast path.
pub(crate) fn promise_has_own_property(promise_addr: usize, name: &str) -> bool {
    crate::object::exotic_expando::exotic_has_own_property(
        crate::object::exotic_expando::ExoticKind::Promise,
        promise_addr,
        name,
    )
}

// SpeciesConstructor(promiseReceiver, %Promise%) per ECMA-262 §7.3.25.
// Reads `this.constructor` once, then `C[@@species]`, returns the resolved constructor.
// Throws TypeError for null/non-object constructor, non-constructor species, or getter throws.
fn promise_species_constructor(receiver: f64) -> f64 {
    use crate::value::{TAG_NULL, TAG_UNDEFINED};

    // Step 1: Get(receiver, "constructor") — getter throws → propagate via longjmp.
    let c = unsafe {
        crate::value::js_dynamic_object_get_property(
            receiver,
            b"constructor".as_ptr() as *const i8,
            11,
        )
    };

    // Step 2: undefined → use intrinsic %Promise%.
    if c.to_bits() == TAG_UNDEFINED {
        return get_intrinsic_promise();
    }

    // Step 3: Type(C) not Object → TypeError.
    if !is_promise_species_object(c) {
        throw_type_error_thunk("Promise.prototype.then: constructor property is not an object");
    }

    // Step 4: S = Get(C, @@species) — getter throws → propagate.
    let sp = crate::symbol::well_known_symbol("species");
    let s = if sp.is_null() {
        f64::from_bits(TAG_UNDEFINED)
    } else {
        let sym_val = f64::from_bits(crate::value::JSValue::pointer(sp as *const u8).bits());
        unsafe { crate::symbol::js_object_get_symbol_property(c, sym_val) }
    };

    // Step 5: undefined or null → use intrinsic %Promise%.
    if s.to_bits() == TAG_UNDEFINED || s.to_bits() == TAG_NULL {
        return get_intrinsic_promise();
    }

    // Step 6: IsConstructor(S) → return S.
    if crate::object::js_value_is_constructor(s) {
        return s;
    }

    // Step 7: throw TypeError.
    throw_type_error_thunk("Promise.prototype.then: species is not a constructor")
}

// ---------------------------------------------------------------------------
// PerformPromiseThen with custom capability (ECMA-262 §27.2.5.4 steps 4-12)
//
// When SpeciesConstructor returns a non-default constructor C, we create a
// PromiseCapability via NewPromiseCapability(C) and attach wrapper reactions to
// `promise` that settle cap.promise. js_promise_then is still used to get the
// native-promise side-effects (overflow-reaction tracking, already-settled
// dispatch), but the returned native `next` is discarded — cap.promise is the
// observable result.
// ---------------------------------------------------------------------------

// Fulfill reaction wrapper for PerformPromiseThen with capability.
// Captures: [on_fulfilled_f64, cap_resolve_f64, cap_reject_f64]
extern "C" fn then_cap_fulfill_fn(
    closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    use crate::closure::{js_closure_get_capture_f64, js_native_call_value};
    let on_fulfilled = js_closure_get_capture_f64(closure, 0);
    let cap_resolve = js_closure_get_capture_f64(closure, 1);
    let cap_reject = js_closure_get_capture_f64(closure, 2);
    let undef = f64::from_bits(crate::value::TAG_UNDEFINED);

    let on_ful_cl = if super::spec_combinators::is_callable_value(on_fulfilled) {
        arg_to_closure(on_fulfilled)
    } else {
        ptr::null()
    };
    let (result, threw) = if on_ful_cl.is_null() {
        (value, false)
    } else {
        let trap = crate::exception::js_try_push();
        let jumped = unsafe { crate::ffi::setjmp::setjmp(trap as *mut std::os::raw::c_int) };
        if jumped == 0 {
            let ret = crate::closure::js_closure_call1(on_ful_cl, value);
            crate::exception::js_try_end();
            (ret, false)
        } else {
            let exc = crate::exception::js_get_exception();
            crate::exception::js_clear_exception();
            crate::exception::js_try_end();
            (exc, true)
        }
    };

    let func = if threw { cap_reject } else { cap_resolve };
    let args = [result];
    let _ = unsafe { js_native_call_value(func, args.as_ptr(), 1) };
    undef
}

// Reject reaction wrapper for PerformPromiseThen with capability.
// Captures: [on_rejected_f64, cap_resolve_f64, cap_reject_f64]
extern "C" fn then_cap_reject_fn(
    closure: *const crate::closure::ClosureHeader,
    reason: f64,
) -> f64 {
    use crate::closure::{js_closure_get_capture_f64, js_native_call_value};
    let on_rejected = js_closure_get_capture_f64(closure, 0);
    let cap_resolve = js_closure_get_capture_f64(closure, 1);
    let cap_reject = js_closure_get_capture_f64(closure, 2);
    let undef = f64::from_bits(crate::value::TAG_UNDEFINED);

    let on_rej_cl = if super::spec_combinators::is_callable_value(on_rejected) {
        arg_to_closure(on_rejected)
    } else {
        ptr::null()
    };
    let (result, threw) = if on_rej_cl.is_null() {
        (reason, true) // passthrough rejection
    } else {
        let trap = crate::exception::js_try_push();
        let jumped = unsafe { crate::ffi::setjmp::setjmp(trap as *mut std::os::raw::c_int) };
        if jumped == 0 {
            let ret = crate::closure::js_closure_call1(on_rej_cl, reason);
            crate::exception::js_try_end();
            (ret, false)
        } else {
            let exc = crate::exception::js_get_exception();
            crate::exception::js_clear_exception();
            crate::exception::js_try_end();
            (exc, true)
        }
    };

    let func = if threw { cap_reject } else { cap_resolve };
    let args = [result];
    let _ = unsafe { js_native_call_value(func, args.as_ptr(), 1) };
    undef
}

// PerformPromiseThen with a custom capability (species path).
fn perform_promise_then_with_cap(
    promise: *mut Promise,
    on_fulfilled: f64,
    on_rejected: f64,
    cap_resolve: f64,
    cap_reject: f64,
    cap_promise: f64,
) -> f64 {
    use crate::closure::{js_closure_alloc, js_closure_set_capture_f64};
    let ful_wrap = js_closure_alloc(then_cap_fulfill_fn as *const u8, 3);
    js_closure_set_capture_f64(ful_wrap, 0, on_fulfilled);
    js_closure_set_capture_f64(ful_wrap, 1, cap_resolve);
    js_closure_set_capture_f64(ful_wrap, 2, cap_reject);

    let rej_wrap = js_closure_alloc(then_cap_reject_fn as *const u8, 3);
    js_closure_set_capture_f64(rej_wrap, 0, on_rejected);
    js_closure_set_capture_f64(rej_wrap, 1, cap_resolve);
    js_closure_set_capture_f64(rej_wrap, 2, cap_reject);

    // Attach handlers; discard the returned native next promise.
    let _ = js_promise_then(promise, ful_wrap, rej_wrap);
    cap_promise
}

// ---------------------------------------------------------------------------
// Spec-compliant finally closures (ECMA-262 §27.2.5.3 steps 6a–b)
//
// thenFinally(value): call onFinally(), return C.resolve(result).then(valueThunk)
// catchFinally(reason): call onFinally(), return C.resolve(result).then(thrower)
// ---------------------------------------------------------------------------

// Captures [value_f64]: ignores its argument and returns the captured value.
extern "C" fn spec_value_return_fn(
    closure: *const crate::closure::ClosureHeader,
    _arg: f64,
) -> f64 {
    crate::closure::js_closure_get_capture_f64(closure, 0)
}

// Captures [reason_f64]: ignores its argument and throws the captured reason.
extern "C" fn spec_reason_thrower_fn(
    closure: *const crate::closure::ClosureHeader,
    _arg: f64,
) -> f64 {
    let reason = crate::closure::js_closure_get_capture_f64(closure, 0);
    crate::exception::js_throw(reason)
}

// thenFinally: captures [C_f64, on_finally_f64], called with fulfilled value.
extern "C" fn spec_then_finally_fn(
    closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    use crate::closure::{
        js_closure_alloc, js_closure_get_capture_f64, js_closure_set_capture_f64,
    };
    let c = js_closure_get_capture_f64(closure, 0);
    let on_finally = js_closure_get_capture_f64(closure, 1);
    let undef = f64::from_bits(crate::value::TAG_UNDEFINED);

    // Call on_finally() with zero args; if it throws, propagate.
    let on_finally_cl = if super::spec_combinators::is_callable_value(on_finally) {
        arg_to_closure(on_finally)
    } else {
        ptr::null()
    };
    let result = if on_finally_cl.is_null() {
        undef
    } else {
        crate::closure::js_closure_call0(on_finally_cl)
    };

    // Build valueThunk = () => value
    let value_thunk_cl = js_closure_alloc(spec_value_return_fn as *const u8, 1);
    js_closure_set_capture_f64(value_thunk_cl, 0, value);
    let value_thunk =
        f64::from_bits(crate::value::JSValue::pointer(value_thunk_cl as *const u8).bits());

    // C.resolve(result) then valueThunk
    let c_resolved = crate::promise::spec_combinators::js_promise_resolve_spec(c, result);
    let args = [value_thunk, undef];
    call_receiver_then(c_resolved, &args)
}

// catchFinally: captures [C_f64, on_finally_f64], called with rejection reason.
extern "C" fn spec_catch_finally_fn(
    closure: *const crate::closure::ClosureHeader,
    reason: f64,
) -> f64 {
    use crate::closure::{
        js_closure_alloc, js_closure_get_capture_f64, js_closure_set_capture_f64,
    };
    let c = js_closure_get_capture_f64(closure, 0);
    let on_finally = js_closure_get_capture_f64(closure, 1);
    let undef = f64::from_bits(crate::value::TAG_UNDEFINED);

    // Call on_finally() with zero args; if it throws, propagate.
    let on_finally_cl = if super::spec_combinators::is_callable_value(on_finally) {
        arg_to_closure(on_finally)
    } else {
        ptr::null()
    };
    let result = if on_finally_cl.is_null() {
        undef
    } else {
        crate::closure::js_closure_call0(on_finally_cl)
    };

    // Build thrower = () => { throw reason; }
    let thrower_cl = js_closure_alloc(spec_reason_thrower_fn as *const u8, 1);
    js_closure_set_capture_f64(thrower_cl, 0, reason);
    let thrower = f64::from_bits(crate::value::JSValue::pointer(thrower_cl as *const u8).bits());

    // C.resolve(result) then thrower
    let c_resolved = crate::promise::spec_combinators::js_promise_resolve_spec(c, result);
    let args = [thrower, undef];
    call_receiver_then(c_resolved, &args)
}

fn ensure_spec_finally_arities_registered() {
    thread_local! {
        static DONE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }
    DONE.with(|d| {
        if d.get() {
            return;
        }
        d.set(true);
        use crate::closure::js_register_closure_arity;
        js_register_closure_arity(then_cap_fulfill_fn as *const u8, 1);
        js_register_closure_arity(then_cap_reject_fn as *const u8, 1);
        js_register_closure_arity(spec_then_finally_fn as *const u8, 1);
        js_register_closure_arity(spec_catch_finally_fn as *const u8, 1);
        js_register_closure_arity(spec_value_return_fn as *const u8, 1);
        js_register_closure_arity(spec_reason_thrower_fn as *const u8, 1);
    });
}

// ---------------------------------------------------------------------------
// Promise.prototype.then (ECMA-262 §27.2.5.4) — SpeciesConstructor-aware
// ---------------------------------------------------------------------------

pub(crate) extern "C" fn promise_prototype_then_thunk(
    _closure: *const crate::closure::ClosureHeader,
    on_fulfilled: f64,
    on_rejected: f64,
) -> f64 {
    ensure_spec_finally_arities_registered();
    let receiver = crate::object::js_implicit_this_get();
    if js_value_is_promise(receiver) == 0 {
        throw_promise_prototype_incompatible_receiver("then", receiver);
    }
    let promise = crate::value::js_nanbox_get_pointer(receiver) as *mut Promise;

    // SpeciesConstructor(promise, %Promise%) — reads this.constructor once.
    let c = promise_species_constructor(receiver);

    if super::spec_combinators::is_default_promise_constructor(c) {
        // Fast path: native then.
        return box_promise_ptr(js_promise_then(
            promise,
            arg_to_closure(on_fulfilled),
            arg_to_closure(on_rejected),
        ));
    }

    // Slow path: NewPromiseCapability(C) + PerformPromiseThen.
    let cap = super::spec_combinators::new_promise_capability(c);
    perform_promise_then_with_cap(
        promise,
        on_fulfilled,
        on_rejected,
        cap.resolve,
        cap.reject,
        cap.promise,
    )
}

pub(crate) extern "C" fn promise_prototype_catch_thunk(
    _closure: *const crate::closure::ClosureHeader,
    on_rejected: f64,
) -> f64 {
    let receiver = crate::object::js_implicit_this_get();
    let args = [f64::from_bits(crate::value::TAG_UNDEFINED), on_rejected];
    call_receiver_then(receiver, &args)
}

// ---------------------------------------------------------------------------
// Promise.prototype.finally (ECMA-262 §27.2.5.3) — spec-compliant
// ---------------------------------------------------------------------------

pub(crate) extern "C" fn promise_prototype_finally_thunk(
    _closure: *const crate::closure::ClosureHeader,
    on_finally: f64,
) -> f64 {
    use crate::closure::{js_closure_alloc, js_closure_set_capture_f64};
    ensure_spec_finally_arities_registered();

    let receiver = crate::object::js_implicit_this_get();

    // receiver must be a JS Object — pointer-tagged, not a registered symbol, not a handle.
    if !is_promise_species_object(receiver) {
        throw_promise_finally_non_object();
    }

    // SpeciesConstructor(receiver, %Promise%).
    let c = promise_species_constructor(receiver);
    let undef = f64::from_bits(crate::value::TAG_UNDEFINED);

    let (then_finally, catch_finally) = if !super::spec_combinators::is_callable_value(on_finally) {
        // Not callable: pass on_finally as both args (§27.2.5.3 step 5).
        (on_finally, on_finally)
    } else {
        // Build spec-compliant thenFinally and catchFinally closures.
        let tf = js_closure_alloc(spec_then_finally_fn as *const u8, 2);
        js_closure_set_capture_f64(tf, 0, c);
        js_closure_set_capture_f64(tf, 1, on_finally);

        let cf = js_closure_alloc(spec_catch_finally_fn as *const u8, 2);
        js_closure_set_capture_f64(cf, 0, c);
        js_closure_set_capture_f64(cf, 1, on_finally);

        let tf_f = f64::from_bits(crate::value::JSValue::pointer(tf as *const u8).bits());
        let cf_f = f64::from_bits(crate::value::JSValue::pointer(cf as *const u8).bits());
        (tf_f, cf_f)
    };

    // Invoke(receiver, "then", [thenFinally, catchFinally]).
    let args = [then_finally, catch_finally];
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

/// Return the `Promise.prototype[property]` closure for a promise's
/// `then`/`catch`/`finally` value-read, or `None` for any other property.
/// Returns the shared prototype method so `p.then === Promise.prototype.then`
/// (spec-required identity) and `.call(receiver)` properly receives the caller's
/// `this` via IMPLICIT_THIS rather than a captured pointer.
pub unsafe fn js_promise_bound_method(_promise: *mut Promise, property: &str) -> Option<f64> {
    if !matches!(property, "then" | "catch" | "finally") {
        return None;
    }
    let ctor = get_intrinsic_promise();
    let ctor_bits = ctor.to_bits();
    if (ctor_bits & crate::value::TAG_MASK) != crate::value::POINTER_TAG {
        return None;
    }
    let ctor_ptr = (ctor_bits & crate::value::POINTER_MASK) as usize;
    if ctor_ptr == 0 {
        return None;
    }
    let proto_val = crate::closure::closure_get_dynamic_prop(ctor_ptr, "prototype");
    if proto_val.to_bits() == crate::value::TAG_UNDEFINED {
        return None;
    }
    let method = crate::value::js_dynamic_object_get_property(
        proto_val,
        property.as_ptr() as *const i8,
        property.len(),
    );
    if method.to_bits() == crate::value::TAG_UNDEFINED {
        return None;
    }
    Some(method)
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
    // Spec (Promise.prototype.finally): `onFinally` is invoked with NO
    // arguments. Calling it with a single `undefined` made `arguments.length`
    // report 1, failing every finally test that asserts a zero-arg invocation.
    let ret = crate::closure::js_closure_call0(on_finally);
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
            // Use Invoke(cleanup, "then", …) to respect any user-installed own
            // `then` property on the cleanup promise (observable-then-calls tests).
            // Wrap in a try-frame: call_receiver_then can throw (e.g. non-callable
            // `then`, or a getter that throws) after the earlier frame has ended.
            let on_ok_f = f64::from_bits(crate::value::JSValue::pointer(on_ok as *const u8).bits());
            let on_err_f =
                f64::from_bits(crate::value::JSValue::pointer(on_err as *const u8).bits());
            let args = [on_ok_f, on_err_f];
            let trap2 = crate::exception::js_try_push();
            let jumped2 = unsafe { crate::ffi::setjmp::setjmp(trap2 as *mut std::os::raw::c_int) };
            if jumped2 != 0 {
                let exc = crate::exception::js_get_exception();
                crate::exception::js_clear_exception();
                crate::exception::js_try_end();
                if !next.is_null() {
                    js_promise_reject(next, exc);
                }
                return undef;
            }
            call_receiver_then(cleanup, &args);
            crate::exception::js_try_end();
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
