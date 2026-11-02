use super::*;

/// Snapshot malloc-backed headers before invoking a verifier callback.
///
/// Slot validation can exact-check a candidate malloc pointer, which lazily
/// builds `MallocState.set` under a mutable borrow. Keeping even a shared
/// `MALLOC_STATE` borrow across that validation would make the diagnostic
/// verifier re-enter the same `RefCell` and panic instead of checking the heap.
#[inline]
pub(super) fn malloc_headers_for_verification() -> Vec<*mut GcHeader> {
    MALLOC_STATE.with(|state| state.borrow().objects.clone())
}

/// Calls `f` with the user pointer of every marked or pinned, non-forwarded
/// `GC_TYPE_OBJECT` in the arena. At the sweep start of a synchronous full
/// collection marks are final and nothing is swept, so "marked" is exactly
/// "live". The header walk lives here, with the collector, because it reads
/// headers by linear block iteration rather than from a NaN-box payload.
#[cfg(feature = "shape-fact-audit")]
pub(crate) fn for_each_live_object_at_sweep_start(
    mut f: impl FnMut(*const crate::object::ObjectHeader),
) {
    crate::arena::arena_walk_objects(|header_ptr| unsafe {
        let header = &*(header_ptr as *const GcHeader);
        let flags = header.gc_flags;
        if crate::gc_type_is_not_object!(header.obj_type)
            || flags & GC_FLAG_FORWARDED != 0
            || flags & (GC_FLAG_MARKED | GC_FLAG_PINNED) == 0
        {
            return;
        }
        f(header_ptr.add(GC_HEADER_SIZE) as *const crate::object::ObjectHeader);
    });
}

/// The copying minor retains non-moving array-growth stubs. Other evacuation
/// paths rewrite every forwarding alias before releasing moved originals.
/// Carry that distinction through every verifier surface, including FFI roots.
#[derive(Clone, Copy)]
pub(super) struct EvacuationVerifier<'a> {
    pub(super) valid_ptrs: &'a ValidPointerSet,
    copying_minor: bool,
    pub(super) context: Option<EvacuationVerifyCycleContext<'a>>,
    pub(super) parent_header: Option<*mut GcHeader>,
}

impl<'a> EvacuationVerifier<'a> {
    pub(super) fn all_forwarded(valid_ptrs: &'a ValidPointerSet) -> Self {
        Self {
            valid_ptrs,
            copying_minor: false,
            context: None,
            parent_header: None,
        }
    }

    /// Must run before the copying minor resets from-space and flips survivors.
    pub(super) fn copying_minor(valid_ptrs: &'a ValidPointerSet) -> Self {
        Self {
            valid_ptrs,
            copying_minor: true,
            context: None,
            parent_header: None,
        }
    }

    pub(super) fn with_context(mut self, context: EvacuationVerifyCycleContext<'a>) -> Self {
        self.context = Some(context);
        self
    }

    fn with_parent(mut self, parent_header: *mut GcHeader) -> Self {
        self.parent_header = Some(parent_header);
        self
    }

    pub(super) fn stale_raw_addr(self, addr: usize) -> Option<usize> {
        follow_forwarding_raw_addr(addr, self.valid_ptrs, |source, target| {
            if !self.copying_minor {
                return true;
            }
            // Only array growth creates permanent forwarding aliases. The
            // copying minor neither moves nor frees these non-moving sources.
            // Both ends must be retained arrays. An old -> young growth edge
            // is forbidden even if the young target was never forwarded.
            // Follow the whole chain so an indirect unsafe hop is also caught.
            !self.retained_growth_array(source) || !self.retained_growth_array(target)
        })
    }

    fn retained_growth_array(self, addr: usize) -> bool {
        if !self.valid_ptrs.contains(&addr) {
            return false;
        }
        let header = unsafe { header_from_user_ptr(addr as *const u8) };
        (unsafe { (*header).obj_type == GC_TYPE_ARRAY })
            && matches!(
                crate::arena::classify_heap_space(addr),
                crate::arena::HeapSpace::Old
                    | crate::arena::HeapSpace::Longlived
                    | crate::arena::HeapSpace::PromotedYoung
            )
    }

    pub(super) fn stale_value(self, bits: u64) -> Option<u64> {
        let word = decode_root_word(bits)?;
        Some(word.encode(self.stale_raw_addr(word.addr())?))
    }

    pub(super) fn stale_nanboxed_value(self, bits: u64) -> Option<u64> {
        let word = decode_nanboxed_root_word(bits)?;
        Some(word.encode(self.stale_raw_addr(word.addr())?))
    }
}

#[cold]
pub(super) fn panic_stale_forwarded_reference(
    verifier: EvacuationVerifier<'_>,
    surface: &str,
    slot_addr: usize,
    old_bits: u64,
    new_bits: u64,
) -> ! {
    panic_stale_forwarded_reference_detailed(verifier, surface, slot_addr, old_bits, new_bits);
}

#[cfg(test)]
#[inline]
pub(super) unsafe fn verify_slot(
    slot: *const u64,
    verifier: EvacuationVerifier<'_>,
    surface: &str,
) {
    let bits = *slot;
    if let Some(new_bits) = verifier.stale_value(bits) {
        panic_stale_forwarded_reference(verifier, surface, slot as usize, bits, new_bits);
    }
}

#[inline]
unsafe fn verify_mutable_slot(
    slot: GcMutableSlot,
    verifier: EvacuationVerifier<'_>,
    surface: &str,
) {
    let bits = slot.read();
    if let Some(new_bits) = verifier.stale_value(bits) {
        panic_stale_forwarded_reference(verifier, surface, slot.slot as usize, bits, new_bits);
    }
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

/// Can the next minor's dirty scan actually REACH `slot`?
///
/// Two coverage forms, and which one applies is a question of CONTAINMENT,
/// not of generation (#7538). A bare dirty old page is found again by walking
/// the objects on it, so it only proves reachability for a slot that lives
/// inside a heap object whose own descriptor enumerates it. A slot in a buffer
/// the parent merely POINTS AT is reachable only through the parent's
/// descriptor, which needs the `(page → owner header)` external pair.
///
/// Dispatching on generation instead accepted a bare page mark for any old-gen
/// external buffer. The lazy JSON array's sparse element cache is exactly
/// that — a separate `GC_TYPE_STRING` block, born old at ≥2049 elements,
/// readable only by `GcRewriteDescriptorKind::LazyArray` on the owning
/// `LazyArrayHeader` — so the verifier's own predicate agreed with the
/// producer's wrong barrier and neither could see the hole.
///
/// An external pair is accepted for an in-object slot too: it strictly
/// implies reachability, and the malloc-parent barrier
/// (`runtime_write_barrier_gc_slot`) legitimately emits it.
#[inline]
pub(super) unsafe fn old_young_slot_covered(
    snapshot: &RememberedDirtySnapshot,
    parent_header: usize,
    slot: *mut u64,
) -> bool {
    if old_young_external_slot_covered(snapshot, parent_header, slot) {
        return true;
    }
    if slot_is_external_to(parent_header as *mut GcHeader, slot) {
        return false;
    }
    snapshot
        .dirty_old_pages
        .contains(&crate::arena::generation_page_for_addr(slot as usize))
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
    mutable_slot: GcMutableSlot,
) {
    let slot = mutable_slot.slot;
    if slot.is_null() {
        return;
    }
    let child_addr = decode_heap_addr(mutable_slot.read());
    if child_addr == 0 || !crate::gc::barrier::remembered_child_needs_tracking(child_addr) {
        return;
    }
    stats.checked_old_to_young_edges = stats.checked_old_to_young_edges.saturating_add(1);
    let parent_addr = parent_header as usize;
    if !old_young_slot_covered(snapshot, parent_addr, slot) {
        // gh #6206: record parent/child GC types so the panic below can
        // print per-type histograms of the dropped edges.
        let parent_obj_type = (*parent_header).obj_type;
        let parent_marked = (*parent_header).gc_flags & (GC_FLAG_MARKED | GC_FLAG_PINNED) != 0;
        let parent_user = (parent_header as *mut u8).add(GC_HEADER_SIZE) as usize;
        let parent_is_old_arena = matches!(
            crate::arena::classify_heap_generation(parent_user),
            crate::arena::HeapGeneration::Old
        );
        let child_obj_type = {
            let ch = (child_addr as *const u8).sub(GC_HEADER_SIZE) as *const GcHeader;
            (*ch).obj_type
        };
        stats.record_missing_diag(
            parent_addr,
            slot as usize,
            child_addr,
            parent_obj_type,
            child_obj_type,
            parent_is_old_arena,
            parent_marked,
        );
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
    visit_gc_rewrite_slots::<true>(header, |slot| unsafe {
        if crate::weakref::is_weak_target_trace_slot(header, slot.slot)
            && (*header).obj_type != GC_TYPE_WEAK_STORAGE
        {
            return;
        }
        // Shared runtime-root words are traced through receivers, but their
        // root scanner covers them between minors, independently of any one
        // carrier's lifetime. Use the same custody as remembering.
        if !slot.is_parent_owned() {
            return;
        }
        slot.record_layout_read();
        verify_old_young_slot_covered(snapshot, stats, header, slot);
    });
}

#[cold]
pub(super) fn panic_old_young_edge_verifier_failed(stats: OldYoungEdgeVerifyStats) -> ! {
    let missing = stats.first_missing.unwrap_or_default();
    // gh #6206: readable per-type histograms of the missing edges.
    let type_name = |t: u8| -> &'static str { gc_type_info(t).map_or("?", |i| i.name) };
    let mut parent_hist = String::new();
    let mut child_hist = String::new();
    for t in 0u8..32 {
        let p = stats.missing_by_parent_type[t as usize];
        if p != 0 {
            parent_hist.push_str(&format!(" {}({})={}", type_name(t), t, p));
        }
        let c = stats.missing_by_child_type[t as usize];
        if c != 0 {
            child_hist.push_str(&format!(" {}({})={}", type_name(t), t, c));
        }
    }
    // Decisive split for the missing-edge hunt: was the slot's page EVER dirtied
    // (edge recorded by a barrier then LOST by a clear/restore gap) or never
    // (a store path that skips the barrier entirely)?
    let slot_page_ever_dirty =
        super::barrier::ever_dirty_old_page(crate::arena::generation_page_for_addr(missing.slot));
    panic!(
        "old-young-edge-verifier failed: checked_old_objects={} checked_remembered_pages={} checked_old_to_young_edges={} missing_edges={} malloc_parents={} unmarked_parents={}\n  first_missing: parent=0x{:x} type={}({}) old_arena={} marked={} slot=0x{:x} child=0x{:x} child_type={}({}) slot_page_ever_dirty={slot_page_ever_dirty}\n  missing_by_parent_type:{}\n  missing_by_child_type:{}",
        stats.checked_old_objects,
        stats.checked_remembered_pages,
        stats.checked_old_to_young_edges,
        stats.missing_edges,
        stats.missing_parent_malloc,
        stats.missing_parent_unmarked,
        missing.parent,
        type_name(missing.parent_obj_type),
        missing.parent_obj_type,
        missing.parent_is_old_arena,
        missing.parent_marked,
        missing.slot,
        missing.child,
        type_name(missing.child_obj_type),
        missing.child_obj_type,
        parent_hist,
        child_hist,
    );
}

pub(super) fn verify_old_to_young_edges_collect() -> OldYoungEdgeVerifyStats {
    let snapshot = remembered_dirty_snapshot::<true>();
    let mut stats = OldYoungEdgeVerifyStats {
        checked_remembered_pages: snapshot.dirty_pages.len(),
        ..OldYoungEdgeVerifyStats::default()
    };
    crate::arena::old_arena_walk_objects(|hp| unsafe {
        verify_old_young_parent_slots_covered(&snapshot, &mut stats, hp as *mut GcHeader);
    });
    for header in malloc_headers_for_verification() {
        unsafe {
            verify_old_young_parent_slots_covered(&snapshot, &mut stats, header);
        }
    }
    // WRT: every image cell holding a collectable child, of any generation,
    // must be in the image's remembered set (`gc::image_cells`).
    super::image_cells::verify_image_cells_remembered(&mut stats);
    stats
}

pub(super) fn verify_old_to_young_edges_covered() -> OldYoungEdgeVerifyStats {
    let stats = verify_old_to_young_edges_collect();
    if stats.missing_edges != 0 {
        // gh #6206: this check runs at AtomicFinalize, AFTER the mark phase
        // consumed the dirty logs, so its "missing" edges can be a
        // measurement artifact. PERRY_GC_VERIFY_RS_NONFATAL=1 demotes it to
        // a warning so the stale-forwarded-refs verifier (which runs later
        // in the same cycle) can be reached.
        use std::sync::OnceLock;
        static NONFATAL: OnceLock<bool> = OnceLock::new();
        if *crate::once_init::get_or_init(&NONFATAL, || {
            super::env_flag_enabled("PERRY_GC_VERIFY_RS_NONFATAL")
        }) {
            eprintln!(
                "[gc-verify] old-young-edge-verifier (non-fatal): missing_edges={}",
                stats.missing_edges
            );
        } else {
            panic_old_young_edge_verifier_failed(stats);
        }
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
#[allow(dead_code)] // GC heap-invariant verifier (PERRY_GC_VERIFY_EVACUATION); driven by verify_marked_heap_no_unmarked_children and its cfg(test) callers in gc/tests/barrier.rs
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
    visit_gc_rewrite_slots::<true>(header, |slot| unsafe {
        if crate::weakref::is_weak_target_trace_slot(header, slot.slot)
            && !super::ephemeron::value_slot_is_enabled(header, slot.slot, false)
        {
            return;
        }
        slot.record_layout_read();
        let Some((child, child_header)) = current_heap_header_for_heap_word(slot.read(), None)
        else {
            return;
        };
        stats.checked_edges = stats.checked_edges.saturating_add(1);
        if (*child_header).gc_flags & (GC_FLAG_MARKED | GC_FLAG_PINNED) == 0 {
            stats.record_missing(parent, slot.slot as usize, child);
        }
    });
}

/// One array element the collector's own slot enumeration does not reach,
/// even though the element holds a heap reference.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct UnenumeratedArraySlot {
    pub(super) array: usize,
    pub(super) index: usize,
    pub(super) length: u32,
    pub(super) reserved: u16,
    pub(super) child: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct ArraySlotEnumerationStats {
    pub(super) checked_arrays: usize,
    pub(super) checked_pointer_slots: usize,
    pub(super) unenumerated_slots: usize,
    pub(super) first: Option<UnenumeratedArraySlot>,
}

/// Does the collector's own slot enumeration reach every element of `header`
/// that holds a heap reference?
///
/// This is the invariant the per-object pointer LAYOUT rests on, asked of the
/// one walk that consumes it. `heap_payload_slot_selection` may answer with a
/// full scan, an all-pointer claim, or a per-object mask; the first two cannot
/// under-report, the mask can — and when it does, the omitted element is a live
/// edge that marking never marks and the evacuation rewrite never rewrites. The
/// child is then swept while its owner still names it, and the collector reads
/// the recycled bytes as a `GcHeader` one or more cycles later.
///
/// That is #9261: an object's spill (overflow-field) buffer whose mask covered
/// every `POINTER_TAG` element and omitted three live `STRING_TAG` ones
/// (`mask=0xc7fc live=0xfffc missing=0x3800`), surfacing ~200 collections later
/// as `[gc-pin-latch] FATAL … obj_type=10 size=1347565393` — payload bytes read
/// as a header. Nothing between the store and that abort could see it: the
/// store paths all note the layout correctly, and no runtime GC probe asks this
/// question. So ask it directly.
///
/// Arrays only, deliberately: an array's payload is `length` elements at a
/// known offset, so "which words are supposed to be reachable" needs no layout
/// interpretation and the check cannot inherit the bug it is looking for.
pub(super) unsafe fn verify_array_pointer_slots_enumerated_for(
    stats: &mut ArraySlotEnumerationStats,
    header: *mut GcHeader,
) {
    if header.is_null() || (*header).obj_type != GC_TYPE_ARRAY {
        return;
    }
    let flags = (*header).gc_flags;
    // A FORWARDED array is a growth stub or an evacuation original: its first
    // payload word is a forwarding pointer, not an element.
    if flags & GC_FLAG_FORWARDED != 0 {
        return;
    }
    let user = (header as *mut u8).add(GC_HEADER_SIZE) as usize;
    let arr = user as *const crate::array::ArrayHeader;
    let length = (*arr).length as usize;
    let capacity = (*arr).capacity as usize;
    // The same admission gate `array::gc_element_slot_range` applies: outside
    // it the collector enumerates nothing, and neither should this.
    if length == 0 || length > capacity || length > 16_000_000 {
        return;
    }
    let elements =
        crate::array::array_elements_ptr(user as *const crate::array::ArrayHeader) as *mut u8;
    let elements_addr = elements as usize;
    let words = elements as *const u64;

    let mut reached = vec![0u64; length.div_ceil(64)];
    visit_gc_rewrite_slots::<true>(header, |slot| {
        let addr = slot.slot as usize;
        if addr < elements_addr {
            return;
        }
        let index = (addr - elements_addr) / std::mem::size_of::<u64>();
        if index < length {
            reached[index / 64] |= 1u64 << (index % 64);
        }
    });

    stats.checked_arrays += 1;
    for index in 0..length {
        let bits = *words.add(index);
        if !super::layout::layout_pointer_bearing_bits(bits) {
            continue;
        }
        stats.checked_pointer_slots += 1;
        if reached[index / 64] & (1u64 << (index % 64)) != 0 {
            continue;
        }
        stats.unenumerated_slots += 1;
        if stats.first.is_none() {
            stats.first = Some(UnenumeratedArraySlot {
                array: user,
                index,
                length: (*arr).length,
                reserved: (*header)._reserved,
                child: (bits & POINTER_MASK) as usize,
            });
        }
    }
}

/// Is this array dead this cycle, so that its elements are whatever the
/// previous tenant of the address left behind rather than live edges?
///
/// Mirrors `verify_heap_objects`' gate exactly. It belongs to the heap WALK
/// and not to the per-object check: liveness is only decidable while marks are
/// final, and the per-object form is also asked about a specific array by
/// tests, where nothing has marked anything.
unsafe fn array_is_sweep_eligible(header: *mut GcHeader) -> bool {
    let flags = (*header).gc_flags;
    flags & (GC_FLAG_MARKED | GC_FLAG_PINNED) == 0
        && crate::arena::pointer_in_nursery((header as *mut u8).add(GC_HEADER_SIZE) as usize)
}

/// A slot in an array's `[length, capacity)` that does not hold `TAG_HOLE`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ArrayHoleTailViolation {
    pub array: usize,
    pub index: u32,
    pub length: u32,
    pub capacity: u32,
    pub bits: u64,
}

/// The first array whose `[length, capacity)` is not all `TAG_HOLE`.
///
/// The runtime keeps that range hole-filled (`array::array_truncate_length`)
/// because emitted element reads bound an index by `capacity` and read any
/// slot past `length` as a hole. A length decrease that leaves the old value
/// behind makes a read past the new `length` return it. This walk is the
/// invariant's falsifier: debug builds run it at the start of every
/// collection and panic on a violation, release builds when
/// `PERRY_GC_VERIFY_ARRAY_HOLES` is set.
pub(crate) fn verify_array_hole_tails() -> Option<ArrayHoleTailViolation> {
    let mut first = None;
    let mut check = |header: *mut GcHeader| unsafe {
        if first.is_some()
            || header.is_null()
            || (*header).obj_type != GC_TYPE_ARRAY
            || (*header).gc_flags & GC_FLAG_FORWARDED != 0
        {
            return;
        }
        let arr = (header as *mut u8).add(GC_HEADER_SIZE) as *mut crate::array::ArrayHeader;
        let (length, capacity) = ((*arr).length, (*arr).capacity);
        if length >= capacity {
            return;
        }
        let elements = crate::array::array_elements_ptr(arr);
        for index in length..capacity {
            let bits = *elements.add(index as usize);
            if bits != crate::value::TAG_HOLE {
                first = Some(ArrayHoleTailViolation {
                    array: arr as usize,
                    index,
                    length,
                    capacity,
                    bits,
                });
                return;
            }
        }
    };
    crate::arena::arena_walk_objects(|hp| check(hp as *mut GcHeader));
    for header in malloc_headers_for_verification() {
        check(header);
    }
    first
}

fn gc_verify_array_holes_enabled() -> bool {
    if cfg!(debug_assertions) {
        return true;
    }
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *crate::once_init::get_or_init(&ENABLED, || {
        super::env_flag_enabled("PERRY_GC_VERIFY_ARRAY_HOLES")
    })
}

/// [`verify_array_hole_tails`] at a collection entry (every entry that builds
/// the stack maps: the minor, compacting, evacuating and full collections and
/// both budgeted cycle starts); panics on a violation.
pub(super) fn verify_array_hole_tails_at_collection() {
    if !gc_verify_array_holes_enabled() {
        return;
    }
    if let Some(v) = verify_array_hole_tails() {
        panic!(
            "[gc-array-holes] array 0x{:x}: slot {} of [length {}, capacity {}) holds 0x{:x}, \
             not TAG_HOLE — a length decrease skipped array_truncate_length",
            v.array, v.index, v.length, v.capacity, v.bits
        );
    }
}

/// [`verify_array_pointer_slots_enumerated_for`] over every live array.
pub(super) fn verify_array_pointer_slots_enumerated() -> ArraySlotEnumerationStats {
    let mut stats = ArraySlotEnumerationStats::default();
    crate::arena::arena_walk_objects(|hp| unsafe {
        let header = hp as *mut GcHeader;
        if array_is_sweep_eligible(header) {
            return;
        }
        verify_array_pointer_slots_enumerated_for(&mut stats, header);
    });
    for header in malloc_headers_for_verification() {
        unsafe {
            verify_array_pointer_slots_enumerated_for(&mut stats, header);
        }
    }
    stats
}

/// `PERRY_GC_VERIFY_MARK` report form. Non-fatal, like its two siblings: the
/// point is to name the array, the element and the child at the cycle the
/// invariant breaks, instead of leaving a garbage-header abort hundreds of
/// collections downstream.
pub(super) fn verify_array_pointer_slots_enumerated_report(phase: &str) {
    let stats = verify_array_pointer_slots_enumerated();
    match stats.first {
        Some(missing) => eprintln!(
            "[gc-array-slots:{}] UNENUMERATED slots={} arrays={} pointer_slots={} | first array=0x{:x} index={} length={} reserved=0x{:x} child=0x{:x}",
            phase,
            stats.unenumerated_slots,
            stats.checked_arrays,
            stats.checked_pointer_slots,
            missing.array,
            missing.index,
            missing.length,
            missing.reserved,
            missing.child,
        ),
        None => eprintln!(
            "[gc-array-slots:{}] OK arrays={} pointer_slots={}",
            phase, stats.checked_arrays, stats.checked_pointer_slots,
        ),
    }
}

#[allow(dead_code)] // GC heap-invariant verifier exercised by cfg(test) suite in gc/tests/barrier.rs
/// An image cell's own slots after a full mark: every collectable child is
/// marked (the image root pass made it a root). Only parent-owned slots, as
/// the old→young gate checks: the shape record's keys word the layout also
/// yields is a shared runtime root with its own scanner.
#[cold]
#[optimize(size)]
#[inline(never)]
unsafe fn verify_image_cell_child_marks(
    stats: &mut MarkInvariantVerifyStats,
    header: *mut GcHeader,
) {
    stats.checked_marked_objects = stats.checked_marked_objects.saturating_add(1);
    visit_gc_rewrite_slots::<true>(header, |slot| {
        if !slot.is_parent_owned() {
            return;
        }
        let child = decode_heap_addr(slot.read());
        if child == 0 || !super::image_cells::image_child_is_collectable(child) {
            return;
        }
        stats.checked_edges = stats.checked_edges.saturating_add(1);
        let flags = (*header_from_user_ptr(child as *const u8)).gc_flags;
        if flags & (GC_FLAG_MARKED | GC_FLAG_PINNED) == 0 {
            stats.record_missing(header as usize, slot.slot as usize, child);
        }
    });
}

#[allow(dead_code)] // Test-facing whole-heap verifier; release uses the nonfatal report.
pub(super) fn verify_marked_heap_no_unmarked_children() -> MarkInvariantVerifyStats {
    let mut stats = MarkInvariantVerifyStats::default();
    crate::arena::arena_walk_objects(|hp| unsafe {
        verify_marked_object_child_marks(&mut stats, hp as *mut GcHeader);
    });
    // WRT: image cells are born marked; their children are roots.
    super::image_cells::for_each_image_cell(|header| unsafe {
        verify_image_cell_child_marks(&mut stats, header);
    });
    for header in malloc_headers_for_verification() {
        unsafe {
            verify_marked_object_child_marks(&mut stats, header);
        }
    }
    if stats.missing_edges != 0 {
        panic_mark_invariant_verifier_failed(stats);
    }
    stats
}

/// Non-fatal mark-invariant probe (`PERRY_GC_VERIFY_MARK`): walks the marked
/// heap and, instead of panicking, logs the first marked→UNMARKED-child edge
/// with parent/child obj_types. Lets the bundle be driven to reproduce a
/// swept-live-child (freed Map value) without aborting. Diagnostic only.
pub(super) fn verify_marked_heap_report_nonfatal(phase: &str) {
    let mut stats = MarkInvariantVerifyStats::default();
    crate::arena::arena_walk_objects(|hp| unsafe {
        verify_marked_object_child_marks(&mut stats, hp as *mut GcHeader);
    });
    super::image_cells::for_each_image_cell(|header| unsafe {
        verify_image_cell_child_marks(&mut stats, header);
    });
    for header in malloc_headers_for_verification() {
        unsafe {
            verify_marked_object_child_marks(&mut stats, header);
        }
    }
    let tn = |t: u8| gc_type_info(t).map_or("?", |i| i.name);
    if let Some(m) = stats.first_missing {
        let (ptype, ctype) = unsafe {
            let ph = (m.parent as *const u8).sub(GC_HEADER_SIZE) as *const GcHeader;
            let ch = (m.child as *const u8).sub(GC_HEADER_SIZE) as *const GcHeader;
            ((*ph).obj_type, (*ch).obj_type)
        };
        eprintln!(
            "[gc-mark-verify:{}] marked->UNMARKED edges={} checked_marked={} checked_edges={} | first parent=0x{:x} ptype={}({}) slot=0x{:x} child=0x{:x} ctype={}({})",
            phase,
            stats.missing_edges,
            stats.checked_marked_objects,
            stats.checked_edges,
            m.parent,
            tn(ptype),
            ptype,
            m.slot,
            m.child,
            tn(ctype),
            ctype,
        );
    } else {
        eprintln!(
            "[gc-mark-verify:{}] OK (no marked->unmarked) checked_marked={} checked_edges={}",
            phase, stats.checked_marked_objects, stats.checked_edges,
        );
    }
}

/// Non-fatal minor-sweep probe (`PERRY_GC_VERIFY_MARK`): at the minor's
/// AtomicFinalize→Sweep boundary (marks final, nothing freed yet), walk every
/// OLD-gen parent (implicitly live in a minor) and report any child slot that
/// points at a sweep-eligible (young/malloc) object which is UNMARKED — i.e.
/// about to be freed while its parent survives. This is the direct signature
/// of a dropped remembered-set edge. Logs a per-(parent,child)-type histogram
/// plus the first edge; diagnostic only.
pub(super) fn verify_minor_unmarked_young_children_report(phase: &str) {
    let mut missing = 0usize;
    let mut checked_parents = 0usize;
    let mut checked_edges = 0usize;
    let mut first: Option<(usize, usize, usize, u8, u8)> = None;
    let mut hist: std::collections::HashMap<(u8, u8), usize> = std::collections::HashMap::new();
    let mut visit_parent = |header: *mut GcHeader| unsafe {
        if header.is_null() || (*header).gc_flags & GC_FLAG_FORWARDED != 0 {
            return;
        }
        let user = (header as *mut u8).add(GC_HEADER_SIZE) as usize;
        if !matches!(
            crate::arena::classify_heap_generation(user),
            crate::arena::HeapGeneration::Old
        ) {
            return;
        }
        checked_parents += 1;
        visit_gc_rewrite_slots::<true>(header, |slot| {
            if crate::weakref::is_weak_target_trace_slot(header, slot.slot)
                && !super::ephemeron::value_slot_is_enabled(header, slot.slot, true)
            {
                return;
            }
            slot.record_layout_read();
            let child_addr = decode_heap_addr(slot.read());
            if child_addr == 0 || !crate::gc::barrier::remembered_child_needs_tracking(child_addr) {
                return;
            }
            checked_edges += 1;
            let ch = (child_addr as *const u8).sub(GC_HEADER_SIZE) as *const GcHeader;
            if (*ch).gc_flags & (GC_FLAG_MARKED | GC_FLAG_PINNED) == 0 {
                missing += 1;
                *hist
                    .entry(((*header).obj_type, (*ch).obj_type))
                    .or_insert(0) += 1;
                if first.is_none() {
                    first = Some((
                        header as usize,
                        slot.slot as usize,
                        child_addr,
                        (*header).obj_type,
                        (*ch).obj_type,
                    ));
                }
            }
        });
    };
    crate::arena::old_arena_walk_objects(|hp| {
        visit_parent(hp as *mut GcHeader);
    });
    let tn = |t: u8| gc_type_info(t).map_or("?", |i| i.name);
    if let Some((p, s, c, pt, ct)) = first {
        let mut hist_str = String::new();
        for ((pt, ct), n) in &hist {
            hist_str.push_str(&format!(" {}({})->{}({})={}", tn(*pt), pt, tn(*ct), ct, n));
        }
        eprintln!(
            "[gc-mark-verify:{}] SWEEP-LIVE-CHILD edges={} parents={} young_edges={} | first parent=0x{:x} ptype={}({}) slot=0x{:x} child=0x{:x} ctype={}({}) | hist:{}",
            phase, missing, checked_parents, checked_edges, p, tn(pt), pt, s, c, tn(ct), ct, hist_str,
        );
    } else {
        eprintln!(
            "[gc-mark-verify:{}] OK parents={} young_edges={}",
            phase, checked_parents, checked_edges,
        );
    }
}

pub(super) unsafe fn verify_heap_object_fields(
    header: *mut GcHeader,
    verifier: EvacuationVerifier<'_>,
    surface: &'static str,
) -> usize {
    let flags = (*header).gc_flags;
    if flags & GC_FLAG_FORWARDED != 0 {
        return 0;
    }
    let verifier = verifier.with_parent(header);
    let mut slots = 0usize;
    visit_gc_rewrite_slot_descriptors::<true>(header, |descriptor| unsafe {
        slots = slots.saturating_add(match descriptor {
            GcMutableSlotDescriptor::Slot(_) => 1,
            GcMutableSlotDescriptor::Range { range, .. } => range.slot_count(),
            GcMutableSlotDescriptor::PointerFreeRange(_) => 0,
        });
        descriptor.visit_slots(&mut |slot| {
            slot.record_layout_read();
            verify_mutable_slot(slot, verifier, surface);
        });
    });
    slots
}

pub(super) fn verify_mutable_root_slots(verifier: EvacuationVerifier<'_>) {
    visit_mutable_root_slots(|slot| unsafe {
        let bits = slot.read();
        if bits == 0 {
            return;
        }
        if let Some(new_bits) = slot
            .pointer_word(bits)
            .and_then(|_| verifier.stale_value(bits))
        {
            let surface = match slot.kind {
                MutableRootSlotKind::ShadowStack => "shadow stack roots",
                MutableRootSlotKind::NativeStack => "native stack-map roots",
                MutableRootSlotKind::GlobalRoot => "global roots",
            };
            panic_stale_forwarded_reference(verifier, surface, slot.ptr as usize, bits, new_bits);
        }
    });
}

pub(super) fn verify_mutable_registered_roots(verifier: EvacuationVerifier<'_>) {
    let scanners: Vec<MutableRootScannerEntry> = MUTABLE_ROOT_SCANNERS.with(|s| s.borrow().clone());
    for entry in scanners {
        let mut visitor = RuntimeRootVisitor::for_verify(verifier, entry.name);
        (entry.scanner)(&mut visitor);
    }
    let mut visitor = RuntimeRootVisitor::for_verify(verifier, "ffi mutable root scanner");
    visit_ffi_mutable_registered_roots(&mut visitor);
}

pub(super) fn verify_copy_only_scanner_bits(
    bits: u64,
    verifier: EvacuationVerifier<'_>,
    surface: &'static str,
) {
    if let Some(new_bits) = verifier.stale_nanboxed_value(bits) {
        panic_stale_forwarded_reference(verifier, surface, 0, bits, new_bits);
    }
}

pub(super) extern "C" fn perry_ffi_verify_root(value: f64, ctx: *mut c_void) {
    if ctx.is_null() {
        return;
    }
    let verifier = unsafe { *(ctx as *const EvacuationVerifier<'_>) };
    verify_copy_only_scanner_bits(value.to_bits(), verifier, "ffi copy-only root scanner");
}

pub(super) fn verify_copy_only_registered_roots(verifier: EvacuationVerifier<'_>) {
    let scanners: Vec<fn(&mut dyn FnMut(f64))> = ROOT_SCANNERS.with(|s| s.borrow().clone());
    for scanner in scanners {
        scanner(&mut |value: f64| {
            verify_copy_only_scanner_bits(value.to_bits(), verifier, "copy-only root scanner");
        });
    }

    let ffi_scanners: Vec<PerryFfiRootScanner> = FFI_ROOT_SCANNERS.with(|s| s.borrow().clone());
    let mut ctx = verifier;
    let ctx = &mut ctx as *mut EvacuationVerifier<'_> as *mut c_void;
    for scanner in ffi_scanners {
        scanner(perry_ffi_verify_root, ctx);
    }
}

pub(super) fn verify_remembered_dirty_ranges(verifier: EvacuationVerifier<'_>) {
    let snapshot = remembered_dirty_snapshot::<true>();
    let mut stats = RememberedSetTraceStats::default();
    let mut seen_headers = crate::fast_hash::new_ptr_hash_set();
    let mut verify_header = |header: *mut GcHeader| unsafe {
        if !seen_headers.insert(header as usize) {
            return;
        }
        let parent_verifier = verifier.with_parent(header);
        let mut verify_dirty_slot = |slot: GcMutableSlot, _stats: &mut RememberedSetTraceStats| {
            verify_mutable_slot(slot, parent_verifier, "remembered dirty ranges");
        };
        scan_dirty_header_once::<true>(
            header,
            &snapshot.dirty_pages,
            verifier.valid_ptrs,
            &mut stats,
            &mut verify_dirty_slot,
        );
    };
    if !snapshot.dirty_old_pages.is_empty() {
        crate::arena::old_arena_walk_objects_on_pages(&snapshot.dirty_old_pages, |header| {
            verify_header(header as *mut GcHeader);
        });
    }
    for &(_, header) in &snapshot.external_dirty_entries {
        verify_header(header as *mut GcHeader);
    }

    for header_addr in snapshot.fallback_headers {
        let user_ptr = header_addr + GC_HEADER_SIZE;
        if !verifier.valid_ptrs.contains(&user_ptr) {
            continue;
        }
        unsafe {
            verify_heap_object_fields(
                header_addr as *mut GcHeader,
                verifier,
                "remembered fallback headers",
            );
        }
    }
}

pub(super) fn verify_heap_objects(verifier: EvacuationVerifier<'_>) -> EvacuationVerifyStats {
    let mut stats = EvacuationVerifyStats::default();
    let mut verify_one = |header: *mut GcHeader| unsafe {
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
        stats.parents = stats.parents.saturating_add(1);
        stats.slots =
            stats
                .slots
                .saturating_add(verify_heap_object_fields(header, verifier, "heap fields"));
    };
    crate::arena::arena_walk_objects(|hp| verify_one(hp as *mut GcHeader));
    for header in malloc_headers_for_verification() {
        verify_one(header);
    }
    // WRT: every realm image cell, remembered or not, is live and must name
    // no forwarded object.
    super::image_cells::for_each_image_cell(|header| verify_one(header));
    stats
}

pub(super) fn verify_evacuated_no_stale_forwarded_refs(
    verifier: EvacuationVerifier<'_>,
) -> EvacuationVerifyStats {
    verify_mutable_root_slots(verifier);
    verify_mutable_registered_roots(verifier);
    verify_copy_only_registered_roots(verifier);
    verify_remembered_dirty_ranges(verifier);
    verify_heap_objects(verifier)
}
