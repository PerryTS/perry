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
pub(super) fn mark_remembered_set_roots(valid_ptrs: &ValidPointerSet) -> RememberedSetTraceStats {
    let snapshot = remembered_dirty_snapshot();
    let mut stats = RememberedSetTraceStats {
        entries_scanned: snapshot.dirty_old_pages.len()
            + snapshot.external_dirty_entries.len()
            + snapshot.fallback_headers.len(),
        dirty_pages_before: snapshot.dirty_pages.len(),
        dirty_pages_scanned: snapshot.dirty_pages.len(),
        ..RememberedSetTraceStats::default()
    };

    let mut mark_slot = |slot: *mut u64, stats: &mut RememberedSetTraceStats| unsafe {
        if try_mark_young_value_as_seed(*slot, valid_ptrs) {
            stats.newly_marked += 1;
        }
    };
    scan_remembered_dirty_slot_ranges(&snapshot, valid_ptrs, &mut stats, &mut mark_slot);

    // Test-only fallback path. Production barriers no longer insert
    // object headers here, but keeping the scan lets tests compare the
    // old object-set behavior against the dirty-page path.
    for header_addr in snapshot.fallback_headers {
        // Header sits at GcHeader; user pointer is +GC_HEADER_SIZE.
        let user_ptr = header_addr + GC_HEADER_SIZE;
        if !valid_ptrs.contains(&user_ptr) {
            continue;
        }
        stats.valid_roots += 1;
        let nanbox = POINTER_TAG | (user_ptr as u64);
        if try_mark_value(nanbox, valid_ptrs) {
            stats.newly_marked += 1;
        }
    }
    stats.dirty_pages_after = remembered_dirty_page_count();
    stats
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
        let start_idx = (start_addr - slots_start + 7) / 8;
        let end_idx = (end_addr - slots_start + 7) / 8;
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
                if let Some(layout_kind) = layout_kind {
                    scan_dirty_slot_range_with_layout(
                        range,
                        layout_kind,
                        dirty_pages,
                        stats,
                        visit_slot,
                    );
                } else {
                    scan_dirty_slot_range(
                        range.slots(),
                        range.slot_count(),
                        dirty_pages,
                        stats,
                        visit_slot,
                    );
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
    bump_write_barrier_trace_counter(BarrierTraceCounter::Calls);

    // Decode child first: primitive stores are the most common skip.
    let child_addr = decode_heap_addr(child);
    if child_addr == 0 {
        bump_write_barrier_trace_counter(BarrierTraceCounter::NonPointerChildSkips);
        return;
    }
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
    if !matches!(
        crate::arena::classify_heap_generation(child_addr),
        crate::arena::HeapGeneration::Nursery
    ) {
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
            && size <= (1usize << 34)
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
        // Possible raw pointer. Accept only if the arena side metadata
        // recognizes it as a heap address; ordinary f64 payload bits
        // miss the metadata table and remain non-pointers.
        let addr = bits as usize;
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
    layout_note_slot(parent_user, slot_index, value_bits);
    runtime_write_barrier_slot(parent_user, slot_addr, value_bits);
}

pub(crate) fn runtime_write_barrier_external_slot(
    parent_addr: usize,
    slot_addr: usize,
    child_bits: u64,
) {
    if !write_barriers_enabled() {
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

/// Gen-GC Phase C: clear the remembered set. Will be called by
/// minor GC after the rs-scan completes (Phase C3). Test-only
/// for now to enable test isolation.
pub fn remembered_set_clear() {
    DIRTY_OLD_PAGES.with(|s| {
        let mut pages = s.borrow_mut();
        for &page in pages.iter() {
            crate::arena::old_page_clear_dirty(page);
        }
        pages.clear();
    });
    EXTERNAL_DIRTY_SLOT_PAGES.with(|s| s.borrow_mut().clear());
    REMEMBERED_SET.with(|s| s.borrow_mut().clear());
}
