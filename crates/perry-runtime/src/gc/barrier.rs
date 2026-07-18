use super::*;

/// Snapshot the remembered dirty ranges before the collection clears them.
pub(super) struct RememberedDirtySnapshot {
    pub(super) dirty_old_pages: crate::fast_hash::PtrHashSet<usize>,
    pub(super) external_dirty_entries: Vec<(usize, usize)>,
    pub(super) dirty_pages: crate::fast_hash::PtrHashSet<usize>,
    pub(super) fallback_headers: Vec<usize>,
}

pub(super) fn remembered_dirty_snapshot() -> RememberedDirtySnapshot {
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

    let mut ranges = Vec::new();
    for &page in dirty_pages {
        let page_start = page << PAGE_SHIFT;
        let page_end = page_start + PAGE_SIZE;
        let start = slots.max(page_start);
        let end = slots_end.min(page_end);
        if start >= end {
            continue;
        }
        stats.dirty_slot_pages_considered += 1;
        let first = (start - slots) / std::mem::size_of::<u64>();
        let last = (end - slots).div_ceil(std::mem::size_of::<u64>());
        if first < last {
            ranges.push((first.min(slot_count), last.min(slot_count)));
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

pub(super) unsafe fn scan_dirty_slot_range(
    slots: *mut u64,
    slot_count: usize,
    dirty_pages: &crate::fast_hash::PtrHashSet<usize>,
    stats: &mut RememberedSetTraceStats,
    visit_slot: &mut dyn FnMut(*mut u64, &mut RememberedSetTraceStats),
) {
    if slots.is_null() || slot_count == 0 || dirty_pages.is_empty() {
        return;
    }
    const PAGE_SHIFT: usize = 12;
    const PAGE_SIZE: usize = 1 << PAGE_SHIFT;

    let slots_start = slots as usize;
    let Some(slots_bytes) = slot_count.checked_mul(std::mem::size_of::<u64>()) else {
        return;
    };
    let Some(slots_end) = slots_start.checked_add(slots_bytes) else {
        return;
    };
    let mut ranges = Vec::<(usize, usize)>::new();

    for &page in dirty_pages {
        let page_start = page << PAGE_SHIFT;
        let page_end = page_start + PAGE_SIZE;
        if page_end <= slots_start || page_start >= slots_end {
            continue;
        }
        stats.dirty_slot_pages_considered += 1;
        let start_addr = page_start.max(slots_start);
        let end_addr = page_end.min(slots_end);
        let start_idx = (start_addr - slots_start).div_ceil(8);
        let end_idx = (end_addr - slots_start).div_ceil(8);
        if start_idx < end_idx && start_idx < slot_count {
            ranges.push((start_idx, end_idx.min(slot_count)));
        }
    }

    if ranges.is_empty() {
        return;
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

    for (start, end) in merged {
        stats.dirty_slot_ranges_scanned += 1;
        for i in start..end {
            stats.dirty_slots_scanned += 1;
            let slot = slots.add(i);
            crate::arena::old_page_account_dirty_slot(slot as usize);
            visit_slot(slot, stats);
        }
    }
}

pub(super) unsafe fn scan_dirty_slot_range_with_layout(
    range: HeapSlotRange,
    layout_kind: HeapChildSlotReadKind,
    dirty_pages: &crate::fast_hash::PtrHashSet<usize>,
    stats: &mut RememberedSetTraceStats,
    visit_slot: &mut dyn FnMut(*mut u64, &mut RememberedSetTraceStats),
) {
    if range.slots().is_null() || range.slot_count() == 0 || dirty_pages.is_empty() {
        return;
    }
    const PAGE_SHIFT: usize = 12;
    const PAGE_SIZE: usize = 1 << PAGE_SHIFT;

    let slots = range.slots();
    let slot_count = range.slot_count();
    let slots_start = slots as usize;
    let Some(slots_bytes) = slot_count.checked_mul(std::mem::size_of::<u64>()) else {
        return;
    };
    let Some(slots_end) = slots_start.checked_add(slots_bytes) else {
        return;
    };
    let mut ranges = Vec::<(usize, usize)>::new();
    for &page in dirty_pages {
        let page_start = page << PAGE_SHIFT;
        let page_end = page_start + PAGE_SIZE;
        let start = slots_start.max(page_start);
        let end = slots_end.min(page_end);
        if start >= end {
            continue;
        }
        stats.dirty_slot_pages_considered += 1;
        let first = (start - slots_start) / std::mem::size_of::<u64>();
        let last = (end - slots_start).div_ceil(std::mem::size_of::<u64>());
        ranges.push((first.min(slot_count), last.min(slot_count)));
    }

    if ranges.is_empty() {
        return;
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

    for (start, end) in merged {
        stats.dirty_slot_ranges_scanned += 1;
        for i in start..end {
            stats.dirty_slots_scanned += 1;
            let slot = slots.add(i);
            record_layout_child_slot_read(layout_kind);
            crate::arena::old_page_account_dirty_slot(slot as usize);
            visit_slot(slot, stats);
        }
    }
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
                for (start, end) in dirty_slot_ranges_for(range, dirty_pages, stats) {
                    stats.dirty_slot_ranges_scanned += 1;
                    for i in start..end {
                        let slot = range.slot(i);
                        if crate::weakref::is_weak_target_trace_slot(header, slot) {
                            continue;
                        }
                        if let Some(layout_kind) = layout_kind {
                            record_layout_child_slot_read(layout_kind);
                        }
                        stats.dirty_slots_scanned += 1;
                        crate::arena::old_page_account_dirty_slot(slot as usize);
                        visit_slot(slot, stats);
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

pub(super) static GENERATED_WRITE_BARRIERS_EMITTED: AtomicUsize = AtomicUsize::new(0);

pub(super) fn incremental_mark_barrier_enable(valid_ptrs: &ValidPointerSet, minor_only: bool) {
    INCREMENTAL_MARK_BARRIER_MINOR_ONLY.with(|cell| cell.set(minor_only));
    INCREMENTAL_MARK_BARRIER_VALID_PTRS.with(|cell| {
        cell.set(valid_ptrs as *const ValidPointerSet);
    });
}

pub(super) fn incremental_mark_barrier_disable() {
    INCREMENTAL_MARK_BARRIER_VALID_PTRS.with(|cell| {
        cell.set(std::ptr::null());
    });
    INCREMENTAL_MARK_BARRIER_MINOR_ONLY.with(|cell| cell.set(false));
    // Keep allocate-black aligned with the barrier (see enable). Sweep-phase
    // births need no mark either: both the arena cursor and the malloc sweep
    // are snapshot-bounded at sweep-state construction, so the in-flight
    // sweep can never visit them — while a birth mark they carry would leak
    // into the next cycle as "already traced" (post-snapshot objects are
    // exactly the ones this sweep never visits and never unmarks).
    GC_BIRTH_EXTRA_FLAGS.with(|cell| cell.set(0));
}

/// Allocate-black birth flags for runtime-path allocations — see
/// `GC_BIRTH_EXTRA_FLAGS`.
#[inline(always)]
pub fn gc_birth_extra_flags() -> u8 {
    GC_BIRTH_EXTRA_FLAGS.with(|cell| cell.get())
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
    if GC_BIRTH_EXTRA_FLAGS.with(|cell| cell.get()) & GC_FLAG_MARKED == 0 {
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

#[inline]
pub(super) fn incremental_mark_barrier_active() -> bool {
    INCREMENTAL_MARK_BARRIER_VALID_PTRS.with(|cell| !cell.get().is_null())
}

#[inline]
pub(super) fn heap_word_candidate_addr(bits: u64) -> Option<usize> {
    let tag = bits & TAG_MASK;
    if tag == POINTER_TAG || tag == STRING_TAG || tag == BIGINT_TAG {
        let payload = bits & POINTER_MASK;
        // arm64_32 (ILP32): a real heap pointer fits in 32 bits, so `payload as
        // usize` is lossless only when the high 16 payload bits are zero. A
        // mistagged/immediate value with a >32-bit payload would otherwise
        // truncate to a garbage 32-bit address that the GC marks/derefs,
        // corrupting unrelated heap memory. Reject it.
        #[cfg(not(target_pointer_width = "64"))]
        if payload > 0xFFFF_FFFF {
            return None;
        }
        let ptr = payload as usize;
        return (ptr != 0).then_some(ptr);
    }
    if tag >= 0x7FF8_0000_0000_0000 {
        return None;
    }
    if (0x1000..=0x0000_FFFF_FFFF_FFFF).contains(&bits) {
        // Same ILP32 guard for the raw-f64 pointer path.
        #[cfg(not(target_pointer_width = "64"))]
        if bits > 0xFFFF_FFFF {
            return None;
        }
        Some(bits as usize)
    } else {
        None
    }
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
    if INCREMENTAL_MARK_BARRIER_MINOR_ONLY.with(|cell| cell.get())
        && !crate::arena::pointer_in_nursery(addr)
    {
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
    INCREMENTAL_MARK_BARRIER_VALID_PTRS.with(|cell| {
        let ptr = cell.get();
        if ptr.is_null() {
            return false;
        }
        let valid_ptrs = unsafe { &*ptr };
        incremental_mark_barrier_value_with_valid_ptrs(value_bits, valid_ptrs)
    })
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
            BarrierTraceCounter::NewDirtyPages => counters.new_dirty_pages += 1,
            BarrierTraceCounter::ConservativeParentSpanMarks => {
                counters.conservative_parent_span_marks += 1;
            }
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
    let child_addr = decode_heap_addr(child);
    if child_addr == 0 {
        bump_write_barrier_trace_counter(BarrierTraceCounter::Calls);
        bump_write_barrier_trace_counter(BarrierTraceCounter::NonPointerChildSkips);
        return;
    }
    bump_write_barrier_trace_counter(BarrierTraceCounter::Calls);
    incremental_mark_barrier_value(child);
    // Decode the parent — must be a NaN-boxed heap pointer.
    let parent_addr = decode_heap_addr(parent);
    if parent_addr == 0 {
        bump_write_barrier_trace_counter(BarrierTraceCounter::NonPointerParentSkips);
        return;
    }
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
        return true;
    }
    external_slot && malloc_gc_parent_addr(parent_addr)
}

#[inline]
pub(super) fn malloc_gc_parent_addr(parent_addr: usize) -> bool {
    if parent_addr < GC_HEADER_SIZE + 0x1000 {
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

pub(super) fn mark_dirty_old_page(page: usize) -> bool {
    bump_write_barrier_trace_counter(BarrierTraceCounter::DirtyPageMarkAttempts);
    DIRTY_OLD_PAGES.with(|s| {
        let inserted = s.borrow_mut().insert(page);
        crate::arena::old_page_mark_dirty(page);
        if inserted {
            bump_write_barrier_trace_counter(BarrierTraceCounter::NewDirtyPages);
        }
        inserted
    })
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

pub(crate) fn runtime_write_barrier_slot(parent_addr: usize, slot_addr: usize, child_bits: u64) {
    if !write_barriers_enabled() {
        incremental_mark_barrier_value(child_bits);
        return;
    }
    js_write_barrier_slot(parent_addr as u64, slot_addr as u64, child_bits);
}

#[inline]
pub(crate) fn runtime_store_jsvalue_slot(
    parent_user: usize,
    slot_addr: usize,
    slot_index: usize,
    value_bits: u64,
) {
    unsafe {
        std::ptr::write(slot_addr as *mut u64, value_bits);
    }
    // A heap string stored into an object field / array element is now aliased
    // from the heap, so a later `js_string_append` must NOT mutate its buffer
    // in place while this slot still references it. Demote it from "uniquely
    // owned" (refcount==1) to shared (refcount==0). This realizes the
    // documented `js_string_addref` contract ("stored into an array/object" —
    // see string/alloc.rs): codegen wires the local-copy alias case (`let y =
    // x`) but never the heap-store case, so refcount=1 strings leaked into
    // object/array slots and were corrupted by the in-place append fast path.
    // Concretely: code that snapshots a string into a heap slot
    // (`slot = newState`) and later grows the same buffer via `+=` would have
    // the in-place append silently rewrite the stored slot, so a later equality
    // check against the snapshot wrongly saw the two as identical. Every dynamic
    // object-field and array-element write funnels through here, so this is the
    // single complete choke point.
    if value_bits & TAG_MASK == STRING_TAG {
        crate::string::js_string_addref((value_bits & POINTER_MASK) as *mut crate::StringHeader);
    }
    layout_note_slot(parent_user, slot_index, value_bits);
    runtime_write_barrier_slot(parent_user, slot_addr, value_bits);
}

pub(crate) fn runtime_write_barrier_external_slot(
    parent_addr: usize,
    slot_addr: usize,
    child_bits: u64,
) {
    if !write_barriers_enabled() {
        incremental_mark_barrier_value(child_bits);
        return;
    }
    write_barrier_slot_inner(
        POINTER_TAG | (parent_addr as u64),
        slot_addr,
        child_bits,
        true,
    );
}

pub(crate) fn runtime_write_barrier_gc_slot(parent_addr: usize, slot_addr: usize, child_bits: u64) {
    if !write_barriers_enabled() {
        incremental_mark_barrier_value(child_bits);
        return;
    }
    let parent_is_malloc_gc = matches!(
        crate::arena::classify_heap_generation(parent_addr),
        crate::arena::HeapGeneration::Unknown
    ) && malloc_gc_parent_addr(parent_addr);
    write_barrier_slot_inner(
        POINTER_TAG | (parent_addr as u64 & POINTER_MASK),
        slot_addr,
        child_bits,
        parent_is_malloc_gc,
    );
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
#[used]
static KEEP_WRITE_BARRIER_ROOT_HEAP_WORD: extern "C" fn(u64) = js_write_barrier_root_heap_word;
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
pub fn remembered_set_size() -> usize {
    remembered_dirty_page_count() + REMEMBERED_SET.with(|s| s.borrow().len())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct MaintenanceClearStep {
    pub(super) done: bool,
    pub(super) work_units: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RememberedSetClearSubphase {
    DirtyOldPages,
    ExternalDirtySlots,
    FallbackHeaders,
    Done,
}

pub(super) struct RememberedSetClearState {
    subphase: RememberedSetClearSubphase,
}

impl RememberedSetClearState {
    pub(super) fn new() -> Self {
        Self {
            subphase: RememberedSetClearSubphase::DirtyOldPages,
        }
    }

    pub(super) fn step(&mut self, budget: usize) -> bool {
        self.step_counted(budget).done
    }

    pub(super) fn step_counted(&mut self, budget: usize) -> MaintenanceClearStep {
        let mut work_units = 0usize;
        loop {
            match self.subphase {
                RememberedSetClearSubphase::DirtyOldPages => {
                    if dirty_old_pages_empty() {
                        self.subphase = RememberedSetClearSubphase::ExternalDirtySlots;
                        continue;
                    }
                    if work_units == budget {
                        break;
                    }
                    if clear_one_dirty_old_page() {
                        work_units = work_units.saturating_add(1);
                    }
                }
                RememberedSetClearSubphase::ExternalDirtySlots => {
                    if external_dirty_slot_headers_empty() {
                        self.subphase = RememberedSetClearSubphase::FallbackHeaders;
                        continue;
                    }
                    if work_units == budget {
                        break;
                    }
                    if clear_one_external_dirty_slot_header() {
                        work_units = work_units.saturating_add(1);
                    }
                }
                RememberedSetClearSubphase::FallbackHeaders => {
                    if fallback_remembered_set_empty() {
                        self.subphase = RememberedSetClearSubphase::Done;
                        continue;
                    }
                    if work_units == budget {
                        break;
                    }
                    if clear_one_fallback_remembered_header() {
                        work_units = work_units.saturating_add(1);
                    }
                }
                RememberedSetClearSubphase::Done => {
                    return MaintenanceClearStep {
                        done: true,
                        work_units,
                    };
                }
            }
        }
        MaintenanceClearStep {
            done: self.subphase == RememberedSetClearSubphase::Done,
            work_units,
        }
    }
}

fn dirty_old_pages_empty() -> bool {
    DIRTY_OLD_PAGES.with(|s| s.borrow().is_empty())
}

fn clear_one_dirty_old_page() -> bool {
    DIRTY_OLD_PAGES.with(|s| {
        let mut pages = s.borrow_mut();
        let Some(page) = pages.iter().next().copied() else {
            return false;
        };
        crate::arena::old_page_clear_dirty(page);
        pages.remove(&page);
        true
    })
}

fn external_dirty_slot_headers_empty() -> bool {
    EXTERNAL_DIRTY_SLOT_PAGES.with(|s| s.borrow().is_empty())
}

fn clear_one_external_dirty_slot_header() -> bool {
    EXTERNAL_DIRTY_SLOT_PAGES.with(|s| {
        let mut pages = s.borrow_mut();
        let Some(page) = pages.keys().next().copied() else {
            return false;
        };
        let remove_page = match pages.get_mut(&page) {
            Some(headers) => {
                headers.pop();
                headers.is_empty()
            }
            None => false,
        };
        if remove_page {
            pages.remove(&page);
        }
        true
    })
}

fn fallback_remembered_set_empty() -> bool {
    REMEMBERED_SET.with(|s| s.borrow().is_empty())
}

fn clear_one_fallback_remembered_header() -> bool {
    REMEMBERED_SET.with(|s| {
        let mut headers = s.borrow_mut();
        let Some(header) = headers.iter().next().copied() else {
            return false;
        };
        headers.remove(&header);
        true
    })
}

pub(super) struct ConservativePinClearState {
    done: bool,
}

impl ConservativePinClearState {
    pub(super) fn new() -> Self {
        Self { done: false }
    }

    pub(super) fn step_counted(&mut self, budget: usize) -> MaintenanceClearStep {
        if self.done {
            return MaintenanceClearStep {
                done: true,
                work_units: 0,
            };
        }

        let mut work_units = 0usize;
        while work_units < budget {
            if clear_one_conservative_pin() {
                work_units = work_units.saturating_add(1);
            } else {
                self.done = true;
                break;
            }
        }

        if !self.done && conservative_pins_empty() {
            self.done = true;
        }

        MaintenanceClearStep {
            done: self.done,
            work_units,
        }
    }
}

fn conservative_pins_empty() -> bool {
    CONS_PINNED.with(|s| s.borrow().is_empty())
}

fn clear_one_conservative_pin() -> bool {
    CONS_PINNED.with(|s| {
        let mut pinned = s.borrow_mut();
        let Some(header) = pinned.iter().next().copied() else {
            return false;
        };
        pinned.remove(&header);
        true
    })
}

/// Gen-GC Phase C: clear the remembered set. Will be called by
/// minor GC after the rs-scan completes (Phase C3). Test-only
/// for now to enable test isolation.
pub fn remembered_set_clear() {
    let mut state = RememberedSetClearState::new();
    while !state.step(usize::MAX) {}
}
