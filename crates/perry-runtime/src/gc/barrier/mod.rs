use super::hot_tls::{
    hot_birth_extra_flags, hot_incremental_mark_minor_only, hot_incremental_mark_valid_ptrs,
};
use super::*;

/// Snapshot the remembered dirty ranges before the collection clears them.
pub(super) struct RememberedDirtySnapshot {
    pub(super) dirty_old_pages: crate::fast_hash::PtrHashSet<usize>,
    pub(super) external_dirty_entries: Vec<(usize, usize)>,
    pub(super) dirty_pages: crate::fast_hash::PtrHashSet<usize>,
    pub(super) fallback_headers: Vec<usize>,
}

/// The **sole read path** for the remembered set.
///
/// Every collector obtains the dirty set here: the budgeted/full cycle's
/// `RememberedSetRootMarkState::new`, the copying nursery fast path and its
/// preflight, the cycle's pre-clear coverage snapshot, and the evacuation
/// verifier. The barrier *writes* `DIRTY_OLD_PAGES` /
/// `EXTERNAL_DIRTY_SLOT_PAGES` / `REMEMBERED_SET`; `remembered_set_clear`
/// empties them; nothing else reads them for collection decisions. That is
/// what lets #7187's lazy arming be sound by construction rather than by
/// audit: arming the barrier here means no collector can observe an unarmed,
/// and therefore empty, log.
///
/// If a future collector reads those thread-locals directly instead of coming
/// through here, it must call
/// [`arm_and_reconstruct_remembered_set_if_unarmed`] itself.
pub(super) fn remembered_dirty_snapshot() -> RememberedDirtySnapshot {
    arm_and_reconstruct_remembered_set_if_unarmed();
    let dirty_old_pages: crate::fast_hash::PtrHashSet<usize> =
        DIRTY_OLD_PAGES.with(|s| s.borrow().iter().copied().collect());
    let external_dirty_entries: Vec<(usize, usize)> = EXTERNAL_DIRTY_SLOT_PAGES.with(|s| {
        s.borrow()
            .iter()
            .flat_map(|(&page, headers)| headers.iter().copied().map(move |header| (page, header)))
            .collect()
    });
    let mut dirty_pages = dirty_old_pages.clone();
    for (page, _) in &external_dirty_entries {
        dirty_pages.insert(*page);
    }
    let fallback_headers = REMEMBERED_SET.with(|s| s.borrow().iter().copied().collect());

    RememberedDirtySnapshot {
        dirty_old_pages,
        external_dirty_entries,
        dirty_pages,
        fallback_headers,
    }
}

/// Gen-GC Phase C3: mark the remembered set as roots. Old-gen
/// dirty pages may hold pointers to young-gen objects that would
/// otherwise be missed by a minor GC. This is Perry's compact
/// equivalent of MMTk's modbuf / ProcessModBuf: barriers log old
/// pages, this phase scans those bounded regions, and the clear at
/// collection end gives the log consumed semantics.
#[allow(dead_code)]
pub(super) fn mark_remembered_set_roots(valid_ptrs: &ValidPointerSet) -> RememberedSetTraceStats {
    let mut state = RememberedSetRootMarkState::new();
    while !state.step(valid_ptrs, usize::MAX) {}
    state.stats()
}

struct DirtySlotRangeWork {
    slots: *mut u64,
    cursor: usize,
    end: usize,
    layout_kind: Option<HeapChildSlotReadKind>,
    range_started: bool,
}

enum DirtySlotWork {
    Single {
        slot: *mut u64,
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

        let mut work = Vec::new();
        visit_gc_rewrite_slot_descriptors(header, |descriptor| match descriptor {
            GcMutableSlotDescriptor::Slot(slot) => {
                if dirty_pages_contains_addr(dirty_pages, slot.slot as usize) {
                    work.push(DirtySlotWork::Single {
                        slot: slot.slot,
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
            GcMutableSlotDescriptor::PointerFreeRange => {}
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
        visit_slot: &mut dyn FnMut(*mut u64, &mut RememberedSetTraceStats),
    ) -> bool {
        while *remaining > 0 && self.cursor < self.work.len() {
            match &mut self.work[self.cursor] {
                DirtySlotWork::Single { slot, layout_kind } => unsafe {
                    if !crate::weakref::is_weak_target_trace_slot(self.header, *slot) {
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
                                slot,
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
    slot: *mut u64,
    layout_kind: Option<HeapChildSlotReadKind>,
    stats: &mut RememberedSetTraceStats,
    visit_slot: &mut dyn FnMut(*mut u64, &mut RememberedSetTraceStats),
    changed: &mut bool,
) {
    if let Some(layout_kind) = layout_kind {
        record_layout_child_slot_read(layout_kind);
    }
    stats.dirty_slots_scanned += 1;
    crate::arena::old_page_account_dirty_slot(slot as usize);
    let before = *slot;
    visit_slot(slot, stats);
    *changed |= *slot != before;
}

fn dirty_slot_ranges_for(
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

pub(super) struct RememberedSetRootMarkState {
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
    pub(super) fn new() -> Self {
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

        Self {
            snapshot,
            stats,
            old_page_cursor,
            external_cursor: 0,
            fallback_cursor: 0,
            seen_headers: crate::fast_hash::new_ptr_hash_set(),
            current_header: None,
            finalized: false,
        }
    }

    pub(super) fn step(&mut self, valid_ptrs: &ValidPointerSet, budget: usize) -> bool {
        if self.finalized {
            return true;
        }

        let mut remaining = budget;
        let mut mark_slot = |slot: *mut u64, stats: &mut RememberedSetTraceStats| unsafe {
            if try_mark_young_value_as_seed(*slot, valid_ptrs) {
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

    pub(super) fn stats(&self) -> RememberedSetTraceStats {
        self.stats
    }
}

pub(super) fn scan_remembered_dirty_slot_ranges(
    snapshot: &RememberedDirtySnapshot,
    valid_ptrs: &ValidPointerSet,
    stats: &mut RememberedSetTraceStats,
    visit_slot: &mut dyn FnMut(*mut u64, &mut RememberedSetTraceStats),
) {
    if snapshot.dirty_old_pages.is_empty() && snapshot.external_dirty_entries.is_empty() {
        return;
    }

    let mut seen_headers = crate::fast_hash::new_ptr_hash_set();
    if !snapshot.dirty_old_pages.is_empty() {
        crate::arena::old_arena_walk_objects_on_pages(
            &snapshot.dirty_old_pages,
            |header_ptr| unsafe {
                let header = header_ptr as *mut GcHeader;
                if !seen_headers.insert(header as usize) {
                    return;
                }
                scan_dirty_header_once(
                    header,
                    &snapshot.dirty_pages,
                    valid_ptrs,
                    stats,
                    visit_slot,
                );
            },
        );
    }
    for &(_, header_addr) in &snapshot.external_dirty_entries {
        if !seen_headers.insert(header_addr) {
            continue;
        }
        unsafe {
            scan_dirty_header_once(
                header_addr as *mut GcHeader,
                &snapshot.dirty_pages,
                valid_ptrs,
                stats,
                visit_slot,
            );
        }
    }
}

pub(super) unsafe fn scan_dirty_header_once(
    header: *mut GcHeader,
    dirty_pages: &crate::fast_hash::PtrHashSet<usize>,
    valid_ptrs: &ValidPointerSet,
    stats: &mut RememberedSetTraceStats,
    visit_slot: &mut dyn FnMut(*mut u64, &mut RememberedSetTraceStats),
) {
    let total_size = (*header).size as usize;
    if total_size == 0 {
        return;
    }
    if (*header).gc_flags & GC_FLAG_FORWARDED != 0 {
        return;
    }
    let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
    if !valid_ptrs.contains(&(user_ptr as usize)) {
        return;
    }
    stats.old_objects_considered += 1;
    stats.valid_roots += 1;
    stats.dirty_objects_scanned += 1;
    scan_dirty_object_slots(header, dirty_pages, stats, visit_slot);
}

#[inline]
pub(super) fn dirty_pages_contains_addr(
    dirty_pages: &crate::fast_hash::PtrHashSet<usize>,
    addr: usize,
) -> bool {
    dirty_pages.contains(&crate::arena::generation_page_for_addr(addr))
}

pub(super) unsafe fn scan_dirty_slot(
    slot: *mut u64,
    dirty_pages: &crate::fast_hash::PtrHashSet<usize>,
    stats: &mut RememberedSetTraceStats,
    visit_slot: &mut dyn FnMut(*mut u64, &mut RememberedSetTraceStats),
) {
    if !dirty_pages_contains_addr(dirty_pages, slot as usize) {
        return;
    }
    stats.dirty_slots_scanned += 1;
    crate::arena::old_page_account_dirty_slot(slot as usize);
    visit_slot(slot, stats);
}

pub(super) unsafe fn scan_dirty_slot_with_layout(
    slot: *mut u64,
    layout_kind: HeapChildSlotReadKind,
    dirty_pages: &crate::fast_hash::PtrHashSet<usize>,
    stats: &mut RememberedSetTraceStats,
    visit_slot: &mut dyn FnMut(*mut u64, &mut RememberedSetTraceStats),
) {
    if !dirty_pages_contains_addr(dirty_pages, slot as usize) {
        return;
    }
    record_layout_child_slot_read(layout_kind);
    stats.dirty_slots_scanned += 1;
    crate::arena::old_page_account_dirty_slot(slot as usize);
    visit_slot(slot, stats);
}

pub(super) unsafe fn scan_dirty_object_slots(
    header: *mut GcHeader,
    dirty_pages: &crate::fast_hash::PtrHashSet<usize>,
    stats: &mut RememberedSetTraceStats,
    visit_slot: &mut dyn FnMut(*mut u64, &mut RememberedSetTraceStats),
) {
    visit_gc_rewrite_slot_descriptors(header, |descriptor| unsafe {
        match descriptor {
            GcMutableSlotDescriptor::Slot(slot) => {
                if crate::weakref::is_weak_target_trace_slot(header, slot.slot) {
                    return;
                }
                if let Some(layout_kind) = slot.layout_kind {
                    scan_dirty_slot_with_layout(
                        slot.slot,
                        layout_kind,
                        dirty_pages,
                        stats,
                        visit_slot,
                    );
                } else {
                    scan_dirty_slot(slot.slot, dirty_pages, stats, visit_slot);
                }
            }
            GcMutableSlotDescriptor::Range { range, layout_kind } => {
                // Three per-slot costs hoisted out of this loop, which is the
                // copying minor's hottest (750 k iterations per cycle on
                // `gc-handoff/bench/retain.ts`):
                //
                // * `is_weak_target_trace_slot` asks a question about the
                //   PARENT — only a `WeakRef` / weak-entry / finalization-record
                //   object has weak slots at all. Deciding that once per
                //   descriptor instead of once per slot is exact, because a
                //   parent that is not one of those three classes answers
                //   `false` for every slot it owns.
                // * `old_page_account_dirty_slot` is a page-keyed counter
                //   reached through a hash map. Slots here are contiguous and
                //   ascending, so 512 consecutive slots share one page: batch
                //   the increment and pay one map probe per page.
                // * the value in slot `i` is about to be classified, which
                //   reads its target's (cold) GC header — prefetch ahead.
                let parent_has_weak_slots =
                    crate::weakref::header_may_hold_weak_target_slots(header);
                for (start, end) in dirty_slot_ranges_for(range, dirty_pages, stats) {
                    stats.dirty_slot_ranges_scanned += 1;
                    let mut acct_page = usize::MAX;
                    let mut acct_slots = 0usize;
                    for i in start..end {
                        let slot = range.slot(i);
                        if let Some(ahead) = (i + super::prefetch::PREFETCH_DISTANCE < end)
                            .then(|| range.slot(i + super::prefetch::PREFETCH_DISTANCE))
                        {
                            super::prefetch::prefetch_boxed_child(*ahead);
                        }
                        if parent_has_weak_slots
                            && crate::weakref::is_weak_target_trace_slot(header, slot)
                        {
                            continue;
                        }
                        if let Some(layout_kind) = layout_kind {
                            record_layout_child_slot_read(layout_kind);
                        }
                        stats.dirty_slots_scanned += 1;
                        let page = crate::arena::generation_page_for_addr(slot as usize);
                        if page != acct_page {
                            if acct_slots != 0 {
                                crate::arena::old_page_account_dirty_slots(acct_page, acct_slots);
                            }
                            acct_page = page;
                            acct_slots = 0;
                        }
                        acct_slots += 1;
                        visit_slot(slot, stats);
                    }
                    if acct_slots != 0 {
                        crate::arena::old_page_account_dirty_slots(acct_page, acct_slots);
                    }
                }
            }
            GcMutableSlotDescriptor::PointerFreeRange => {}
        }
    });
}

// ---------------------------------------------------------------------------
// Phase C — write barrier + remembered set
// (docs/generational-gc-plan.md §Phase C)
// ---------------------------------------------------------------------------
//
// Generational GC needs to know which old-gen regions hold
// references to young-gen objects, so a minor GC can scan just
// those dirty pages instead of the entire old-gen.
//
// The write barrier fires on every heap store. Semantics:
//   if parent is OLD and child points to YOUNG, dirty the page
//   containing the written slot.
//
// Bounded false-positive policy: dirty pages are allowed to scan
// extra slots on the same 4 KiB page; false negatives would skip a
// live young-gen object and break correctness. `REMEMBERED_SET` is
// retained only as a test fallback for the previous object-level
// HashSet behavior.

thread_local! {
    /// Active incremental mark barrier state (Full AND budgeted Minor
    /// cycles — a Minor cycle sliced across mutator turns has exactly the
    /// same lost-store hazard as a Full one; see the #6224 pacing fix, which
    /// made budgeted minors actually complete and thereby exposed it).
    ///
    /// The valid pointer set is owned by the current `GcCycleState`. This raw
    /// pointer is installed only after that set has been built and is cleared
    /// before sweep/reclaim or if the cycle is dropped.
    pub(super) static INCREMENTAL_MARK_BARRIER_VALID_PTRS: Cell<*const ValidPointerSet> =
        const { Cell::new(std::ptr::null()) };

    /// Extra GcHeader flags stamped on RUNTIME-path allocations at birth:
    /// `GC_FLAG_MARKED` while an incremental mark barrier is active, 0
    /// otherwise (allocate-black). A budgeted cycle's sweep may only collect
    /// what its own trace could have seen; an object born mid-cycle and
    /// installed via a runtime-internal RAW store (a grown array's elements
    /// buffer, a map entry node, a string builder's data — none of which pass
    /// through the nanboxed value-barrier path) would otherwise sit unmarked
    /// and be freed live. Measured: 2,890 of 32,000 live graph nodes silently
    /// lost (checksum mismatch) the moment #6224's pacing made budgeted
    /// cycles complete; escalates to a swept-live-key SIGSEGV with manual
    /// `gc()` mixed in. Born-marked objects survive to the NEXT cycle —
    /// bounded floating garbage, already priced by the debt pacer.
    ///
    /// Codegen's inline bump allocator (lower_call.rs IR) does NOT read this
    /// flag; codegen-born objects are ordinary JS values whose installs all
    /// go through codegen store barriers → `incremental_mark_barrier_value`.
    /// The runtime choke points below cover every raw-install allocation.
    pub(crate) static GC_BIRTH_EXTRA_FLAGS: Cell<u8> = const { Cell::new(0) };

    /// True while the active barrier belongs to a MINOR cycle: the barrier
    /// must then shade only NURSERY children. Marking an old-gen child during
    /// a minor would leave a stray mark bit that the minor's sweep never
    /// clears (minors don't walk the old gen), and the next full cycle would
    /// read that stale MARKED as "already traced" and skip the object's
    /// children — unmarking-by-omission, i.e. a live-object sweep one cycle
    /// later. Old children need no shading in a minor anyway: minors never
    /// collect live old-gen objects.
    pub(super) static INCREMENTAL_MARK_BARRIER_MINOR_ONLY: Cell<bool> = const { Cell::new(false) };

    /// Dirty old-generation pages that have received a YOUNG-gen
    /// pointer since the last collection. This is Perry's compact
    /// modbuf: barriers log bounded page regions, and minor GC scans
    /// old objects intersecting those pages.
    pub(crate) static DIRTY_OLD_PAGES: std::cell::RefCell<crate::fast_hash::PtrHashSet<usize>> =
        std::cell::RefCell::new(crate::fast_hash::new_ptr_hash_set());

    /// Dirty non-arena slot pages owned by old-generation parents.
    /// `Map.entries` lives in a malloc buffer behind an old MapHeader,
    /// so its slot page cannot be discovered from the old-arena page
    /// index. Key by external page and retain the owning old headers.
    pub(crate) static EXTERNAL_DIRTY_SLOT_PAGES: std::cell::RefCell<crate::fast_hash::PtrHashMap<usize, Vec<usize>>> =
        std::cell::RefCell::new(crate::fast_hash::new_ptr_hash_map());

    /// Test-only object-level fallback remembered set. Production
    /// barriers use `DIRTY_OLD_PAGES`; tests keep this path available
    /// for parity checks and rollback coverage without a user-facing
    /// runtime mode.
    pub(crate) static REMEMBERED_SET: std::cell::RefCell<std::collections::HashSet<usize>> =
        std::cell::RefCell::new(std::collections::HashSet::new());

    /// Gen-GC Phase C4b: set of GcHeader addresses pinned this
    /// collection cycle because they may be referenced by the
    /// conservative C-stack scan. Conservative scan finds candidate
    /// pointers by bit-pattern matching memory words; we cannot
    /// safely rewrite those words after evacuation because they
    /// might not actually be pointers (false positives). Therefore
    /// any object discovered conservatively is excluded from the
    /// evacuation candidate set.
    ///
    /// Populated by `pin_currently_marked_as_conservative` after
    /// `mark_stack_roots` runs in `gc_collect_minor`. Cleared at
    /// the end of every collection so the next cycle starts fresh.
    pub(crate) static CONS_PINNED: std::cell::RefCell<std::collections::HashSet<usize>> =
        std::cell::RefCell::new(std::collections::HashSet::new());

    pub(super) static WRITE_BARRIER_TRACE_COUNTERS: Cell<BarrierTraceCounters> =
        const { Cell::new(BarrierTraceCounters::zero()) };
}

per_test_global! {
    /// #7672, fifth instance. Two test guards own this flag under two DIFFERENT
    /// locks — `CopyingNurseryTestGuard` sets it to 1 under the copying-nursery
    /// isolation lock, `GeneratedWriteBarrierTestGuard` swaps it under
    /// `GENERATED_BARRIER_TEST_LOCK` — and `generated_write_barriers_active()`
    /// is read by tests holding neither. A `GeneratedWriteBarrierTestGuard::
    /// inactive()` on one libtest thread therefore silences the runtime barrier
    /// under another thread's test, whose store then dirties no page.
    ///
    /// Observed, not theoretical: `sabotaged_parent_gate_strands_a_young_child_
    /// the_shipped_gate_keeps` failed 1 run in 22 with
    /// `missing_edges=1 ... slot_page_ever_dirty=false` — the barrier did not
    /// fire, which is a wrong VALUE and not a timing symptom.
    pub(super) static GENERATED_WRITE_BARRIERS_EMITTED: AtomicUsize = AtomicUsize::new(0);
}

/// Number of threads whose incremental mark barrier is currently active.
///
/// Generated code reads this before calling a root barrier from a persistent
/// shadow-slot update. Zero is authoritative: the current thread cannot have
/// an active incremental cycle, so a root store needs no shading and can skip
/// the Rust/TLS call entirely. A non-zero value is deliberately conservative:
/// another thread's cycle may be active while this thread's is not, in which
/// case the ordinary barrier call observes its null thread-local pointer and
/// returns. The count (rather than a bool) prevents one worker disabling its
/// cycle from hiding another worker's still-active cycle.
#[no_mangle]
pub static PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(0);

pub(super) fn incremental_mark_barrier_enable(valid_ptrs: &ValidPointerSet, minor_only: bool) {
    INCREMENTAL_MARK_BARRIER_MINOR_ONLY.with(|cell| cell.set(minor_only));
    // Arm the global count BEFORE installing the thread-local pointer, and
    // (in `disable`) decrement it AFTER clearing the pointer. Both halves keep
    // the count conservatively armed across the whole window in which the
    // pointer is live.
    //
    // #7469 made this ordering load-bearing rather than merely tidy:
    // `incremental_mark_barrier_value` now treats a zero count as proof that
    // no shading is needed, so a window where the pointer is installed but the
    // count is not yet bumped would be a window where a store silently skips
    // its insertion barrier — a lost mark, i.e. a live object swept.
    let newly_active = INCREMENTAL_MARK_BARRIER_VALID_PTRS.with(|cell| cell.get().is_null());
    if newly_active {
        super::instruments::note_mark_barrier_armed();
        PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT.fetch_add(1, Ordering::SeqCst);
    }
    INCREMENTAL_MARK_BARRIER_VALID_PTRS.with(|cell| cell.set(valid_ptrs as *const ValidPointerSet));
}

pub(super) fn incremental_mark_barrier_disable() {
    let was_active = INCREMENTAL_MARK_BARRIER_VALID_PTRS.with(|cell| {
        let was_active = !cell.get().is_null();
        cell.set(std::ptr::null());
        was_active
    });
    if was_active {
        super::instruments::note_mark_barrier_disarmed();
        let _ = PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT.fetch_update(
            Ordering::SeqCst,
            Ordering::SeqCst,
            |count| count.checked_sub(1),
        );
    }
    INCREMENTAL_MARK_BARRIER_MINOR_ONLY.with(|cell| cell.set(false));
    // Keep allocate-black aligned with the barrier (see enable). Sweep-phase
    // births need no mark either: both the arena cursor and the malloc sweep
    // are snapshot-bounded at sweep-state construction, so the in-flight
    // sweep can never visit them — while a birth mark they carry would leak
    // into the next cycle as "already traced" (post-snapshot objects are
    // exactly the ones this sweep never visits and never unmarks).
    GC_BIRTH_EXTRA_FLAGS.with(|cell| cell.set(0));
}

/// True when no thread anywhere has an incremental mark barrier installed.
///
/// The same authority `PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT` already
/// gives generated code (see its doc comment): zero proves *this* thread's
/// `INCREMENTAL_MARK_BARRIER_VALID_PTRS` is null, because a thread arming its
/// own barrier increments the count before any store can observe the armed
/// pointer. Non-zero is conservative and falls through to the thread-local
/// read, which then finds its own null and returns.
///
/// #7469: the point is to skip the *thread-local* read. On Darwin that read is
/// an out-of-line `_tlv_get_addr` call on every heap-pointer store, and it was
/// 91 of the 653 attributed `_tlv_get_addr` samples on `churn.ts` — all of them
/// spent proving a null pointer was still null. This is a relaxed load of a
/// static: `adrp` + `ldr` and a perfectly-predicted branch.
#[inline(always)]
fn incremental_mark_barrier_globally_idle() -> bool {
    PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT.load(Ordering::Relaxed) == 0
}

/// Allocate-black birth flags for runtime-path allocations — see
/// `GC_BIRTH_EXTRA_FLAGS`.
#[inline(always)]
pub fn gc_birth_extra_flags() -> u8 {
    hot_birth_extra_flags().get()
}

/// A born-black object must also be TRACED: marking treats MARKED as
/// "already visited", so without a seed the object's children are reachable
/// through it only via the store-time shade — and the insertion barrier is
/// not active during the budgeted BuildValidPointerSet phase's mutator
/// windows. A child linked into a build-window birth before barrier-enable
/// and reachable through nothing else was never marked and got swept live
/// (the compiled-TUI lost-fiber-field bug: a React WIP fiber born in a
/// build window, its `alternate` back-edge holding the only path to the
/// old fiber tree). Seeding every black birth closes this for all phases;
/// the trace drains absorb seeds continuously, so the cost is one worklist
/// visit per mid-cycle runtime allocation.
#[inline]
pub(crate) fn gc_note_black_birth(header: *mut GcHeader) {
    if hot_birth_extra_flags().get() & GC_FLAG_MARKED == 0 {
        return;
    }
    // Leaf types (strings, pointer-free payloads) carry no child edges — the
    // birth mark alone protects them, and seeding them would turn a lazy
    // init burst (e.g. the globalThis builtins table populating mid-cycle:
    // thousands of interned strings) into pure drain traffic.
    if unsafe { gc_type_is_pointer_free((*header).obj_type) } {
        return;
    }
    push_mark_seed(header);
}

/// Is an incremental mark cycle in progress anywhere, or is this thread
/// currently birthing objects black?
///
/// #7888 uses this to refuse the untraced promotion path. An allocate-black
/// birth puts `GC_FLAG_MARKED` on a NURSERY object, and a cycle that neither
/// reads nor clears marks would carry that bit into old-gen, where a stale
/// mark reads as "live" to the next full sweep. The untraced path makes no
/// liveness claim, so it must not be running while anything else is making one
/// through the mark bit.
#[inline]
pub(super) fn incremental_mark_in_progress() -> bool {
    !incremental_mark_barrier_globally_idle() || gc_birth_extra_flags() != 0
}

#[inline]
pub(super) fn incremental_mark_barrier_active() -> bool {
    if incremental_mark_barrier_globally_idle() {
        return false;
    }
    !hot_incremental_mark_valid_ptrs().get().is_null()
}

#[inline]
/// The address an incremental-mark barrier must shade for a stored word,
/// NaN-boxed or bare.
///
/// Shares [`decode_root_word`] with the mark and rewrite paths (#6910): a
/// word form the barrier shades but root marking skips (or vice versa) is
/// exactly the kind of drift that module exists to prevent.
pub(super) fn heap_word_candidate_addr(bits: u64) -> Option<usize> {
    decode_root_word(bits).map(RootWord::addr)
}

#[inline]
pub(super) unsafe fn plausible_arena_user_ptr_header(
    header: *mut GcHeader,
) -> Option<*mut GcHeader> {
    if header.is_null() {
        return None;
    }
    if !(header as usize).is_multiple_of(std::mem::align_of::<GcHeader>()) {
        return None;
    }
    let obj_type = (*header).obj_type;
    let size = (*header).size as usize;
    if gc_type_info(obj_type).is_none()
        || size < GC_HEADER_SIZE
        || size as u64 > (1u64 << 34)
        || (*header).gc_flags & GC_FLAG_ARENA == 0
        || (*header).gc_flags & GC_FLAG_FORWARDED != 0
    {
        None
    } else {
        Some(header)
    }
}

pub(super) fn current_heap_header_for_user_ptr(
    user_ptr: usize,
    valid_ptrs: Option<&ValidPointerSet>,
) -> Option<*mut GcHeader> {
    if user_ptr < GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    if valid_ptrs.is_some_and(|ptrs| ptrs.contains(&user_ptr)) {
        return Some(unsafe { header_from_user_ptr(user_ptr as *const u8) });
    }

    match crate::arena::classify_heap_generation(user_ptr) {
        crate::arena::HeapGeneration::Unknown => {
            let header = unsafe { header_from_user_ptr(user_ptr as *const u8) };
            gc_malloc_header_is_tracked(header).then_some(header)
        }
        _ => unsafe {
            plausible_arena_user_ptr_header(header_from_user_ptr(user_ptr as *const u8))
        },
    }
}

pub(super) fn current_heap_header_for_heap_word(
    bits: u64,
    valid_ptrs: Option<&ValidPointerSet>,
) -> Option<(usize, *mut GcHeader)> {
    let addr = heap_word_candidate_addr(bits)?;
    let header = current_heap_header_for_user_ptr(addr, valid_ptrs)?;
    Some((addr, header))
}

fn incremental_mark_barrier_value_with_valid_ptrs(
    value_bits: u64,
    valid_ptrs: &ValidPointerSet,
) -> bool {
    let Some((addr, header)) = current_heap_header_for_heap_word(value_bits, Some(valid_ptrs))
    else {
        return false;
    };
    // Minor cycles shade only nursery children (see the
    // INCREMENTAL_MARK_BARRIER_MINOR_ONLY doc: stray old-gen marks survive a
    // minor's sweep and poison the next full cycle's trace).
    if hot_incremental_mark_minor_only().get() && !crate::arena::pointer_in_nursery(addr) {
        return false;
    }
    unsafe {
        let flags = (*header).gc_flags;
        if flags & (GC_FLAG_MARKED | GC_FLAG_PINNED | GC_FLAG_FORWARDED) != 0 {
            return false;
        }
        (*header).gc_flags = flags | GC_FLAG_MARKED;
        push_mark_seed(header);
    }
    true
}

pub(super) fn incremental_mark_barrier_value(value_bits: u64) -> bool {
    // #7469: the overwhelmingly common case is "no cycle anywhere", and
    // proving it must not cost a thread-local resolution — this runs on every
    // heap-pointer store in compiled code.
    if incremental_mark_barrier_globally_idle() {
        return false;
    }
    let ptr = hot_incremental_mark_valid_ptrs().get();
    if ptr.is_null() {
        return false;
    }
    let valid_ptrs = unsafe { &*ptr };
    incremental_mark_barrier_value_with_valid_ptrs(value_bits, valid_ptrs)
}

#[allow(dead_code)]
pub(super) fn drain_incremental_mark_barrier_seeds(valid_ptrs: &ValidPointerSet) {
    loop {
        let mut worklist = take_mark_seeds();
        if worklist.is_empty() {
            return;
        }
        drain_trace_worklist(&mut worklist, valid_ptrs);
    }
}

#[no_mangle]
pub extern "C" fn js_gc_write_barriers_emitted(active: u32) {
    if active != 0 {
        GENERATED_WRITE_BARRIERS_EMITTED.fetch_add(1, Ordering::AcqRel);
    } else {
        let _ = GENERATED_WRITE_BARRIERS_EMITTED.fetch_update(
            Ordering::AcqRel,
            Ordering::Acquire,
            |count| count.checked_sub(1),
        );
    }
}

#[inline]
pub(super) fn generated_write_barriers_emitted() -> bool {
    GENERATED_WRITE_BARRIERS_EMITTED.load(Ordering::Acquire) > 0
}

pub(crate) fn write_barriers_enabled() -> bool {
    use std::sync::OnceLock;
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_WRITE_BARRIERS").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

#[inline]
pub(super) fn old_to_young_tracking_complete() -> bool {
    generated_write_barriers_emitted() && write_barriers_enabled()
}

#[inline]
pub(super) fn bump_write_barrier_trace_counter(counter: BarrierTraceCounter) {
    if !gc_trace_enabled() {
        return;
    }
    WRITE_BARRIER_TRACE_COUNTERS.with(|cell| {
        let mut counters = cell.get();
        match counter {
            BarrierTraceCounter::Calls => counters.calls += 1,
            BarrierTraceCounter::NonPointerParentSkips => counters.non_pointer_parent_skips += 1,
            BarrierTraceCounter::NonPointerChildSkips => counters.non_pointer_child_skips += 1,
            BarrierTraceCounter::ParentNotOldSkips => counters.parent_not_old_skips += 1,
            BarrierTraceCounter::ChildNotYoungSkips => counters.child_not_young_skips += 1,
            BarrierTraceCounter::OldToYoungSlowHits => counters.old_to_young_slow_hits += 1,
            BarrierTraceCounter::RememberedSetInsertAttempts => {
                counters.remembered_set_insert_attempts += 1;
            }
            BarrierTraceCounter::NewInserts => counters.new_inserts += 1,
            BarrierTraceCounter::DirtyPageMarkAttempts => counters.dirty_page_mark_attempts += 1,
            BarrierTraceCounter::DirtyPageCacheHits => {
                counters.dirty_page_mark_attempts += 1;
                counters.dirty_page_cache_hits += 1;
            }
            BarrierTraceCounter::NewDirtyPages => counters.new_dirty_pages += 1,
            BarrierTraceCounter::ConservativeParentSpanMarks => {
                counters.conservative_parent_span_marks += 1;
            }
            BarrierTraceCounter::UnarmedSkips => counters.unarmed_skips += 1,
        }
        cell.set(counters);
    });
}

pub(super) fn take_write_barrier_trace_counters() -> BarrierTraceCounters {
    WRITE_BARRIER_TRACE_COUNTERS.with(|cell| {
        let counters = cell.get();
        cell.set(BarrierTraceCounters::zero());
        counters
    })
}

/// Gen-GC Phase C4b: walk the current arena+malloc marked set and
/// record every header address as conservatively pinned. Returns the
/// count/bytes inserted by this stack-scan snapshot only; later
/// legacy copy-only scanner pins share CONS_PINNED for evacuation
/// safety but are reported separately in GC trace output. Called
/// after `mark_stack_roots` (the conservative scan) and before
/// mutable roots, registered scanners, and RS scan — so only the
/// conservative-scan results are captured. Subsequently-marked
/// objects from rewriteable precise sources stay out of CONS_PINNED,
/// and copy-only scanner roots are pinned directly by their callback
/// path when evacuation is enabled.
///
/// Called only from the minor-GC path. The full GC path

#[no_mangle]
pub extern "C" fn js_write_barrier(parent: u64, child: u64) {
    js_write_barrier_slot(parent, 0, child);
}

/// Gen-GC Phase C1: slot-aware write barrier. Called by
/// codegen-emitted store sites unless `PERRY_WRITE_BARRIERS=0`/
/// `off`/`false` disabled barrier emission at compile time.
///
/// Decode the parent + child as raw addresses. If parent's
/// GcHeader sits in the old-gen arena AND child's NaN-boxed
/// pointer (any of POINTER / STRING / BIGINT / SHORT_STRING)
/// resolves to a heap address inside the nursery, dirty the page
/// containing the written slot. A zero slot address falls back to
/// dirtying every occupied page in the parent object.
///
/// Hot-path constraints: this fires on EVERY heap store in
/// compiled code by default. Must be cheap:
/// generation checks use arena page side metadata rather than
/// scanning every arena block.
#[no_mangle]
pub extern "C" fn js_write_barrier_slot(parent: u64, slot_addr: u64, child: u64) {
    write_barrier_slot_inner(parent, slot_addr as usize, child, false);
}

pub(super) fn write_barrier_slot_inner(
    parent: u64,
    slot_addr: usize,
    child: u64,
    external_slot: bool,
) {
    // Decode child first: primitive stores are the overwhelmingly common
    // case (every numeric array/field store) and need NEITHER the
    // incremental-mark probe (nothing to mark) NOR the remembered set (no
    // old→young edge) — so they must not pay the incremental barrier's
    // unconditional thread-local access, which dominated tight numeric store
    // loops (#6011: `ema[i] = <f64>` spent more time in this preamble than
    // in the store itself).
    let Some(child_addr) = barrier_child_prologue(child) else {
        return;
    };
    if !barrier_remembering_active() {
        return;
    }
    // Decode the parent — must be a NaN-boxed heap pointer.
    let parent_addr = decode_heap_addr(parent);
    if parent_addr == 0 {
        bump_write_barrier_trace_counter(BarrierTraceCounter::NonPointerParentSkips);
        return;
    }
    write_barrier_decoded_parent(parent_addr, slot_addr, child_addr, external_slot);
}

/// The never-skippable half of the barrier: decode the stored child and shade
/// it for any in-progress incremental cycle. Returns the child's heap address,
/// or `None` when the store published no heap pointer at all (every numeric
/// array/field store — the #6011 fast path, which must stay the cheapest exit).
#[inline]
fn barrier_child_prologue(child: u64) -> Option<usize> {
    let child_addr = decode_heap_addr(child);
    bump_write_barrier_trace_counter(BarrierTraceCounter::Calls);
    if child_addr == 0 {
        bump_write_barrier_trace_counter(BarrierTraceCounter::NonPointerChildSkips);
        return None;
    }
    incremental_mark_barrier_value(child);
    Some(child_addr)
}

/// #7187: should this barrier call do remembered-set work at all?
///
/// Placed **after** [`barrier_child_prologue`] and **before** the parent
/// decode, in every entry point. Both halves of that placement are
/// load-bearing:
///
///   * After the prologue, so the #6011 fast path (any number stored into any
///     slot — the overwhelmingly common store) pays literally nothing new, and
///     so SATB/insertion shading for an in-progress incremental cycle is never
///     skipped. An incremental cycle implies a collection has run implies
///     armed, so this could not bite today; writing the order down keeps a
///     later refactor from hoisting the check above the shading.
///   * Before the parent decode, so the unarmed window also skips
///     `decode_heap_addr`'s raw-pointer arm — itself a
///     `classify_heap_generation` on the bare-`u64` entry point.
///
/// Cost once armed: one relaxed load of a `static` (`adrp`/`ldr`) plus a
/// perfectly-predicted, permanently-taken branch.
#[inline]
fn barrier_remembering_active() -> bool {
    if barrier_remembering_armed() {
        return true;
    }
    bump_write_barrier_trace_counter(BarrierTraceCounter::UnarmedSkips);
    false
}

/// [`write_barrier_slot_inner`] for a caller that already holds the parent as
/// a plain GC user pointer — see [`write_barrier_decoded_parent`] for why the
/// `u64` round-trip is worth avoiding (#7187).
pub(super) fn write_barrier_slot_decoded(
    parent_addr: usize,
    slot_addr: usize,
    child: u64,
    external_slot: bool,
) {
    let Some(child_addr) = barrier_child_prologue(child) else {
        return;
    };
    if !barrier_remembering_active() {
        return;
    }
    // The NaN-box round-trip this replaces was also FILTERING, not just
    // decoding, and dropping the filter is a segfault rather than a wrong
    // answer: `barrier_parent_needs_remembering` reaches
    // `malloc_gc_parent_addr`, which dereferences. Two filters were in play
    // and both are reproduced here, explicitly:
    //
    //   * bare-`u64` callers (`runtime_write_barrier_slot`) took
    //     `decode_heap_addr`'s raw-pointer arm: 48-bit, above `0x10000`,
    //     8-aligned, then an arena classification.
    //   * NaN-boxing callers (`runtime_write_barrier_external_slot`) were
    //     filtered by ACCIDENT — a parent address with high bits set ORs into
    //     something that is no longer `POINTER_TAG`, so `decode_heap_addr`
    //     returned 0. `closure/dynamic_props.rs` parks props under
    //     non-address owner keys and relies on this (its unit test uses
    //     `0xC10C_AB1E_0000_1803`).
    //
    // [`barrier_parent_addr_is_dereferenceable`] subsumes both. A real GC user
    // pointer — arena block or malloc — satisfies every clause, so no genuine
    // old→young edge is filtered here.
    if !barrier_parent_addr_is_dereferenceable(parent_addr) {
        bump_write_barrier_trace_counter(BarrierTraceCounter::NonPointerParentSkips);
        return;
    }
    write_barrier_decoded_parent(parent_addr, slot_addr, child_addr, external_slot);
}

/// May the barrier treat `parent_addr` as a GC user pointer — classify it and,
/// on the external-slot path, read its `GcHeader`?
///
/// The canonical magnitude predicate plus the 8-alignment that
/// `decode_heap_addr`'s raw-pointer arm checked and `addr_class` does not: a
/// misaligned `GcHeader` read is UB before it is a wrong answer.
///
/// This is a *plausibility* test, not a validity test. Its contract is the one
/// [`crate::value::addr_class::try_read_gc_header`] documents — an aligned,
/// in-range, but stale or unmapped address is still dereferenced, and the
/// `obj_type` / registry checks layered above are what catch reuse.
#[inline]
pub(super) fn barrier_parent_addr_is_dereferenceable(parent_addr: usize) -> bool {
    crate::value::addr_class::is_plausible_heap_addr(parent_addr) && parent_addr.is_multiple_of(8)
}

/// The remembered-set half of the barrier, entered with the parent address
/// **already decoded**.
///
/// #7187: every Rust-side barrier caller holds the parent as a plain `usize`
/// GC user pointer. Routing those through [`write_barrier_slot_inner`] meant
/// re-encoding the address as a bare `u64` so [`decode_heap_addr`] could
/// re-derive it — and its bare-pointer arm pays a full
/// `classify_heap_generation` to do so, immediately before
/// [`barrier_parent_needs_remembering`] classifies the same address again.
/// That was one of FOUR page-map classifications per barriered store on the
/// `batch.ts` sort path (#7170 measured `classify_heap_generation` at 19.03%
/// of that program, ~657M instructions, with zero collections running), and
/// the only one that answered a question the caller had already answered.
///
/// Dropping the round-trip is outcome-preserving, including for the one
/// operand class that reached the bare-pointer arm and failed it: a
/// malloc-GC parent classifies `Unknown`, so `decode_heap_addr` used to
/// return 0 and the barrier exited at `NonPointerParentSkips`. It now
/// reaches `barrier_parent_needs_remembering(parent, external_slot)`, which
/// classifies `Unknown`, is not `Old`, and — for the non-external callers
/// that took this path — exits at `ParentNotOldSkips`. Different counter,
/// same remembered-set effect (none). The external/malloc parents that
/// genuinely need remembering arrive through
/// [`runtime_write_barrier_external_slot`] / [`runtime_write_barrier_gc_slot`],
/// which already tag their parent and are unaffected.
///
/// Callers must pass a real GC user pointer. `decode_heap_addr`'s shape
/// pre-filter (48-bit, above the handle band, 8-aligned) is not applied
/// here, because the Rust callers derive `parent_addr` from a live
/// `*mut ArrayHeader` / `*mut ObjectHeader` / … rather than from JS value
/// bits.
#[inline]
pub(super) fn write_barrier_decoded_parent(
    parent_addr: usize,
    slot_addr: usize,
    child_addr: usize,
    external_slot: bool,
) {
    // Old → young check. Runtime-owned malloc GC objects are outside
    // the nursery and must be treated as old when the caller uses the
    // external-slot path for fields or side buffers.
    if !barrier_parent_needs_remembering(parent_addr, external_slot) {
        bump_write_barrier_trace_counter(BarrierTraceCounter::ParentNotOldSkips);
        return;
    }
    if !remembered_child_needs_tracking(child_addr) {
        bump_write_barrier_trace_counter(BarrierTraceCounter::ChildNotYoungSkips);
        return;
    }

    bump_write_barrier_trace_counter(BarrierTraceCounter::OldToYoungSlowHits);
    bump_write_barrier_trace_counter(BarrierTraceCounter::RememberedSetInsertAttempts);
    let inserted = if external_slot {
        remember_old_to_young_external_slot(parent_addr, slot_addr)
    } else {
        remember_old_to_young_slot(parent_addr, slot_addr)
    };
    if inserted {
        bump_write_barrier_trace_counter(BarrierTraceCounter::NewInserts);
    }
}

/// Re-establish the old→young remembered-set invariant over a contiguous run
/// of `count` slots belonging to the single old-gen parent `parent_addr`.
///
/// This is the array-growth barrier replay (`replay_array_growth_write_barriers`)
/// and it is the one barrier caller that is a LOOP over one parent, so three
/// things the per-store entry point must re-derive every call are loop
/// invariants here:
///
/// * the incremental-shading decision (one static probe, hoisted into a
///   separate pass so the common "no cycle anywhere" case pays nothing per
///   slot),
/// * `barrier_parent_addr_is_dereferenceable` + `barrier_parent_needs_remembering`,
///   which is a page-map classification of the same address `count` times,
/// * and — the big one — the remembered set is PAGE granular, so once a page
///   has been dirtied the remaining ~511 slots on it can be skipped outright.
///   The invariant they would each re-assert ("this slot's page is dirty") is
///   already true.
///
/// Measured motivation: `gc-handoff/bench/retain.ts` grows one array to 3 M
/// elements, so the geometric growth replays ~6 M barriers, and the replay was
/// 2.6× the cost of the `memcpy` it follows (11% of the whole program in a
/// symbolicated profile).
///
/// The skip costs trace-counter fidelity: `calls` / `child_not_young_skips` /
/// `old_to_young_slow_hits` no longer count the slots this proves redundant.
/// That is a diagnostic, not a semantic — the remembered set built is the same
/// set of pages.
pub(super) fn replay_old_parent_slot_range(parent_addr: usize, slots: *mut u64, count: usize) {
    if slots.is_null() || count == 0 {
        return;
    }
    // SATB / insertion shading is never skippable while a cycle is live: a
    // page already dirty says nothing about whether an in-progress mark has
    // seen this value. Only the "no cycle anywhere" proof lets it be hoisted.
    if !incremental_mark_barrier_globally_idle() {
        for i in 0..count {
            incremental_mark_barrier_value(unsafe { *slots.add(i) });
        }
    }
    if !write_barriers_enabled() || !barrier_remembering_active() {
        return;
    }
    if !barrier_parent_addr_is_dereferenceable(parent_addr) {
        bump_write_barrier_trace_counter(BarrierTraceCounter::NonPointerParentSkips);
        return;
    }
    if !barrier_parent_needs_remembering(parent_addr, false) {
        bump_write_barrier_trace_counter(BarrierTraceCounter::ParentNotOldSkips);
        return;
    }
    let mut dirtied_page = usize::MAX;
    for i in 0..count {
        let slot_addr = slots as usize + i * std::mem::size_of::<u64>();
        let page = crate::arena::generation_page_for_addr(slot_addr);
        if page == dirtied_page {
            continue;
        }
        let child = unsafe { *(slot_addr as *const u64) };
        let child_addr = decode_heap_addr(child);
        bump_write_barrier_trace_counter(BarrierTraceCounter::Calls);
        if child_addr == 0 {
            bump_write_barrier_trace_counter(BarrierTraceCounter::NonPointerChildSkips);
            continue;
        }
        if !remembered_child_needs_tracking(child_addr) {
            bump_write_barrier_trace_counter(BarrierTraceCounter::ChildNotYoungSkips);
            continue;
        }
        bump_write_barrier_trace_counter(BarrierTraceCounter::OldToYoungSlowHits);
        bump_write_barrier_trace_counter(BarrierTraceCounter::RememberedSetInsertAttempts);
        // Only a slot that classifies Old is described by its own page; the
        // fallback (`mark_dirty_parent_span`) covers the parent's whole span
        // and gives no single page to latch, so it must not arm the skip.
        if matches!(
            crate::arena::classify_heap_generation(slot_addr),
            crate::arena::HeapGeneration::Old
        ) {
            if mark_dirty_old_page(page) {
                bump_write_barrier_trace_counter(BarrierTraceCounter::NewInserts);
            }
            dirtied_page = page;
        } else if remember_old_to_young_slot(parent_addr, slot_addr) {
            bump_write_barrier_trace_counter(BarrierTraceCounter::NewInserts);
        }
    }
}

/// Carry an old-gen object's dirty-page coverage across a verbatim relocation.
///
/// `js_array_grow` allocates a new backing store and `memcpy`s the old one into
/// it at offset 0, then has to re-establish the remembered set at the new
/// address. Re-deriving it from the slot VALUES is O(length) and dominated by a
/// generation classification per element — 9.7% of `gc-handoff/bench/retain.ts`,
/// 2.6× the `memcpy` it follows.
///
/// It does not have to be re-derived. The barrier's standing invariant is
/// "an old parent's slot holding a young child is on a dirty page", and the copy
/// preserves every value at its byte offset. So old byte offset `o` holds a
/// young child ⟹ the old page covering `o` is dirty ⟹ marking the NEW page
/// covering `o` dirty re-establishes the invariant for it. Walking the old
/// object's pages instead of its slots is O(bytes / 4096) — 512× less work for
/// a `u64` slot run — and it is the SAME invariant the minor collector already
/// trusts every cycle, not a new assumption.
///
/// Returns `false` when it declines (an incremental cycle is live, so the
/// values also owe SATB shading), and the caller must fall back to the full
/// value-derived replay.
pub(crate) fn relocate_copied_old_object_dirty_pages(
    new_parent_addr: usize,
    old_base: usize,
    new_base: usize,
    copied_bytes: usize,
) -> bool {
    if copied_bytes == 0 {
        return true;
    }
    // Shading is about values an in-progress mark may not have seen; a page is
    // not an answer to it. Hand those cycles back to the full replay.
    if !incremental_mark_barrier_globally_idle() {
        return false;
    }
    if !write_barriers_enabled() || !barrier_remembering_active() {
        return true;
    }
    if !barrier_parent_addr_is_dereferenceable(new_parent_addr)
        || !barrier_parent_needs_remembering(new_parent_addr, false)
    {
        return true;
    }
    if !growth_source_can_donate_dirty_pages(old_base) {
        return false;
    }
    const PAGE_SIZE: usize = 1 << 12; // crate::arena::GENERATION_PAGE_SHIFT
    let page_size = PAGE_SIZE;
    let first = crate::arena::generation_page_for_addr(old_base);
    let last = crate::arena::generation_page_for_addr(old_base + copied_bytes - 1);
    for page in first..=last {
        if !dirty_old_page_is_marked(page) {
            continue;
        }
        // The byte window this page contributes, mapped to the new base.
        let window_start = (page * page_size).max(old_base);
        let window_end = ((page + 1) * page_size).min(old_base + copied_bytes);
        if window_start >= window_end {
            continue;
        }
        let new_start = new_base + (window_start - old_base);
        let new_end = new_base + (window_end - old_base);
        let new_first = crate::arena::generation_page_for_addr(new_start);
        let new_last = crate::arena::generation_page_for_addr(new_end - 1);
        for new_page in new_first..=new_last {
            bump_write_barrier_trace_counter(BarrierTraceCounter::RememberedSetInsertAttempts);
            if mark_dirty_old_page(new_page) {
                bump_write_barrier_trace_counter(BarrierTraceCounter::NewInserts);
            }
        }
    }
    true
}

/// Is `page` currently in the old-gen dirty set?
#[inline]
fn dirty_old_page_is_marked(page: usize) -> bool {
    DIRTY_OLD_PAGES.with(|s| s.borrow().contains(&page))
}

/// ★ May the dirty-page coverage of the copy SOURCE be inherited by the copy?
///
/// Only if the source was itself an old-gen parent. The barrier does not dirty
/// pages for a YOUNG parent — a young parent's children are found by the
/// ordinary trace instead — so a young source's empty coverage is not evidence
/// that it has no young children, it is evidence that nobody was recording.
///
/// Array growth crosses that line routinely: a nursery array whose new backing
/// store is big enough to be born old-gen has nothing to inherit. Translating
/// its empty set left every young child unremembered and the next minor swept
/// live objects — `gc-handoff/apps/shapes.ts` printed 1277282 where node prints
/// 1176000, deterministically, exit 0. A young source must re-derive from the
/// slot values, which is self-correcting.
#[inline]
pub(super) fn growth_source_can_donate_dirty_pages(old_base: usize) -> bool {
    matches!(
        crate::arena::classify_heap_generation(old_base),
        crate::arena::HeapGeneration::Old
    )
}

/// Which stored children must an old parent's slot be remembered for?
/// Minor GCs sweep BOTH the nursery and the malloc registry, and old
/// parents are black leaves in minors — so an unremembered old→nursery OR
/// old→malloc edge leaves the child unmarked: the nursery sweep or the
/// malloc sweep frees it while live (and a malloc child's own nursery
/// children die with it, since marked malloc objects are the only path
/// that traces them). Longlived and old children need no remembering:
/// longlived is never swept individually and old is reclaimed only by
/// full cycles that trace everything.
#[inline]
pub(super) fn remembered_child_needs_tracking(child_addr: usize) -> bool {
    match crate::arena::classify_heap_generation(child_addr) {
        crate::arena::HeapGeneration::Nursery => true,
        crate::arena::HeapGeneration::Old | crate::arena::HeapGeneration::Longlived => false,
        crate::arena::HeapGeneration::Unknown => {
            // Non-arena child: candidate malloc-GC object (RegExp, Symbol,
            // hook-mode Promise, grown string, large-capture closure).
            // EXACT malloc-registry membership — deliberately not a header
            // sniff: barrier child values can be uninitialized slot
            // contents (array-growth barrier replay passes raw slot bits),
            // and a plausibility sniff on garbage dirtied pages whose
            // dirty-scan then treated neighboring garbage slots as movable
            // young pointers. Band ids and foreign pointers are never in
            // the registry, so this also needs no pre-deref band guard.
            child_addr > GC_HEADER_SIZE
                && super::malloc::gc_malloc_header_is_tracked(
                    (child_addr - GC_HEADER_SIZE) as *const GcHeader,
                )
        }
    }
}

#[inline]
pub(super) fn barrier_parent_needs_remembering(parent_addr: usize, external_slot: bool) -> bool {
    if matches!(
        crate::arena::classify_heap_generation(parent_addr),
        crate::arena::HeapGeneration::Old
    ) {
        // #7511: generated code skips this whole call when the parent's header
        // has no `GC_FLAG_TENURED` (`emit_parent_may_need_remembering_check`),
        // which is sound only while `Old ⟹ TENURED` — and nothing in the
        // allocator enforces that, so it is pinned by
        // `gc::tests::inline_generation_gate_contract` over the production
        // birth paths instead.
        //
        // A `debug_assert!` here was tried and REVERTED: it is the right
        // enforcement point in principle, but dozens of tests build old-gen
        // fixtures straight from `arena_alloc_gc_old` without the bit
        // (`alloc_old_test_object`, `alloc_old_test_promise`, the
        // `gc/tests/oldgen.rs` family), some deliberately. It fired on those,
        // not on a defect. Reinstating it means fixing those fixtures first.
        return true;
    }
    external_slot && malloc_gc_parent_addr(parent_addr)
}

/// #7187: this DEREFERENCES `parent_addr - GC_HEADER_SIZE`, and its only
/// pre-deref guard used to be a bare `< GC_HEADER_SIZE + 0x1000` floor —
/// which admits every handle-band id and every out-of-range garbage word.
/// It was safe purely because its callers happened to filter first
/// (`decode_heap_addr`'s shape pre-filter, or a NaN-box tag that a
/// non-canonical address corrupted into rejection). That is exactly the
/// "raw address deref behind an accidental guard" class `addr_class` exists
/// to end, and `forwarded_heap_owner` three modules over already reaches for
/// the safe reader. Classify the magnitude FIRST, then dereference.
#[inline]
pub(super) fn malloc_gc_parent_addr(parent_addr: usize) -> bool {
    if !barrier_parent_addr_is_dereferenceable(parent_addr) {
        return false;
    }
    unsafe {
        let header = header_from_user_ptr(parent_addr as *const u8);
        let obj_type = (*header).obj_type;
        let size = (*header).size as usize;
        gc_type_info(obj_type).is_some()
            && size >= GC_HEADER_SIZE
            && size as u64 <= (1u64 << 34)
            && (*header).gc_flags & GC_FLAG_ARENA == 0
            && (*header).gc_flags & GC_FLAG_FORWARDED == 0
    }
}

/// Decode a NaN-boxed value into a heap address. Returns 0 for
/// non-pointer values (numbers / booleans / undefined / null).
/// Accepts POINTER_TAG / STRING_TAG / BIGINT_TAG / SHORT_STRING_TAG;
/// SHORT_STRING values return 0 because they're inline data, not
/// heap pointers.
#[inline]
pub(super) fn decode_heap_addr(bits: u64) -> usize {
    let tag = bits & TAG_MASK;
    if tag == POINTER_TAG || tag == STRING_TAG || tag == BIGINT_TAG {
        (bits & POINTER_MASK) as usize
    } else if tag < 0x7FF8_0000_0000_0000 {
        // Possible raw pointer. Cheap shape pre-filter first (#6011): a real
        // heap address is 48-bit, above the handle band, and 8-aligned — an
        // ordinary f64 payload (e.g. 100.5 = 0x4059_4000_…) has non-zero
        // high bits and is rejected here without paying the page-map
        // classification, which dominated tight numeric store loops. Only
        // the (rare) subnormal doubles whose bits look address-shaped fall
        // through to the authoritative arena lookup.
        let addr = bits as usize;
        if (bits >> 48) != 0 || addr < 0x10000 || addr & 0x7 != 0 {
            return 0;
        }
        if matches!(
            crate::arena::classify_heap_generation(addr),
            crate::arena::HeapGeneration::Unknown
        ) {
            0
        } else {
            addr
        }
    } else {
        // SHORT_STRING_TAG (0x7FF9), INT32_TAG (0x7FFE),
        // primitive (0x7FFC), JS_HANDLE (0x7FFB) — none are
        // young-gen pointers.
        0
    }
}

pub(super) fn remember_old_to_young_slot(parent_addr: usize, slot_addr: usize) -> bool {
    if slot_addr != 0
        && matches!(
            crate::arena::classify_heap_generation(slot_addr),
            crate::arena::HeapGeneration::Old
        )
    {
        return mark_dirty_old_page(crate::arena::generation_page_for_addr(slot_addr));
    }
    bump_write_barrier_trace_counter(BarrierTraceCounter::ConservativeParentSpanMarks);
    mark_dirty_parent_span(parent_addr)
}

pub(super) fn mark_dirty_parent_span(parent_addr: usize) -> bool {
    if parent_addr < GC_HEADER_SIZE {
        return false;
    }
    let header_addr = parent_addr - GC_HEADER_SIZE;
    let header = header_addr as *const GcHeader;
    let total_size = unsafe { (*header).size as usize };
    if total_size == 0 {
        return false;
    }
    let first_page = crate::arena::generation_page_for_addr(header_addr);
    let last_page = crate::arena::generation_page_for_addr(header_addr + total_size - 1);
    let mut inserted_any = false;
    for page in first_page..=last_page {
        inserted_any |= mark_dirty_old_page(page);
    }
    inserted_any
}

pub(super) fn remember_old_to_young_external_slot(parent_addr: usize, slot_addr: usize) -> bool {
    if slot_addr == 0 || parent_addr < GC_HEADER_SIZE {
        return false;
    }
    let header_addr = parent_addr - GC_HEADER_SIZE;
    mark_dirty_external_slot_page(
        header_addr,
        crate::arena::generation_page_for_addr(slot_addr),
    )
}

/// #7187 Phase B: record `page` in this thread's modbuf, unless it is already
/// there. Returns whether the page was NEWLY inserted.
///
/// The guard is the whole of Phase B — see [`super::dirty_page_cache`] for the
/// invariant it rests on and the measurement that picked a one-entry cache.
/// Armed on `batch.ts` this call fires 1 774 374 times for 517 distinct pages;
/// the guard turns 99.78% of those into a thread-local load and a compare.
#[inline]
pub(super) fn mark_dirty_old_page(page: usize) -> bool {
    if super::dirty_page_cache::dirty_old_page_already_marked(page) {
        // Bumps `dirty_page_mark_attempts` too, so that counter keeps meaning
        // "calls", comparable across the change, and
        // `attempts - dirty_page_cache_hits` is what still reaches the modbuf.
        bump_write_barrier_trace_counter(BarrierTraceCounter::DirtyPageCacheHits);
        return false;
    }
    mark_dirty_old_page_uncached(page)
}

/// Out of line: the hot path is the guard above, and this body's two
/// thread-local accesses plus two hash operations are the 6.73% #7170 measured.
#[inline(never)]
fn mark_dirty_old_page_uncached(page: usize) -> bool {
    bump_write_barrier_trace_counter(BarrierTraceCounter::DirtyPageMarkAttempts);
    ever_dirty_note(page);
    let inserted = DIRTY_OLD_PAGES.with(|s| {
        let inserted = s.borrow_mut().insert(page);
        if inserted {
            bump_write_barrier_trace_counter(BarrierTraceCounter::NewDirtyPages);
        }
        inserted
    });
    // Cache ONLY when the arena stamp landed as well. `old_page_mark_dirty`
    // does nothing for a page with no metadata entry, and caching such a page
    // would let a later `old_page_summary()` under-report `dirty_pages` if the
    // metadata appeared afterwards. Half a recording is not a recording.
    if crate::arena::old_page_mark_dirty(page) {
        super::dirty_page_cache::note_dirty_old_page_marked(page);
    }
    inserted
}

thread_local! {
    /// PERRY_GC_VERIFY_EVACUATION diagnostic only: every old page EVER marked
    /// dirty over the process lifetime (never cleared). Lets the verifier's
    /// missing-edge report say whether the slot's page was recorded at some
    /// point (edge recorded-then-LOST by a clear/restore gap) or never recorded
    /// at all (a store path that skips the barrier) — the decisive split for the
    /// missing old→young edge bug. Empty/unused unless the verifier env is set.
    pub(super) static EVER_DIRTY_OLD_PAGES: std::cell::RefCell<crate::fast_hash::PtrHashSet<usize>> =
        std::cell::RefCell::new(crate::fast_hash::new_ptr_hash_set());
}

fn ever_dirty_tracking_enabled() -> bool {
    use std::sync::OnceLock;
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        std::env::var_os("PERRY_GC_VERIFY_EVACUATION").is_some()
            || super::fromspace_scan::fromspace_scan_enabled()
    })
}

fn ever_dirty_note(page: usize) {
    if !ever_dirty_tracking_enabled() {
        return;
    }
    EVER_DIRTY_OLD_PAGES.with(|s| {
        s.borrow_mut().insert(page);
    });
}

/// Was `page` ever dirtied over the process lifetime? (Diagnostic; only
/// meaningful when PERRY_GC_VERIFY_EVACUATION is set.)
pub(super) fn ever_dirty_old_page(page: usize) -> bool {
    EVER_DIRTY_OLD_PAGES.with(|s| s.borrow().contains(&page))
}

pub(super) fn mark_dirty_external_slot_page(header_addr: usize, page: usize) -> bool {
    bump_write_barrier_trace_counter(BarrierTraceCounter::DirtyPageMarkAttempts);
    EXTERNAL_DIRTY_SLOT_PAGES.with(|s| {
        let mut pages = s.borrow_mut();
        let page_was_new = !pages.contains_key(&page);
        let headers = pages.entry(page).or_insert_with(Vec::new);
        let header_was_new = if headers.contains(&header_addr) {
            false
        } else {
            headers.push(header_addr);
            true
        };
        if page_was_new {
            bump_write_barrier_trace_counter(BarrierTraceCounter::NewDirtyPages);
        }
        header_was_new
    })
}

#[inline]
pub(crate) fn runtime_write_barrier_root_heap_word(value_bits: u64) {
    incremental_mark_barrier_value(value_bits);
}

#[inline]
pub(crate) fn runtime_write_barrier_root_nanbox(value_bits: u64) {
    incremental_mark_barrier_value(value_bits);
}

#[inline]
pub(crate) fn runtime_write_barrier_root_raw_ptr<T>(ptr: *const T) {
    if !ptr.is_null() {
        incremental_mark_barrier_value(ptr as u64);
    }
}

#[inline]
pub(crate) unsafe fn runtime_store_root_nanbox_f64_raw_slot(slot: *mut f64, value: f64) {
    std::ptr::write(slot, value);
    runtime_write_barrier_root_nanbox(value.to_bits());
}

#[inline]
pub(crate) unsafe fn runtime_store_root_raw_mut_ptr_slot<T>(slot: *mut *mut T, value: *mut T) {
    std::ptr::write(slot, value);
    runtime_write_barrier_root_raw_ptr(value);
}

#[inline]
pub(crate) unsafe fn runtime_store_root_usize_slot(slot: *mut usize, value: usize) {
    std::ptr::write(slot, value);
    runtime_write_barrier_root_heap_word(value as u64);
}

#[inline]
pub(crate) fn runtime_store_root_atomic_nanbox_u64(
    slot: &std::sync::atomic::AtomicU64,
    value_bits: u64,
    ordering: std::sync::atomic::Ordering,
) {
    slot.store(value_bits, ordering);
    runtime_write_barrier_root_nanbox(value_bits);
}

#[inline]
pub(crate) fn runtime_store_root_atomic_raw_i64(
    slot: &std::sync::atomic::AtomicI64,
    value: i64,
    ordering: std::sync::atomic::Ordering,
) {
    slot.store(value, ordering);
    runtime_write_barrier_root_heap_word(value as u64);
}

#[inline]
pub(crate) fn runtime_compare_exchange_root_atomic_raw_i64(
    slot: &std::sync::atomic::AtomicI64,
    current: i64,
    new: i64,
    success: std::sync::atomic::Ordering,
    failure: std::sync::atomic::Ordering,
) -> Result<i64, i64> {
    let result = slot.compare_exchange(current, new, success, failure);
    if result.is_ok() {
        runtime_write_barrier_root_heap_word(new as u64);
    }
    result
}

#[no_mangle]
pub extern "C" fn js_write_barrier_root_heap_word(value_bits: u64) {
    runtime_write_barrier_root_heap_word(value_bits);
}

#[no_mangle]
pub extern "C" fn js_write_barrier_root_nanbox(value_bits: u64) {
    runtime_write_barrier_root_nanbox(value_bits);
}

// #2345 symbol retention. Codegen emits calls to these two root write-barrier
// entry points from `__perry_init_strings` (module-level string roots), but no
// Rust caller in the crate graph references them. The default `.a` staticlib
// keeps them via staticlib-export semantics; the auto-optimize build round-trips
// the runtime through whole-program LLVM bitcode and is free to internalize and
// dead-strip an unreferenced `#[no_mangle]` symbol — which broke the default
// `perry file.ts -o out` link with `undefined _js_write_barrier_root_*`. The
// `#[used]` statics pin retained reference edges so both survive every link mode.
// Same pattern as `node_stream_keepalive.rs` / `typedarray.rs`.
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_WRITE_BARRIER_ROOT_HEAP_WORD: extern "C" fn(u64) = js_write_barrier_root_heap_word;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_WRITE_BARRIER_ROOT_NANBOX: extern "C" fn(u64) = js_write_barrier_root_nanbox;

#[inline]
pub(crate) fn runtime_store_gc_heap_word_slot(
    parent_user: usize,
    slot_addr: usize,
    value_bits: u64,
) {
    unsafe {
        std::ptr::write(slot_addr as *mut u64, value_bits);
    }
    runtime_write_barrier_gc_slot(parent_user, slot_addr, value_bits);
}

#[inline]
pub(crate) fn runtime_store_gc_jsvalue_slot(parent_user: usize, slot_addr: usize, value_bits: u64) {
    runtime_store_gc_heap_word_slot(parent_user, slot_addr, value_bits);
}

#[inline]
pub(crate) fn runtime_store_external_heap_word_slot(
    parent_user: usize,
    slot_addr: usize,
    value_bits: u64,
) {
    unsafe {
        std::ptr::write(slot_addr as *mut u64, value_bits);
    }
    runtime_write_barrier_external_slot(parent_user, slot_addr, value_bits);
}

#[inline]
pub(crate) fn runtime_store_external_jsvalue_slot(
    parent_user: usize,
    slot_addr: usize,
    value_bits: u64,
) {
    runtime_store_external_heap_word_slot(parent_user, slot_addr, value_bits);
}

// #854: GC write-barrier external-slot store-with-layout path
#[allow(dead_code)]
#[inline]
pub(crate) fn runtime_store_external_jsvalue_slot_with_layout(
    parent_user: usize,
    slot_addr: usize,
    slot_index: usize,
    value_bits: u64,
) {
    unsafe {
        std::ptr::write(slot_addr as *mut u64, value_bits);
    }
    layout_note_slot(parent_user, slot_index, value_bits);
    runtime_write_barrier_external_slot(parent_user, slot_addr, value_bits);
}

pub(crate) fn runtime_dirty_external_slot_span(
    parent_addr: usize,
    first_slot_addr: usize,
    slot_count: usize,
) {
    if !write_barriers_enabled() {
        return;
    }
    dirty_external_slot_span(parent_addr, first_slot_addr, slot_count);
}

pub(super) fn dirty_external_slot_span(
    parent_addr: usize,
    first_slot_addr: usize,
    slot_count: usize,
) {
    if parent_addr < GC_HEADER_SIZE || first_slot_addr == 0 || slot_count == 0 {
        return;
    }
    if !barrier_parent_needs_remembering(parent_addr, true) {
        return;
    }
    let Some(bytes) = slot_count.checked_mul(std::mem::size_of::<u64>()) else {
        return;
    };
    let Some(last_byte) = first_slot_addr.checked_add(bytes.saturating_sub(1)) else {
        return;
    };
    bump_write_barrier_trace_counter(BarrierTraceCounter::ConservativeParentSpanMarks);
    let header_addr = parent_addr - GC_HEADER_SIZE;
    let first_page = crate::arena::generation_page_for_addr(first_slot_addr);
    let last_page = crate::arena::generation_page_for_addr(last_byte);
    for page in first_page..=last_page {
        mark_dirty_external_slot_page(header_addr, page);
    }
}

pub(super) fn remembered_dirty_page_count() -> usize {
    DIRTY_OLD_PAGES.with(|old| {
        let old = old.borrow();
        EXTERNAL_DIRTY_SLOT_PAGES.with(|external| {
            let external = external.borrow();
            if external.is_empty() {
                return old.len();
            }
            let mut pages = crate::fast_hash::new_ptr_hash_set();
            for &page in old.iter() {
                pages.insert(page);
            }
            for &page in external.keys() {
                pages.insert(page);
            }
            pages.len()
        })
    })
}

/// Gen-GC Phase C: read the current remembered set size — used
/// by tests and `PERRY_GC_DIAG=1` output to confirm barrier
/// activity. Returns 0 in Phase C1 since no codegen-emitted
/// barrier has fired yet.
// Remembered-set inspection and drain/maintenance helpers live in a sibling
// module purely for the 2000-line file-size gate; same module tree, same
// visibility semantics (the statics they read are pub(super)/pub(crate)).
mod maintenance;
pub use maintenance::*;
