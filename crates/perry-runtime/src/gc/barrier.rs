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

/// #7187 Phase A — is the remembered-set half of the write barrier armed?
///
/// The remembered set has exactly one consumer: a collection, and every
/// collector reaches it through [`remembered_dirty_snapshot`] (see that
/// function for why that is the *sole* read path). Until the first such read,
/// maintaining the log buys nothing — and on `batch.ts` it is the program's
/// single largest cost: #7170 measured `classify_heap_generation` at 19.03%
/// (657M instructions) with **zero collections running**, reached from this
/// barrier on the sort's own element stores.
///
/// So the log starts unarmed and records nothing. The exchange, and the whole
/// soundness argument, lives on the *reading* side: the first snapshot on a
/// thread does not trust the log at all — it reconstructs the complete
/// old→young edge set from the heap
/// ([`arm_and_reconstruct_remembered_set_if_unarmed`]) and arms the barrier on
/// the way past. From then on the barrier maintains it incrementally exactly
/// as before.
///
/// **The naive premise "nothing has collected, so there is no old generation"
/// is false and is deliberately not used here.** `arena_alloc_gc` routes any
/// allocation over `LARGE_OBJECT_THRESHOLD_BYTES` straight to
/// `arena_alloc_gc_old`, so old-gen exists from the first large allocation —
/// `batch.ts`'s own 40 000-element arrays are born old while every record they
/// hold is nursery. `note_array_slot_layout_only`'s comment in
/// `array/header.rs` is the postmortem of the last time that was got wrong
/// ("155 edges, all born-old array→young object"). The reconstruct, not the
/// flag, is what makes this sound.
///
/// A process-global `AtomicBool` rather than a thread-local: arming is
/// monotone and one-way, and conservative in the early direction (a thread
/// that arms early merely stops being free), while a `thread_local!` costs a
/// `_tlv_get_addr` *call* on macOS on a path that runs on every heap store.
/// Per-thread state — "has *this* thread reconstructed yet" — stays in
/// [`REMEMBERED_SET_RECONSTRUCTED`], where a wrong answer would matter.
///
/// **In `cfg(test)` builds this starts ARMED**, and so does
/// [`REMEMBERED_SET_RECONSTRUCTED`]. The runtime's existing barrier unit tests
/// assert the armed steady state — they hand-build an old parent and a young
/// child and expect the store to be logged — and they are right to: that is
/// the state every program is in after its first collection. Letting them
/// observe the unarmed window instead would have deleted their subject
/// wholesale, and made the suite order-dependent besides (the first test that
/// collects arms the process for every later test). The unarmed window gets
/// dedicated tests that opt back out via [`reset_barrier_arming_for_tests`],
/// plus end-to-end coverage from every compiled binary the parity/gap suites
/// run — those start unarmed for real.
static BARRIER_REMEMBERING_ARMED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(cfg!(test));

#[inline]
pub(super) fn barrier_remembering_armed() -> bool {
    BARRIER_REMEMBERING_ARMED.load(Ordering::Relaxed)
}

/// What the unarmed-window reconstruct recovered. The #7187 gate asserts on
/// these rather than on "nothing crashed": a reconstruct that recovers no
/// edges on a workload built from born-old parents means the walk never ran or
/// never looked, which is precisely the `PERRY_GC_FORCE_EVACUATE` failure mode
/// (#6942/#6946) — a green test whose subject was inert.
#[derive(Clone, Copy, Default, Debug, Eq, PartialEq)]
pub(super) struct RememberedReconstructCensus {
    /// Reconstructs run on this thread: 0 (never collected) or 1.
    pub(super) reconstructs: u64,
    /// Old-gen pages re-derived from the heap — the old→young edges the
    /// unarmed barrier deliberately did not log.
    pub(super) recovered_old_pages: u64,
    /// External (malloc-backed) slot-page entries re-derived from the heap.
    pub(super) recovered_external_pages: u64,
}

impl RememberedReconstructCensus {
    pub(super) const fn zero() -> Self {
        Self {
            reconstructs: 0,
            recovered_old_pages: 0,
            recovered_external_pages: 0,
        }
    }
}

thread_local! {
    /// Has this thread already reconstructed its remembered set from the heap?
    /// Set by the first [`remembered_dirty_snapshot`] on the thread. Threads
    /// are independent here: [`BARRIER_REMEMBERING_ARMED`] going true because
    /// some *other* thread collected does not excuse this thread from its own
    /// reconstruct, because this thread's stores before that moment were never
    /// logged. The two flags are a superset and a no-op respectively, so there
    /// is no window in which an edge is neither logged nor reconstructed.
    static REMEMBERED_SET_RECONSTRUCTED: Cell<bool> = const { Cell::new(cfg!(test)) };

    static RECONSTRUCT_CENSUS: Cell<RememberedReconstructCensus> =
        const { Cell::new(RememberedReconstructCensus::zero()) };
}

pub(super) fn remembered_reconstruct_census() -> RememberedReconstructCensus {
    RECONSTRUCT_CENSUS.with(Cell::get)
}

/// Arm the barrier and rebuild this thread's remembered set from the heap, if
/// that has not happened yet. Called by [`remembered_dirty_snapshot`] — i.e.
/// on the read side, before any collector observes the log.
///
/// Ordering is load-bearing: **arm first, walk second.** A store that lands
/// while the walk is in flight must be logged, because the walk may already
/// have passed its parent. Arming first makes the two coverages overlap; the
/// reverse order leaves a hole.
fn arm_and_reconstruct_remembered_set_if_unarmed() {
    if REMEMBERED_SET_RECONSTRUCTED.with(Cell::get) {
        return;
    }
    // Set before the walk: the walk touches layout side tables and GC rewrite
    // hooks, and must not be able to recurse into a second reconstruct.
    REMEMBERED_SET_RECONSTRUCTED.with(|cell| cell.set(true));
    BARRIER_REMEMBERING_ARMED.store(true, Ordering::Relaxed);

    // `require_marked = false`: nothing is marked yet when a collector first
    // asks for the log, so the walk must consider every retained old parent.
    // That over-approximates (a dead-but-unswept old parent's young children
    // are kept one extra cycle) in the same direction the barrier itself
    // over-approximates — an edge logged while the parent was alive also
    // outlives the parent's death.
    let sticky = super::verify::rebuild_minor_old_to_young_remembered_set();
    let recovered_old_pages = sticky.old_pages.len() as u64;
    let recovered_external_pages = sticky.external_pages.len() as u64;
    sticky.restore();

    RECONSTRUCT_CENSUS.with(|cell| {
        let mut census = cell.get();
        census.reconstructs = census.reconstructs.saturating_add(1);
        census.recovered_old_pages = census
            .recovered_old_pages
            .saturating_add(recovered_old_pages);
        census.recovered_external_pages = census
            .recovered_external_pages
            .saturating_add(recovered_external_pages);
        cell.set(census);
    });
}

/// Test-only: return the barrier to its process-start unarmed state so a test
/// can exercise the unarmed window deliberately. Production has no way back —
/// arming is one-way.
#[cfg(test)]
pub(super) fn reset_barrier_arming_for_tests() {
    BARRIER_REMEMBERING_ARMED.store(false, Ordering::Relaxed);
    REMEMBERED_SET_RECONSTRUCTED.with(|cell| cell.set(false));
    RECONSTRUCT_CENSUS.with(|cell| cell.set(RememberedReconstructCensus::zero()));
}

/// Test-only: arm the barrier without a reconstruct, for the barrier tests
/// that assert the ARMED steady state and build their own heap shapes
/// directly.
#[cfg(test)]
pub(super) fn arm_barrier_for_tests() {
    BARRIER_REMEMBERING_ARMED.store(true, Ordering::Relaxed);
    REMEMBERED_SET_RECONSTRUCTED.with(|cell| cell.set(true));
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
    let newly_active = INCREMENTAL_MARK_BARRIER_VALID_PTRS.with(|cell| {
        let newly_active = cell.get().is_null();
        cell.set(valid_ptrs as *const ValidPointerSet);
        newly_active
    });
    if newly_active {
        PERRY_INCREMENTAL_MARK_BARRIER_ACTIVE_COUNT.fetch_add(1, Ordering::SeqCst);
    }
}

pub(super) fn incremental_mark_barrier_disable() {
    let was_active = INCREMENTAL_MARK_BARRIER_VALID_PTRS.with(|cell| {
        let was_active = !cell.get().is_null();
        cell.set(std::ptr::null());
        was_active
    });
    if was_active {
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

pub(super) fn mark_dirty_old_page(page: usize) -> bool {
    bump_write_barrier_trace_counter(BarrierTraceCounter::DirtyPageMarkAttempts);
    ever_dirty_note(page);
    DIRTY_OLD_PAGES.with(|s| {
        let inserted = s.borrow_mut().insert(page);
        crate::arena::old_page_mark_dirty(page);
        if inserted {
            bump_write_barrier_trace_counter(BarrierTraceCounter::NewDirtyPages);
        }
        inserted
    })
}

thread_local! {
    /// PERRY_GC_VERIFY_EVACUATION diagnostic only: every old page EVER marked
    /// dirty over the process lifetime (never cleared). Lets the verifier's
    /// missing-edge report say whether the slot's page was recorded at some
    /// point (edge recorded-then-LOST by a clear/restore gap) or never recorded
    /// at all (a store path that skips the barrier) — the decisive split for the
    /// missing old→young edge bug. Empty/unused unless the verifier env is set.
    static EVER_DIRTY_OLD_PAGES: std::cell::RefCell<crate::fast_hash::PtrHashSet<usize>> =
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

pub(crate) fn runtime_write_barrier_slot(parent_addr: usize, slot_addr: usize, child_bits: u64) {
    if !write_barriers_enabled() {
        incremental_mark_barrier_value(child_bits);
        return;
    }
    write_barrier_slot_decoded(parent_addr, slot_addr, child_bits, false);
}

/// Canonicalize an **INT32-boxed** numeric store into a raw-f64-masked slot of
/// an intact typed-shape descriptor (`0x7FFE…` → the plain IEEE bits of the same
/// number). The object twin of `canonicalize_array_numeric_store_bits`
/// (`array/header.rs`), and needed for the same reason.
///
/// `layout_note_slot` treats any non-raw-f64 bit pattern landing in a raw-f64
/// slot as a representation change and calls `layout_set_typed_unknown`, which
/// evicts the object's `TypedLayoutDescriptor` **permanently and one-way**.
/// INT32 boxes genuinely reach object fields from FFI / native modules (sqlite
/// row columns, `v8` deserialization, …), and unlike codegen's guarded class-
/// field store — which canonicalizes inline behind a plain-finite check — this
/// runtime choke point wrote the bits verbatim. One FFI integer therefore cost
/// the object its typed fast path forever.
///
/// There is no observable behavior change: an INT32 box and its f64 are `===`
/// and print identically. `value_bits_to_number` supplies the class-ref
/// exclusion (a `ClassRef` shares INT32_TAG and must keep its tag), so a class
/// value still downgrades the descriptor rather than being stripped to a bare
/// number.
///
/// Ordered tag-first so the (hot) non-INT32 store never pays the thread-local
/// descriptor probe.
#[inline]
fn canonicalize_typed_slot_store_bits(
    parent_user: usize,
    slot_index: usize,
    value_bits: u64,
) -> u64 {
    if value_bits & TAG_MASK != crate::value::INT32_TAG {
        return value_bits;
    }
    if !crate::gc::layout_slot_is_raw_f64_typed(parent_user, slot_index) {
        return value_bits;
    }
    match crate::array::value_bits_to_number(value_bits) {
        Some(number) => number.to_bits(),
        None => value_bits,
    }
}

#[inline]
pub(crate) fn runtime_store_jsvalue_slot(
    parent_user: usize,
    slot_addr: usize,
    slot_index: usize,
    value_bits: u64,
) {
    let value_bits = canonicalize_typed_slot_store_bits(parent_user, slot_index, value_bits);
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
    write_barrier_slot_decoded(parent_addr, slot_addr, child_bits, true);
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
    write_barrier_slot_decoded(parent_addr, slot_addr, child_bits, parent_is_malloc_gc);
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

/// #7035: is `addr`'s old page currently in the remembered set?
pub(super) fn dirty_now_for_addr(addr: usize) -> bool {
    DIRTY_OLD_PAGES.with(|s| {
        s.borrow()
            .contains(&crate::arena::generation_page_for_addr(addr))
    })
}

/// #7035: was `addr`'s old page EVER dirtied? Distinguishes "barrier never ran"
/// from "edge was recorded then lost".
pub(super) fn ever_dirty_for_addr(addr: usize) -> bool {
    ever_dirty_old_page(crate::arena::generation_page_for_addr(addr))
}
