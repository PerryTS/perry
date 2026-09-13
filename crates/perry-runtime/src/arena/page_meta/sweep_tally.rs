//! Per-page batching of a sweep's old-generation page accounting (#10182).
//!
//! `old_page_account_swept_object` costs one heap-allocated overlap vector and
//! one page-meta hash lookup per swept old object. A full sweep over a live
//! 20 MB JSON tree calls it ~585k times, and it was about three quarters of the
//! sweep. The sweep walks each block in address order, so consecutive objects
//! share a page: summing their deltas and applying the sum once per page gives
//! the same page meta, because every field involved is a plain sum and
//! `refresh_policy_bits` is a pure recompute of the page's own fields.
//!
//! The one interleaving that could observe the difference is the batched
//! page-index removal (`unregister_old_objects_batch`), which zeroes a page's
//! sweep accounting when its last object leaves. The sweep therefore applies
//! its tally before every such flush, which keeps the order of resets and sums
//! on every page exactly as the one-by-one calls had it.

use super::*;

/// Sweep accounting deltas of consecutive old objects that each lie wholly on
/// one page.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct OldPageSweepTally {
    pub(crate) live_bytes: usize,
    pub(crate) live_objects: usize,
    pub(crate) pinned_bytes: usize,
    pub(crate) pinned_objects: usize,
    pub(crate) dead_bytes: usize,
    pub(crate) dead_objects: usize,
}

impl OldPageSweepTally {
    #[inline(always)]
    pub(crate) fn add(&mut self, total_size: usize, live: bool, pinned: bool) {
        if live {
            self.live_bytes += total_size;
            self.live_objects += 1;
            if pinned {
                self.pinned_bytes += total_size;
                self.pinned_objects += 1;
            }
        } else {
            self.dead_bytes += total_size;
            self.dead_objects += 1;
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.live_objects == 0 && self.dead_objects == 0
    }
}

/// The page an old object lies on when it lies wholly on one page.
#[inline(always)]
pub(crate) fn old_object_single_page(header_addr: usize, total_size: usize) -> Option<usize> {
    if header_addr == 0 || total_size == 0 {
        return None;
    }
    let first_page = generation_page_for_addr(header_addr);
    (first_page == generation_page_for_addr(header_addr + total_size - 1)).then_some(first_page)
}

/// Apply `tally` to `page` exactly as the same objects' one-by-one
/// `old_page_account_swept_object` calls would have.
pub(crate) fn old_page_account_swept_tally(page: usize, tally: &OldPageSweepTally) {
    if tally.is_empty() {
        return;
    }
    OLD_GEN_PAGE_META.with(|meta| {
        let mut meta = meta.borrow_mut();
        let page_meta = meta
            .entry(page)
            .or_insert_with(|| OldPageMeta::zero_for_page(page));
        page_meta.live_bytes = page_meta.live_bytes.saturating_add(tally.live_bytes);
        page_meta.live_object_count = page_meta
            .live_object_count
            .saturating_add(tally.live_objects);
        page_meta.pinned_bytes = page_meta.pinned_bytes.saturating_add(tally.pinned_bytes);
        page_meta.pinned_object_count = page_meta
            .pinned_object_count
            .saturating_add(tally.pinned_objects);
        page_meta.dead_bytes = page_meta.dead_bytes.saturating_add(tally.dead_bytes);
        page_meta.dead_object_count = page_meta
            .dead_object_count
            .saturating_add(tally.dead_objects);
        page_meta.refresh_policy_bits();
    });
}
