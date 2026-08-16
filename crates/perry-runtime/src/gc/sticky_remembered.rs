//! The copied minor's sticky remembered-set buffer.
//!
//! A copying minor clears the remembered set and rebuilds it from what the
//! cycle observed. Entries discovered during the scan are buffered here rather
//! than written straight through, because the write would be undone by the
//! clear; `restore` replays them after it.
//!
//! Split out of `gc::copying` for the 2000-line file cap; it is a
//! self-contained buffer with no cycle state.

use super::barrier::{mark_dirty_external_slot_page, mark_dirty_old_page};
use super::GcHeader;

#[derive(Default)]
pub(super) struct StickyRememberedSet {
    pub(super) old_pages: crate::fast_hash::PtrHashSet<usize>,
    pub(super) external_pages: Vec<(usize, usize)>,
}

impl StickyRememberedSet {
    pub(super) fn remember_slot(
        &mut self,
        parent_header: *mut GcHeader,
        slot: *mut u64,
        external: bool,
    ) {
        if parent_header.is_null() || slot.is_null() {
            return;
        }
        let page = crate::arena::generation_page_for_addr(slot as usize);
        if external {
            // #7538: an owner's external buffer can contribute thousands of
            // slots (a lazy JSON array's sparse element cache is one 8-byte
            // slot per element), and they are visited in address order — so
            // one adjacent-duplicate check collapses a whole page's worth of
            // pushes into a single entry. `restore` dedupes again inside
            // `mark_dirty_external_slot_page`; this keeps the intermediate
            // Vec from growing with the element count.
            let entry = (parent_header as usize, page);
            if self.external_pages.last() != Some(&entry) {
                self.external_pages.push(entry);
            }
        } else {
            self.old_pages.insert(page);
        }
    }

    pub(super) fn restore(&self) {
        for &page in &self.old_pages {
            mark_dirty_old_page(page);
        }
        for &(header, page) in &self.external_pages {
            mark_dirty_external_slot_page(header, page);
        }
    }

    pub(super) fn extend(&mut self, other: StickyRememberedSet) {
        self.old_pages.extend(other.old_pages);
        self.external_pages.extend(other.external_pages);
    }
}
