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

    /// [`Self::restore`], reporting how many entries were NOT already in the
    /// remembered set — the pages this restore genuinely added (#9754).
    pub(super) fn restore_counted(&self) -> usize {
        let mut added = 0usize;
        for &page in &self.old_pages {
            // `mark_dirty_old_page`'s return is not "inserted" (its uncached
            // arm answers the ever-dirty question), so ask the set first.
            let already = super::barrier::DIRTY_OLD_PAGES.with(|s| s.borrow().contains(&page));
            mark_dirty_old_page(page);
            if !already {
                added += 1;
            }
        }
        for &(header, page) in &self.external_pages {
            if mark_dirty_external_slot_page(header, page) {
                added += 1;
            }
        }
        added
    }

    /// How many of this set's entries the remembered set does NOT hold yet —
    /// what `restore` would add. Read-only: the debug check of the coverage
    /// restore asks this about objects it skipped.
    #[cfg(debug_assertions)]
    pub(super) fn count_not_yet_dirty(&self) -> usize {
        let old_missing = super::barrier::DIRTY_OLD_PAGES.with(|s| {
            let s = s.borrow();
            self.old_pages.iter().filter(|page| !s.contains(page)).count()
        });
        let external_missing = super::barrier::EXTERNAL_DIRTY_SLOT_PAGES.with(|s| {
            let s = s.borrow();
            self.external_pages
                .iter()
                .filter(|(header, page)| {
                    !s.get(page).is_some_and(|headers| headers.contains(header))
                })
                .count()
        });
        old_missing + external_missing
    }

    pub(super) fn extend(&mut self, other: StickyRememberedSet) {
        self.old_pages.extend(other.old_pages);
        self.external_pages.extend(other.external_pages);
    }
}
