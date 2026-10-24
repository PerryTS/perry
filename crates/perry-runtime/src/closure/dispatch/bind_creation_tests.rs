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
        .with_flags(
            crate::codegen_abi::FN_COMPILED_BODY
                | crate::codegen_abi::FN_PERMANENT_IMAGE
                | crate::codegen_abi::FN_NON_STRICT_ORDINARY,
        );

#[test]
fn ordinary_object_receiver_uses_compact_birth() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _trigger = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let target = js_closure_alloc(&ORDINARY, 0);
    let receiver = crate::object::js_object_alloc(0, 0);
    let this = crate::value::js_nanbox_pointer(receiver as i64);
    let bound =
        unsafe { js_function_bind(crate::value::js_nanbox_pointer(target as i64), &this, 1) };
    let bound = JSValue::from_bits(bound.to_bits()).as_pointer::<ClosureHeader>();
    unsafe {
        assert_eq!((*bound).capture_count, 3);
        assert_eq!(
            js_closure_get_capture_f64(bound, 1).to_bits(),
            this.to_bits()
        );
    }
}

#[test]
fn immutable_bind_birth_has_three_slots_and_no_metadata_bag() {
    // Production initializes the agent at js_gc_init, before closure births.
    // A raw native allocation no longer initializes shape metadata as a side effect.
    let _agent = crate::state::state();
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
            crate::object::shapes::shape_object_kind_for_word((*b).shape_id),
            Some(crate::object::shapes::ShapeObjectKind::FunctionBound)
        );
        assert_eq!(
            js_closure_get_capture_f64(b, 0).to_bits(),
            target_value.to_bits()
        );
        assert_eq!(js_closure_get_capture_f64(b, 1), 4.0);
        assert!(crate::closure::bound_function_args(b).is_empty());
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
    // The compact zero-argument bind performs no collecting entry that would
    // lazily initialize GC. Production installs scanners before closure birth.
    crate::gc::gc_init();
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

#[test]
fn compact_partials_are_traced_and_length_is_a_snapshot() {
    let _agent = crate::state::state();
    let _lock = crate::gc::global_side_table_test_lock();
    let _trigger = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    for count in 1..=4 {
        let target = js_closure_alloc(&COMPILED, 0);
        let child = crate::object::js_object_alloc(0, 0);
        let child_value = crate::value::js_nanbox_pointer(child as i64);
        let mut args = vec![4.0];
        args.extend(std::iter::repeat_n(child_value, count));
        let bound = unsafe {
            js_function_bind(
                crate::value::js_nanbox_pointer(target as i64),
                args.as_ptr(),
                args.len(),
            )
        };
        let scope = crate::gc::RuntimeHandleScope::new();
        let handle = scope.root_nanbox_f64(bound);
        crate::gc::js_gc_collect();
        let bound =
            JSValue::from_bits(handle.get_nanbox_f64().to_bits()).as_pointer::<ClosureHeader>();
        unsafe {
            assert_eq!((*bound).capture_count, 3 + count as u32);
            assert!((*bound).props.is_null());
            let children = crate::closure::bound_function_args(bound);
            assert_eq!(children.len(), count);
            assert!(children
                .iter()
                .all(|v| JSValue::from_bits(v.to_bits()).is_pointer()));
            let slots = crate::closure::closure_capture_slots_mut(bound as *mut ClosureHeader);
            let rewritten = crate::gc::test_gc_rewrite_slot_addresses(bound as usize).unwrap();
            for i in 3..3 + count {
                assert!(rewritten.contains(&(slots.add(i) as usize)));
            }
            assert_eq!(
                crate::closure::bound_function_length(bound as usize),
                Some(0)
            );
        }
    }
}

#[test]
fn bound_of_bound_preserves_first_receiver_and_argument_order() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _trigger = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let target = js_closure_alloc(&COMPILED, 0);
    let first = unsafe {
        js_function_bind(
            crate::value::js_nanbox_pointer(target as i64),
            [4.0, 2.0].as_ptr(),
            2,
        )
    };
    let second = unsafe { js_function_bind(first, [99.0, 3.0].as_ptr(), 2) };
    let closure = JSValue::from_bits(second.to_bits()).as_pointer::<ClosureHeader>();
    assert_eq!(js_closure_call0(closure, JsThis::UNDEFINED), 6.0);
}

#[test]
fn receiver_witness_rejects_an_internal_slot_mutation() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _trigger = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let target = js_closure_alloc(&COMPILED, 0);
    let bound =
        unsafe { js_function_bind(crate::value::js_nanbox_pointer(target as i64), &4.0, 1) };
    let closure = JSValue::from_bits(bound.to_bits()).as_pointer::<ClosureHeader>();
    let witness = || js_closure_call1(closure, JsThis::UNDEFINED, 2.0) == 6.0;
    assert!(witness());
    // Negative control: a wrong bound receiver must make the same witness fail.
    js_closure_set_capture_f64(closure as *mut ClosureHeader, 1, 99.0);
    assert!(!witness());
}
