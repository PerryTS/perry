//! Restricted `caller` / `arguments` store tests for function receivers.

extern "C" fn non_strict_restricted_store_test_body(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    9525.0
}

extern "C" fn strict_restricted_store_test_body(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    9526.0
}

extern "C" fn method_restricted_store_test_body(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    9527.0
}

/// #9525: a rejected `caller`/`arguments` store on a plain non-strict
/// function is an ordinary failed [[Set]]. Sloppy PutValue absorbs that
/// `false`; strict PutValue promotes it to TypeError. Functions whose own
/// semantics are strict retain the poison-pill throw even from sloppy code.
#[test]
fn plain_function_restricted_stores_honor_put_value_throw() {
    let source = b"function target() {}";
    let plain_func = non_strict_restricted_store_test_body as *const u8;
    unsafe {
        crate::builtins::js_register_function_source(
            plain_func,
            source.as_ptr(),
            source.len() as u32,
            1,
        );
    }
    let plain = crate::closure::js_closure_alloc(plain_func, 0);
    let plain_value = crate::value::js_nanbox_pointer(plain as i64);

    for name in ["caller", "arguments"] {
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let key_value = f64::from_bits(crate::value::JSValue::string_ptr(key).bits());
        assert!(
            crate::exception::catch_js_throw(|| crate::proxy::js_put_value_set(
                plain_value,
                key_value,
                2.0,
                plain_value,
                0,
            ))
            .is_ok(),
            "sloppy assignment to {name} must be silent"
        );
        assert!(
            !crate::closure::closure_has_own_dynamic_prop(plain as usize, name),
            "a rejected sloppy {name} store must not create an own property"
        );
        assert!(
            crate::exception::catch_js_throw(|| crate::proxy::js_put_value_set(
                plain_value,
                key_value,
                2.0,
                plain_value,
                1,
            ))
            .is_err(),
            "strict assignment to {name} must throw"
        );
        assert!(
            !crate::closure::closure_has_own_dynamic_prop(plain as usize, name),
            "a rejected strict {name} store must not create an own property"
        );
    }

    let strict_func = strict_restricted_store_test_body as *const u8;
    unsafe {
        crate::builtins::js_register_function_source(
            strict_func,
            source.as_ptr(),
            source.len() as u32,
            0,
        );
    }
    crate::closure::js_register_closure_strict_function(strict_func);
    let strict = crate::closure::js_closure_alloc(strict_func, 0);
    let strict_value = crate::value::js_nanbox_pointer(strict as i64);
    let caller = crate::string::js_string_from_bytes(b"caller".as_ptr(), 6);
    let caller_value = f64::from_bits(crate::value::JSValue::string_ptr(caller).bits());
    assert!(
        crate::exception::catch_js_throw(|| crate::proxy::js_put_value_set(
            strict_value,
            caller_value,
            2.0,
            strict_value,
            0,
        ))
        .is_err(),
        "a strict function's poison-pill setter must throw even in sloppy code"
    );

    let method_func = method_restricted_store_test_body as *const u8;
    let method_source = b"method() {}";
    unsafe {
        crate::builtins::js_register_function_source(
            method_func,
            method_source.as_ptr(),
            method_source.len() as u32,
            0,
        );
    }
    let method = crate::closure::js_closure_alloc(method_func, 0);
    let method_value = crate::value::js_nanbox_pointer(method as i64);
    assert!(
        crate::exception::catch_js_throw(|| crate::proxy::js_put_value_set(
            method_value,
            caller_value,
            2.0,
            method_value,
            0,
        ))
        .is_err(),
        "a non-strict method still uses the poison-pill setter"
    );
}
