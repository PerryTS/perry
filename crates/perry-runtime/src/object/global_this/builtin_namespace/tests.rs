use super::*;

fn name(text: &str) -> *const crate::StringHeader {
    crate::string::intern_ascii_literal(text.as_bytes())
}

#[test]
fn bulk_namespace_matches_individual_definitions_and_remains_mutable() {
    let _guard = crate::gc::CopyingNurseryTestGuard::new(0);
    let _no_move = crate::gc::GcSuppressScope::new();
    let members = [
        Member::method(
            "abs",
            crate::fn_info!(super::super::builtin_thunks::math_abs_thunk, 1; with_declared(1), with_flags(crate::closure::FN_BUILTIN)),
            1,
        ),
        Member::constant("PI", std::f64::consts::PI),
        Member::tag("Math"),
    ];
    let fast = create(&members);
    let slow = object::js_object_alloc(0, members.len() as u32);
    super::super::install_static::install_proto_method(
        slow,
        "abs",
        match members[0].value {
            Value::Method(info, _) => info,
            _ => unreachable!(),
        },
        1,
    );
    super::super::set_intrinsic_data_prop(
        slow,
        "PI",
        std::f64::consts::PI,
        object::PropertyAttrs::new(false, false, false),
    );
    super::super::set_intrinsic_to_string_tag(slow, "Math");
    unsafe {
        let a = object::object_keys(fast);
        let b = object::object_keys(slow);
        assert_eq!(a.count(), 3);
        assert_eq!(
            a.arr(),
            b.arr(),
            "same canonical keys, symbols and descriptors"
        );
        assert_eq!(
            object::shapes::object_shape_descriptor(fast),
            object::shapes::object_shape_descriptor(slow),
            "bulk birth retains the individually defined object's shape facts"
        );
    }
    let first = object::js_object_get_field_by_name(fast, name("abs"));
    let second = object::js_object_get_field_by_name(slow, name("abs"));
    assert_ne!(
        first.bits(),
        second.bits(),
        "functions are independent allocations"
    );
    assert_eq!(object::js_object_delete_field(fast, name("PI")), 0);
    object::js_object_set_field_by_name(fast, name("abs"), 17.0);
    assert_eq!(
        object::js_object_get_field_by_name(fast, name("abs")).bits(),
        17f64.to_bits()
    );
    assert_eq!(
        object::js_object_get_field_by_name(slow, name("abs")).bits(),
        second.bits()
    );
    assert_eq!(object::js_object_delete_field(fast, name("abs")), 1);
    object::js_object_set_field_by_name(fast, name("abs"), 23.0);
    assert_eq!(
        object::js_object_get_field_by_name(fast, name("abs")).bits(),
        23f64.to_bits()
    );
}

#[test]
fn math_birth_has_complete_inline_storage_and_independent_functions() {
    let _guard = crate::gc::CopyingNurseryTestGuard::new(0);
    let _no_move = crate::gc::GcSuppressScope::new();
    let a = super::super::math_namespace::create_math_namespace();
    let b = super::super::math_namespace::create_math_namespace();
    unsafe {
        let keys = object::object_keys(a);
        assert_eq!(keys.count(), 45);
        assert_eq!(object::object_live_slot_count(a), 45);
        assert_eq!(keys.arr(), object::object_keys(b).arr());
        assert!(!object::overflow::has_overflow(a));
        assert_eq!(
            key_attrs::keys_entry(keys.arr(), 44),
            key_attrs::ENTRY_NON_ENUMERABLE | key_attrs::ENTRY_NON_WRITABLE
        );
    }
    for method in ["abs", "hypot", "max", "min", "f16round"] {
        assert_ne!(
            object::js_object_get_field_by_name(a, name(method)).bits(),
            object::js_object_get_field_by_name(b, name(method)).bits()
        );
    }
}

#[test]
fn json_reflect_namespace_births_preserve_order_and_use_inline_storage() {
    let _guard = crate::gc::CopyingNurseryTestGuard::new(0);
    let _no_move = crate::gc::GcSuppressScope::new();
    type Factory = fn() -> *mut ObjectHeader;
    let cases: [(Factory, &[&str]); 2] = [
        (
            super::super::json_reflect_namespaces::create_json_namespace,
            &["parse", "stringify", "rawJSON", "isRawJSON"],
        ),
        (
            super::super::json_reflect_namespaces::create_reflect_namespace,
            &[
                "defineProperty",
                "deleteProperty",
                "apply",
                "construct",
                "get",
                "getOwnPropertyDescriptor",
                "getPrototypeOf",
                "has",
                "isExtensible",
                "ownKeys",
                "preventExtensions",
                "set",
                "setPrototypeOf",
            ],
        ),
    ];
    for (create, methods) in cases {
        let a = create();
        let b = create();
        unsafe {
            let keys = object::object_keys(a);
            let count = (methods.len() + 1) as u32;
            assert_eq!(keys.count(), count);
            assert_eq!(object::object_live_slot_count(a), count);
            assert_eq!(keys.arr(), object::object_keys(b).arr());
            assert!(!object::overflow::has_overflow(a));
            let elements = crate::array::array_elements_ptr(keys.arr());
            for (i, method) in methods.iter().enumerate() {
                let expected = f64::from_bits(JSValue::string_ptr(name(method).cast_mut()).bits());
                assert_eq!(
                    crate::string::js_string_equals_value(
                        f64::from_bits(*elements.add(i)),
                        expected
                    ),
                    1
                );
                assert_eq!(
                    key_attrs::keys_entry(keys.arr(), i as u32),
                    key_attrs::ENTRY_NON_ENUMERABLE
                );
                assert_ne!(
                    object::js_object_get_field_by_name(a, name(method)).bits(),
                    object::js_object_get_field_by_name(b, name(method)).bits(),
                    "method allocations are independent"
                );
            }
            assert_eq!(
                key_attrs::keys_entry(keys.arr(), count - 1),
                key_attrs::ENTRY_NON_ENUMERABLE | key_attrs::ENTRY_NON_WRITABLE
            );
        }
    }
}
