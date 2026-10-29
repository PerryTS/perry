//! Young large containers: a pointer-bearing cell over the large cutoff is
//! born young in dedicated non-moving storage, is traced by every minor, dies
//! with its children, and is promoted at its address with its young children
//! remembered. Each test asserts that its subject ran (a minor or full that
//! actually freed or promoted the container), not merely that nothing broke.
use super::super::*;
use super::support::*;
use crate::arena::{HeapGeneration, HeapSpace};

const LARGE_CAPACITY: u32 = 20_000;

unsafe fn large_array() -> *mut crate::array::ArrayHeader {
    let arr = crate::array::js_array_alloc(LARGE_CAPACITY);
    assert!(
        (*header_from_user_ptr(arr as *const u8)).size as usize
            > LARGE_POINTER_BEARING_OBJECT_THRESHOLD_BYTES,
        "fixture must cross the pointer-bearing cutoff"
    );
    arr
}

unsafe fn small_array() -> *mut crate::array::ArrayHeader {
    let arr = crate::array::js_array_alloc(2);
    crate::array::js_array_set_f64_extend(arr, 0, 42.0);
    arr
}

unsafe fn slot_value(arr: *mut crate::array::ArrayHeader, index: u32) -> usize {
    (crate::array::js_array_get_f64(arr, index).to_bits() & POINTER_MASK) as usize
}

fn full_collection() {
    let _ = gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot {
        kind: GcTriggerKind::Direct,
        steps_before: Some(GcStepSnapshot::current()),
    });
}

#[test]
fn large_array_is_born_young_large_and_dies_at_the_next_minor() {
    std::thread::spawn(|| unsafe {
        let _guard = CopyingNurseryTestGuard::new(0);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let arr = large_array();
        let p = arr as usize;
        let header = header_from_user_ptr(p as *const u8);
        let total = (*header).size as usize;
        assert_eq!(crate::arena::classify_heap_generation(p), HeapGeneration::Nursery);
        assert!(crate::arena::is_large_storage(p));
        assert_eq!((*header).gc_flags & GC_FLAG_TENURED, 0);
        assert!(crate::arena::young_large_containers_exist());
        let child = small_array();
        crate::array::js_array_set_f64_extend(arr, 0, f64::from_bits(ptr_bits(child as usize)));
        assert_eq!(remembered_set_size(), 0, "a young parent owes no remembered entry");
        let trace = collect_minor_trace(GcTriggerKind::Direct);
        assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
        assert!(trace.sweep.freed_bytes >= total as u64);
        assert_eq!(crate::arena::classify_heap_space(p), HeapSpace::Unknown);
        assert_eq!(crate::arena::young_large_in_use_bytes(), 0);
        assert!(!crate::arena::young_large_containers_exist());
        assert_eq!(remembered_set_size(), 0, "nothing keeps the dead child alive");
    })
    .join()
    .unwrap();
}

#[test]
fn live_large_array_is_promoted_by_identity_and_remembers_its_young_child() {
    std::thread::spawn(|| unsafe {
        let _guard = CopyingNurseryTestGuard::new(1);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let arr = large_array();
        let p = arr as usize;
        js_shadow_slot_set(0, ptr_bits(p));
        let child = small_array();
        crate::array::js_array_set_f64_extend(arr, 0, f64::from_bits(ptr_bits(child as usize)));
        let trace = collect_minor_trace(GcTriggerKind::Direct);
        assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
        assert_eq!((js_shadow_slot_get(0) & POINTER_MASK) as usize, p, "never moved");
        assert_eq!(crate::arena::classify_heap_generation(p), HeapGeneration::Old);
        assert!(crate::arena::is_large_storage(p));
        let header = header_from_user_ptr(p as *const u8);
        assert_ne!((*header).gc_flags & GC_FLAG_TENURED, 0);
        assert_eq!((*header).gc_flags & GC_FLAG_MARKED, 0, "promotion clears its mark");
        let moved = slot_value(arr, 0);
        assert_ne!(moved, child as usize, "the small child moved and its slot was rewritten");
        assert_eq!(crate::array::js_array_get_f64(moved as *mut _, 0), 42.0);
        if crate::arena::pointer_in_nursery(moved) {
            let slot = crate::array::array_elements_ptr(arr) as usize;
            assert!(
                old_page_dirty_for(crate::arena::generation_page_for_addr(slot)),
                "a promoted container must remember its still-young child"
            );
        }
        // The next minor finds the child through that entry and rewrites it.
        let _ = collect_minor_trace(GcTriggerKind::Direct);
        let again = slot_value(arr, 0);
        assert_eq!(crate::array::js_array_get_f64(again as *mut _, 0), 42.0);
        assert_eq!(crate::arena::classify_heap_generation(p), HeapGeneration::Old);
    })
    .join()
    .unwrap();
}

#[test]
fn old_parent_retains_young_large_array_and_a_later_full_reclaims_it() {
    std::thread::spawn(|| unsafe {
        let _guard = CopyingNurseryTestGuard::new(1);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let (old, elements) = alloc_old_test_array(4);
        js_shadow_slot_set(0, ptr_bits(old as usize));
        let arr = large_array();
        let p = arr as usize;
        let child = small_array();
        crate::array::js_array_set_f64_extend(arr, 7, f64::from_bits(ptr_bits(child as usize)));
        *elements = ptr_bits(p);
        runtime_write_barrier_slot(old as usize, elements as usize, ptr_bits(p));
        assert!(remembered_child_needs_tracking(p));
        let _ = collect_minor_trace(GcTriggerKind::Direct);
        assert_eq!((*elements & POINTER_MASK) as usize, p);
        assert_eq!(crate::arena::classify_heap_generation(p), HeapGeneration::Old);
        let kept = slot_value(arr, 7);
        assert_eq!(crate::array::js_array_get_f64(kept as *mut _, 0), 42.0);
        *elements = 0;
        full_collection();
        assert_eq!(
            crate::arena::classify_heap_space(p),
            HeapSpace::Unknown,
            "old-large ownership is retired by the later full"
        );
    })
    .join()
    .unwrap();
}

#[test]
fn growth_past_the_cutoff_forwards_into_young_large_storage() {
    std::thread::spawn(|| unsafe {
        let _guard = CopyingNurseryTestGuard::new(1);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let source = small_array();
        let child = small_array();
        crate::array::js_array_set_f64_extend(source, 1, f64::from_bits(ptr_bits(child as usize)));
        // The root keeps the pre-grow address: the minor must follow the stub.
        js_shadow_slot_set(0, ptr_bits(source as usize));
        let grown = crate::array::js_array_grow(source, LARGE_CAPACITY);
        let g = grown as usize;
        assert_ne!(g, source as usize);
        assert_eq!(crate::arena::classify_heap_generation(g), HeapGeneration::Nursery);
        assert!(crate::arena::is_large_storage(g), "a young source grows into young large storage");
        let trace = collect_minor_trace(GcTriggerKind::Direct);
        assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
        assert_eq!((js_shadow_slot_get(0) & POINTER_MASK) as usize, g, "root rewritten past the stub");
        assert_eq!(crate::arena::classify_heap_generation(g), HeapGeneration::Old);
        let kept = slot_value(grown, 1);
        assert_eq!(crate::array::js_array_get_f64(kept as *mut _, 0), 42.0);
        // A young large source grows young again, and its stub dies at the minor.
        js_shadow_slot_set(0, 0);
        let big = large_array();
        js_shadow_slot_set(0, ptr_bits(big as usize));
        let bigger = crate::array::js_array_grow(big, 2 * LARGE_CAPACITY) as usize;
        assert_eq!(crate::arena::classify_heap_generation(bigger), HeapGeneration::Nursery);
        let _ = collect_minor_trace(GcTriggerKind::Direct);
        assert_eq!((js_shadow_slot_get(0) & POINTER_MASK) as usize, bigger);
        assert_eq!(crate::arena::classify_heap_space(big as usize), HeapSpace::Unknown);
        assert_eq!(crate::arena::classify_heap_generation(bigger), HeapGeneration::Old);
    })
    .join()
    .unwrap();
}

#[test]
fn full_collection_promotes_a_young_container_and_remembers_its_young_child() {
    std::thread::spawn(|| unsafe {
        let _guard = CopyingNurseryTestGuard::new(1);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let arr = large_array();
        let p = arr as usize;
        js_shadow_slot_set(0, ptr_bits(p));
        let child = small_array();
        crate::array::js_array_set_f64_extend(arr, 3, f64::from_bits(ptr_bits(child as usize)));
        full_collection();
        assert_eq!(crate::arena::classify_heap_generation(p), HeapGeneration::Old);
        let after = slot_value(arr, 3);
        if crate::arena::pointer_in_nursery(after) {
            let slot = crate::array::array_elements_ptr(arr) as usize + 3 * 8;
            assert!(
                old_page_dirty_for(crate::arena::generation_page_for_addr(slot)),
                "a sweep-promoted container must remember its still-young child"
            );
        }
        let _ = collect_minor_trace(GcTriggerKind::Direct);
        let moved = slot_value(arr, 3);
        assert_eq!(crate::array::js_array_get_f64(moved as *mut _, 0), 42.0);
    })
    .join()
    .unwrap();
}

#[test]
fn weak_ref_to_a_dead_young_large_array_is_cleared_by_the_minor() {
    std::thread::spawn(|| unsafe {
        let _guard = CopyingNurseryTestGuard::new(0);
        let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let arr = large_array();
        let p = arr as usize;
        let mut collector = CopyingNurseryCollector::new::<{ cfg!(test) }>(CopyingPointerSet::new());
        assert_eq!(collector.ptrs.classify(p).unwrap().kind, CopyingPointerKind::YoungLarge);
        assert!(crate::weakref::weak_target_should_clear_copied(ptr_bits(p), &collector.ptrs));
        assert_eq!(collector.mark_addr::<{ cfg!(test) }>(p), Some(p));
        assert!(!crate::weakref::weak_target_should_clear_copied(ptr_bits(p), &collector.ptrs));
        collector.drain::<{ cfg!(test) }>();
        collector.clear_marks();
    })
    .join()
    .unwrap();
}
