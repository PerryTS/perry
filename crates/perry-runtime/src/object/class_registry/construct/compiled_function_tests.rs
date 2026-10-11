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

#[test]
fn construct_site_revalidates_the_current_function_bag_and_prototype() {
    let _global = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    let site = std::array::from_fn::<_, 4, _>(|_| AtomicU64::new(0));
    let func = compiled_function();
    let make = |f| unsafe {
        super::super::site::js_new_function_construct_site(f, std::ptr::null(), 0, site.as_ptr())
    };
    make(func);
    let first = make(func);
    assert_ne!(
        site[0].load(Ordering::Relaxed) & FUNCTION_SITE,
        0,
        "the memo must be live"
    );
    let closure = (func.to_bits() & crate::value::POINTER_MASK) as usize;
    assert!(unsafe { site_prototype_object(closure, site.as_ptr()) }.is_some());
    let old = own_prototype(func);
    let replacement = crate::object::js_object_alloc(0, 0);
    crate::object::js_set_function_prototype(
        func,
        crate::value::js_nanbox_pointer(replacement as i64),
    );
    let second = make(func);
    assert_eq!(
        prototype_of(
            JSValue::from_bits(first.to_bits()).as_pointer::<ObjectHeader>() as *mut ObjectHeader
        ),
        old
    );
    assert_eq!(
        prototype_of(
            JSValue::from_bits(second.to_bits()).as_pointer::<ObjectHeader>() as *mut ObjectHeader
        ),
        replacement
    );
    // A prototype descriptor mutation re-stamps the prototype but must not
    // change the instance's [[Prototype]]. It also revokes the site proof.
    let key = crate::string::intern_ascii_literal(b"marker");
    crate::object::js_object_set_field_by_name(replacement, key as *mut _, 7.0);
    assert!(unsafe { site_prototype_object(closure, site.as_ptr()) }.is_none());
    let third = make(func);
    assert_eq!(
        prototype_of(
            JSValue::from_bits(third.to_bits()).as_pointer::<ObjectHeader>() as *mut ObjectHeader
        ),
        replacement
    );
    let other = compiled_function();
    let fourth = make(other);
    assert_eq!(
        prototype_of(
            JSValue::from_bits(fourth.to_bits()).as_pointer::<ObjectHeader>() as *mut ObjectHeader
        ),
        own_prototype(other)
    );
    let arrow = crate::closure::js_closure_alloc(
        crate::fn_info!(empty_body, 0; with_declared(0), with_flags(FN_COMPILED_BODY | FN_ARROW)),
        0,
    );
    assert!(unsafe {
        ordinary_compiled_function_at_site(
            crate::value::js_nanbox_pointer(arrow as i64),
            site.as_ptr(),
        )
    }
    .is_none());
}

#[test]
fn construct_site_uses_a_spilled_prototype_from_the_current_bag() {
    let _global = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    let site = std::array::from_fn::<_, 4, _>(|_| AtomicU64::new(0));
    let func = compiled_function();
    let closure = (func.to_bits() & crate::value::POINTER_MASK) as usize;
    unsafe {
        for key in ["extra0", "extra1", "extra2"] {
            crate::closure::props::bag_set(closure, key, 1.0);
        }
        super::super::site::js_new_function_construct_site(
            func,
            std::ptr::null(),
            0,
            site.as_ptr(),
        );
        let instance = super::super::site::js_new_function_construct_site(
            func,
            std::ptr::null(),
            0,
            site.as_ptr(),
        );
        assert_ne!(
            site[0].load(Ordering::Relaxed) & FUNCTION_SITE,
            0,
            "a spill bag must prime the memo"
        );
        let position = (site[2].load(Ordering::Relaxed) >> 32) as u32;
        let live = (site[3].load(Ordering::Relaxed) >> 32) as u32;
        assert!(
            position >= live,
            "the fixture must exercise the spill proof"
        );
        assert!(site_prototype_object(closure, site.as_ptr()).is_some());
        assert_eq!(
            prototype_of(
                JSValue::from_bits(instance.to_bits()).as_pointer::<ObjectHeader>()
                    as *mut ObjectHeader
            ),
            own_prototype(func)
        );
    }
}
