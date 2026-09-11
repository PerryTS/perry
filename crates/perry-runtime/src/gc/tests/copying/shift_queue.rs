//! Moving-collector witnesses for the array queue's logical slot range.
use super::*;
use crate::array::{self, ArrayHeader};

#[test]
fn shift_queue_mixed_survivors_move_and_removed_slots_are_not_roots() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let arr = array::js_array_alloc(16);
    let removed = young_leaf();
    array::js_array_push_f64(arr, f64::from_bits(ptr_bits(removed)));
    array::js_array_push_f64(arr, 12.0);
    let child = young_leaf();
    array::js_array_push_f64(arr, f64::from_bits(ptr_bits(child)));
    let original_slots = unsafe { array::array_elements_ptr(arr) };
    assert_eq!(array::js_array_shift_f64(arr).to_bits(), ptr_bits(removed));
    unsafe {
        assert_eq!(*original_slots, crate::value::TAG_HOLE);
        let slots = test_heap_child_slots_for_user(arr.cast());
        assert!(slots
            .iter()
            .any(|s| matches!(s, HeapChildSlot::Child(slot, _) if *slot == original_slots.add(2))));
        assert!(slots
            .iter()
            .all(|s| !matches!(s, HeapChildSlot::Child(slot, _) if *slot == original_slots)));
    }
    js_shadow_slot_set(0, ptr_bits(arr as usize));
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert!(trace.copying_nursery.copied_objects >= 2);
    let moved = (js_shadow_slot_get(0) & POINTER_MASK) as *mut ArrayHeader;
    assert_ne!(moved, arr);
    assert_eq!(array::js_array_get_f64(moved, 0), 12.0);
    let moved_child = (array::js_array_get_f64(moved, 1).to_bits() & POINTER_MASK) as usize;
    assert_ne!(moved_child, child);
    assert!(crate::arena::pointer_in_nursery(moved_child));
    unsafe {
        assert_eq!(*((moved.add(1)) as *const u64), crate::value::TAG_HOLE);
    }
    js_shadow_slot_set(0, crate::value::TAG_UNDEFINED);
}

#[test]
fn shift_queue_old_destination_remembers_young_edges_across_growth_and_refill() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let capacity = OLD_BORN_ELEMENTS;
    let arr = array::js_array_alloc(capacity);
    assert!(crate::arena::pointer_in_old_gen(arr as usize));
    js_shadow_slot_set(0, ptr_bits(arr as usize));
    let mut current = arr;
    for cycle in 0..3 {
        for _ in 0..capacity {
            let child = young_leaf();
            current = array::js_array_push_f64(current, f64::from_bits(ptr_bits(child)));
        }
        for _ in 0..capacity / 2 {
            array::js_array_shift_f64(current);
        }
        // This append crosses the remaining capacity on the first refill and
        // normalizes shifted storage while installing a growth forwarding stub.
        for _ in 0..capacity {
            let child = young_leaf();
            current = array::js_array_push_f64(current, f64::from_bits(ptr_bits(child)));
        }
        js_shadow_slot_set(0, ptr_bits(current as usize));
        let before = array::js_array_get_f64(current, 0).to_bits() & POINTER_MASK;
        let trace = collect_minor_trace(GcTriggerKind::Direct);
        assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
        assert!(
            trace.copying_nursery.copied_objects > 0,
            "cycle {cycle} must copy children"
        );
        let after = array::js_array_get_f64(current, 0).to_bits() & POINTER_MASK;
        assert_ne!(after, before);
        assert_eq!(array::js_array_length(arr), array::js_array_length(current));
        for i in 0..array::js_array_length(current) {
            let bits = array::js_array_get_f64(current, i).to_bits();
            assert!(crate::arena::pointer_in_nursery(
                (bits & POINTER_MASK) as usize
            ));
        }
        while array::js_array_length(current) != 0 {
            array::js_array_shift_f64(current);
        }
    }
    js_shadow_slot_set(0, crate::value::TAG_UNDEFINED);
}
