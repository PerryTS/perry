use super::*;
use crate::value::JSValue;

extern "C" fn body(_: *const ClosureHeader, this: crate::closure::JsThis, a: f64) -> f64 {
    this.as_f64() + a
}

static COMPILED: JsFunctionInfo =
    JsFunctionInfo::of(body as crate::codegen_abi::JsBody1<ClosureHeader>)
        .with_declared(1)
        .with_flags(crate::codegen_abi::FN_COMPILED_BODY | crate::codegen_abi::FN_PERMANENT_IMAGE);

static ORDINARY: JsFunctionInfo =
    JsFunctionInfo::of(body as crate::codegen_abi::JsBody1<ClosureHeader>)
        .with_declared(1)
        .with_flags(crate::codegen_abi::FN_COMPILED_BODY
            | crate::codegen_abi::FN_PERMANENT_IMAGE
            | crate::codegen_abi::FN_NON_STRICT_ORDINARY);

#[test]
fn ordinary_object_receiver_uses_compact_birth() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _trigger = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let target = js_closure_alloc(&ORDINARY, 0);
    let receiver = crate::object::js_object_alloc(0, 0);
    let this = crate::value::js_nanbox_pointer(receiver as i64);
    let bound = unsafe { js_function_bind(crate::value::js_nanbox_pointer(target as i64), &this, 1) };
    let bound = JSValue::from_bits(bound.to_bits()).as_pointer::<ClosureHeader>();
    unsafe {
        assert_eq!((*bound).capture_count, 3);
        assert_eq!(js_closure_get_capture_f64(bound, 1).to_bits(), this.to_bits());
    }
}

#[test]
fn immutable_bind_birth_has_three_slots_and_no_metadata_bag() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _trigger = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let target = js_closure_alloc(&COMPILED, 0);
    let target_value = crate::value::js_nanbox_pointer(target as i64);
    let bound = unsafe { js_function_bind(target_value, &4.0, 1) };
    let b = JSValue::from_bits(bound.to_bits()).as_pointer::<ClosureHeader>();
    unsafe {
        assert_eq!((*b).capture_count, 3, "metadata must remain lazy at birth");
        assert!((*b).props.is_null(), "no per-bind own-property allocation");
        let header = (b as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        assert_eq!(
            (*header).size as usize,
            crate::gc::GC_HEADER_SIZE + crate::closure::closure_payload_size(3)
        );
        assert_eq!(
            crate::object::shapes::shape_object_kind_by_id((*b).shape_id),
            Some(crate::object::shapes::ShapeObjectKind::FunctionBound)
        );
        assert_eq!(
            js_closure_get_capture_f64(b, 0).to_bits(),
            target_value.to_bits()
        );
        assert_eq!(js_closure_get_capture_f64(b, 1), 4.0);
        assert_eq!(js_closure_get_capture_ptr(b, 2), 0);
        // Late property edits must not change this bind's immutable snapshot.
        crate::closure::closure_define_dynamic_prop(target as usize, "length", 99.0);
        assert_eq!(crate::closure::bound_function_length(b as usize), Some(1));
    }
    assert_eq!(
        js_closure_call1(b, crate::closure::plain_call_receiver(), 2.0),
        6.0
    );
}

#[test]
fn observable_length_uses_the_snapshot_layout() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _trigger = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let target = js_closure_alloc(&COMPILED, 0);
    crate::closure::closure_define_dynamic_prop(target as usize, "length", 7.0);
    let target_value = crate::value::js_nanbox_pointer(target as i64);
    let bound = unsafe { js_function_bind(target_value, &4.0, 1) };
    let b = JSValue::from_bits(bound.to_bits()).as_pointer::<ClosureHeader>();
    unsafe {
        assert_eq!((*b).capture_count, 5);
        crate::closure::closure_define_dynamic_prop(target as usize, "length", 99.0);
        assert_eq!(crate::closure::bound_function_length(b as usize), Some(7));
    }
}

#[test]
fn compact_birth_traces_target_and_receiver_across_full_collection() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _trigger = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let target = js_closure_alloc(&COMPILED, 0);
    let receiver = crate::object::js_object_alloc(0, 0);
    let this = crate::value::js_nanbox_pointer(receiver as i64);
    let bound =
        unsafe { js_function_bind(crate::value::js_nanbox_pointer(target as i64), &this, 1) };
    let scope = crate::gc::RuntimeHandleScope::new();
    let b = scope.root_nanbox_f64(bound);
    crate::gc::js_gc_collect();
    let bound = JSValue::from_bits(b.get_nanbox_f64().to_bits()).as_pointer::<ClosureHeader>();
    unsafe {
        assert_eq!((*bound).capture_count, 3);
        let slots = crate::closure::closure_capture_slots_mut(bound as *mut ClosureHeader);
        let rewritten = crate::gc::test_gc_rewrite_slot_addresses(bound as usize).unwrap();
        assert!(
            rewritten.contains(&(slots as usize)),
            "target is a traced child"
        );
        assert!(
            rewritten.contains(&(slots.add(1) as usize)),
            "receiver is a traced child"
        );
        let target = JSValue::from_bits(js_closure_get_capture_f64(bound, 0).to_bits())
            .as_pointer::<ClosureHeader>();
        assert!(crate::closure::is_closure_ptr(target as usize));
        let receiver =
            JSValue::from_bits(js_closure_get_capture_f64(bound, 1).to_bits()).as_pointer::<u8>();
        let header = receiver.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        assert_eq!((*header).obj_type, crate::gc::GC_TYPE_OBJECT);
        assert_eq!(
            crate::closure::bound_function_length(bound as usize),
            Some(1)
        );
    }
}
