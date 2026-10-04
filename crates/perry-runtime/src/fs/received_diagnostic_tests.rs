use super::*;

fn text(value: &str) -> f64 {
    crate::value::js_nanbox_string(js_string_from_bytes(value.as_ptr(), value.len() as u32) as i64)
}

fn check(value: f64, expected: &str) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    assert_eq!(describe_received(value.get_nanbox_f64()), expected);
    let result = crate::validators::js_runtime_describe_received(value.get_nanbox_f64());
    assert!(JSValue::from_bits(result.to_bits()).is_any_string());
    assert_eq!(read_js_string_pub(result), expected);
}

#[test]
fn received_primitives_and_numbers() {
    for (bits, expected) in [
        (crate::value::TAG_UNDEFINED, "undefined"),
        (crate::value::TAG_NULL, "null"),
        (crate::value::TAG_FALSE, "type boolean (false)"),
        (crate::value::TAG_TRUE, "type boolean (true)"),
    ] {
        check(f64::from_bits(bits), expected);
    }
    for (value, expected) in [
        (f64::NAN, "NaN"),
        (-0.0, "-0"),
        (1e21, "1e+21"),
        (1e20, "100000000000000000000"),
        (1e-7, "1e-7"),
        (5e-324, "5e-324"),
        (f64::INFINITY, "Infinity"),
        (f64::NEG_INFINITY, "-Infinity"),
    ] {
        check(value, &format!("type number ({expected})"));
    }
    check(
        f64::from_bits(JSValue::int32(-42).bits()),
        "type number (-42)",
    );
}

#[test]
fn received_utf16_and_quotes() {
    for len in [27, 28, 29] {
        let input = "a".repeat(len);
        let expected = if len > 28 {
            "a".repeat(25) + "..."
        } else {
            input.clone()
        };
        check(text(&input), &format!("type string ('{expected}')"));
    }
    check(text("a'\"\\\n"), "type string (\"a'\\\"\\\\\\n\")");
    check(
        text(&("'".to_owned() + "a".repeat(23).as_str() + "😀abcd")),
        &format!("type string (\"'{}\\ud83d...\")", "a".repeat(23)),
    );
    check(text("'😀"), "type string (\"'😀\")");
    for (bytes, expected) in [
        (b"'\xed\xa0\xbd".as_slice(), "type string (\"'\\ud83d\")"),
        (b"'\xed\xb1\x8d".as_slice(), "type string (\"'\\udc4d\")"),
        (
            b"'\"\\\n\xed\xb1\x8d".as_slice(),
            "type string (\"'\\\"\\\\\\n\\udc4d\")",
        ),
    ] {
        let ptr = crate::string::js_string_from_wtf8_bytes(bytes.as_ptr(), bytes.len() as u32);
        check(crate::value::js_nanbox_string(ptr as i64), expected);
    }
    for prefix_len in [22, 23, 24] {
        let prefix = "'".to_owned() + "a".repeat(prefix_len).as_str();
        let suffix = match prefix_len {
            22 => "😀...",
            23 => "\\ud83d...",
            _ => "...",
        };
        check(
            text(&(prefix.clone() + "😀abcd")),
            &format!("type string (\"{prefix}{suffix}\")"),
        );
    }
}

#[test]
fn received_abi_preserves_split_surrogate() {
    let scope = crate::gc::RuntimeHandleScope::new();
    let input = scope.root_nanbox_f64(text(&("a".repeat(24) + "😀abcd")));
    let result = scope.root_nanbox_f64(crate::validators::js_runtime_describe_received(
        input.get_nanbox_f64(),
    ));
    let ptr =
        crate::value::js_get_string_pointer_unified(result.get_nanbox_f64()) as *const StringHeader;
    assert_eq!(crate::string::js_string_char_code_at(ptr, 38), 55357.0);
    assert_eq!(crate::string::js_string_char_code_at(ptr, 39), b'.' as f64);
    let error = crate::exception::catch_js_throw(|| {
        validate_function("cb", input.get_nanbox_f64());
    })
    .expect_err("invalid callback must throw");
    let error = scope.root_nanbox_f64(error);
    let message = scope.root_string_ptr(crate::error::js_error_get_message(
        JSValue::from_bits(error.get_nanbox_u64()).as_pointer::<crate::error::ErrorHeader>()
            as *mut crate::error::ErrorHeader,
    ));
    let ptr = message.with_const_ptr(|s: *const StringHeader| s);
    let prefix = "The \"cb\" argument must be of type function. Received type string ('";
    assert_eq!(
        crate::string::js_string_char_code_at(ptr, prefix.len() as i32 + 24),
        55357.0
    );
}

#[test]
fn received_native_brands() {
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = crate::gc::RuntimeHandleScope::new();
    let mut values = Vec::new();
    let mut add = |value, name| values.push((scope.root_nanbox_f64(value), name));
    add(
        crate::value::js_nanbox_pointer(crate::buffer::buffer_alloc(1) as i64),
        "Buffer",
    );
    add(
        crate::value::js_nanbox_pointer(crate::buffer::js_uint8array_alloc(1) as i64),
        "Uint8Array",
    );
    add(
        crate::value::js_nanbox_pointer(crate::typedarray::js_typed_array_new_empty(
            crate::typedarray::KIND_INT16 as i32,
            1,
        ) as i64),
        "Int16Array",
    );
    let view = crate::buffer::buffer_alloc(1);
    crate::buffer::mark_as_data_view(view as usize);
    add(crate::value::js_nanbox_pointer(view as i64), "DataView");
    let backing = crate::buffer::buffer_alloc(1);
    crate::buffer::mark_as_array_buffer(backing as usize);
    add(
        crate::value::js_nanbox_pointer(backing as i64),
        "ArrayBuffer",
    );
    add(crate::date::js_date_new_from_timestamp(0.0), "Date");
    add(
        crate::value::js_nanbox_pointer(crate::array::js_array_alloc(0) as i64),
        "Array",
    );
    add(
        crate::value::js_nanbox_pointer(crate::object::js_object_alloc(0, 0) as i64),
        "Object",
    );
    for (value, name) in values {
        check(value.get_nanbox_f64(), &format!("an instance of {name}"));
    }
    let bigint = crate::bigint::js_bigint_from_i64(-123);
    check(
        crate::value::js_nanbox_bigint(bigint as i64),
        "type bigint (-123n)",
    );
}

extern "C" fn received_function(
    _closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

#[test]
fn received_function_and_constructor_names() {
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = crate::gc::RuntimeHandleScope::new();
    let function = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::closure::js_closure_alloc(crate::fn_info!(received_function, 0), 0) as i64,
    ));
    let name = text("namedReceived");
    crate::closure::closure_set_dynamic_prop(
        (function.get_nanbox_u64() & crate::value::POINTER_MASK) as usize,
        "name",
        name,
    );
    check(function.get_nanbox_f64(), "function namedReceived");
    let obj = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc(0, 0) as i64,
    ));
    let ctor = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc(0, 0) as i64,
    ));
    for (value, expected) in [
        (text("Custom"), "Custom"),
        (text(""), ""),
        (f64::from_bits(crate::value::TAG_UNDEFINED), "undefined"),
    ] {
        set(ctor.get_nanbox_f64(), "name", value);
        set(obj.get_nanbox_f64(), "constructor", ctor.get_nanbox_f64());
        check(obj.get_nanbox_f64(), &format!("an instance of {expected}"));
    }
    let null_proto = crate::object::js_object_alloc_null_proto(0, 0);
    check(
        crate::value::js_nanbox_pointer(null_proto as i64),
        "[Object: null prototype] {}",
    );
}

fn set(value: f64, key: &str, field: f64) {
    unsafe {
        crate::object::js_object_set_property_key(value, text(key), field);
    }
}

extern "C" fn received_collecting_name(
    _closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    crate::gc::js_gc_collect();
    text("CollectedName")
}

#[test]
fn received_abi_roots_across_reentrant_name_getter() {
    let _lock = crate::gc::global_side_table_test_lock();
    let value = {
        let scope = crate::gc::RuntimeHandleScope::new();
        let obj = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::object::js_object_alloc(0, 0) as i64,
        ));
        let ctor = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::object::js_object_alloc(0, 0) as i64,
        ));
        let descriptor = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::object::js_object_alloc(0, 0) as i64,
        ));
        let getter = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::closure::js_closure_alloc(crate::fn_info!(received_collecting_name, 0), 0)
                as i64,
        ));
        set(descriptor.get_nanbox_f64(), "get", getter.get_nanbox_f64());
        let key = scope.root_nanbox_f64(text("name"));
        crate::object::js_object_define_property(
            ctor.get_nanbox_f64(),
            key.get_nanbox_f64(),
            descriptor.get_nanbox_f64(),
        );
        set(obj.get_nanbox_f64(), "constructor", ctor.get_nanbox_f64());
        obj.get_nanbox_f64()
    };
    let mut before = 0;
    crate::gc::js_gc_stats(&mut before, std::ptr::null_mut(), std::ptr::null_mut());
    let result = crate::validators::js_runtime_describe_received(value);
    assert_eq!(read_js_string_pub(result), "an instance of CollectedName");
    let mut after = 0;
    crate::gc::js_gc_stats(&mut after, std::ptr::null_mut(), std::ptr::null_mut());
    assert!(after > before, "the reentrant getter must actually collect");
}

#[test]
fn received_symbol_names_reject_implicit_coercion() {
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = crate::gc::RuntimeHandleScope::new();
    let symbol = scope.root_nanbox_f64(unsafe { crate::symbol::js_symbol_new(text("n")) });
    let function = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::closure::js_closure_alloc(crate::fn_info!(received_function, 0), 0) as i64,
    ));
    crate::closure::closure_set_dynamic_prop(
        (function.get_nanbox_u64() & crate::value::POINTER_MASK) as usize,
        "name",
        symbol.get_nanbox_f64(),
    );
    let obj = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc(0, 0) as i64,
    ));
    let ctor = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc(0, 0) as i64,
    ));
    set(ctor.get_nanbox_f64(), "name", symbol.get_nanbox_f64());
    set(obj.get_nanbox_f64(), "constructor", ctor.get_nanbox_f64());
    for value in [function, obj] {
        for abi in [false, true] {
            let error = crate::exception::catch_js_throw(|| {
                if abi {
                    crate::validators::js_runtime_describe_received(value.get_nanbox_f64());
                } else {
                    describe_received(value.get_nanbox_f64());
                }
            })
            .expect_err("Symbol name must throw during implicit coercion");
            let error = scope.root_nanbox_f64(error);
            assert_eq!(
                read_js_string_pub(property(error.get_nanbox_f64(), "name")),
                "TypeError"
            );
            assert_eq!(
                read_js_string_pub(property(error.get_nanbox_f64(), "message")),
                "Cannot convert a Symbol value to a string"
            );
            assert_eq!(
                property(error.get_nanbox_f64(), "code").to_bits(),
                crate::value::TAG_UNDEFINED
            );
        }
    }
    check(symbol.get_nanbox_f64(), "type symbol (Symbol(n))");
}

#[test]
fn received_abi_preserves_surrogate_names() {
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = crate::gc::RuntimeHandleScope::new();
    let function = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::closure::js_closure_alloc(crate::fn_info!(received_function, 0), 0) as i64,
    ));
    let obj = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc(0, 0) as i64,
    ));
    let ctor = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc(0, 0) as i64,
    ));
    set(obj.get_nanbox_f64(), "constructor", ctor.get_nanbox_f64());
    for unit in [0xd800, 0xdc00] {
        let name = scope.root_string_ptr(crate::string::js_string_from_char_code(unit as f64));
        let name =
            name.with_const_ptr(|s: *const StringHeader| crate::value::js_nanbox_string(s as i64));
        crate::closure::closure_set_dynamic_prop(
            (function.get_nanbox_u64() & crate::value::POINTER_MASK) as usize,
            "name",
            name,
        );
        set(ctor.get_nanbox_f64(), "name", name);
        for (value, prefix) in [(&function, "function "), (&obj, "an instance of ")] {
            let result = scope.root_nanbox_f64(crate::validators::js_runtime_describe_received(
                value.get_nanbox_f64(),
            ));
            let ptr = crate::value::js_get_string_pointer_unified(result.get_nanbox_f64())
                as *const StringHeader;
            assert_eq!(
                crate::string::js_string_char_code_at(ptr, prefix.len() as i32),
                unit as f64
            );
            assert_eq!(unsafe { (*ptr).utf16_len } as usize, prefix.len() + 1);
            let error = scope.root_nanbox_f64(build_received_type_error(
                "Received ",
                value.get_nanbox_f64(),
            ));
            let message = scope.root_nanbox_f64(property(error.get_nanbox_f64(), "message"));
            let ptr = crate::value::js_get_string_pointer_unified(message.get_nanbox_f64())
                as *const StringHeader;
            assert_eq!(
                crate::string::js_string_char_code_at(ptr, 9 + prefix.len() as i32),
                unit as f64
            );
            assert_eq!(
                read_js_string_pub(property(error.get_nanbox_f64(), "code")),
                "ERR_INVALID_ARG_TYPE"
            );
        }
    }
}

fn property(value: f64, key: &str) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    let key = scope.root_nanbox_f64(text(key));
    unsafe {
        crate::object::js_object_get_property_key(value.get_nanbox_f64(), key.get_nanbox_f64())
    }
}
