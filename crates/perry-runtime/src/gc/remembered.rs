use super::*;

// Evacuation copies land in OLD_ARENA after the remembered-set scan
// for this cycle has already run. Rebuild only the pages for copied
// old objects that still hold nursery children so the next minor GC
// sees those old→young edges after the normal collection clear.
#[inline]
pub(super) unsafe fn remember_evacuated_old_to_young_slot(
    sticky: &mut StickyRememberedSet,
    parent_header: *mut GcHeader,
    slot: *mut u64,
) -> bool {
    remember_mutable_old_to_young_slot(sticky, parent_header, GcMutableSlot::new(slot, None))
}

#[inline]
pub(super) unsafe fn remember_mutable_old_to_young_slot(
    sticky: &mut StickyRememberedSet,
    parent_header: *mut GcHeader,
    mutable_slot: GcMutableSlot,
) -> bool {
    let slot = mutable_slot.slot;
    if slot.is_null() || !mutable_slot.is_parent_owned() {
        return false;
    }
    let bits = mutable_slot.read();
    let child_addr = decode_heap_addr(bits);
    // Nursery AND malloc-GC children both need their pages kept dirty:
    // minors sweep the malloc registry too, and old parents are black
    // leaves — dropping an old→malloc page here would free the malloc
    // child on the next minor (see remembered_child_needs_tracking).
    if child_addr == 0 || !crate::gc::barrier::remembered_child_needs_tracking(child_addr) {
        return false;
    }
    // Every rebuild (arming reconstruct, promotion repair) holds an old or
    // malloc parent here, so a young leaf in the slot has escaped.
    super::leaf_escape::probe_old_parent_child(child_addr);
    sticky.remember_slot(
        parent_header,
        slot,
        slot_is_external_to(parent_header, slot),
    );
    // Report the child's tracking requirement, not whether its page was new.
    // The repair diagnostic can count productive slots without decoding and
    // classifying the same child a second time.
    true
}

/// Full mark's observer-free visit already decoded this immutable slot word.
/// Do not use this entry across foreign handle observers; those visits retain
/// the slot-reading wrapper above. Tracking is independent of marking success.
#[inline(always)]
pub(super) unsafe fn remember_decoded_full_mark_slot(
    sticky: &mut StickyRememberedSet,
    parent_header: *mut GcHeader,
    mutable_slot: GcMutableSlot,
    word: mark::FieldWord,
    old_parent: bool,
) -> bool {
    let slot = mutable_slot.slot;
    if slot.is_null() || !mutable_slot.is_parent_owned() || !word.needs_tracking() {
        return false;
    }
    // The object-boundary proof replaces a generation lookup per inline
    // slot. Containment is still checked: old external buffers must enter
    // through their owning descriptor, never as bare old pages (#7538).
    let start = parent_header as usize;
    let external = !old_parent
        || (slot as usize) < start
        || (slot as usize) >= start + (*parent_header).size as usize;
    #[cfg(test)]
    if mark::remembered_mark_sabotage::drop_next_entry() {
        return false;
    }
    #[cfg(test)]
    let external = if mark::full_mark_decode_sabotage::generation_only_custody() {
        mutable_slot.external()
    } else {
        external
    };
    sticky.remember_slot(parent_header, slot, external);
    true
}

/// Is `slot` outside `parent_header`'s own allocation, or on a page the
/// old-page modbuf cannot describe?
///
/// #7538: this decides whether the re-arm records a bare page (found again by
/// walking the objects ON that page) or a `(page → owner header)` pair (found
/// again by re-entering the OWNER's descriptor). Deciding it by GENERATION —
/// "not old ⟹ external" — is right for a malloc side buffer and wrong for
/// every old-gen buffer a GC object owns but does not contain. The lazy JSON
/// array's sparse element cache is exactly that: a separate `GC_TYPE_STRING`
/// block, born old at ≥2049 elements, whose slots only
/// `GcRewriteDescriptorKind::LazyArray` on the owning `LazyArrayHeader` can
/// read. Re-armed as a bare old page, the next minor's dirty scan walked that
/// page, found the cache's own leaf header, and scanned nothing — so a cached
/// element pointer went stale one collection AFTER the store's own barrier
/// entry had been consumed and cleared. Containment is the question the
/// modbuf is actually asking.
#[inline]
pub(super) unsafe fn slot_is_external_to(parent_header: *mut GcHeader, slot: *mut u64) -> bool {
    if !matches!(
        crate::arena::classify_heap_generation(slot as usize),
        crate::arena::HeapGeneration::Old
    ) {
        return true;
    }
    let start = parent_header as usize;
    let total_size = (*parent_header).size as usize;
    if total_size == 0 {
        return true;
    }
    let slot_addr = slot as usize;
    slot_addr < start || slot_addr >= start + total_size
}

pub(super) unsafe fn remember_evacuated_old_copy_young_slots<const DIAGNOSTICS: bool>(
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
    // #10362: no child slot means no old->young edge to remember. This pass
    // ignores `PointerFreeRange`, so unlike the full mark it needs no proxy
    // term.
    if crate::gc::gc_object_yields_no_child_slots(header) {
        return;
    }
    visit_gc_rewrite_slots::<DIAGNOSTICS>(header, |slot| unsafe {
        if crate::weakref::is_weak_target_trace_slot(header, slot.slot)
            && (*header).obj_type != GC_TYPE_WEAK_STORAGE
        {
            return;
        }
        if DIAGNOSTICS {
            slot.record_layout_read()
        };
        remember_mutable_old_to_young_slot(sticky, header, slot);
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
///
/// #9754: `covered` names the objects the cycle's own dirty scan visited
/// COMPLETELY (every pointer slot on a dirty page and inside the body —
/// `scan_dirty_object_slots`). For those, `visit_slot_with_parent` already
/// re-remembered every slot whose post-visit child still needs tracking with
/// the same predicate this walk applies, so re-walking them can only re-insert
/// pages the sticky restore just inserted. They are skipped; the walk is then
/// proportional to the objects the dirty scan could NOT fully cover
/// (multi-page arrays, owners of out-of-body buffers) instead of to every slot
/// on every dirty page. Under `debug_assertions`, and in unit-test builds, the
/// skipped objects are walked anyway and any page the walk would have ADDED is
/// a panic — the machine check of the equivalence argument above. Including
/// `cfg(test)` keeps the check active in `cargo test --release` without adding
/// work to production release builds.
pub(super) fn restore_surviving_dirty_coverage<const DIAGNOSTICS: bool>(
    snapshot: &RememberedDirtySnapshot,
    covered: &crate::fast_hash::PtrHashSet<usize>,
    cycle_label: &str,
) {
    // Keep slot accounting out of the normal GC walk. Both instantiations
    // perform exactly the same repair; only the diagnostic one counts it.
    if DIAGNOSTICS && crate::gc::diagnostic_hooks::enabled() {
        restore_surviving_dirty_coverage_impl::<true>(snapshot, covered, cycle_label);
    } else {
        restore_surviving_dirty_coverage_impl::<false>(snapshot, covered, cycle_label);
    }
}

fn restore_surviving_dirty_coverage_impl<const DIAGNOSTICS: bool>(
    snapshot: &RememberedDirtySnapshot,
    covered: &crate::fast_hash::PtrHashSet<usize>,
    cycle_label: &str,
) {
    let mut sticky = StickyRememberedSet::default();
    let mut walked = 0usize;
    let mut skipped = 0usize;
    let mut parents_visited = 0usize;
    let mut slots_visited = 0usize;
    let mut slots_tracking = 0usize;
    #[cfg(any(test, debug_assertions))]
    let mut skipped_sticky = StickyRememberedSet::default();
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
        if DIAGNOSTICS {
            parents_visited += 1;
        }
        visit_gc_rewrite_slots::<DIAGNOSTICS>(header, |slot| {
            if DIAGNOSTICS {
                // Count all enumerated slots, including unproductive weak
                // targets and primitive values. This measures traversal work.
                slots_visited += 1;
            }
            if crate::weakref::is_weak_target_trace_slot(header, slot.slot)
                && (*header).obj_type != GC_TYPE_WEAK_STORAGE
            {
                return;
            }
            if DIAGNOSTICS {
                slot.record_layout_read()
            };
            let tracking = remember_mutable_old_to_young_slot(&mut sticky, header, slot);
            if DIAGNOSTICS && tracking {
                slots_tracking += 1;
            }
        });
    };
    if !snapshot.dirty_old_pages.is_empty() {
        crate::arena::old_arena_walk_objects_on_pages(&snapshot.dirty_old_pages, |hp| {
            if covered.contains(&(hp as usize)) {
                skipped += 1;
                #[cfg(any(test, debug_assertions))]
                cross_check_covered_parent(hp as *mut GcHeader, &mut skipped_sticky);
                return;
            }
            walked += 1;
            visit_parent(hp as *mut GcHeader);
        });
    }
    let mut seen_external = crate::fast_hash::new_ptr_hash_set();
    for &(_, header_addr) in &snapshot.external_dirty_entries {
        if !seen_external.insert(header_addr) {
            continue;
        }
        if covered.contains(&header_addr) {
            skipped += 1;
            #[cfg(any(test, debug_assertions))]
            cross_check_covered_parent(header_addr as *mut GcHeader, &mut skipped_sticky);
            continue;
        }
        walked += 1;
        // External entries may be stale (or, in the GC unit tests,
        // synthetic). Establish that the address is dereference-safe
        // WITHOUT touching it: old/longlived arena pages are always
        // mapped; anything else must still be a registered malloc GC
        // object.
        let deref_safe =
            matches!(
                crate::arena::classify_heap_generation(header_addr),
                crate::arena::HeapGeneration::Old | crate::arena::HeapGeneration::Longlived
            ) || super::malloc::gc_malloc_header_is_local(header_addr as *const GcHeader);
        if deref_safe {
            visit_parent(header_addr as *mut GcHeader);
        }
    }
    let added = sticky.restore_counted();
    #[cfg(any(test, debug_assertions))]
    {
        let would_add = skipped_sticky.count_not_yet_dirty();
        assert_eq!(
            would_add, 0,
            "restore_surviving_dirty_coverage: {would_add} page(s) of {skipped} \
             dirty-scan-covered object(s) are not remembered — the dirty scan's \
             per-slot re-remembering disagrees with the coverage walk for an \
             object `scan_dirty_object_slots` reported complete"
        );
    }
    if DIAGNOSTICS {
        // These are the two actual snapshot inputs and the skip-set size.
        // `dirty_pages` also includes external pages and is NOT the set the
        // old-arena walk iterates. Candidate counts precede validity guards;
        // parent visits and slot counts describe the admitted traversal.
        eprintln!(
            "[gc-restore-coverage] {cycle_label} dirty_old_pages={} external_entries={} \
             covered={} objects_walked={walked} objects_skipped={skipped} \
             parents_visited={parents_visited} slots_visited={slots_visited} \
             slots_tracking={slots_tracking} pages_added={added}",
            snapshot.dirty_old_pages.len(),
            snapshot.external_dirty_entries.len(),
            covered.len(),
        );
    }
}

/// Test/debug twin of the restore's `visit_parent` for a skipped object:
/// re-derive what the full walk would have remembered so the caller can assert
/// it adds nothing beyond what the dirty scan already restored.
#[cfg(any(test, debug_assertions))]
fn cross_check_covered_parent(header: *mut GcHeader, sticky: &mut StickyRememberedSet) {
    unsafe {
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
        visit_gc_rewrite_slots::<{ cfg!(test) }>(header, |slot| {
            if crate::weakref::is_weak_target_trace_slot(header, slot.slot)
                && (*header).obj_type != GC_TYPE_WEAK_STORAGE
            {
                return;
            }
            slot.record_layout_read();
            remember_mutable_old_to_young_slot(sticky, header, slot);
        });
    }
}

pub(super) fn rebuild_evacuated_old_to_young_remembered_set<const DIAGNOSTICS: bool>(
    evacuated_headers: &[*mut GcHeader],
) -> StickyRememberedSet {
    let mut sticky = StickyRememberedSet::default();
    for &header in evacuated_headers {
        unsafe {
            remember_evacuated_old_copy_young_slots::<DIAGNOSTICS>(&mut sticky, header);
        }
    }
    sticky
}

unsafe fn remember_retained_old_to_young_slots<const DIAGNOSTICS: bool>(
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
    if !barrier_parent_needs_remembering(user_ptr as usize) {
        return;
    }
    visit_gc_rewrite_slots::<DIAGNOSTICS>(header, |slot| unsafe {
        if crate::weakref::is_weak_target_trace_slot(header, slot.slot)
            && (*header).obj_type != GC_TYPE_WEAK_STORAGE
        {
            return;
        }
        if DIAGNOSTICS {
            slot.record_layout_read()
        };
        remember_mutable_old_to_young_slot(sticky, header, slot);
    });
}

pub(super) struct OldToYoungRememberedRebuildState {
    require_marked: bool,
    sticky: StickyRememberedSet,
    arena_cursor: Option<crate::arena::ArenaObjectCursor>,
    arena_done: bool,
    malloc_index: usize,
    objects_scanned: usize,
    done: bool,
}

impl OldToYoungRememberedRebuildState {
    pub(super) fn new(require_marked: bool) -> Self {
        // Arena parents on uniformly young or longlived blocks cannot pass
        // barrier_parent_needs_remembering. Keep every other range, including
        // blocks retagged old before in-place promotion changes their owner.
        // Malloc parents retain their separate walk below.
        let arena_cursor = crate::arena::ArenaObjectCursor::new_remembered_parents();
        Self {
            require_marked,
            sticky: StickyRememberedSet::default(),
            arena_cursor: Some(arena_cursor),
            arena_done: false,
            malloc_index: 0,
            objects_scanned: 0,
            done: false,
        }
    }

    /// Reference implementation for equivalence/visited-work regression tests.
    #[cfg(test)]
    pub(super) fn whole_heap_for_test(require_marked: bool) -> Self {
        let mut state = Self::new(require_marked);
        state.arena_cursor = Some(crate::arena::ArenaObjectCursor::new(
            crate::arena::ArenaWalkOrder::BlockIndex,
        ));
        state
    }

    /// Candidate arena and malloc objects visited by the arming reconstruct
    /// or the test reference walk. Full cycles fold their entries into marking
    /// and no longer construct this state.
    pub(super) fn objects_scanned(&self) -> usize {
        self.objects_scanned
    }

    pub(super) fn step(&mut self, budget: usize) -> bool {
        self.step_for_cycle::<{ cfg!(test) }>(budget)
    }

    pub(super) fn step_for_cycle<const DIAGNOSTICS: bool>(&mut self, budget: usize) -> bool {
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
            self.objects_scanned += 1;
            let header = header_ptr as *mut GcHeader;
            unsafe {
                remember_retained_old_to_young_slots::<DIAGNOSTICS>(
                    &mut self.sticky,
                    header,
                    self.require_marked,
                );
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
            self.objects_scanned += 1;
            unsafe {
                remember_retained_old_to_young_slots::<DIAGNOSTICS>(
                    &mut self.sticky,
                    header,
                    self.require_marked,
                );
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

/// The arming reconstruct's walk (#7187): every retained old parent's
/// old→young edges.
///
/// It skips every arena block that is wholly NURSERY. The walk keeps a parent
/// only if `barrier_parent_needs_remembering` says so — an Old-generation
/// object, or a malloc object — and an object on a nursery block is neither,
/// so each object there was a classification and a rejection. Skipping the
/// block is therefore exact, not a heuristic. It matters because the walk runs
/// at the first minor of every thread, when the young generation is at its
/// fullest: on binary-trees at n = 3 it visited 131 k young objects to find no
/// parent, about 20 M instructions.
pub(super) fn rebuild_minor_old_to_young_remembered_set<const DIAGNOSTICS: bool>(
) -> StickyRememberedSet {
    let mut state = OldToYoungRememberedRebuildState::new(false);
    if let Some(cursor) = state.arena_cursor.as_mut() {
        cursor.skip_blocks_where(arming_walk_skips_block);
    }
    while !state.step_for_cycle::<DIAGNOSTICS>(usize::MAX) {}
    super::barrier_arming::note_reconstruct_objects_walked(state.objects_scanned());
    state.finish()
}

/// Can the arming walk skip the block `[data, end)` without looking? Only when
/// one registered range covers it and that range is the nursery.
fn arming_walk_skips_block(data: usize, end: usize) -> bool {
    #[cfg(test)]
    if arming_walk_sabotage::skipping_every_block() {
        return true;
    }
    matches!(
        crate::arena::uniform_heap_generation(data, end),
        Some(crate::arena::HeapGeneration::Nursery)
    )
}

/// Test-only sabotage for [`arming_walk_skips_block`]: skip EVERY block, old
/// ones included, so the witness can prove that the old→young edge it recovers
/// comes from the walk it is testing.
#[cfg(test)]
pub(crate) mod arming_walk_sabotage {
    use std::cell::Cell;

    thread_local! {
        static SKIP_ALL: Cell<bool> = const { Cell::new(false) };
    }

    pub(crate) fn skipping_every_block() -> bool {
        SKIP_ALL.with(Cell::get)
    }

    pub(crate) struct Guard(bool);

    impl Guard {
        pub(crate) fn arm() -> Self {
            Self(SKIP_ALL.with(|s| s.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            let prior = self.0;
            SKIP_ALL.with(|s| s.set(prior));
        }
    }
}
