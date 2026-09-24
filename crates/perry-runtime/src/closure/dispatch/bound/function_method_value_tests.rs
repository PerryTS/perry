#[test]
fn function_method_reads_share_the_prototype_function() {
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = crate::gc::RuntimeHandleScope::new();
    let proto = scope.root_nanbox_f64(crate::object::builtin_prototype_value("Function"));
    for method in [b"call".as_slice(), b"apply".as_slice(), b"bind".as_slice()] {
        let key = scope.root_string_ptr(crate::string::js_string_from_bytes(
            method.as_ptr(),
            method.len() as u32,
        ));
        let key_value = key
            .with_const_ptr::<crate::StringHeader, _>(|p| crate::value::js_nanbox_string(p as i64));
        let key_value = scope.root_nanbox_f64(key_value);
        let expected = scope.root_nanbox_f64(crate::proxy::js_reflect_get(
            proto.get_nanbox_f64(),
            key_value.get_nanbox_f64(),
            proto.get_nanbox_f64(),
        ));
        assert!(crate::value::JSValue::from_bits(expected.get_nanbox_u64()).is_pointer());
        for name in ["Function", "Object", "Array"] {
            let ctor = scope.root_nanbox_f64(crate::object::js_get_global_this_builtin_value(
                name.as_ptr(),
                name.len(),
            ));
            let actual = crate::proxy::js_reflect_get(
                ctor.get_nanbox_f64(),
                key_value.get_nanbox_f64(),
                ctor.get_nanbox_f64(),
            );
            assert_eq!(
                actual.to_bits(),
                expected.get_nanbox_u64(),
                "{name}.{} must be the inherited function value",
                String::from_utf8_lossy(method)
            );
        }
    }
}
