use super::*;

// This file used to exercise the `PERRY_UNBOXED_OBJECT_FIELDS` prototype's
// `js_gc_init_unboxed_object_layout` installer. That prototype was deleted
// (Phase 4b cleanup): its write path was bit-identical to the default
// typed-shape path and its read side was never implemented. The coverage the
// file carried for behavior that still exists moved onto the load-bearing
// `js_gc_init_typed_shape_layout` installer: raw-numeric scan skipping, the
// copying-minor descriptor transfer, and descriptor invalidation on every
// shape-changing mutation (dynamic add, delete, defineProperty, accessors).
// The prototype-only twins (zero-raw-field scan, pointer-write fallback,
// `layout_transfer` on move) were dropped — `typed_shape.rs` and
// `object_closure_slots.rs` already pin those paths for the default installer.
// Charter step 5: the raw-numeric scan skipping and the copy transfer are now
// the shape's `F64` lanes (the collector traces an object by its shape).

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
    crate::object::js_object_set_field(obj, 0, crate::value::JSValue::number(1.25));
    crate::object::js_object_set_field(obj, 1, crate::value::JSValue::number(-2.5));
    // Charter step 5: the raw-numeric fields are the shape's `F64` lanes.
    unsafe { restamp_with_rep(obj, f64_lanes(0..2)) };

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
    crate::object::js_object_set_field(
        obj,
        0,
        crate::value::JSValue::number(f64::from_bits(0x1000)),
    );
    let child = crate::string::js_string_from_bytes(b"mixed-child".as_ptr(), 11);
    crate::object::js_object_set_field(obj, 1, crate::value::JSValue::string_ptr(child));
    // Charter step 5: slot 0 is the shape's `F64` lane, slot 1 stays `Any`.
    unsafe { restamp_with_rep(obj, f64_lanes([0])) };

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
fn test_f64_lanes_move_with_the_shape_on_copying_minor_and_skip_raw_slots() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    let child = young_leaf();
    let obj = crate::object::js_object_alloc(0, 3);
    crate::object::js_object_set_field(obj, 0, crate::value::JSValue::number(10.5));
    crate::object::js_object_set_field(obj, 1, crate::value::JSValue::from_bits(ptr_bits(child)));
    crate::object::js_object_set_field(obj, 2, crate::value::JSValue::number(-3.25));
    // Charter step 5: slots 0 and 2 are the shape's `F64` lanes; the shape
    // moves with the object, so the copy is traced the same way.
    let shape = unsafe { restamp_with_rep(obj, f64_lanes([0, 2])) };
    assert_eq!(test_heap_child_slot_count(obj as *mut u8), 1);
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
    assert_eq!(
        unsafe { crate::object::shapes::object_shape_id(after as *const _) },
        shape,
        "the copy carries the shape, and with it the F64 lanes"
    );
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

#[test]
fn test_heap_child_iterator_all_f64_lane_object_yields_no_child_slots() {
    clear_marks();
    clear_mark_seeds();

    let obj = crate::object::js_object_alloc(0, 3);
    crate::object::js_object_set_field(obj, 0, crate::value::JSValue::number(1.0));
    crate::object::js_object_set_field(obj, 1, crate::value::JSValue::number(2.0));
    crate::object::js_object_set_field(obj, 2, crate::value::JSValue::number(3.0));
    // Charter step 5: an object is traced by its shape; every lane `F64`
    // makes the payload pointer-free.
    unsafe { restamp_with_rep(obj, f64_lanes(0..3)) };

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
