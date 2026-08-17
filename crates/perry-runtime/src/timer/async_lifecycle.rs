//! Small async-hooks helpers kept outside `timer.rs`'s 2,000-line CI cap.

pub(super) type IntervalCallback = (
    i64,
    i64,
    Vec<f64>,
    crate::async_context::AsyncContextSnapshot,
    u64,
    u64,
);

/// Timer destruction is deferred in Node. This matters most when an interval
/// clears itself: the current `after` event must precede `destroy`.
pub(super) fn enqueue_destroy_ids(ids: [Option<u64>; 2]) {
    for async_id in ids.into_iter().flatten().filter(|id| *id != 0) {
        crate::async_hooks::enqueue_gc_destroy(async_id);
    }
}
