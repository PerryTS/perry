//! #12531: the function property bag is the only owner of a default prototype.
use super::super::*;
use super::support::*;

extern "C" fn body(_: *const crate::closure::ClosureHeader, _: crate::closure::JsThis) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

fn function() -> f64 {
    crate::value::js_nanbox_pointer(
        crate::closure::js_closure_alloc(crate::fn_info!(body, 0), 0) as i64,
    )
}

fn prototype(func: f64) -> f64 {
    crate::object::js_function_prototype_value_for_read(func)
}

#[test]
fn default_prototype_is_not_a_class_side_table_root() {
    let _guard = GcTestIsolationGuard::with_realm_bootstrapped();
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    crate::object::test_clear_class_side_table_roots();
    let func = function();
    let proto = prototype(func);
    let addr = (proto.to_bits() & POINTER_MASK) as usize;
    assert_ne!(addr, 0, "premise: a default prototype was materialized");
    clear_marks();
    clear_mark_seeds();
    let valid = build_valid_pointer_set();
    crate::object::scan_class_side_table_roots_mut(&mut RuntimeRootVisitor::for_mark(&valid));
    unsafe {
        assert_eq!(
            (*header_from_user_ptr(addr as *const u8)).gc_flags & GC_FLAG_MARKED,
            0,
            "a function prototype must not be a class registry root"
        );
    }
}

#[test]
fn live_function_alone_keeps_prototype_and_constructor_through_copying_and_full_gc() {
    let _guard = CopyingNurseryTestGuard::new(1);
    let _triggers = GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    gc_register_mutable_root_scanner(crate::object::shapes::scan_shape_table_rekey_mut);
    let func = function();
    js_shadow_slot_set(0, func.to_bits());
    let proto = prototype(func);
    let closure = (func.to_bits() & POINTER_MASK) as *const crate::closure::ClosureHeader;
    let bag = unsafe { (*closure).props };
    assert!(!bag.is_null());
    let _ = gc_collect_minor();
    let moved_func = f64::from_bits(js_shadow_slot_get(0));
    assert_ne!(
        moved_func.to_bits(),
        func.to_bits(),
        "premise: the function moved"
    );
    let moved_closure =
        (moved_func.to_bits() & POINTER_MASK) as *const crate::closure::ClosureHeader;
    assert_ne!(
        unsafe { (*moved_closure).props },
        bag,
        "premise: the property bag moved"
    );
    let moved_proto = prototype(moved_func);
    assert_ne!(moved_proto.to_bits(), crate::value::TAG_UNDEFINED);
    // The prototype may be born old; identity is the refreshed slot, not a raw address.
    let key = crate::string::js_string_from_bytes(b"constructor".as_ptr(), 11);
    let ctor = crate::object::js_object_get_field_by_name(
        (moved_proto.to_bits() & POINTER_MASK) as *const crate::object::ObjectHeader,
        key,
    );
    assert_eq!(ctor.bits(), moved_func.to_bits());
    let _ =
        gc_collect_full_mark_sweep_with_trigger(GcTriggerSnapshot::capture(GcTriggerKind::Direct));
    let live = f64::from_bits(js_shadow_slot_get(0));
    assert_eq!(prototype(live).to_bits(), moved_proto.to_bits());
    assert_ne!(proto.to_bits(), crate::value::TAG_UNDEFINED);
}
