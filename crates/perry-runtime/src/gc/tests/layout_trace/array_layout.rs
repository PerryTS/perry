use super::*;

#[test]
fn test_layout_mask_overflow_fields_and_array_grow_transfer() {
    clear_marks();
    clear_mark_seeds();
    // #6812 spill: overflow writes now allocate GC memory (meta record +
    // spill buffer), so an automatic minor GC mid-build could move `obj`
    // out from under this test's raw pointers. The test asserts layout and
    // tracing, not move-resilience — pin the heap while building.
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    let child = crate::string::js_string_from_bytes(b"overflow-child".as_ptr(), 14) as *mut u8;
    let child_header = unsafe { header_from_user_ptr(child) };
    let obj = crate::object::js_object_alloc(0, 0);
    for i in 0..9 {
        let name = format!("k{i}");
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let value = if i == 8 {
            f64::from_bits(STRING_TAG | (child as u64 & POINTER_MASK))
        } else {
            i as f64
        };
        crate::object::js_object_set_field_by_name(obj, key, value);
    }

    // #6812 spill: the k8 pointer lives in the object-owned spill buffer
    // (owner inline slots hold only k0..k3 numerics), so the pointer-slot
    // count moves from the owner's mask to the buffer's. Legacy mode keeps
    // the original owner-mask expectation.
    if crate::object::test_object_spill_enabled() {
        let spill = crate::object::test_spill_buffer_addr(obj as usize);
        assert_ne!(spill, 0, "overflow write must have created a spill buffer");
        assert_eq!(test_layout_pointer_slot_count(spill, 9), Some(1));
    } else {
        assert_eq!(test_layout_pointer_slot_count(obj as usize, 9), Some(1));
    }
    let valid_ptrs = build_valid_pointer_set();
    let mut worklist = Vec::new();
    unsafe {
        trace_object(obj as *mut u8, &valid_ptrs, &mut worklist);
        // #6812 spill: the overflow value is no longer owner-adjacent — the
        // chain is obj → meta record → spill buffer → child, so drain the
        // worklist exactly like production marking does instead of relying
        // on a single hop.
        while let Some(queued) = worklist.pop() {
            let user = (queued as *mut u8).add(crate::gc::GC_HEADER_SIZE);
            trace_object(user, &valid_ptrs, &mut worklist);
        }
    }
    unsafe {
        assert_ne!((*child_header).gc_flags & GC_FLAG_MARKED, 0);
    }

    let arr = crate::array::js_array_alloc_with_length(1);
    crate::array::js_array_set_f64(
        arr,
        0,
        f64::from_bits(STRING_TAG | (child as u64 & POINTER_MASK)),
    );
    let grown = crate::array::js_array_grow(arr, 128);
    assert_eq!(test_layout_pointer_slot_count(grown as usize, 1), Some(1));

    let moved = crate::array::js_array_alloc_with_length(1);
    unsafe {
        layout_transfer(grown as *mut u8, moved as *mut u8);
    }
    assert_eq!(test_layout_pointer_slot_count(moved as usize, 1), Some(1));

    clear_marks();
    clear_mark_seeds();
}

#[test]
fn test_trace_array_uses_pointer_layout_mask() {
    clear_marks();
    clear_mark_seeds();

    let numeric = crate::array::js_array_alloc_with_length(3);
    crate::array::js_array_set_f64(numeric, 0, 1.0);
    crate::array::js_array_set_f64(numeric, 1, 2.0);
    crate::array::js_array_set_f64(numeric, 2, 3.0);
    assert_eq!(test_layout_pointer_slot_count(numeric as usize, 3), Some(0));
    assert_eq!(test_heap_child_slot_count(numeric as *mut u8), 0);

    let valid_ptrs = build_valid_pointer_set();
    assert!(try_mark_value(
        POINTER_TAG | (numeric as u64 & POINTER_MASK),
        &valid_ptrs
    ));
    test_reset_trace_slot_reads();
    trace_marked_objects(&valid_ptrs);
    assert_eq!(test_trace_slot_reads(), 0);
    clear_marks();
    clear_mark_seeds();

    let child = crate::string::js_string_from_bytes(b"array-child".as_ptr(), 11) as *mut u8;
    let child_header = unsafe { header_from_user_ptr(child) };
    let mixed = crate::array::js_array_alloc_with_length(3);
    crate::array::js_array_set_f64(mixed, 0, 1.0);
    crate::array::js_array_set_f64(
        mixed,
        1,
        f64::from_bits(STRING_TAG | (child as u64 & POINTER_MASK)),
    );
    crate::array::js_array_set_f64(mixed, 2, 3.0);
    assert_eq!(test_layout_pointer_slot_count(mixed as usize, 3), Some(1));

    let valid_ptrs = build_valid_pointer_set();
    assert!(try_mark_value(
        POINTER_TAG | (mixed as u64 & POINTER_MASK),
        &valid_ptrs
    ));
    test_reset_trace_slot_reads();
    trace_marked_objects(&valid_ptrs);
    assert_eq!(test_trace_slot_reads(), 1);
    unsafe {
        assert_ne!((*child_header).gc_flags & GC_FLAG_MARKED, 0);
    }

    clear_marks();
    clear_mark_seeds();
}

fn assert_array_root_trace_reads(arr: *mut crate::array::ArrayHeader, expected_reads: usize) {
    clear_marks();
    clear_mark_seeds();

    let valid_ptrs = build_valid_pointer_set();
    assert!(try_mark_value(
        POINTER_TAG | (arr as u64 & POINTER_MASK),
        &valid_ptrs
    ));
    test_reset_trace_slot_reads();
    trace_marked_objects(&valid_ptrs);
    assert_eq!(test_trace_slot_reads(), expected_reads);
}

fn assert_numeric_array_trace_free(arr: *mut crate::array::ArrayHeader, len: usize) {
    assert_eq!(test_layout_pointer_slot_count(arr as usize, len), Some(0));
    assert_eq!(test_heap_child_slot_count(arr as *mut u8), 0);
    assert_array_root_trace_reads(arr, 0);
}

#[test]
fn test_array_numeric_producers_stay_pointer_free() {
    clear_marks();
    clear_mark_seeds();

    let values = [1.0, 2.5, 3.0, 4.25];
    let from_f64 = crate::array::js_array_from_f64(values.as_ptr(), values.len() as u32);
    assert_numeric_array_trace_free(from_f64, values.len());

    let keys_src = crate::array::js_array_alloc_with_length(4);
    for i in 0..4 {
        crate::array::js_array_set_f64(keys_src, i, (i + 10) as f64);
    }
    let keys = crate::array::js_array_keys(keys_src);
    assert_numeric_array_trace_free(keys, 4);

    let filled = crate::array::js_array_alloc_with_length(4);
    crate::array::js_array_fill(filled, 42.0);
    assert_numeric_array_trace_free(filled, 4);

    let cloned = crate::array::js_array_clone(filled);
    assert_numeric_array_trace_free(cloned, 4);

    let concat_dest = crate::array::js_array_alloc(0);
    let concatenated = crate::array::js_array_concat(concat_dest, filled);
    assert_numeric_array_trace_free(concatenated, 4);

    crate::array::js_array_copy_within(concatenated, 1.0, 0.0, 0, 0.0);
    assert_numeric_array_trace_free(concatenated, 4);

    clear_marks();
    clear_mark_seeds();
}

#[test]
fn test_array_mixed_bulk_producers_preserve_pointer_layout() {
    clear_marks();
    clear_mark_seeds();

    let child = crate::string::js_string_from_bytes(b"bulk-child".as_ptr(), 10) as *mut u8;
    let child_header = unsafe { header_from_user_ptr(child) };
    let child_box = f64::from_bits(STRING_TAG | (child as u64 & POINTER_MASK));

    let src = crate::array::js_array_alloc_with_length(2);
    crate::array::js_array_set_f64(src, 0, 1.0);
    crate::array::js_array_set_f64(src, 1, child_box);

    let cloned = crate::array::js_array_clone(src);
    assert_eq!(test_layout_pointer_slot_count(cloned as usize, 2), Some(1));
    assert_array_root_trace_reads(cloned, 1);
    unsafe {
        assert_ne!((*child_header).gc_flags & GC_FLAG_MARKED, 0);
    }
    clear_marks();
    clear_mark_seeds();

    let concatenated = crate::array::js_array_concat(crate::array::js_array_alloc(0), src);
    assert_eq!(
        test_layout_pointer_slot_count(concatenated as usize, 2),
        Some(1)
    );
    assert_array_root_trace_reads(concatenated, 1);
    unsafe {
        assert_ne!((*child_header).gc_flags & GC_FLAG_MARKED, 0);
    }
    clear_marks();
    clear_mark_seeds();

    let set = crate::set::js_set_alloc(4);
    let set = crate::set::js_set_add(set, child_box);
    let set_arr = crate::set::js_set_to_array(set);
    assert_eq!(test_layout_pointer_slot_count(set_arr as usize, 1), Some(1));
    assert_array_root_trace_reads(set_arr, 1);
    unsafe {
        assert_ne!((*child_header).gc_flags & GC_FLAG_MARKED, 0);
    }
    clear_marks();
    clear_mark_seeds();

    let map = crate::map::js_map_alloc(4);
    let map = crate::map::js_map_set(map, 7.0, child_box);
    let entries = crate::map::js_map_entries(map);
    assert_eq!(test_layout_pointer_slot_count(entries as usize, 1), Some(1));
    let pair_box = crate::array::js_array_get_f64(entries, 0);
    let pair = (pair_box.to_bits() & POINTER_MASK) as *mut crate::array::ArrayHeader;
    assert_eq!(test_layout_pointer_slot_count(pair as usize, 2), Some(1));
    assert_array_root_trace_reads(entries, 2);
    unsafe {
        assert_ne!((*child_header).gc_flags & GC_FLAG_MARKED, 0);
    }
    clear_marks();
    clear_mark_seeds();

    let overwritten = crate::array::js_array_alloc_with_length(1);
    crate::array::js_array_set_f64(overwritten, 0, child_box);
    assert_eq!(
        test_layout_pointer_slot_count(overwritten as usize, 1),
        Some(1)
    );
    crate::array::js_array_set_f64(overwritten, 0, 99.0);
    assert_numeric_array_trace_free(overwritten, 1);

    clear_marks();
    clear_mark_seeds();
}

#[test]
fn test_numeric_array_push_heap_value_transitions_and_traces() {
    clear_marks();
    clear_mark_seeds();

    let mut arr = crate::array::js_array_alloc(4);
    arr = crate::array::js_array_push_f64(arr, 1.0);
    arr = crate::array::js_array_push_f64(arr, 2.0);
    assert_eq!(test_layout_pointer_slot_count(arr as usize, 2), Some(0));

    let child = crate::string::js_string_from_bytes(b"pushed-child".as_ptr(), 12) as *mut u8;
    let child_header = unsafe { header_from_user_ptr(child) };
    let child_box = f64::from_bits(STRING_TAG | (child as u64 & POINTER_MASK));
    let pushed = crate::array::js_array_push_f64(arr, child_box);

    assert_eq!(pushed, arr, "fixture should exercise the no-grow push path");
    assert_eq!(
        test_layout_pointer_slot_count(pushed as usize, 3),
        Some(1),
        "heap writes into a numeric array must transition to a pointer-bearing layout"
    );

    let valid_ptrs = build_valid_pointer_set();
    assert!(try_mark_value(
        POINTER_TAG | (pushed as u64 & POINTER_MASK),
        &valid_ptrs
    ));
    test_reset_trace_slot_reads();
    trace_marked_objects(&valid_ptrs);
    assert_eq!(test_trace_slot_reads(), 1);
    unsafe {
        assert_ne!((*child_header).gc_flags & GC_FLAG_MARKED, 0);
    }

    clear_marks();
    clear_mark_seeds();
}

#[test]
fn test_numeric_array_layout_metadata_matches_gc_scan_state() {
    clear_marks();
    clear_mark_seeds();

    let mut arr = crate::array::js_array_alloc(4);
    arr = crate::array::js_array_push_f64(arr, 1.0);
    arr = crate::array::js_array_push_f64(arr, 2.0);

    assert_eq!(crate::array::js_array_is_numeric_f64_layout(arr), 1);
    assert_numeric_array_trace_free(arr, 2);

    let child = crate::string::js_string_from_bytes(b"layout-child".as_ptr(), 12) as *mut u8;
    let child_header = unsafe { header_from_user_ptr(child) };
    let child_box = f64::from_bits(STRING_TAG | (child as u64 & POINTER_MASK));
    arr = crate::array::js_array_push_f64(arr, child_box);

    assert_eq!(crate::array::js_array_is_numeric_f64_layout(arr), 0);
    assert_eq!(test_layout_pointer_slot_count(arr as usize, 3), Some(1));

    clear_marks();
    clear_mark_seeds();
    let valid_ptrs = build_valid_pointer_set();
    assert!(try_mark_value(
        POINTER_TAG | (arr as u64 & POINTER_MASK),
        &valid_ptrs
    ));
    test_reset_trace_slot_reads();
    trace_marked_objects(&valid_ptrs);
    assert_eq!(test_trace_slot_reads(), 1);
    unsafe {
        assert_ne!((*child_header).gc_flags & GC_FLAG_MARKED, 0);
    }

    clear_marks();
    clear_mark_seeds();
}
