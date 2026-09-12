//! Moving-collector witnesses for splice/unshift dense slot moves (#10087).
use super::*;
use crate::array::{self, ArrayHeader};

#[test]
fn promoted_array_keeps_new_unshift_and_splice_children_through_minor_and_full_gc() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _tenuring =
        crate::gc::tenuring::set_survivals_for_test(crate::gc::tenuring::GC_TENURING_SURVIVALS_MAX);
    let mut arr = array::js_array_alloc(16);
    for value in [1.0, 2.0, 3.0, 4.0] {
        arr = array::js_array_push_f64(arr, value);
    }
    js_shadow_slot_set(0, ptr_bits(arr as usize));

    for _ in 0..crate::gc::tenuring::GC_TENURING_SURVIVALS_MAX {
        let _ = gc_collect_minor();
        arr = (js_shadow_slot_get(0) & POINTER_MASK) as *mut ArrayHeader;
    }
    assert!(
        crate::arena::pointer_in_old_gen(arr as usize),
        "the fixture array must actually be promoted before its stores"
    );

    let first = young_leaf();
    arr = array::js_array_unshift_f64(arr, f64::from_bits(ptr_bits(first)));
    js_shadow_slot_set(0, ptr_bits(arr as usize));
    let second = young_leaf();
    let items = [f64::from_bits(ptr_bits(second))];
    let mut out = arr;
    array::js_array_splice(arr, 3, 0, items.as_ptr(), 1, &mut out);
    arr = out;
    js_shadow_slot_set(0, ptr_bits(arr as usize));

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    let first_after = (array::js_array_get_f64(arr, 0).to_bits() & POINTER_MASK) as usize;
    let second_after = (array::js_array_get_f64(arr, 3).to_bits() & POINTER_MASK) as usize;
    assert_ne!(first_after, first);
    assert_ne!(second_after, second);
    assert!(crate::arena::pointer_in_nursery(first_after));
    assert!(crate::arena::pointer_in_nursery(second_after));

    gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));
    for child in [
        array::js_array_get_f64(arr, 0).to_bits(),
        array::js_array_get_f64(arr, 3).to_bits(),
    ] {
        let child = (child & POINTER_MASK) as *const u8;
        unsafe {
            assert_ne!((*header_from_user_ptr(child)).size, 0);
        }
    }
    js_shadow_slot_set(0, crate::value::TAG_UNDEFINED);
}

#[test]
fn old_array_unshift_translates_a_young_edge_across_a_page_boundary() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let arr = array::js_array_alloc(OLD_BORN_ELEMENTS);
    assert!(crate::arena::pointer_in_old_gen(arr as usize));
    js_shadow_slot_set(0, ptr_bits(arr as usize));

    let slots = unsafe { array::array_elements_ptr(arr) };
    let source_index = (0..OLD_BORN_ELEMENTS as usize - 1)
        .find(|&index| {
            crate::arena::generation_page_for_addr(unsafe { slots.add(index) } as usize)
                != crate::arena::generation_page_for_addr(unsafe { slots.add(index + 1) } as usize)
        })
        .expect("old-born array must span an element page boundary");
    for index in 0..source_index {
        assert_eq!(array::js_array_push_f64(arr, index as f64), arr);
    }
    let child = young_leaf();
    assert_eq!(
        array::js_array_push_f64(arr, f64::from_bits(ptr_bits(child))),
        arr
    );
    assert_eq!(array::js_array_length(arr) as usize, source_index + 1);

    assert_eq!(array::js_array_unshift_f64(arr, -1.0), arr);
    let destination = unsafe { slots.add(source_index + 1) };
    assert_ne!(
        crate::arena::generation_page_for_addr(unsafe { destination.sub(1) } as usize),
        crate::arena::generation_page_for_addr(destination as usize),
        "the moved child must cross into a different remembered-set page"
    );

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    let child_after =
        (array::js_array_get_f64(arr, (source_index + 1) as u32).to_bits() & POINTER_MASK) as usize;
    assert_ne!(child_after, child);
    assert!(crate::arena::pointer_in_nursery(child_after));
    js_shadow_slot_set(0, crate::value::TAG_UNDEFINED);
}
