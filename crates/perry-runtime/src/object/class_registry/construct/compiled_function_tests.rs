//! #10507: ordinary compiled function construction and `instanceof`.
use super::*;

extern "C" fn empty_body(_closure: *const ClosureHeader, _this: crate::closure::JsThis) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

fn compiled_function() -> f64 {
    let closure = crate::closure::js_closure_alloc(
        crate::fn_info!(empty_body, 0; with_declared(0), with_flags(FN_COMPILED_BODY)),
        0,
    );
    crate::value::js_nanbox_pointer(closure as i64)
}

fn construct(func: f64) -> *mut ObjectHeader {
    let value = unsafe { js_new_function_construct(func, std::ptr::null(), 0) };
    (value.to_bits() & crate::value::POINTER_MASK) as *mut ObjectHeader
}

fn own_prototype(func: f64) -> *mut ObjectHeader {
    let ptr = (func.to_bits() & crate::value::POINTER_MASK) as usize;
    let value = crate::closure::closure_get_own_dynamic_prop(ptr, "prototype")
        .expect("constructing materializes F.prototype");
    (value.to_bits() & crate::value::POINTER_MASK) as *mut ObjectHeader
}

fn prototype_of(obj: *mut ObjectHeader) -> *mut ObjectHeader {
    let value =
        crate::object::js_object_get_prototype_of(crate::value::js_nanbox_pointer(obj as i64));
    (value.to_bits() & crate::value::POINTER_MASK) as *mut ObjectHeader
}

fn is_instance(obj: *mut ObjectHeader, func: f64) -> bool {
    crate::object::js_instanceof_dynamic(crate::value::js_nanbox_pointer(obj as i64), func)
        .to_bits()
        == crate::value::TAG_TRUE
}

#[test]
fn construction_is_born_from_the_prototype_birth_record() {
    let _global = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    let func = compiled_function();
    assert!(ordinary_compiled_function(func).is_some());
    let first = construct(func);
    let second = construct(func);
    let proto = own_prototype(func);
    unsafe {
        let word = (*(*proto).meta).instance_birth;
        assert_ne!(word, 0, "the first construction mints F.prototype's record");
        assert_eq!((*first).class_id, word as u32);
        assert_eq!((*second).class_id, word as u32);
        assert_eq!(
            crate::object::shapes::object_shape_stamp(second),
            (word >> 32) as u32,
            "a construction is stamped with the record's birth shape"
        );
        assert_eq!(
            (*second).class_id,
            synthetic_class_id_for_function(func),
            "a construction carries its function's class"
        );
        // The birth shape names the prototype; a replayed construction
        // carries no per-instance record.
        assert!(
            (*second).meta.is_null(),
            "a replayed construction allocates no meta record"
        );
        assert_eq!(
            crate::object::shapes::object_prototype_word(second),
            crate::value::js_nanbox_pointer(proto as i64).to_bits()
        );
        assert_eq!(
            crate::object::shapes::object_shape_stamp(first),
            crate::object::shapes::object_shape_stamp(second),
            "the replayed birth is the minted one"
        );
    }
    assert_eq!(prototype_of(second), proto);
    assert!(is_instance(second, func));
    assert_eq!(
        ordinary_compiled_function_has_instance(
            crate::value::js_nanbox_pointer(second as i64),
            func
        ),
        Some(OrdinaryInstanceof::Instance),
        "the shape answers `instanceof` for a construction"
    );
}

#[test]
fn a_reassigned_prototype_leaves_earlier_constructions_alone() {
    let _global = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    let func = compiled_function();
    let before = construct(func);
    let old_proto = own_prototype(func);
    let new_proto = crate::object::js_object_alloc(0, 0);
    crate::object::js_set_function_prototype(
        func,
        crate::value::js_nanbox_pointer(new_proto as i64),
    );
    let after = construct(func);
    assert_eq!(prototype_of(before), old_proto);
    assert_eq!(prototype_of(after), new_proto);
    unsafe {
        assert_eq!(
            (*(*old_proto).meta).instance_birth,
            0,
            "moving the class's prototype retires the old prototype's record"
        );
        assert_ne!(
            crate::object::shapes::object_shape_stamp(before),
            crate::object::shapes::object_shape_stamp(after),
            "the ShapeId names the prototype"
        );
    }
    assert!(
        !is_instance(before, func),
        "F.prototype moved away from `before`"
    );
    assert!(is_instance(after, func));
    crate::object::js_set_function_prototype(
        func,
        crate::value::js_nanbox_pointer(old_proto as i64),
    );
    assert!(is_instance(before, func));
    assert!(!is_instance(after, func));
}

#[test]
fn an_own_has_instance_is_never_answered_from_the_shape() {
    let _global = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    let func = compiled_function();
    let obj = construct(func);
    let obj_value = crate::value::js_nanbox_pointer(obj as i64);
    assert_eq!(
        ordinary_compiled_function_has_instance(obj_value, func),
        Some(OrdinaryInstanceof::Instance)
    );
    let has_instance = crate::symbol::well_known_symbol("hasInstance");
    let sym = f64::from_bits(crate::value::JSValue::pointer(has_instance as *const u8).bits());
    let reject = compiled_function();
    unsafe { crate::symbol::js_object_set_symbol_property(func, sym, reject) };
    assert_eq!(
        ordinary_compiled_function_has_instance(obj_value, func),
        None,
        "a function owning @@hasInstance must reach InstanceofOperator"
    );
}

#[test]
fn only_compiled_ordinary_bodies_take_the_lane() {
    let runtime_native =
        crate::closure::js_closure_alloc(crate::fn_info!(empty_body, 0; with_declared(0)), 0);
    let runtime_native = crate::value::js_nanbox_pointer(runtime_native as i64);
    assert!(ordinary_compiled_function(runtime_native).is_none());
    let arrow = crate::closure::js_closure_alloc(
        crate::fn_info!(empty_body, 0; with_declared(0), with_flags(FN_COMPILED_BODY | FN_ARROW)),
        0,
    );
    let arrow = crate::value::js_nanbox_pointer(arrow as i64);
    assert!(ordinary_compiled_function(arrow).is_none());
}
