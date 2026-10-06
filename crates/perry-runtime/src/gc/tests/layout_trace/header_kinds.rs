//! Birth, rebuild and store classify payloads without creating owner records.
use super::*;

#[test]
fn layout_birth_rebuild_and_store_never_mint_masks() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let child = crate::string::js_string_from_bytes(b"child".as_ptr(), 5);
    let arr = crate::array::js_array_alloc_with_length(8);
    crate::array::js_array_set_f64(arr, 1, crate::value::js_nanbox_string(child as i64));
    unsafe {
        let slots = crate::array::array_elements_ptr(arr);
        layout_rebuild_from_slots(arr.cast(), slots, 8);
        assert_eq!(
            (*header_from_user_ptr(arr.cast()))._reserved & GC_LAYOUT_STATE_MASK,
            GC_LAYOUT_UNKNOWN
        );
        let closure = crate::closure::js_closure_alloc(std::ptr::null(), 8);
        let captures = crate::closure::closure_capture_slots_mut(closure);
        *captures.add(1) = string_bits(child as usize);
        assert!(layout_init_from_slots(closure.cast(), captures, 8));
    }
}

#[test]
fn holey_numeric_array_keeps_holes_fact_across_layout_operations() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let arr = crate::array::js_array_alloc_with_length(8);
    crate::array::js_array_set_f64(arr, 2, 42.5);
    unsafe {
        assert!(crate::array::rebuild_array_numeric_raw_f64_allow_holes(arr));
        let header = header_from_user_ptr(arr.cast());
        let slots = crate::array::array_elements_ptr(arr);
        assert_ne!(
            (*header)._reserved & GC_ARRAY_RAW_F64_HOLES,
            0,
            "fixture must be holey numeric"
        );
        layout_init_pointer_free(arr.cast());
        assert_ne!(
            (*header)._reserved & GC_ARRAY_RAW_F64_HOLES,
            0,
            "birth layout must preserve the holes proof"
        );
        layout_rebuild_from_slots(arr.cast(), slots, 8);
        assert_ne!(
            (*header)._reserved & GC_ARRAY_RAW_F64_HOLES,
            0,
            "rebuild must preserve the holes proof"
        );
        assert_eq!(*slots, crate::value::TAG_HOLE);
        assert_eq!(*slots.add(2), 42.5f64.to_bits());
    }
    js_shadow_slot_set(0, ptr_bits(arr as usize));
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert!(trace.copying_nursery.copied_objects > 0);
    let moved = (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::array::ArrayHeader;
    assert_ne!(moved, arr);
    unsafe {
        assert_ne!(
            (*header_from_user_ptr(moved.cast()))._reserved & GC_ARRAY_RAW_F64_HOLES,
            0
        );
        assert_eq!(
            *crate::array::array_elements_ptr(moved),
            crate::value::TAG_HOLE
        );
    }
}
