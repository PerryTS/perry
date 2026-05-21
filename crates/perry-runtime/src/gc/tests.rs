use super::*;

#[test]
fn test_gc_malloc_basic() {
    // Allocate a string-type object
    let ptr = gc_malloc(64, GC_TYPE_STRING);
    assert!(!ptr.is_null());

    // Verify header is set correctly
    unsafe {
        let header = header_from_user_ptr(ptr);
        assert_eq!((*header).obj_type, GC_TYPE_STRING);
        assert_eq!((*header).gc_flags, 0); // not arena, not marked
        assert_eq!((*header).size as usize, GC_HEADER_SIZE + 64);
    }

    // Verify it's tracked in MALLOC_OBJECTS (rebuild lazy set first)
    let tracked = MALLOC_STATE.with(|s| {
        let header = unsafe { header_from_user_ptr(ptr) };
        let mut s = s.borrow_mut();
        ensure_set_built(&mut s);
        s.set.contains(&(header as usize))
    });
    assert!(tracked, "allocated object should be tracked in MALLOC_SET");
}

#[test]
fn test_gc_malloc_different_types() {
    let string_ptr = gc_malloc(32, GC_TYPE_STRING);
    let closure_ptr = gc_malloc(48, GC_TYPE_CLOSURE);
    let bigint_ptr = gc_malloc(16, GC_TYPE_BIGINT);

    unsafe {
        init_test_closure(closure_ptr);
        assert_eq!((*header_from_user_ptr(string_ptr)).obj_type, GC_TYPE_STRING);
        assert_eq!(
            (*header_from_user_ptr(closure_ptr)).obj_type,
            GC_TYPE_CLOSURE
        );
        assert_eq!((*header_from_user_ptr(bigint_ptr)).obj_type, GC_TYPE_BIGINT);
    }
}

#[test]
fn test_sweep_removes_unmarked_malloc_object() {
    let ptr = gc_malloc(64, GC_TYPE_STRING);
    let header = unsafe { header_from_user_ptr(ptr) };
    let header_addr = header as usize;

    let tracked_before = MALLOC_STATE.with(|s| {
        s.borrow()
            .objects
            .iter()
            .any(|&tracked| tracked as usize == header_addr)
    });
    assert!(
        tracked_before,
        "new gc_malloc object should be tracked before sweep"
    );

    // Direct sweep is intentionally rootless for this regression. Keep
    // older test allocations marked so this assertion is about only the
    // object created above.
    MALLOC_STATE.with(|s| {
        for &tracked in s.borrow().objects.iter() {
            if tracked as usize != header_addr {
                unsafe {
                    (*tracked).gc_flags |= GC_FLAG_MARKED;
                }
            }
        }
    });
    crate::arena::arena_walk_objects(|arena_header| unsafe {
        (*(arena_header as *mut GcHeader)).gc_flags |= GC_FLAG_MARKED;
    });

    let freed = sweep();
    assert!(
        freed >= (GC_HEADER_SIZE + 64) as u64,
        "sweep should report at least the target malloc object as freed"
    );

    let tracked_after = MALLOC_STATE.with(|s| {
        s.borrow()
            .objects
            .iter()
            .any(|&tracked| tracked as usize == header_addr)
    });
    assert!(
        !tracked_after,
        "unmarked malloc object should be removed from MALLOC_STATE.objects"
    );

    clear_marks();
    clear_mark_seeds();
}

unsafe fn test_heap_child_slots_for_user(user_ptr: *mut u8) -> Vec<HeapChildSlot> {
    let header = header_from_user_ptr(user_ptr as *const u8);
    gc_child_slots(header).collect()
}

fn test_heap_child_slot_count(user_ptr: *mut u8) -> usize {
    unsafe {
        test_heap_child_slots_for_user(user_ptr)
            .into_iter()
            .filter(|slot| matches!(slot, HeapChildSlot::Child(_, _)))
            .count()
    }
}

#[test]
fn test_trace_array_marks_child() {
    clear_marks();
    clear_mark_seeds();

    let child = crate::string::js_string_from_bytes(b"child".as_ptr(), 5) as *mut u8;
    let child_header = unsafe { header_from_user_ptr(child) };
    unsafe {
        assert_eq!(
            (*child_header).gc_flags & GC_FLAG_MARKED,
            0,
            "child should start unmarked before array tracing"
        );
    }
    let parent = crate::array::js_array_alloc_with_length(1);
    crate::array::js_array_set_f64(
        parent,
        0,
        f64::from_bits(STRING_TAG | (child as u64 & POINTER_MASK)),
    );

    let valid_ptrs = build_valid_pointer_set();
    let parent_bits = POINTER_TAG | (parent as u64 & POINTER_MASK);
    assert!(
        try_mark_value(parent_bits, &valid_ptrs),
        "parent array should be marked as a root"
    );

    trace_marked_objects(&valid_ptrs);

    unsafe {
        assert_ne!(
            (*child_header).gc_flags & GC_FLAG_MARKED,
            0,
            "tracing the marked array should mark its child element"
        );
    }

    clear_marks();
    clear_mark_seeds();
}

#[test]
fn test_layout_mask_pointer_free_array_scans_zero_slots() {
    clear_marks();
    clear_mark_seeds();

    let arr = crate::array::js_array_alloc_with_length(4);
    for i in 0..4 {
        crate::array::js_array_set_f64(arr, i, (i + 1) as f64);
    }

    let valid_ptrs = build_valid_pointer_set();
    let mut worklist = Vec::new();
    test_reset_trace_slot_reads();
    unsafe {
        trace_array(arr as *mut u8, &valid_ptrs, &mut worklist);
    }

    assert_eq!(test_layout_pointer_slot_count(arr as usize, 4), Some(0));
    let slots = unsafe { test_heap_child_slots_for_user(arr as *mut u8) };
    assert_eq!(
        slots
            .iter()
            .filter(|slot| matches!(slot, HeapChildSlot::Child(_, _)))
            .count(),
        0
    );
    assert!(matches!(
        slots.as_slice(),
        [HeapChildSlot::PointerFreeRange(range)] if range.slot_count() == 4
    ));
    assert_eq!(test_trace_slot_reads(), 0);

    clear_marks();
    clear_mark_seeds();
}

#[test]
fn test_layout_scan_trace_json_counts_pointer_free_slots() {
    clear_marks();
    clear_mark_seeds();

    let trace = GcCycleTrace::new(
        GcCollectionKind::Minor,
        GcTriggerSnapshot {
            kind: GcTriggerKind::Direct,
            steps_before: Some(GcStepSnapshot::current()),
        },
    )
    .expect("test requested GC trace capture");
    let arr = crate::array::js_array_alloc_with_length(4);
    for i in 0..4 {
        crate::array::js_array_set_f64(arr, i, (i + 1) as f64);
    }

    let valid_ptrs = build_valid_pointer_set();
    assert!(try_mark_value(
        POINTER_TAG | (arr as u64 & POINTER_MASK),
        &valid_ptrs
    ));
    trace_marked_objects(&valid_ptrs);

    let event = trace.into_json(GcStepSnapshot::current());
    let layout_scans = &event["layout_scans"];
    assert_eq!(layout_scans["pointer_slots_read"].as_u64(), Some(0));
    assert_eq!(
        layout_scans["pointer_free_ranges_skipped"].as_u64(),
        Some(1)
    );
    assert_eq!(layout_scans["pointer_free_slots_skipped"].as_u64(), Some(4));

    clear_marks();
    clear_mark_seeds();
}

#[test]
fn test_pointer_free_target_gate_emits_trace() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    let arr = crate::array::js_array_alloc_with_length(64);
    for i in 0..64 {
        crate::array::js_array_set_f64(arr, i, (i + 1) as f64);
    }
    js_shadow_slot_set(0, ptr_bits(arr as usize));

    let _ = gc_collect_minor();
    let after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;

    assert_ne!(after, arr as usize);
    assert!(crate::arena::pointer_in_nursery(after));
}

#[test]
fn test_layout_mask_small_mixed_array_falls_back_to_full_scan() {
    clear_marks();
    clear_mark_seeds();

    let child = crate::string::js_string_from_bytes(b"array-child".as_ptr(), 11) as *mut u8;
    let child_header = unsafe { header_from_user_ptr(child) };
    let arr = crate::array::js_array_alloc_with_length(3);
    crate::array::js_array_set_f64(arr, 0, 1.0);
    crate::array::js_array_set_f64(
        arr,
        1,
        f64::from_bits(STRING_TAG | (child as u64 & POINTER_MASK)),
    );
    crate::array::js_array_set_f64(arr, 2, 3.0);

    assert_eq!(test_layout_pointer_slot_count(arr as usize, 3), None);

    let valid_ptrs = build_valid_pointer_set();
    let mut worklist = Vec::new();
    test_reset_trace_slot_reads();
    unsafe {
        trace_array(arr as *mut u8, &valid_ptrs, &mut worklist);
        assert_ne!((*child_header).gc_flags & GC_FLAG_MARKED, 0);
    }
    assert_eq!(test_trace_slot_reads(), 3);

    crate::array::js_array_set_f64(arr, 1, 2.0);
    assert_eq!(test_layout_pointer_slot_count(arr as usize, 3), None);

    clear_marks();
    clear_mark_seeds();
}

#[test]
fn test_layout_mask_heap_conversion_keeps_sparse_words_zeroed() {
    clear_marks();
    clear_mark_seeds();

    let first_child = crate::string::js_string_from_bytes(b"first-child".as_ptr(), 11) as *mut u8;
    let later_child = crate::string::js_string_from_bytes(b"later-child".as_ptr(), 11) as *mut u8;
    let first_child_header = unsafe { header_from_user_ptr(first_child) };
    let later_child_header = unsafe { header_from_user_ptr(later_child) };
    let arr = crate::array::js_array_alloc_with_length(66);
    crate::array::js_array_set_f64(
        arr,
        0,
        f64::from_bits(STRING_TAG | (first_child as u64 & POINTER_MASK)),
    );
    crate::array::js_array_set_f64(arr, 64, 64.0);
    crate::array::js_array_set_f64(
        arr,
        65,
        f64::from_bits(STRING_TAG | (later_child as u64 & POINTER_MASK)),
    );

    assert_eq!(test_layout_pointer_slot_count(arr as usize, 66), Some(2));

    let valid_ptrs = build_valid_pointer_set();
    let mut worklist = Vec::new();
    test_reset_trace_slot_reads();
    unsafe {
        trace_array(arr as *mut u8, &valid_ptrs, &mut worklist);
        assert_ne!((*first_child_header).gc_flags & GC_FLAG_MARKED, 0);
        assert_ne!((*later_child_header).gc_flags & GC_FLAG_MARKED, 0);
    }
    assert_eq!(test_trace_slot_reads(), 2);

    clear_marks();
    clear_mark_seeds();
}

#[test]
fn test_layout_mask_object_and_closure_slots() {
    clear_marks();
    clear_mark_seeds();

    let object_child = crate::string::js_string_from_bytes(b"object-child".as_ptr(), 12) as *mut u8;
    let object_child_header = unsafe { header_from_user_ptr(object_child) };
    let obj = crate::object::js_object_alloc(0, 3);
    crate::object::js_object_set_field(obj, 0, crate::value::JSValue::number(1.0));
    crate::object::js_object_set_field(
        obj,
        1,
        crate::value::JSValue::from_bits(STRING_TAG | (object_child as u64 & POINTER_MASK)),
    );
    crate::object::js_object_set_field(obj, 2, crate::value::JSValue::number(3.0));

    assert_eq!(test_layout_pointer_slot_count(obj as usize, 3), None);
    let valid_ptrs = build_valid_pointer_set();
    let mut worklist = Vec::new();
    test_reset_trace_slot_reads();
    unsafe {
        trace_object(obj as *mut u8, &valid_ptrs, &mut worklist);
        assert_ne!((*object_child_header).gc_flags & GC_FLAG_MARKED, 0);
    }
    assert_eq!(test_trace_slot_reads(), 3);

    let closure_child =
        crate::string::js_string_from_bytes(b"closure-child".as_ptr(), 13) as *mut u8;
    let closure_child_header = unsafe { header_from_user_ptr(closure_child) };
    let closure = crate::closure::js_closure_alloc(std::ptr::null(), 3);
    crate::closure::js_closure_set_capture_f64(closure, 0, 10.0);
    crate::closure::js_closure_set_capture_f64(
        closure,
        1,
        f64::from_bits(STRING_TAG | (closure_child as u64 & POINTER_MASK)),
    );
    crate::closure::js_closure_set_capture_f64(closure, 2, 30.0);

    assert_eq!(test_layout_pointer_slot_count(closure as usize, 3), None);
    let valid_ptrs = build_valid_pointer_set();
    let mut worklist = Vec::new();
    test_reset_trace_slot_reads();
    unsafe {
        trace_closure(closure as *mut u8, &valid_ptrs, &mut worklist);
        assert_ne!((*closure_child_header).gc_flags & GC_FLAG_MARKED, 0);
    }
    assert_eq!(test_trace_slot_reads(), 3);

    clear_marks();
    clear_mark_seeds();
}

#[test]
fn test_typed_shape_descriptor_preserves_pointer_slots_after_non_pointer_overwrite() {
    clear_marks();
    clear_mark_seeds();

    let obj = crate::object::js_object_alloc(0, 2);
    let mask = [0b10u64];
    js_gc_init_typed_shape_layout(obj as u64, 2, mask.as_ptr(), mask.len() as u32);

    assert_eq!(test_layout_pointer_slot_count(obj as usize, 2), Some(1));
    assert_eq!(test_heap_child_slot_count(obj as *mut u8), 1);

    crate::object::js_object_set_field(obj, 1, crate::value::JSValue::number(7.0));

    assert_eq!(test_layout_pointer_slot_count(obj as usize, 2), Some(1));
    assert_eq!(test_heap_child_slot_count(obj as *mut u8), 1);

    clear_marks();
    clear_mark_seeds();
}

#[test]
fn test_typed_shape_descriptor_pointer_write_to_non_pointer_slot_falls_back() {
    clear_marks();
    clear_mark_seeds();

    let child = crate::string::js_string_from_bytes(b"typed-child".as_ptr(), 11);
    let child_header = unsafe { header_from_user_ptr(child as *mut u8) };
    let obj = crate::object::js_object_alloc(0, 2);
    let mask = [0b10u64];
    js_gc_init_typed_shape_layout(obj as u64, 2, mask.as_ptr(), mask.len() as u32);

    crate::object::js_object_set_field(obj, 0, crate::value::JSValue::string_ptr(child));

    assert_eq!(test_layout_pointer_slot_count(obj as usize, 2), None);
    assert_eq!(test_heap_child_slot_count(obj as *mut u8), 2);

    let valid_ptrs = build_valid_pointer_set();
    assert!(try_mark_value(
        POINTER_TAG | (obj as u64 & POINTER_MASK),
        &valid_ptrs
    ));
    trace_marked_objects(&valid_ptrs);
    unsafe {
        assert_ne!(
            (*child_header).gc_flags & GC_FLAG_MARKED,
            0,
            "fallback all-field tracing should mark a pointer written to a numeric slot"
        );
    }

    clear_marks();
    clear_mark_seeds();
}

#[test]
fn test_typed_shape_descriptor_growing_new_field_falls_back() {
    clear_marks();
    clear_mark_seeds();

    let packed_keys = b"stable\0";
    let keys = crate::object::js_build_class_keys_array(
        65_001,
        1,
        packed_keys.as_ptr(),
        packed_keys.len() as u32,
    );
    let obj = crate::object::js_object_alloc_class_inline_keys(65_001, 0, 1, keys);
    js_gc_init_typed_shape_layout(obj as u64, 1, std::ptr::null(), 0);

    let extra_key = crate::string::js_string_from_bytes(b"extra".as_ptr(), 5);
    crate::object::js_object_set_field_by_name(obj, extra_key, 42.0);

    unsafe {
        assert_eq!((*obj).field_count, 2);
    }
    assert_eq!(test_layout_pointer_slot_count(obj as usize, 2), None);

    clear_marks();
    clear_mark_seeds();
}

#[test]
fn test_typed_shape_descriptor_transfers_on_object_move() {
    clear_marks();
    clear_mark_seeds();

    let src = crate::object::js_object_alloc(0, 2);
    let dst = crate::object::js_object_alloc(0, 2);
    let mask = [0b10u64];
    js_gc_init_typed_shape_layout(src as u64, 2, mask.as_ptr(), mask.len() as u32);

    unsafe {
        layout_transfer(src as *mut u8, dst as *mut u8);
    }

    assert_eq!(test_layout_pointer_slot_count(dst as usize, 2), Some(1));
    crate::object::js_object_set_field(dst, 1, crate::value::JSValue::number(9.0));
    assert_eq!(test_layout_pointer_slot_count(dst as usize, 2), Some(1));

    let child = crate::string::js_string_from_bytes(b"moved-child".as_ptr(), 11);
    crate::object::js_object_set_field(dst, 0, crate::value::JSValue::string_ptr(child));
    assert_eq!(test_layout_pointer_slot_count(dst as usize, 2), None);

    clear_marks();
    clear_mark_seeds();
}

#[test]
fn test_unboxed_object_layout_scans_zero_raw_numeric_fields() {
    clear_marks();
    clear_mark_seeds();

    let obj = crate::object::js_object_alloc(0, 2);
    crate::object::js_object_set_unboxed_f64_field(obj, 0, 1.25);
    crate::object::js_object_set_unboxed_f64_field(obj, 1, -2.5);
    js_gc_init_unboxed_object_layout(obj as u64, 2, 0b11, 0);

    assert_eq!(test_layout_pointer_slot_count(obj as usize, 2), Some(0));
    assert_eq!(test_heap_child_slot_count(obj as *mut u8), 0);

    let valid_ptrs = build_valid_pointer_set();
    assert!(try_mark_value(
        POINTER_TAG | (obj as u64 & POINTER_MASK),
        &valid_ptrs
    ));
    test_reset_trace_slot_reads();
    trace_marked_objects(&valid_ptrs);
    assert_eq!(test_trace_slot_reads(), 0);

    clear_marks();
    clear_mark_seeds();
}

#[test]
fn test_unboxed_object_pointer_write_to_raw_slot_falls_back_and_traces() {
    clear_marks();
    clear_mark_seeds();

    let obj = crate::object::js_object_alloc(0, 2);
    crate::object::js_object_set_unboxed_f64_field(obj, 0, 1.0);
    crate::object::js_object_set_unboxed_f64_field(obj, 1, 2.0);
    js_gc_init_unboxed_object_layout(obj as u64, 2, 0b11, 0);
    assert_eq!(test_layout_pointer_slot_count(obj as usize, 2), Some(0));

    let child = crate::string::js_string_from_bytes(b"unboxed-child".as_ptr(), 13);
    let child_header = unsafe { header_from_user_ptr(child as *mut u8) };
    crate::object::js_object_set_field(obj, 0, crate::value::JSValue::string_ptr(child));

    assert_eq!(
        test_layout_pointer_slot_count(obj as usize, 2),
        None,
        "non-number writes to raw f64 slots must deopt to full scanning"
    );

    let valid_ptrs = build_valid_pointer_set();
    assert!(try_mark_value(
        POINTER_TAG | (obj as u64 & POINTER_MASK),
        &valid_ptrs
    ));
    test_reset_trace_slot_reads();
    trace_marked_objects(&valid_ptrs);
    assert_eq!(test_trace_slot_reads(), 2);
    unsafe {
        assert_ne!((*child_header).gc_flags & GC_FLAG_MARKED, 0);
    }

    clear_marks();
    clear_mark_seeds();
}

#[test]
fn test_unboxed_object_descriptor_transfers_on_object_move() {
    clear_marks();
    clear_mark_seeds();

    let src = crate::object::js_object_alloc(0, 2);
    let dst = crate::object::js_object_alloc(0, 2);
    crate::object::js_object_set_unboxed_f64_field(src, 0, 3.0);
    crate::object::js_object_set_unboxed_f64_field(src, 1, 4.0);
    js_gc_init_unboxed_object_layout(src as u64, 2, 0b11, 0);

    unsafe {
        layout_transfer(src as *mut u8, dst as *mut u8);
    }

    assert_eq!(test_layout_pointer_slot_count(dst as usize, 2), Some(0));
    crate::object::js_object_set_unboxed_f64_field(dst, 1, 5.0);
    assert_eq!(test_layout_pointer_slot_count(dst as usize, 2), Some(0));

    let child = crate::string::js_string_from_bytes(b"moved-child".as_ptr(), 11);
    crate::object::js_object_set_field(dst, 1, crate::value::JSValue::string_ptr(child));
    assert_eq!(test_layout_pointer_slot_count(dst as usize, 2), None);

    clear_marks();
    clear_mark_seeds();
}

#[test]
fn test_heap_child_iterator_pointer_free_object_yields_no_child_slots() {
    clear_marks();
    clear_mark_seeds();

    let obj = crate::object::js_object_alloc(0, 3);
    crate::object::js_object_set_field(obj, 0, crate::value::JSValue::number(1.0));
    crate::object::js_object_set_field(obj, 1, crate::value::JSValue::number(2.0));
    crate::object::js_object_set_field(obj, 2, crate::value::JSValue::bool(false));

    assert_eq!(test_layout_pointer_slot_count(obj as usize, 3), Some(0));
    assert_eq!(test_heap_child_slot_count(obj as *mut u8), 0);

    let valid_ptrs = build_valid_pointer_set();
    let mut worklist = Vec::new();
    test_reset_trace_slot_reads();
    unsafe {
        trace_object(obj as *mut u8, &valid_ptrs, &mut worklist);
    }
    assert_eq!(test_trace_slot_reads(), 0);

    clear_marks();
    clear_mark_seeds();
}

#[test]
fn test_layout_mask_overflow_fields_and_array_grow_transfer() {
    clear_marks();
    clear_mark_seeds();

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

    assert_eq!(test_layout_pointer_slot_count(obj as usize, 9), None);
    let valid_ptrs = build_valid_pointer_set();
    crate::object::scan_overflow_fields_roots(&mut |value| {
        try_mark_value(value.to_bits(), &valid_ptrs);
    });
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
    assert_eq!(test_layout_pointer_slot_count(mixed as usize, 3), None);

    let valid_ptrs = build_valid_pointer_set();
    assert!(try_mark_value(
        POINTER_TAG | (mixed as u64 & POINTER_MASK),
        &valid_ptrs
    ));
    test_reset_trace_slot_reads();
    trace_marked_objects(&valid_ptrs);
    assert_eq!(test_trace_slot_reads(), 3);
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
fn test_trace_object_uses_pointer_layout_mask() {
    clear_marks();
    clear_mark_seeds();

    let numeric = crate::object::js_object_alloc(0, 3);
    crate::object::js_object_set_field(numeric, 0, crate::value::JSValue::number(1.0));
    crate::object::js_object_set_field(numeric, 1, crate::value::JSValue::number(2.0));
    crate::object::js_object_set_field(numeric, 2, crate::value::JSValue::bool(false));
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

    let child = crate::string::js_string_from_bytes(b"object-child".as_ptr(), 12);
    let child_header = unsafe { header_from_user_ptr(child as *mut u8) };
    let mixed = crate::object::js_object_alloc(0, 3);
    crate::object::js_object_set_field(mixed, 0, crate::value::JSValue::number(1.0));
    crate::object::js_object_set_field(mixed, 1, crate::value::JSValue::string_ptr(child));
    crate::object::js_object_set_field(mixed, 2, crate::value::JSValue::number(3.0));
    assert_eq!(test_layout_pointer_slot_count(mixed as usize, 3), None);

    let valid_ptrs = build_valid_pointer_set();
    assert!(try_mark_value(
        POINTER_TAG | (mixed as u64 & POINTER_MASK),
        &valid_ptrs
    ));
    test_reset_trace_slot_reads();
    trace_marked_objects(&valid_ptrs);
    assert_eq!(test_trace_slot_reads(), 3);
    unsafe {
        assert_ne!((*child_header).gc_flags & GC_FLAG_MARKED, 0);
    }

    clear_marks();
    clear_mark_seeds();
}

#[test]
fn test_typed_shape_descriptor_scans_only_declared_pointer_slots() {
    clear_marks();
    clear_mark_seeds();

    let child = crate::string::js_string_from_bytes(b"typed-child".as_ptr(), 11);
    let child_header = unsafe { header_from_user_ptr(child as *mut u8) };
    let obj = crate::object::js_object_alloc(0, 3);
    crate::object::js_object_set_field(obj, 0, crate::value::JSValue::number(1.0));
    crate::object::js_object_set_field(obj, 1, crate::value::JSValue::string_ptr(child));
    crate::object::js_object_set_field(obj, 2, crate::value::JSValue::number(3.0));

    let mask = [1u64 << 1];
    js_gc_init_typed_shape_layout(obj as u64, 3, mask.as_ptr(), mask.len() as u32);

    assert_eq!(test_layout_pointer_slot_count(obj as usize, 3), Some(1));
    assert_eq!(test_heap_child_slot_count(obj as *mut u8), 1);

    let valid_ptrs = build_valid_pointer_set();
    assert!(try_mark_value(
        POINTER_TAG | (obj as u64 & POINTER_MASK),
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
fn test_typed_shape_descriptor_dynamic_pointer_mutation_falls_back_to_unknown_layout() {
    clear_marks();
    clear_mark_seeds();

    let obj = crate::object::js_object_alloc(0, 2);
    crate::object::js_object_set_field(obj, 0, crate::value::JSValue::number(1.0));
    crate::object::js_object_set_field(obj, 1, crate::value::JSValue::number(2.0));
    js_gc_init_typed_shape_layout(obj as u64, 2, std::ptr::null(), 0);
    assert_eq!(test_layout_pointer_slot_count(obj as usize, 2), Some(0));

    let child = crate::string::js_string_from_bytes(b"fallback-child".as_ptr(), 14);
    let child_header = unsafe { header_from_user_ptr(child as *mut u8) };
    crate::object::js_object_set_field(obj, 0, crate::value::JSValue::string_ptr(child));

    assert_eq!(
        test_layout_pointer_slot_count(obj as usize, 2),
        None,
        "storing a pointer into a non-pointer typed slot must drop to safe full scanning"
    );

    let valid_ptrs = build_valid_pointer_set();
    assert!(try_mark_value(
        POINTER_TAG | (obj as u64 & POINTER_MASK),
        &valid_ptrs
    ));
    test_reset_trace_slot_reads();
    trace_marked_objects(&valid_ptrs);
    assert_eq!(test_trace_slot_reads(), 2);
    unsafe {
        assert_ne!((*child_header).gc_flags & GC_FLAG_MARKED, 0);
    }

    clear_marks();
    clear_mark_seeds();
}

extern "C" fn layout_mask_test_closure(_closure: *const crate::closure::ClosureHeader) -> f64 {
    0.0
}

#[test]
fn test_trace_closure_uses_pointer_layout_mask() {
    clear_marks();
    clear_mark_seeds();

    let numeric = crate::closure::js_closure_alloc(layout_mask_test_closure as *const u8, 3);
    crate::closure::js_closure_set_capture_f64(numeric, 0, 1.0);
    crate::closure::js_closure_set_capture_f64(numeric, 1, 2.0);
    crate::closure::js_closure_set_capture_ptr(numeric, 2, 7);
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

    let child = crate::string::js_string_from_bytes(b"closure-child".as_ptr(), 13) as *mut u8;
    let child_header = unsafe { header_from_user_ptr(child) };
    let mixed = crate::closure::js_closure_alloc(layout_mask_test_closure as *const u8, 3);
    crate::closure::js_closure_set_capture_f64(mixed, 0, 1.0);
    crate::closure::js_closure_set_capture_f64(
        mixed,
        1,
        f64::from_bits(STRING_TAG | (child as u64 & POINTER_MASK)),
    );
    crate::closure::js_closure_set_capture_ptr(mixed, 2, 7);
    assert_eq!(test_layout_pointer_slot_count(mixed as usize, 3), None);

    let valid_ptrs = build_valid_pointer_set();
    assert!(try_mark_value(
        POINTER_TAG | (mixed as u64 & POINTER_MASK),
        &valid_ptrs
    ));
    test_reset_trace_slot_reads();
    trace_marked_objects(&valid_ptrs);
    assert_eq!(test_trace_slot_reads(), 3);
    unsafe {
        assert_ne!((*child_header).gc_flags & GC_FLAG_MARKED, 0);
    }

    clear_marks();
    clear_mark_seeds();
}

#[test]
fn test_gc_collect_updates_stats() {
    // Get initial stats
    let initial_count = GC_STATS.with(|s| s.borrow().collection_count);

    // Run GC
    gc_collect_inner();

    // Stats should have incremented
    let new_count = GC_STATS.with(|s| s.borrow().collection_count);
    assert_eq!(
        new_count,
        initial_count + 1,
        "collection count should increment"
    );
}

#[test]
fn test_gc_header_size() {
    assert_eq!(GC_HEADER_SIZE, 8, "GC header should be 8 bytes");
}

/// Issue #179: block-persist's age window must match the reset
/// policy's `keep_low` window — both define the set of blocks
/// where caller-saved-register handles might still be uncaptured.
/// If the two drift apart, block-persist either over-retains old
/// blocks (RSS regression) or under-protects recent blocks
/// (re-opens the issues #43 / #44 dangling-pointer failure mode).
#[test]
fn block_persist_window_matches_reset_keep_low() {
    // `keep_low = current.saturating_sub(4)` → 5 blocks
    // (current-4..=current). `BLOCK_PERSIST_WINDOW` gates Pass 2
    // of `mark_block_persisting_arena_objects` via
    // `persist_low = general_n.saturating_sub(BLOCK_PERSIST_WINDOW)`.
    // Both windows must describe the same "register-miss risk"
    // horizon for the correctness invariant to hold.
    assert_eq!(
        BLOCK_PERSIST_WINDOW, 5,
        "block-persist window must match reset's keep_low window (5 blocks)"
    );
}

/// Issue #179: `gc_collect_inner` must return the sweep's
/// freed_bytes so the adaptive step logic can react to
/// object-reclaim activity immediately, not wait for blocks to
/// clear the 2-cycle grace and surface as a `pre - post` drop on
/// the next cycle. The return value drives the `>90% halve /
/// 10-90% halve / <10% double` classifier in `gc_check_trigger`.
#[test]
fn gc_collect_inner_returns_freed_bytes() {
    // Allocate an object that's guaranteed unreachable (no
    // roots hold it — we immediately drop the pointer).
    let _throwaway = gc_malloc(128, GC_TYPE_STRING);
    // freed_bytes is the per-sweep reclaim count; for this
    // tiny test we just assert the signature (returns u64).
    // The exact freed count depends on thread-local state from
    // other tests, so we only assert the type/shape.
    let _freed: u64 = gc_collect_inner();
}

#[test]
fn test_gc_realloc_basic() {
    let ptr = gc_malloc(32, GC_TYPE_STRING);
    assert!(!ptr.is_null());

    // Write some data
    unsafe {
        std::ptr::write_bytes(ptr, 0xAB, 32);
    }

    // Reallocate to larger size
    let new_ptr = gc_realloc(ptr, 128);
    assert!(!new_ptr.is_null());

    // Verify old data preserved (first 32 bytes should still be 0xAB)
    unsafe {
        for i in 0..32 {
            assert_eq!(
                *new_ptr.add(i),
                0xAB,
                "byte {} should be preserved after realloc",
                i
            );
        }
    }

    // Verify tracking updated (rebuild lazy set first)
    let tracked = MALLOC_STATE.with(|s| {
        let header = unsafe { header_from_user_ptr(new_ptr) };
        let mut s = s.borrow_mut();
        ensure_set_built(&mut s);
        s.set.contains(&(header as usize))
    });
    assert!(tracked, "reallocated object should be tracked");
}

#[test]
fn test_gc_realloc_null_allocates_fresh() {
    let ptr = gc_realloc(std::ptr::null_mut(), 64);
    assert!(!ptr.is_null(), "realloc(null) should allocate fresh");
}

#[test]
fn test_gc_mark_flags() {
    let ptr = gc_malloc(32, GC_TYPE_STRING);
    unsafe {
        let header = header_from_user_ptr(ptr);

        // Initially not marked
        assert_eq!((*header).gc_flags & GC_FLAG_MARKED, 0);

        // Mark it
        (*header).gc_flags |= GC_FLAG_MARKED;
        assert_ne!((*header).gc_flags & GC_FLAG_MARKED, 0);

        // Clear mark
        (*header).gc_flags &= !GC_FLAG_MARKED;
        assert_eq!((*header).gc_flags & GC_FLAG_MARKED, 0);
    }
}

#[test]
fn test_gc_pinned_flag() {
    let ptr = gc_malloc(32, GC_TYPE_STRING);
    unsafe {
        let header = header_from_user_ptr(ptr);

        // Pin it
        (*header).gc_flags |= GC_FLAG_PINNED;

        // Run GC - pinned objects should survive
        gc_collect_inner();

        // Verify still tracked (rebuild lazy set first)
        let tracked = MALLOC_STATE.with(|s| {
            let mut s = s.borrow_mut();
            ensure_set_built(&mut s);
            s.set.contains(&(header as usize))
        });
        assert!(tracked, "pinned object should survive GC");

        // Unpin
        (*header).gc_flags &= !GC_FLAG_PINNED;
    }
}

#[test]
fn test_build_valid_pointer_set() {
    // Allocate some objects
    let ptr1 = gc_malloc(32, GC_TYPE_STRING);
    let ptr2 = gc_malloc(64, GC_TYPE_CLOSURE);
    unsafe {
        init_test_closure(ptr2);
    }

    let valid_set = build_valid_pointer_set();

    // Our malloc objects should be in the valid set
    assert!(
        valid_set.contains(&(ptr1 as usize)),
        "ptr1 should be in valid set"
    );
    assert!(
        valid_set.contains(&(ptr2 as usize)),
        "ptr2 should be in valid set"
    );
}

/// Helper: reset the shadow stack to a known-empty state
/// between tests. Needed because Rust's thread-local state
/// persists across tests in the same thread.
fn reset_shadow_stack() {
    SHADOW.with(|cell| unsafe {
        let s = &mut *cell.get();
        s.stack.clear();
        s.frame_top = usize::MAX;
    });
}

fn reset_global_roots() {
    GLOBAL_ROOTS.with(|roots| roots.borrow_mut().clear());
}

struct ShadowAndGlobalRootResetGuard;

impl Drop for ShadowAndGlobalRootResetGuard {
    fn drop(&mut self) {
        reset_shadow_stack();
        reset_global_roots();
    }
}

fn assert_panics_with(expected: &str, f: impl FnOnce()) {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    let Err(payload) = result else {
        panic!("expected panic containing {expected:?}");
    };
    let message = if let Some(s) = payload.downcast_ref::<&str>() {
        *s
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.as_str()
    } else {
        "<non-string panic>"
    };
    assert!(
        message.contains(expected),
        "panic message {message:?} did not contain {expected:?}"
    );
}

thread_local! {
    static LOCK_SAFE_RUNTIME_SCANNERS_REGISTERED: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };
}

static LOCK_SAFE_RUNTIME_SCANNER_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lock_safe_runtime_scanner_test_guard() -> std::sync::MutexGuard<'static, ()> {
    LOCK_SAFE_RUNTIME_SCANNER_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn ensure_lock_safe_runtime_scanners_registered() {
    LOCK_SAFE_RUNTIME_SCANNERS_REGISTERED.with(|registered| {
        if registered.get() {
            return;
        }
        gc_register_mutable_root_scanner(crate::tui::hooks::scan_hook_slot_roots_mut);
        gc_register_mutable_root_scanner(crate::tui::state::scan_state_slot_roots_mut);
        #[cfg(feature = "ohos-napi")]
        {
            gc_register_mutable_root_scanner(
                crate::arkts_callbacks::arkts_callbacks_root_scanner_mut,
            );
            gc_register_mutable_root_scanner(
                crate::media_playback::media_callbacks_root_scanner_mut,
            );
        }
        registered.set(true);
    });
}

struct ActiveShadowFrame(u64);

impl ActiveShadowFrame {
    fn push_empty() -> Self {
        reset_shadow_stack();
        Self(js_shadow_frame_push(0))
    }
}

impl Drop for ActiveShadowFrame {
    fn drop(&mut self) {
        js_shadow_frame_pop(self.0);
    }
}

fn lock_safe_runtime_scanner_closure() -> (*mut u8, u64, f64) {
    let ptr = gc_malloc(
        std::mem::size_of::<crate::closure::ClosureHeader>(),
        GC_TYPE_CLOSURE,
    );
    unsafe {
        let closure = ptr as *mut crate::closure::ClosureHeader;
        (*closure).func_ptr = test_no_capture_singleton_func as *const u8;
        (*closure).capture_count = 0;
        (*closure).type_tag = crate::closure::CLOSURE_MAGIC;
        layout_init_pointer_free(ptr);
    }
    let bits = POINTER_TAG | (ptr as u64 & POINTER_MASK);
    (ptr, bits, f64::from_bits(bits))
}

fn malloc_user_ptr_tracked(ptr: *mut u8) -> bool {
    let header = unsafe { header_from_user_ptr(ptr) };
    MALLOC_STATE.with(|s| s.borrow().objects.iter().any(|&tracked| tracked == header))
}

fn activate_malloc_registry_for_tests() {
    MALLOC_STATE.with(|s| {
        let mut s = s.borrow_mut();
        ensure_set_built(&mut s);
    });
}

fn deactivate_malloc_registry_for_tests() {
    MALLOC_STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.set.clear();
        s.registry_state = MallocRegistryState::Inactive;
    });
}

fn malloc_registry_active_for_tests() -> bool {
    MALLOC_STATE.with(|s| s.borrow().malloc_registry_available())
}

fn reset_malloc_kind_telemetry_for_tests() {
    MALLOC_STATE.with(|s| {
        let mut s = s.borrow_mut();
        let mut telemetry = [MallocKindTelemetry::zero(); MALLOC_KIND_BUCKET_COUNT];
        for &header in s.objects.iter() {
            unsafe {
                let counters = &mut telemetry[malloc_kind_index((*header).obj_type)];
                counters.survivor_count = counters.survivor_count.saturating_add(1);
                counters.survivor_bytes = counters
                    .survivor_bytes
                    .saturating_add((*header).size as u64);
            }
        }
        s.kind_telemetry = telemetry;
    });
}

fn malloc_kind_telemetry_for_tests(obj_type: u8) -> MallocKindTelemetry {
    MALLOC_STATE.with(|s| s.borrow().kind_telemetry[malloc_kind_index(obj_type)])
}

#[test]
fn test_gc_type_metadata_covers_all_declared_types() {
    let infos = gc_type_infos().collect::<Vec<_>>();
    assert_eq!(infos.len(), GC_TYPE_MAX as usize);

    let mut seen = [false; MALLOC_KIND_BUCKET_COUNT];
    for info in infos {
        assert_ne!(info.type_id, 0, "unknown is not a declared GC type");
        assert!(
            (info.type_id as usize) < MALLOC_KIND_BUCKET_COUNT,
            "metadata type id out of range: {}",
            info.type_id
        );
        assert!(
            !seen[info.type_id as usize],
            "duplicate metadata for {}",
            info.name
        );
        seen[info.type_id as usize] = true;
        assert_eq!(gc_type_info(info.type_id).copied(), Some(*info));
        assert_eq!(gc_type_name(info.type_id), info.name);
    }

    for type_id in 1..MALLOC_KIND_BUCKET_COUNT {
        assert!(seen[type_id], "missing metadata for GC type {type_id}");
    }

    assert!(gc_type_is_arena_walkable(GC_TYPE_BUFFER));
    assert!(gc_type_is_arena_walkable(GC_TYPE_TYPED_ARRAY));
    assert!(!gc_type_is_movable(GC_TYPE_BUFFER));
    assert!(!gc_type_is_movable(GC_TYPE_TYPED_ARRAY));
}

fn mark_existing_malloc_and_arena_objects_except(excluded: &[usize]) {
    MALLOC_STATE.with(|s| {
        for &tracked in s.borrow().objects.iter() {
            if !excluded.contains(&(tracked as usize)) {
                unsafe {
                    (*tracked).gc_flags |= GC_FLAG_MARKED;
                }
            }
        }
    });
    crate::arena::arena_walk_objects(|arena_header| unsafe {
        (*(arena_header as *mut GcHeader)).gc_flags |= GC_FLAG_MARKED;
    });
}

fn gc_collection_count() -> u64 {
    GC_STATS.with(|s| s.borrow().collection_count)
}

struct GcUnsafeZoneResetGuard;

impl GcUnsafeZoneResetGuard {
    fn clear() -> Self {
        GC_UNSAFE_ZONES.store(0, std::sync::atomic::Ordering::Release);
        GC_UNSAFE_WARNED.store(false, std::sync::atomic::Ordering::Release);
        Self
    }

    fn enter() -> Self {
        let guard = Self::clear();
        GC_UNSAFE_ZONES.store(1, std::sync::atomic::Ordering::Release);
        guard
    }
}

impl Drop for GcUnsafeZoneResetGuard {
    fn drop(&mut self) {
        GC_UNSAFE_ZONES.store(0, std::sync::atomic::Ordering::Release);
        GC_UNSAFE_WARNED.store(false, std::sync::atomic::Ordering::Release);
    }
}

#[test]
fn lock_safe_runtime_scanners_tui_state_defers_gc_check_trigger() {
    let _test_lock = lock_safe_runtime_scanner_test_guard();
    let _reset = ShadowAndGlobalRootResetGuard;
    ensure_lock_safe_runtime_scanners_registered();
    crate::tui::state::test_reset_state_slots();

    let (ptr, bits, value) = lock_safe_runtime_scanner_closure();
    let handle = crate::tui::state::js_perry_tui_state_alloc(value);
    GC_NEXT_MALLOC_TRIGGER.with(|trigger| {
        trigger.set(MALLOC_STATE.with(|s| s.borrow().objects.len()));
    });

    let before = gc_collection_count();
    let _shadow = ActiveShadowFrame::push_empty();
    crate::tui::state::test_with_state_slots_locked(|| {
        gc_check_trigger();
        assert_eq!(
            gc_collection_count(),
            before,
            "gc_check_trigger should defer while a state root lock is held"
        );
    });

    assert!(
        gc_collection_count() > before,
        "deferred trigger check should run after the state root lock is released"
    );
    assert!(
        malloc_user_ptr_tracked(ptr),
        "state slot root should survive the deferred collection"
    );
    assert_eq!(
        crate::tui::state::js_perry_tui_state_get(handle).to_bits(),
        bits
    );
    crate::tui::state::test_reset_state_slots();
}

#[test]
fn lock_safe_runtime_scanners_tui_hooks_defers_direct_minor_gc() {
    let _test_lock = lock_safe_runtime_scanner_test_guard();
    let _reset = ShadowAndGlobalRootResetGuard;
    ensure_lock_safe_runtime_scanners_registered();

    let (ptr, bits, _value) = lock_safe_runtime_scanner_closure();
    crate::tui::hooks::test_seed_hook_slot_roots(bits);

    let before = gc_collection_count();
    let _shadow = ActiveShadowFrame::push_empty();
    crate::tui::hooks::test_with_hook_slots_locked(|| {
        let freed = gc_collect_minor();
        assert_eq!(freed, 0);
        assert_eq!(
            gc_collection_count(),
            before,
            "direct minor GC should defer while a hook root lock is held"
        );
    });

    assert!(
        gc_collection_count() > before,
        "deferred direct minor GC should run after the hook root lock is released"
    );
    assert!(
        malloc_user_ptr_tracked(ptr),
        "hook slot root should survive the deferred collection"
    );
    assert_eq!(
        crate::tui::hooks::test_hook_slot_roots(),
        (bits, bits, bits)
    );
}

#[test]
fn lock_safe_runtime_scanners_tui_state_defers_manual_gc() {
    let _test_lock = lock_safe_runtime_scanner_test_guard();
    let _reset = ShadowAndGlobalRootResetGuard;
    ensure_lock_safe_runtime_scanners_registered();
    crate::tui::state::test_reset_state_slots();
    let _unsafe_zone = GcUnsafeZoneResetGuard::clear();

    let (ptr, bits, value) = lock_safe_runtime_scanner_closure();
    let handle = crate::tui::state::js_perry_tui_state_alloc(value);

    let before = gc_collection_count();
    let _shadow = ActiveShadowFrame::push_empty();
    crate::tui::state::test_with_state_slots_locked(|| {
        js_gc_collect();
        assert_eq!(
            gc_collection_count(),
            before,
            "manual GC should defer while a state root lock is held"
        );
    });

    assert!(
        gc_collection_count() > before,
        "deferred manual GC should run after the state root lock is released"
    );
    assert!(
        malloc_user_ptr_tracked(ptr),
        "state slot root should survive deferred manual GC"
    );
    assert_eq!(
        crate::tui::state::js_perry_tui_state_get(handle).to_bits(),
        bits
    );
    crate::tui::state::test_reset_state_slots();
}

#[test]
fn lock_safe_runtime_scanners_manual_gc_unsafe_zone_stays_noop_after_unlock() {
    let _test_lock = lock_safe_runtime_scanner_test_guard();
    let _reset = ShadowAndGlobalRootResetGuard;
    ensure_lock_safe_runtime_scanners_registered();
    crate::tui::state::test_reset_state_slots();
    let _unsafe_zone = GcUnsafeZoneResetGuard::enter();

    let (_ptr, bits, value) = lock_safe_runtime_scanner_closure();
    let handle = crate::tui::state::js_perry_tui_state_alloc(value);

    let before = gc_collection_count();
    let _shadow = ActiveShadowFrame::push_empty();
    crate::tui::state::test_with_state_slots_locked(|| {
        js_gc_collect();
        assert_eq!(
            gc_collection_count(),
            before,
            "manual GC should no-op while unsafe zones are active"
        );
    });

    assert_eq!(
        gc_collection_count(),
        before,
        "manual GC skipped by an unsafe zone must not flush after the state root lock unlocks"
    );
    assert_eq!(
        crate::tui::state::js_perry_tui_state_get(handle).to_bits(),
        bits
    );
    crate::tui::state::test_reset_state_slots();
}

#[test]
fn lock_safe_runtime_scanners_deferred_manual_gc_respects_unsafe_zone_at_flush() {
    let _test_lock = lock_safe_runtime_scanner_test_guard();
    let _reset = ShadowAndGlobalRootResetGuard;
    ensure_lock_safe_runtime_scanners_registered();
    crate::tui::state::test_reset_state_slots();
    let _unsafe_zone = GcUnsafeZoneResetGuard::clear();

    let (_ptr, bits, value) = lock_safe_runtime_scanner_closure();
    let handle = crate::tui::state::js_perry_tui_state_alloc(value);

    let before = gc_collection_count();
    let _shadow = ActiveShadowFrame::push_empty();
    crate::tui::state::test_with_state_slots_locked(|| {
        js_gc_collect();
        assert_eq!(
            gc_collection_count(),
            before,
            "manual GC should defer while a state root lock is held"
        );
        GC_UNSAFE_ZONES.store(1, std::sync::atomic::Ordering::Release);
    });

    assert_eq!(
        gc_collection_count(),
        before,
        "deferred manual GC should re-check unsafe zones before flushing after unlock"
    );
    assert_eq!(
        crate::tui::state::js_perry_tui_state_get(handle).to_bits(),
        bits
    );
    crate::tui::state::test_reset_state_slots();
}

#[test]
fn lock_safe_runtime_scanners_tui_hooks_defers_direct_full_gc() {
    let _test_lock = lock_safe_runtime_scanner_test_guard();
    let _reset = ShadowAndGlobalRootResetGuard;
    ensure_lock_safe_runtime_scanners_registered();

    let (ptr, bits, _value) = lock_safe_runtime_scanner_closure();
    crate::tui::hooks::test_seed_hook_slot_roots(bits);

    let before = gc_collection_count();
    let _shadow = ActiveShadowFrame::push_empty();
    crate::tui::hooks::test_with_hook_slots_locked(|| {
        let freed = gc_collect_inner();
        assert_eq!(freed, 0);
        assert_eq!(
            gc_collection_count(),
            before,
            "direct full GC should defer while a hook root lock is held"
        );
    });

    assert!(
        gc_collection_count() > before,
        "deferred direct full GC should run after the hook root lock is released"
    );
    assert!(
        malloc_user_ptr_tracked(ptr),
        "hook slot root should survive deferred direct full GC"
    );
    assert_eq!(
        crate::tui::hooks::test_hook_slot_roots(),
        (bits, bits, bits)
    );
}

#[cfg(feature = "ohos-napi")]
#[test]
fn lock_safe_runtime_scanners_arkts_callbacks_defers_direct_minor_gc() {
    let _test_lock = lock_safe_runtime_scanner_test_guard();
    let _reset = ShadowAndGlobalRootResetGuard;
    ensure_lock_safe_runtime_scanners_registered();
    crate::arkts_callbacks::test_clear_arkts_callback_roots();

    let (ptr, bits, value) = lock_safe_runtime_scanner_closure();
    let callback_idx = 17;
    crate::arkts_callbacks::test_seed_arkts_callback_root(callback_idx, value);

    let before = gc_collection_count();
    let _shadow = ActiveShadowFrame::push_empty();
    crate::arkts_callbacks::test_with_arkts_callback_roots_locked(|| {
        let freed = gc_collect_minor();
        assert_eq!(freed, 0);
        assert_eq!(
            gc_collection_count(),
            before,
            "direct minor GC should defer while ArkTS callback roots are locked"
        );
    });

    assert!(
        gc_collection_count() > before,
        "deferred direct minor GC should run after ArkTS callback roots unlock"
    );
    assert!(
        malloc_user_ptr_tracked(ptr),
        "ArkTS callback root should survive deferred GC"
    );
    assert_eq!(
        crate::arkts_callbacks::test_arkts_callback_root(callback_idx),
        bits
    );
    crate::arkts_callbacks::test_clear_arkts_callback_roots();
}

#[cfg(feature = "ohos-napi")]
#[test]
fn lock_safe_runtime_scanners_media_callbacks_defers_direct_minor_gc() {
    let _test_lock = lock_safe_runtime_scanner_test_guard();
    let _reset = ShadowAndGlobalRootResetGuard;
    ensure_lock_safe_runtime_scanners_registered();

    let (ptr, bits, value) = lock_safe_runtime_scanner_closure();
    let handle = i64::MIN + 861;
    crate::media_playback::test_seed_media_callback_roots(handle, value, value);

    let before = gc_collection_count();
    let _shadow = ActiveShadowFrame::push_empty();
    crate::media_playback::test_with_media_callback_roots_locked(|| {
        let freed = gc_collect_minor();
        assert_eq!(freed, 0);
        assert_eq!(
            gc_collection_count(),
            before,
            "direct minor GC should defer while media callback roots are locked"
        );
    });

    assert!(
        gc_collection_count() > before,
        "deferred direct minor GC should run after media callback roots unlock"
    );
    assert!(
        malloc_user_ptr_tracked(ptr),
        "media callback root should survive deferred GC"
    );
    assert_eq!(
        crate::media_playback::test_media_callback_roots(handle),
        (bits, bits)
    );
}

#[test]
fn test_conservative_stack_scan_auto_policy_skips_active_shadow_frame() {
    let _guard = ShadowAndGlobalRootResetGuard;
    reset_shadow_stack();
    assert_eq!(
        conservative_stack_scan_mode_from_value(None),
        ConservativeStackScanMode::Auto
    );
    assert_eq!(
        conservative_stack_scan_decision_for(ConservativeStackScanMode::Auto, false),
        ConservativeStackScanDecision::Scan
    );

    let h = js_shadow_frame_push(1);
    assert!(shadow_stack_has_active_frame());
    assert_eq!(
        conservative_stack_scan_decision_for(
            ConservativeStackScanMode::Auto,
            shadow_stack_has_active_frame()
        ),
        ConservativeStackScanDecision::SkipShadowStackActive
    );
    js_shadow_frame_pop(h);
}

#[test]
fn test_conservative_stack_scan_env_off_disables_decision() {
    for value in ["0", "off", "false"] {
        let mode = conservative_stack_scan_mode_from_value(Some(value));
        assert_eq!(mode, ConservativeStackScanMode::Disabled);
        assert_eq!(
            conservative_stack_scan_decision_for(mode, false),
            ConservativeStackScanDecision::SkipDisabled
        );
        assert_eq!(
            conservative_stack_scan_decision_for(mode, true),
            ConservativeStackScanDecision::SkipDisabled
        );
    }
}

#[test]
fn test_conservative_stack_scan_full_preserves_legacy_fallback_decision() {
    for value in ["1", "on", "true", "full", "debug"] {
        let mode = conservative_stack_scan_mode_from_value(Some(value));
        assert_eq!(mode, ConservativeStackScanMode::Full);
        assert_eq!(
            conservative_stack_scan_decision_for(mode, false),
            ConservativeStackScanDecision::Scan
        );
        assert_eq!(
            conservative_stack_scan_decision_for(mode, true),
            ConservativeStackScanDecision::Scan
        );
    }
}

#[test]
fn test_shadow_stack_push_pop_single_frame() {
    reset_shadow_stack();
    assert_eq!(shadow_stack_depth(), 0);
    let h = js_shadow_frame_push(3);
    assert_eq!(shadow_stack_depth(), 1);
    // Slots initialized to 0.
    for i in 0..3 {
        assert_eq!(js_shadow_slot_get(i), 0, "slot {} not zero", i);
    }
    js_shadow_frame_pop(h);
    assert_eq!(shadow_stack_depth(), 0);
    // After pop, reads return 0 (no active frame).
    assert_eq!(js_shadow_slot_get(0), 0);
}

#[test]
fn test_shadow_stack_slot_store_load() {
    reset_shadow_stack();
    let h = js_shadow_frame_push(4);
    // Store some pointer bit patterns.
    js_shadow_slot_set(0, 0x7FFD_0000_1234_5678); // POINTER_TAG
    js_shadow_slot_set(1, 0x7FFF_0000_9ABC_DEF0); // STRING_TAG
    js_shadow_slot_set(2, 0); // hole
    js_shadow_slot_set(3, 0x7FF9_0200_0000_6B6F); // SSO "ok"
    assert_eq!(js_shadow_slot_get(0), 0x7FFD_0000_1234_5678);
    assert_eq!(js_shadow_slot_get(1), 0x7FFF_0000_9ABC_DEF0);
    assert_eq!(js_shadow_slot_get(2), 0);
    assert_eq!(js_shadow_slot_get(3), 0x7FF9_0200_0000_6B6F);
    // Out-of-range read returns 0 (clamp).
    assert_eq!(js_shadow_slot_get(4), 0);
    js_shadow_frame_pop(h);
}

#[test]
fn test_shadow_stack_nested_frames() {
    reset_shadow_stack();
    let outer = js_shadow_frame_push(2);
    js_shadow_slot_set(0, 0x1111);
    js_shadow_slot_set(1, 0x2222);
    assert_eq!(shadow_stack_depth(), 1);

    let inner = js_shadow_frame_push(3);
    js_shadow_slot_set(0, 0xAAAA);
    js_shadow_slot_set(1, 0xBBBB);
    js_shadow_slot_set(2, 0xCCCC);
    assert_eq!(shadow_stack_depth(), 2);
    // Inner frame sees its own slots, not the outer's.
    assert_eq!(js_shadow_slot_get(0), 0xAAAA);
    assert_eq!(js_shadow_slot_get(1), 0xBBBB);
    assert_eq!(js_shadow_slot_get(2), 0xCCCC);

    js_shadow_frame_pop(inner);
    assert_eq!(shadow_stack_depth(), 1);
    // Outer slots preserved across the inner push+pop — this is
    // the load-bearing invariant for codegen: a called function
    // can freely mutate its own frame without corrupting the
    // caller's.
    assert_eq!(js_shadow_slot_get(0), 0x1111);
    assert_eq!(js_shadow_slot_get(1), 0x2222);

    js_shadow_frame_pop(outer);
    assert_eq!(shadow_stack_depth(), 0);
}

#[test]
fn test_shadow_stack_frame_with_zero_slots() {
    reset_shadow_stack();
    let h = js_shadow_frame_push(0);
    assert_eq!(shadow_stack_depth(), 1);
    // No slots to read; get returns 0 anyway (out-of-range path).
    assert_eq!(js_shadow_slot_get(0), 0);
    js_shadow_frame_pop(h);
    assert_eq!(shadow_stack_depth(), 0);
}

#[test]
fn test_shadow_stack_deep_nesting() {
    reset_shadow_stack();
    let mut handles = Vec::new();
    for i in 0..16 {
        let h = js_shadow_frame_push(2);
        js_shadow_slot_set(0, i as u64);
        js_shadow_slot_set(1, (i * 2) as u64);
        handles.push(h);
    }
    assert_eq!(shadow_stack_depth(), 16);
    // Pop back down; slots restore on each pop.
    for i in (0..16).rev() {
        assert_eq!(js_shadow_slot_get(0), i as u64);
        assert_eq!(js_shadow_slot_get(1), (i * 2) as u64);
        js_shadow_frame_pop(handles.pop().unwrap());
    }
    assert_eq!(shadow_stack_depth(), 0);
}

#[test]
fn test_shadow_stack_root_scanner_empty() {
    reset_shadow_stack();
    let mut count = 0;
    shadow_stack_root_scanner(&mut |_| count += 1);
    assert_eq!(count, 0, "empty shadow stack yields no roots");
}

#[test]
fn test_shadow_stack_root_scanner_single_frame() {
    reset_shadow_stack();
    let h = js_shadow_frame_push(4);
    // Mix of set / unset slots.
    js_shadow_slot_set(0, 0x7FFD_0000_1234_5678);
    // slot 1 left zero — must NOT be emitted
    js_shadow_slot_set(2, 0x7FFF_0000_9ABC_DEF0);
    js_shadow_slot_set(3, 0x7FFA_0000_DEAD_BEEF);
    let mut emitted: Vec<u64> = Vec::new();
    shadow_stack_root_scanner(&mut |v| emitted.push(v.to_bits()));
    assert_eq!(emitted.len(), 3, "only non-zero slots should be emitted");
    assert!(emitted.contains(&0x7FFD_0000_1234_5678));
    assert!(emitted.contains(&0x7FFF_0000_9ABC_DEF0));
    assert!(emitted.contains(&0x7FFA_0000_DEAD_BEEF));
    js_shadow_frame_pop(h);
}

#[test]
fn test_shadow_stack_root_scanner_nested_frames() {
    reset_shadow_stack();
    let outer = js_shadow_frame_push(2);
    js_shadow_slot_set(0, 0xAAAA);
    js_shadow_slot_set(1, 0xBBBB);
    let inner = js_shadow_frame_push(3);
    js_shadow_slot_set(0, 0xCCCC);
    js_shadow_slot_set(1, 0xDDDD);
    js_shadow_slot_set(2, 0xEEEE);

    let mut emitted: Vec<u64> = Vec::new();
    shadow_stack_root_scanner(&mut |v| emitted.push(v.to_bits()));

    // Scanner should hit BOTH frames — outer frame's slots
    // must also be reported, not just the innermost. This is
    // the load-bearing invariant for Phase B+ where the GC
    // collects while deep in a call chain.
    assert_eq!(emitted.len(), 5);
    assert!(emitted.contains(&0xAAAA));
    assert!(emitted.contains(&0xBBBB));
    assert!(emitted.contains(&0xCCCC));
    assert!(emitted.contains(&0xDDDD));
    assert!(emitted.contains(&0xEEEE));

    js_shadow_frame_pop(inner);
    js_shadow_frame_pop(outer);
}

#[test]
fn test_shadow_stack_root_scanner_zero_slot_frames() {
    reset_shadow_stack();
    // Zero-slot frame (function with no pointer-typed locals)
    // contributes nothing. Nested non-zero frame still works.
    let a = js_shadow_frame_push(0);
    let b = js_shadow_frame_push(2);
    js_shadow_slot_set(0, 0x1234);
    js_shadow_slot_set(1, 0x5678);
    let c = js_shadow_frame_push(0);

    let mut emitted: Vec<u64> = Vec::new();
    shadow_stack_root_scanner(&mut |v| emitted.push(v.to_bits()));
    assert_eq!(emitted.len(), 2);

    js_shadow_frame_pop(c);
    js_shadow_frame_pop(b);
    js_shadow_frame_pop(a);
}

/// Helper for write-barrier tests: clear the remembered set
/// to a known-empty state.
fn reset_remembered_set() {
    remembered_set_clear();
    crate::arena::old_arena_page_index_clear_for_tests();
}

static COPYING_NURSERY_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn copying_nursery_isolation_lock() -> std::sync::MutexGuard<'static, ()> {
    COPYING_NURSERY_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

struct CopyingNurseryTestGuard {
    frame: u64,
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl CopyingNurseryTestGuard {
    fn new(slot_count: u32) -> Self {
        let lock = copying_nursery_isolation_lock();
        reset_shadow_stack();
        reset_global_roots();
        reset_remembered_set();
        js_gc_write_barriers_emitted(1);
        let frame = js_shadow_frame_push(slot_count);
        Self { frame, _lock: lock }
    }
}

impl Drop for CopyingNurseryTestGuard {
    fn drop(&mut self) {
        js_shadow_frame_pop(self.frame);
        reset_shadow_stack();
        reset_global_roots();
        reset_remembered_set();
        js_gc_write_barriers_emitted(0);
    }
}

struct GcTriggerThresholdTestGuard {
    next_arena_trigger: usize,
    next_malloc_trigger: usize,
    malloc_step: usize,
}

impl GcTriggerThresholdTestGuard {
    fn suppress_automatic_triggers() -> Self {
        let next_arena_trigger = GC_NEXT_TRIGGER_BYTES.with(|trigger| {
            let previous = trigger.get();
            trigger.set(usize::MAX);
            previous
        });
        let next_malloc_trigger = GC_NEXT_MALLOC_TRIGGER.with(|trigger| {
            let previous = trigger.get();
            trigger.set(usize::MAX);
            previous
        });
        let malloc_step = GC_MALLOC_COUNT_STEP.with(|step| step.get());
        Self {
            next_arena_trigger,
            next_malloc_trigger,
            malloc_step,
        }
    }

    fn make_malloc_sweep_due(&self) {
        let current = malloc_object_count();
        GC_NEXT_MALLOC_TRIGGER.with(|trigger| trigger.set(current));
    }

    fn make_arena_trigger_due(&self) {
        GC_NEXT_TRIGGER_BYTES.with(|trigger| trigger.set(0));
    }
}

impl Drop for GcTriggerThresholdTestGuard {
    fn drop(&mut self) {
        GC_NEXT_TRIGGER_BYTES.with(|trigger| trigger.set(self.next_arena_trigger));
        GC_NEXT_MALLOC_TRIGGER.with(|trigger| trigger.set(self.next_malloc_trigger));
        GC_MALLOC_COUNT_STEP.with(|step| step.set(self.malloc_step));
    }
}

struct GcBumpTriggerTestGuard {
    next_arena_trigger: usize,
    arena_step: usize,
    next_malloc_trigger: usize,
    malloc_step: usize,
    trigger_bumped: bool,
    pre_suppress_bytes: usize,
}

impl GcBumpTriggerTestGuard {
    fn new(next_arena_trigger: usize, arena_step: usize) -> Self {
        let previous = Self {
            next_arena_trigger: GC_NEXT_TRIGGER_BYTES.with(|trigger| {
                let previous = trigger.get();
                trigger.set(next_arena_trigger);
                previous
            }),
            arena_step: GC_STEP_BYTES.with(|step| {
                let previous = step.get();
                step.set(arena_step);
                previous
            }),
            next_malloc_trigger: GC_NEXT_MALLOC_TRIGGER.with(|trigger| {
                let previous = trigger.get();
                trigger.set(usize::MAX);
                previous
            }),
            malloc_step: GC_MALLOC_COUNT_STEP.with(|step| step.get()),
            trigger_bumped: GC_TRIGGER_BUMPED.with(|bumped| {
                let previous = bumped.get();
                bumped.set(false);
                previous
            }),
            pre_suppress_bytes: GC_PRE_SUPPRESS_BYTES.with(|bytes| bytes.get()),
        };
        GC_PRE_SUPPRESS_BYTES.with(|bytes| bytes.set(0));
        previous
    }

    fn set_pre_suppress(bytes: usize) {
        GC_PRE_SUPPRESS_BYTES.with(|pre| pre.set(bytes));
    }

    fn next_arena_trigger() -> usize {
        GC_NEXT_TRIGGER_BYTES.with(|trigger| trigger.get())
    }

    fn trigger_bumped() -> bool {
        GC_TRIGGER_BUMPED.with(|bumped| bumped.get())
    }

    fn reset_cycle_bump() {
        GC_TRIGGER_BUMPED.with(|bumped| bumped.set(false));
    }
}

impl Drop for GcBumpTriggerTestGuard {
    fn drop(&mut self) {
        GC_NEXT_TRIGGER_BYTES.with(|trigger| trigger.set(self.next_arena_trigger));
        GC_STEP_BYTES.with(|step| step.set(self.arena_step));
        GC_NEXT_MALLOC_TRIGGER.with(|trigger| trigger.set(self.next_malloc_trigger));
        GC_MALLOC_COUNT_STEP.with(|step| step.set(self.malloc_step));
        GC_TRIGGER_BUMPED.with(|bumped| bumped.set(self.trigger_bumped));
        GC_PRE_SUPPRESS_BYTES.with(|bytes| bytes.set(self.pre_suppress_bytes));
    }
}

#[test]
fn test_gc_bump_tiny_parse_caps_arena_trigger_at_collector_ceiling() {
    let _guard = GcBumpTriggerTestGuard::new(0, GC_THRESHOLD_INITIAL_BYTES);
    let bytes_now = GC_TRIGGER_ABSOLUTE_CEILING - 1024;
    GcBumpTriggerTestGuard::set_pre_suppress(bytes_now);

    assert!(gc_bump_malloc_trigger_with_snapshot(0, bytes_now));

    assert_eq!(
        GcBumpTriggerTestGuard::next_arena_trigger(),
        GC_TRIGGER_ABSOLUTE_CEILING
    );
    assert!(
        !GcBumpTriggerTestGuard::trigger_bumped(),
        "tiny parses must not consume the medium/large per-cycle bump"
    );
}

#[test]
fn test_gc_bump_repeated_tiny_parses_cannot_exceed_collector_ceiling() {
    let _guard = GcBumpTriggerTestGuard::new(
        GC_TRIGGER_ABSOLUTE_CEILING - (2 * 1024 * 1024),
        GC_THRESHOLD_INITIAL_BYTES,
    );

    let first_bytes_now = GC_TRIGGER_ABSOLUTE_CEILING - 1024;
    GcBumpTriggerTestGuard::set_pre_suppress(first_bytes_now);
    assert!(gc_bump_malloc_trigger_with_snapshot(0, first_bytes_now));
    assert_eq!(
        GcBumpTriggerTestGuard::next_arena_trigger(),
        GC_TRIGGER_ABSOLUTE_CEILING
    );

    let later_bytes_now = GC_TRIGGER_ABSOLUTE_CEILING + (32 * 1024 * 1024);
    GcBumpTriggerTestGuard::set_pre_suppress(later_bytes_now);
    assert!(gc_bump_malloc_trigger_with_snapshot(0, later_bytes_now));

    assert_eq!(
        GcBumpTriggerTestGuard::next_arena_trigger(),
        GC_TRIGGER_ABSOLUTE_CEILING
    );
}

#[test]
fn test_gc_bump_one_block_parse_uses_tiny_ceiling() {
    let _guard = GcBumpTriggerTestGuard::new(0, GC_THRESHOLD_INITIAL_BYTES);
    let bytes_now = GC_TRIGGER_ABSOLUTE_CEILING + GC_SUPPRESSED_TINY_PARSE_BYTES;
    GcBumpTriggerTestGuard::set_pre_suppress(bytes_now - GC_SUPPRESSED_TINY_PARSE_BYTES);

    assert!(gc_bump_malloc_trigger_with_snapshot(0, bytes_now));

    assert_eq!(
        GcBumpTriggerTestGuard::next_arena_trigger(),
        GC_TRIGGER_ABSOLUTE_CEILING
    );
    assert!(!GcBumpTriggerTestGuard::trigger_bumped());
}

#[test]
fn test_gc_bump_medium_parse_allows_one_arena_bump_per_gc_cycle() {
    let _guard = GcBumpTriggerTestGuard::new(0, GC_THRESHOLD_INITIAL_BYTES);
    let first_bytes_now = 2 * GC_SUPPRESSED_TINY_PARSE_BYTES;
    let first_expected = first_bytes_now + GC_THRESHOLD_INITIAL_BYTES;

    GcBumpTriggerTestGuard::set_pre_suppress(0);
    assert!(!gc_bump_malloc_trigger_with_snapshot(0, first_bytes_now));
    assert_eq!(GcBumpTriggerTestGuard::next_arena_trigger(), first_expected);
    assert!(GcBumpTriggerTestGuard::trigger_bumped());

    let later_bytes_now = first_expected + (16 * 1024 * 1024);
    assert!(!gc_bump_malloc_trigger_with_snapshot(0, later_bytes_now));
    assert_eq!(
        GcBumpTriggerTestGuard::next_arena_trigger(),
        first_expected,
        "second medium/large bump in the same cycle must be ignored"
    );

    GcBumpTriggerTestGuard::reset_cycle_bump();
    let second_expected = later_bytes_now + GC_THRESHOLD_INITIAL_BYTES;
    assert!(!gc_bump_malloc_trigger_with_snapshot(0, later_bytes_now));
    assert_eq!(
        GcBumpTriggerTestGuard::next_arena_trigger(),
        second_expected
    );
    assert!(GcBumpTriggerTestGuard::trigger_bumped());
}

#[test]
fn test_gc_bump_never_lowers_existing_arena_trigger() {
    let existing_trigger = GC_TRIGGER_ABSOLUTE_CEILING + (32 * 1024 * 1024);
    let _guard = GcBumpTriggerTestGuard::new(existing_trigger, GC_THRESHOLD_INITIAL_BYTES);
    let bytes_now = GC_TRIGGER_ABSOLUTE_CEILING + (16 * 1024 * 1024);
    GcBumpTriggerTestGuard::set_pre_suppress(bytes_now);

    assert!(gc_bump_malloc_trigger_with_snapshot(0, bytes_now));

    assert_eq!(
        GcBumpTriggerTestGuard::next_arena_trigger(),
        existing_trigger
    );
    assert!(!GcBumpTriggerTestGuard::trigger_bumped());
}

#[test]
fn test_old_reclaim_pressure_uses_threshold_and_growth() {
    assert!(!old_reclaim_pressure_due(
        GC_OLD_GEN_RECLAIM_THRESHOLD_BYTES - 1,
        GC_OLD_GEN_RECLAIM_GROWTH_BYTES,
    ));
    assert!(old_reclaim_pressure_due(
        GC_OLD_GEN_RECLAIM_THRESHOLD_BYTES,
        GC_OLD_GEN_RECLAIM_THRESHOLD_BYTES - 1,
    ));
    assert!(!old_reclaim_pressure_due(
        GC_OLD_GEN_RECLAIM_THRESHOLD_BYTES + 1,
        GC_OLD_GEN_RECLAIM_THRESHOLD_BYTES,
    ));
    assert!(old_reclaim_pressure_due(
        GC_OLD_GEN_RECLAIM_THRESHOLD_BYTES + GC_OLD_GEN_RECLAIM_GROWTH_BYTES,
        GC_OLD_GEN_RECLAIM_THRESHOLD_BYTES,
    ));
}

#[test]
fn test_copying_minor_promotion_handoff_uses_predicted_old_pressure() {
    assert!(!copied_minor_promotion_handoff_pressure_due(
        GC_COPY_PROMOTION_HANDOFF_MIN_BYTES - 1,
        GC_OLD_GEN_RECLAIM_THRESHOLD_BYTES,
        0,
    ));
    assert!(copied_minor_promotion_handoff_pressure_due(
        GC_COPY_PROMOTION_HANDOFF_MIN_BYTES,
        GC_OLD_GEN_RECLAIM_THRESHOLD_BYTES - GC_COPY_PROMOTION_HANDOFF_MIN_BYTES,
        0,
    ));
    assert!(copied_minor_promotion_handoff_pressure_due(
        26 * 1024 * 1024,
        20 * 1024 * 1024,
        8 * 1024 * 1024,
    ));
    assert!(!copied_minor_promotion_handoff_pressure_due(
        26 * 1024 * 1024,
        20 * 1024 * 1024,
        20 * 1024 * 1024,
    ));
}

fn collect_minor_trace(trigger_kind: GcTriggerKind) -> GcCycleTrace {
    gc_collect_minor_with_trigger(GcTriggerSnapshot {
        kind: trigger_kind,
        steps_before: Some(GcStepSnapshot::current()),
    })
    .trace
    .expect("test requested GC trace capture")
}

fn assert_copied_minor_trace(
    trace: &GcCycleTrace,
    eligible: bool,
    fallback_reason: CopiedMinorFallbackReason,
    malloc_sweep_due: bool,
) {
    assert_eq!(trace.copying_nursery.eligible, eligible);
    assert_eq!(trace.copying_nursery.fallback_reason, fallback_reason);
    assert_eq!(trace.copying_nursery.malloc_sweep_due, malloc_sweep_due);
}

static ENV_VAR_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct EnvVarGuard {
    key: &'static str,
    previous: Option<std::ffi::OsString>,
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: &'static str) -> Self {
        let lock = ENV_VAR_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous = std::env::var_os(key);
        std::env::set_var(key, value);
        Self {
            key,
            previous,
            _lock: lock,
        }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        if let Some(previous) = self.previous.as_ref() {
            std::env::set_var(self.key, previous);
        } else {
            std::env::remove_var(self.key);
        }
    }
}

static GENERATED_BARRIER_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct GeneratedWriteBarrierTestGuard {
    previous: usize,
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl GeneratedWriteBarrierTestGuard {
    fn active() -> Self {
        let lock = GENERATED_BARRIER_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous = GENERATED_WRITE_BARRIERS_EMITTED.swap(0, Ordering::AcqRel);
        js_gc_write_barriers_emitted(1);
        Self {
            previous,
            _lock: lock,
        }
    }

    fn inactive() -> Self {
        let lock = GENERATED_BARRIER_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous = GENERATED_WRITE_BARRIERS_EMITTED.swap(0, Ordering::AcqRel);
        Self {
            previous,
            _lock: lock,
        }
    }
}

impl Drop for GeneratedWriteBarrierTestGuard {
    fn drop(&mut self) {
        GENERATED_WRITE_BARRIERS_EMITTED.store(self.previous, Ordering::Release);
    }
}

thread_local! {
    static TEST_COPY_ONLY_ROOTS: RefCell<Vec<f64>> = const { RefCell::new(Vec::new()) };
}

fn test_copy_only_root_scanner(mark: &mut dyn FnMut(f64)) {
    TEST_COPY_ONLY_ROOTS.with(|roots| {
        for &value in roots.borrow().iter() {
            mark(value);
        }
    });
}

extern "C" fn test_ffi_copy_only_root_scanner(mark: PerryFfiRootMarker, ctx: *mut c_void) {
    TEST_COPY_ONLY_ROOTS.with(|roots| {
        for &value in roots.borrow().iter() {
            mark(value, ctx);
        }
    });
}

enum TemporaryCopyOnlyRootScannerKind {
    Rust,
    Ffi,
}

struct TemporaryCopyOnlyRootScanner {
    previous_rust_len: usize,
    previous_ffi_len: usize,
    previous_roots: Vec<f64>,
}

impl TemporaryCopyOnlyRootScanner {
    fn rust_bits(bits: &[u64]) -> Self {
        Self::new(TemporaryCopyOnlyRootScannerKind::Rust, bits)
    }

    fn ffi_bits(bits: &[u64]) -> Self {
        Self::new(TemporaryCopyOnlyRootScannerKind::Ffi, bits)
    }

    fn new(kind: TemporaryCopyOnlyRootScannerKind, bits: &[u64]) -> Self {
        let previous_roots = TEST_COPY_ONLY_ROOTS.with(|roots| {
            roots.replace(bits.iter().copied().map(f64::from_bits).collect::<Vec<_>>())
        });
        let previous_rust_len = ROOT_SCANNERS.with(|scanners| {
            let mut scanners = scanners.borrow_mut();
            let previous_rust_len = scanners.len();
            if matches!(kind, TemporaryCopyOnlyRootScannerKind::Rust) {
                scanners.push(test_copy_only_root_scanner);
            }
            previous_rust_len
        });
        let previous_ffi_len = FFI_ROOT_SCANNERS.with(|scanners| {
            let mut scanners = scanners.borrow_mut();
            let previous_ffi_len = scanners.len();
            if matches!(kind, TemporaryCopyOnlyRootScannerKind::Ffi) {
                scanners.push(test_ffi_copy_only_root_scanner);
            }
            previous_ffi_len
        });
        Self {
            previous_rust_len,
            previous_ffi_len,
            previous_roots,
        }
    }
}

impl Drop for TemporaryCopyOnlyRootScanner {
    fn drop(&mut self) {
        ROOT_SCANNERS.with(|scanners| {
            scanners.borrow_mut().truncate(self.previous_rust_len);
        });
        FFI_ROOT_SCANNERS.with(|scanners| {
            scanners.borrow_mut().truncate(self.previous_ffi_len);
        });
        TEST_COPY_ONLY_ROOTS.with(|roots| {
            roots.replace(std::mem::take(&mut self.previous_roots));
        });
    }
}

#[derive(Default)]
struct TestFfiMutableRootSlots {
    i64_slots: Vec<i64>,
    usize_slots: Vec<usize>,
    raw_ptr_slots: Vec<*mut u8>,
    nanbox_f64_slots: Vec<f64>,
    nanbox_u64_slots: Vec<u64>,
}

thread_local! {
    static TEST_FFI_MUTABLE_ROOTS: RefCell<TestFfiMutableRootSlots> =
        RefCell::new(TestFfiMutableRootSlots::default());
}

extern "C" fn test_ffi_mutable_root_scanner(visit: PerryFfiMutableRootVisitor, ctx: *mut c_void) {
    TEST_FFI_MUTABLE_ROOTS.with(|roots| {
        let mut roots = roots.borrow_mut();
        for slot in roots.i64_slots.iter_mut() {
            visit(
                PERRY_FFI_ROOT_SLOT_I64,
                slot as *mut i64 as *mut c_void,
                ctx,
            );
        }
        for slot in roots.usize_slots.iter_mut() {
            visit(
                PERRY_FFI_ROOT_SLOT_USIZE,
                slot as *mut usize as *mut c_void,
                ctx,
            );
        }
        for slot in roots.raw_ptr_slots.iter_mut() {
            visit(
                PERRY_FFI_ROOT_SLOT_RAW_MUT_PTR,
                slot as *mut *mut u8 as *mut c_void,
                ctx,
            );
        }
        for slot in roots.nanbox_f64_slots.iter_mut() {
            visit(
                PERRY_FFI_ROOT_SLOT_NANBOX_F64,
                slot as *mut f64 as *mut c_void,
                ctx,
            );
        }
        for slot in roots.nanbox_u64_slots.iter_mut() {
            visit(
                PERRY_FFI_ROOT_SLOT_NANBOX_U64,
                slot as *mut u64 as *mut c_void,
                ctx,
            );
        }
    });
}

struct TemporaryFfiMutableRootScanner {
    previous_len: usize,
    previous_roots: TestFfiMutableRootSlots,
}

impl TemporaryFfiMutableRootScanner {
    fn new(slots: TestFfiMutableRootSlots) -> Self {
        let previous_roots = TEST_FFI_MUTABLE_ROOTS.with(|roots| roots.replace(slots));
        let previous_len = FFI_MUTABLE_ROOT_SCANNERS.with(|scanners| {
            let mut scanners = scanners.borrow_mut();
            let previous_len = scanners.len();
            scanners.push(test_ffi_mutable_root_scanner);
            previous_len
        });
        Self {
            previous_len,
            previous_roots,
        }
    }
}

impl Drop for TemporaryFfiMutableRootScanner {
    fn drop(&mut self) {
        FFI_MUTABLE_ROOT_SCANNERS.with(|scanners| {
            scanners.borrow_mut().truncate(self.previous_len);
        });
        TEST_FFI_MUTABLE_ROOTS.with(|roots| {
            roots.replace(std::mem::take(&mut self.previous_roots));
        });
    }
}

fn young_leaf() -> usize {
    crate::arena::arena_alloc_gc(32, 8, GC_TYPE_STRING) as usize
}

fn ptr_bits(addr: usize) -> u64 {
    POINTER_TAG | (addr as u64 & POINTER_MASK)
}

fn string_bits(addr: usize) -> u64 {
    STRING_TAG | (addr as u64 & POINTER_MASK)
}

unsafe fn assert_string_bytes(ptr: *const crate::StringHeader, expected: &[u8]) {
    assert!(!ptr.is_null(), "expected non-null string pointer");
    assert_eq!((*ptr).byte_len as usize, expected.len());
    let data = (ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
    let bytes = std::slice::from_raw_parts(data, expected.len());
    assert_eq!(bytes, expected);
}

fn force_next_general_arena_alloc_slow() {
    const TEST_BLOCK_SIZE: usize = 1024 * 1024;
    let _ = crate::arena::arena_alloc(TEST_BLOCK_SIZE, 8);
}

fn old_page_dirty_for(page: usize) -> bool {
    crate::arena::old_page_meta_for_tests(page)
        .map(|meta| meta.dirty)
        .unwrap_or(false)
}

fn arena_block_index_for_user(user: usize) -> Option<usize> {
    let mut found = None;
    crate::arena::arena_walk_objects_with_block_index(|header_ptr, block_idx| {
        let current_user = unsafe { (header_ptr as *mut u8).add(GC_HEADER_SIZE) as usize };
        if current_user == user {
            found = Some(block_idx);
        }
    });
    found
}

extern "C" fn test_no_capture_singleton_func(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    0.0
}

extern "C" fn test_captured_singleton_func(_closure: *const crate::closure::ClosureHeader) -> f64 {
    0.0
}

unsafe fn init_test_closure(ptr: *mut u8) {
    let closure = ptr as *mut crate::closure::ClosureHeader;
    (*closure).func_ptr = std::ptr::null();
    (*closure).capture_count = 0;
    (*closure).type_tag = crate::closure::CLOSURE_MAGIC;
}

unsafe fn init_test_closure_with_one_capture(ptr: *mut u8, capture_bits: u64) -> *mut u64 {
    let closure = ptr as *mut crate::closure::ClosureHeader;
    (*closure).func_ptr = std::ptr::null();
    (*closure).capture_count = 1;
    (*closure).type_tag = crate::closure::CLOSURE_MAGIC;
    let capture_slot = ptr.add(std::mem::size_of::<crate::closure::ClosureHeader>()) as *mut u64;
    *capture_slot = capture_bits;
    layout_note_slot(ptr as usize, 0, capture_bits);
    capture_slot
}

#[inline(never)]
fn allocate_dead_malloc_churn_headers(per_type: usize) -> Vec<usize> {
    let mut headers = Vec::with_capacity(per_type * 3);
    for _ in 0..per_type {
        let ptr = gc_malloc(32, GC_TYPE_STRING);
        unsafe {
            std::ptr::write_bytes(ptr, 0xA5, 32);
            headers.push(header_from_user_ptr(ptr) as usize);
        }
    }
    for _ in 0..per_type {
        let ptr = gc_malloc(
            std::mem::size_of::<crate::closure::ClosureHeader>(),
            GC_TYPE_CLOSURE,
        );
        unsafe {
            init_test_closure(ptr);
            headers.push(header_from_user_ptr(ptr) as usize);
        }
    }
    for _ in 0..per_type {
        let ptr = gc_malloc(
            std::mem::size_of::<crate::promise::Promise>(),
            GC_TYPE_PROMISE,
        ) as *mut crate::promise::Promise;
        unsafe {
            std::ptr::write(
                ptr,
                crate::promise::Promise {
                    state: crate::promise::PromiseState::Pending,
                    value: 0.0,
                    reason: 0.0,
                    on_fulfilled: std::ptr::null(),
                    on_rejected: std::ptr::null(),
                    next: std::ptr::null_mut(),
                    async_id: 0,
                    trigger_async_id: 0,
                },
            );
            headers.push(header_from_user_ptr(ptr as *const u8) as usize);
        }
    }
    headers
}

fn tracked_malloc_headers_matching(headers: &[usize]) -> usize {
    MALLOC_STATE.with(|state| {
        let state = state.borrow();
        headers
            .iter()
            .filter(|&&addr| state.objects.iter().any(|&header| header as usize == addr))
            .count()
    })
}

fn malloc_kind_test_payload_size(obj_type: u8) -> usize {
    match obj_type {
        GC_TYPE_STRING => std::mem::size_of::<crate::string::StringHeader>() + 8,
        GC_TYPE_CLOSURE => std::mem::size_of::<crate::closure::ClosureHeader>(),
        GC_TYPE_PROMISE => std::mem::size_of::<crate::promise::Promise>(),
        GC_TYPE_BIGINT => std::mem::size_of::<crate::bigint::BigIntHeader>(),
        GC_TYPE_ERROR => std::mem::size_of::<crate::error::ErrorHeader>(),
        _ => 16,
    }
}

fn alloc_malloc_kind_test_object(obj_type: u8) -> *mut u8 {
    let ptr = gc_malloc(malloc_kind_test_payload_size(obj_type), obj_type);
    unsafe {
        match obj_type {
            GC_TYPE_STRING => {
                std::ptr::write(
                    ptr as *mut crate::string::StringHeader,
                    crate::string::StringHeader {
                        utf16_len: 0,
                        byte_len: 0,
                        capacity: 8,
                        refcount: 0,
                        flags: 0,
                    },
                );
            }
            GC_TYPE_CLOSURE => init_test_closure(ptr),
            GC_TYPE_PROMISE => {
                std::ptr::write(
                    ptr as *mut crate::promise::Promise,
                    crate::promise::Promise {
                        state: crate::promise::PromiseState::Pending,
                        value: 0.0,
                        reason: 0.0,
                        on_fulfilled: std::ptr::null(),
                        on_rejected: std::ptr::null(),
                        next: std::ptr::null_mut(),
                        async_id: 0,
                        trigger_async_id: 0,
                    },
                );
            }
            GC_TYPE_BIGINT => {
                std::ptr::write(
                    ptr as *mut crate::bigint::BigIntHeader,
                    crate::bigint::BigIntHeader {
                        limbs: [0; crate::bigint::BIGINT_LIMBS],
                    },
                );
            }
            GC_TYPE_ERROR => {
                std::ptr::write(
                    ptr as *mut crate::error::ErrorHeader,
                    crate::error::ErrorHeader {
                        object_type: crate::error::OBJECT_TYPE_ERROR,
                        error_kind: crate::error::ERROR_KIND_ERROR,
                        message: std::ptr::null_mut(),
                        name: std::ptr::null_mut(),
                        stack: std::ptr::null_mut(),
                        cause: 0.0,
                        errors: std::ptr::null_mut(),
                    },
                );
            }
            _ => {}
        }
    }
    ptr
}

#[test]
fn test_small_js_string_alloc_uses_managed_nursery_page() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let string = crate::string::js_string_from_bytes(b"managed-string".as_ptr(), 14);
    let header = unsafe { header_from_user_ptr(string as *const u8) };

    unsafe {
        assert_eq!((*header).obj_type, GC_TYPE_STRING);
        assert_ne!((*header).gc_flags & GC_FLAG_ARENA, 0);
    }
    assert_eq!(
        crate::arena::classify_heap_generation(string as usize),
        crate::arena::HeapGeneration::Nursery
    );
    assert!(
        !malloc_user_ptr_tracked(string as *mut u8),
        "ordinary heap strings should not be tracked in MALLOC_STATE"
    );
}

#[test]
fn test_small_js_closure_alloc_uses_managed_nursery_page() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let closure = crate::closure::js_closure_alloc(test_captured_singleton_func as *const u8, 2);
    let header = unsafe { header_from_user_ptr(closure as *const u8) };

    unsafe {
        assert_eq!((*header).obj_type, GC_TYPE_CLOSURE);
        assert_ne!((*header).gc_flags & GC_FLAG_ARENA, 0);
    }
    assert_eq!(
        crate::arena::classify_heap_generation(closure as usize),
        crate::arena::HeapGeneration::Nursery
    );
    assert!(
        !malloc_user_ptr_tracked(closure as *mut u8),
        "ordinary closures should not be tracked in MALLOC_STATE"
    );
}

#[test]
fn test_large_js_closure_alloc_remains_malloc_tracked() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let max_managed_captures = (LARGE_OBJECT_THRESHOLD_BYTES
        - GC_HEADER_SIZE
        - std::mem::size_of::<crate::closure::ClosureHeader>())
        / std::mem::size_of::<u64>();
    let closure = crate::closure::js_closure_alloc(
        test_captured_singleton_func as *const u8,
        (max_managed_captures + 1) as u32,
    );
    let header = unsafe { header_from_user_ptr(closure as *const u8) };

    unsafe {
        assert_eq!((*header).obj_type, GC_TYPE_CLOSURE);
        assert_eq!((*header).gc_flags & GC_FLAG_ARENA, 0);
    }
    assert!(
        malloc_user_ptr_tracked(closure as *mut u8),
        "large closure environments should keep the explicit gc_malloc path"
    );
}

#[test]
fn test_old_managed_closure_capture_write_dirties_old_page() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let child = young_leaf();
    let payload = std::mem::size_of::<crate::closure::ClosureHeader>() + std::mem::size_of::<u64>();
    let closure = crate::arena::arena_alloc_gc_old(
        payload,
        std::mem::align_of::<crate::closure::ClosureHeader>(),
        GC_TYPE_CLOSURE,
    ) as *mut crate::closure::ClosureHeader;
    unsafe {
        (*closure).func_ptr = test_captured_singleton_func as *const u8;
        (*closure).capture_count = 1;
        (*closure).type_tag = crate::closure::CLOSURE_MAGIC;
        layout_init_pointer_free(closure as *mut u8);
    }
    let slot = unsafe {
        (closure as *mut u8).add(std::mem::size_of::<crate::closure::ClosureHeader>()) as *mut u64
    };
    let page = crate::arena::generation_page_for_addr(slot as usize);
    crate::arena::old_page_clear_dirty(page);
    assert!(!old_page_dirty_for(page));

    crate::closure::js_closure_set_capture_f64(closure, 0, f64::from_bits(ptr_bits(child)));

    assert!(old_page_dirty_for(page));
    assert!(remembered_set_size() > 0);
}

#[test]
fn test_copying_minor_relocates_managed_closure_and_rewrites_capture() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let child = young_leaf();
    let closure = crate::closure::js_closure_alloc(test_captured_singleton_func as *const u8, 1);
    crate::closure::js_closure_set_capture_f64(closure, 0, f64::from_bits(ptr_bits(child)));
    js_shadow_slot_set(0, ptr_bits(closure as usize));

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    let closure_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    let capture_after_bits = unsafe {
        let slot = (closure_after as *const u8)
            .add(std::mem::size_of::<crate::closure::ClosureHeader>())
            as *const u64;
        *slot
    };
    let capture_after = (capture_after_bits & POINTER_MASK) as usize;

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_ne!(closure_after, closure as usize);
    assert_ne!(capture_after, child);
    assert!(crate::arena::pointer_in_nursery(closure_after));
    assert!(crate::arena::pointer_in_nursery(capture_after));
    assert!(
        trace.copying_nursery.copied_objects >= 2,
        "managed closure and captured child should both move"
    );
}

#[test]
fn test_copying_minor_preserves_dynamic_object_values_after_numeric_first_growth() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    let id_key = crate::string::js_string_from_bytes(b"id".as_ptr(), 2);
    let name_key = crate::string::js_string_from_bytes(b"name".as_ptr(), 4);
    let child_key = crate::string::js_string_from_bytes(b"child".as_ptr(), 5);
    let nested_key = crate::string::js_string_from_bytes(b"nested".as_ptr(), 6);

    let template = crate::object::js_object_alloc(0, 0);
    let template_name = crate::string::js_string_from_bytes(b"template".as_ptr(), 8);
    let template_child = crate::object::js_object_alloc(0, 0);
    crate::object::js_object_set_field_by_name(template, id_key, 1.0);
    crate::object::js_object_set_field_by_name(
        template,
        name_key,
        f64::from_bits(string_bits(template_name as usize)),
    );
    crate::object::js_object_set_field_by_name(
        template,
        child_key,
        f64::from_bits(ptr_bits(template_child as usize)),
    );

    let obj = crate::object::js_object_alloc(0, 0);
    let name_value = crate::string::js_string_from_bytes(b"roundtrip".as_ptr(), 9);
    let child = crate::object::js_object_alloc(0, 0);
    let nested_value = crate::string::js_string_from_bytes(b"retained".as_ptr(), 8);
    crate::object::js_object_set_field_by_name(
        child,
        nested_key,
        f64::from_bits(string_bits(nested_value as usize)),
    );
    crate::object::js_object_set_field_by_name(obj, id_key, 1.0);
    crate::object::js_object_set_field_by_name(
        obj,
        name_key,
        f64::from_bits(string_bits(name_value as usize)),
    );
    crate::object::js_object_set_field_by_name(
        obj,
        child_key,
        f64::from_bits(ptr_bits(child as usize)),
    );
    js_shadow_slot_set(0, ptr_bits(obj as usize));

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    let obj_after = (js_shadow_slot_get(0) & POINTER_MASK) as *const crate::object::ObjectHeader;

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_ne!(obj_after as usize, obj as usize);
    unsafe {
        let keys = (*obj_after).keys_array;
        assert!(!keys.is_null());
        assert_eq!(crate::array::js_array_length(keys), 3);
        let key0 = crate::array::js_array_get(keys, 0);
        let key1 = crate::array::js_array_get(keys, 1);
        let key2 = crate::array::js_array_get(keys, 2);
        assert!(key0.is_string());
        assert!(key1.is_string());
        assert!(key2.is_string());
        assert_string_bytes(key0.as_string_ptr(), b"id");
        assert_string_bytes(key1.as_string_ptr(), b"name");
        assert_string_bytes(key2.as_string_ptr(), b"child");
    }

    let id_lookup = crate::string::js_string_from_bytes(b"id".as_ptr(), 2);
    let name_lookup = crate::string::js_string_from_bytes(b"name".as_ptr(), 4);
    let child_lookup = crate::string::js_string_from_bytes(b"child".as_ptr(), 5);
    let nested_lookup = crate::string::js_string_from_bytes(b"nested".as_ptr(), 6);
    let id_value = crate::object::js_object_get_field_by_name(obj_after, id_lookup);
    let name_value = crate::object::js_object_get_field_by_name(obj_after, name_lookup);
    let child_value = crate::object::js_object_get_field_by_name(obj_after, child_lookup);

    assert_eq!(f64::from_bits(id_value.bits()), 1.0);
    assert!(name_value.is_string());
    unsafe {
        assert_string_bytes(name_value.as_string_ptr(), b"roundtrip");
    }
    assert!(child_value.is_pointer());
    let child_after = (child_value.bits() & POINTER_MASK) as *const crate::object::ObjectHeader;
    assert_ne!(child_after as usize, child as usize);
    let nested_after = crate::object::js_object_get_field_by_name(child_after, nested_lookup);
    assert!(nested_after.is_string());
    unsafe {
        assert_string_bytes(nested_after.as_string_ptr(), b"retained");
    }
}

#[test]
fn test_copying_minor_marks_array_growth_forwarding_target() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let stale_arr = crate::array::js_array_alloc(0);
    let mut current_arr = stale_arr;
    let mut first_closure = 0usize;

    for i in 0..50 {
        let child = young_leaf();
        let closure =
            crate::closure::js_closure_alloc(test_captured_singleton_func as *const u8, 1);
        crate::closure::js_closure_set_capture_f64(closure, 0, f64::from_bits(ptr_bits(child)));
        if i == 0 {
            first_closure = closure as usize;
        }
        current_arr = crate::array::js_array_push_f64(
            current_arr,
            f64::from_bits(ptr_bits(closure as usize)),
        );
    }

    assert_ne!(
        stale_arr, current_arr,
        "test setup should grow the array and leave a forwarding stub"
    );
    js_shadow_slot_set(0, ptr_bits(stale_arr as usize));

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    let arr_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    let first_value_bits =
        crate::array::js_array_get_f64(arr_after as *const crate::array::ArrayHeader, 0).to_bits();
    let closure_after = (first_value_bits & POINTER_MASK) as usize;
    let closure_header =
        unsafe { (closure_after as *const u8).sub(GC_HEADER_SIZE) as *const GcHeader };
    let capture_after_bits = unsafe {
        let closure = closure_after as *const crate::closure::ClosureHeader;
        assert_eq!((*closure).type_tag, crate::closure::CLOSURE_MAGIC);
        let slot = (closure as *const u8).add(std::mem::size_of::<crate::closure::ClosureHeader>())
            as *const u64;
        *slot
    };
    let capture_after = (capture_after_bits & POINTER_MASK) as usize;

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_ne!(arr_after, stale_arr as usize);
    assert_ne!(arr_after, current_arr as usize);
    assert_ne!(closure_after, first_closure);
    assert_eq!(unsafe { (*closure_header).obj_type }, GC_TYPE_CLOSURE);
    assert!(crate::arena::pointer_in_nursery(arr_after));
    assert!(crate::arena::pointer_in_nursery(closure_after));
    assert!(crate::arena::pointer_in_nursery(capture_after));
}

#[test]
fn test_malloc_kind_telemetry_sweep_by_kind() {
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_malloc_kind_telemetry_for_tests();
    let kinds = [
        GC_TYPE_STRING,
        GC_TYPE_CLOSURE,
        GC_TYPE_PROMISE,
        GC_TYPE_BIGINT,
        GC_TYPE_ERROR,
    ];
    let baselines: Vec<(u64, u64)> = kinds
        .iter()
        .map(|&kind| {
            let stats = malloc_kind_telemetry_for_tests(kind);
            (stats.survivor_count, stats.survivor_bytes)
        })
        .collect();

    let mut dead = Vec::new();
    let mut live = Vec::new();
    for &kind in &kinds {
        let dead_ptr = alloc_malloc_kind_test_object(kind);
        let live_ptr = alloc_malloc_kind_test_object(kind);
        unsafe {
            dead.push((kind, header_from_user_ptr(dead_ptr) as usize));
            live.push((kind, header_from_user_ptr(live_ptr) as usize));
        }
    }

    let dead_headers: Vec<usize> = dead.iter().map(|&(_, header)| header).collect();
    mark_existing_malloc_and_arena_objects_except(&dead_headers);
    let dead_bytes: Vec<u64> = dead
        .iter()
        .map(|&(_, header)| unsafe { (*(header as *mut GcHeader)).size as u64 })
        .collect();
    let live_bytes: Vec<u64> = live
        .iter()
        .map(|&(_, header)| unsafe { (*(header as *mut GcHeader)).size as u64 })
        .collect();

    let freed = sweep_malloc_objects();
    assert_eq!(
        freed,
        dead_bytes.iter().sum::<u64>(),
        "target sweep should reclaim only the intentionally-dead malloc objects"
    );

    for &(_, header) in &dead {
        assert!(
            !MALLOC_STATE.with(|s| s
                .borrow()
                .objects
                .iter()
                .any(|&tracked| tracked as usize == header)),
            "dead malloc header should be removed from tracking"
        );
    }
    for &(_, header) in &live {
        assert!(
            MALLOC_STATE.with(|s| s
                .borrow()
                .objects
                .iter()
                .any(|&tracked| tracked as usize == header)),
            "live malloc header should remain tracked"
        );
    }

    for (idx, &kind) in kinds.iter().enumerate() {
        let stats = malloc_kind_telemetry_for_tests(kind);
        assert_eq!(stats.allocated_count, 2, "{}", gc_type_name(kind));
        assert_eq!(
            stats.allocated_bytes,
            dead_bytes[idx] + live_bytes[idx],
            "{}",
            gc_type_name(kind)
        );
        assert_eq!(stats.freed_count, 1, "{}", gc_type_name(kind));
        assert_eq!(stats.freed_bytes, dead_bytes[idx], "{}", gc_type_name(kind));
        assert_eq!(
            stats.survivor_count,
            baselines[idx].0 + 1,
            "{}",
            gc_type_name(kind)
        );
        assert_eq!(
            stats.survivor_bytes,
            baselines[idx].1 + live_bytes[idx],
            "{}",
            gc_type_name(kind)
        );
    }
    clear_marks();
    clear_mark_seeds();
}

#[test]
fn test_malloc_kind_telemetry_batch_and_realloc() {
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_malloc_kind_telemetry_for_tests();
    let baseline = malloc_kind_telemetry_for_tests(GC_TYPE_STRING);
    let sizes = [8usize, 16, 24];
    let ptrs = gc_malloc_batch(&sizes, GC_TYPE_STRING);
    let old_total = unsafe { (*header_from_user_ptr(ptrs[1])).size as u64 };
    let new_ptr = gc_realloc(ptrs[1], 64);
    let new_total = unsafe { (*header_from_user_ptr(new_ptr)).size as u64 };
    let allocated_bytes = sizes
        .iter()
        .map(|size| (GC_HEADER_SIZE + size) as u64)
        .sum::<u64>();

    let stats = malloc_kind_telemetry_for_tests(GC_TYPE_STRING);
    assert_eq!(stats.allocated_count, sizes.len() as u64);
    assert_eq!(stats.allocated_bytes, allocated_bytes);
    assert_eq!(stats.realloc_count, 1);
    assert_eq!(stats.realloc_old_bytes, old_total);
    assert_eq!(stats.realloc_new_bytes, new_total);
    assert_eq!(
        stats.survivor_count,
        baseline.survivor_count + sizes.len() as u64
    );
    assert_eq!(
        stats.survivor_bytes,
        baseline
            .survivor_bytes
            .saturating_add(allocated_bytes)
            .saturating_sub(old_total)
            .saturating_add(new_total)
    );
    assert!(malloc_user_ptr_tracked(new_ptr));
}

#[test]
fn test_malloc_kind_telemetry_copied_minor_validation_by_kind() {
    let _guard = CopyingNurseryTestGuard::new(2);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let live_child = young_leaf();
    let live_malloc = gc_malloc(
        std::mem::size_of::<crate::closure::ClosureHeader>() + std::mem::size_of::<u64>(),
        GC_TYPE_CLOSURE,
    );
    let capture_slot =
        unsafe { init_test_closure_with_one_capture(live_malloc, ptr_bits(live_child)) };
    js_shadow_slot_set(0, ptr_bits(live_malloc as usize));
    let rejected_malloc_probe = (live_malloc as usize).saturating_add(16);
    js_shadow_slot_set(1, ptr_bits(rejected_malloc_probe));
    activate_malloc_registry_for_tests();

    let churn_headers = allocate_dead_malloc_churn_headers(128);
    reset_malloc_kind_telemetry_for_tests();
    trigger_guard.make_malloc_sweep_due();
    let outcome = gc_collect_minor_with_trigger(GcTriggerSnapshot {
        kind: GcTriggerKind::ArenaBytes,
        steps_before: Some(GcStepSnapshot::current()),
    });
    let trace = outcome.trace.expect("test requested GC trace capture");

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, true);
    assert!(
        trace.copying_nursery.malloc_validation_lookups > 0,
        "copied-minor should preserve the existing total malloc validation counter"
    );
    let closure_stats = malloc_kind_telemetry_for_tests(GC_TYPE_CLOSURE);
    assert!(
        closure_stats.copied_minor_validation_lookups > 0,
        "live malloc closure validation should be attributed to closure"
    );
    assert!(
        closure_stats.copied_minor_validation_lookups < churn_headers.len() as u64,
        "per-kind validation must scale with reachable malloc candidates, not dead churn"
    );
    let unknown_stats = malloc_kind_telemetry_for_tests(0);
    assert!(
        unknown_stats.copied_minor_validation_lookups > 0,
        "rejected copied-minor malloc validation probes should land in unknown"
    );
    assert_eq!(tracked_malloc_headers_matching(&churn_headers), 0);
    assert!(malloc_user_ptr_tracked(live_malloc));
    let capture_after = unsafe { (*capture_slot & POINTER_MASK) as usize };
    assert_ne!(capture_after, live_child);
    assert!(crate::arena::pointer_in_nursery(capture_after));
}

#[test]
fn test_malloc_kind_telemetry_trace_json() {
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_malloc_kind_telemetry_for_tests();
    let _ptr = gc_malloc(24, GC_TYPE_STRING);
    let trace = GcCycleTrace::new(
        GcCollectionKind::Minor,
        GcTriggerSnapshot {
            kind: GcTriggerKind::Direct,
            steps_before: Some(GcStepSnapshot::current()),
        },
    )
    .expect("test requested GC trace capture");

    let event = trace.into_json(GcStepSnapshot::current());
    let rows = event["malloc_kinds"]
        .as_array()
        .expect("malloc_kinds should be an array");
    assert_eq!(rows.len(), MALLOC_KIND_BUCKET_COUNT);
    for info in gc_type_infos() {
        let kind = info.type_id;
        let row = rows
            .iter()
            .find(|row| row["obj_type"].as_u64() == Some(kind as u64))
            .unwrap_or_else(|| panic!("missing malloc_kinds row for {}", gc_type_name(kind)));
        assert_eq!(row["kind"].as_str(), Some(gc_type_name(kind)));
        for field in [
            "allocated_count",
            "allocated_bytes",
            "realloc_count",
            "realloc_old_bytes",
            "realloc_new_bytes",
            "freed_count",
            "freed_bytes",
            "survivor_count",
            "survivor_bytes",
            "copied_minor_validation_lookups",
        ] {
            assert!(
                row.get(field).and_then(|value| value.as_u64()).is_some(),
                "missing numeric field {field} for {}",
                gc_type_name(kind)
            );
        }
    }
    let string_row = rows
        .iter()
        .find(|row| row["obj_type"].as_u64() == Some(GC_TYPE_STRING as u64))
        .expect("string row should be present");
    assert_eq!(string_row["allocated_count"].as_u64(), Some(1));
    assert_eq!(
        string_row["allocated_bytes"].as_u64(),
        Some((GC_HEADER_SIZE + 24) as u64)
    );
    let unknown_row = rows
        .iter()
        .find(|row| row["obj_type"].as_u64() == Some(0))
        .expect("unknown row should be present");
    assert_eq!(unknown_row["kind"].as_str(), Some("unknown"));
}

unsafe fn alloc_old_test_object(field_count: u32) -> (*mut crate::object::ObjectHeader, *mut u64) {
    let payload = std::mem::size_of::<crate::object::ObjectHeader>() + field_count as usize * 8;
    let obj = crate::arena::arena_alloc_gc_old(payload, 8, GC_TYPE_OBJECT)
        as *mut crate::object::ObjectHeader;
    (*obj).object_type = 1;
    (*obj).class_id = 0;
    (*obj).parent_class_id = 0;
    (*obj).field_count = field_count;
    (*obj).keys_array = std::ptr::null_mut();
    let fields =
        (obj as *mut u8).add(std::mem::size_of::<crate::object::ObjectHeader>()) as *mut u64;
    for i in 0..field_count as usize {
        *fields.add(i) = 0;
    }
    (obj, fields)
}

unsafe fn alloc_nursery_test_object(
    field_count: u32,
) -> (*mut crate::object::ObjectHeader, *mut u64) {
    let payload = std::mem::size_of::<crate::object::ObjectHeader>() + field_count as usize * 8;
    let obj = crate::arena::arena_alloc_gc(payload, 8, GC_TYPE_OBJECT)
        as *mut crate::object::ObjectHeader;
    (*obj).object_type = 1;
    (*obj).class_id = 0;
    (*obj).parent_class_id = 0;
    (*obj).field_count = field_count;
    (*obj).keys_array = std::ptr::null_mut();
    let fields =
        (obj as *mut u8).add(std::mem::size_of::<crate::object::ObjectHeader>()) as *mut u64;
    for i in 0..field_count as usize {
        *fields.add(i) = 0;
    }
    (obj, fields)
}

unsafe fn alloc_old_test_array(length: u32) -> (*mut crate::array::ArrayHeader, *mut u64) {
    let payload = std::mem::size_of::<crate::array::ArrayHeader>() + length as usize * 8;
    let arr = crate::arena::arena_alloc_gc_old(payload, 8, GC_TYPE_ARRAY)
        as *mut crate::array::ArrayHeader;
    (*arr).length = length;
    (*arr).capacity = length;
    let elements =
        (arr as *mut u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *mut u64;
    for i in 0..length as usize {
        *elements.add(i) = 0;
    }
    (arr, elements)
}

fn old_test_header_and_size(user: usize) -> (*mut GcHeader, usize) {
    let header = unsafe { header_from_user_ptr(user as *const u8) as *mut GcHeader };
    let total = unsafe { (*header).size as usize };
    (header, total)
}

#[test]
fn test_large_buffer_and_typed_array_enter_valid_pointer_set() {
    let _isolation = copying_nursery_isolation_lock();
    reset_remembered_set();
    clear_marks();
    clear_mark_seeds();

    let buffer = crate::buffer::buffer_alloc(LARGE_OBJECT_THRESHOLD_BYTES as u32) as usize;
    let typed_array = crate::typedarray::typed_array_alloc(
        crate::typedarray::KIND_UINT8,
        LARGE_OBJECT_THRESHOLD_BYTES as u32,
    ) as usize;
    assert!(crate::arena::pointer_in_old_gen(buffer));
    assert!(crate::arena::pointer_in_old_gen(typed_array));

    let valid_ptrs = build_valid_pointer_set();
    assert!(
        valid_ptrs.contains(&buffer),
        "large old Buffer must be in the valid pointer set"
    );
    assert!(
        valid_ptrs.contains(&typed_array),
        "large old TypedArray must be in the valid pointer set"
    );

    let buffer_data = buffer + std::mem::size_of::<crate::buffer::BufferHeader>();
    let typed_array_data = typed_array + std::mem::size_of::<crate::typedarray::TypedArrayHeader>();
    assert_eq!(valid_ptrs.enclosing_object(buffer_data), Some(buffer));
    assert_eq!(
        valid_ptrs.enclosing_object(typed_array_data),
        Some(typed_array)
    );

    clear_marks();
    remembered_set_clear();
}

#[test]
fn test_old_page_sweep_accounting_includes_large_buffer_and_typed_array() {
    let _isolation = copying_nursery_isolation_lock();
    reset_remembered_set();
    clear_marks();
    clear_mark_seeds();
    crate::arena::old_pages_begin_gc_cycle();

    let live_buffer = crate::buffer::buffer_alloc(LARGE_OBJECT_THRESHOLD_BYTES as u32) as usize;
    let dead_typed_array = crate::typedarray::typed_array_alloc(
        crate::typedarray::KIND_UINT8,
        LARGE_OBJECT_THRESHOLD_BYTES as u32,
    ) as usize;
    let (live_header, live_total) = old_test_header_and_size(live_buffer);
    let (_dead_header, dead_total) = old_test_header_and_size(dead_typed_array);
    unsafe {
        (*live_header).gc_flags |= GC_FLAG_MARKED;
    }

    let sweep = sweep_with_age_bump(false);
    let summary = crate::arena::old_page_summary();

    assert!(
        sweep.freed_bytes >= dead_total as u64,
        "dead old TypedArray should use the existing sweep dead decision"
    );
    assert_eq!(summary.live_bytes, live_total);
    assert_eq!(summary.dead_bytes, dead_total);
    assert_eq!(summary.reusable_bytes, 0);
    assert_eq!(summary.returned_bytes, 0);
    assert_eq!(summary.pinned_bytes, 0);
    assert_eq!(
        summary.live_object_count,
        crate::arena::old_object_page_overlaps(live_header as usize, live_total).len()
    );
    assert_eq!(
        summary.dead_object_count,
        crate::arena::old_object_page_overlaps(dead_typed_array - GC_HEADER_SIZE, dead_total,)
            .len()
    );

    clear_marks();
    remembered_set_clear();
}

#[test]
fn test_old_page_sweep_accounting_live_dead_fragmentation() {
    let _isolation = copying_nursery_isolation_lock();
    reset_remembered_set();
    clear_marks();
    clear_mark_seeds();
    crate::arena::old_pages_begin_gc_cycle();

    let live = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_STRING) as usize;
    let dead = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_STRING) as usize;
    let (live_header, live_total) = old_test_header_and_size(live);
    let (_dead_header, dead_total) = old_test_header_and_size(dead);
    unsafe {
        (*live_header).gc_flags |= GC_FLAG_MARKED;
    }

    let sweep = sweep_with_age_bump(false);
    let summary = crate::arena::old_page_summary();

    assert!(
        sweep.freed_bytes >= dead_total as u64,
        "dead old object should use the existing sweep dead decision"
    );
    assert_eq!(summary.live_bytes, live_total);
    assert_eq!(summary.dead_bytes, dead_total);
    assert_eq!(summary.reusable_bytes, 0);
    assert_eq!(summary.returned_bytes, 0);
    assert_eq!(summary.pinned_bytes, 0);
    assert_eq!(summary.live_object_count, 1);
    assert_eq!(summary.dead_object_count, 1);
    assert_eq!(summary.pinned_object_count, 0);
    assert_eq!(summary.fragmented_pages, 1);
    assert_eq!(summary.evacuation_eligible_pages, 1);
}

#[test]
fn test_old_page_reclamation_telemetry_dead_old_object_not_reusable_or_returned() {
    let _isolation = copying_nursery_isolation_lock();
    reset_remembered_set();
    clear_marks();
    clear_mark_seeds();
    crate::arena::old_pages_begin_gc_cycle();

    let dead = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_STRING) as usize;
    let (_dead_header, dead_total) = old_test_header_and_size(dead);

    let sweep = sweep_with_age_bump(false);
    let summary = crate::arena::old_page_summary();

    assert!(summary.dead_bytes >= dead_total);
    assert_eq!(summary.reusable_bytes, 0);
    assert_eq!(summary.returned_bytes, 0);
    assert!(sweep.dead_bytes >= dead_total as u64);
    assert_eq!(sweep.reusable_bytes, 0);
    assert_eq!(sweep.returned_bytes, 0);

    clear_marks();
    remembered_set_clear();
}

#[test]
fn test_old_page_reclamation_telemetry_dead_large_object_not_reusable_or_returned() {
    let _isolation = copying_nursery_isolation_lock();
    reset_remembered_set();
    clear_marks();
    clear_mark_seeds();
    crate::arena::old_pages_begin_gc_cycle();

    let dead_buffer = crate::buffer::buffer_alloc(LARGE_OBJECT_THRESHOLD_BYTES as u32) as usize;
    let (_dead_header, dead_total) = old_test_header_and_size(dead_buffer);

    let sweep = sweep_with_age_bump(false);
    let summary = crate::arena::old_page_summary();

    assert!(summary.dead_bytes >= dead_total);
    assert_eq!(summary.reusable_bytes, 0);
    assert_eq!(summary.returned_bytes, 0);
    assert!(sweep.dead_bytes >= dead_total as u64);
    assert_eq!(sweep.reusable_bytes, 0);
    assert_eq!(sweep.returned_bytes, 0);

    clear_marks();
    remembered_set_clear();
}

#[test]
fn test_full_sweep_reclaims_dead_old_block_and_clears_page_index() {
    let _isolation = copying_nursery_isolation_lock();
    reset_remembered_set();
    clear_marks();
    clear_mark_seeds();
    crate::arena::old_pages_begin_gc_cycle();

    let live = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_STRING) as usize;
    let dead = crate::arena::arena_alloc_gc_old(2 * 1024 * 1024, 8, GC_TYPE_STRING) as usize;
    let (live_header, _live_total) = old_test_header_and_size(live);
    let (dead_header, dead_total) = old_test_header_and_size(dead);
    let mut dead_pages = crate::fast_hash::new_ptr_hash_set();
    for (page, _) in crate::arena::old_object_page_overlaps(dead_header as usize, dead_total) {
        dead_pages.insert(page);
    }
    unsafe {
        (*live_header).gc_flags |= GC_FLAG_MARKED;
    }
    let old_before = crate::arena::old_gen_in_use_bytes();

    let sweep = sweep_with_age_bump_and_old_reclaim(false, true);
    let summary = crate::arena::old_page_summary();
    let old_after = crate::arena::old_gen_in_use_bytes();

    assert!(
        sweep.freed_bytes >= dead_total as u64,
        "dead old object should be swept before block reclaim"
    );
    assert!(
        old_after < old_before,
        "dead old block reset/deallocation should lower old in-use bytes"
    );
    assert!(
        sweep.reusable_bytes > 0 || sweep.returned_bytes > 0,
        "dead old block should be reset for reuse or returned"
    );
    assert!(
        summary.reusable_bytes > 0 || summary.returned_bytes > 0,
        "old-page summary should expose current-cycle reclaim telemetry"
    );
    assert_eq!(
        crate::arena::old_arena_walk_objects_on_pages(&dead_pages, |_| {}),
        0,
        "dead old block pages must not retain stale object-index entries"
    );
    for page in dead_pages {
        assert!(
            crate::arena::old_page_meta_for_tests(page).is_none(),
            "dead old block page metadata should be cleared"
        );
    }
    unsafe {
        assert_eq!((*live_header).obj_type, GC_TYPE_STRING);
        assert_eq!((*live_header).gc_flags & GC_FLAG_MARKED, 0);
    }
}

#[test]
fn test_old_page_sweep_accounting_pinned_is_live_and_not_evacuation_eligible() {
    let _isolation = copying_nursery_isolation_lock();
    reset_remembered_set();
    clear_marks();
    clear_mark_seeds();
    crate::arena::old_pages_begin_gc_cycle();

    let pinned = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_STRING) as usize;
    let (pinned_header, pinned_total) = old_test_header_and_size(pinned);
    unsafe {
        (*pinned_header).gc_flags |= GC_FLAG_PINNED;
    }

    let _sweep = sweep_with_age_bump(false);
    let summary = crate::arena::old_page_summary();

    assert_eq!(summary.live_bytes, pinned_total);
    assert_eq!(summary.dead_bytes, 0);
    assert_eq!(summary.pinned_bytes, pinned_total);
    assert_eq!(summary.live_object_count, 1);
    assert_eq!(summary.pinned_object_count, 1);
    assert_eq!(summary.evacuation_eligible_pages, 0);

    unsafe {
        (*pinned_header).gc_flags &= !GC_FLAG_PINNED;
    }
}

#[test]
fn test_old_page_sweep_accounting_spanning_object_distributes_bytes() {
    let _isolation = copying_nursery_isolation_lock();
    reset_remembered_set();
    clear_marks();
    clear_mark_seeds();
    crate::arena::old_pages_begin_gc_cycle();

    let user = crate::arena::arena_alloc_gc_old(4096 * 2 + 77, 8, GC_TYPE_STRING) as usize;
    let (header, total) = old_test_header_and_size(user);
    let overlaps = crate::arena::old_object_page_overlaps(header as usize, total);
    assert!(
        overlaps.len() > 1,
        "test object should span more than one old page"
    );
    unsafe {
        (*header).gc_flags |= GC_FLAG_MARKED;
    }

    let _sweep = sweep_with_age_bump(false);
    let summary = crate::arena::old_page_summary();

    assert_eq!(summary.live_bytes, total);
    assert_eq!(summary.dead_bytes, 0);
    assert_eq!(summary.live_object_count, overlaps.len());
    assert_eq!(summary.evacuation_eligible_pages, 0);
    for (page, bytes) in overlaps {
        let meta = crate::arena::old_page_meta_for_tests(page)
            .expect("spanned old page should have metadata");
        assert_eq!(meta.live_bytes, bytes);
        assert_eq!(meta.dead_bytes, 0);
        assert_eq!(meta.live_object_count, 1);
    }
}

#[test]
fn test_dirty_page_scan_accounts_old_page_dirty_slots() {
    let _isolation = copying_nursery_isolation_lock();
    reset_remembered_set();
    clear_marks();
    clear_mark_seeds();

    let young = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let (old_obj, fields) = unsafe { alloc_old_test_object(1) };
    unsafe {
        *fields = POINTER_TAG | young as u64;
    }
    js_write_barrier_slot(
        POINTER_TAG | old_obj as u64,
        fields as u64,
        POINTER_TAG | young as u64,
    );
    crate::arena::old_pages_begin_gc_cycle();

    let valid_ptrs = build_valid_pointer_set();
    let stats = mark_remembered_set_roots(&valid_ptrs);
    let dirty_page = crate::arena::generation_page_for_addr(fields as usize);
    let meta = crate::arena::old_page_meta_for_tests(dirty_page)
        .expect("dirty old page should have metadata");
    let summary = crate::arena::old_page_summary();

    assert!(
        stats.dirty_slots_scanned >= 1,
        "remembered scan should visit at least the written old slot"
    );
    assert!(
        meta.dirty_slots >= 1,
        "old page metadata should count scanned dirty slots"
    );
    assert_eq!(summary.dirty_pages, 1);
    assert_eq!(summary.dirty_slots, meta.dirty_slots);

    clear_marks();
    remembered_set_clear();
}

#[test]
fn test_old_page_sweep_accounting_trace_json_includes_summary() {
    let _isolation = copying_nursery_isolation_lock();
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_remembered_set();
    clear_marks();
    clear_mark_seeds();

    let pinned = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_STRING) as usize;
    let (pinned_header, pinned_total) = old_test_header_and_size(pinned);
    unsafe {
        (*pinned_header).gc_flags |= GC_FLAG_PINNED;
    }

    let outcome = gc_collect_minor_with_trigger(GcTriggerSnapshot {
        kind: GcTriggerKind::Direct,
        steps_before: Some(GcStepSnapshot::current()),
    });
    let trace = outcome.trace.expect("test requested GC trace capture");
    let event = trace.into_json(GcStepSnapshot::current());
    let old_pages = &event["old_pages"];

    assert!(old_pages["pages"].as_u64().unwrap_or(0) > 0);
    assert_eq!(old_pages["live_bytes"].as_u64(), Some(pinned_total as u64));
    assert_eq!(
        old_pages["pinned_bytes"].as_u64(),
        Some(pinned_total as u64)
    );
    assert_eq!(old_pages["dead_bytes"].as_u64(), Some(0));
    assert_eq!(old_pages["reusable_bytes"].as_u64(), Some(0));
    assert_eq!(old_pages["returned_bytes"].as_u64(), Some(0));
    assert_eq!(old_pages["pinned_object_count"].as_u64(), Some(1));
    assert_eq!(old_pages["evacuation_eligible_pages"].as_u64(), Some(0));
    assert!(event["sweep"]["dead_bytes"].as_u64().is_some());
    assert!(event["sweep"]["reusable_bytes"].as_u64().is_some());
    assert!(event["sweep"]["returned_bytes"].as_u64().is_some());
    assert_eq!(
        event["evacuation"]["released_original_reusable_bytes"].as_u64(),
        Some(0)
    );
    assert_eq!(
        event["evacuation"]["released_original_returned_bytes"].as_u64(),
        Some(0)
    );

    unsafe {
        (*pinned_header).gc_flags &= !GC_FLAG_PINNED;
    }
}

#[test]
fn test_old_page_defrag_policy_selection_prefers_fragmented_unpinned_pages() {
    fn meta(
        page_base: usize,
        allocated_bytes: usize,
        live_bytes: usize,
        dead_bytes: usize,
        pinned_bytes: usize,
    ) -> crate::arena::OldPageMeta {
        crate::arena::OldPageMeta {
            page_base,
            page_end: page_base + 4096,
            allocated_bytes,
            live_bytes,
            dead_bytes,
            object_count: 1,
            live_object_count: usize::from(live_bytes > 0),
            dead_object_count: usize::from(dead_bytes > 0),
            pinned_bytes,
            pinned_object_count: usize::from(pinned_bytes > 0),
            dirty_slots: 0,
            dirty: false,
            evacuation_eligible: false,
        }
    }

    let low_dead = meta(0x1000_0000, 100, 80, 20, 0);
    let high_dead = meta(0x1000_1000, 100, 10, 90, 0);
    let high_dead_more_live = meta(0x1000_2000, 100, 20, 80, 0);
    let pinned = meta(0x1000_3000, 100, 10, 90, 8);
    let empty = meta(0x1000_4000, 0, 0, 0, 0);
    let snapshot = [low_dead, high_dead_more_live, pinned, empty, high_dead];

    let selection = select_old_page_defrag_pages_from_snapshot(&snapshot, false);
    let high_dead_page = crate::arena::generation_page_for_addr(high_dead.page_base);
    let high_dead_more_live_page =
        crate::arena::generation_page_for_addr(high_dead_more_live.page_base);
    let low_dead_page = crate::arena::generation_page_for_addr(low_dead.page_base);

    assert_eq!(selection.candidate_pages, 3);
    assert_eq!(selection.selected_pages, 2);
    assert_eq!(selection.selected_live_bytes, 30);
    assert_eq!(selection.selected_reclaimable_bytes, 170);
    assert_eq!(selection.skipped_pinned_pages, 1);
    assert!(selection.pages.contains(&high_dead_page));
    assert!(selection.pages.contains(&high_dead_more_live_page));
    assert!(!selection.pages.contains(&low_dead_page));
    assert_eq!(
        selection.page_order,
        vec![high_dead_page, high_dead_more_live_page],
        "selected pages should be ordered by highest dead ratio, then lowest live bytes"
    );

    let forced = select_old_page_defrag_pages_from_snapshot(&snapshot, true);
    assert_eq!(forced.selected_pages, 3);
    assert!(forced.pages.contains(&low_dead_page));
    assert_eq!(forced.skipped_pinned_pages, 1);
}

#[test]
fn test_old_page_defrag_forced_moves_only_marked_old_objects_on_selected_pages() {
    let _isolation = copying_nursery_isolation_lock();
    reset_remembered_set();
    clear_marks();
    clear_mark_seeds();
    CONS_PINNED.with(|s| s.borrow_mut().clear());

    let movable = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT) as usize;
    let unmarked = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT) as usize;
    let (movable_header, movable_total) = old_test_header_and_size(movable);
    let (unmarked_header, _) = old_test_header_and_size(unmarked);
    let mut selected_pages = crate::fast_hash::new_ptr_hash_set();
    for (page, _) in crate::arena::old_object_page_overlaps(movable_header as usize, movable_total)
    {
        selected_pages.insert(page);
    }
    unsafe {
        (*movable_header).gc_flags |= GC_FLAG_MARKED;
    }

    let mut new_headers = Vec::new();
    let mut original_headers = Vec::new();
    let moved = evacuate_selected_old_pages_collecting(
        &selected_pages,
        &mut new_headers,
        &mut original_headers,
    );

    assert_eq!(moved.old_page_moved_objects, 1);
    assert_eq!(moved.old_page_moved_bytes, movable_total);
    assert_eq!(new_headers.len(), 1);
    assert_eq!(original_headers, vec![movable_header]);
    assert!(
        old_object_pages_disjoint_from_selected(new_headers[0], movable_total, &selected_pages),
        "old-page copy must not land in any selected source page"
    );
    unsafe {
        assert_ne!((*movable_header).gc_flags & GC_FLAG_FORWARDED, 0);
        assert_eq!(
            (*unmarked_header).gc_flags & GC_FLAG_FORWARDED,
            0,
            "unmarked old object on the selected page must not move"
        );
        assert!(crate::arena::pointer_in_old_gen(
            forwarding_address(movable_header) as usize
        ));
    }

    let released = release_evacuated_original_forwarding_stubs(&original_headers);
    assert_eq!(released.released_original_objects, 1);
    assert_eq!(released.released_original_reusable_bytes, 0);
    assert_eq!(released.released_original_returned_bytes, 0);
    clear_marks();
    CONS_PINNED.with(|s| s.borrow_mut().clear());
}

#[test]
fn test_old_page_defrag_copy_avoids_selected_pages_and_rebuilds_remembered_set() {
    let _isolation = copying_nursery_isolation_lock();
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    reset_remembered_set();
    clear_marks();
    clear_mark_seeds();
    CONS_PINNED.with(|s| s.borrow_mut().clear());

    let (parent, fields) = unsafe { alloc_old_test_object(1) };
    let parent_user = parent as usize;
    let parent_header = unsafe { header_from_user_ptr(parent as *const u8) };
    let parent_total = unsafe { (*parent_header).size as usize };
    let child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let child_header = unsafe { header_from_user_ptr(child as *const u8) };
    let mut selected_pages = crate::fast_hash::new_ptr_hash_set();
    for (page, _) in crate::arena::old_object_page_overlaps(parent_header as usize, parent_total) {
        selected_pages.insert(page);
    }
    unsafe {
        *fields = ptr_bits(child);
        (*parent_header).gc_flags |= GC_FLAG_MARKED;
    }
    js_write_barrier_slot(ptr_bits(parent_user), fields as u64, ptr_bits(child));

    let mut new_headers = Vec::new();
    let mut original_headers = Vec::new();
    let moved = evacuate_selected_old_pages_collecting(
        &selected_pages,
        &mut new_headers,
        &mut original_headers,
    );

    assert_eq!(moved.old_page_moved_objects, 1);
    assert_eq!(new_headers.len(), 1);
    assert!(
        old_object_pages_disjoint_from_selected(new_headers[0], parent_total, &selected_pages),
        "forwarded old-page copy must land outside all selected source pages"
    );
    unsafe {
        let forwarded_page =
            crate::arena::generation_page_for_addr(forwarding_address(parent_header) as usize);
        assert!(
            !selected_pages.contains(&forwarded_page),
            "forwarded address page must not be a selected source page"
        );
    }

    let sticky = rebuild_evacuated_old_to_young_remembered_set(&new_headers);
    remembered_set_clear();
    sticky.restore();
    let released = release_evacuated_original_forwarding_stubs(&original_headers);
    assert_eq!(released.released_original_objects, 1);
    assert!(
        remembered_set_size() > 0,
        "rebuilt remembered set should keep the evacuated old-to-young edge dirty"
    );

    clear_marks();
    let valid_ptrs = build_valid_pointer_set();
    let stats = mark_remembered_set_roots(&valid_ptrs);
    assert!(
        stats.newly_marked > 0,
        "rebuilt remembered set should mark the young child"
    );
    unsafe {
        assert_ne!(
            (*child_header).gc_flags & GC_FLAG_MARKED,
            0,
            "young child should remain reachable through the moved old parent"
        );
    }

    clear_marks();
    remembered_set_clear();
    CONS_PINNED.with(|s| s.borrow_mut().clear());
}

#[test]
fn test_old_page_defrag_skips_pinned_old_objects() {
    let _isolation = copying_nursery_isolation_lock();
    reset_remembered_set();
    clear_marks();
    clear_mark_seeds();
    CONS_PINNED.with(|s| s.borrow_mut().clear());

    let pinned = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT) as usize;
    let (pinned_header, pinned_total) = old_test_header_and_size(pinned);
    let mut selected_pages = crate::fast_hash::new_ptr_hash_set();
    for (page, _) in crate::arena::old_object_page_overlaps(pinned_header as usize, pinned_total) {
        selected_pages.insert(page);
    }
    unsafe {
        (*pinned_header).gc_flags |= GC_FLAG_MARKED | GC_FLAG_PINNED;
    }

    let mut new_headers = Vec::new();
    let mut original_headers = Vec::new();
    let moved = evacuate_selected_old_pages_collecting(
        &selected_pages,
        &mut new_headers,
        &mut original_headers,
    );

    assert_eq!(moved.old_page_moved_objects, 0);
    assert!(new_headers.is_empty());
    assert!(original_headers.is_empty());
    unsafe {
        assert_eq!(
            (*pinned_header).gc_flags & GC_FLAG_FORWARDED,
            0,
            "pinned old object address must remain stable"
        );
        (*pinned_header).gc_flags &= !(GC_FLAG_MARKED | GC_FLAG_PINNED);
    }
    CONS_PINNED.with(|s| s.borrow_mut().clear());
}

#[test]
fn test_old_page_defrag_skips_non_movable_buffer_and_typed_array() {
    let _isolation = copying_nursery_isolation_lock();
    reset_remembered_set();
    clear_marks();
    clear_mark_seeds();
    CONS_PINNED.with(|s| s.borrow_mut().clear());

    let buffer = crate::buffer::buffer_alloc(LARGE_OBJECT_THRESHOLD_BYTES as u32) as usize;
    let typed_array = crate::typedarray::typed_array_alloc(
        crate::typedarray::KIND_UINT8,
        LARGE_OBJECT_THRESHOLD_BYTES as u32,
    ) as usize;
    let (buffer_header, buffer_total) = old_test_header_and_size(buffer);
    let (typed_array_header, typed_array_total) = old_test_header_and_size(typed_array);
    let mut selected_pages = crate::fast_hash::new_ptr_hash_set();
    for (page, _) in crate::arena::old_object_page_overlaps(buffer_header as usize, buffer_total) {
        selected_pages.insert(page);
    }
    for (page, _) in
        crate::arena::old_object_page_overlaps(typed_array_header as usize, typed_array_total)
    {
        selected_pages.insert(page);
    }
    unsafe {
        (*buffer_header).gc_flags |= GC_FLAG_MARKED;
        (*typed_array_header).gc_flags |= GC_FLAG_MARKED;
    }

    let mut new_headers = Vec::new();
    let mut original_headers = Vec::new();
    let moved = evacuate_selected_old_pages_collecting(
        &selected_pages,
        &mut new_headers,
        &mut original_headers,
    );

    assert_eq!(moved.old_page_moved_objects, 0);
    assert_eq!(moved.old_page_moved_bytes, 0);
    assert!(new_headers.is_empty());
    assert!(original_headers.is_empty());
    unsafe {
        assert_eq!(
            (*buffer_header).gc_flags & GC_FLAG_FORWARDED,
            0,
            "old Buffer address must remain stable"
        );
        assert_eq!(
            (*typed_array_header).gc_flags & GC_FLAG_FORWARDED,
            0,
            "old TypedArray address must remain stable"
        );
        (*buffer_header).gc_flags &= !GC_FLAG_MARKED;
        (*typed_array_header).gc_flags &= !GC_FLAG_MARKED;
    }
    CONS_PINNED.with(|s| s.borrow_mut().clear());
}

#[test]
fn test_old_page_defrag_re_remembers_young_child_after_collection_clear() {
    struct ResetGcTestState;

    impl Drop for ResetGcTestState {
        fn drop(&mut self) {
            reset_shadow_stack();
            reset_global_roots();
            reset_remembered_set();
            clear_marks();
            clear_mark_seeds();
            CONS_PINNED.with(|s| s.borrow_mut().clear());
        }
    }

    let _reset = ResetGcTestState;
    let _isolation = copying_nursery_isolation_lock();
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = EnvVarGuard::set("PERRY_GC_FORCE_EVACUATE", "1");
    let _barrier_guard = GeneratedWriteBarrierTestGuard::active();
    reset_shadow_stack();
    reset_global_roots();
    reset_remembered_set();
    clear_marks();
    clear_mark_seeds();
    CONS_PINNED.with(|s| s.borrow_mut().clear());

    let (parent, fields) = unsafe { alloc_old_test_object(1) };
    let parent_user = parent as usize;
    let parent_header = unsafe { header_from_user_ptr(parent as *const u8) };
    let _dead = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_STRING) as usize;
    unsafe {
        (*parent_header).gc_flags |= GC_FLAG_MARKED;
    }
    let _ = sweep_with_age_bump(false);

    let frame = js_shadow_frame_push(1);
    let child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let child_header = unsafe { header_from_user_ptr(child as *const u8) };
    let _copy_only_root_guard = TemporaryCopyOnlyRootScanner::rust_bits(&[ptr_bits(child)]);
    unsafe {
        *fields = ptr_bits(child);
    }
    js_write_barrier_slot(ptr_bits(parent_user), fields as u64, ptr_bits(child));
    js_shadow_slot_set(0, ptr_bits(parent_user));

    let trace = collect_minor_trace(GcTriggerKind::Direct);

    assert!(
        trace.evacuation.old_page_moved_objects >= 1,
        "forced old-page defrag should move the rooted old parent"
    );
    let parent_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(parent_after, parent_user);
    assert!(crate::arena::pointer_in_old_gen(parent_after));
    assert!(
        remembered_set_size() > 0,
        "moved old parent retaining a young child must be re-remembered after clear"
    );

    clear_marks();
    let valid_ptrs = build_valid_pointer_set();
    let stats = mark_remembered_set_roots(&valid_ptrs);
    assert!(stats.newly_marked > 0);
    unsafe {
        assert_ne!(
            (*child_header).gc_flags & GC_FLAG_MARKED,
            0,
            "rebuilt remembered set should mark the young child"
        );
    }

    js_shadow_frame_pop(frame);
}

#[test]
fn test_old_page_defrag_target_gate_emits_trace() {
    struct ResetGcTestState;

    impl Drop for ResetGcTestState {
        fn drop(&mut self) {
            reset_shadow_stack();
            reset_global_roots();
            reset_remembered_set();
            clear_marks();
            clear_mark_seeds();
            CONS_PINNED.with(|s| s.borrow_mut().clear());
        }
    }

    let _reset = ResetGcTestState;
    let _isolation = copying_nursery_isolation_lock();
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _barrier_guard = GeneratedWriteBarrierTestGuard::active();
    reset_shadow_stack();
    reset_global_roots();
    reset_remembered_set();
    clear_marks();
    clear_mark_seeds();
    CONS_PINNED.with(|s| s.borrow_mut().clear());
    if !gc_force_evacuate_enabled() {
        return;
    }

    let (parent, fields) = unsafe { alloc_old_test_object(1) };
    let parent_user = parent as usize;
    let parent_header = unsafe { header_from_user_ptr(parent as *const u8) };
    let _dead = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_STRING) as usize;
    unsafe {
        (*parent_header).gc_flags |= GC_FLAG_MARKED;
    }
    let _ = sweep_with_age_bump(false);

    let frame = js_shadow_frame_push(1);
    let child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let child_header = unsafe { header_from_user_ptr(child as *const u8) };
    let _copy_only_root_guard = TemporaryCopyOnlyRootScanner::rust_bits(&[ptr_bits(child)]);
    unsafe {
        *fields = ptr_bits(child);
    }
    js_write_barrier_slot(ptr_bits(parent_user), fields as u64, ptr_bits(child));
    js_shadow_slot_set(0, ptr_bits(parent_user));

    let _ = gc_collect_minor();

    let parent_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(
        parent_after, parent_user,
        "forced old-page defrag should rewrite the shadow root to the moved parent"
    );
    assert!(crate::arena::pointer_in_old_gen(parent_after));
    assert!(
        remembered_set_size() > 0,
        "moved old parent retaining a young child must be re-remembered"
    );

    clear_marks();
    let valid_ptrs = build_valid_pointer_set();
    let stats = mark_remembered_set_roots(&valid_ptrs);
    assert!(stats.newly_marked > 0);
    unsafe {
        assert_ne!((*child_header).gc_flags & GC_FLAG_MARKED, 0);
    }

    js_shadow_frame_pop(frame);
}

#[test]
fn test_old_page_defrag_trace_json_distinguishes_moved_from_reclaimable() {
    let mut trace = GcCycleTrace::new(
        GcCollectionKind::Minor,
        GcTriggerSnapshot {
            kind: GcTriggerKind::Direct,
            steps_before: Some(GcStepSnapshot::current()),
        },
    )
    .expect("test requested GC trace capture");
    trace.evacuation_policy.snapshot.old_page_candidate_pages = 2;
    trace.evacuation_policy.snapshot.old_page_selected_pages = 1;
    trace
        .evacuation_policy
        .snapshot
        .old_page_selected_live_bytes = 64;
    trace.evacuation_policy.snapshot.old_page_reclaimable_bytes = 192;
    trace.evacuation.old_page_moved_objects = 1;
    trace.evacuation.old_page_moved_bytes = 64;
    trace.evacuation.released_original_objects = 1;
    trace.evacuation.released_original_bytes = 64;
    trace.sweep.dead_bytes = 192;
    trace.sweep.freed_bytes = 192;
    trace.sweep.reusable_bytes = 128;
    trace.sweep.returned_bytes = 32;
    trace.sweep.deallocated_bytes = 32;

    let event = trace.into_json(GcStepSnapshot::current());

    assert_eq!(
        event["evacuation_policy"]["old_page_reclaimable_bytes"].as_u64(),
        Some(192)
    );
    assert_eq!(
        event["evacuation_policy"]["old_page_selected_live_bytes"].as_u64(),
        Some(64)
    );
    assert_eq!(
        event["evacuation"]["old_page_moved_bytes"].as_u64(),
        Some(64)
    );
    assert_eq!(
        event["evacuation"]["released_original_bytes"].as_u64(),
        Some(64)
    );
    assert_eq!(
        event["evacuation"]["released_original_reusable_bytes"].as_u64(),
        Some(0)
    );
    assert_eq!(
        event["evacuation"]["released_original_returned_bytes"].as_u64(),
        Some(0)
    );
    assert_eq!(event["sweep"]["dead_bytes"].as_u64(), Some(192));
    assert_eq!(event["sweep"]["freed_bytes"].as_u64(), Some(192));
    assert_eq!(event["sweep"]["reusable_bytes"].as_u64(), Some(128));
    assert_eq!(event["sweep"]["returned_bytes"].as_u64(), Some(32));
    assert_eq!(event["sweep"]["deallocated_bytes"].as_u64(), Some(32));
}

#[test]
fn test_copied_minor_eligibility_falls_back_for_barriers_inactive() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _barrier_guard = GeneratedWriteBarrierTestGuard::inactive();

    let trace = collect_minor_trace(GcTriggerKind::Direct);

    assert_copied_minor_trace(
        &trace,
        false,
        CopiedMinorFallbackReason::BarriersInactive,
        false,
    );
}

#[test]
fn test_copied_minor_eligibility_falls_back_for_conservative_stack_scan() {
    let _isolation = copying_nursery_isolation_lock();
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _barrier_guard = GeneratedWriteBarrierTestGuard::active();
    reset_shadow_stack();
    reset_global_roots();
    reset_remembered_set();

    let trace = collect_minor_trace(GcTriggerKind::Direct);

    assert_copied_minor_trace(
        &trace,
        false,
        CopiedMinorFallbackReason::ConservativeStack,
        false,
    );
}

#[test]
fn test_copied_minor_eligibility_active_shadow_frame_skips_conservative_stack_scan() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    assert!(shadow_stack_has_active_frame());

    let trace = collect_minor_trace(GcTriggerKind::Direct);

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_eq!(trace.conservative_root_count, 0);
    assert_eq!(trace.conservative_pinned, 0);
    assert_eq!(trace.conservative_pinned_bytes, 0);
    assert_eq!(trace.legacy_copy_only_scanner_pinned.pinned_bytes, 0);
}

#[test]
fn test_copied_minor_eligibility_empty_copy_only_scanner_stays_eligible() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _copy_only_root_guard = TemporaryCopyOnlyRootScanner::rust_bits(&[]);

    let trace = collect_minor_trace(GcTriggerKind::Direct);

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_eq!(
        trace
            .legacy_copy_only_scanner_pinned
            .registered_rust_scanners,
        1
    );
    assert_eq!(trace.legacy_copy_only_scanner_pinned.emitted_roots, 0);
}

#[test]
fn test_copied_minor_eligibility_falls_back_for_live_young_rust_copy_only_root() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let child = young_leaf();
    let _copy_only_root_guard = TemporaryCopyOnlyRootScanner::rust_bits(&[ptr_bits(child)]);

    let trace = collect_minor_trace(GcTriggerKind::Direct);

    assert_copied_minor_trace(
        &trace,
        false,
        CopiedMinorFallbackReason::CopyOnlyRoots,
        false,
    );
    assert_eq!(
        trace
            .legacy_copy_only_scanner_pinned
            .registered_rust_scanners,
        1
    );
    assert_eq!(trace.legacy_copy_only_scanner_pinned.emitted_roots, 1);
    assert_eq!(trace.legacy_copy_only_scanner_pinned.emitted_young_roots, 1);
}

#[test]
fn test_copied_minor_eligibility_falls_back_for_live_young_ffi_copy_only_root() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let child = young_leaf();
    let _copy_only_root_guard = TemporaryCopyOnlyRootScanner::ffi_bits(&[ptr_bits(child)]);

    let trace = collect_minor_trace(GcTriggerKind::Direct);

    assert_copied_minor_trace(
        &trace,
        false,
        CopiedMinorFallbackReason::CopyOnlyRoots,
        false,
    );
    assert_eq!(
        trace
            .legacy_copy_only_scanner_pinned
            .registered_ffi_scanners,
        1
    );
    assert_eq!(trace.legacy_copy_only_scanner_pinned.emitted_roots, 1);
    assert_eq!(trace.legacy_copy_only_scanner_pinned.emitted_young_roots, 1);
}

#[test]
fn test_ffi_mutable_i64_root_is_copied_without_copy_only_fallback() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let child = young_leaf();
    let _mutable_root_guard = TemporaryFfiMutableRootScanner::new(TestFfiMutableRootSlots {
        i64_slots: vec![child as i64],
        ..TestFfiMutableRootSlots::default()
    });

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    let after = TEST_FFI_MUTABLE_ROOTS.with(|roots| roots.borrow().i64_slots[0] as usize);

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_ne!(after, child);
    assert!(crate::arena::pointer_in_nursery(after));
    assert_eq!(
        trace
            .legacy_copy_only_scanner_pinned
            .registered_ffi_scanners,
        0
    );
    assert_eq!(trace.legacy_copy_only_scanner_pinned.emitted_roots, 0);
}

#[test]
fn test_ffi_mutable_active_registry_malloc_root_does_not_report_copy_only_roots() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let live_malloc = gc_malloc(
        std::mem::size_of::<crate::closure::ClosureHeader>(),
        GC_TYPE_CLOSURE,
    );
    unsafe {
        init_test_closure(live_malloc);
    }
    activate_malloc_registry_for_tests();
    let _mutable_root_guard = TemporaryFfiMutableRootScanner::new(TestFfiMutableRootSlots {
        raw_ptr_slots: vec![live_malloc],
        ..TestFfiMutableRootSlots::default()
    });

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    let after = TEST_FFI_MUTABLE_ROOTS.with(|roots| roots.borrow().raw_ptr_slots[0]);

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_eq!(after, live_malloc);
    assert!(trace.copying_nursery.malloc_validation_lookups > 0);
    assert_eq!(
        trace
            .legacy_copy_only_scanner_pinned
            .registered_ffi_scanners,
        0
    );
    assert_eq!(trace.legacy_copy_only_scanner_pinned.emitted_roots, 0);
}

#[test]
fn test_ffi_mutable_trampoline_visits_all_slot_kinds() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let i64_root = young_leaf();
    let usize_root = young_leaf();
    let raw_root = young_leaf();
    let f64_root = young_leaf();
    let u64_root = young_leaf();
    let _mutable_root_guard = TemporaryFfiMutableRootScanner::new(TestFfiMutableRootSlots {
        i64_slots: vec![i64_root as i64],
        usize_slots: vec![usize_root],
        raw_ptr_slots: vec![raw_root as *mut u8],
        nanbox_f64_slots: vec![f64::from_bits(ptr_bits(f64_root))],
        nanbox_u64_slots: vec![ptr_bits(u64_root)],
    });

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    let (i64_after, usize_after, raw_after, f64_after, u64_after) =
        TEST_FFI_MUTABLE_ROOTS.with(|roots| {
            let roots = roots.borrow();
            (
                roots.i64_slots[0] as usize,
                roots.usize_slots[0],
                roots.raw_ptr_slots[0] as usize,
                (roots.nanbox_f64_slots[0].to_bits() & POINTER_MASK) as usize,
                (roots.nanbox_u64_slots[0] & POINTER_MASK) as usize,
            )
        });

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    for (before, after) in [
        (i64_root, i64_after),
        (usize_root, usize_after),
        (raw_root, raw_after),
        (f64_root, f64_after),
        (u64_root, u64_after),
    ] {
        assert_ne!(after, before);
        assert!(crate::arena::pointer_in_nursery(after));
    }
    assert_eq!(trace.legacy_copy_only_scanner_pinned.emitted_roots, 0);
}

#[test]
fn test_copied_minor_eligibility_old_only_copy_only_root_stays_eligible() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let old = crate::arena::arena_alloc_gc_old(32, 8, GC_TYPE_OBJECT) as usize;
    let _copy_only_root_guard = TemporaryCopyOnlyRootScanner::rust_bits(&[ptr_bits(old)]);

    let trace = collect_minor_trace(GcTriggerKind::Direct);

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_eq!(trace.legacy_copy_only_scanner_pinned.emitted_roots, 1);
    assert_eq!(trace.legacy_copy_only_scanner_pinned.emitted_old_roots, 1);
    assert_eq!(trace.legacy_copy_only_scanner_pinned.emitted_young_roots, 0);
}

#[test]
fn test_copied_minor_eligibility_malformed_copy_only_root_stays_eligible() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _copy_only_root_guard = TemporaryCopyOnlyRootScanner::rust_bits(&[0x7FFD_0000_0000_1000]);

    let trace = collect_minor_trace(GcTriggerKind::Direct);

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_eq!(trace.legacy_copy_only_scanner_pinned.emitted_roots, 1);
    assert_eq!(trace.legacy_copy_only_scanner_pinned.malformed_roots, 1);
}

#[test]
fn test_copied_minor_eligibility_falls_back_for_malloc_copy_only_root() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let live_malloc = gc_malloc(
        std::mem::size_of::<crate::closure::ClosureHeader>(),
        GC_TYPE_CLOSURE,
    );
    unsafe {
        init_test_closure(live_malloc);
    }
    let _copy_only_root_guard =
        TemporaryCopyOnlyRootScanner::rust_bits(&[ptr_bits(live_malloc as usize)]);

    let trace = collect_minor_trace(GcTriggerKind::Direct);

    assert_copied_minor_trace(
        &trace,
        false,
        CopiedMinorFallbackReason::CopyOnlyRoots,
        false,
    );
    assert_eq!(trace.legacy_copy_only_scanner_pinned.emitted_roots, 1);
    assert_eq!(
        trace.legacy_copy_only_scanner_pinned.emitted_malloc_roots,
        1
    );
}

#[test]
fn test_copying_minor_rewrites_shadow_and_global_roots() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let shadow_child = young_leaf();
    let global_child = young_leaf();
    let mut global_slot = global_child as u64;
    js_shadow_slot_set(0, ptr_bits(shadow_child));
    js_gc_register_global_root(&mut global_slot as *mut u64 as i64);

    let _ = gc_collect_minor();
    let shadow_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    let global_after = global_slot as usize;

    assert_ne!(shadow_after, shadow_child);
    assert_ne!(global_after, global_child);
    assert!(crate::arena::pointer_in_nursery(shadow_after));
    assert!(crate::arena::pointer_in_nursery(global_after));
    assert_eq!(
        crate::arena::classify_heap_space(shadow_after),
        crate::arena::active_survivor_space()
    );
}

#[test]
fn test_copying_minor_ignores_cleared_dead_shadow_slot_but_preserves_live_slot() {
    let _guard = CopyingNurseryTestGuard::new(2);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let dead = young_leaf();
    let live = young_leaf();
    js_shadow_slot_set(0, ptr_bits(dead));
    js_shadow_slot_set(0, 0);
    js_shadow_slot_set(1, ptr_bits(live));

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    let live_after = (js_shadow_slot_get(1) & POINTER_MASK) as usize;

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_eq!(js_shadow_slot_get(0), 0);
    assert_ne!(live_after, live);
    assert!(crate::arena::pointer_in_nursery(live_after));
    assert_eq!(trace.copying_nursery.copied_objects, 1);
    assert_eq!(trace.copying_nursery.promoted_objects, 0);
    assert_eq!(trace.shadow_roots.slots_scanned, 2);
    assert_eq!(trace.shadow_roots.nonzero_slots, 1);
    assert_eq!(trace.shadow_roots.pointer_roots, 1);
    assert_eq!(trace.shadow_roots.rewritten_slots, 1);
}

#[test]
fn large_object_copying_minor_excludes_rooted_old_object_from_copy_counts() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let large =
        crate::arena::arena_alloc_gc(LARGE_OBJECT_THRESHOLD_BYTES, 8, GC_TYPE_STRING) as usize;
    let header = unsafe { header_from_user_ptr(large as *const u8) };
    let total = unsafe { (*header).size as usize };

    assert!(is_large_object_total_size(total));
    assert!(crate::arena::pointer_in_old_gen(large));
    js_shadow_slot_set(0, ptr_bits(large));

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    let after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_eq!(after, large);
    assert_eq!(trace.copying_nursery.copied_objects, 0);
    assert_eq!(trace.copying_nursery.copied_bytes, 0);
    assert_eq!(trace.copying_nursery.promoted_objects, 0);
    assert_eq!(trace.copying_nursery.promoted_bytes, 0);
    assert_eq!(trace.copying_nursery.large_excluded_objects, 1);
    assert_eq!(trace.copying_nursery.large_excluded_bytes, total);

    let event = trace.into_json(GcStepSnapshot::current());
    assert_eq!(
        event["copying_nursery"]["large_excluded_objects"].as_u64(),
        Some(1)
    );
    assert_eq!(
        event["copying_nursery"]["large_excluded_bytes"].as_u64(),
        Some(total as u64)
    );
}

#[test]
fn large_object_old_born_array_slot_write_keeps_young_child_alive() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let child = young_leaf();
    let arr = crate::array::js_array_alloc(4096);

    assert!(crate::arena::pointer_in_old_gen(arr as usize));
    crate::array::js_array_set_f64_extend(arr, 0, f64::from_bits(ptr_bits(child)));
    assert!(
        remembered_set_size() > 0,
        "large old-born array write should dirty old-page metadata"
    );

    let elements = unsafe {
        (arr as *mut u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *mut u64
    };
    let trace = collect_minor_trace(GcTriggerKind::Direct);
    let rewritten = unsafe { (*elements & POINTER_MASK) as usize };

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_ne!(rewritten, child);
    assert!(crate::arena::pointer_in_nursery(rewritten));
    assert_eq!(trace.copying_nursery.copied_objects, 1);
    assert!(
        remembered_set_size() > 0,
        "old-to-survivor edge must remain remembered after copied minor"
    );
}

#[test]
fn large_object_array_literal_direct_store_keeps_young_child_alive_and_excludes_parent() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let child = young_leaf();
    let child_total = unsafe { (*header_from_user_ptr(child as *const u8)).size as usize };
    let arr = crate::array::js_array_alloc_literal(4096);
    let parent_total = unsafe { (*header_from_user_ptr(arr as *const u8)).size as usize };

    assert!(crate::arena::pointer_in_old_gen(arr as usize));
    assert!(is_large_object_total_size(parent_total));
    let elements = unsafe {
        (arr as *mut u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *mut u64
    };
    unsafe {
        *elements = ptr_bits(child);
    }
    layout_note_slot(arr as usize, 0, unsafe { *elements });
    runtime_write_barrier_slot(arr as usize, elements as usize, unsafe { *elements });
    assert!(
        remembered_set_size() > 0,
        "direct large literal store should dirty old-page metadata"
    );
    js_shadow_slot_set(0, ptr_bits(arr as usize));

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    let arr_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    let rewritten = unsafe { (*elements & POINTER_MASK) as usize };

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_eq!(arr_after, arr as usize);
    assert_ne!(rewritten, child);
    assert!(crate::arena::pointer_in_nursery(rewritten));
    assert_eq!(trace.copying_nursery.copied_objects, 1);
    assert_eq!(trace.copying_nursery.copied_bytes, child_total);
    assert_eq!(trace.copying_nursery.promoted_objects, 0);
    assert_eq!(trace.copying_nursery.promoted_bytes, 0);
    assert_eq!(trace.copying_nursery.large_excluded_objects, 1);
    assert_eq!(trace.copying_nursery.large_excluded_bytes, parent_total);
}

#[test]
fn large_object_inline_push_store_keeps_young_child_alive_and_excludes_parent() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let child = young_leaf();
    let child_total = unsafe { (*header_from_user_ptr(child as *const u8)).size as usize };
    let arr = crate::array::js_array_alloc(4096);
    let parent_total = unsafe { (*header_from_user_ptr(arr as *const u8)).size as usize };

    assert!(crate::arena::pointer_in_old_gen(arr as usize));
    assert!(is_large_object_total_size(parent_total));

    let elements = unsafe {
        (arr as *mut u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *mut u64
    };
    let slot = unsafe {
        let length = (*arr).length as usize;
        assert!(length < (*arr).capacity as usize);
        let slot = elements.add(length);
        *slot = ptr_bits(child);
        (*arr).length = length as u32 + 1;
        layout_note_slot(arr as usize, length, *slot);
        runtime_write_barrier_slot(arr as usize, slot as usize, *slot);
        slot
    };
    assert!(
        remembered_set_size() > 0,
        "optimized direct push store should dirty old-page metadata"
    );
    js_shadow_slot_set(0, ptr_bits(arr as usize));

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    let arr_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    let rewritten = unsafe { (*slot & POINTER_MASK) as usize };

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_eq!(arr_after, arr as usize);
    assert_ne!(rewritten, child);
    assert!(crate::arena::pointer_in_nursery(rewritten));
    assert_eq!(trace.copying_nursery.copied_objects, 1);
    assert_eq!(trace.copying_nursery.copied_bytes, child_total);
    assert_eq!(trace.copying_nursery.promoted_objects, 0);
    assert_eq!(trace.copying_nursery.promoted_bytes, 0);
    assert_eq!(trace.copying_nursery.large_excluded_objects, 1);
    assert_eq!(trace.copying_nursery.large_excluded_bytes, parent_total);
    assert!(
        remembered_set_size() > 0,
        "old-to-survivor edge must remain remembered after copied minor"
    );
}

#[test]
fn large_object_clone_direct_copy_keeps_young_child_alive_and_excludes_parent() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let child = young_leaf();
    let child_total = unsafe { (*header_from_user_ptr(child as *const u8)).size as usize };
    let src = crate::object::js_object_alloc(0, 1);
    crate::object::js_object_set_field(src, 0, crate::value::JSValue::from_bits(ptr_bits(child)));

    let clone = unsafe {
        crate::object::js_object_clone_with_extra(
            f64::from_bits(ptr_bits(src as usize)),
            4096,
            std::ptr::null(),
            0,
        )
    };
    let parent_total = unsafe { (*header_from_user_ptr(clone as *const u8)).size as usize };
    let fields = unsafe {
        (clone as *mut u8).add(std::mem::size_of::<crate::object::ObjectHeader>()) as *mut u64
    };

    assert!(crate::arena::pointer_in_old_gen(clone as usize));
    assert!(is_large_object_total_size(parent_total));
    assert!(
        remembered_set_size() > 0,
        "old-born clone field copy should dirty old-page metadata"
    );
    js_shadow_slot_set(0, ptr_bits(clone as usize));

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    let clone_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    let rewritten = unsafe { (*fields & POINTER_MASK) as usize };

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_eq!(clone_after, clone as usize);
    assert_ne!(rewritten, child);
    assert!(crate::arena::pointer_in_nursery(rewritten));
    assert_eq!(trace.copying_nursery.copied_objects, 1);
    assert_eq!(trace.copying_nursery.copied_bytes, child_total);
    assert_eq!(trace.copying_nursery.promoted_objects, 0);
    assert_eq!(trace.copying_nursery.promoted_bytes, 0);
    assert!(trace.copying_nursery.large_excluded_objects >= 1);
    assert!(trace.copying_nursery.large_excluded_bytes >= parent_total);
}

#[test]
fn test_copied_minor_verify_evacuation_env_remains_eligible() {
    let _env_guard = EnvVarGuard::set("PERRY_GC_VERIFY_EVACUATION", "1");
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let child = young_leaf();
    js_shadow_slot_set(0, ptr_bits(child));

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    let after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert!(
        trace.phase_us.contains_key("evacuation_verify"),
        "forced copied-minor verification should run before from-space reset"
    );
    assert_ne!(after, child);
    assert!(crate::arena::pointer_in_nursery(after));
}

#[test]
fn test_copying_minor_rewrites_dirty_old_slot_and_keeps_sticky_page() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let child = young_leaf();
    let (old_arr, elements) = unsafe { alloc_old_test_array(1) };
    unsafe {
        *elements = ptr_bits(child);
    }
    js_write_barrier_slot(ptr_bits(old_arr as usize), elements as u64, ptr_bits(child));
    assert!(remembered_set_size() > 0);

    let _ = gc_collect_minor();
    let rewritten = unsafe { (*elements & POINTER_MASK) as usize };

    assert_ne!(rewritten, child);
    assert!(crate::arena::pointer_in_nursery(rewritten));
    assert!(
        remembered_set_size() > 0,
        "old-to-survivor edge must stay dirty for the next minor"
    );
}

#[test]
fn test_copying_minor_copies_transitive_young_graph() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let arr = crate::array::js_array_alloc(1);
    let child = young_leaf();
    unsafe {
        (*arr).length = 1;
        let elements =
            (arr as *mut u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *mut u64;
        *elements = ptr_bits(child);
        layout_note_slot(arr as usize, 0, *elements);
    }
    js_shadow_slot_set(0, ptr_bits(arr as usize));

    let _ = gc_collect_minor();
    let arr_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    let child_after = unsafe {
        let elements = (arr_after as *mut u8).add(std::mem::size_of::<crate::array::ArrayHeader>())
            as *mut u64;
        (*elements & POINTER_MASK) as usize
    };

    assert_ne!(arr_after, arr as usize);
    assert_ne!(child_after, child);
    assert!(crate::arena::pointer_in_nursery(arr_after));
    assert!(crate::arena::pointer_in_nursery(child_after));
}

#[test]
fn test_copying_minor_moves_layout_masked_transitive_object() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let arr = crate::array::js_array_alloc(1);
    let (child, _child_fields) = unsafe { alloc_nursery_test_object(0) };
    unsafe {
        (*arr).length = 1;
        let elements =
            (arr as *mut u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *mut u64;
        *elements = ptr_bits(child as usize);
        layout_note_slot(arr as usize, 0, *elements);
    }
    js_shadow_slot_set(0, ptr_bits(arr as usize));

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    let arr_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    let child_after = unsafe {
        let elements = (arr_after as *mut u8).add(std::mem::size_of::<crate::array::ArrayHeader>())
            as *mut u64;
        (*elements & POINTER_MASK) as usize
    };

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_ne!(arr_after, arr as usize);
    assert_ne!(child_after, child as usize);
    assert!(crate::arena::pointer_in_nursery(arr_after));
    assert!(crate::arena::pointer_in_nursery(child_after));
    assert!(
        trace.copying_nursery.copied_objects >= 2,
        "root array and transitive object should both move"
    );
}

#[test]
fn test_copying_minor_rewrites_singleton_closure_caches() {
    struct SingletonClosureCacheGuard;

    impl Drop for SingletonClosureCacheGuard {
        fn drop(&mut self) {
            crate::closure::test_clear_singleton_closure_caches();
        }
    }

    let _guard = CopyingNurseryTestGuard::new(1);
    let _cache_guard = SingletonClosureCacheGuard;
    crate::closure::test_clear_singleton_closure_caches();
    gc_register_mutable_root_scanner(crate::closure::scan_singleton_closure_roots_mut);

    let no_capture_func = test_no_capture_singleton_func as *const u8;
    let no_capture = crate::closure::js_closure_alloc_singleton(no_capture_func);
    assert_eq!(
        crate::closure::test_singleton_closure_cache_entry(no_capture_func),
        Some(no_capture)
    );

    let captured_value = young_leaf();
    let capture_bits = ptr_bits(captured_value);
    js_shadow_slot_set(0, capture_bits);

    let captured_func = test_captured_singleton_func as *const u8;
    let captures = [capture_bits];
    let captured = crate::closure::js_closure_alloc_with_captures_singleton(
        captured_func,
        1,
        captures.as_ptr(),
    );
    assert_eq!(
        crate::closure::js_closure_alloc_with_captures_singleton(
            captured_func,
            1,
            captures.as_ptr(),
        ),
        captured,
        "captured singleton cache should hit before GC"
    );

    let before_entries =
        crate::closure::test_captured_singleton_closure_cache_entries(captured_func);
    assert_eq!(before_entries.len(), 1);
    assert_eq!(before_entries[0].0, vec![capture_bits]);
    assert_eq!(before_entries[0].1, captured);

    let capture_slot = unsafe {
        (captured as *mut u8).add(std::mem::size_of::<crate::closure::ClosureHeader>()) as *mut u64
    };
    assert_eq!(unsafe { *capture_slot }, capture_bits);

    activate_malloc_registry_for_tests();
    js_shadow_slot_set(0, 0);
    let _ = gc_collect_minor();

    let no_capture_after = crate::closure::test_singleton_closure_cache_entry(no_capture_func)
        .expect("no-capture singleton cache should remain populated");
    assert_ne!(
        no_capture_after, no_capture,
        "managed no-capture singleton should be rewritten after copied-minor"
    );
    assert_eq!(
        crate::closure::js_closure_alloc_singleton(no_capture_func),
        no_capture_after,
        "no-capture singleton should remain a cache hit across copied-minor"
    );

    let after_entries =
        crate::closure::test_captured_singleton_closure_cache_entries(captured_func);
    assert_eq!(after_entries.len(), 1);
    let captured_after = after_entries[0].1;
    assert_eq!(
        crate::arena::classify_heap_space(captured_after as usize),
        crate::arena::active_survivor_space()
    );
    assert_ne!(
        captured_after, captured,
        "captured singleton closure should be rewritten after copied-minor"
    );

    let capture_after_slot = unsafe {
        (captured_after as *mut u8).add(std::mem::size_of::<crate::closure::ClosureHeader>())
            as *mut u64
    };
    let capture_after_bits = unsafe { *capture_after_slot };
    let capture_after = (capture_after_bits & POINTER_MASK) as usize;
    assert_ne!(
        capture_after, captured_value,
        "captured young value should move out of eden"
    );
    assert_eq!(
        crate::arena::classify_heap_space(capture_after),
        crate::arena::active_survivor_space()
    );

    assert_eq!(after_entries[0].1, captured_after);
    assert_eq!(
        after_entries[0].0,
        vec![capture_after_bits],
        "captured-cache key should be rewritten to the moved capture"
    );

    let rewritten_captures = [capture_after_bits];
    assert_eq!(
        crate::closure::js_closure_alloc_with_captures_singleton(
            captured_func,
            1,
            rewritten_captures.as_ptr(),
        ),
        captured_after,
        "future cache lookups should hit with the rewritten capture key"
    );
}

#[test]
fn test_copying_minor_rewrites_overflow_owner_metadata_key() {
    struct OverflowFieldsRootGuard;

    impl Drop for OverflowFieldsRootGuard {
        fn drop(&mut self) {
            crate::object::test_clear_overflow_fields_root();
        }
    }

    let _guard = CopyingNurseryTestGuard::new(1);
    let _overflow_guard = OverflowFieldsRootGuard;
    crate::object::test_clear_overflow_fields_root();
    gc_register_mutable_root_scanner(overflow_fields_mutable_root_scanner);

    let owner = crate::object::js_object_alloc(0, 0) as usize;
    let overflow_value = young_leaf();
    crate::object::test_seed_overflow_fields_root(owner, ptr_bits(overflow_value));
    js_shadow_slot_set(0, ptr_bits(owner));

    let _ = gc_collect_minor();
    let owner_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    let (mapped_owner, mapped_value_bits) = crate::object::test_overflow_fields_root();
    let mapped_value = (mapped_value_bits & POINTER_MASK) as usize;

    assert_ne!(owner_after, owner);
    assert_eq!(mapped_owner, owner_after);
    assert_ne!(mapped_value, overflow_value);
    assert!(crate::arena::pointer_in_nursery(owner_after));
    assert!(crate::arena::pointer_in_nursery(mapped_value));
}

#[test]
fn test_copying_minor_promotes_survivor_on_fourth_survival() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let child = young_leaf();
    js_shadow_slot_set(0, ptr_bits(child));

    let _ = gc_collect_minor();
    let survivor = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert!(crate::arena::pointer_in_nursery(survivor));

    let _ = gc_collect_minor();
    let survivor_second = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(survivor_second, survivor);
    assert!(crate::arena::pointer_in_nursery(survivor_second));

    let _ = gc_collect_minor();
    let survivor_third = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(survivor_third, survivor_second);
    assert!(crate::arena::pointer_in_nursery(survivor_third));

    let _ = gc_collect_minor();
    let promoted = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(promoted, survivor_third);
    assert!(crate::arena::pointer_in_old_gen(promoted));
}

#[test]
fn test_copying_minor_preserves_old_page_accounting_for_defrag_policy() {
    struct ResetGcTestState {
        pinned_header: *mut GcHeader,
    }

    impl Drop for ResetGcTestState {
        fn drop(&mut self) {
            reset_shadow_stack();
            reset_global_roots();
            reset_remembered_set();
            clear_marks();
            clear_mark_seeds();
            CONS_PINNED.with(|s| s.borrow_mut().clear());
            if !self.pinned_header.is_null() {
                unsafe {
                    (*self.pinned_header).gc_flags &= !GC_FLAG_PINNED;
                }
            }
        }
    }

    let mut reset = ResetGcTestState {
        pinned_header: std::ptr::null_mut(),
    };
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    clear_marks();
    clear_mark_seeds();
    CONS_PINNED.with(|s| s.borrow_mut().clear());

    let child = young_leaf();
    js_shadow_slot_set(0, ptr_bits(child));

    let first_trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&first_trace, true, CopiedMinorFallbackReason::None, false);
    let survivor = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(survivor, child);
    assert!(crate::arena::pointer_in_nursery(survivor));

    let second_trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&second_trace, true, CopiedMinorFallbackReason::None, false);
    assert_eq!(second_trace.copying_nursery.promoted_objects, 0);
    let survivor = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert!(crate::arena::pointer_in_nursery(survivor));

    let third_trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&third_trace, true, CopiedMinorFallbackReason::None, false);
    assert_eq!(third_trace.copying_nursery.promoted_objects, 0);
    let survivor = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert!(crate::arena::pointer_in_nursery(survivor));

    let survivor_header = unsafe { header_from_user_ptr(survivor as *const u8) };
    let survivor_total = unsafe { (*survivor_header).size as usize };

    crate::arena::old_pages_begin_gc_cycle();
    let live = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_STRING) as usize;
    let dead = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_STRING) as usize;
    let (live_header, live_total) = old_test_header_and_size(live);
    let (_dead_header, dead_total) = old_test_header_and_size(dead);
    let mut fragmented_pages = crate::fast_hash::new_ptr_hash_set();
    for (page, _) in crate::arena::old_object_page_overlaps(live_header as usize, live_total) {
        fragmented_pages.insert(page);
    }
    for (page, _) in crate::arena::old_object_page_overlaps(dead - GC_HEADER_SIZE, dead_total) {
        fragmented_pages.insert(page);
    }
    let pinned =
        crate::arena::arena_alloc_gc_old_excluding_pages(40, 8, GC_TYPE_STRING, &fragmented_pages)
            as usize;
    let (pinned_header, pinned_total) = old_test_header_and_size(pinned);
    reset.pinned_header = pinned_header;

    unsafe {
        (*survivor_header).gc_flags |= GC_FLAG_MARKED;
        (*live_header).gc_flags |= GC_FLAG_MARKED;
        (*pinned_header).gc_flags |= GC_FLAG_PINNED;
    }

    let sweep = sweep_with_age_bump(false);
    let before_summary = crate::arena::old_page_summary();
    let before_selection = select_old_page_defrag_pages(false);

    assert!(
        sweep.freed_bytes >= dead_total as u64,
        "seeded dead old object should be observed by sweep accounting"
    );
    assert!(
        before_summary.dead_bytes >= dead_total,
        "old-page summary should include seeded dead bytes before copied minor"
    );
    assert!(
        before_summary.pinned_bytes >= pinned_total,
        "old-page summary should include seeded pinned bytes before copied minor"
    );
    assert!(
        before_selection.selected_pages > 0,
        "seeded unpinned live/dead old page should be selected for defrag"
    );

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    let promoted = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    let promoted_header = unsafe { header_from_user_ptr(promoted as *const u8) };
    let promoted_total = unsafe { (*promoted_header).size as usize };
    let promoted_page_count =
        crate::arena::old_object_page_overlaps(promoted_header as usize, promoted_total).len();
    let post_summary = crate::arena::old_page_summary();
    let after_selection = select_old_page_defrag_pages(false);

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_ne!(promoted, survivor);
    assert!(crate::arena::pointer_in_old_gen(promoted));
    assert_eq!(promoted_total, survivor_total);
    assert_eq!(trace.copying_nursery.promoted_objects, 1);
    assert_eq!(trace.copying_nursery.promoted_bytes, survivor_total);
    assert_eq!(trace.old_pages, post_summary);
    assert_eq!(trace.old_pages.dead_bytes, before_summary.dead_bytes);
    assert_eq!(
        trace.old_pages.dead_object_count,
        before_summary.dead_object_count
    );
    assert_eq!(trace.old_pages.pinned_bytes, before_summary.pinned_bytes);
    assert_eq!(
        trace.old_pages.pinned_object_count,
        before_summary.pinned_object_count
    );
    assert_eq!(
        post_summary.live_bytes,
        before_summary.live_bytes + survivor_total
    );
    assert_eq!(
        post_summary.live_object_count,
        before_summary.live_object_count + promoted_page_count
    );
    assert!(
        after_selection.selected_pages > 0,
        "copied minor must leave old-page defrag candidates selectable"
    );
}

#[test]
fn test_copying_minor_sticky_old_to_survivor_edge_promotes_on_fourth_cycle() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let child = young_leaf();
    let (old_arr, elements) = unsafe { alloc_old_test_array(1) };
    unsafe {
        *elements = ptr_bits(child);
    }
    js_write_barrier_slot(ptr_bits(old_arr as usize), elements as u64, ptr_bits(child));

    let _ = gc_collect_minor();
    let survivor = unsafe { (*elements & POINTER_MASK) as usize };
    assert!(crate::arena::pointer_in_nursery(survivor));
    assert!(remembered_set_size() > 0);

    let _ = gc_collect_minor();
    let survivor_second = unsafe { (*elements & POINTER_MASK) as usize };
    assert!(crate::arena::pointer_in_nursery(survivor_second));
    assert!(remembered_set_size() > 0);

    let _ = gc_collect_minor();
    let survivor_third = unsafe { (*elements & POINTER_MASK) as usize };
    assert!(crate::arena::pointer_in_nursery(survivor_third));
    assert!(remembered_set_size() > 0);

    let _ = gc_collect_minor();
    let promoted = unsafe { (*elements & POINTER_MASK) as usize };
    assert!(crate::arena::pointer_in_old_gen(promoted));
}

#[test]
fn test_copying_minor_resets_eden_wholesale() {
    let _guard = CopyingNurseryTestGuard::new(1);
    for _ in 0..128 {
        let _ = young_leaf();
    }
    let live = young_leaf();
    js_shadow_slot_set(0, ptr_bits(live));

    let _ = gc_collect_minor();
    let snapshot = crate::arena::arena_telemetry_snapshot();
    let live_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;

    assert_eq!(snapshot.arena.in_use_bytes, 0);
    assert!(crate::arena::pointer_in_nursery(live_after));
}

#[test]
fn test_copying_minor_sweeps_malloc_when_due_on_arena_trigger() {
    let _guard = CopyingNurseryTestGuard::new(2);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    assert!(copied_minor_malloc_sweep_due(GcTriggerKind::MallocCount));
    let live_young = young_leaf();
    js_shadow_slot_set(0, ptr_bits(live_young));
    let live_malloc = gc_malloc(
        std::mem::size_of::<crate::closure::ClosureHeader>(),
        GC_TYPE_CLOSURE,
    );
    unsafe {
        init_test_closure(live_malloc);
    }
    js_shadow_slot_set(1, ptr_bits(live_malloc as usize));
    activate_malloc_registry_for_tests();

    let churn_headers = allocate_dead_malloc_churn_headers(32);
    assert_eq!(
        tracked_malloc_headers_matching(&churn_headers),
        churn_headers.len(),
        "malloc churn should be tracked before the collection"
    );
    let tracked_before = malloc_object_count();
    trigger_guard.make_malloc_sweep_due();
    assert!(copied_minor_malloc_sweep_due(GcTriggerKind::ArenaBytes));

    let outcome = gc_collect_minor_with_trigger(GcTriggerSnapshot {
        kind: GcTriggerKind::ArenaBytes,
        steps_before: Some(GcStepSnapshot::current()),
    });
    let trace = outcome.trace.expect("test requested GC trace capture");

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, true);
    assert_eq!(
        tracked_malloc_headers_matching(&churn_headers),
        0,
        "copied-minor GC must sweep dead malloc churn when malloc pressure is due"
    );
    assert!(
        malloc_user_ptr_tracked(live_malloc),
        "live malloc root should survive copied-minor malloc sweep"
    );
    assert!(
        malloc_object_count() < tracked_before,
        "malloc sweep should reduce the tracked malloc object count"
    );
    assert!(
        outcome.freed_bytes > 0,
        "copied-minor path should report malloc reclaim"
    );
}

#[test]
fn test_gc_check_trigger_copied_minor_malloc_sweep_rebaselines_trigger() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let live_malloc = gc_malloc(
        std::mem::size_of::<crate::closure::ClosureHeader>(),
        GC_TYPE_CLOSURE,
    );
    unsafe {
        init_test_closure(live_malloc);
    }
    js_shadow_slot_set(0, ptr_bits(live_malloc as usize));
    activate_malloc_registry_for_tests();

    let churn_headers = allocate_dead_malloc_churn_headers(48);
    assert_eq!(
        tracked_malloc_headers_matching(&churn_headers),
        churn_headers.len(),
        "malloc churn should be tracked before gc_check_trigger"
    );
    let tracked_before = malloc_object_count();
    trigger_guard.make_malloc_sweep_due();
    let collections_before = gc_collection_count();

    gc_check_trigger();

    assert!(
        gc_collection_count() > collections_before,
        "gc_check_trigger should collect when malloc pressure is due"
    );
    assert_eq!(
        tracked_malloc_headers_matching(&churn_headers),
        0,
        "copied-minor collection should reclaim dead malloc churn"
    );
    assert!(
        malloc_user_ptr_tracked(live_malloc),
        "live malloc root should survive gc_check_trigger collection"
    );
    let survivors_after = malloc_object_count();
    assert!(
        survivors_after < tracked_before,
        "malloc sweep should reduce MALLOC_STATE.objects"
    );
    let malloc_step_after = GC_MALLOC_COUNT_STEP.with(|step| step.get());
    let next_malloc_trigger = GC_NEXT_MALLOC_TRIGGER.with(|trigger| trigger.get());
    assert_eq!(
        next_malloc_trigger,
        survivors_after + malloc_step_after,
        "gc_check_trigger should rebaseline the next malloc trigger to survivors + step"
    );
}

#[test]
fn test_gc_check_trigger_copied_minor_without_malloc_sweep_preserves_malloc_trigger() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    deactivate_malloc_registry_for_tests();

    let live_young = young_leaf();
    js_shadow_slot_set(0, ptr_bits(live_young));
    let churn_headers = allocate_dead_malloc_churn_headers(48);
    let tracked_before = tracked_malloc_headers_matching(&churn_headers);
    assert_eq!(
        tracked_before,
        churn_headers.len(),
        "malloc churn should be tracked before gc_check_trigger"
    );

    let malloc_count_before = malloc_object_count();
    let next_malloc_trigger = malloc_count_before + 1;
    GC_NEXT_MALLOC_TRIGGER.with(|trigger| trigger.set(next_malloc_trigger));
    trigger_guard.make_arena_trigger_due();
    assert!(
        !copied_minor_malloc_sweep_due(GcTriggerKind::ArenaBytes),
        "arena-triggered copied-minor should not sweep malloc while below malloc pressure"
    );

    let collections_before = gc_collection_count();
    gc_check_trigger();

    assert!(
        gc_collection_count() > collections_before,
        "gc_check_trigger should collect when arena pressure is due"
    );
    assert_eq!(
        tracked_malloc_headers_matching(&churn_headers),
        tracked_before,
        "malloc sweep was not due, so dead churn should remain tracked"
    );
    assert_eq!(
        malloc_object_count(),
        malloc_count_before,
        "copied-minor collection should not sweep malloc while below malloc pressure"
    );
    assert_eq!(
        GC_NEXT_MALLOC_TRIGGER.with(|trigger| trigger.get()),
        next_malloc_trigger,
        "arena-triggered copied-minor without malloc sweep must preserve the existing malloc trigger"
    );
}

#[test]
fn test_copied_minor_malloc_scaling_no_roots_skips_registry_walk() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    deactivate_malloc_registry_for_tests();

    let churn_headers = allocate_dead_malloc_churn_headers(512);
    let tracked_before = tracked_malloc_headers_matching(&churn_headers);
    assert_eq!(tracked_before, churn_headers.len());
    let live_young = young_leaf();
    js_shadow_slot_set(0, ptr_bits(live_young));

    let outcome = gc_collect_minor_with_trigger(GcTriggerSnapshot {
        kind: GcTriggerKind::Direct,
        steps_before: Some(GcStepSnapshot::current()),
    });
    let trace = outcome.trace.expect("test requested GC trace capture");

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_eq!(
        trace.copying_nursery.malloc_validation_lookups, 0,
        "copied-minor should not probe malloc entries when no roots mention malloc"
    );
    assert_eq!(
        trace.copying_nursery.malloc_registry_rebuilds, 0,
        "copied-minor must not rebuild the malloc registry"
    );
    assert!(
        !malloc_registry_active_for_tests(),
        "copied-minor should leave an inactive malloc registry inactive"
    );
    assert_eq!(
        tracked_malloc_headers_matching(&churn_headers),
        tracked_before,
        "malloc sweep was not due, so dead churn should remain tracked without being walked"
    );
}

#[test]
fn test_copied_minor_malloc_scaling_live_root_with_active_registry() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let live_child = young_leaf();
    let live_malloc = gc_malloc(
        std::mem::size_of::<crate::closure::ClosureHeader>() + std::mem::size_of::<u64>(),
        GC_TYPE_CLOSURE,
    );
    let capture_slot =
        unsafe { init_test_closure_with_one_capture(live_malloc, ptr_bits(live_child)) };
    js_shadow_slot_set(0, ptr_bits(live_malloc as usize));
    activate_malloc_registry_for_tests();
    assert!(malloc_registry_active_for_tests());

    let churn_headers = allocate_dead_malloc_churn_headers(128);
    trigger_guard.make_malloc_sweep_due();
    let outcome = gc_collect_minor_with_trigger(GcTriggerSnapshot {
        kind: GcTriggerKind::ArenaBytes,
        steps_before: Some(GcStepSnapshot::current()),
    });
    let trace = outcome.trace.expect("test requested GC trace capture");

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, true);
    assert!(
        trace.copying_nursery.malloc_validation_lookups > 0,
        "active registry should validate the live malloc root"
    );
    assert!(
        trace.copying_nursery.malloc_validation_lookups < churn_headers.len(),
        "malloc validation should scale with reachable candidates, not dead churn"
    );
    assert_eq!(
        trace.copying_nursery.malloc_registry_rebuilds, 0,
        "copied-minor should use the active registry without rebuilding it"
    );
    assert_eq!(tracked_malloc_headers_matching(&churn_headers), 0);
    assert!(malloc_user_ptr_tracked(live_malloc));
    let capture_after = unsafe { (*capture_slot & POINTER_MASK) as usize };
    assert_ne!(capture_after, live_child);
    assert!(crate::arena::pointer_in_nursery(capture_after));
}

#[test]
fn test_copied_minor_malloc_scaling_falls_back_when_registry_unavailable() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let live_malloc = gc_malloc(
        std::mem::size_of::<crate::closure::ClosureHeader>(),
        GC_TYPE_CLOSURE,
    );
    unsafe {
        init_test_closure(live_malloc);
    }
    let mut raw_root = live_malloc as u64;
    js_gc_register_global_root(&mut raw_root as *mut u64 as i64);
    deactivate_malloc_registry_for_tests();

    let outcome = gc_collect_minor_with_trigger(GcTriggerSnapshot {
        kind: GcTriggerKind::Direct,
        steps_before: Some(GcStepSnapshot::current()),
    });
    let trace = outcome.trace.expect("test requested GC trace capture");

    assert_copied_minor_trace(
        &trace,
        false,
        CopiedMinorFallbackReason::MallocRegistryUnavailable,
        false,
    );
    assert_eq!(
        trace.copying_nursery.malloc_registry_rebuilds, 0,
        "copied-minor fallback must not rebuild the malloc registry"
    );
    assert!(malloc_user_ptr_tracked(live_malloc));
    assert_eq!(raw_root as usize, live_malloc as usize);
    assert!(
        !malloc_registry_active_for_tests(),
        "fallback mark-sweep should not activate the copied-minor malloc registry"
    );
}

#[test]
fn test_copying_minor_falls_back_for_pinned_young_root() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let child = young_leaf();
    unsafe {
        (*header_from_user_ptr(child as *const u8)).gc_flags |= GC_FLAG_PINNED;
    }
    js_shadow_slot_set(0, ptr_bits(child));

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    let after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;

    assert_copied_minor_trace(
        &trace,
        false,
        CopiedMinorFallbackReason::PinnedYoungRoot,
        false,
    );
    assert_eq!(after, child);
    unsafe {
        (*header_from_user_ptr(child as *const u8)).gc_flags &= !GC_FLAG_PINNED;
    }
}

#[test]
fn test_copying_minor_falls_back_for_pinned_young_dirty_slot() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let child = young_leaf();
    let (old_arr, elements) = unsafe { alloc_old_test_array(1) };
    unsafe {
        *elements = ptr_bits(child);
        (*header_from_user_ptr(child as *const u8)).gc_flags |= GC_FLAG_PINNED;
    }
    js_write_barrier_slot(ptr_bits(old_arr as usize), elements as u64, ptr_bits(child));

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    let child_after = unsafe { (*elements & POINTER_MASK) as usize };

    assert_copied_minor_trace(
        &trace,
        false,
        CopiedMinorFallbackReason::PinnedYoungDirtySlot,
        false,
    );
    assert_eq!(child_after, child);
    unsafe {
        (*header_from_user_ptr(child as *const u8)).gc_flags &= !GC_FLAG_PINNED;
    }
}

#[test]
fn test_copying_minor_falls_back_for_transitive_pinned_young_child() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let arr = crate::array::js_array_alloc(1);
    let child = young_leaf();
    let elements = unsafe {
        (*arr).length = 1;
        let elements =
            (arr as *mut u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *mut u64;
        *elements = ptr_bits(child);
        layout_note_slot(arr as usize, 0, *elements);
        (*header_from_user_ptr(child as *const u8)).gc_flags |= GC_FLAG_PINNED;
        elements
    };
    if gc_force_evacuate_enabled() {
        // This test is about copying-preflight fallback; forced
        // evacuation would otherwise move the parent after fallback.
        let arr_header = unsafe { header_from_user_ptr(arr as *const u8) };
        CONS_PINNED.with(|s| {
            s.borrow_mut().insert(arr_header as usize);
        });
    }
    js_shadow_slot_set(0, ptr_bits(arr as usize));

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    let arr_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    let child_after = unsafe { (*elements & POINTER_MASK) as usize };

    assert_copied_minor_trace(
        &trace,
        false,
        CopiedMinorFallbackReason::PinnedYoungTransitive,
        false,
    );
    assert_eq!(
        arr_after, arr as usize,
        "copying nursery must fall back before moving the young parent"
    );
    assert_eq!(
        child_after, child,
        "pinned transitive young child must keep its raw address"
    );
    unsafe {
        let child_header = header_from_user_ptr(child as *const u8);
        assert_eq!(
            (*child_header).gc_flags & GC_FLAG_FORWARDED,
            0,
            "pinned child must not receive a forwarding pointer"
        );
        (*child_header).gc_flags &= !GC_FLAG_PINNED;
    }
}

unsafe fn alloc_old_test_map(
    capacity: u32,
) -> (*mut crate::map::MapHeader, *mut u64, std::alloc::Layout) {
    let map = crate::arena::arena_alloc_gc_old(
        std::mem::size_of::<crate::map::MapHeader>(),
        8,
        GC_TYPE_MAP,
    ) as *mut crate::map::MapHeader;
    let layout = std::alloc::Layout::from_size_align((capacity as usize * 16).max(8), 8)
        .expect("valid map entries layout");
    let entries = std::alloc::alloc_zeroed(layout) as *mut u64;
    assert!(!entries.is_null());
    (*map).size = 0;
    (*map).capacity = capacity;
    (*map).entries = entries as *mut f64;
    (map, entries, layout)
}

unsafe fn retire_old_test_map(
    map: *mut crate::map::MapHeader,
    entries: *mut u64,
    layout: std::alloc::Layout,
) {
    (*map).size = 0;
    (*map).capacity = 0;
    (*map).entries = std::ptr::null_mut();
    std::alloc::dealloc(entries as *mut u8, layout);
}

unsafe fn field_index_not_on_last_page(fields: *mut u64, field_count: u32) -> usize {
    assert!(field_count > 1);
    let last_page =
        crate::arena::generation_page_for_addr(fields.add(field_count as usize - 1) as usize);
    for i in 0..field_count as usize {
        if crate::arena::generation_page_for_addr(fields.add(i) as usize) != last_page {
            return i;
        }
    }
    panic!("test object did not span multiple field pages");
}

unsafe fn field_indices_on_distinct_pages(fields: *mut u64, field_count: u32) -> (usize, usize) {
    assert!(field_count > 1);
    let first = field_index_not_on_last_page(fields, field_count);
    let first_page = crate::arena::generation_page_for_addr(fields.add(first) as usize);
    for i in 0..field_count as usize {
        if crate::arena::generation_page_for_addr(fields.add(i) as usize) != first_page {
            return (first, i);
        }
    }
    panic!("test object did not span two field pages");
}

#[test]
fn test_write_barrier_old_to_young_records() {
    reset_remembered_set();
    let young = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let old = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_OBJECT) as usize;
    let parent_nanbox = POINTER_TAG | (old as u64);
    let child_nanbox = POINTER_TAG | (young as u64);
    let dirty_page = crate::arena::generation_page_for_addr(old - GC_HEADER_SIZE);
    assert_eq!(remembered_set_size(), 0);
    assert!(!old_page_dirty_for(dirty_page));
    js_write_barrier(parent_nanbox, child_nanbox);
    assert_eq!(
        remembered_set_size(),
        1,
        "old→young write must dirty the remembered page"
    );
    assert!(
        old_page_dirty_for(dirty_page),
        "old-page metadata should mirror the remembered dirty page"
    );
    // Same write again should NOT double-count (dirty pages dedup).
    js_write_barrier(parent_nanbox, child_nanbox);
    assert_eq!(
        remembered_set_size(),
        1,
        "duplicate barrier call must dedup the dirty page"
    );
    assert!(old_page_dirty_for(dirty_page));
}

#[test]
fn test_write_barrier_slot_marks_dirty_page_and_dedups() {
    reset_remembered_set();
    let young = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let (old_obj, fields) = unsafe { alloc_old_test_object(1) };
    unsafe {
        *fields = POINTER_TAG | young as u64;
    }
    let dirty_page = crate::arena::generation_page_for_addr(fields as usize);
    assert!(!old_page_dirty_for(dirty_page));
    js_write_barrier_slot(
        POINTER_TAG | old_obj as u64,
        fields as u64,
        POINTER_TAG | young as u64,
    );
    assert_eq!(remembered_dirty_page_count(), 1);
    assert!(
        old_page_dirty_for(dirty_page),
        "old-page metadata should mirror the remembered dirty page"
    );
    js_write_barrier_slot(
        POINTER_TAG | old_obj as u64,
        fields as u64,
        POINTER_TAG | young as u64,
    );
    assert_eq!(
        remembered_dirty_page_count(),
        1,
        "same dirty page should be logged once"
    );
    assert!(old_page_dirty_for(dirty_page));
}

#[test]
fn test_write_barrier_young_to_young_skipped() {
    reset_remembered_set();
    let parent = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    js_write_barrier(POINTER_TAG | (parent as u64), POINTER_TAG | (child as u64));
    assert_eq!(
        remembered_set_size(),
        0,
        "young→young write must not enter remembered set"
    );
}

#[test]
fn test_write_barrier_old_to_old_skipped() {
    reset_remembered_set();
    let parent = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_OBJECT) as usize;
    let child = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_OBJECT) as usize;
    js_write_barrier(POINTER_TAG | (parent as u64), POINTER_TAG | (child as u64));
    assert_eq!(
        remembered_set_size(),
        0,
        "old→old write must not enter remembered set (no inter-gen edge)"
    );
}

#[test]
fn test_write_barrier_old_to_young_string_tag() {
    reset_remembered_set();
    let young_str = crate::arena::arena_alloc_gc(32, 8, GC_TYPE_STRING) as usize;
    let old = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_OBJECT) as usize;
    // STRING_TAG should also fire the barrier — strings can be young.
    js_write_barrier(POINTER_TAG | (old as u64), STRING_TAG | (young_str as u64));
    assert_eq!(remembered_set_size(), 1);
}

#[test]
fn test_write_barrier_non_pointer_child_skipped() {
    reset_remembered_set();
    let old = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_OBJECT) as usize;
    // INT32_TAG in child position.
    let int32_val = 0x7FFE_0000_0000_002A_u64;
    js_write_barrier(POINTER_TAG | (old as u64), int32_val);
    assert_eq!(
        remembered_set_size(),
        0,
        "non-pointer child must not enter remembered set"
    );
    // SHORT_STRING_TAG (SSO inline) — also not a heap pointer.
    let sso = 0x7FF9_0500_0000_0000_u64;
    js_write_barrier(POINTER_TAG | (old as u64), sso);
    assert_eq!(
        remembered_set_size(),
        0,
        "SSO child is inline data, not a heap pointer"
    );
    // Plain double in child position.
    js_write_barrier(POINTER_TAG | (old as u64), 3.14_f64.to_bits());
    assert_eq!(
        remembered_set_size(),
        0,
        "number child must not enter remembered set"
    );
}

#[test]
fn test_write_barrier_non_pointer_parent_skipped() {
    reset_remembered_set();
    let young = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    js_write_barrier_slot(0x7FFE_0000_0000_002A_u64, 0, POINTER_TAG | young as u64);
    assert_eq!(
        remembered_set_size(),
        0,
        "non-pointer parent must not dirty remembered pages"
    );
}

#[test]
fn test_write_barrier_remembered_set_clear() {
    reset_remembered_set();
    let young = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let old = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_OBJECT) as usize;
    let dirty_page = crate::arena::generation_page_for_addr(old - GC_HEADER_SIZE);
    js_write_barrier(POINTER_TAG | (old as u64), POINTER_TAG | (young as u64));
    assert_eq!(remembered_set_size(), 1);
    assert!(old_page_dirty_for(dirty_page));
    remembered_set_clear();
    assert_eq!(remembered_set_size(), 0);
    assert!(
        !old_page_dirty_for(dirty_page),
        "old-page metadata dirty bit should clear with the remembered set"
    );
}

#[test]
fn test_write_barrier_slot_clear() {
    reset_remembered_set();
    let young = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let (old_obj, fields) = unsafe { alloc_old_test_object(1) };
    let dirty_page = crate::arena::generation_page_for_addr(fields as usize);
    js_write_barrier_slot(
        POINTER_TAG | old_obj as u64,
        fields as u64,
        POINTER_TAG | young as u64,
    );
    assert_eq!(remembered_dirty_page_count(), 1);
    assert!(old_page_dirty_for(dirty_page));
    remembered_set_clear();
    assert_eq!(remembered_dirty_page_count(), 0);
    assert_eq!(remembered_set_size(), 0);
    assert!(
        !old_page_dirty_for(dirty_page),
        "old-page metadata dirty bit should clear with the remembered set"
    );
}

#[test]
fn test_gc_collect_minor_clears_rs() {
    reset_remembered_set();
    let young = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let (old, fields) = unsafe { alloc_old_test_object(1) };
    unsafe {
        *fields = POINTER_TAG | young as u64;
    }
    js_write_barrier_slot(
        POINTER_TAG | old as u64,
        fields as u64,
        POINTER_TAG | young as u64,
    );
    assert_eq!(remembered_set_size(), 1);
    let _freed = gc_collect_minor();
    assert_eq!(
        remembered_set_size(),
        0,
        "minor GC must clear RS just like full GC does"
    );
}

#[test]
fn test_dirty_page_scan_marks_young_child() {
    reset_remembered_set();
    clear_marks();
    let young = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let (old_obj, fields) = unsafe { alloc_old_test_object(1) };
    unsafe {
        *fields = POINTER_TAG | young as u64;
    }
    js_write_barrier_slot(
        POINTER_TAG | old_obj as u64,
        fields as u64,
        POINTER_TAG | young as u64,
    );
    let valid_ptrs = build_valid_pointer_set();
    let stats = mark_remembered_set_roots(&valid_ptrs);
    assert_eq!(stats.dirty_pages_scanned, 1);
    assert_eq!(stats.old_objects_considered, 1);
    assert_eq!(stats.dirty_objects_scanned, 1);
    assert!(
        stats.dirty_slots_scanned >= 1,
        "dirty page should scan at least the written field slot"
    );
    assert_eq!(stats.newly_marked, 1);
    unsafe {
        let child_header = header_from_user_ptr(young as *const u8);
        assert_ne!((*child_header).gc_flags & GC_FLAG_MARKED, 0);
    }
    clear_marks();
    remembered_set_clear();
}

#[test]
fn test_dirty_page_scan_skips_pointer_free_old_object_payload_slots() {
    reset_remembered_set();
    clear_marks();
    let (old_obj, fields) = unsafe { alloc_old_test_object(2048) };
    let dirty_idx = unsafe { field_index_not_on_last_page(fields, 2048) };
    let dirty_slot = unsafe { fields.add(dirty_idx) };
    unsafe {
        layout_init_pointer_free(old_obj as *mut u8);
        *dirty_slot = 42.0_f64.to_bits();
        mark_dirty_old_page(crate::arena::generation_page_for_addr(dirty_slot as usize));
    }

    assert_eq!(
        test_layout_pointer_slot_count(old_obj as usize, 2048),
        Some(0)
    );
    assert_eq!(test_heap_child_slot_count(old_obj as *mut u8), 0);

    let valid_ptrs = build_valid_pointer_set();
    let stats = mark_remembered_set_roots(&valid_ptrs);
    assert_eq!(stats.dirty_pages_scanned, 1);
    assert_eq!(stats.old_objects_considered, 1);
    assert_eq!(stats.dirty_objects_scanned, 1);
    assert_eq!(
        stats.dirty_slots_scanned, 0,
        "pointer-free old objects must not read payload slots during dirty-page scans"
    );
    assert_eq!(stats.dirty_slot_ranges_scanned, 0);

    clear_marks();
    remembered_set_clear();
}

#[test]
fn test_dirty_page_array_scan_is_slot_range_bounded() {
    reset_remembered_set();
    clear_marks();
    let dirty_child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let clean_child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let (old_arr, elements) = unsafe { alloc_old_test_array(2048) };
    let (dirty_idx, clean_idx) = unsafe { field_indices_on_distinct_pages(elements, 2048) };
    let dirty_slot = unsafe { elements.add(dirty_idx) };
    unsafe {
        *dirty_slot = POINTER_TAG | dirty_child as u64;
        *elements.add(clean_idx) = POINTER_TAG | clean_child as u64;
    }
    js_write_barrier_slot(
        POINTER_TAG | old_arr as u64,
        dirty_slot as u64,
        POINTER_TAG | dirty_child as u64,
    );

    let valid_ptrs = build_valid_pointer_set();
    let stats = mark_remembered_set_roots(&valid_ptrs);
    assert_eq!(stats.old_objects_considered, 1);
    assert_eq!(stats.dirty_objects_scanned, 1);
    assert_eq!(stats.dirty_slot_ranges_scanned, 1);
    assert!(
        stats.dirty_slots_scanned <= 512,
        "one dirty page should scan at most one 4 KiB page of u64 slots"
    );
    unsafe {
        let dirty_header = header_from_user_ptr(dirty_child as *const u8);
        let clean_header = header_from_user_ptr(clean_child as *const u8);
        assert_ne!((*dirty_header).gc_flags & GC_FLAG_MARKED, 0);
        assert_eq!((*clean_header).gc_flags & GC_FLAG_MARKED, 0);
    }
    clear_marks();
    remembered_set_clear();
}

#[test]
fn test_dirty_page_scan_ignores_clean_old_pages() {
    reset_remembered_set();
    clear_marks();
    let dirty_child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let clean_child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let (dirty_obj, dirty_fields) = unsafe { alloc_old_test_object(2048) };
    let dirty_idx = unsafe { field_index_not_on_last_page(dirty_fields, 2048) };
    let dirty_slot = unsafe { dirty_fields.add(dirty_idx) };
    unsafe {
        *dirty_slot = POINTER_TAG | dirty_child as u64;
    }
    let (_clean_obj, clean_fields) = unsafe { alloc_old_test_object(2048) };
    let clean_idx = unsafe { field_index_not_on_last_page(clean_fields, 2048) };
    unsafe {
        *clean_fields.add(clean_idx) = POINTER_TAG | clean_child as u64;
    }

    js_write_barrier_slot(
        POINTER_TAG | dirty_obj as u64,
        dirty_slot as u64,
        POINTER_TAG | dirty_child as u64,
    );

    let valid_ptrs = build_valid_pointer_set();
    let stats = mark_remembered_set_roots(&valid_ptrs);
    assert_eq!(stats.dirty_pages_scanned, 1);
    assert_eq!(
        stats.old_objects_considered, 1,
        "clean old pages must not feed objects into the dirty scan"
    );
    assert_eq!(stats.dirty_objects_scanned, 1);
    assert_eq!(stats.dirty_slot_ranges_scanned, 1);
    assert!(
        stats.dirty_slots_scanned <= 512,
        "one dirty field page should not scan the whole old object"
    );
    unsafe {
        let dirty_header = header_from_user_ptr(dirty_child as *const u8);
        let clean_header = header_from_user_ptr(clean_child as *const u8);
        assert_ne!((*dirty_header).gc_flags & GC_FLAG_MARKED, 0);
        assert_eq!(
            (*clean_header).gc_flags & GC_FLAG_MARKED,
            0,
            "young child stored only on a clean old page should not be marked"
        );
    }
    clear_marks();
    remembered_set_clear();
}

#[test]
fn test_dirty_page_scan_dedupes_object_spanning_dirty_pages() {
    reset_remembered_set();
    clear_marks();
    let young_a = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let young_b = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let (old_obj, fields) = unsafe { alloc_old_test_object(2048) };
    let (idx_a, idx_b) = unsafe { field_indices_on_distinct_pages(fields, 2048) };
    let slot_a = unsafe { fields.add(idx_a) };
    let slot_b = unsafe { fields.add(idx_b) };
    unsafe {
        *slot_a = POINTER_TAG | young_a as u64;
        *slot_b = POINTER_TAG | young_b as u64;
    }

    js_write_barrier_slot(
        POINTER_TAG | old_obj as u64,
        slot_a as u64,
        POINTER_TAG | young_a as u64,
    );
    js_write_barrier_slot(
        POINTER_TAG | old_obj as u64,
        slot_b as u64,
        POINTER_TAG | young_b as u64,
    );

    let valid_ptrs = build_valid_pointer_set();
    let stats = mark_remembered_set_roots(&valid_ptrs);
    assert_eq!(stats.dirty_pages_scanned, 2);
    assert_eq!(
        stats.old_objects_considered, 1,
        "one object spanning two dirty pages should be considered once"
    );
    assert_eq!(stats.dirty_objects_scanned, 1);
    assert_eq!(stats.dirty_slot_pages_considered, 2);
    assert!(stats.dirty_slot_ranges_scanned <= 2);
    assert!(
        stats.dirty_slots_scanned <= 1024,
        "two dirty field pages should bound scanning to two pages"
    );
    assert_eq!(stats.newly_marked, 2);
    unsafe {
        let header_a = header_from_user_ptr(young_a as *const u8);
        let header_b = header_from_user_ptr(young_b as *const u8);
        assert_ne!((*header_a).gc_flags & GC_FLAG_MARKED, 0);
        assert_ne!((*header_b).gc_flags & GC_FLAG_MARKED, 0);
    }
    clear_marks();
    remembered_set_clear();
}

#[test]
fn test_dirty_page_map_entry_scan_is_external_range_bounded() {
    reset_remembered_set();
    clear_marks();
    let dirty_child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let clean_child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let (map, entries, layout) = unsafe { alloc_old_test_map(2048) };
    unsafe {
        (*map).size = 2048;
    }
    let (dirty_idx, clean_idx) = unsafe { field_indices_on_distinct_pages(entries, 4096) };
    let dirty_slot = unsafe { entries.add(dirty_idx) };
    unsafe {
        *dirty_slot = POINTER_TAG | dirty_child as u64;
        *entries.add(clean_idx) = POINTER_TAG | clean_child as u64;
    }
    write_barrier_slot_inner(
        POINTER_TAG | map as u64,
        dirty_slot as usize,
        POINTER_TAG | dirty_child as u64,
        true,
    );

    let valid_ptrs = build_valid_pointer_set();
    let stats = mark_remembered_set_roots(&valid_ptrs);
    assert_eq!(stats.dirty_pages_scanned, 1);
    assert_eq!(stats.old_objects_considered, 1);
    assert_eq!(stats.dirty_objects_scanned, 1);
    assert_eq!(stats.dirty_slot_ranges_scanned, 1);
    assert!(
        stats.dirty_slots_scanned <= 512,
        "one dirty map entries page should not scan the whole map"
    );
    unsafe {
        let dirty_header = header_from_user_ptr(dirty_child as *const u8);
        let clean_header = header_from_user_ptr(clean_child as *const u8);
        assert_ne!((*dirty_header).gc_flags & GC_FLAG_MARKED, 0);
        assert_eq!((*clean_header).gc_flags & GC_FLAG_MARKED, 0);
        retire_old_test_map(map, entries, layout);
    }
    clear_marks();
    remembered_set_clear();
}

#[test]
fn test_dirty_lazy_array_external_cache_scan_marks_bitmap_selected_child() {
    reset_remembered_set();
    clear_marks();

    let cached_length = 4usize;
    let lazy = crate::arena::arena_alloc_gc_old(
        std::mem::size_of::<crate::json_tape::LazyArrayHeader>(),
        8,
        GC_TYPE_LAZY_ARRAY,
    ) as *mut crate::json_tape::LazyArrayHeader;
    let cache_bytes = cached_length * std::mem::size_of::<crate::value::JSValue>();
    let cache =
        crate::arena::arena_alloc_gc(cache_bytes, 8, GC_TYPE_STRING) as *mut crate::value::JSValue;
    let bitmap =
        crate::arena::arena_alloc_gc(std::mem::size_of::<u64>(), 8, GC_TYPE_STRING) as *mut u64;
    let selected_child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let unselected_child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;

    unsafe {
        std::ptr::write_bytes(cache as *mut u8, 0, cache_bytes);
        *bitmap = 0;
        (*lazy).cached_length = cached_length as u32;
        (*lazy).magic = crate::json_tape::LAZY_ARRAY_MAGIC;
        (*lazy).root_idx = 0;
        (*lazy).tape_len = 0;
        (*lazy).blob_str = std::ptr::null();
        (*lazy).materialized = std::ptr::null_mut();
        (*lazy).materialized_elements = cache;
        (*lazy).materialized_bitmap = bitmap;
        (*lazy).walk_idx = u32::MAX;
        (*lazy).walk_tape_pos = 0;
        (*lazy).cumulative_walk_steps = 0;

        *(cache.add(1) as *mut u64) = ptr_bits(selected_child);
        *(cache.add(2) as *mut u64) = ptr_bits(unselected_child);
        *bitmap = 1u64 << 1;
    }

    let lazy_header = unsafe { header_from_user_ptr(lazy as *const u8) };
    let dirty_cache_page = crate::arena::generation_page_for_addr(unsafe { cache.add(1) } as usize);
    assert!(mark_dirty_external_slot_page(
        lazy_header as usize,
        dirty_cache_page
    ));

    let valid_ptrs = build_valid_pointer_set();
    let stats = mark_remembered_set_roots(&valid_ptrs);
    assert_eq!(stats.old_objects_considered, 1);
    assert_eq!(stats.dirty_objects_scanned, 1);
    assert_eq!(
        stats.newly_marked, 1,
        "external lazy-array cache page should mark bitmap-selected nursery values"
    );
    unsafe {
        let selected_header = header_from_user_ptr(selected_child as *const u8);
        let unselected_header = header_from_user_ptr(unselected_child as *const u8);
        assert_ne!((*selected_header).gc_flags & GC_FLAG_MARKED, 0);
        assert_eq!(
            (*unselected_header).gc_flags & GC_FLAG_MARKED,
            0,
            "unset cache bitmap entries must not be treated as live slots"
        );
    }

    clear_marks();
    remembered_set_clear();
}

#[test]
fn test_dirty_page_map_external_dedupes_and_clears() {
    reset_remembered_set();
    let young = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let (map, entries, layout) = unsafe { alloc_old_test_map(16) };
    unsafe {
        (*map).size = 16;
        *entries.add(1) = POINTER_TAG | young as u64;
    }
    let slot = unsafe { entries.add(1) };
    write_barrier_slot_inner(
        POINTER_TAG | map as u64,
        slot as usize,
        POINTER_TAG | young as u64,
        true,
    );
    write_barrier_slot_inner(
        POINTER_TAG | map as u64,
        slot as usize,
        POINTER_TAG | young as u64,
        true,
    );
    assert_eq!(remembered_set_size(), 1);
    remembered_set_clear();
    assert_eq!(remembered_set_size(), 0);
    unsafe {
        retire_old_test_map(map, entries, layout);
    }
}

#[test]
fn test_dirty_page_map_realloc_span_marks_new_entries_pages() {
    reset_remembered_set();
    clear_marks();
    let young = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let (map, entries, layout) = unsafe { alloc_old_test_map(1024) };
    unsafe {
        (*map).size = 1024;
        *entries.add(1023) = POINTER_TAG | young as u64;
    }
    let new_layout = std::alloc::Layout::from_size_align(2048 * 16, 8).unwrap();
    let new_entries = unsafe { std::alloc::alloc_zeroed(new_layout) as *mut u64 };
    assert!(!new_entries.is_null());
    unsafe {
        std::ptr::copy_nonoverlapping(entries, new_entries, 2048);
        (*map).entries = new_entries as *mut f64;
        (*map).capacity = 2048;
    }
    dirty_external_slot_span(map as usize, new_entries as usize, 2048);

    let valid_ptrs = build_valid_pointer_set();
    let stats = mark_remembered_set_roots(&valid_ptrs);
    assert!(stats.dirty_pages_scanned >= 1);
    assert_eq!(stats.old_objects_considered, 1);
    assert_eq!(stats.newly_marked, 1);
    unsafe {
        let header = header_from_user_ptr(young as *const u8);
        assert_ne!((*header).gc_flags & GC_FLAG_MARKED, 0);
        retire_old_test_map(map, new_entries, new_layout);
        std::alloc::dealloc(entries as *mut u8, layout);
    }
    clear_marks();
    remembered_set_clear();
}

#[test]
fn test_rewrite_remembered_dirty_range_updates_unmarked_old_parent_slot() {
    reset_remembered_set();
    clear_marks();
    let child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let (old_obj, fields) = unsafe { alloc_old_test_object(1) };
    unsafe {
        *fields = POINTER_TAG | child as u64;
    }
    js_write_barrier_slot(
        POINTER_TAG | old_obj as u64,
        fields as u64,
        POINTER_TAG | child as u64,
    );
    let valid_ptrs = build_valid_pointer_set();
    let new_child = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_OBJECT) as usize;
    unsafe {
        set_forwarding_address(
            header_from_user_ptr(child as *const u8),
            new_child as *mut u8,
        );
        let old_header = header_from_user_ptr(old_obj as *const u8);
        assert_eq!(
            (*old_header).gc_flags & GC_FLAG_MARKED,
            0,
            "test must prove dirty rewrite does not depend on marked parent walk"
        );
    }

    rewrite_remembered_dirty_ranges(&valid_ptrs);

    unsafe {
        assert_eq!(
            *fields,
            POINTER_TAG | new_child as u64,
            "dirty old parent slot should be rewritten even when parent is unmarked"
        );
    }
    remembered_set_clear();
}

#[test]
fn test_rewrite_remembered_dirty_range_updates_map_external_entry_span() {
    reset_remembered_set();
    clear_marks();
    let dirty_child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let clean_child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let (map, entries, layout) = unsafe { alloc_old_test_map(2048) };
    unsafe {
        (*map).size = 2048;
    }
    let (dirty_idx, clean_idx) = unsafe { field_indices_on_distinct_pages(entries, 4096) };
    let dirty_slot = unsafe { entries.add(dirty_idx) };
    unsafe {
        *dirty_slot = POINTER_TAG | dirty_child as u64;
        *entries.add(clean_idx) = POINTER_TAG | clean_child as u64;
    }
    write_barrier_slot_inner(
        POINTER_TAG | map as u64,
        dirty_slot as usize,
        POINTER_TAG | dirty_child as u64,
        true,
    );
    let valid_ptrs = build_valid_pointer_set();
    let new_dirty_child = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_OBJECT) as usize;
    let new_clean_child = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_OBJECT) as usize;
    unsafe {
        set_forwarding_address(
            header_from_user_ptr(dirty_child as *const u8),
            new_dirty_child as *mut u8,
        );
        set_forwarding_address(
            header_from_user_ptr(clean_child as *const u8),
            new_clean_child as *mut u8,
        );
    }

    rewrite_remembered_dirty_ranges(&valid_ptrs);

    unsafe {
        assert_eq!(*dirty_slot, POINTER_TAG | new_dirty_child as u64);
        assert_eq!(
            *entries.add(clean_idx),
            POINTER_TAG | clean_child as u64,
            "external dirty rewrite should stay bounded to the logged entry page"
        );
        retire_old_test_map(map, entries, layout);
    }
    remembered_set_clear();
}

#[test]
fn test_rewrite_remembered_fallback_header_updates_fields() {
    reset_remembered_set();
    clear_marks();
    let child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let (old_obj, fields) = unsafe { alloc_old_test_object(1) };
    unsafe {
        *fields = POINTER_TAG | child as u64;
    }
    REMEMBERED_SET.with(|s| {
        s.borrow_mut().insert(old_obj as usize - GC_HEADER_SIZE);
    });
    let valid_ptrs = build_valid_pointer_set();
    let new_child = crate::arena::arena_alloc_gc_old(40, 8, GC_TYPE_OBJECT) as usize;
    unsafe {
        set_forwarding_address(
            header_from_user_ptr(child as *const u8),
            new_child as *mut u8,
        );
    }

    rewrite_remembered_dirty_ranges(&valid_ptrs);

    unsafe {
        assert_eq!(*fields, POINTER_TAG | new_child as u64);
    }
    remembered_set_clear();
}

#[test]
fn test_object_hashset_fallback_still_scans() {
    reset_remembered_set();
    clear_marks();
    let (old_obj, _fields) = unsafe { alloc_old_test_object(1) };
    let old_header = old_obj as usize - GC_HEADER_SIZE;
    REMEMBERED_SET.with(|s| {
        s.borrow_mut().insert(old_header);
    });
    let valid_ptrs = build_valid_pointer_set();
    let stats = mark_remembered_set_roots(&valid_ptrs);
    assert_eq!(stats.entries_scanned, 1);
    assert_eq!(stats.valid_roots, 1);
    assert_eq!(stats.newly_marked, 1);
    unsafe {
        let header = header_from_user_ptr(old_obj as *const u8);
        assert_ne!((*header).gc_flags & GC_FLAG_MARKED, 0);
    }
    clear_marks();
    remembered_set_clear();
}

#[test]
fn test_gc_collect_minor_keeps_dirty_page_child_alive() {
    reset_remembered_set();
    let young = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let (old_obj, fields) = unsafe { alloc_old_test_object(1) };
    unsafe {
        *fields = POINTER_TAG | young as u64;
    }
    js_write_barrier_slot(
        POINTER_TAG | old_obj as u64,
        fields as u64,
        POINTER_TAG | young as u64,
    );
    let _ = gc_collect_minor();
    unsafe {
        let child_header = header_from_user_ptr(young as *const u8);
        assert_ne!(
            (*child_header).gc_flags & GC_FLAG_HAS_SURVIVED,
            0,
            "dirty-page remembered scan should keep the young child alive through minor GC"
        );
    }
    remembered_set_clear();
}

#[test]
fn test_minor_gc_promotes_after_two_survivals() {
    reset_remembered_set();
    // Allocate an arena object and pin it so it survives every GC.
    let user_ptr = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    unsafe {
        let header = header_from_user_ptr(user_ptr);
        (*header).gc_flags |= GC_FLAG_PINNED;
        // Initial state: not yet survived, not tenured.
        assert_eq!((*header).gc_flags & GC_FLAG_HAS_SURVIVED, 0);
        assert_eq!((*header).gc_flags & GC_FLAG_TENURED, 0);
    }
    // First minor GC: object survives, gets HAS_SURVIVED bit.
    let _ = gc_collect_minor();
    unsafe {
        let header = header_from_user_ptr(user_ptr);
        assert_ne!(
            (*header).gc_flags & GC_FLAG_HAS_SURVIVED,
            0,
            "first survival should set HAS_SURVIVED"
        );
        assert_eq!(
            (*header).gc_flags & GC_FLAG_TENURED,
            0,
            "first survival should not yet tenure"
        );
    }
    // Second minor GC: HAS_SURVIVED + survives → TENURED, clear HAS_SURVIVED.
    let _ = gc_collect_minor();
    unsafe {
        let header = header_from_user_ptr(user_ptr);
        assert_ne!(
            (*header).gc_flags & GC_FLAG_TENURED,
            0,
            "second survival should tenure"
        );
        assert_eq!(
            (*header).gc_flags & GC_FLAG_HAS_SURVIVED,
            0,
            "tenuring should clear HAS_SURVIVED"
        );
    }
    // Third minor GC: stays tenured idempotently.
    let _ = gc_collect_minor();
    unsafe {
        let header = header_from_user_ptr(user_ptr);
        assert_ne!(
            (*header).gc_flags & GC_FLAG_TENURED,
            0,
            "tenured stays tenured across subsequent collections"
        );
    }
}

#[test]
fn test_forwarding_pointer_roundtrip() {
    // Allocate a nursery object, simulate evacuation by copying
    // its bytes into an old-gen alloc, install the forwarding
    // address in the nursery header. Read back via
    // forwarding_address to confirm round-trip.
    let nursery_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let old_user = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
    unsafe {
        // Pre-condition: not forwarded yet.
        let nursery_hdr = header_from_user_ptr(nursery_user);
        assert_eq!((*nursery_hdr).gc_flags & GC_FLAG_FORWARDED, 0);
        // Install forwarding pointer.
        set_forwarding_address(nursery_hdr as *mut GcHeader, old_user);
        // Post-condition: flag set, address readable.
        assert_ne!((*nursery_hdr).gc_flags & GC_FLAG_FORWARDED, 0);
        assert_eq!(forwarding_address(nursery_hdr), old_user);
    }
}

#[test]
fn test_forwarding_does_not_disturb_other_flags() {
    // Setting FORWARDED must preserve every other gc_flags bit.
    let user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let old = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
    unsafe {
        let hdr = header_from_user_ptr(user) as *mut GcHeader;
        // Set a few unrelated flags.
        (*hdr).gc_flags |= GC_FLAG_MARKED | GC_FLAG_TENURED | GC_FLAG_HAS_SURVIVED;
        let before = (*hdr).gc_flags;
        set_forwarding_address(hdr, old);
        let after = (*hdr).gc_flags;
        assert_eq!(after & GC_FLAG_FORWARDED, GC_FLAG_FORWARDED);
        // Every bit that was set before stays set.
        assert_eq!(
            after & before,
            before,
            "forwarding installation cleared an existing flag"
        );
    }
}

#[test]
fn test_forwarding_pointer_value_is_8_bytes_at_user_offset_zero() {
    // The forwarding pointer is stored in the first 8 bytes of
    // the user payload. This invariant is load-bearing for any
    // future walker that wants to skip over forwarded objects
    // by reading the new address inline. Verify by direct
    // pointer arithmetic.
    let nursery_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let target = 0x12345678_9ABCDEF0_u64 as *mut u8;
    unsafe {
        let hdr = header_from_user_ptr(nursery_user) as *mut GcHeader;
        set_forwarding_address(hdr, target);
        // Read directly: user_ptr cast to *const *mut u8.
        let raw = nursery_user as *const *mut u8;
        assert_eq!(*raw, target);
    }
}

#[test]
fn test_rewrite_mutable_root_slots_updates_shadow_and_global_roots() {
    let _guard = ShadowAndGlobalRootResetGuard;
    reset_shadow_stack();
    reset_global_roots();

    let nursery_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let valid_ptrs = build_valid_pointer_set();
    let old_user = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
    unsafe {
        let nursery_hdr = header_from_user_ptr(nursery_user) as *mut GcHeader;
        set_forwarding_address(nursery_hdr, old_user);
    }

    let shadow_bits = POINTER_TAG | ((nursery_user as u64) & POINTER_MASK);
    let expected_shadow_bits = POINTER_TAG | ((old_user as u64) & POINTER_MASK);
    let shadow = js_shadow_frame_push(1);
    js_shadow_slot_set(0, shadow_bits);

    let mut global_bits = nursery_user as u64;
    js_gc_register_global_root((&mut global_bits as *mut u64) as i64);

    rewrite_mutable_root_slots(&valid_ptrs, None);

    assert_eq!(
        js_shadow_slot_get(0),
        expected_shadow_bits,
        "shadow stack slot should be rewritten to the forwarding target"
    );
    assert_eq!(
        global_bits, old_user as u64,
        "registered global root slot should be rewritten in place"
    );

    js_shadow_frame_pop(shadow);
}

#[test]
fn test_rewrite_mutable_root_slots_follows_forwarding_chain() {
    let _guard = ShadowAndGlobalRootResetGuard;
    reset_shadow_stack();

    let first = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let second = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let valid_ptrs = build_valid_pointer_set();
    let final_user = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
    unsafe {
        set_forwarding_address(header_from_user_ptr(first) as *mut GcHeader, second);
        set_forwarding_address(header_from_user_ptr(second) as *mut GcHeader, final_user);
    }

    let shadow_bits = POINTER_TAG | (first as u64 & POINTER_MASK);
    let expected_bits = POINTER_TAG | (final_user as u64 & POINTER_MASK);
    let shadow = js_shadow_frame_push(1);
    js_shadow_slot_set(0, shadow_bits);

    rewrite_mutable_root_slots(&valid_ptrs, None);

    assert_eq!(
        js_shadow_slot_get(0),
        expected_bits,
        "shadow stack slot should be rewritten through every forwarding hop"
    );

    js_shadow_frame_pop(shadow);
}

#[test]
fn test_runtime_root_visitor_marks_and_rewrites_nanbox_slot() {
    let nursery_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let valid_ptrs = build_valid_pointer_set();
    let old_user = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
    let nursery_hdr = unsafe { header_from_user_ptr(nursery_user) as *mut GcHeader };
    unsafe {
        set_forwarding_address(nursery_hdr, old_user);
    }

    let mut slot = f64::from_bits(POINTER_TAG | (nursery_user as u64 & POINTER_MASK));
    RuntimeRootVisitor::for_mark(&valid_ptrs).visit_nanbox_f64_slot(&mut slot);
    unsafe {
        assert_ne!((*nursery_hdr).gc_flags & GC_FLAG_MARKED, 0);
    }

    RuntimeRootVisitor::for_rewrite(&valid_ptrs).visit_nanbox_f64_slot(&mut slot);
    assert_eq!(
        slot.to_bits(),
        POINTER_TAG | (old_user as u64 & POINTER_MASK)
    );
}

#[test]
fn test_runtime_root_visitor_rewrites_raw_pointer_slots() {
    let nursery_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let valid_ptrs = build_valid_pointer_set();
    let old_user = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
    unsafe {
        set_forwarding_address(
            header_from_user_ptr(nursery_user) as *mut GcHeader,
            old_user,
        );
    }

    let mut mut_ptr = nursery_user;
    let mut const_ptr = nursery_user as *const u8;
    let mut usize_slot = nursery_user as usize;
    let mut i64_slot = nursery_user as i64;

    let mut visitor = RuntimeRootVisitor::for_rewrite(&valid_ptrs);
    visitor.visit_raw_mut_ptr_slot(&mut mut_ptr);
    visitor.visit_raw_const_ptr_slot(&mut const_ptr);
    visitor.visit_usize_slot(&mut usize_slot);
    visitor.visit_i64_slot(&mut i64_slot);

    assert_eq!(mut_ptr, old_user);
    assert_eq!(const_ptr, old_user as *const u8);
    assert_eq!(usize_slot, old_user as usize);
    assert_eq!(i64_slot, old_user as i64);
}

#[test]
fn test_runtime_root_visitor_rewrites_cell_and_atomic_slots() {
    let nursery_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let valid_ptrs = build_valid_pointer_set();
    let old_user = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
    unsafe {
        set_forwarding_address(
            header_from_user_ptr(nursery_user) as *mut GcHeader,
            old_user,
        );
    }

    let cell = Cell::new(f64::from_bits(
        POINTER_TAG | (nursery_user as u64 & POINTER_MASK),
    ));
    let atomic = std::sync::atomic::AtomicPtr::new(nursery_user);
    let atomic_i64 = std::sync::atomic::AtomicI64::new(nursery_user as i64);

    let mut visitor = RuntimeRootVisitor::for_rewrite(&valid_ptrs);
    visitor.visit_cell_f64_slot(&cell);
    visitor.visit_atomic_raw_mut_ptr_slot(
        &atomic,
        std::sync::atomic::Ordering::Acquire,
        std::sync::atomic::Ordering::Release,
    );
    visitor.visit_atomic_i64_slot(
        &atomic_i64,
        std::sync::atomic::Ordering::Acquire,
        std::sync::atomic::Ordering::Release,
    );

    assert_eq!(
        cell.get().to_bits(),
        POINTER_TAG | (old_user as u64 & POINTER_MASK)
    );
    assert_eq!(atomic.load(std::sync::atomic::Ordering::Acquire), old_user);
    assert_eq!(
        atomic_i64.load(std::sync::atomic::Ordering::Acquire),
        old_user as i64
    );
}

#[test]
fn test_runtime_root_visitor_rewrites_metadata_without_marking() {
    let nursery_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let valid_ptrs = build_valid_pointer_set();
    let old_user = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
    let nursery_hdr = unsafe { header_from_user_ptr(nursery_user) as *mut GcHeader };
    unsafe {
        set_forwarding_address(nursery_hdr, old_user);
    }

    let mut metadata = nursery_user as usize;
    RuntimeRootVisitor::for_mark(&valid_ptrs).visit_metadata_usize_slot(&mut metadata);
    unsafe {
        assert_eq!(
            (*nursery_hdr).gc_flags & GC_FLAG_MARKED,
            0,
            "metadata-only slots must not become roots"
        );
    }

    RuntimeRootVisitor::for_rewrite(&valid_ptrs).visit_metadata_usize_slot(&mut metadata);
    assert_eq!(metadata, old_user as usize);
}

#[test]
fn test_transient_runtime_handle_slots_mark_and_rewrite() {
    clear_marks();
    clear_mark_seeds();

    let nanbox_f64_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let nanbox_u64_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let raw_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let raw_string_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_STRING);
    let heap_word_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let valid_ptrs = build_valid_pointer_set();

    let old_nanbox_f64 = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
    let old_nanbox_u64 = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
    let old_raw = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
    let old_raw_string = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_STRING);
    let old_heap_word = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
    unsafe {
        set_forwarding_address(
            header_from_user_ptr(nanbox_f64_user) as *mut GcHeader,
            old_nanbox_f64,
        );
        set_forwarding_address(
            header_from_user_ptr(nanbox_u64_user) as *mut GcHeader,
            old_nanbox_u64,
        );
        set_forwarding_address(header_from_user_ptr(raw_user) as *mut GcHeader, old_raw);
        set_forwarding_address(
            header_from_user_ptr(raw_string_user) as *mut GcHeader,
            old_raw_string,
        );
        set_forwarding_address(
            header_from_user_ptr(heap_word_user) as *mut GcHeader,
            old_heap_word,
        );
    }

    let scope = RuntimeHandleScope::new();
    let nanbox_f64 = scope.root_nanbox_f64(f64::from_bits(ptr_bits(nanbox_f64_user as usize)));
    let nanbox_u64 = scope.root_nanbox_u64(string_bits(nanbox_u64_user as usize));
    let raw = scope.root_raw_mut_ptr(raw_user);
    let raw_string = scope.root_string_ptr(raw_string_user as *const crate::StringHeader);
    let heap_word = scope.root_heap_word_u64(heap_word_user as u64);

    let mut marker = RuntimeRootVisitor::for_mark(&valid_ptrs);
    scan_runtime_handle_roots_mut(&mut marker);
    unsafe {
        assert_ne!(
            (*header_from_user_ptr(nanbox_f64_user)).gc_flags & GC_FLAG_MARKED,
            0
        );
        assert_ne!(
            (*header_from_user_ptr(nanbox_u64_user)).gc_flags & GC_FLAG_MARKED,
            0
        );
        assert_ne!(
            (*header_from_user_ptr(raw_user)).gc_flags & GC_FLAG_MARKED,
            0
        );
        assert_ne!(
            (*header_from_user_ptr(raw_string_user)).gc_flags & GC_FLAG_MARKED,
            0
        );
        assert_ne!(
            (*header_from_user_ptr(heap_word_user)).gc_flags & GC_FLAG_MARKED,
            0
        );
    }

    let mut rewriter = RuntimeRootVisitor::for_rewrite(&valid_ptrs);
    scan_runtime_handle_roots_mut(&mut rewriter);

    assert_eq!(
        nanbox_f64.get_nanbox_f64().to_bits(),
        ptr_bits(old_nanbox_f64 as usize)
    );
    assert_eq!(
        nanbox_u64.get_nanbox_u64(),
        string_bits(old_nanbox_u64 as usize)
    );
    assert_eq!(raw.get_raw_mut_ptr::<u8>(), old_raw);
    assert_eq!(
        raw_string.get_raw_const_ptr::<crate::StringHeader>() as *mut u8,
        old_raw_string
    );
    assert_eq!(heap_word.get_heap_word_u64(), old_heap_word as u64);
}

#[test]
fn test_transient_runtime_handle_scope_drop_removes_roots() {
    clear_marks();
    clear_mark_seeds();

    let user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let header = unsafe { header_from_user_ptr(user) as *mut GcHeader };
    let valid_ptrs = build_valid_pointer_set();

    {
        let scope = RuntimeHandleScope::new();
        let _handle = scope.root_nanbox_u64(ptr_bits(user as usize));
        assert!(RuntimeHandleScope::active_len_for_tests() > 0);
    }
    assert_eq!(RuntimeHandleScope::active_len_for_tests(), 0);

    let mut marker = RuntimeRootVisitor::for_mark(&valid_ptrs);
    scan_runtime_handle_roots_mut(&mut marker);
    unsafe {
        assert_eq!(
            (*header).gc_flags & GC_FLAG_MARKED,
            0,
            "dropped handle scopes must not retain transient roots"
        );
    }
}

#[test]
fn test_transient_runtime_handle_string_concat_gc() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    gc_register_mutable_root_scanner(scan_runtime_handle_roots_mut);

    let left_bytes = vec![b'a'; 600_000];
    let right_bytes = vec![b'b'; 600_000];
    let left = crate::string::js_string_from_bytes(left_bytes.as_ptr(), left_bytes.len() as u32);
    let right = crate::string::js_string_from_bytes(right_bytes.as_ptr(), right_bytes.len() as u32);

    trigger_guard.make_arena_trigger_due();
    let before = gc_collection_count();
    let result = crate::string::js_string_concat(left, right);

    assert!(
        gc_collection_count() > before,
        "concat allocation should trigger copied-minor GC"
    );
    unsafe {
        assert_eq!((*result).byte_len, 1_200_000);
        let data = (result as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        assert_eq!(*data, b'a');
        assert_eq!(*data.add(599_999), b'a');
        assert_eq!(*data.add(600_000), b'b');
        assert_eq!(*data.add(1_199_999), b'b');
    }
}

#[test]
fn test_transient_runtime_handle_array_push_gc() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    gc_register_mutable_root_scanner(scan_runtime_handle_roots_mut);

    let arr = crate::array::js_array_alloc_with_length(200_000);
    let value = crate::string::js_string_from_bytes(b"array-payload".as_ptr(), 13);
    let value_bits = string_bits(value as usize);

    trigger_guard.make_arena_trigger_due();
    let before = gc_collection_count();
    let grown = crate::array::js_array_push_f64(arr, f64::from_bits(value_bits));

    assert!(
        gc_collection_count() > before,
        "array grow should trigger copied-minor GC"
    );
    unsafe {
        assert_eq!((*grown).length, 200_001);
        let elements =
            (grown as *const u8).add(std::mem::size_of::<crate::ArrayHeader>()) as *const u64;
        let stored = *elements.add(200_000);
        assert_eq!(stored & TAG_MASK, STRING_TAG);
        let stored_ptr = (stored & POINTER_MASK) as *const crate::StringHeader;
        assert_ne!(stored_ptr as usize, value as usize);
        assert_string_bytes(stored_ptr, b"array-payload");
    }
}

#[test]
fn test_transient_runtime_handle_object_set_gc() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    gc_register_mutable_root_scanner(scan_runtime_handle_roots_mut);

    let obj = crate::object::js_object_alloc(0, 1);
    js_shadow_slot_set(0, ptr_bits(obj as usize));
    let key = crate::string::js_string_from_bytes(b"name".as_ptr(), 4);
    let value = crate::string::js_string_from_bytes(b"object-payload".as_ptr(), 14);
    force_next_general_arena_alloc_slow();

    trigger_guard.make_arena_trigger_due();
    let before = gc_collection_count();
    crate::object::js_object_set_field_by_name(
        obj,
        key,
        f64::from_bits(string_bits(value as usize)),
    );

    assert!(
        gc_collection_count() > before,
        "keys-array allocation should trigger copied-minor GC"
    );
    let obj_after = (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::object::ObjectHeader;
    unsafe {
        assert!(!(*obj_after).keys_array.is_null());
        let stored_value = crate::object::js_object_get_field(obj_after, 0).bits();
        assert_eq!(stored_value & TAG_MASK, STRING_TAG);
        let stored_value_ptr = (stored_value & POINTER_MASK) as *const crate::StringHeader;
        assert_ne!(stored_value_ptr as usize, value as usize);
        assert_string_bytes(stored_value_ptr, b"object-payload");

        let key_value = crate::array::js_array_get((*obj_after).keys_array, 0).bits();
        assert_eq!(key_value & TAG_MASK, STRING_TAG);
        let stored_key_ptr = (key_value & POINTER_MASK) as *const crate::StringHeader;
        assert_ne!(stored_key_ptr as usize, key as usize);
        assert_string_bytes(stored_key_ptr, b"name");
    }
}

#[test]
fn test_transient_runtime_handle_closure_captures_gc() {
    extern "C" fn captured_func(_closure: *const crate::closure::ClosureHeader) -> f64 {
        0.0
    }

    let _guard = CopyingNurseryTestGuard::new(0);
    let trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    gc_register_mutable_root_scanner(scan_runtime_handle_roots_mut);
    crate::closure::test_clear_singleton_closure_caches();

    let captured = crate::string::js_string_from_bytes(b"closure-payload".as_ptr(), 15);
    let captures = [string_bits(captured as usize)];

    force_next_general_arena_alloc_slow();
    trigger_guard.make_arena_trigger_due();
    let before = gc_collection_count();
    let closure = crate::closure::js_closure_alloc_with_captures_singleton(
        captured_func as *const u8,
        1,
        captures.as_ptr(),
    );

    assert!(
        gc_collection_count() > before,
        "closure arena allocation should trigger copied-minor GC"
    );
    unsafe {
        let capture_slot = (closure as *const u8)
            .add(std::mem::size_of::<crate::closure::ClosureHeader>())
            as *const u64;
        let stored = *capture_slot;
        assert_eq!(stored & TAG_MASK, STRING_TAG);
        let stored_ptr = (stored & POINTER_MASK) as *const crate::StringHeader;
        assert_ne!(stored_ptr as usize, captured as usize);
        assert_string_bytes(stored_ptr, b"closure-payload");
    }

    let entries =
        crate::closure::test_captured_singleton_closure_cache_entries(captured_func as *const u8);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].0.len(), 1);
    assert_ne!(entries[0].0[0], captures[0]);
    assert_eq!(entries[0].0[0] & TAG_MASK, STRING_TAG);
    crate::closure::test_clear_singleton_closure_caches();
}

#[test]
fn test_promise_iter_result_mutable_scanner_rewrites_slot() {
    let nursery_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let valid_ptrs = build_valid_pointer_set();
    let old_user = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
    unsafe {
        set_forwarding_address(
            header_from_user_ptr(nursery_user) as *mut GcHeader,
            old_user,
        );
    }

    let initial = f64::from_bits(POINTER_TAG | (nursery_user as u64 & POINTER_MASK));
    crate::promise::js_iter_result_set(initial, 0);

    let mut visitor = RuntimeRootVisitor::for_rewrite(&valid_ptrs);
    crate::promise::scan_iter_result_root_mut(&mut visitor);

    assert_eq!(
        crate::promise::js_iter_result_get_value().to_bits(),
        POINTER_TAG | (old_user as u64 & POINTER_MASK)
    );
    crate::promise::js_iter_result_set(0.0, 0);
}

#[test]
fn test_evacuation_verify_detects_stale_forwarded_root_slot() {
    let _guard = ShadowAndGlobalRootResetGuard;
    reset_shadow_stack();
    reset_global_roots();
    let fixture = ForwardedRootFixture::new();
    let shadow = js_shadow_frame_push(1);
    js_shadow_slot_set(0, fixture.nursery_bits);

    assert_panics_with("shadow stack roots", || {
        verify_mutable_root_slots(&fixture.valid_ptrs);
    });

    js_shadow_frame_pop(shadow);
}

#[test]
fn test_evacuation_verify_detects_stale_forwarded_runtime_scanner_slot() {
    let fixture = ForwardedRootFixture::new();
    crate::promise::test_seed_promise_scanner_roots(
        fixture.nursery_user as *mut crate::promise::Promise,
        fixture.nursery_value(),
        fixture.nursery_value(),
        fixture.nursery_user as *const crate::closure::ClosureHeader,
        fixture.nursery_user as *mut crate::promise::Promise,
    );

    assert_panics_with("runtime mutable root scanner", || {
        let mut visitor =
            RuntimeRootVisitor::for_verify(&fixture.valid_ptrs, "runtime mutable root scanner");
        promise_mutable_root_scanner(&mut visitor);
    });

    crate::promise::test_clear_promise_scanner_roots();
}

#[test]
fn test_evacuation_verify_detects_stale_forwarded_dirty_range_slot() {
    reset_remembered_set();
    clear_marks();
    let child = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let (old_obj, fields) = unsafe { alloc_old_test_object(1) };
    let child_bits = POINTER_TAG | (child as u64 & POINTER_MASK);
    unsafe {
        *fields = child_bits;
    }
    js_write_barrier_slot(POINTER_TAG | old_obj as u64, fields as u64, child_bits);
    let valid_ptrs = build_valid_pointer_set();
    let old_child = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
    unsafe {
        set_forwarding_address(header_from_user_ptr(child), old_child);
    }

    assert_panics_with("remembered dirty ranges", || {
        verify_remembered_dirty_ranges(&valid_ptrs);
    });

    remembered_set_clear();
}

#[test]
fn test_evacuation_verify_detects_stale_forwarded_heap_field() {
    clear_marks();
    let fixture = ForwardedRootFixture::new();
    let (old_obj, fields) = unsafe { alloc_old_test_object(1) };
    unsafe {
        *fields = fixture.nursery_bits;
        let header = header_from_user_ptr(old_obj as *const u8);
        (*header).gc_flags |= GC_FLAG_MARKED;
        assert_panics_with("heap fields", || {
            verify_heap_object_fields(header, &fixture.valid_ptrs, "heap fields");
        });
        (*header).gc_flags &= !GC_FLAG_MARKED;
    }
}

#[test]
fn test_evacuation_verify_copy_only_pinned_root_allows_non_forwarded_target() {
    let user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let valid_ptrs = build_valid_pointer_set();
    unsafe {
        (*header_from_user_ptr(user)).gc_flags |= GC_FLAG_PINNED;
    }
    verify_copy_only_scanner_bits(
        POINTER_TAG | (user as u64 & POINTER_MASK),
        &valid_ptrs,
        "copy-only root scanner",
    );
    unsafe {
        (*header_from_user_ptr(user)).gc_flags &= !GC_FLAG_PINNED;
    }
}

#[test]
fn test_evacuation_verify_copy_only_root_rejects_forwarded_target() {
    let fixture = ForwardedRootFixture::new();
    assert_panics_with("copy-only root scanner", || {
        verify_copy_only_scanner_bits(
            fixture.nursery_bits,
            &fixture.valid_ptrs,
            "copy-only root scanner",
        );
    });
}

struct ForwardedRootFixture {
    valid_ptrs: ValidPointerSet,
    nursery_user: *mut u8,
    old_user: *mut u8,
    nursery_bits: u64,
    old_bits: u64,
}

impl ForwardedRootFixture {
    fn new() -> Self {
        let nursery_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
        let valid_ptrs = build_valid_pointer_set();
        let old_user = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
        unsafe {
            set_forwarding_address(
                header_from_user_ptr(nursery_user) as *mut GcHeader,
                old_user,
            );
        }
        Self {
            valid_ptrs,
            nursery_user,
            old_user,
            nursery_bits: POINTER_TAG | (nursery_user as u64 & POINTER_MASK),
            old_bits: POINTER_TAG | (old_user as u64 & POINTER_MASK),
        }
    }

    fn nursery_value(&self) -> f64 {
        f64::from_bits(self.nursery_bits)
    }

    fn old_addr(&self) -> usize {
        self.old_user as usize
    }

    fn nursery_addr(&self) -> usize {
        self.nursery_user as usize
    }

    fn nursery_i64(&self) -> i64 {
        self.nursery_user as i64
    }
}

#[test]
fn test_gc_init_mutable_scanner_families_rewrite_runtime_slots() {
    let fixture = ForwardedRootFixture::new();
    let active_context_handle = -724_331;
    let shape_id = 0x51A9_E001;
    let box_ptr = crate::r#box::js_box_alloc(fixture.nursery_value());

    crate::promise::test_seed_promise_scanner_roots(
        fixture.nursery_user as *mut crate::promise::Promise,
        fixture.nursery_value(),
        fixture.nursery_value(),
        fixture.nursery_user as *const crate::closure::ClosureHeader,
        fixture.nursery_user as *mut crate::promise::Promise,
    );
    crate::timer::test_seed_timer_scanner_roots(
        fixture.nursery_user as *mut crate::promise::Promise,
        fixture.nursery_value(),
        fixture.nursery_i64(),
        fixture.nursery_value(),
        fixture.nursery_value(),
    );
    crate::exception::test_set_exception(fixture.nursery_value());
    crate::async_context::clear_store(active_context_handle);
    crate::async_context::enter_with(active_context_handle, fixture.nursery_value());
    crate::builtins::test_seed_queued_microtask(fixture.nursery_i64(), fixture.nursery_value());
    crate::async_hooks::test_seed_async_hooks_scanner_roots(
        fixture.nursery_user as *const crate::closure::ClosureHeader,
        fixture.nursery_value(),
    );
    crate::object::test_seed_shape_cache_root(
        shape_id,
        fixture.nursery_user as *mut crate::array::ArrayHeader,
    );
    crate::regex::test_set_last_exec_groups(
        fixture.nursery_user as *mut crate::object::ObjectHeader,
    );
    crate::array::test_seed_template_raw_roots(
        fixture.nursery_user as *mut crate::array::ArrayHeader,
        fixture.nursery_user as *mut crate::array::ArrayHeader,
    );
    crate::object::test_seed_transition_cache_root(fixture.nursery_addr());
    crate::object::test_seed_overflow_fields_root(fixture.nursery_addr(), fixture.nursery_bits);
    crate::json::test_seed_parse_roots(
        fixture.nursery_value(),
        fixture.nursery_user as *const crate::string::StringHeader,
    );
    crate::string::test_seed_intern_table_root(fixture.nursery_addr());
    crate::builtins::test_set_console_log_singleton(fixture.nursery_i64());
    crate::node_submodules::test_seed_node_submodule_roots(
        fixture.nursery_user as *mut crate::closure::ClosureHeader,
        fixture.nursery_user as *mut crate::object::ObjectHeader,
        fixture.nursery_user as *mut crate::closure::ClosureHeader,
    );
    crate::promise::js_iter_result_set(fixture.nursery_value(), 0);
    crate::promise::test_seed_async_step_thunk_cache(
        fixture.nursery_addr(),
        fixture.nursery_user as *mut crate::closure::ClosureHeader,
        fixture.nursery_user as *mut crate::closure::ClosureHeader,
    );
    crate::closure::test_clear_singleton_closure_caches();
    crate::closure::test_seed_singleton_closure_cache(
        test_no_capture_singleton_func as *const u8,
        fixture.nursery_user as *mut crate::closure::ClosureHeader,
    );
    crate::closure::test_seed_captured_singleton_closure_cache(
        test_captured_singleton_func as *const u8,
        vec![fixture.nursery_bits],
        fixture.nursery_user as *mut crate::closure::ClosureHeader,
    );
    crate::tui::hooks::test_seed_hook_slot_roots(fixture.nursery_bits);
    crate::tui::state::test_reset_state_slots();
    let tui_state = crate::tui::state::js_perry_tui_state_alloc(fixture.nursery_value());

    let mut visitor = RuntimeRootVisitor::for_rewrite(&fixture.valid_ptrs);
    promise_mutable_root_scanner(&mut visitor);
    timer_mutable_root_scanner(&mut visitor);
    exception_mutable_root_scanner(&mut visitor);
    async_context_mutable_root_scanner(&mut visitor);
    async_hooks_mutable_root_scanner(&mut visitor);
    shape_cache_mutable_root_scanner(&mut visitor);
    crate::regex::scan_last_exec_groups_root_mut(&mut visitor);
    crate::array::scan_template_raw_roots_mut(&mut visitor);
    transition_cache_mutable_root_scanner(&mut visitor);
    overflow_fields_mutable_root_scanner(&mut visitor);
    json_parse_mutable_root_scanner(&mut visitor);
    intern_table_mutable_root_scanner(&mut visitor);
    crate::builtins::scan_console_log_singleton_roots_mut(&mut visitor);
    crate::node_submodules::scan_node_submodule_singleton_roots_mut(&mut visitor);
    crate::r#box::scan_box_roots_mut(&mut visitor);
    crate::promise::scan_iter_result_root_mut(&mut visitor);
    crate::promise::scan_async_step_thunk_cache_mut(&mut visitor);
    crate::closure::scan_singleton_closure_roots_mut(&mut visitor);
    crate::tui::hooks::scan_hook_slot_roots_mut(&mut visitor);
    crate::tui::state::scan_state_slot_roots_mut(&mut visitor);

    let promise = crate::promise::test_promise_scanner_snapshot();
    assert_eq!(promise.task_promise_ptr, fixture.old_addr());
    assert_eq!(promise.task_value_bits, fixture.old_bits);
    assert_eq!(promise.task_context_store_bits, fixture.old_bits);
    assert_eq!(promise.inline_callback_ptr, fixture.old_addr());
    assert_eq!(promise.inline_next_ptr, fixture.old_addr());
    assert_eq!(promise.inline_value_bits, fixture.old_bits);
    assert_eq!(promise.async_step_callback_ptr, fixture.old_addr());
    assert_eq!(promise.async_step_next_ptr, fixture.old_addr());
    assert_eq!(promise.async_step_value_bits, fixture.old_bits);
    assert_eq!(promise.promise_context_key, fixture.old_addr());
    assert_eq!(promise.promise_context_store_bits, fixture.old_bits);
    assert_eq!(promise.scheduled_promise_ptr, fixture.old_addr());
    assert_eq!(promise.scheduled_value_bits, fixture.old_bits);

    let timer = crate::timer::test_timer_scanner_snapshot();
    assert_eq!(timer.timeout_promise_ptr, fixture.old_addr());
    assert_eq!(timer.timeout_value_bits, fixture.old_bits);
    assert_eq!(timer.callback_ptr, fixture.old_addr());
    assert_eq!(timer.callback_arg_bits, fixture.old_bits);
    assert_eq!(timer.callback_context_store_bits, fixture.old_bits);
    assert_eq!(timer.interval_callback_ptr, fixture.old_addr());
    assert_eq!(timer.interval_context_store_bits, fixture.old_bits);

    assert_eq!(
        crate::exception::js_get_exception().to_bits(),
        fixture.old_bits
    );
    assert_eq!(
        crate::async_context::get_store(active_context_handle)
            .map(f64::to_bits)
            .unwrap_or(0),
        fixture.old_bits
    );
    assert_eq!(
        crate::builtins::test_queued_microtask_snapshot(),
        (fixture.old_addr(), fixture.old_bits)
    );
    assert_eq!(
        crate::async_hooks::test_async_hooks_scanner_snapshot(),
        (fixture.old_addr(), fixture.old_bits)
    );
    assert_eq!(
        crate::object::test_shape_cache_root(shape_id),
        (fixture.old_addr(), fixture.old_addr())
    );
    assert_eq!(crate::regex::test_last_exec_groups(), fixture.old_addr());
    assert_eq!(
        crate::array::test_template_raw_roots(),
        (fixture.old_addr(), fixture.old_addr())
    );
    assert_eq!(
        crate::object::test_transition_cache_root(),
        fixture.old_addr()
    );
    assert_eq!(
        crate::object::test_overflow_fields_root(),
        (fixture.old_addr(), fixture.old_bits)
    );
    assert_eq!(
        crate::json::test_parse_roots_snapshot(),
        (fixture.old_bits, fixture.old_addr())
    );
    assert_eq!(crate::string::test_intern_table_root(), fixture.old_addr());
    assert_eq!(
        crate::builtins::test_console_log_singleton() as usize,
        fixture.old_addr()
    );
    assert_eq!(
        crate::node_submodules::test_node_submodule_roots(),
        (fixture.old_addr(), fixture.old_addr(), fixture.old_addr())
    );
    assert_eq!(
        crate::r#box::js_box_get(box_ptr).to_bits(),
        fixture.old_bits
    );
    assert_eq!(
        crate::promise::js_iter_result_get_value().to_bits(),
        fixture.old_bits
    );
    assert_eq!(
        crate::promise::test_async_step_thunk_cache(),
        (fixture.old_addr(), fixture.old_addr(), fixture.old_addr())
    );
    assert_eq!(
        crate::closure::test_singleton_closure_cache_entry(
            test_no_capture_singleton_func as *const u8
        )
        .map(|ptr| ptr as usize),
        Some(fixture.old_addr())
    );
    assert_eq!(
        crate::closure::test_captured_singleton_closure_cache_entries(
            test_captured_singleton_func as *const u8
        ),
        vec![(
            vec![fixture.old_bits],
            fixture.old_user as *mut crate::closure::ClosureHeader
        )]
    );
    assert_eq!(
        crate::tui::hooks::test_hook_slot_roots(),
        (fixture.old_bits, fixture.old_bits, fixture.old_bits)
    );
    assert_eq!(
        crate::tui::state::js_perry_tui_state_get(tui_state).to_bits(),
        fixture.old_bits
    );

    crate::promise::test_clear_promise_scanner_roots();
    crate::timer::test_clear_timer_scanner_roots(fixture.nursery_addr(), fixture.old_addr());
    crate::exception::js_clear_exception();
    crate::async_context::clear_store(active_context_handle);
    crate::object::test_clear_transition_cache_root();
    crate::string::test_clear_intern_table_root();
    crate::builtins::test_set_console_log_singleton(0);
    crate::async_hooks::reset_for_tests();
    crate::promise::js_iter_result_set(0.0, 0);
    crate::closure::test_clear_singleton_closure_caches();
    crate::tui::state::test_reset_state_slots();
}

#[cfg(feature = "ohos-napi")]
#[test]
fn test_arkts_callbacks_mutable_scanner_rewrites_callback_slots() {
    let fixture = ForwardedRootFixture::new();
    let callback_idx = 3;
    crate::arkts_callbacks::test_clear_arkts_callback_roots();
    crate::arkts_callbacks::test_seed_arkts_callback_root(callback_idx, fixture.nursery_value());

    let mut visitor = RuntimeRootVisitor::for_rewrite(&fixture.valid_ptrs);
    crate::arkts_callbacks::arkts_callbacks_root_scanner_mut(&mut visitor);

    assert_eq!(
        crate::arkts_callbacks::test_arkts_callback_root(callback_idx),
        fixture.old_bits
    );
    crate::arkts_callbacks::test_clear_arkts_callback_roots();
}

#[cfg(feature = "ohos-napi")]
#[test]
fn test_lazy_media_mutable_scanner_rewrites_callback_slots() {
    let fixture = ForwardedRootFixture::new();
    let handle = i64::MIN + 377;
    crate::media_playback::test_seed_media_callback_roots(
        handle,
        fixture.nursery_value(),
        fixture.nursery_value(),
    );

    let mut visitor = RuntimeRootVisitor::for_rewrite(&fixture.valid_ptrs);
    crate::media_playback::media_callbacks_root_scanner_mut(&mut visitor);

    assert_eq!(
        crate::media_playback::test_media_callback_roots(handle),
        (fixture.old_bits, fixture.old_bits)
    );
}

#[test]
fn test_cons_pinned_cleared_after_minor_gc() {
    // Allocate something to give the GC sweep work to do.
    let _ = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    // Pre-populate CONS_PINNED to simulate a prior GC's leftover.
    CONS_PINNED.with(|s| {
        s.borrow_mut().insert(0xDEAD_BEEF);
    });
    assert!(cons_pinned_count() >= 1);
    let _ = gc_collect_minor();
    assert_eq!(
        cons_pinned_count(),
        0,
        "minor GC must clear CONS_PINNED after collection"
    );
}

#[test]
fn test_pin_currently_marked_captures_marked_objects() {
    // Manually mark an arena object, then run the pinning
    // scan. The pinned set should contain the marked header.
    CONS_PINNED.with(|s| s.borrow_mut().clear());
    clear_marks();
    let user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let header = unsafe { header_from_user_ptr(user) as *mut GcHeader };
    unsafe {
        (*header).gc_flags |= GC_FLAG_MARKED;
    }
    let stats = pin_currently_marked_as_conservative();
    assert!(
        is_conservatively_pinned(header),
        "marked header should land in CONS_PINNED"
    );
    assert_eq!(stats.pinned_roots, 1);
    assert_eq!(stats.pinned_bytes, unsafe { (*header).size as usize });
    // Cleanup for test isolation.
    unsafe {
        (*header).gc_flags &= !GC_FLAG_MARKED;
    }
    CONS_PINNED.with(|s| s.borrow_mut().clear());
}

#[test]
fn test_pin_currently_marked_skips_unmarked() {
    CONS_PINNED.with(|s| s.borrow_mut().clear());
    clear_marks();
    let user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let header = unsafe { header_from_user_ptr(user) as *const GcHeader };
    // Ensure unmarked.
    unsafe {
        assert_eq!((*(header as *mut GcHeader)).gc_flags & GC_FLAG_MARKED, 0);
    }
    let stats = pin_currently_marked_as_conservative();
    assert_eq!(stats.pinned_roots, 0);
    assert_eq!(stats.pinned_bytes, 0);
    assert!(
        !is_conservatively_pinned(header),
        "unmarked header should NOT land in CONS_PINNED"
    );
}

#[test]
fn test_conservative_pin_stats_exclude_legacy_copy_only_scanner_pins() {
    CONS_PINNED.with(|s| s.borrow_mut().clear());
    clear_marks();
    let conservative_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let legacy_user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let conservative_header = unsafe { header_from_user_ptr(conservative_user) as *mut GcHeader };
    let legacy_header = unsafe { header_from_user_ptr(legacy_user) as *mut GcHeader };
    unsafe {
        (*conservative_header).gc_flags |= GC_FLAG_MARKED;
    }

    let stats = pin_currently_marked_as_conservative();
    let conservative_bytes = unsafe { (*conservative_header).size as usize };
    assert_eq!(stats.pinned_roots, 1);
    assert_eq!(stats.pinned_bytes, conservative_bytes);

    let valid_ptrs = build_valid_pointer_set();
    let legacy_bits = POINTER_TAG | (legacy_user as u64 & POINTER_MASK);
    let legacy_bytes = mark_copy_only_scanner_bits(legacy_bits, &valid_ptrs, true);
    assert_eq!(
        legacy_bytes,
        Some(unsafe { (*legacy_header).size as usize })
    );
    assert_eq!(
        cons_pinned_count(),
        2,
        "evacuation set still contains both conservative and legacy pins"
    );
    assert_eq!(
        stats.pinned_roots, 1,
        "conservative pin stats must not absorb later legacy scanner pins"
    );
    assert_eq!(stats.pinned_bytes, conservative_bytes);

    clear_marks();
    CONS_PINNED.with(|s| s.borrow_mut().clear());
}

#[test]
fn test_evacuation_policy() {
    fn snapshot(
        tenured: usize,
        candidate: usize,
        candidate_objects: usize,
        pinned: usize,
        rss: u64,
        previous_pause_us: u64,
        pre_evac_pause_us: u64,
    ) -> EvacuationPolicySnapshot {
        EvacuationPolicySnapshot {
            tenured_still_in_nursery_bytes: tenured,
            candidate_bytes: candidate,
            candidate_objects,
            reclaimable_candidate_bytes: candidate,
            reclaimable_candidate_objects: candidate_objects,
            conservative_pinned_bytes: pinned,
            rss_bytes: rss,
            previous_pause_us,
            pre_evac_pause_us,
            ..EvacuationPolicySnapshot::default()
        }
    }

    fn decide(
        snapshot: EvacuationPolicySnapshot,
        considered: bool,
        force: bool,
    ) -> EvacuationPolicyDecision {
        evacuation_policy_final_decision(
            EvacuationPolicyDecision {
                allowed: true,
                considered,
                force,
                enabled: false,
                reason: "test",
                snapshot,
            },
            snapshot,
        )
    }

    let zero_candidates = decide(
        snapshot(MIN_TENURED_NURSERY_BYTES, 0, 0, 0, 0, 0, 0),
        true,
        false,
    );
    assert!(!zero_candidates.enabled);
    assert_eq!(zero_candidates.reason, "zero_candidates");

    let productive = decide(
        snapshot(
            MIN_TENURED_NURSERY_BYTES * 2,
            MIN_CANDIDATE_BYTES * 2,
            2,
            0,
            0,
            0,
            0,
        ),
        true,
        false,
    );
    assert!(productive.enabled);
    assert_eq!(productive.reason, "nursery_pressure");

    let rss_pressure = decide(
        snapshot(
            MIN_CANDIDATE_BYTES,
            MIN_CANDIDATE_BYTES,
            1,
            0,
            RSS_PRESSURE_BYTES,
            0,
            0,
        ),
        true,
        false,
    );
    assert!(rss_pressure.enabled);
    assert_eq!(rss_pressure.reason, "rss_pressure");

    let pinned_dominated = decide(
        snapshot(
            MIN_TENURED_NURSERY_BYTES * 4,
            MIN_CANDIDATE_BYTES,
            1,
            MIN_TENURED_NURSERY_BYTES * 3,
            0,
            0,
            0,
        ),
        true,
        false,
    );
    assert!(!pinned_dominated.enabled);
    assert_eq!(
        pinned_dominated.reason,
        "reclaimable_candidate_ratio_below_threshold"
    );

    let retained_stub_dominated = decide(
        EvacuationPolicySnapshot {
            tenured_still_in_nursery_bytes: MIN_TENURED_NURSERY_BYTES * 2,
            candidate_bytes: MIN_CANDIDATE_BYTES * 2,
            candidate_objects: 16,
            reclaimable_candidate_bytes: 0,
            reclaimable_candidate_objects: 0,
            retained_forwarded_stub_bytes: 64,
            retained_forwarded_stub_objects: 1,
            conservative_pinned_bytes: 0,
            rss_bytes: 0,
            previous_pause_us: 0,
            pre_evac_pause_us: 0,
            ..EvacuationPolicySnapshot::default()
        },
        true,
        false,
    );
    assert!(
        !retained_stub_dominated.enabled,
        "movable bytes alone must not enable evacuation when retained forwarded stubs keep the candidate blocks live"
    );
    assert_eq!(
        retained_stub_dominated.reason,
        "zero_reclaimable_candidates"
    );

    let pause_skip = decide(
        snapshot(
            MIN_TENURED_NURSERY_BYTES,
            MIN_CANDIDATE_BYTES,
            1,
            0,
            0,
            MAX_PREVIOUS_PAUSE_US + 1,
            0,
        ),
        true,
        false,
    );
    assert!(!pause_skip.enabled);
    assert_eq!(pause_skip.reason, "pause_budget_exceeded");

    let hard_rss_override = decide(
        snapshot(
            MIN_TENURED_NURSERY_BYTES,
            MIN_CANDIDATE_BYTES,
            1,
            0,
            RSS_HARD_PRESSURE_BYTES,
            MAX_PREVIOUS_PAUSE_US + 1,
            0,
        ),
        true,
        false,
    );
    assert!(hard_rss_override.enabled);
    assert_eq!(hard_rss_override.reason, "rss_hard_pressure");

    let force = decide(snapshot(0, 64, 1, 0, 0, 0, 0), true, true);
    assert!(force.enabled);
    assert_eq!(force.reason, "force");

    let low_pressure =
        evacuation_policy_initial_decision(0, RSS_PRESSURE_BYTES - 1, 0, 0, true, false, true, 0);
    assert!(!low_pressure.considered);
    assert!(!low_pressure.enabled);
    assert_eq!(low_pressure.reason, "low_pressure");

    let pressure_barriers_inactive = evacuation_policy_initial_decision(
        MIN_TENURED_NURSERY_BYTES,
        RSS_HARD_PRESSURE_BYTES,
        0,
        0,
        true,
        false,
        false,
        0,
    );
    assert!(!pressure_barriers_inactive.considered);
    assert!(!pressure_barriers_inactive.enabled);
    assert_eq!(pressure_barriers_inactive.reason, "barriers_inactive");

    let force_barriers_inactive =
        evacuation_policy_initial_decision(0, 0, 0, 0, true, true, false, 1);
    assert!(force_barriers_inactive.force);
    assert!(!force_barriers_inactive.considered);
    assert!(!force_barriers_inactive.enabled);
    assert_eq!(force_barriers_inactive.reason, "barriers_inactive");

    let disabled = evacuation_policy_initial_decision(
        MIN_TENURED_NURSERY_BYTES,
        RSS_HARD_PRESSURE_BYTES,
        0,
        0,
        false,
        true,
        false,
        0,
    );
    assert!(!disabled.considered);
    assert!(!disabled.enabled);
    assert_eq!(disabled.reason, "disabled");
}

#[test]
fn test_evacuation_policy_snapshot_excludes_retained_forwarded_stub_blocks() {
    clear_marks();
    CONS_PINNED.with(|s| s.borrow_mut().clear());

    let mut pair = None;
    for _ in 0..64 {
        let candidate = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT) as usize;
        let stub = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_ARRAY) as usize;
        let candidate_block = arena_block_index_for_user(candidate);
        let stub_block = arena_block_index_for_user(stub);
        if candidate_block.is_some()
            && candidate_block == stub_block
            && candidate_block.unwrap() < crate::arena::general_block_count()
        {
            pair = Some((candidate, stub));
            break;
        }
    }
    let (candidate, stub) =
        pair.expect("test setup should find two nursery allocations in one general block");
    let candidate_header = unsafe { header_from_user_ptr(candidate as *const u8) };
    let stub_header = unsafe { header_from_user_ptr(stub as *const u8) };
    let stub_target = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_ARRAY);
    unsafe {
        (*candidate_header).gc_flags |= GC_FLAG_MARKED | GC_FLAG_TENURED;
        set_forwarding_address(stub_header, stub_target);
    }

    let old_page_selection = OldPageDefragSelection::default();
    let snapshot = evacuation_policy_snapshot_after_mark(
        EvacuationPolicySnapshot::default(),
        false,
        0,
        &old_page_selection,
    );
    let candidate_size = unsafe { (*candidate_header).size as usize };
    let stub_size = unsafe { (*stub_header).size as usize };
    assert!(
        snapshot.candidate_bytes >= candidate_size,
        "marked tenured object should be a movable candidate"
    );
    assert_eq!(
        snapshot.reclaimable_candidate_bytes, 0,
        "candidate sharing a block with a retained forwarded stub is not block-reclaimable"
    );
    assert!(
        snapshot.retained_forwarded_stub_bytes >= stub_size,
        "policy snapshot should report retained forwarded stubs that keep blocks live"
    );

    unsafe {
        (*candidate_header).gc_flags &= !(GC_FLAG_MARKED | GC_FLAG_TENURED);
        (*stub_header).gc_flags &= !GC_FLAG_FORWARDED;
    }
    CONS_PINNED.with(|s| s.borrow_mut().clear());
}

#[test]
fn test_evacuate_tenured_skips_pinned() {
    // An object that's MARKED + TENURED + CONS_PINNED must
    // NOT be evacuated.
    CONS_PINNED.with(|s| s.borrow_mut().clear());
    let user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let header = unsafe { header_from_user_ptr(user) as *mut GcHeader };
    unsafe {
        (*header).gc_flags |= GC_FLAG_MARKED | GC_FLAG_TENURED;
    }
    // Pin it.
    CONS_PINNED.with(|s| s.borrow_mut().insert(header as usize));
    let n = evacuate_tenured_nursery_objects();
    assert_eq!(n.objects, 0, "pinned tenured object must not be evacuated");
    unsafe {
        assert_eq!(
            (*header).gc_flags & GC_FLAG_FORWARDED,
            0,
            "FORWARDED flag must not be set on pinned object"
        );
    }
    // Cleanup
    unsafe {
        (*header).gc_flags &= !(GC_FLAG_MARKED | GC_FLAG_TENURED);
    }
    CONS_PINNED.with(|s| s.borrow_mut().clear());
}

#[test]
fn test_evacuate_tenured_skips_unmarked() {
    // TENURED but not MARKED → dead this cycle, sweep handles it.
    // Evacuation must skip.
    CONS_PINNED.with(|s| s.borrow_mut().clear());
    let user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let header = unsafe { header_from_user_ptr(user) as *mut GcHeader };
    unsafe {
        (*header).gc_flags |= GC_FLAG_TENURED; // no MARK
    }
    let _n = evacuate_tenured_nursery_objects();
    unsafe {
        assert_eq!(
            (*header).gc_flags & GC_FLAG_FORWARDED,
            0,
            "unmarked object must not be evacuated"
        );
    }
    unsafe {
        (*header).gc_flags &= !GC_FLAG_TENURED;
    }
}

#[test]
fn test_evacuate_tenured_marks_forwarded_and_copies_payload() {
    // The happy path: marked + tenured + not pinned → evacuated.
    // Verify (a) GC_FLAG_FORWARDED set on nursery header,
    // (b) forwarding_address points into OLD_ARENA,
    // (c) payload bytes copied.
    CONS_PINNED.with(|s| s.borrow_mut().clear());
    let user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let header = unsafe { header_from_user_ptr(user) as *mut GcHeader };
    // Write a sentinel pattern into the user payload so we can
    // confirm it survives the copy.
    unsafe {
        let p = user as *mut u64;
        *p = 0xCAFE_BABE_DEAD_BEEF;
        *p.add(1) = 0x1234_5678_9ABC_DEF0;
        (*header).gc_flags |= GC_FLAG_MARKED | GC_FLAG_TENURED;
    }
    let n = evacuate_tenured_nursery_objects();
    assert_eq!(
        n.objects, 1,
        "tenured non-pinned marked object must evacuate"
    );
    unsafe {
        assert_ne!((*header).gc_flags & GC_FLAG_FORWARDED, 0);
        let new_user = forwarding_address(header);
        // Verify old_user points into nursery, new_user points into OLD.
        assert!(
            crate::arena::pointer_in_old_gen(new_user as usize),
            "forwarding address should point into OLD_ARENA"
        );
        assert!(
            !crate::arena::pointer_in_old_gen(user as usize),
            "old (nursery) location should NOT be in OLD_ARENA"
        );
        // Verify payload was copied.
        let new_p = new_user as *const u64;
        // Note: payload starts at user_ptr offset 0, but the
        // forwarding write at the OLD slot overwrites the first 8
        // bytes with the new address. So the payload at the OLD
        // location is partially clobbered now — we can only
        // verify the NEW location's payload.
        assert_eq!(*new_p, 0xCAFE_BABE_DEAD_BEEF);
        assert_eq!(*new_p.add(1), 0x1234_5678_9ABC_DEF0);
    }
    unsafe {
        (*header).gc_flags &= !(GC_FLAG_MARKED | GC_FLAG_TENURED);
    }
}

#[test]
fn test_release_evacuated_original_forwarding_stub_before_sweep() {
    CONS_PINNED.with(|s| s.borrow_mut().clear());
    clear_marks();
    let user = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let header = unsafe { header_from_user_ptr(user) as *mut GcHeader };
    unsafe {
        (*header).gc_flags |= GC_FLAG_MARKED | GC_FLAG_TENURED;
    }
    let total = unsafe { (*header).size as usize };
    let mut evacuated_new_headers = Vec::new();
    let mut evacuated_original_headers = Vec::new();
    let moved = evacuate_tenured_nursery_objects_collecting(
        false,
        &mut evacuated_new_headers,
        &mut evacuated_original_headers,
    );
    assert_eq!(moved.moved_objects, 1);
    assert_eq!(moved.moved_bytes, total);
    assert_eq!(evacuated_original_headers, vec![header]);
    unsafe {
        assert_ne!(
            (*header).gc_flags & GC_FLAG_FORWARDED,
            0,
            "evacuation must install a forwarding stub for rewrite"
        );
    }

    let released = release_evacuated_original_forwarding_stubs(&evacuated_original_headers);
    assert_eq!(released.released_original_objects, 1);
    assert_eq!(released.released_original_bytes, total);
    assert_eq!(released.released_original_reusable_bytes, 0);
    assert_eq!(released.released_original_returned_bytes, 0);
    unsafe {
        assert_eq!(
            (*header).gc_flags & GC_FLAG_FORWARDED,
            0,
            "GC-evacuation originals should release FORWARDED before sweep"
        );
    }

    let sweep = sweep_with_age_bump(false);
    assert_eq!(sweep.dead_bytes, sweep.freed_bytes);
    assert!(
        sweep.freed_bytes >= total as u64,
        "released evacuation original should contribute to sweep reclaimable bytes"
    );
    CONS_PINNED.with(|s| s.borrow_mut().clear());
}

#[test]
fn test_sweep_reports_and_retains_non_evacuation_forwarded_stub() {
    clear_marks();
    let stub = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_ARRAY);
    let target = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_ARRAY);
    let stub_header = unsafe { header_from_user_ptr(stub) as *mut GcHeader };
    let total = unsafe { (*stub_header).size as usize };
    unsafe {
        set_forwarding_address(stub_header, target);
        (*stub_header).gc_flags |= GC_FLAG_MARKED;
    }
    for _ in 0..90_000 {
        let _ = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    }

    let sweep = sweep_with_age_bump(false);
    assert!(
        sweep.retained_forwarded_stub_objects >= 1,
        "sweep should count retained non-evacuation forwarding stubs"
    );
    assert!(
        sweep.retained_forwarded_stub_bytes >= total,
        "sweep should report bytes retained by non-evacuation forwarding stubs"
    );
    unsafe {
        assert_ne!(
            (*stub_header).gc_flags & GC_FLAG_FORWARDED,
            0,
            "sweep must not clear array-growth forwarding stubs"
        );
        (*stub_header).gc_flags &= !GC_FLAG_FORWARDED;
    }
}

#[test]
fn test_sweep_reclaims_unreached_old_forwarded_stub() {
    clear_marks();
    let stub = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_ARRAY);
    let target = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_ARRAY);
    let stub_header = unsafe { header_from_user_ptr(stub) as *mut GcHeader };
    let total = unsafe { (*stub_header).size as usize };
    unsafe {
        set_forwarding_address(stub_header, target);
    }
    for _ in 0..90_000 {
        let _ = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    }

    let sweep = sweep_with_age_bump(false);
    assert!(
        sweep.freed_bytes >= total as u64,
        "unreached old forwarding stub should be reclaimable"
    );
    unsafe {
        assert_eq!(
            (*stub_header).gc_flags & GC_FLAG_FORWARDED,
            0,
            "sweep should release stale unreachable forwarding stubs"
        );
    }
}

#[test]
fn test_forced_evacuation_barriers_inactive_does_not_forward_candidate() {
    struct ResetGcTestState;

    impl Drop for ResetGcTestState {
        fn drop(&mut self) {
            reset_shadow_stack();
            reset_global_roots();
            reset_remembered_set();
            clear_marks();
            clear_mark_seeds();
            CONS_PINNED.with(|s| s.borrow_mut().clear());
        }
    }

    let _reset = ResetGcTestState;
    let _isolation = copying_nursery_isolation_lock();
    let _barrier_guard = GeneratedWriteBarrierTestGuard::inactive();
    reset_shadow_stack();
    reset_global_roots();
    reset_remembered_set();
    clear_marks();
    clear_mark_seeds();
    CONS_PINNED.with(|s| s.borrow_mut().clear());
    if !gc_force_evacuate_enabled() {
        return;
    }
    assert!(
        !generated_write_barriers_emitted(),
        "this canary must verify the barriers-inactive evacuation gate"
    );

    let frame = js_shadow_frame_push(1);
    let (parent, _) = unsafe { alloc_nursery_test_object(0) };
    let parent_user = parent as usize;
    let parent_header = unsafe { header_from_user_ptr(parent as *const u8) };

    unsafe {
        (*parent_header).gc_flags |= GC_FLAG_TENURED;
    }
    js_shadow_slot_set(0, ptr_bits(parent_user));

    let _ = gc_collect_minor();

    let parent_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_eq!(
        parent_after, parent_user,
        "forced evacuation must not move candidates when generated barriers are inactive"
    );
    unsafe {
        assert_eq!(
            (*parent_header).gc_flags & GC_FLAG_FORWARDED,
            0,
            "barriers-inactive policy gate must leave the nursery candidate unforwarded"
        );
    }

    js_shadow_frame_pop(frame);
}

#[test]
fn test_evacuated_old_parent_re_remembers_young_child_canary() {
    struct ResetGcTestState;

    impl Drop for ResetGcTestState {
        fn drop(&mut self) {
            reset_shadow_stack();
            reset_global_roots();
            reset_remembered_set();
            clear_marks();
            clear_mark_seeds();
            CONS_PINNED.with(|s| s.borrow_mut().clear());
        }
    }

    let _reset = ResetGcTestState;
    let _isolation = copying_nursery_isolation_lock();
    let _barrier_guard = GeneratedWriteBarrierTestGuard::active();
    reset_shadow_stack();
    reset_global_roots();
    reset_remembered_set();
    clear_marks();
    clear_mark_seeds();
    CONS_PINNED.with(|s| s.borrow_mut().clear());
    if !gc_force_evacuate_enabled() {
        return;
    }
    assert!(
        generated_write_barriers_emitted(),
        "this canary must exercise policy evacuation with generated barriers active"
    );

    let frame = js_shadow_frame_push(1);
    let (parent, fields) = unsafe { alloc_nursery_test_object(1) };
    let child = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let parent_user = parent as usize;
    let parent_header = unsafe { header_from_user_ptr(parent as *const u8) };
    let child_header = unsafe { header_from_user_ptr(child as *const u8) };
    let _copy_only_root_guard = TemporaryCopyOnlyRootScanner::rust_bits(&[ptr_bits(child)]);

    unsafe {
        *fields = ptr_bits(child);
        (*parent_header).gc_flags |= GC_FLAG_TENURED;
    }
    js_shadow_slot_set(0, ptr_bits(parent_user));
    CONS_PINNED.with(|s| {
        s.borrow_mut().insert(child_header as usize);
    });

    let _ = gc_collect_minor();

    let parent_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_ne!(
        parent_after, parent_user,
        "rooted parent should be rewritten to its evacuated old-gen copy"
    );
    assert!(
        crate::arena::pointer_in_old_gen(parent_after),
        "evacuated parent should live in old-gen"
    );
    unsafe {
        assert_eq!(
            (*parent_header).gc_flags & GC_FLAG_FORWARDED,
            0,
            "original nursery parent should release its GC forwarding pointer after rewrite"
        );
    }

    let parent_after_fields = unsafe {
        (parent_after as *mut u8).add(std::mem::size_of::<crate::object::ObjectHeader>())
            as *mut u64
    };
    let child_after = unsafe { (*parent_after_fields & POINTER_MASK) as usize };
    assert_eq!(
        child_after, child,
        "evacuated parent should still point at the pinned nursery child"
    );
    assert!(
        crate::arena::pointer_in_nursery(child_after),
        "child should remain young after parent evacuation"
    );

    assert!(
        remembered_set_size() > 0,
        "evacuated old parent retaining a nursery child must be re-remembered after the collection clear"
    );

    clear_marks();
    let valid_ptrs = build_valid_pointer_set();
    let stats = mark_remembered_set_roots(&valid_ptrs);
    assert!(
        stats.newly_marked > 0,
        "remembered scan should mark the nursery child reachable only from the evacuated old parent"
    );
    unsafe {
        assert_ne!(
            (*child_header).gc_flags & GC_FLAG_MARKED,
            0,
            "remembered scan should mark the pinned nursery child"
        );
    }

    clear_marks();
    CONS_PINNED.with(|s| {
        s.borrow_mut().insert(child_header as usize);
    });
    let _ = gc_collect_minor();

    let parent_after_second = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    assert_eq!(
        parent_after_second, parent_after,
        "second minor GC should keep using the evacuated old parent"
    );
    let child_after_second = unsafe { (*parent_after_fields & POINTER_MASK) as usize };
    assert_eq!(
        child_after_second, child,
        "second minor GC should keep the nursery child alive through the rebuilt remembered entry"
    );
    unsafe {
        assert_ne!(
            (*child_header).gc_flags & GC_FLAG_TENURED,
            0,
            "second minor GC should mark and age the nursery child"
        );
    }

    js_shadow_frame_pop(frame);
}

#[test]
fn test_gc_collect_minor_runs_without_panic() {
    // Smoke test: minor GC over an arena with a mix of nursery
    // and old-gen objects must complete without panic. Real
    // correctness is checked by the broader regression suite
    // (test_json_*.ts under PERRY_GEN_GC=1).
    let _y1 = crate::arena::arena_alloc_gc(64, 8, GC_TYPE_OBJECT);
    let _y2 = crate::arena::arena_alloc_gc(32, 8, GC_TYPE_STRING);
    let _o1 = crate::arena::arena_alloc_gc_old(64, 8, GC_TYPE_OBJECT);
    let _o2 = crate::arena::arena_alloc_gc_old(48, 8, GC_TYPE_ARRAY);
    let _ = gc_collect_minor();
    // Following collection runs interleave nicely (cleared marks).
    let _ = gc_collect_minor();
    let _ = gc_collect_minor();
}

#[test]
fn test_remembered_set_cleared_after_full_gc() {
    reset_remembered_set();
    // Set up an old→young edge to populate the RS.
    let young = crate::arena::arena_alloc_gc(40, 8, GC_TYPE_OBJECT) as usize;
    let (old, fields) = unsafe { alloc_old_test_object(1) };
    unsafe {
        *fields = POINTER_TAG | young as u64;
    }
    js_write_barrier_slot(
        POINTER_TAG | old as u64,
        fields as u64,
        POINTER_TAG | young as u64,
    );
    assert_eq!(remembered_set_size(), 1);
    // Run a full collection.
    let _freed = gc_collect_inner();
    // RS must be empty after collection — coherence invariant.
    assert_eq!(
        remembered_set_size(),
        0,
        "remembered set must be cleared after gc_collect_inner"
    );
}

#[test]
fn test_clear_marks_resets_all() {
    // Allocate and mark some objects
    let ptr1 = gc_malloc(32, GC_TYPE_STRING);
    let ptr2 = gc_malloc(64, GC_TYPE_CLOSURE);

    unsafe {
        init_test_closure(ptr2);
        (*header_from_user_ptr(ptr1)).gc_flags |= GC_FLAG_MARKED;
        (*header_from_user_ptr(ptr2)).gc_flags |= GC_FLAG_MARKED;
    }

    clear_marks();

    unsafe {
        assert_eq!(
            (*header_from_user_ptr(ptr1)).gc_flags & GC_FLAG_MARKED,
            0,
            "mark should be cleared on ptr1"
        );
        assert_eq!(
            (*header_from_user_ptr(ptr2)).gc_flags & GC_FLAG_MARKED,
            0,
            "mark should be cleared on ptr2"
        );
    }
}

/// Issue #856 regression: `mark_stack_roots` performs a `setjmp`
/// into a `u64` register-snapshot buffer, and `promise.rs` does a
/// `setjmp` into an `i32` trap buffer. Both used to declare their
/// own conflicting `extern "C" fn setjmp(...)` — the Rust compiler
/// emitted `clashing_extern_declarations`, and on platforms where
/// the ABI didn't happen to round-trip the bits the behaviour was
/// UB. The fix routes both through `crate::ffi::setjmp::setjmp`
/// with a libc-matching `*mut c_int` signature; this test exists
/// to make sure the GC stack-scan path keeps running without
/// crashing now that the extern is shared.
///
/// `gc_collect_inner` invokes `mark_stack_roots`, which is the
/// real production setjmp call site. The matching promise.rs
/// trap path is exercised by `crate::ffi::setjmp::tests` and by
/// any test that drains microtasks; the regression here is
/// specifically the GC half of the pair.
#[test]
fn test_issue_856_setjmp_stack_scan_does_not_crash() {
    // A few allocations so `mark_stack_roots` actually has
    // pointers to consider; the test is about the setjmp not
    // crashing, not about a specific mark outcome.
    let _ptr1 = gc_malloc(32, GC_TYPE_STRING);
    let ptr2 = gc_malloc(48, GC_TYPE_CLOSURE);
    let _ptr3 = gc_malloc(16, GC_TYPE_BIGINT);
    unsafe {
        init_test_closure(ptr2);
    }

    // Should complete cleanly. If the shared `_setjmp` extern is
    // mis-sized, libc will scribble past the 256-byte buffer in
    // `mark_stack_roots` and corrupt this frame's stack — the
    // test would crash long before reaching the assert.
    gc_collect_inner();

    // Sanity: GC ran (count advanced). We don't assert anything
    // about WHICH allocations survived — that's covered by other
    // tests.
    let count = GC_STATS.with(|s| s.borrow().collection_count);
    assert!(count > 0, "gc_collect_inner should bump collection_count");
}
