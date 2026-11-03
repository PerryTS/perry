//! #10507: ordinary compiled function construction and `instanceof`.
use super::*;
use crate::codegen_abi::LayoutWord;

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
            LayoutWord((word >> 32) as u32),
            "a construction is stamped with the record's birth shape"
        );
        assert_eq!(
            (*second).class_id,
            0,
            "ordinary function instances use their actual prototype identity"
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
    let old_birth = unsafe { (*(*old_proto).meta).instance_birth };
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
            old_birth,
            "the old prototype retains its valid birth shape for later reuse"
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

#[test]
fn classless_function_births_learn_width_from_the_actual_prototype() {
    let _global = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    let func = compiled_function();
    unsafe {
        for _ in 0..8 {
            let instance = construct(func);
            assert_eq!((*instance).class_id, 0);
            assert_eq!(crate::object::object_live_slot_count(instance), 8);
        }
        let narrow = construct(func);
        assert_eq!(crate::object::object_live_slot_count(narrow), 0);
        for i in 0..12 {
            let name = format!("function_width_{i}");
            let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            crate::object::js_object_set_field_by_name(narrow, key, i as f64);
        }
        assert!(!(*narrow).meta.is_null() && (*(*narrow).meta).spill != 0);
        let wide = construct(func);
        assert_eq!(crate::object::object_live_slot_count(wide), 12);
        assert_eq!(prototype_of(wide), own_prototype(func));
        let other = crate::object::js_object_alloc(0, 0);
        crate::object::js_set_function_prototype(
            func,
            crate::value::js_nanbox_pointer(other as i64),
        );
        assert_eq!(crate::object::object_live_slot_count(construct(func)), 8);
    }
}

#[test]
fn classless_function_store_records_a_shape_plan() {
    let _global = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    let instance = construct(compiled_function());
    let key = crate::string::intern_ascii_literal(b"shape_plan_field");
    unsafe {
        let shape = crate::object::shapes::object_shape_stamp(instance);
        assert_eq!((*instance).class_id, 0);
        crate::object::js_object_set_field_by_name(instance, key, 7.0);
        assert!(
            crate::object::prop_plan::store_plan_check(shape, key as usize),
            "a full negative vet records the classless predecessor shape"
        );
        crate::object::prop_plan::prop_plan_epoch_bump();
        assert!(!crate::object::prop_plan::store_plan_check(
            shape,
            key as usize
        ));
    }
}

#[test]
fn function_and_object_create_share_the_birth_allocator_without_extra_records() {
    let _global = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    let func = compiled_function();
    let first = construct(func);
    let proto = own_prototype(func);
    let record = unsafe { (*(*proto).meta).instance_birth };
    let created = crate::object::js_object_create(crate::value::js_nanbox_pointer(proto as i64));
    let created =
        JSValue::from_bits(created.to_bits()).as_pointer::<ObjectHeader>() as *mut ObjectHeader;
    assert_eq!(
        unsafe { crate::object::shapes::object_shape_stamp(first) },
        unsafe { crate::object::shapes::object_shape_stamp(created) }
    );
    unsafe {
        assert_eq!(
            (*(*proto).meta).instance_birth,
            record,
            "Object.create consumes the existing birth model without writing a function record"
        );
    }
    for _ in 2..8 {
        construct(func);
    }
    let narrow = construct(func);
    unsafe {
        assert_eq!(
            crate::object::object_live_slot_count(narrow),
            0,
            "the existing tracking proof cannot freeze the chosen width"
        );
    }
    let record = unsafe { (*(*proto).meta).instance_birth };
    let created = crate::object::js_object_create(crate::value::js_nanbox_pointer(proto as i64));
    let created =
        JSValue::from_bits(created.to_bits()).as_pointer::<ObjectHeader>() as *mut ObjectHeader;
    assert_eq!(prototype_of(created), proto);
    assert_eq!(
        unsafe { crate::object::shapes::object_shape_stamp(narrow) },
        unsafe { crate::object::shapes::object_shape_stamp(created) }
    );
    unsafe {
        assert_eq!((*(*proto).meta).instance_birth, record);
    }
    let plain = crate::object::js_object_alloc(0, 0);
    crate::object::js_object_create(crate::value::js_nanbox_pointer(plain as i64));
    unsafe {
        assert_eq!(
            (*(*plain).meta).instance_birth,
            0,
            "the shared allocator adds no persistent caching to Object.create"
        );
    }
}
