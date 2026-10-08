//! Remembered-set root marking: the dirty-slot work items, the per-header
//! incremental slot scan, `dirty_slot_ranges_for`, and the budgeted
//! `RememberedSetRootMarkState` machine that drives them.
//!
//! Split out of `barrier/mod.rs` to keep that file under the 2,000-line cap
//! (#10750); pure relocation.

use super::*;

struct DirtySlotRangeWork {
    slots: *mut u64,
    cursor: usize,
    end: usize,
    layout_kind: Option<HeapChildSlotReadKind>,
    range_started: bool,
}

enum DirtySlotWork {
    Single {
        slot: GcMutableSlot,
        layout_kind: Option<HeapChildSlotReadKind>,
    },
    Range(DirtySlotRangeWork),
}

struct DirtyHeaderSlotScan {
    header: *mut GcHeader,
    user_ptr: usize,
    work: Vec<DirtySlotWork>,
    cursor: usize,
    changed: bool,
}

impl DirtyHeaderSlotScan {
    unsafe fn new(
        header: *mut GcHeader,
        dirty_pages: &crate::fast_hash::PtrHashSet<usize>,
        valid_ptrs: &ValidPointerSet,
        stats: &mut RememberedSetTraceStats,
    ) -> Option<Self> {
        let total_size = (*header).size as usize;
        if total_size == 0 || (*header).gc_flags & GC_FLAG_FORWARDED != 0 {
            return None;
        }
        let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE) as usize;
        if !valid_ptrs.contains(&user_ptr) {
            return None;
        }

        stats.old_objects_considered += 1;
        stats.valid_roots += 1;
        stats.dirty_objects_scanned += 1;

        if (*header).obj_type == GC_TYPE_WEAK_STORAGE {
            crate::gc::ephemeron::discover(header);
            return Some(Self {
                header,
                user_ptr,
                work: Vec::new(),
                cursor: 0,
                changed: false,
            });
        }
        let mut work = Vec::new();
        visit_gc_rewrite_slot_descriptors(header, |descriptor| match descriptor {
            GcMutableSlotDescriptor::Slot(slot) => {
                if dirty_pages_contains_addr(dirty_pages, slot.slot as usize) {
                    work.push(DirtySlotWork::Single {
                        slot,
                        layout_kind: slot.layout_kind,
                    });
                }
            }
            GcMutableSlotDescriptor::Range { range, layout_kind } => {
                // Preserve single-slot scan semantics for every dirty range
                // entry: weak-target skip, layout tracking, accounting, visit.
                for (start, end) in dirty_slot_ranges_for(range, dirty_pages, stats) {
                    work.push(DirtySlotWork::Range(DirtySlotRangeWork {
                        slots: range.slots(),
                        cursor: start,
                        end,
                        layout_kind,
                        range_started: false,
                    }));
                }
            }
            GcMutableSlotDescriptor::PointerFreeRange(_) => {}
        });

        Some(Self {
            header,
            user_ptr,
            work,
            cursor: 0,
            changed: false,
        })
    }

    fn step(
        &mut self,
        remaining: &mut usize,
        stats: &mut RememberedSetTraceStats,
        visit_slot: &mut dyn FnMut(GcMutableSlot, &mut RememberedSetTraceStats),
    ) -> bool {
        while *remaining > 0 && self.cursor < self.work.len() {
            match &mut self.work[self.cursor] {
                DirtySlotWork::Single { slot, layout_kind } => unsafe {
                    if !crate::weakref::is_weak_target_trace_slot(self.header, slot.slot) {
                        process_dirty_slot_work(
                            *slot,
                            *layout_kind,
                            stats,
                            visit_slot,
                            &mut self.changed,
                        );
                    }
                    self.cursor += 1;
                    *remaining -= 1;
                },
                DirtySlotWork::Range(range) => unsafe {
                    if !range.range_started {
                        stats.dirty_slot_ranges_scanned += 1;
                        range.range_started = true;
                    }
                    while *remaining > 0 && range.cursor < range.end {
                        let slot = range.slots.add(range.cursor);
                        if !crate::weakref::is_weak_target_trace_slot(self.header, slot) {
                            process_dirty_slot_work(
                                GcMutableSlot::new(slot, range.layout_kind),
                                range.layout_kind,
                                stats,
                                visit_slot,
                                &mut self.changed,
                            );
                        }
                        range.cursor += 1;
                        *remaining -= 1;
                    }
                    if range.cursor >= range.end {
                        self.cursor += 1;
                    }
                },
            }
        }

        if self.cursor >= self.work.len() {
            unsafe {
                if self.changed {
                    run_gc_rewrite_hook((*self.header).obj_type, self.user_ptr as usize);
                }
            }
            true
        } else {
            false
        }
    }
}

#[inline]
unsafe fn process_dirty_slot_work(
    slot: GcMutableSlot,
    layout_kind: Option<HeapChildSlotReadKind>,
    stats: &mut RememberedSetTraceStats,
    visit_slot: &mut dyn FnMut(GcMutableSlot, &mut RememberedSetTraceStats),
    changed: &mut bool,
) {
    if let Some(layout_kind) = layout_kind {
        record_layout_child_slot_read(layout_kind);
    }
    stats.dirty_slots_scanned += 1;
    crate::arena::old_page_account_dirty_slot(slot.slot as usize);
    let before = slot.read();
    visit_slot(slot, stats);
    *changed |= slot.read() != before;
}

pub(in crate::gc) fn dirty_slot_ranges_for(
    range: HeapSlotRange,
    dirty_pages: &crate::fast_hash::PtrHashSet<usize>,
    stats: &mut RememberedSetTraceStats,
) -> Vec<(usize, usize)> {
    if range.is_empty() || dirty_pages.is_empty() {
        return Vec::new();
    }

    const PAGE_SHIFT: usize = 12;
    const PAGE_SIZE: usize = 1 << PAGE_SHIFT;

    let slots = range.slots() as usize;
    let slot_count = range.slot_count();
    let Some(slots_bytes) = slot_count.checked_mul(std::mem::size_of::<u64>()) else {
        return Vec::new();
    };
    let Some(slots_end) = slots.checked_add(slots_bytes) else {
        return Vec::new();
    };

    // Walk whichever side is smaller. Iterating the dirty-page set is O(dirty
    // pages) regardless of the range's size, which is the right shape for the
    // one huge array this exists for — but it is quadratic when a heap holds
    // MANY small pointer ranges (each would rescan the whole set). Enumerating
    // the range's own pages instead is O(range pages) with one set probe each.
    // Both arms produce the same ranges; only the traversal order differs, and
    // the merge below sorts.
    let range_pages = (slots_end - 1).saturating_sub(slots) / PAGE_SIZE + 1;
    let mut ranges = Vec::new();
    let push_page = |page: usize, ranges: &mut Vec<(usize, usize)>, stats: &mut _| {
        let page_start = page << PAGE_SHIFT;
        let page_end = page_start + PAGE_SIZE;
        let start = slots.max(page_start);
        let end = slots_end.min(page_end);
        if start >= end {
            return;
        }
        let stats: &mut RememberedSetTraceStats = stats;
        stats.dirty_slot_pages_considered += 1;
        let first = (start - slots) / std::mem::size_of::<u64>();
        let last = (end - slots).div_ceil(std::mem::size_of::<u64>());
        if first < last {
            ranges.push((first.min(slot_count), last.min(slot_count)));
        }
    };
    if range_pages <= dirty_pages.len() {
        let first_page = slots >> PAGE_SHIFT;
        let last_page = (slots_end - 1) >> PAGE_SHIFT;
        for page in first_page..=last_page {
            if dirty_pages.contains(&page) {
                push_page(page, &mut ranges, stats);
            }
        }
    } else {
        for &page in dirty_pages {
            push_page(page, &mut ranges, stats);
        }
    }

    if ranges.is_empty() {
        return ranges;
    }
    ranges.sort_unstable();
    let mut merged = Vec::<(usize, usize)>::with_capacity(ranges.len());
    for (start, end) in ranges {
        if let Some((_, last_end)) = merged.last_mut() {
            if start <= *last_end {
                *last_end = (*last_end).max(end);
                continue;
            }
        }
        merged.push((start, end));
    }
    merged
}

pub(in crate::gc) struct RememberedSetRootMarkState {
    snapshot: RememberedDirtySnapshot,
    stats: RememberedSetTraceStats,
    old_page_cursor: Option<crate::arena::OldArenaPageObjectCursor>,
    external_cursor: usize,
    fallback_cursor: usize,
    seen_headers: crate::fast_hash::PtrHashSet<usize>,
    current_header: Option<DirtyHeaderSlotScan>,
    finalized: bool,
}

impl RememberedSetRootMarkState {
    pub(in crate::gc) fn new() -> Self {
        Self::new_with_marking(true)
    }

    /// #9629: a FULL trace visits the old generation from the real root set,
    /// so every young object a LIVE old object points at is reached anyway.
    /// Marking from the remembered set on top of that adds exactly one thing:
    /// the young objects reachable only from old objects that are themselves
    /// DEAD. `DirtyHeaderSlotScan::new` validates that the dirty page's header
    /// is a plausible pointer (`valid_ptrs`), never that it is live, so a dead
    /// old owner's slots are marked as roots like any other.
    ///
    /// `mark = false` therefore skips the marking for full traces while still
    /// taking the snapshot, which is NOT optional: `remembered_dirty_snapshot`
    /// is what lazily arms the write barrier and reconstructs the log from the
    /// heap (`barrier_arming::arm_and_reconstruct_remembered_set_if_unarmed`).
    /// Skipping the snapshot as well would leave a thread's old-to-young
    /// stores unlogged, which fails in the opposite and far worse direction.
    ///
    /// A minor keeps marking: it deliberately does not trace the old
    /// generation, so there old-to-young edges genuinely are roots.
    pub(in crate::gc) fn new_with_marking(mark: bool) -> Self {
        let snapshot = remembered_dirty_snapshot();
        let stats = RememberedSetTraceStats {
            entries_scanned: snapshot.dirty_old_pages.len()
                + snapshot.external_dirty_entries.len()
                + snapshot.fallback_headers.len(),
            dirty_pages_before: snapshot.dirty_pages.len(),
            dirty_pages_scanned: snapshot.dirty_pages.len(),
            ..RememberedSetTraceStats::default()
        };
        let old_page_cursor = (!snapshot.dirty_old_pages.is_empty())
            .then(|| crate::arena::OldArenaPageObjectCursor::new(&snapshot.dirty_old_pages));

        let mut state = Self {
            snapshot,
            stats,
            old_page_cursor,
            external_cursor: 0,
            fallback_cursor: 0,
            seen_headers: crate::fast_hash::new_ptr_hash_set(),
            current_header: None,
            finalized: false,
        };
        if !mark {
            // Snapshot taken (barrier armed, log reconstructed); mark nothing.
            // The reported set size stays truthful — only `newly_marked` is 0.
            state.old_page_cursor = None;
            state.external_cursor = state.snapshot.external_dirty_entries.len();
            state.fallback_cursor = state.snapshot.fallback_headers.len();
            state.stats.dirty_pages_after = remembered_dirty_page_count();
            state.finalized = true;
        }
        state
    }

    pub(in crate::gc) fn step(&mut self, valid_ptrs: &ValidPointerSet, budget: usize) -> bool {
        if self.finalized {
            return true;
        }

        let mut remaining = budget;
        let mut mark_slot = |slot: GcMutableSlot, stats: &mut RememberedSetTraceStats| unsafe {
            if try_mark_young_value_as_seed(slot.read(), valid_ptrs) {
                stats.newly_marked += 1;
            }
        };

        while remaining > 0 {
            if let Some(current) = self.current_header.as_mut() {
                if !current.step(&mut remaining, &mut self.stats, &mut mark_slot) {
                    return false;
                }
                self.current_header = None;
                continue;
            }

            if let Some(header_addr) = self.next_dirty_header_addr() {
                remaining -= 1;
                if !self.seen_headers.insert(header_addr) {
                    continue;
                }
                self.current_header = unsafe {
                    DirtyHeaderSlotScan::new(
                        header_addr as *mut GcHeader,
                        &self.snapshot.dirty_pages,
                        valid_ptrs,
                        &mut self.stats,
                    )
                };
                if self.current_header.is_none() {
                    continue;
                }
                continue;
            }

            break;
        }

        while remaining > 0 && self.fallback_cursor < self.snapshot.fallback_headers.len() {
            let header_addr = self.snapshot.fallback_headers[self.fallback_cursor];
            self.fallback_cursor += 1;
            remaining -= 1;

            let user_ptr = header_addr + GC_HEADER_SIZE;
            if !valid_ptrs.contains(&user_ptr) {
                continue;
            }
            self.stats.valid_roots += 1;
            let nanbox = POINTER_TAG | (user_ptr as u64);
            if try_mark_value(nanbox, valid_ptrs) {
                self.stats.newly_marked += 1;
            }
        }

        if self.current_header.is_none()
            && self.old_page_cursor.is_none()
            && self.external_cursor >= self.snapshot.external_dirty_entries.len()
            && self.fallback_cursor >= self.snapshot.fallback_headers.len()
        {
            self.stats.dirty_pages_after = remembered_dirty_page_count();
            self.finalized = true;
        }

        self.finalized
    }

    fn next_dirty_header_addr(&mut self) -> Option<usize> {
        if let Some(cursor) = self.old_page_cursor.as_mut() {
            if let Some(header) = cursor.next() {
                return Some(header);
            }
            debug_assert!(cursor.is_done());
            self.old_page_cursor = None;
        }
        if self.external_cursor < self.snapshot.external_dirty_entries.len() {
            let (_, header) = self.snapshot.external_dirty_entries[self.external_cursor];
            self.external_cursor += 1;
            return Some(header);
        }
        None
    }

    pub(in crate::gc) fn stats(&self) -> RememberedSetTraceStats {
        self.stats
    }
}
