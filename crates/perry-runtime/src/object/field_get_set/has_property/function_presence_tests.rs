use super::*;

extern "C" fn noop(_closure: *const crate::closure::ClosureHeader) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

#[test]
fn function_presence_follows_prototypes_without_reading_values() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let scope = crate::gc::RuntimeHandleScope::new();
        let function = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::closure::js_closure_alloc(noop as *const u8, 0) as i64,
        ));
        let has = |receiver: f64, name: &str| {
            let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            crate::value::js_is_truthy(js_object_has_property(
                receiver,
                crate::value::js_nanbox_string(key as i64),
            )) != 0
        };
        for name in ["call", "apply", "bind", "toString", "length"] {
            assert!(has(function.get_nanbox_f64(), name), "missing {name}");
        }
        let addr = crate::value::js_nanbox_get_pointer(function.get_nanbox_f64()) as usize;
        crate::closure::closure_set_dynamic_prop(
            addr,
            "presentUndefined10366",
            f64::from_bits(crate::value::TAG_UNDEFINED),
        );
        assert!(has(function.get_nanbox_f64(), "presentUndefined10366"));
        assert!(!has(function.get_nanbox_f64(), "absent10366"));
        let object = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::object::js_object_alloc(0, 0) as i64,
        ));
        crate::object::js_object_set_prototype_of(
            object.get_nanbox_f64(),
            function.get_nanbox_f64(),
        );
        assert!(has(object.get_nanbox_f64(), "presentUndefined10366"));
        assert!(has(object.get_nanbox_f64(), "toString"));
        crate::object::js_object_set_prototype_of(
            function.get_nanbox_f64(),
            f64::from_bits(crate::value::TAG_NULL),
        );
        assert!(!has(function.get_nanbox_f64(), "call"));
        assert!(has(function.get_nanbox_f64(), "length"));
        assert!(!has(object.get_nanbox_f64(), "toString"));
    }
}
