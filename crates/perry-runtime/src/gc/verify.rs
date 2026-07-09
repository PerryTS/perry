use super::*;

pub(super) fn try_rewrite_value(bits: u64, valid_ptrs: &ValidPointerSet) -> Option<u64> {
    let tag = bits & TAG_MASK;
    let (ptr_addr, is_nanbox) = match tag {
        t if t == POINTER_TAG || t == STRING_TAG || t == BIGINT_TAG => {
            ((bits & POINTER_MASK) as usize, true)
        }
        _ => {
            // Reject NaN-tagged non-pointer values (numbers,
            // booleans, undefined, null, SSO, INT32, handles).
            if tag >= 0x7FF8_0000_0000_0000 {
                return None;
            }
            // Raw pointer fallback: lower 48 bits valid range.
            if !(0x1000..=0x0000_FFFF_FFFF_FFFF).contains(&bits) {
                return None;
            }
            (bits as usize, false)
        }
    };
    let new_user = try_rewrite_raw_addr(ptr_addr, valid_ptrs)?;
    Some(if is_nanbox {
        tag | (new_user as u64 & POINTER_MASK)
    } else {
        new_user as u64
    })
}

pub(super) fn try_rewrite_nanboxed_value(bits: u64, valid_ptrs: &ValidPointerSet) -> Option<u64> {
    let tag = bits & TAG_MASK;
    if tag != POINTER_TAG && tag != STRING_TAG && tag != BIGINT_TAG {
        return None;
    }
    let ptr_addr = (bits & POINTER_MASK) as usize;
    let new_user = try_rewrite_raw_addr(ptr_addr, valid_ptrs)?;
    Some(tag | (new_user as u64 & POINTER_MASK))
}

pub(super) fn try_rewrite_raw_addr(ptr_addr: usize, valid_ptrs: &ValidPointerSet) -> Option<usize> {
    if ptr_addr == 0 {
        return None;
    }
    let mut current = ptr_addr;
    let mut rewrote = false;
    for _ in 0..64 {
        if !valid_ptrs.contains(&current) {
            return rewrote.then_some(current);
        }
        unsafe {
            let header = (current as *const u8).sub(GC_HEADER_SIZE) as *const GcHeader;
            if (*header).gc_flags & GC_FLAG_FORWARDED == 0 {
                return rewrote.then_some(current);
            }
            let next = forwarding_address(header) as usize;
            if next == 0 || next == current {
                return rewrote.then_some(current);
            }
            current = next;
            rewrote = true;
        }
    }
    rewrote.then_some(current)
}

#[cold]
pub(super) fn panic_stale_forwarded_reference(
    surface: &str,
    slot_addr: usize,
    old_bits: u64,
    new_bits: u64,
) -> ! {
    panic!(
        "gc evacuation verification failed: stale forwarded pointer in {surface}: slot=0x{slot_addr:x} old=0x{old_bits:x} forwarded_to=0x{new_bits:x}"
    );
}

/// In-place rewrite helper: read `*slot`, run it through
/// `try_rewrite_value`, write back if a rewrite was produced.
#[inline]
pub(super) unsafe fn rewrite_slot(slot: *mut u64, valid_ptrs: &ValidPointerSet) {
    let bits = *slot;
    if let Some(new_bits) = try_rewrite_value(bits, valid_ptrs) {
        *slot = new_bits;
    }
}

#[inline]
pub(super) unsafe fn verify_slot(slot: *const u64, valid_ptrs: &ValidPointerSet, surface: &str) {
    let bits = *slot;
    if let Some(new_bits) = try_rewrite_value(bits, valid_ptrs) {
        panic_stale_forwarded_reference(surface, slot as usize, bits, new_bits);
    }
}

pub(super) unsafe fn rewrite_heap_object_fields(
    header: *mut GcHeader,
    valid_ptrs: &ValidPointerSet,
) {
    let flags = (*header).gc_flags;
    if flags & GC_FLAG_FORWARDED != 0 {
        return;
    }
    let mut changed = false;
    visit_gc_rewrite_slots(header, |slot| unsafe {
        slot.record_layout_read();
        let before = *slot.slot;
        rewrite_slot(slot.slot, valid_ptrs);
        changed |= *slot.slot != before;
    });
    if changed && gc_type_rewrite_hook_kind((*header).obj_type) == GcRewriteHookKind::SetIndex {
        let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
        crate::set::rebuild_set_index_for_gc(user_ptr as *mut crate::set::SetHeader);
    }
}

// Evacuation copies land in OLD_ARENA after the remembered-set scan
// for this cycle has already run. Rebuild only the pages for copied
// old objects that still hold nursery children so the next minor GC
// sees those old→young edges after the normal collection clear.
#[inline]
pub(super) unsafe fn remember_evacuated_old_to_young_slot(
    sticky: &mut StickyRememberedSet,
    parent_header: *mut GcHeader,
    slot: *mut u64,
) {
    if slot.is_null() {
        return;
    }
    let child_addr = decode_heap_addr(*slot);
    // Nursery AND malloc-GC children both need their pages kept dirty:
    // minors sweep the malloc registry too, and old parents are black
    // leaves — dropping an old→malloc page here would free the malloc
    // child on the next minor (see remembered_child_needs_tracking).
    if child_addr == 0 || !crate::gc::barrier::remembered_child_needs_tracking(child_addr) {
        return;
    }
    let external = !matches!(
        crate::arena::classify_heap_generation(slot as usize),
        crate::arena::HeapGeneration::Old
    );
    sticky.remember_slot(parent_header, slot, external);
}

pub(super) unsafe fn remember_evacuated_old_copy_young_slots(
    sticky: &mut StickyRememberedSet,
    header: *mut GcHeader,
) {
    if header.is_null() {
        return;
    }
    let flags = (*header).gc_flags;
    if flags & GC_FLAG_FORWARDED != 0 || flags & (GC_FLAG_MARKED | GC_FLAG_PINNED) == 0 {
        return;
    }
    let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
    if !crate::arena::pointer_in_old_gen(user_ptr as usize) {
        return;
    }
    visit_gc_rewrite_slots(header, |slot| unsafe {
        if crate::weakref::is_weak_target_trace_slot(header, slot.slot) {
            return;
        }
        slot.record_layout_read();
        remember_evacuated_old_to_young_slot(sticky, header, slot.slot);
    });
}

/// Post-cycle remembered-set repair (#5029): after `remembered_set_clear` +
/// sticky restore, rescan every PRE-cycle dirty page (and external dirty
/// entry) with the same slot predicate `verify_old_to_young_edges_covered`
/// uses, and re-remember any slot that still points into the nursery. The
/// from-scratch rebuild can disagree with the verifier across the cycle
/// boundary (measured: ~130 covered pages at cycle entry, ~10 after the
/// rebuild, verifier missing_edges=7710 on the next minor while the swept
/// children were still referenced); deriving the kept set from the SAME walk
/// the verifier performs makes dropping a still-needed page impossible by
/// construction. Pages whose every slot now points old (the common case
/// after evacuation rewrites) are still dropped, so the remembered set keeps
/// shrinking as before.
pub(super) fn restore_surviving_dirty_coverage(snapshot: &RememberedDirtySnapshot) {
    let mut sticky = StickyRememberedSet::default();
    // Mirror scan_remembered_dirty_slots_copying's scan_header guards: the
    // external dirty entries can carry headers the harness seeded
    // synthetically, and a dead entry may point at reclaimed memory — never
    // dereference before the plausibility check.
    let mut visit_parent = |header: *mut GcHeader| unsafe {
        if header.is_null() {
            return;
        }
        let arena_parent = plausible_gc_header(header, true);
        let malloc_parent = !arena_parent && plausible_gc_header(header, false);
        if !arena_parent && !malloc_parent {
            return;
        }
        if (*header).gc_flags & GC_FLAG_FORWARDED != 0 {
            return;
        }
        let user = (header as *mut u8).add(GC_HEADER_SIZE) as usize;
        if arena_parent
            && !matches!(
                crate::arena::classify_heap_generation(user),
                crate::arena::HeapGeneration::Old
            )
        {
            return;
        }
        visit_gc_rewrite_slots(header, |slot| unsafe {
            if crate::weakref::is_weak_target_trace_slot(header, slot.slot) {
                return;
            }
            slot.record_layout_read();
            remember_evacuated_old_to_young_slot(&mut sticky, header, slot.slot);
        });
    };
    if !snapshot.dirty_old_pages.is_empty() {
        crate::arena::old_arena_walk_objects_on_pages(&snapshot.dirty_old_pages, |hp| {
            visit_parent(hp as *mut GcHeader);
        });
    }
    let mut seen_external = crate::fast_hash::new_ptr_hash_set();
    for &(_, header_addr) in &snapshot.external_dirty_entries {
        if !seen_external.insert(header_addr) {
            continue;
        }
        // External entries may be stale (or, in the GC unit tests,
        // synthetic). Establish that the address is dereference-safe
        // WITHOUT touching it: old/longlived arena pages are always
        // mapped; anything else must still be a registered malloc GC
        // object.
        let deref_safe = matches!(
            crate::arena::classify_heap_generation(header_addr),
            crate::arena::HeapGeneration::Old | crate::arena::HeapGeneration::Longlived
        ) || MALLOC_STATE.with(|s| {
            s.borrow()
                .objects
                .iter()
                .any(|&h| h as usize == header_addr)
        });
        if deref_safe {
            visit_parent(header_addr as *mut GcHeader);
        }
    }
    sticky.restore();
}

pub(super) fn rebuild_evacuated_old_to_young_remembered_set(
    evacuated_headers: &[*mut GcHeader],
) -> StickyRememberedSet {
    let mut sticky = StickyRememberedSet::default();
    for &header in evacuated_headers {
        unsafe {
            remember_evacuated_old_copy_young_slots(&mut sticky, header);
        }
    }
    sticky
}

unsafe fn remember_retained_old_to_young_slots(
    sticky: &mut StickyRememberedSet,
    header: *mut GcHeader,
    require_marked: bool,
) {
    if header.is_null() || (*header).gc_flags & GC_FLAG_FORWARDED != 0 {
        return;
    }
    if require_marked && (*header).gc_flags & (GC_FLAG_MARKED | GC_FLAG_PINNED) == 0 {
        return;
    }
    let user_ptr = (header as *mut u8).add(GC_HEADER_SIZE);
    if !barrier_parent_needs_remembering(user_ptr as usize, true) {
        return;
    }
    visit_gc_rewrite_slots(header, |slot| unsafe {
        if crate::weakref::is_weak_target_trace_slot(header, slot.slot) {
            return;
        }
        slot.record_layout_read();
        remember_evacuated_old_to_young_slot(sticky, header, slot.slot);
    });
}

pub(super) struct OldToYoungRememberedRebuildState {
    require_marked: bool,
    sticky: StickyRememberedSet,
    arena_cursor: Option<crate::arena::ArenaObjectCursor>,
    arena_done: bool,
    malloc_index: usize,
    done: bool,
}

impl OldToYoungRememberedRebuildState {
    pub(super) fn new(require_marked: bool) -> Self {
        Self {
            require_marked,
            sticky: StickyRememberedSet::default(),
            arena_cursor: Some(crate::arena::ArenaObjectCursor::new(
                crate::arena::ArenaWalkOrder::BlockIndex,
            )),
            arena_done: false,
            malloc_index: 0,
            done: false,
        }
    }

    pub(super) fn step(&mut self, budget: usize) -> bool {
        if self.done {
            return true;
        }

        let mut remaining = budget;
        while remaining > 0 && !self.arena_done {
            let next = self
                .arena_cursor
                .as_mut()
                .and_then(crate::arena::ArenaObjectCursor::next);
            let Some((header_ptr, _block_idx)) = next else {
                self.arena_done = true;
                self.arena_cursor = None;
                break;
            };
            remaining -= 1;
            let header = header_ptr as *mut GcHeader;
            unsafe {
                remember_retained_old_to_young_slots(&mut self.sticky, header, self.require_marked);
            }
        }

        while remaining > 0 && self.arena_done {
            let maybe_header = MALLOC_STATE.with(|s| {
                let s = s.borrow();
                s.objects.get(self.malloc_index).copied()
            });
            let Some(header) = maybe_header else {
                self.done = true;
                return true;
            };
            self.malloc_index += 1;
            remaining -= 1;
            unsafe {
                remember_retained_old_to_young_slots(&mut self.sticky, header, self.require_marked);
            }
        }

        if self.arena_done {
            let malloc_len = MALLOC_STATE.with(|s| s.borrow().objects.len());
            if self.malloc_index >= malloc_len {
                self.done = true;
            }
        }

        self.done
    }

    #[allow(dead_code)]
    pub(super) fn finish_unbounded(mut self) -> StickyRememberedSet {
        while !self.step(usize::MAX) {}
        self.sticky
    }

    pub(super) fn finish(self) -> StickyRememberedSet {
        debug_assert!(self.done);
        self.sticky
    }
}

#[allow(dead_code)]
fn rebuild_retained_old_to_young_remembered_set(require_marked: bool) -> StickyRememberedSet {
    OldToYoungRememberedRebuildState::new(require_marked).finish_unbounded()
}

#[allow(dead_code)]
pub(super) fn rebuild_live_old_to_young_remembered_set() -> StickyRememberedSet {
    rebuild_retained_old_to_young_remembered_set(true)
}

#[allow(dead_code)]
pub(super) fn rebuild_minor_old_to_young_remembered_set() -> StickyRememberedSet {
    rebuild_retained_old_to_young_remembered_set(false)
}

#[inline]
pub(super) fn old_young_external_slot_covered(
    snapshot: &RememberedDirtySnapshot,
    parent_header: usize,
    slot: *mut u64,
) -> bool {
    let page = crate::arena::generation_page_for_addr(slot as usize);
    snapshot
        .external_dirty_entries
        .iter()
        .any(|&(entry_page, entry_header)| entry_page == page && entry_header == parent_header)
}

#[inline]
pub(super) fn old_young_slot_covered(
    snapshot: &RememberedDirtySnapshot,
    parent_header: usize,
    slot: *mut u64,
) -> bool {
    let page = crate::arena::generation_page_for_addr(slot as usize);
    if matches!(
        crate::arena::classify_heap_generation(slot as usize),
        crate::arena::HeapGeneration::Old
    ) {
        snapshot.dirty_old_pages.contains(&page)
    } else {
        old_young_external_slot_covered(snapshot, parent_header, slot)
    }
}

#[inline]
pub(super) unsafe fn old_parent_has_remembered_metadata(
    snapshot: &RememberedDirtySnapshot,
    header: *mut GcHeader,
) -> bool {
    let header_addr = header as usize;
    let total_size = (*header).size as usize;
    if total_size != 0
        && crate::arena::old_object_page_overlaps(header_addr, total_size)
            .iter()
            .any(|(page, _)| snapshot.dirty_old_pages.contains(page))
    {
        return true;
    }
    snapshot
        .external_dirty_entries
        .iter()
        .any(|&(_, entry_header)| entry_header == header_addr)
}

#[inline]
pub(super) unsafe fn old_young_parent_should_be_checked(
    snapshot: &RememberedDirtySnapshot,
    header: *mut GcHeader,
) -> bool {
    if header.is_null() || (*header).gc_flags & GC_FLAG_FORWARDED != 0 {
        return false;
    }
    if (*header).gc_flags & (GC_FLAG_MARKED | GC_FLAG_PINNED) != 0 {
        return true;
    }
    old_parent_has_remembered_metadata(snapshot, header)
}

pub(super) unsafe fn verify_old_young_slot_covered(
    snapshot: &RememberedDirtySnapshot,
    stats: &mut OldYoungEdgeVerifyStats,
    parent_header: *mut GcHeader,
    slot: *mut u64,
) {
    if slot.is_null() {
        return;
    }
    let child_addr = decode_heap_addr(*slot);
    if child_addr == 0 || !crate::gc::barrier::remembered_child_needs_tracking(child_addr) {
        return;
    }
    stats.checked_old_to_young_edges = stats.checked_old_to_young_edges.saturating_add(1);
    let parent_addr = parent_header as usize;
    if !old_young_slot_covered(snapshot, parent_addr, slot) {
        stats.record_missing(parent_addr, slot as usize, child_addr);
    }
}

pub(super) unsafe fn verify_old_young_parent_slots_covered(
    snapshot: &RememberedDirtySnapshot,
    stats: &mut OldYoungEdgeVerifyStats,
    header: *mut GcHeader,
) {
    if !old_young_parent_should_be_checked(snapshot, header) {
        return;
    }
    stats.checked_old_objects = stats.checked_old_objects.saturating_add(1);
    visit_gc_rewrite_slots(header, |slot| unsafe {
        if crate::weakref::is_weak_target_trace_slot(header, slot.slot) {
            return;
        }
        slot.record_layout_read();
        verify_old_young_slot_covered(snapshot, stats, header, slot.slot);
    });
}

#[cold]
pub(super) fn panic_old_young_edge_verifier_failed(stats: OldYoungEdgeVerifyStats) -> ! {
    let missing = stats.first_missing.unwrap_or_default();
    panic!(
        "old-young-edge-verifier failed: checked_old_objects={} checked_remembered_pages={} checked_old_to_young_edges={} missing_edges={} first_missing_parent=0x{:x} first_missing_slot=0x{:x} first_missing_child=0x{:x}",
        stats.checked_old_objects,
        stats.checked_remembered_pages,
        stats.checked_old_to_young_edges,
        stats.missing_edges,
        missing.parent,
        missing.slot,
        missing.child
    );
}

pub(super) fn verify_old_to_young_edges_covered() -> OldYoungEdgeVerifyStats {
    let snapshot = remembered_dirty_snapshot();
    let mut stats = OldYoungEdgeVerifyStats {
        checked_remembered_pages: snapshot.dirty_pages.len(),
        ..OldYoungEdgeVerifyStats::default()
    };
    crate::arena::old_arena_walk_objects(|hp| unsafe {
        verify_old_young_parent_slots_covered(&snapshot, &mut stats, hp as *mut GcHeader);
    });
    MALLOC_STATE.with(|s| {
        let s = s.borrow();
        for &header in s.objects.iter() {
            unsafe {
                verify_old_young_parent_slots_covered(&snapshot, &mut stats, header);
            }
        }
    });
    if stats.missing_edges != 0 {
        panic_old_young_edge_verifier_failed(stats);
    }
    stats
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct MarkInvariantMissingEdge {
    pub(super) parent: usize,
    pub(super) slot: usize,
    pub(super) child: usize,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct MarkInvariantVerifyStats {
    pub(super) checked_marked_objects: usize,
    pub(super) checked_edges: usize,
    pub(super) missing_edges: usize,
    pub(super) first_missing: Option<MarkInvariantMissingEdge>,
}

impl MarkInvariantVerifyStats {
    fn record_missing(&mut self, parent: usize, slot: usize, child: usize) {
        self.missing_edges = self.missing_edges.saturating_add(1);
        if self.first_missing.is_none() {
            self.first_missing = Some(MarkInvariantMissingEdge {
                parent,
                slot,
                child,
            });
        }
    }
}

#[cold]
pub(super) fn panic_mark_invariant_verifier_failed(stats: MarkInvariantVerifyStats) -> ! {
    let missing = stats.first_missing.unwrap_or_default();
    panic!(
        "mark-invariant-verifier failed: checked_marked_objects={} checked_edges={} missing_edges={} first_missing_parent=0x{:x} first_missing_slot=0x{:x} first_missing_child=0x{:x}",
        stats.checked_marked_objects,
        stats.checked_edges,
        stats.missing_edges,
        missing.parent,
        missing.slot,
        missing.child
    );
}

pub(super) unsafe fn verify_marked_object_child_marks(
    stats: &mut MarkInvariantVerifyStats,
    header: *mut GcHeader,
) {
    if header.is_null() {
        return;
    }
    let flags = (*header).gc_flags;
    if flags & GC_FLAG_FORWARDED != 0 || flags & GC_FLAG_MARKED == 0 {
        return;
    }
    let parent = (header as *mut u8).add(GC_HEADER_SIZE) as usize;
    stats.checked_marked_objects = stats.checked_marked_objects.saturating_add(1);
    visit_gc_rewrite_slots(header, |slot| unsafe {
        if crate::weakref::is_weak_target_trace_slot(header, slot.slot) {
            return;
        }
        slot.record_layout_read();
        let Some((child, child_header)) = current_heap_header_for_heap_word(*slot.slot, None)
        else {
            return;
        };
        stats.checked_edges = stats.checked_edges.saturating_add(1);
        if (*child_header).gc_flags & (GC_FLAG_MARKED | GC_FLAG_PINNED) == 0 {
            stats.record_missing(parent, slot.slot as usize, child);
        }
    });
}

pub(super) fn verify_marked_heap_no_unmarked_children() -> MarkInvariantVerifyStats {
    let mut stats = MarkInvariantVerifyStats::default();
    crate::arena::arena_walk_objects(|hp| unsafe {
        verify_marked_object_child_marks(&mut stats, hp as *mut GcHeader);
    });
    MALLOC_STATE.with(|s| {
        let s = s.borrow();
        for &header in s.objects.iter() {
            unsafe {
                verify_marked_object_child_marks(&mut stats, header);
            }
        }
    });
    if stats.missing_edges != 0 {
        panic_mark_invariant_verifier_failed(stats);
    }
    stats
}

pub(super) unsafe fn verify_heap_object_fields(
    header: *mut GcHeader,
    valid_ptrs: &ValidPointerSet,
    surface: &'static str,
) {
    let flags = (*header).gc_flags;
    if flags & GC_FLAG_FORWARDED != 0 {
        return;
    }
    visit_gc_rewrite_slots(header, |slot| unsafe {
        slot.record_layout_read();
        verify_slot(slot.slot as *const u64, valid_ptrs, surface);
    });
}

/// Walk every live (MARKED, non-FORWARDED) object on the heap and
/// rewrite any forwarded references in its fields. Includes new
/// evac copies (marked at evac time) and surviving non-evacuated
/// objects.
pub(super) fn rewrite_heap_objects(valid_ptrs: &ValidPointerSet) {
    let rewrite_one = |header: *mut GcHeader| {
        unsafe {
            let flags = (*header).gc_flags;
            // FORWARDED originals are stale — first 8 bytes of
            // payload now holds the forwarding address, not real
            // field data. Skip them entirely.
            if flags & GC_FLAG_FORWARDED != 0 {
                return;
            }
            // Skip dead NURSERY objects — this cycle's sweep frees them.
            // An UNMARKED object outside the nursery is NOT dead: a minor
            // cycle neither traces nor sweeps the old generation, so being
            // unmarked is the normal state of a live old object whose pages
            // are clean. Old→old references have no dirty-page coverage
            // (barriers only track old→young), which makes this walk the
            // ONLY pass that re-points an old referrer at an old-page
            // evacuation target. Skipping unmarked old objects left their
            // slots aimed at forwarding stubs that
            // `release_evacuated_original_forwarding_stubs` then released —
            // dangling pointers into reused memory (#5029).
            if flags & (GC_FLAG_MARKED | GC_FLAG_PINNED) == 0 {
                let user = (header as *mut u8).add(GC_HEADER_SIZE) as usize;
                if crate::arena::pointer_in_nursery(user) {
                    return;
                }
            }
            rewrite_heap_object_fields(header, valid_ptrs);
        }
    };
    crate::arena::arena_walk_objects(|hp| rewrite_one(hp as *mut GcHeader));
    MALLOC_STATE.with(|s| {
        let s = s.borrow();
        for &h in s.objects.iter() {
            rewrite_one(h);
        }
    });
}

pub(super) fn rewrite_remembered_dirty_ranges(valid_ptrs: &ValidPointerSet) {
    let snapshot = remembered_dirty_snapshot();
    let mut stats = RememberedSetTraceStats::default();
    let mut rewrite_dirty_slot = |slot: *mut u64, _stats: &mut RememberedSetTraceStats| unsafe {
        rewrite_slot(slot, valid_ptrs);
    };
    scan_remembered_dirty_slot_ranges(&snapshot, valid_ptrs, &mut stats, &mut rewrite_dirty_slot);

    for header_addr in snapshot.fallback_headers {
        let user_ptr = header_addr + GC_HEADER_SIZE;
        if !valid_ptrs.contains(&user_ptr) {
            continue;
        }
        unsafe {
            rewrite_heap_object_fields(header_addr as *mut GcHeader, valid_ptrs);
        }
    }
}

/// Walk every mutable root slot and rewrite forwarded pointers.
/// Shadow slots are NaN-boxed JSValues; globals can be NaN-boxed or
/// raw object-start pointers. `try_rewrite_value` handles both forms.
pub(super) fn rewrite_mutable_root_slots(
    valid_ptrs: &ValidPointerSet,
    shadow_stats: Option<&mut ShadowRootTraceStats>,
) {
    rewrite_mutable_root_slots_with_sources(valid_ptrs, shadow_stats, None);
}

pub(super) fn rewrite_mutable_root_slots_with_sources(
    valid_ptrs: &ValidPointerSet,
    mut shadow_stats: Option<&mut ShadowRootTraceStats>,
    mut root_sources: Option<&mut RootSourcesTraceStats>,
) {
    visit_mutable_root_slots(|slot| unsafe {
        let bits = slot.read();
        record_mutable_slot_scan_source(slot, bits, valid_ptrs, &mut root_sources);
        if bits == 0 {
            return;
        }
        if let Some(new_bits) = try_rewrite_value(bits, valid_ptrs) {
            slot.write(new_bits);
            record_mutable_slot_rewrite_source(slot, &mut root_sources);
            if matches!(slot.kind, MutableRootSlotKind::ShadowStack) {
                if let Some(stats) = shadow_stats.as_mut() {
                    stats.record_rewrite();
                }
            }
        }
    });
}

pub(super) fn rewrite_mutable_registered_roots(valid_ptrs: &ValidPointerSet) {
    rewrite_mutable_registered_roots_with_sources(valid_ptrs, None);
}

pub(super) fn rewrite_mutable_registered_roots_with_sources(
    valid_ptrs: &ValidPointerSet,
    mut root_sources: Option<&mut RootSourcesTraceStats>,
) {
    let scanners: Vec<MutableRootScannerEntry> = MUTABLE_ROOT_SCANNERS.with(|s| s.borrow().clone());
    let mut visitor = RuntimeRootVisitor::for_rewrite(valid_ptrs);
    for entry in scanners {
        let stats = match &mut root_sources {
            Some(sources) => match entry.source {
                MutableRootScannerSource::RuntimeHandles => {
                    Some(&mut sources.runtime_handles as *mut RootSourceSlotTraceStats)
                }
                MutableRootScannerSource::RuntimeMutableScanner => {
                    Some(&mut sources.runtime_mutable_scanners as *mut RootSourceSlotTraceStats)
                }
            },
            None => None,
        };
        let previous = visitor.set_root_source_stats(stats);
        (entry.scanner)(&mut visitor);
        visitor.set_root_source_stats(previous);
    }
    visit_ffi_mutable_registered_roots_with_sources(&mut visitor, root_sources);
}

pub(super) fn verify_mutable_root_slots(valid_ptrs: &ValidPointerSet) {
    visit_mutable_root_slots(|slot| unsafe {
        let bits = slot.read();
        if bits == 0 {
            return;
        }
        if let Some(new_bits) = try_rewrite_value(bits, valid_ptrs) {
            let surface = match slot.kind {
                MutableRootSlotKind::ShadowStack => "shadow stack roots",
                MutableRootSlotKind::GlobalRoot => "global roots",
            };
            panic_stale_forwarded_reference(surface, slot.ptr as usize, bits, new_bits);
        }
    });
}

pub(super) fn verify_mutable_registered_roots(valid_ptrs: &ValidPointerSet) {
    let scanners: Vec<MutableRootScannerEntry> = MUTABLE_ROOT_SCANNERS.with(|s| s.borrow().clone());
    let mut visitor = RuntimeRootVisitor::for_verify(valid_ptrs, "runtime mutable root scanner");
    for entry in scanners {
        (entry.scanner)(&mut visitor);
    }
    visit_ffi_mutable_registered_roots(&mut visitor);
}

pub(super) fn verify_copy_only_scanner_bits(
    bits: u64,
    valid_ptrs: &ValidPointerSet,
    surface: &'static str,
) {
    if let Some(new_bits) = try_rewrite_nanboxed_value(bits, valid_ptrs) {
        panic_stale_forwarded_reference(surface, 0, bits, new_bits);
    }
}

pub(super) struct RegisteredRootVerifyContext {
    pub(super) valid_ptrs: *const ValidPointerSet,
}

pub(super) extern "C" fn perry_ffi_verify_root(value: f64, ctx: *mut c_void) {
    if ctx.is_null() {
        return;
    }
    let ctx = unsafe { &*(ctx as *const RegisteredRootVerifyContext) };
    if ctx.valid_ptrs.is_null() {
        return;
    }
    let valid_ptrs = unsafe { &*ctx.valid_ptrs };
    verify_copy_only_scanner_bits(value.to_bits(), valid_ptrs, "ffi copy-only root scanner");
}

pub(super) fn verify_copy_only_registered_roots(valid_ptrs: &ValidPointerSet) {
    let scanners: Vec<fn(&mut dyn FnMut(f64))> = ROOT_SCANNERS.with(|s| s.borrow().clone());
    for scanner in scanners {
        scanner(&mut |value: f64| {
            verify_copy_only_scanner_bits(value.to_bits(), valid_ptrs, "copy-only root scanner");
        });
    }

    let ffi_scanners: Vec<PerryFfiRootScanner> = FFI_ROOT_SCANNERS.with(|s| s.borrow().clone());
    let mut ctx = RegisteredRootVerifyContext {
        valid_ptrs: valid_ptrs as *const ValidPointerSet,
    };
    let ctx = &mut ctx as *mut RegisteredRootVerifyContext as *mut c_void;
    for scanner in ffi_scanners {
        scanner(perry_ffi_verify_root, ctx);
    }
}

pub(super) fn verify_remembered_dirty_ranges(valid_ptrs: &ValidPointerSet) {
    let snapshot = remembered_dirty_snapshot();
    let mut stats = RememberedSetTraceStats::default();
    let mut verify_dirty_slot = |slot: *mut u64, _stats: &mut RememberedSetTraceStats| unsafe {
        verify_slot(slot as *const u64, valid_ptrs, "remembered dirty ranges");
    };
    scan_remembered_dirty_slot_ranges(&snapshot, valid_ptrs, &mut stats, &mut verify_dirty_slot);

    for header_addr in snapshot.fallback_headers {
        let user_ptr = header_addr + GC_HEADER_SIZE;
        if !valid_ptrs.contains(&user_ptr) {
            continue;
        }
        unsafe {
            verify_heap_object_fields(
                header_addr as *mut GcHeader,
                valid_ptrs,
                "remembered fallback headers",
            );
        }
    }
}

pub(super) fn verify_heap_objects(valid_ptrs: &ValidPointerSet) {
    let verify_one = |header: *mut GcHeader| unsafe {
        let flags = (*header).gc_flags;
        if flags & GC_FLAG_FORWARDED != 0 {
            return;
        }
        // Mirror rewrite_heap_objects: only unmarked NURSERY objects are
        // dead this cycle. Unmarked old/longlived/malloc objects survive a
        // minor and must hold no stale forwarded references either — this
        // gate previously hid exactly the #5029 dangling old→old slots from
        // PERRY_GC_VERIFY_EVACUATION.
        if flags & (GC_FLAG_MARKED | GC_FLAG_PINNED) == 0 {
            let user = (header as *mut u8).add(GC_HEADER_SIZE) as usize;
            if crate::arena::pointer_in_nursery(user) {
                return;
            }
        }
        verify_heap_object_fields(header, valid_ptrs, "heap fields");
    };
    crate::arena::arena_walk_objects(|hp| verify_one(hp as *mut GcHeader));
    MALLOC_STATE.with(|s| {
        let s = s.borrow();
        for &h in s.objects.iter() {
            verify_one(h);
        }
    });
}

pub(super) fn verify_evacuated_no_stale_forwarded_refs(valid_ptrs: &ValidPointerSet) {
    verify_mutable_root_slots(valid_ptrs);
    verify_mutable_registered_roots(valid_ptrs);
    verify_copy_only_registered_roots(valid_ptrs);
    verify_remembered_dirty_ranges(valid_ptrs);
    verify_heap_objects(valid_ptrs);
}

/// Top-level Phase C4b-γ-2 entry: rewrite every reference site we
/// own. Skipped: conservatively-discovered C-stack words (we can't
/// safely overwrite arbitrary stack memory; pinning of conservative-
/// root targets in `gc_collect_minor` keeps those references valid
/// without rewriting). Legacy copy-only scanners still pin their own
/// discoveries directly during root marking.
pub(super) fn rewrite_forwarded_references(
    valid_ptrs: &ValidPointerSet,
    shadow_stats: Option<&mut ShadowRootTraceStats>,
    root_sources: Option<&mut RootSourcesTraceStats>,
) {
    match root_sources {
        Some(sources) => {
            rewrite_mutable_root_slots_with_sources(valid_ptrs, shadow_stats, Some(&mut *sources));
            rewrite_mutable_registered_roots_with_sources(valid_ptrs, Some(&mut *sources));
        }
        None => {
            rewrite_mutable_root_slots(valid_ptrs, shadow_stats);
            rewrite_mutable_registered_roots(valid_ptrs);
        }
    }
    rewrite_remembered_dirty_ranges(valid_ptrs);
    rewrite_heap_objects(valid_ptrs);
}

/// Gen-GC Phase C4b: is `header` pinned this cycle (cannot be
/// evacuated)? Tested by the evacuation candidate filter in
/// `gc_collect_minor` after the age-bump pass.
#[inline]
pub fn is_conservatively_pinned(header: *const GcHeader) -> bool {
    CONS_PINNED.with(|s| s.borrow().contains(&(header as usize)))
}

/// Test-only diagnostic: number of objects pinned this cycle.
pub fn cons_pinned_count() -> usize {
    CONS_PINNED.with(|s| s.borrow().len())
}
