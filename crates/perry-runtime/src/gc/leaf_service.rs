//! Selective nonmoving reachability for dedicated young leaf owners.
//! Scratch belongs to this traversal; ownership is never a root.
use super::*;

struct LeafTrace {
    ptrs: CopyingPointerSet,
    marked: Vec<*mut GcHeader>,
    work: Vec<*mut GcHeader>,
}

impl LeafTrace {
    fn new() -> Self {
        // Reuse the established malloc classifier for bridge owners even when
        // moving polls are OFF. This is the existing canonical membership set.
        MALLOC_STATE.with(|s| super::malloc::ensure_set_built(&mut s.borrow_mut()));
        Self { ptrs: CopyingPointerSet::new(), marked: Vec::new(), work: Vec::new() }
    }
    fn bits(&mut self, bits: u64) {
        if let Some(word) = decode_root_word(bits) { self.addr(word.addr()); }
    }
    fn addr(&mut self, addr: usize) {
        let Some(ptr) = self.ptrs.classify(addr) else { return; };
        if matches!(ptr.kind, CopyingPointerKind::Old) { return; }
        unsafe {
            let h = ptr.header;
            if (*h).gc_flags & GC_FLAG_FORWARDED != 0 {
                self.addr(forwarding_address(h) as usize);
                return;
            }
            if (*h).gc_flags & GC_FLAG_MARKED != 0 { return; }
            (*h).gc_flags |= GC_FLAG_MARKED;
            self.marked.push(h);
            // Leaf bytes are never interpreted as references.
            if !gc_type_info((*h).obj_type).is_some_and(|i|
                i.rewrite_descriptor_kind == GcRewriteDescriptorKind::Leaf)
            { self.work.push(h); }
        }
    }
    unsafe fn drain(&mut self) {
        while let Some(h) = self.work.pop() {
            // Early cost prototype conservatively retains weak edges. Exact
            // selected-target clearing and ephemeron cleanup are pending the
            // mandatory string cost gate; this image is not landable.
            visit_gc_rewrite_slots_inline(h, |slot| self.bits(slot.read()));
        }
    }
}

struct InAlloc(u8);
impl Drop for InAlloc {
    fn drop(&mut self) {
        GC_FLAGS.with(|flags| flags.set((flags.get() & !GC_FLAG_IN_ALLOC) | self.0));
    }
}

pub(super) fn collect() {
    let _in_alloc = InAlloc(GC_FLAGS.with(|flags| {
        let previous = flags.get();
        flags.set(previous | GC_FLAG_IN_ALLOC);
        previous & GC_FLAG_IN_ALLOC
    }));
    let _step = super::allocation_pacing::CollectorStepGuard::enter();
    let _change = super::heap_generation::HeapChange::begin(
        super::heap_generation::HeapChangeKind::Sweep);
    let pre_live = crate::arena::arena_live_allocated_bytes();
    let pre_from = crate::arena::arena_live_from_space_bytes();
    let pre_large = crate::arena::young_large_in_use_bytes();
    let mut trace = LeafTrace::new();
    visit_mutable_root_slots(|slot| unsafe {
        if let Some(word) = slot.pointer_word(slot.read()) { trace.addr(word.addr()); }
    });
    let scanners = MUTABLE_ROOT_SCANNERS.with(|s| s.borrow().clone());
    {
        let mut mark = |v: f64| {
            if let Some(word) = decode_nanboxed_root_word(v.to_bits()) { trace.addr(word.addr()); }
        };
        let mut visitor = RuntimeRootVisitor::for_copy(&mut mark);
        visitor.young_scope = true;
        for entry in scanners { (entry.scanner)(&mut visitor); }
        visit_ffi_mutable_registered_roots_with_sources(&mut visitor, None);
    }
    let snapshot = remembered_dirty_snapshot();
    scan_remembered_dirty_slots_copying(&snapshot, None, |slot, _, _, _| unsafe {
        trace.bits(slot.read());
    });
    unsafe {
        super::image_cells::visit_leaf_service_roots(|bits| trace.bits(bits));
        trace.drain();
        let stats = crate::arena::finish_copied_young_large();
        super::policy::credit_promoted_bytes_to_old_baseline(pre_large.saturating_sub(stats.reusable_bytes));
        crate::arena::record_arena_live_census(pre_live.saturating_sub(stats.reusable_bytes), Some(pre_from));
        // Retirement unmaps or pools dead headers before this clear; only
        // marked survivors are present here, so every address remains valid.
        for &h in &trace.marked { (*h).gc_flags &= !GC_FLAG_MARKED; }
        if gc_trace_enabled() {
            eprintln!("[gc-leaf] marked={} retired_bytes={}", trace.marked.len(), stats.reusable_bytes);
        }
    }
    // No small reset/flip/age/adaptation, no remembered-set clear/rebuild.
}

/// New old parents must retain coverage for still-young leaves. The caller
/// already parses every promoted header. Whole-cell leaves/free cells incur
/// no outgoing scan; forwarded cells supply only their forwarding edge.
pub(crate) unsafe fn repair_promoted_leaf_edges(h: *mut GcHeader) {
    if !gc_type_is_arena_walkable((*h).obj_type) { return; }
    if gc_type_info((*h).obj_type).is_some_and(|i|
        i.rewrite_descriptor_kind == GcRewriteDescriptorKind::Leaf) { return; }
    if (*h).gc_flags & GC_FLAG_FORWARDED != 0 {
        let mut sticky = StickyRememberedSet::default();
        remember_evacuated_old_to_young_slot(&mut sticky, h,
            (h as *mut u8).add(GC_HEADER_SIZE).cast());
        sticky.restore();
        return;
    }
    visit_gc_rewrite_slots_inline(h, |slot| {
        let bits = slot.read();
        if let Some(word) = decode_root_word(bits) {
            if barrier::remembered_child_needs_tracking(word.addr()) {
                let mut sticky = StickyRememberedSet::default();
                remember_mutable_old_to_young_slot(&mut sticky, h, slot);
                sticky.restore();
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gc::tests::support::*;
    unsafe fn leaf() -> usize {
        let bytes = LARGE_OBJECT_THRESHOLD_BYTES + 8;
        let p = crate::arena::arena_alloc_gc(bytes, 8, GC_TYPE_STRING);
        p.write_bytes(0, bytes);
        p as usize
    }
    #[test]
    fn selective_trace_keeps_small_objects_and_remembered_edges() {
        std::thread::spawn(|| unsafe {
            let _guard = CopyingNurseryTestGuard::new(1);
            let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
            let parent = crate::array::js_array_alloc(20_000);
            let small = crate::array::js_array_alloc(2);
            let live = leaf();
            let dead = leaf();
            crate::array::js_array_set_f64_extend(small, 0, f64::from_bits(STRING_TAG | live as u64));
            crate::array::js_array_set_f64_extend(parent, 0, f64::from_bits(POINTER_TAG | small as u64));
            js_shadow_slot_set(0, POINTER_TAG | parent as u64);
            let before = crate::arena::copying_from_space_in_use_bytes();
            let small_flags = (*header_from_user_ptr(small.cast())).gc_flags;
            assert!(remembered_set_size() > 0);
            collect();
            assert_eq!(crate::arena::classify_heap_space(dead), crate::arena::HeapSpace::Unknown);
            assert_eq!(crate::arena::classify_heap_generation(live), crate::arena::HeapGeneration::Old);
            assert_eq!(crate::array::js_array_get_f64(small, 0).to_bits() & POINTER_MASK, live as u64);
            assert_eq!(crate::arena::copying_from_space_in_use_bytes(), before);
            assert_eq!((*header_from_user_ptr(small.cast())).gc_flags, small_flags);
            assert!(remembered_set_size() > 0, "old to small coverage must survive");
        }).join().unwrap();
    }
    #[test]
    fn selected_leaf_payload_is_never_a_graph_edge() {
        std::thread::spawn(|| unsafe {
            let _guard = CopyingNurseryTestGuard::new(1);
            let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
            let live = leaf();
            let dead = leaf();
            *(live as *mut u64) = STRING_TAG | dead as u64;
            js_shadow_slot_set(0, STRING_TAG | live as u64);
            collect();
            assert_eq!(crate::arena::classify_heap_space(dead), crate::arena::HeapSpace::Unknown);
            assert_eq!(crate::arena::classify_heap_generation(live), crate::arena::HeapGeneration::Old);
            assert_eq!((*header_from_user_ptr(live as *const u8)).gc_flags & GC_FLAG_MARKED, 0);
        }).join().unwrap();
    }
}
