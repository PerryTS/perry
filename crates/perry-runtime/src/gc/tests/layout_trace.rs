use super::super::*;
use super::support::*;

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
    assert_eq!(crate::array::js_array_mark_numeric_f64_layout(arr), 1);

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
    assert_eq!(crate::array::js_array_mark_numeric_f64_layout(arr), 1);

    let valid_ptrs = build_valid_pointer_set();
    assert!(try_mark_value(
        POINTER_TAG | (arr as u64 & POINTER_MASK),
        &valid_ptrs
    ));
    trace_marked_objects(&valid_ptrs);

    let event = trace.into_json(GcStepSnapshot::current());
    let layout_scans = &event["layout_scans"];
    assert_eq!(layout_scans["pointer_slots_read"].as_u64(), Some(0));
    assert_eq!(layout_scans["pointer_slot_bytes_read"].as_u64(), Some(0));
    assert_eq!(
        layout_scans["pointer_free_ranges_skipped"].as_u64(),
        Some(1)
    );
    assert_eq!(layout_scans["pointer_free_slots_skipped"].as_u64(), Some(4));
    assert_eq!(
        layout_scans["pointer_free_payload_bytes_skipped"].as_u64(),
        Some(32)
    );
    assert_eq!(
        layout_scans["raw_numeric_array_ranges_skipped"].as_u64(),
        Some(1)
    );
    assert_eq!(
        layout_scans["raw_numeric_array_slots_skipped"].as_u64(),
        Some(4)
    );
    assert_eq!(
        layout_scans["raw_numeric_array_payload_bytes_skipped"].as_u64(),
        Some(32)
    );

    clear_marks();
    clear_mark_seeds();
}

#[test]
fn test_layout_scan_trace_json_counts_pointer_slot_bytes() {
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
    let child = crate::string::js_string_from_bytes(b"byte-child".as_ptr(), 10) as *mut u8;
    let arr = crate::array::js_array_alloc_with_length(2);
    crate::array::js_array_set_f64(arr, 0, 1.0);
    crate::array::js_array_set_f64(
        arr,
        1,
        f64::from_bits(STRING_TAG | (child as u64 & POINTER_MASK)),
    );

    let valid_ptrs = build_valid_pointer_set();
    assert!(try_mark_value(
        POINTER_TAG | (arr as u64 & POINTER_MASK),
        &valid_ptrs
    ));
    trace_marked_objects(&valid_ptrs);

    let event = trace.into_json(GcStepSnapshot::current());
    let layout_scans = &event["layout_scans"];
    assert_eq!(layout_scans["pointer_slots_read"].as_u64(), Some(1));
    assert_eq!(layout_scans["pointer_slot_bytes_read"].as_u64(), Some(8));
    assert_eq!(layout_scans["masked_pointer_slots_read"].as_u64(), Some(1));
    assert_eq!(layout_scans["unknown_layout_slots_read"].as_u64(), Some(0));

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
fn test_raw_numeric_array_layout_transfers_on_copying_minor_and_skips_payload() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    let arr = crate::array::js_array_alloc_with_length(4);
    for i in 0..4 {
        crate::array::js_array_set_f64(arr, i, (i + 1) as f64 + 0.25);
    }
    assert_eq!(crate::array::js_array_mark_numeric_f64_layout(arr), 1);
    js_shadow_slot_set(0, ptr_bits(arr as usize));

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    let after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    let header = unsafe { header_from_user_ptr(after as *const u8) };

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_ne!(after, arr as usize);
    assert!(crate::arena::pointer_in_nursery(after));
    unsafe {
        assert_ne!((*header)._reserved & GC_ARRAY_RAW_F64_LAYOUT, 0);
    }
    assert_eq!(test_layout_pointer_slot_count(after, 4), Some(0));
    assert_eq!(test_heap_child_slot_count(after as *mut u8), 0);
    assert!(
        trace.layout_scans.raw_numeric_array_slots_skipped >= 4,
        "copied raw numeric array payload should be skipped by layout scan: {:?}",
        trace.layout_scans
    );
}

#[test]
fn test_layout_mask_small_mixed_array_scans_exact_pointer_slot() {
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

    assert_eq!(test_layout_pointer_slot_count(arr as usize, 3), Some(1));

    let valid_ptrs = build_valid_pointer_set();
    let mut worklist = Vec::new();
    test_reset_trace_slot_reads();
    unsafe {
        trace_array(arr as *mut u8, &valid_ptrs, &mut worklist);
        assert_ne!((*child_header).gc_flags & GC_FLAG_MARKED, 0);
    }
    assert_eq!(test_trace_slot_reads(), 1);

    crate::array::js_array_set_f64(arr, 1, 2.0);
    assert_eq!(test_layout_pointer_slot_count(arr as usize, 3), Some(0));

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

    assert_eq!(test_layout_pointer_slot_count(obj as usize, 3), Some(1));
    let valid_ptrs = build_valid_pointer_set();
    let mut worklist = Vec::new();
    test_reset_trace_slot_reads();
    unsafe {
        trace_object(obj as *mut u8, &valid_ptrs, &mut worklist);
        assert_ne!((*object_child_header).gc_flags & GC_FLAG_MARKED, 0);
    }
    assert_eq!(test_trace_slot_reads(), 1);

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

    assert_eq!(test_layout_pointer_slot_count(closure as usize, 3), Some(1));
    let valid_ptrs = build_valid_pointer_set();
    let mut worklist = Vec::new();
    test_reset_trace_slot_reads();
    unsafe {
        trace_closure(closure as *mut u8, &valid_ptrs, &mut worklist);
        assert_ne!((*closure_child_header).gc_flags & GC_FLAG_MARKED, 0);
    }
    assert_eq!(test_trace_slot_reads(), 1);

    clear_marks();
    clear_mark_seeds();
}

#[test]
fn test_typed_shape_descriptor_preserves_pointer_slots_after_non_pointer_overwrite() {
    clear_marks();
    clear_mark_seeds();

    let obj = crate::object::js_object_alloc(0, 2);
    let mask = [0b10u64];
    js_gc_init_typed_shape_layout(
        obj as u64,
        2,
        std::ptr::null(),
        0,
        mask.as_ptr(),
        mask.len() as u32,
    );

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
    js_gc_init_typed_shape_layout(
        obj as u64,
        2,
        std::ptr::null(),
        0,
        mask.as_ptr(),
        mask.len() as u32,
    );

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
fn test_typed_shape_descriptor_tracks_raw_numeric_slots() {
    clear_marks();
    clear_mark_seeds();

    let obj = crate::object::js_object_alloc(0, 2);
    crate::object::js_object_set_unboxed_f64_field(obj, 0, 1.5);
    crate::object::js_object_set_field(obj, 1, crate::value::JSValue::number(2.5));
    let raw_mask = [0b01u64];
    js_gc_init_typed_shape_layout(
        obj as u64,
        2,
        raw_mask.as_ptr(),
        raw_mask.len() as u32,
        std::ptr::null(),
        0,
    );

    assert!(layout_typed_raw_f64_slot_for_user(obj as usize, 0));
    assert!(!layout_typed_raw_f64_slot_for_user(obj as usize, 1));
    assert_eq!(test_layout_pointer_slot_count(obj as usize, 2), Some(0));

    let child = crate::string::js_string_from_bytes(b"raw-child".as_ptr(), 9);
    let child_header = unsafe { header_from_user_ptr(child as *mut u8) };
    crate::object::js_object_set_field(obj, 0, crate::value::JSValue::string_ptr(child));

    assert!(
        !layout_typed_raw_f64_slot_for_user(obj as usize, 0),
        "non-number writes must clear the exact raw-f64 descriptor"
    );
    assert_eq!(test_layout_pointer_slot_count(obj as usize, 2), None);

    let valid_ptrs = build_valid_pointer_set();
    assert!(try_mark_value(
        POINTER_TAG | (obj as u64 & POINTER_MASK),
        &valid_ptrs
    ));
    trace_marked_objects(&valid_ptrs);
    unsafe {
        assert_ne!((*child_header).gc_flags & GC_FLAG_MARKED, 0);
    }

    clear_marks();
    clear_mark_seeds();
}

#[test]
fn test_typed_shape_raw_numeric_slots_accept_pointer_like_f64_bits() {
    clear_marks();
    clear_mark_seeds();

    let obj = crate::object::js_object_alloc(0, 2);
    let pointer_like_number = f64::from_bits(0x1000);
    crate::object::js_object_set_unboxed_f64_field(obj, 0, pointer_like_number);
    let child = crate::string::js_string_from_bytes(b"mixed-child".as_ptr(), 11);
    crate::object::js_object_set_field(obj, 1, crate::value::JSValue::string_ptr(child));

    let raw_mask = [0b01u64];
    let pointer_mask = [0b10u64];
    js_gc_init_typed_shape_layout(
        obj as u64,
        2,
        raw_mask.as_ptr(),
        raw_mask.len() as u32,
        pointer_mask.as_ptr(),
        pointer_mask.len() as u32,
    );

    assert!(layout_typed_raw_f64_slot_for_user(obj as usize, 0));
    assert_eq!(test_layout_pointer_slot_count(obj as usize, 2), Some(1));

    let next_pointer_like_number = f64::from_bits(0x2000);
    crate::object::js_object_set_unboxed_f64_field(obj, 0, next_pointer_like_number);
    assert!(
        layout_typed_raw_f64_slot_for_user(obj as usize, 0),
        "raw f64 slots must not be downgraded by numeric payload bits that resemble raw pointers"
    );
    assert_eq!(test_layout_pointer_slot_count(obj as usize, 2), Some(1));

    clear_marks();
    clear_mark_seeds();
}

#[test]
fn test_typed_shape_descriptor_rejects_nanbox_non_number_tags() {
    clear_marks();
    clear_mark_seeds();

    let raw_mask = [0b1u64];
    let obj = crate::object::js_object_alloc(0, 1);
    crate::object::js_object_set_unboxed_f64_field(obj, 0, 1.5);
    js_gc_init_typed_shape_layout(
        obj as u64,
        1,
        raw_mask.as_ptr(),
        raw_mask.len() as u32,
        std::ptr::null(),
        0,
    );
    assert!(layout_typed_raw_f64_slot_for_user(obj as usize, 0));

    let short = crate::value::JSValue::try_short_string(b"abc").unwrap();
    crate::object::js_object_set_field(obj, 0, short);
    assert!(
        !layout_typed_raw_f64_slot_for_user(obj as usize, 0),
        "SSO string tags must downgrade raw-f64 descriptors"
    );
    assert_eq!(test_layout_pointer_slot_count(obj as usize, 1), None);

    let handle_obj = crate::object::js_object_alloc(0, 1);
    crate::object::js_object_set_unboxed_f64_field(handle_obj, 0, 2.5);
    js_gc_init_typed_shape_layout(
        handle_obj as u64,
        1,
        raw_mask.as_ptr(),
        raw_mask.len() as u32,
        std::ptr::null(),
        0,
    );
    assert!(layout_typed_raw_f64_slot_for_user(handle_obj as usize, 0));

    let handle = crate::value::JSValue::from_bits(crate::value::JS_HANDLE_TAG | 0x1234);
    crate::object::js_object_set_field(handle_obj, 0, handle);
    assert!(
        !layout_typed_raw_f64_slot_for_user(handle_obj as usize, 0),
        "JS handle tags must downgrade raw-f64 descriptors"
    );
    assert_eq!(test_layout_pointer_slot_count(handle_obj as usize, 1), None);

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
    js_gc_init_typed_shape_layout(obj as u64, 1, std::ptr::null(), 0, std::ptr::null(), 0);

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
    js_gc_init_typed_shape_layout(
        src as u64,
        2,
        std::ptr::null(),
        0,
        mask.as_ptr(),
        mask.len() as u32,
    );

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
fn test_all_pointer_layout_transfers_on_array_move() {
    clear_marks();
    clear_mark_seeds();

    let src = crate::array::js_array_alloc_pointer_elements(2);
    let dst = crate::array::js_array_alloc(2);
    unsafe {
        layout_transfer(src as *mut u8, dst as *mut u8);
    }

    assert_eq!(test_layout_pointer_slot_count(dst as usize, 2), Some(2));

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
fn test_layout_scan_trace_counts_raw_numeric_object_fields() {
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

    let obj = crate::object::js_object_alloc(0, 2);
    crate::object::js_object_set_unboxed_f64_field(obj, 0, 1.25);
    crate::object::js_object_set_unboxed_f64_field(obj, 1, -2.5);
    let raw_mask = [0b11u64];
    js_gc_init_typed_shape_layout(
        obj as u64,
        2,
        raw_mask.as_ptr(),
        raw_mask.len() as u32,
        std::ptr::null(),
        0,
    );

    let valid_ptrs = build_valid_pointer_set();
    assert!(try_mark_value(
        POINTER_TAG | (obj as u64 & POINTER_MASK),
        &valid_ptrs
    ));
    trace_marked_objects(&valid_ptrs);

    let event = trace.into_json(GcStepSnapshot::current());
    let layout_scans = &event["layout_scans"];
    assert_eq!(
        layout_scans["raw_numeric_object_field_ranges_skipped"].as_u64(),
        Some(1)
    );
    assert_eq!(
        layout_scans["raw_numeric_object_field_slots_skipped"].as_u64(),
        Some(2)
    );
    assert_eq!(
        layout_scans["raw_numeric_object_field_payload_bytes_skipped"].as_u64(),
        Some(16)
    );

    clear_marks();
    clear_mark_seeds();
}

#[test]
fn test_layout_scan_trace_counts_mixed_raw_numeric_object_fields() {
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

    let obj = crate::object::js_object_alloc(0, 2);
    crate::object::js_object_set_unboxed_f64_field(obj, 0, f64::from_bits(0x1000));
    let child = crate::string::js_string_from_bytes(b"mixed-child".as_ptr(), 11);
    crate::object::js_object_set_field(obj, 1, crate::value::JSValue::string_ptr(child));
    let raw_mask = [0b01u64];
    let pointer_mask = [0b10u64];
    js_gc_init_typed_shape_layout(
        obj as u64,
        2,
        raw_mask.as_ptr(),
        raw_mask.len() as u32,
        pointer_mask.as_ptr(),
        pointer_mask.len() as u32,
    );

    let valid_ptrs = build_valid_pointer_set();
    assert!(try_mark_value(
        POINTER_TAG | (obj as u64 & POINTER_MASK),
        &valid_ptrs
    ));
    trace_marked_objects(&valid_ptrs);

    let event = trace.into_json(GcStepSnapshot::current());
    let layout_scans = &event["layout_scans"];
    assert_eq!(layout_scans["masked_pointer_slots_read"].as_u64(), Some(1));
    assert_eq!(
        layout_scans["raw_numeric_object_field_ranges_skipped"].as_u64(),
        Some(1)
    );
    assert_eq!(
        layout_scans["raw_numeric_object_field_slots_skipped"].as_u64(),
        Some(1)
    );
    assert_eq!(
        layout_scans["raw_numeric_object_field_payload_bytes_skipped"].as_u64(),
        Some(8)
    );

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
fn test_raw_numeric_object_descriptor_transfers_on_copying_minor_and_skips_raw_slots() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    let child = young_leaf();
    let obj = crate::object::js_object_alloc(0, 3);
    crate::object::js_object_set_unboxed_f64_field(obj, 0, 10.5);
    crate::object::js_object_set_field(obj, 1, crate::value::JSValue::from_bits(ptr_bits(child)));
    crate::object::js_object_set_unboxed_f64_field(obj, 2, -3.25);
    let raw_mask = [0b101u64];
    let pointer_mask = [0b010u64];
    js_gc_init_typed_shape_layout(
        obj as u64,
        3,
        raw_mask.as_ptr(),
        raw_mask.len() as u32,
        pointer_mask.as_ptr(),
        pointer_mask.len() as u32,
    );
    assert!(layout_typed_raw_f64_slot_for_user(obj as usize, 0));
    assert!(layout_typed_raw_f64_slot_for_user(obj as usize, 2));
    assert_eq!(test_layout_pointer_slot_count(obj as usize, 3), Some(1));
    js_shadow_slot_set(0, ptr_bits(obj as usize));

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    let after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    let fields = unsafe {
        (after as *const u8).add(std::mem::size_of::<crate::object::ObjectHeader>()) as *const u64
    };
    let first = f64::from_bits(unsafe { *fields.add(0) });
    let child_after = unsafe { (*fields.add(1) & POINTER_MASK) as usize };
    let third = f64::from_bits(unsafe { *fields.add(2) });

    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
    assert_ne!(after, obj as usize);
    assert_ne!(child_after, child);
    assert!(crate::arena::pointer_in_nursery(after));
    assert!(crate::arena::pointer_in_nursery(child_after));
    assert_eq!(first, 10.5);
    assert_eq!(third, -3.25);
    assert!(layout_typed_raw_f64_slot_for_user(after, 0));
    assert!(layout_typed_raw_f64_slot_for_user(after, 2));
    assert_eq!(test_layout_pointer_slot_count(after, 3), Some(1));
    assert_eq!(test_heap_child_slot_count(after as *mut u8), 1);
    assert!(
        trace.layout_scans.masked_pointer_slots_read >= 1,
        "pointer slot should still be scanned: {:?}",
        trace.layout_scans
    );
    assert!(
        trace.layout_scans.raw_numeric_object_field_slots_skipped >= 2,
        "raw numeric object slots should be skipped: {:?}",
        trace.layout_scans
    );
}

fn unboxed_point_for_shape_change_test(shape_id: u32) -> *mut crate::object::ObjectHeader {
    let packed_keys = b"x\0y\0";
    let obj = crate::object::js_object_alloc_with_shape(
        shape_id,
        2,
        packed_keys.as_ptr(),
        packed_keys.len() as u32,
    );
    crate::object::js_object_set_unboxed_f64_field(obj, 0, 1.0);
    crate::object::js_object_set_unboxed_f64_field(obj, 1, 2.0);
    js_gc_init_unboxed_object_layout(obj as u64, 2, 0b11, 0);
    assert_eq!(test_layout_pointer_slot_count(obj as usize, 2), Some(0));
    obj
}

fn descriptor_object_with_single_field(
    shape_id: u32,
    key: &[u8],
    value: crate::value::JSValue,
) -> *mut crate::object::ObjectHeader {
    let mut packed_key = Vec::with_capacity(key.len() + 1);
    packed_key.extend_from_slice(key);
    packed_key.push(0);
    let desc = crate::object::js_object_alloc_with_shape(
        shape_id,
        1,
        packed_key.as_ptr(),
        packed_key.len() as u32,
    );
    crate::object::js_object_set_field(desc, 0, value);
    desc
}

#[test]
fn test_unboxed_object_dynamic_added_property_falls_back() {
    clear_marks();
    clear_mark_seeds();

    let obj = unboxed_point_for_shape_change_test(86_101);
    let z_key = crate::string::js_string_from_bytes(b"z".as_ptr(), 1);
    crate::object::js_object_set_field_by_name(obj, z_key, 3.0);

    assert_eq!(
        test_layout_pointer_slot_count(obj as usize, 3),
        None,
        "adding a dynamic property must invalidate the exact unboxed shape"
    );

    clear_marks();
    clear_mark_seeds();
}

#[test]
fn test_unboxed_object_delete_falls_back() {
    clear_marks();
    clear_mark_seeds();

    let obj = unboxed_point_for_shape_change_test(86_102);
    let x_key = crate::string::js_string_from_bytes(b"x".as_ptr(), 1);
    assert_eq!(crate::object::js_object_delete_field(obj, x_key), 1);

    assert_eq!(
        test_layout_pointer_slot_count(obj as usize, 1),
        None,
        "delete shifts keys/fields and must invalidate the exact unboxed shape"
    );

    clear_marks();
    clear_mark_seeds();
}

#[test]
fn test_unboxed_object_define_property_falls_back() {
    clear_marks();
    clear_mark_seeds();

    let obj = unboxed_point_for_shape_change_test(86_103);
    let x_key = crate::string::js_string_from_bytes(b"x".as_ptr(), 1);
    let desc =
        descriptor_object_with_single_field(86_104, b"value", crate::value::JSValue::number(9.0));

    crate::object::js_object_define_property(
        crate::value::js_nanbox_pointer(obj as i64),
        f64::from_bits(crate::value::JSValue::string_ptr(x_key).bits()),
        crate::value::js_nanbox_pointer(desc as i64),
    );

    assert_eq!(
        test_layout_pointer_slot_count(obj as usize, 2),
        None,
        "Object.defineProperty must invalidate the exact unboxed shape even for existing keys"
    );

    clear_marks();
    clear_mark_seeds();
}

#[test]
fn test_unboxed_object_accessor_define_property_falls_back() {
    clear_marks();
    clear_mark_seeds();

    let obj = unboxed_point_for_shape_change_test(86_105);
    let x_key = crate::string::js_string_from_bytes(b"x".as_ptr(), 1);
    // #2817: an accessor descriptor's `get` must be callable — a non-function
    // value now throws. Use a real (capture-less) closure as the getter so we
    // still exercise the accessor shape-invalidation path under test.
    let getter = crate::closure::js_closure_alloc(std::ptr::null(), 0);
    let desc = descriptor_object_with_single_field(
        86_106,
        b"get",
        crate::value::JSValue::pointer(getter as *const u8),
    );

    crate::object::js_object_define_property(
        crate::value::js_nanbox_pointer(obj as i64),
        f64::from_bits(crate::value::JSValue::string_ptr(x_key).bits()),
        crate::value::js_nanbox_pointer(desc as i64),
    );

    assert_eq!(
        test_layout_pointer_slot_count(obj as usize, 2),
        None,
        "accessor descriptors must invalidate the exact unboxed shape"
    );

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

    assert_eq!(test_layout_pointer_slot_count(obj as usize, 9), Some(1));
    let valid_ptrs = build_valid_pointer_set();
    let mut worklist = Vec::new();
    unsafe {
        trace_object(obj as *mut u8, &valid_ptrs, &mut worklist);
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
    js_gc_init_typed_shape_layout(
        obj as u64,
        3,
        std::ptr::null(),
        0,
        mask.as_ptr(),
        mask.len() as u32,
    );

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
    js_gc_init_typed_shape_layout(obj as u64, 2, std::ptr::null(), 0, std::ptr::null(), 0);
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
