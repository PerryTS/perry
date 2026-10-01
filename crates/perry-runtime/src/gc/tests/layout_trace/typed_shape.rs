use super::*;

#[test]
fn test_typed_shape_descriptor_preserves_pointer_slots_after_non_pointer_overwrite() {
    clear_marks();
    clear_mark_seeds();

    let obj = crate::object::js_object_alloc(0, 2);
    // Charter step 5: the collector traces by the shape. Slot 0 is an `F64`
    // lane, slot 1 an `Any` lane that may hold a pointer.
    crate::object::js_object_set_field(obj, 0, crate::value::JSValue::number(0.5));
    unsafe { restamp_with_rep(obj, f64_lanes([0])) };
    assert_eq!(test_heap_child_slot_count(obj as *mut u8), 1);

    // A number stored into the `Any` lane leaves the lane (and the shape)
    // as it was: the slot is still traced.
    crate::object::js_object_set_field(obj, 1, crate::value::JSValue::number(7.0));
    assert_eq!(test_heap_child_slot_count(obj as *mut u8), 1);

    clear_marks();
    clear_mark_seeds();
}

/// #10362: the relocation contract is asserted in test and debug builds, so a
/// future move path that allocates a destination without copying `_reserved`
/// fails here instead of silently losing a layout state, an `ALL_POINTERS` bit
/// or an element-shape proof at the first collection.
#[test]
#[should_panic(expected = "the caller must copy `_reserved`")]
fn test_layout_transfer_requires_the_relocation_header_copy() {
    let src = crate::array::js_array_alloc_pointer_elements(2);
    let dst = crate::array::js_array_alloc(2);
    unsafe {
        layout_transfer(src as *mut u8, dst as *mut u8);
    }
}

#[test]
fn test_all_pointer_layout_transfers_on_array_move() {
    clear_marks();
    clear_mark_seeds();

    let src = crate::array::js_array_alloc_pointer_elements(2);
    let dst = crate::array::js_array_alloc(2);
    unsafe {
        // `GC_LAYOUT_ALL_POINTERS` rides `_reserved`, so since #10362 the
        // header copy is what carries it and the funnel must leave it alone.
        model_relocation_header_copy(src as usize, dst as usize);
        layout_transfer(src as *mut u8, dst as *mut u8);
    }

    assert_eq!(test_layout_pointer_slot_count(dst as usize, 2), Some(2));

    clear_marks();
    clear_mark_seeds();
}
