//! #11471: thread-exit release of `async_hooks`' process-global tables.

use std::sync::atomic::Ordering;

use super::{
    HookCallbacks, ASYNC_WRAP_PROVIDERS, HOOKS, HOOKS_ACTIVE, PROMISE_HOOKS_ACTIVE,
    TOP_LEVEL_RESOURCE,
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
/// * `TOP_LEVEL_RESOURCE` / `ASYNC_WRAP_PROVIDERS` — lazily allocated by the
///   first thread to ask; cleared if that thread's arena is going away, so the
///   next caller allocates a fresh one on its own heap.
pub(crate) fn release_async_hooks_in_freed_ranges(freed: &crate::arena::thread_exit::FreedRanges) {
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

/// #11471 test probe: is `async_id` owned by a live metadata cell?
#[doc(hidden)]
pub fn resource_tracked_for_test(async_id: u64) -> bool {
    super::resource_state::get(async_id).is_some_and(|meta| unsafe { !(*meta).destroyed })
}
