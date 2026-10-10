//! Exact moving windows behind the raw-handle custody repairs.
use super::*;
use crate::closure::{ClosureHeader, JsThis};
use crate::value::JSValue;

fn pointer_value<T>(ptr: *const T) -> f64 {
    crate::value::js_nanbox_pointer(ptr as i64)
}

#[test]
fn buffer_json_key_moves_between_key_and_value_allocation() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    register_runtime_handle_root_scanner_for_tests();
    let buffer = crate::buffer::js_buffer_alloc(3, 0);
    for (index, byte) in [37, 91, 7].into_iter().enumerate() {
        crate::buffer::js_buffer_set(buffer, index as i32, byte);
    }
    let mut allocations = 0;
    let result =
        crate::buffer::buffer_to_json_with_type_allocation(pointer_value(buffer), |before| {
            allocations += 1;
            assert!(
                crate::arena::pointer_in_nursery(before),
                "the tested key must start young"
            );
            let trace = collect_minor_trace(GcTriggerKind::Direct);
            assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
            assert!(trace.copying_nursery.copied_objects > 0);
            let mut after = None;
            let mut inspect = |value: f64| {
                let value = JSValue::from_bits(value.to_bits());
                if value.is_string() {
                    let key = value.as_string_ptr();
                    unsafe {
                        let bytes = std::slice::from_raw_parts(
                            crate::string::string_data(key),
                            (*key).byte_len as usize,
                        );
                        if bytes == b"type" {
                            after = Some(key as usize);
                        }
                    }
                }
            };
            scan_runtime_handle_roots_mut(&mut RuntimeRootVisitor::for_copy(&mut inspect));
            let after = after.expect("the type key must stay rooted across value allocation");
            assert_ne!(before, after, "the exact type key must relocate");
            crate::string::js_string_from_bytes(b"Buffer".as_ptr(), 6)
        });
    assert_eq!(allocations, 1, "the allocator seam must actually run");
    let scope = RuntimeHandleScope::new();
    let result = scope.root_nanbox_f64(result);
    let type_key = crate::string::intern_ascii_literal(b"type");
    let object =
        crate::value::js_nanbox_get_pointer(result.get_nanbox_f64()) as *const crate::ObjectHeader;
    let kind = crate::object::js_object_get_field_by_name(object, type_key);
    assert_eq!(
        unsafe { crate::string::OwnedStringBytes::copy_from_header(kind.as_string_ptr()) }.as_ref(),
        b"Buffer"
    );
    let data_key = crate::string::intern_ascii_literal(b"data");
    let object =
        crate::value::js_nanbox_get_pointer(result.get_nanbox_f64()) as *const crate::ObjectHeader;
    let array = crate::object::js_object_get_field_by_name(object, data_key)
        .as_pointer::<crate::array::ArrayHeader>();
    assert_eq!(crate::array::js_array_length(array), 3);
    for (index, expected) in [37.0, 91.0, 7.0].into_iter().enumerate() {
        assert_eq!(
            crate::array::js_array_get_f64(array, index as u32),
            expected
        );
    }
}

extern "C" fn current_bigint_comparator(c: *const ClosureHeader, _: JsThis, a: f64, b: f64) -> f64 {
    // Check identity before dereferencing, so the original stale snapshot
    // negative control fails without reading reclaimed/from-space storage.
    assert_eq!(
        c as usize,
        (js_shadow_slot_get(0) & crate::value::POINTER_MASK) as usize,
        "operand boxing must refresh the comparator argument"
    );
    assert_eq!(crate::closure::js_closure_get_capture_f64(c, 0), 37.0);
    crate::bigint::js_bigint_to_f64(JSValue::from_bits(a.to_bits()).as_bigint_ptr())
        - crate::bigint::js_bigint_to_f64(JSValue::from_bits(b.to_bits()).as_bigint_ptr())
}

#[test]
fn bigint_comparator_moves_inside_operand_boxing() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    register_runtime_handle_root_scanner_for_tests();
    let comparator =
        crate::closure::js_closure_alloc(crate::fn_info!(current_bigint_comparator, 2), 1);
    crate::closure::js_closure_set_capture_f64(comparator, 0, 37.0);
    js_shadow_slot_set(0, pointer_value(comparator).to_bits());
    assert!(crate::arena::pointer_in_nursery(comparator as usize));
    let site = unsafe { crate::closure::DirectCall2::resolve(comparator) };
    let mut boxes = 0;
    let order = unsafe {
        crate::typedarray::bigint_lane_compare_with_boxer(site, comparator, 9, 3, |bits| {
            if boxes == 0 {
                let trace = collect_minor_trace(GcTriggerKind::Direct);
                assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);
                assert!(trace.copying_nursery.copied_objects > 0);
                assert_ne!(
                    (js_shadow_slot_get(0) & crate::value::POINTER_MASK) as usize,
                    comparator as usize,
                    "the comparator itself must move while boxing operands"
                );
            }
            boxes += 1;
            crate::value::js_nanbox_bigint(crate::bigint::js_bigint_from_u64(bits) as i64)
        })
    };
    assert_eq!(boxes, 2, "both operand boxes must be made");
    assert_eq!(order, std::cmp::Ordering::Greater);
}

extern "C" fn cleanup(_: *const ClosureHeader, _: JsThis, _: f64) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

#[test]
fn finreg_unregister_refreshes_first_lookup_after_allocating_key() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    let _scan = ConservativeScanDisabledGuard::new();
    let _pacing = crate::gc::policy::force_alloc_point_minor_pacing();
    register_runtime_handle_root_scanner_for_tests();
    let scope = RuntimeHandleScope::new();
    let callback = crate::closure::js_closure_alloc(crate::fn_info!(cleanup, 1), 0);
    let registry = scope.root_nanbox_f64(pointer_value(crate::weakref::js_finreg_new(
        pointer_value(callback),
    )));
    let target = scope.root_nanbox_f64(pointer_value(crate::object::js_object_alloc(0, 0)));
    let token = scope.root_nanbox_f64(pointer_value(crate::object::js_object_alloc(0, 0)));
    crate::weakref::js_finreg_register(
        registry.get_nanbox_f64(),
        target.get_nanbox_f64(),
        91.0,
        token.get_nanbox_f64(),
    );
    let before = registry.get_nanbox_u64();
    assert!(crate::arena::pointer_in_nursery(
        (before & crate::value::POINTER_MASK) as usize
    ));
    // The next general-arena allocation is unregister's FIRST entries key.
    force_next_general_arena_alloc_slow();
    let triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    triggers.make_arena_trigger_due();
    let collections = gc_collection_count();
    let moved = crate::gc::moved_objects_total();
    let removed =
        crate::weakref::js_finreg_unregister(registry.get_nanbox_f64(), token.get_nanbox_f64());
    assert!(
        gc_collection_count() > collections,
        "first key allocation must collect in the entry window"
    );
    assert!(
        crate::gc::moved_objects_total() > moved,
        "the allocation-triggered cycle must move live objects"
    );
    assert_ne!(
        registry.get_nanbox_u64(),
        before,
        "the tested registry must relocate"
    );
    assert_eq!(removed.to_bits(), crate::value::TAG_TRUE);
    assert_eq!(
        crate::weakref::js_finreg_unregister(registry.get_nanbox_f64(), token.get_nanbox_f64())
            .to_bits(),
        crate::value::TAG_FALSE
    );
}

#[test]
fn finreg_record_inputs_follow_collection_inside_record_birth() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    let _scan = ConservativeScanDisabledGuard::new();
    let _pacing = crate::gc::policy::force_alloc_point_minor_pacing();
    register_runtime_handle_root_scanner_for_tests();
    let scope = RuntimeHandleScope::new();
    let callback = crate::closure::js_closure_alloc(crate::fn_info!(cleanup, 1), 0);
    let registry = scope.root_nanbox_f64(pointer_value(crate::weakref::js_finreg_new(
        pointer_value(callback),
    )));
    let target = scope.root_nanbox_f64(pointer_value(crate::object::js_object_alloc(0, 0)));
    let held = scope.root_nanbox_f64(pointer_value(crate::object::js_object_alloc(0, 0)));
    let token = scope.root_nanbox_f64(pointer_value(crate::object::js_object_alloc(0, 0)));
    let before = [
        target.get_nanbox_u64(),
        token.get_nanbox_u64(),
        held.get_nanbox_u64(),
    ];
    assert!(before.iter().all(|bits| crate::arena::pointer_in_nursery(
        (bits & crate::value::POINTER_MASK) as usize
    )));
    force_next_general_arena_alloc_slow();
    let triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    triggers.make_arena_trigger_due();
    let collections = gc_collection_count();
    let moved = crate::gc::moved_objects_total();
    crate::weakref::js_finreg_register(
        registry.get_nanbox_f64(),
        target.get_nanbox_f64(),
        held.get_nanbox_f64(),
        token.get_nanbox_f64(),
    );
    assert!(
        gc_collection_count() > collections,
        "record birth must really collect"
    );
    assert!(crate::gc::moved_objects_total() > moved);
    let after = [
        target.get_nanbox_u64(),
        token.get_nanbox_u64(),
        held.get_nanbox_u64(),
    ];
    assert!(
        before
            .into_iter()
            .zip(after)
            .all(|(before, after)| before != after),
        "all copied record inputs must relocate"
    );
    let registry_ptr = crate::value::js_nanbox_get_pointer(registry.get_nanbox_f64())
        as *const crate::ObjectHeader;
    let entries = crate::object::js_object_get_field(registry_ptr, 1)
        .as_pointer::<crate::array::ArrayHeader>();
    assert_eq!(crate::array::js_array_length(entries), 1);
    let record = JSValue::from_bits(crate::array::js_array_get_f64(entries, 0).to_bits())
        .as_pointer::<crate::ObjectHeader>();
    for (field, expected) in after.into_iter().enumerate() {
        assert_eq!(
            crate::object::js_object_get_field(record, field as u32).bits(),
            expected,
            "record field {field} must contain the refreshed argument"
        );
    }
}

#[test]
fn weakref_deref_refreshes_receiver_after_allocating_target_key() {
    let _guard = CopyingNurseryTestGuard::new(0);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _force = ForcedEvacuationTestGuard::on();
    let _scan = ConservativeScanDisabledGuard::new();
    let _pacing = crate::gc::policy::force_alloc_point_minor_pacing();
    register_runtime_handle_root_scanner_for_tests();
    let scope = RuntimeHandleScope::new();
    let target = scope.root_nanbox_f64(pointer_value(crate::object::js_object_alloc(0, 0)));
    let receiver = scope.root_nanbox_f64(pointer_value(crate::weakref::js_weakref_new(
        target.get_nanbox_f64(),
    )));
    let before = receiver.get_nanbox_u64();
    assert!(crate::arena::pointer_in_nursery(
        (before & crate::value::POINTER_MASK) as usize
    ));
    force_next_general_arena_alloc_slow();
    let triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    triggers.make_arena_trigger_due();
    let collections = gc_collection_count();
    let moved = crate::gc::moved_objects_total();
    let value = crate::weakref::js_weakref_deref(receiver.get_nanbox_f64());
    assert!(
        gc_collection_count() > collections,
        "target-key allocation must collect"
    );
    assert!(crate::gc::moved_objects_total() > moved);
    assert_ne!(
        receiver.get_nanbox_u64(),
        before,
        "the tested WeakRef must move"
    );
    assert_eq!(value.to_bits(), target.get_nanbox_u64());
}
