use super::*;

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
