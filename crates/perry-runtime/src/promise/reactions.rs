//! Promise reaction side tables: settle listeners (internal
//! "run-this-when-settled" callbacks attached by the async machinery) and
//! overflow reactions (2nd+ `.then`/`.catch`/`.finally` reactions on the same
//! promise — the `Promise` struct holds only ONE inline reaction triple).
//! Extracted from `then.rs` to keep that file under the 2000-line cap;
//! behavior is unchanged. Both tables are GC-scanned roots with death hooks
//! (`remove_*_for_dead_promise`) and copied-minor rekey/drop cleanup.

use super::*;

use super::keyed_table::PromiseKeyedTable;

pub(super) struct PromiseSettleListener {
    pub(super) on_fulfilled: ClosurePtr,
    pub(super) on_rejected: ClosurePtr,
    pub(super) context: AsyncContextSnapshot,
}

thread_local! {
    /// Keyed by pending-promise address — see `keyed_table.rs` (#6084 item 2:
    /// this used to be a raw `Vec` that every settlement scanned end to end).
    pub(super) static PROMISE_SETTLE_LISTENERS: RefCell<PromiseKeyedTable<PromiseSettleListener>> =
        const { RefCell::new(PromiseKeyedTable::new()) };
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
                    listeners.borrow_mut().push(
                        promise as usize,
                        PromiseSettleListener {
                            on_fulfilled,
                            on_rejected,
                            context,
                        },
                    );
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

pub(super) fn promise_take_settle_listeners(promise: *mut Promise) -> Vec<PromiseSettleListener> {
    if promise.is_null() {
        return Vec::new();
    }
    PROMISE_SETTLE_LISTENERS.with(|listeners| listeners.borrow_mut().take_all(promise as usize))
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
        let mut listeners = listeners.borrow_mut();
        let mut rekeyed = false;
        for entry in listeners.iter_mut() {
            rekeyed |= visitor.visit_metadata_usize_slot(&mut entry.key);
            visitor.visit_raw_const_ptr_slot(&mut entry.value.on_fulfilled);
            visitor.visit_raw_const_ptr_slot(&mut entry.value.on_rejected);
            scan_snapshot_roots_mut(&mut entry.value.context, visitor);
        }
        if rekeyed {
            listeners.note_key_rewritten();
        }
    });
}

/// GC death hook: `promise` died in a sweep, so listeners parked against it
/// can never fire — drop them so their strongly-rooted closures/contexts
/// become collectible (2026-07-09 GC audit, mirrors `PROMISE_CONTEXTS`).
pub(super) fn remove_settle_listeners_for_dead_promise(promise: *mut Promise) {
    if promise.is_null() {
        return;
    }
    let key = promise as usize;
    PROMISE_SETTLE_LISTENERS.with(|listeners| listeners.borrow_mut().remove_key(key));
}

/// Copied-minor from-space cleanup for the settle-listener table: drop entries
/// keyed by dead from-space promises, rekey any the scanners missed. Order-
/// preserving (`retain_mut`) so live listeners keep registration order.
pub(super) fn cleanup_copied_minor_settle_listeners_for_gc() {
    use super::CopiedMinorPromiseKeyFate::*;
    PROMISE_SETTLE_LISTENERS.with(|listeners| {
        listeners.borrow_mut().retain_mut(|entry| {
            match super::copied_minor_promise_key_fate(entry.key) {
                Keep => true,
                Rekey(new_key) => {
                    entry.key = new_key;
                    true
                }
                Drop => false,
            }
        });
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
    /// Keyed by pending-promise address — see `keyed_table.rs` (#6084 item 2:
    /// this used to be a raw `Vec` that every settlement scanned end to end).
    pub(super) static PROMISE_OVERFLOW_REACTIONS: RefCell<PromiseKeyedTable<OverflowReaction>> =
        const { RefCell::new(PromiseKeyedTable::new()) };
}

/// Park a 2nd+ reaction on a still-pending `promise`.
pub(super) fn push_overflow_reaction(
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
        r.borrow_mut().push(
            promise as usize,
            OverflowReaction {
                on_fulfilled,
                on_rejected,
                next,
                context,
            },
        );
    });
}

/// Drain (in registration order) every overflow reaction registered against
/// `promise`. Returns `Vec::new()` for the overwhelmingly common no-overflow
/// case without touching the table's allocation.
pub(super) fn promise_take_overflow_reactions(promise: *mut Promise) -> Vec<OverflowReaction> {
    // FIFO order is observable (`p.then(a); p.then(b)` must run a before b).
    // `take_all` restores it from each entry's registration seq — the old
    // order-preserving `retain` had to touch every entry in the table, which is
    // what made settling N promises O(N²) (#6084 item 2).
    PROMISE_OVERFLOW_REACTIONS.with(|r| r.borrow_mut().take_all(promise as usize))
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
        q.push_back(Task::Inline(cb, value, r.next, is_fulfilled, r.context));
    }
}

pub(super) fn scan_promise_overflow_reactions_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    PROMISE_OVERFLOW_REACTIONS.with(|reactions| {
        let mut reactions = reactions.borrow_mut();
        let mut rekeyed = false;
        for entry in reactions.iter_mut() {
            rekeyed |= visitor.visit_metadata_usize_slot(&mut entry.key);
            visitor.visit_raw_const_ptr_slot(&mut entry.value.on_fulfilled);
            visitor.visit_raw_const_ptr_slot(&mut entry.value.on_rejected);
            visitor.visit_raw_mut_ptr_slot(&mut entry.value.next);
            scan_snapshot_roots_mut(&mut entry.value.context, visitor);
        }
        if rekeyed {
            reactions.note_key_rewritten();
        }
    });
}

/// GC death hook: `promise` died in a sweep — its parked 2nd+ reactions can
/// never replay, so drop them (their chained `next` promises and closures
/// become collectible). FIFO order of OTHER promises' reactions is preserved
/// (`retain`). 2026-07-09 GC audit, mirrors `PROMISE_CONTEXTS`.
pub(super) fn remove_overflow_reactions_for_dead_promise(promise: *mut Promise) {
    if promise.is_null() {
        return;
    }
    let key = promise as usize;
    PROMISE_OVERFLOW_REACTIONS.with(|reactions| reactions.borrow_mut().remove_key(key));
}

/// Copied-minor from-space cleanup for the overflow-reaction table — see
/// `cleanup_copied_minor_settle_listeners_for_gc`. Order-preserving so live
/// promises' reactions still replay in registration (FIFO) order.
pub(super) fn cleanup_copied_minor_overflow_reactions_for_gc() {
    use super::CopiedMinorPromiseKeyFate::*;
    PROMISE_OVERFLOW_REACTIONS.with(|reactions| {
        reactions.borrow_mut().retain_mut(|entry| {
            match super::copied_minor_promise_key_fate(entry.key) {
                Keep => true,
                Rekey(new_key) => {
                    entry.key = new_key;
                    true
                }
                Drop => false,
            }
        });
    });
}
