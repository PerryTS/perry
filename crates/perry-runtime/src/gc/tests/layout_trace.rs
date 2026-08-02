use super::super::*;
use super::support::*;
mod array_layout;
mod object_closure_slots;
mod typed_shape;
mod unboxed_object;

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
fn test_pointer_store_restores_side_mask_from_stale_pointer_free() {
    // Regression: an array can end up in POINTER_FREE layout state while
    // still carrying a populated element pointer-mask (e.g. after a
    // numeric-layout rebuild observed a short/empty prefix while the mask
    // lingered). `heap_payload_slot_selection` short-circuits POINTER_FREE and
    // skips the WHOLE payload without consulting the mask, so the evacuating
    // minor would drop live pointer elements. `layout_note_slot` must restore
    // SIDE_MASK whenever it records a pointer into an existing mask.
    clear_marks();
    clear_mark_seeds();

    let child0 = crate::string::js_string_from_bytes(b"c0".as_ptr(), 2) as *mut u8;
    let child1 = crate::string::js_string_from_bytes(b"c1".as_ptr(), 2) as *mut u8;
    let child0_h = unsafe { header_from_user_ptr(child0) };
    let child1_h = unsafe { header_from_user_ptr(child1) };

    let arr = crate::array::js_array_alloc_with_length(2);
    // Store a pointer at index 0 -> SIDE_MASK + mask{0}.
    crate::array::js_array_set_f64(
        arr,
        0,
        f64::from_bits(STRING_TAG | (child0 as u64 & POINTER_MASK)),
    );
    assert_eq!(test_layout_pointer_slot_count(arr as usize, 2), Some(1));

    // Reproduce the stale-state hazard: force POINTER_FREE while the mask{0}
    // entry is still present (`set_layout_state` only touches the state bits).
    let arr_header = unsafe { header_from_user_ptr(arr as *mut u8) };
    unsafe {
        set_layout_state(arr_header, GC_LAYOUT_POINTER_FREE);
    }

    // Store a pointer at index 1 through the normal array store path.
    crate::array::js_array_set_f64(
        arr,
        1,
        f64::from_bits(STRING_TAG | (child1 as u64 & POINTER_MASK)),
    );

    // The fix: recording a pointer must have restored SIDE_MASK, so the mask is
    // consulted and BOTH pointer slots are visible to the tracer.
    unsafe {
        assert_eq!(
            (*arr_header)._reserved & GC_LAYOUT_STATE_MASK,
            GC_LAYOUT_SIDE_MASK,
            "recording a pointer into an existing mask must restore SIDE_MASK"
        );
    }
    assert_eq!(test_layout_pointer_slot_count(arr as usize, 2), Some(2));

    let valid_ptrs = build_valid_pointer_set();
    let mut worklist = Vec::new();
    unsafe {
        trace_array(arr as *mut u8, &valid_ptrs, &mut worklist);
        assert_ne!(
            (*child0_h).gc_flags & GC_FLAG_MARKED,
            0,
            "index 0 pointer must be traced"
        );
        assert_ne!(
            (*child1_h).gc_flags & GC_FLAG_MARKED,
            0,
            "index 1 pointer (stored under stale POINTER_FREE) must be traced"
        );
    }

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
