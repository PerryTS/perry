extern "C" fn add(_closure: *const crate::closure::ClosureHeader, a: f64, b: f64) -> f64 {
    a + b
}

#[test]
fn function_apply_reads_proxy_wrapped_arrays() {
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = crate::gc::RuntimeHandleScope::new();
    unsafe {
        let array = scope.root_raw_mut_ptr(crate::array::js_array_alloc_with_length(2));
        array.with_mut_ptr(|ptr| crate::array::js_array_set_f64(ptr, 0, 3.0));
        array.with_mut_ptr(|ptr| crate::array::js_array_set_f64(ptr, 1, 4.0));
        assert_eq!(
            super::function_apply_args(array.with_mut_ptr(|p: *mut crate::array::ArrayHeader| {
                crate::value::js_nanbox_pointer(p as i64)
            })),
            vec![3.0, 4.0]
        );
        let handler = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::object::js_object_alloc(0, 0) as i64,
        ));
        let proxy = scope.root_nanbox_f64(crate::proxy::js_proxy_new(
            array.with_mut_ptr(|p: *mut crate::array::ArrayHeader| {
                crate::value::js_nanbox_pointer(p as i64)
            }),
            handler.get_nanbox_f64(),
        ));
        assert_eq!(
            super::function_apply_args(proxy.get_nanbox_f64()),
            vec![3.0, 4.0]
        );
        let nested = crate::proxy::js_proxy_new(proxy.get_nanbox_f64(), handler.get_nanbox_f64());
        let nested = scope.root_nanbox_f64(nested);
        assert_eq!(
            super::function_apply_args(nested.get_nanbox_f64()),
            vec![3.0, 4.0]
        );

        crate::closure::js_register_closure_arity(add as *const u8, 2);
        let callee = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::closure::js_closure_alloc(add as *const u8, 0) as i64,
        ));
        for arguments in [&proxy, &nested] {
            let args = [
                f64::from_bits(crate::value::TAG_UNDEFINED),
                arguments.get_nanbox_f64(),
            ];
            assert_eq!(
                crate::object::js_native_call_method(
                    callee.get_nanbox_f64(),
                    b"apply".as_ptr().cast(),
                    5,
                    args.as_ptr(),
                    args.len(),
                ),
                7.0
            );
        }
    }
}
