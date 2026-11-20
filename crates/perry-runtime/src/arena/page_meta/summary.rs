use super::*;

pub(crate) fn old_page_summary() -> OldPageSummary {
    // #7624 READER (`OLD_GEN_PAGE_META`): a deferred registration also owes
    // this table an `allocated_bytes`/`object_count` update, so the summary
    // would under-report a mid-cycle promotion burst without the flush.
    flush_deferred_old_page_registrations();
    let current_epoch = old_gen_page_dirty_epoch();
    OLD_GEN_PAGE_META.with(|meta| {
        let meta = meta.borrow();
        let mut summary = OldPageSummary {
            pages: meta.len(),
            ..OldPageSummary::default()
        };
        for page_meta in meta.values() {
            summary.allocated_bytes = summary
                .allocated_bytes
                .saturating_add(page_meta.allocated_bytes as usize);
            summary.live_bytes = summary
                .live_bytes
                .saturating_add(page_meta.live_bytes as usize);
            summary.dead_bytes = summary
                .dead_bytes
                .saturating_add(page_meta.dead_bytes as usize);
            summary.pinned_bytes = summary
                .pinned_bytes
                .saturating_add(page_meta.pinned_bytes as usize);
            summary.object_count = summary
                .object_count
                .saturating_add(page_meta.object_count as usize);
            summary.live_object_count = summary
                .live_object_count
                .saturating_add(page_meta.live_object_count as usize);
            summary.dead_object_count = summary
                .dead_object_count
                .saturating_add(page_meta.dead_object_count as usize);
            summary.pinned_object_count = summary
                .pinned_object_count
                .saturating_add(page_meta.pinned_object_count as usize);
            let dirty_slots = page_meta.effective_dirty_slots(current_epoch);
            if page_meta.dirty || dirty_slots > 0 {
                summary.dirty_pages = summary.dirty_pages.saturating_add(1);
            }
            summary.dirty_slots = summary.dirty_slots.saturating_add(dirty_slots);
            if page_meta.live_bytes > 0 && page_meta.dead_bytes > 0 {
                summary.fragmented_pages = summary.fragmented_pages.saturating_add(1);
            }
            if page_meta.evacuation_eligible {
                summary.evacuation_eligible_pages =
                    summary.evacuation_eligible_pages.saturating_add(1);
            }
        }
        summary.reusable_bytes = OLD_GEN_RECLAIM_REUSABLE_BYTES.with(|bytes| bytes.get());
        summary.pooled_bytes = OLD_GEN_RECLAIM_POOLED_BYTES.with(|bytes| bytes.get());
        summary.returned_bytes = OLD_GEN_RECLAIM_RETURNED_BYTES.with(|bytes| bytes.get());
        summary
    })
}
