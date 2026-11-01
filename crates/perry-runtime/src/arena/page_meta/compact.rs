//! Compact storage for facts about one 4 KiB old-generation page.
//!
//! Addresses belong to the map key, not every value. Byte/object counters
//! describe overlaps with this page, not whole multi-page allocations. Keep
//! dirty-slot counts and epochs wide: unlike occupancy, scan work need not be
//! bounded by the physical page size. Public snapshots retain their usize ABI.

use super::{generation_page_base, OldPageMeta, GENERATION_PAGE_SIZE};

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct StoredPageMeta {
    pub(super) allocated_bytes: u32,
    pub(super) live_bytes: u32,
    pub(super) dead_bytes: u32,
    pub(super) object_count: u32,
    pub(super) live_object_count: u32,
    pub(super) dead_object_count: u32,
    pub(super) pinned_bytes: u32,
    pub(super) pinned_object_count: u32,
    pub(super) dirty_slots: usize,
    pub(super) dirty_slots_epoch: u64,
    pub(super) dirty: bool,
    pub(super) evacuation_eligible: bool,
}

/// Narrow page-local deltas with a checked conversion, never truncation.
#[inline]
pub(super) fn page_count(value: usize) -> u32 {
    u32::try_from(value).expect("old-page occupancy delta exceeds u32")
}

impl StoredPageMeta {
    #[inline]
    pub(super) fn effective_dirty_slots(&self, epoch: u64) -> usize {
        if self.dirty_slots_epoch == epoch {
            self.dirty_slots
        } else {
            0
        }
    }

    #[inline]
    pub(super) fn reset_cycle_sweep_accounting(&mut self) {
        self.live_bytes = 0;
        self.dead_bytes = 0;
        self.pinned_bytes = 0;
        self.live_object_count = 0;
        self.dead_object_count = 0;
        self.pinned_object_count = 0;
        self.evacuation_eligible = false;
    }

    #[inline]
    pub(super) fn refresh_policy_bits(&mut self) {
        self.evacuation_eligible = self.allocated_bytes > 0
            && self.live_bytes > 0
            && self.dead_bytes > 0
            && self.pinned_bytes == 0;
    }

    pub(super) fn snapshot(self, page: usize, epoch: u64) -> OldPageMeta {
        let page_base = generation_page_base(page);
        OldPageMeta {
            page_base,
            page_end: page_base + GENERATION_PAGE_SIZE,
            allocated_bytes: self.allocated_bytes as usize,
            live_bytes: self.live_bytes as usize,
            dead_bytes: self.dead_bytes as usize,
            object_count: self.object_count as usize,
            live_object_count: self.live_object_count as usize,
            dead_object_count: self.dead_object_count as usize,
            pinned_bytes: self.pinned_bytes as usize,
            pinned_object_count: self.pinned_object_count as usize,
            dirty_slots: self.effective_dirty_slots(epoch),
            dirty_slots_epoch: epoch,
            dirty: self.dirty,
            evacuation_eligible: self.evacuation_eligible,
        }
    }
}

/// Preserve header order while storing in-page addresses as two-byte offsets.
/// At most one non-overlapping allocation can enter a page from below. Its
/// full address is kept separately, so even a multi-gigabyte object is exact.
/// One entering header uses the existing address word when the vector is
/// empty. Other headers use compact offsets; an entering sentinel preserves
/// insertion order when a spanning object's tail shares a page.
#[derive(Default)]
pub(super) struct PageObjects {
    offsets: Vec<u16>,
    entering_header: usize,
}

const ENTERING: u16 = u16::MAX;
const _: () = assert!(GENERATION_PAGE_SIZE < ENTERING as usize);

impl PageObjects {
    pub(super) fn with_capacity(capacity: usize) -> Self {
        Self {
            offsets: if capacity > 1 {
                Vec::with_capacity(capacity)
            } else {
                Vec::new()
            },
            entering_header: 0,
        }
    }

    #[inline]
    fn decode(&self, page: usize, offset: u16) -> usize {
        if offset == ENTERING {
            self.entering_header
        } else {
            generation_page_base(page) + offset as usize
        }
    }

    pub(super) fn push(&mut self, page: usize, header: usize) {
        let base = generation_page_base(page);
        let offset = if header < base {
            assert!(
                header != 0 && (self.entering_header == 0 || self.entering_header == header),
                "two different objects enter the same page"
            );
            if self.offsets.is_empty() && self.offsets.capacity() == 0 && self.entering_header == 0
            {
                // A spanning object's entering header uses the address word
                // already present in this page index; dense pages keep their
                // ordinary vector push path.
                self.entering_header = header;
                return;
            }
            self.entering_header = header;
            ENTERING
        } else {
            let offset = header - base;
            assert!(
                offset < GENERATION_PAGE_SIZE,
                "header starts beyond its indexed page"
            );
            offset as u16
        };
        self.push_offset(offset);
    }

    #[inline(always)]
    fn push_offset(&mut self, offset: u16) {
        if self.offsets.len() == self.offsets.capacity() {
            self.reserve_for_push();
        }
        // SAFETY: the cold reserve ensures spare capacity and materializes a
        // previous inline entering header. This is Vec::push's ordinary fast
        // path, with its length reloaded only across that cold reservation.
        unsafe {
            let len = self.offsets.len();
            // GC_STORE_AUDIT(POINTER_FREE): u16 page offsets (or ENTERING), never heap pointers.
            self.offsets.as_mut_ptr().add(len).write(offset);
            self.offsets.set_len(len + 1);
        }
    }

    #[cold]
    #[optimize(size)]
    #[inline(never)]
    fn reserve_for_push(&mut self) {
        let inline = self.offsets.is_empty() && self.entering_header != 0;
        self.offsets.reserve(1 + usize::from(inline));
        if inline {
            self.offsets.push(ENTERING);
        }
    }

    pub(super) fn extend(&mut self, page: usize, headers: &[usize]) {
        let inline = self.offsets.is_empty() && self.entering_header != 0;
        let additional = headers.len() + usize::from(inline);
        if additional > 1 {
            self.offsets.reserve(additional);
            if inline {
                self.offsets.push(ENTERING);
            }
        }
        for &header in headers {
            self.push(page, header);
        }
    }

    pub(super) fn contains_prefix(&self, page: usize, header: usize, len: usize) -> bool {
        if len == 0 {
            return false;
        }
        let base = generation_page_base(page);
        let offset = if header < base {
            if header != self.entering_header {
                return false;
            }
            if self.offsets.is_empty() {
                return len != 0 && self.entering_header != 0;
            }
            ENTERING
        } else {
            let offset = header - base;
            if offset >= GENERATION_PAGE_SIZE {
                return false;
            }
            offset as u16
        };
        self.offsets[..len.min(self.offsets.len())].contains(&offset)
    }

    pub(super) fn iter(&self, page: usize) -> impl Iterator<Item = usize> + '_ {
        let offsets = if self.offsets.is_empty() && self.entering_header != 0 {
            std::slice::from_ref(&ENTERING)
        } else {
            self.offsets.as_slice()
        };
        offsets.iter().map(move |&offset| self.decode(page, offset))
    }

    pub(super) fn get(&self, page: usize, index: usize) -> Option<usize> {
        if let Some(&offset) = self.offsets.get(index) {
            return Some(self.decode(page, offset));
        }
        (index == 0 && self.offsets.is_empty() && self.entering_header != 0)
            .then_some(self.entering_header)
    }

    pub(super) fn retain(&mut self, page: usize, mut keep: impl FnMut(usize) -> bool) {
        if self.offsets.is_empty() {
            if self.entering_header != 0 && !keep(self.entering_header) {
                self.entering_header = 0;
            }
            return;
        }
        let base = generation_page_base(page);
        let entering = self.entering_header;
        let mut kept_entering = false;
        self.offsets.retain(|&offset| {
            let result = keep(if offset == ENTERING {
                entering
            } else {
                base + offset as usize
            });
            kept_entering |= result && offset == ENTERING;
            result
        });
        if !kept_entering {
            self.entering_header = 0;
        }
    }

    pub(super) fn swap_remove(&mut self, index: usize) {
        if self.offsets.is_empty() {
            assert!(
                index == 0 && self.entering_header != 0,
                "page object index out of bounds"
            );
            self.entering_header = 0;
            return;
        }
        if self.offsets.swap_remove(index) == ENTERING && !self.offsets.contains(&ENTERING) {
            self.entering_header = 0;
        }
    }

    pub(super) fn len(&self) -> usize {
        self.offsets
            .len()
            .max(usize::from(self.entering_header != 0))
    }
    pub(super) fn is_empty(&self) -> bool {
        self.offsets.is_empty() && self.entering_header == 0
    }
    pub(super) fn heap_bytes(&self) -> usize {
        self.offsets.capacity() * size_of::<u16>()
    }
}

#[cfg(test)]
mod tests;
