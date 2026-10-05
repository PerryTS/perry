//! #11471: thread-exit release of `async_hooks`' process-global tables.

use std::sync::atomic::Ordering;

use super::{
    HookCallbacks, ASYNC_WRAP_PROVIDERS, CONTEXT_SNAPSHOTS, HOOKS, HOOKS_ACTIVE,
    PROMISE_HOOKS_ACTIVE, RESOURCES, TOP_LEVEL_RESOURCE,
};

/// #11471: release every async_hooks entry naming an exiting thread's arena.
///
/// All six tables are process-global (`per_test_global!` is a plain static
/// outside perry-runtime's own unit tests) while their writers run on any
/// thread, and none is otherwise told about a thread exit:
///
/// * `HOOKS` — a `createHook` record whose callback is freed is disabled
///   exactly as `set_hook_enabled(_, false)` does (keeping `HOOKS_ACTIVE` /
///   `PROMISE_HOOKS_ACTIVE` consistent) and its callbacks nulled. The record
///   stays: the AsyncHook payload names it by index.
/// * `RESOURCES` — an entry whose resource value or captured context store is
///   freed is dropped without a destroy event (the thread that could observe
///   it is gone).
/// * `CONTEXT_SNAPSHOTS` — a snapshot holding a freed store is dropped; only
///   the dead thread's `AsyncLocalStorage.snapshot()` closures named it.
/// * `TOP_LEVEL_RESOURCE` / `ASYNC_WRAP_PROVIDERS` — lazily allocated by the
///   first thread to ask; cleared if that thread's arena is going away, so the
///   next caller allocates a fresh one on its own heap.
pub(crate) fn release_async_hooks_in_freed_ranges(freed: &crate::arena::thread_exit::FreedRanges) {
    let snapshot_is_dead = |snapshot: &crate::async_context::AsyncContextSnapshot| {
        let mut dead = false;
        crate::async_context::scan_snapshot_roots(snapshot, &mut |store| {
            dead |= freed.holds_value(store);
        });
        dead
    };
    {
        let mut hooks = HOOKS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for record in hooks.iter_mut() {
            let c = record.callbacks;
            let dead = [c.init, c.before, c.after, c.destroy, c.promise_resolve]
                .iter()
                .any(|callback| freed.contains(*callback as usize));
            if !dead {
                continue;
            }
            if record.enabled && c.has_any() {
                HOOKS_ACTIVE.fetch_sub(1, Ordering::Relaxed);
                if record.track_promises {
                    PROMISE_HOOKS_ACTIVE.fetch_sub(1, Ordering::Relaxed);
                }
            }
            record.enabled = false;
            record.callbacks = HookCallbacks::empty();
        }
    }
    RESOURCES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .retain(|_, meta| !freed.holds_value(meta.resource) && !snapshot_is_dead(&meta.context));
    CONTEXT_SNAPSHOTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .retain(|_, snapshot| !snapshot_is_dead(snapshot));
    for slot in [&ASYNC_WRAP_PROVIDERS, &TOP_LEVEL_RESOURCE] {
        let bits = slot.load(Ordering::Acquire);
        if bits != 0 && freed.holds_bits(bits) {
            let _ = slot.compare_exchange(bits, 0, Ordering::AcqRel, Ordering::Acquire);
        }
    }
}

/// #11471 test probe: the cached `executionAsyncResource()` bootstrap object
/// and `asyncWrapProviders` bits (0 when unset).
#[doc(hidden)]
pub fn cached_singletons_for_test() -> (u64, u64) {
    (
        TOP_LEVEL_RESOURCE.load(Ordering::Acquire),
        ASYNC_WRAP_PROVIDERS.load(Ordering::Acquire),
    )
}

/// #11471 test probe: forget both lazily cached singletons, so the calling
/// thread allocates its own on next use.
#[doc(hidden)]
pub fn clear_cached_singletons_for_test() {
    TOP_LEVEL_RESOURCE.store(0, Ordering::Release);
    ASYNC_WRAP_PROVIDERS.store(0, Ordering::Release);
}

/// #11471 test probe: does any `createHook` record hold `callback`?
#[doc(hidden)]
pub fn hook_callback_registered_for_test(callback: usize) -> bool {
    HOOKS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .any(|record| {
            let c = record.callbacks;
            [c.init, c.before, c.after, c.destroy, c.promise_resolve]
                .iter()
                .any(|p| *p as usize == callback)
        })
}

/// #11471 test probe: is `async_id` tracked in `RESOURCES`?
#[doc(hidden)]
pub fn resource_tracked_for_test(async_id: u64) -> bool {
    RESOURCES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .contains_key(&async_id)
}

/// #11471 test probe: does any registered context snapshot hold a store with
/// these bits?
#[doc(hidden)]
pub fn context_snapshot_holds_for_test(store_bits: u64) -> bool {
    CONTEXT_SNAPSHOTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .values()
        .any(|snapshot| {
            let mut found = false;
            crate::async_context::scan_snapshot_roots(snapshot, &mut |store| {
                found |= store.to_bits() == store_bits;
            });
            found
        })
}
