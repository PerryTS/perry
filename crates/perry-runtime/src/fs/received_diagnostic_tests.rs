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
    // Function name is configurable but non-writable: build the fixture with
    // [[DefineOwnProperty]], preserving its intrinsic attributes.
    let name = text("namedReceived");
    crate::closure::closure_define_dynamic_prop(
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
    crate::closure::closure_define_dynamic_prop(
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
        crate::closure::closure_define_dynamic_prop(
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

// Restore the intrinsic descriptor even when a regression assertion panics.
fn with_received_typed_array_constructor(test: impl FnOnce(f64, f64)) {
    with_received_intrinsic_constructor("Int16Array", test);
}

static RECEIVED_CHAIN_LINK_CALLS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

fn with_received_intrinsic_constructor(name: &str, test: impl FnOnce(f64, f64)) {
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = crate::gc::RuntimeHandleScope::new();
    let raw = match name {
        "Int16Array" => {
            crate::typedarray::js_typed_array_new_empty(crate::typedarray::KIND_INT16 as i32, 1)
                as i64
        }
        "Uint8Array" => crate::buffer::js_uint8array_alloc(1) as i64,
        "DataView" => {
            let view = crate::buffer::buffer_alloc(1);
            crate::buffer::mark_as_data_view(view as usize);
            view as i64
        }
        "Buffer" => crate::buffer::buffer_alloc(1) as i64,
        "ArrayBuffer" | "SharedArrayBuffer" => {
            let buffer = crate::buffer::buffer_alloc(1);
            if name == "SharedArrayBuffer" {
                crate::buffer::mark_as_shared_array_buffer(buffer as usize);
            } else {
                crate::buffer::mark_as_array_buffer(buffer as usize);
            }
            buffer as i64
        }
        "Date" => crate::value::js_nanbox_get_pointer(crate::date::alloc_date_cell(0.0)),
        _ => panic!("unsupported intrinsic test fixture"),
    };
    let value = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(raw));
    let constructor = scope.root_nanbox_f64(crate::object::js_get_global_this_builtin_value(
        name.as_ptr(),
        name.len(),
    ));
    let prototype = scope.root_nanbox_f64(property(constructor.get_nanbox_f64(), "prototype"));
    let key = scope.root_nanbox_f64(text("constructor"));
    let original = scope.root_nanbox_f64(crate::object::js_object_get_own_property_descriptor(
        prototype.get_nanbox_f64(),
        key.get_nanbox_f64(),
    ));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        test(value.get_nanbox_f64(), prototype.get_nanbox_f64());
    }));
    crate::object::js_object_define_property(
        prototype.get_nanbox_f64(),
        key.get_nanbox_f64(),
        original.get_nanbox_f64(),
    );
    if let Err(panic) = result {
        std::panic::resume_unwind(panic);
    }
    check(value.get_nanbox_f64(), &format!("an instance of {name}"));
}

static RECEIVED_CONSTRUCTOR_CALLS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

extern "C" fn received_patched_constructor(
    _closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    RECEIVED_CONSTRUCTOR_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let scope = crate::gc::RuntimeHandleScope::new();
    let ctor = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc(0, 0) as i64,
    ));
    set(ctor.get_nanbox_f64(), "name", text("PatchedView"));
    ctor.get_nanbox_f64()
}

extern "C" fn received_throwing_constructor(
    _closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    RECEIVED_CONSTRUCTOR_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    crate::exception::js_throw(text("constructor sentinel"));
}

extern "C" fn received_collecting_constructor(
    closure: *const ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    crate::gc::js_gc_collect();
    received_patched_constructor(closure, this)
}

fn install_received_constructor_getter(
    prototype: f64,
    info: *const crate::closure::JsFunctionInfo,
) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let prototype = scope.root_nanbox_f64(prototype);
    let descriptor = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc(0, 0) as i64,
    ));
    let getter = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::closure::js_closure_alloc(info, 0) as i64,
    ));
    set(descriptor.get_nanbox_f64(), "get", getter.get_nanbox_f64());
    set(
        descriptor.get_nanbox_f64(),
        "configurable",
        f64::from_bits(crate::value::TAG_TRUE),
    );
    let key = scope.root_nanbox_f64(text("constructor"));
    crate::object::js_object_define_property(
        prototype.get_nanbox_f64(),
        key.get_nanbox_f64(),
        descriptor.get_nanbox_f64(),
    );
}

#[test]
fn received_typed_array_inherited_constructor_data() {
    with_received_typed_array_constructor(|value, prototype| {
        let scope = crate::gc::RuntimeHandleScope::new();
        let ctor = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::object::js_object_alloc(0, 0) as i64,
        ));
        set(ctor.get_nanbox_f64(), "name", text("PatchedView"));
        set(prototype, "constructor", ctor.get_nanbox_f64());
        check(value, "an instance of PatchedView");
    });
}

#[test]
fn received_typed_array_inherited_constructor_getter() {
    with_received_typed_array_constructor(|value, prototype| {
        install_received_constructor_getter(
            prototype,
            crate::fn_info!(received_patched_constructor, 0),
        );
        RECEIVED_CONSTRUCTOR_CALLS.store(0, std::sync::atomic::Ordering::Relaxed);
        let result = crate::validators::js_runtime_describe_received(value);
        assert_eq!(read_js_string_pub(result), "an instance of PatchedView");
        assert_eq!(
            RECEIVED_CONSTRUCTOR_CALLS.load(std::sync::atomic::Ordering::Relaxed),
            3
        );
    });
}

#[test]
fn received_typed_array_inherited_constructor_throw() {
    with_received_typed_array_constructor(|value, prototype| {
        install_received_constructor_getter(
            prototype,
            crate::fn_info!(received_throwing_constructor, 0),
        );
        RECEIVED_CONSTRUCTOR_CALLS.store(0, std::sync::atomic::Ordering::Relaxed);
        let error = crate::exception::catch_js_throw(|| {
            crate::validators::js_runtime_describe_received(value);
        })
        .expect_err("inherited constructor getter must throw");
        assert_eq!(read_js_string_pub(error), "constructor sentinel");
        assert_eq!(
            RECEIVED_CONSTRUCTOR_CALLS.load(std::sync::atomic::Ordering::Relaxed),
            1
        );
    });
}

#[test]
fn received_typed_array_inherited_constructor_collect() {
    with_received_typed_array_constructor(|value, prototype| {
        install_received_constructor_getter(
            prototype,
            crate::fn_info!(received_collecting_constructor, 0),
        );
        RECEIVED_CONSTRUCTOR_CALLS.store(0, std::sync::atomic::Ordering::Relaxed);
        let mut before = 0;
        crate::gc::js_gc_stats(&mut before, std::ptr::null_mut(), std::ptr::null_mut());
        let result = crate::validators::js_runtime_describe_received(value);
        assert_eq!(read_js_string_pub(result), "an instance of PatchedView");
        let mut after = 0;
        crate::gc::js_gc_stats(&mut after, std::ptr::null_mut(), std::ptr::null_mut());
        assert!(
            after >= before + 3,
            "all three constructor reads must collect"
        );
        assert_eq!(
            RECEIVED_CONSTRUCTOR_CALLS.load(std::sync::atomic::Ordering::Relaxed),
            3
        );
    });
}

// Exercise both the intrinsic shortcut and buffer-backed constructor fallback.
fn inherited_view_data(name: &str) {
    with_received_intrinsic_constructor(name, |value, prototype| {
        let scope = crate::gc::RuntimeHandleScope::new();
        let ctor = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::object::js_object_alloc(0, 0) as i64,
        ));
        set(ctor.get_nanbox_f64(), "name", text("PatchedView"));
        set(prototype, "constructor", ctor.get_nanbox_f64());
        check(value, "an instance of PatchedView");
        // The formatter must keep inherited names in the JS string domain.
        for (bytes, unit) in [(b"\xed\xa0\x80", 0xd800), (b"\xed\xb0\x80", 0xdc00)] {
            let name = scope.root_string_ptr(crate::string::js_string_from_wtf8_bytes(
                bytes.as_ptr(),
                bytes.len() as u32,
            ));
            set(
                ctor.get_nanbox_f64(),
                "name",
                name.with_mut_ptr(|ptr: *mut StringHeader| {
                    crate::value::js_nanbox_string(ptr as i64)
                }),
            );
            let result =
                scope.root_nanbox_f64(crate::validators::js_runtime_describe_received(value));
            let ptr = crate::value::js_get_string_pointer_unified(result.get_nanbox_f64());
            assert_eq!(
                crate::string::js_string_char_code_at(ptr as *const StringHeader, 15),
                unit as f64
            );
        }
    });
}

fn inherited_view_getter(name: &str, collecting: bool) {
    with_received_intrinsic_constructor(name, |value, prototype| {
        let info = if collecting {
            crate::fn_info!(received_collecting_constructor, 0)
        } else {
            crate::fn_info!(received_patched_constructor, 0)
        };
        install_received_constructor_getter(prototype, info);
        RECEIVED_CONSTRUCTOR_CALLS.store(0, std::sync::atomic::Ordering::Relaxed);
        let mut before = 0;
        crate::gc::js_gc_stats(&mut before, std::ptr::null_mut(), std::ptr::null_mut());
        let result = crate::validators::js_runtime_describe_received(value);
        assert_eq!(read_js_string_pub(result), "an instance of PatchedView");
        assert_eq!(
            RECEIVED_CONSTRUCTOR_CALLS.load(std::sync::atomic::Ordering::Relaxed),
            3
        );
        if collecting {
            let mut after = 0;
            crate::gc::js_gc_stats(&mut after, std::ptr::null_mut(), std::ptr::null_mut());
            assert!(
                after >= before + 3,
                "all three constructor reads must collect"
            );
        }
    });
}

fn inherited_view_throw(name: &str) {
    with_received_intrinsic_constructor(name, |value, prototype| {
        install_received_constructor_getter(
            prototype,
            crate::fn_info!(received_throwing_constructor, 0),
        );
        RECEIVED_CONSTRUCTOR_CALLS.store(0, std::sync::atomic::Ordering::Relaxed);
        let error = crate::exception::catch_js_throw(|| {
            crate::validators::js_runtime_describe_received(value);
        })
        .expect_err("inherited constructor getter must throw");
        assert_eq!(read_js_string_pub(error), "constructor sentinel");
        assert_eq!(
            RECEIVED_CONSTRUCTOR_CALLS.load(std::sync::atomic::Ordering::Relaxed),
            1
        );
    });
}

#[test]
fn received_data_view_inherited_constructor_data() {
    inherited_view_data("DataView");
}

#[test]
fn received_data_view_inherited_constructor_getter() {
    inherited_view_getter("DataView", false);
}

#[test]
fn received_data_view_inherited_constructor_throw() {
    inherited_view_throw("DataView");
}

#[test]
fn received_data_view_inherited_constructor_collect() {
    inherited_view_getter("DataView", true);
}

#[test]
fn received_buffer_inherited_constructor_data() {
    inherited_view_data("Buffer");
}

#[test]
fn received_buffer_inherited_constructor_getter() {
    inherited_view_getter("Buffer", false);
}

#[test]
fn received_buffer_inherited_constructor_throw() {
    inherited_view_throw("Buffer");
}

#[test]
fn received_buffer_inherited_constructor_collect() {
    inherited_view_getter("Buffer", true);
}

#[test]
fn received_uint8_array_inherited_constructor_data() {
    inherited_view_data("Uint8Array");
}

#[test]
fn received_uint8_array_inherited_constructor_getter() {
    inherited_view_getter("Uint8Array", false);
}

#[test]
fn received_uint8_array_inherited_constructor_throw() {
    inherited_view_throw("Uint8Array");
}

#[test]
fn received_uint8_array_inherited_constructor_collect() {
    inherited_view_getter("Uint8Array", true);
}

fn assert_received_in_error(value: f64, rhs: &str) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    for abi in [false, true] {
        let error = crate::exception::catch_js_throw(|| {
            if abi {
                crate::validators::js_runtime_describe_received(value.get_nanbox_f64());
            } else {
                validate_function("cb", value.get_nanbox_f64());
            }
        })
        .expect_err("primitive constructor RHS must throw");
        let error = scope.root_nanbox_f64(error);
        assert_eq!(
            read_js_string_pub(property(error.get_nanbox_f64(), "name")),
            "TypeError"
        );
        assert_eq!(
            property(error.get_nanbox_f64(), "code").to_bits(),
            crate::value::TAG_UNDEFINED
        );
        assert_eq!(
            read_js_string_pub(property(error.get_nanbox_f64(), "message")),
            format!("Cannot use 'in' operator to search for 'name' in {rhs}")
        );
    }
}

#[test]
fn received_primitive_constructor_data() {
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc(0, 0) as i64,
    ));
    let symbol = scope.root_nanbox_f64(unsafe { crate::symbol::js_symbol_new(text("n")) });
    let string = scope.root_nanbox_f64(text("ctor"));
    for (constructor, rhs) in [
        (1.0, "1"),
        (string.get_nanbox_f64(), "ctor"),
        (f64::from_bits(crate::value::TAG_TRUE), "true"),
        (symbol.get_nanbox_f64(), "Symbol(n)"),
    ] {
        set(value.get_nanbox_f64(), "constructor", constructor);
        assert_received_in_error(value.get_nanbox_f64(), rhs);
    }
    for bits in [
        crate::value::TAG_NULL,
        crate::value::TAG_UNDEFINED,
        crate::value::TAG_FALSE,
    ] {
        set(value.get_nanbox_f64(), "constructor", f64::from_bits(bits));
        check(value.get_nanbox_f64(), "[Object]");
    }
}

extern "C" fn received_collecting_primitive_constructor(
    _closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    RECEIVED_CONSTRUCTOR_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    crate::gc::js_gc_collect();
    1.0
}

fn primitive_view_getter(name: &str) {
    with_received_intrinsic_constructor(name, |value, prototype| {
        let scope = crate::gc::RuntimeHandleScope::new();
        let value = scope.root_nanbox_f64(value);
        let prototype = scope.root_nanbox_f64(prototype);
        for own in [true, false] {
            let target = if own { &value } else { &prototype };
            install_received_constructor_getter(
                target.get_nanbox_f64(),
                crate::fn_info!(received_collecting_primitive_constructor, 0),
            );
            // Each public/ABI invocation reads twice, including across collecting getters.
            RECEIVED_CONSTRUCTOR_CALLS.store(0, std::sync::atomic::Ordering::Relaxed);
            let mut before = 0;
            crate::gc::js_gc_stats(&mut before, std::ptr::null_mut(), std::ptr::null_mut());
            assert_received_in_error(value.get_nanbox_f64(), "1");
            assert_eq!(
                RECEIVED_CONSTRUCTOR_CALLS.load(std::sync::atomic::Ordering::Relaxed),
                4
            );
            let mut after = 0;
            crate::gc::js_gc_stats(&mut after, std::ptr::null_mut(), std::ptr::null_mut());
            assert!(after >= before + 4, "all constructor reads must collect");
            if own {
                let key = scope.root_nanbox_f64(text("constructor"));
                crate::object::js_object_delete_dynamic_value(
                    value.get_nanbox_f64(),
                    key.get_nanbox_f64(),
                );
            }
        }
    });
}

#[test]
fn received_primitive_buffer_constructor_getter() {
    primitive_view_getter("Buffer");
}
#[test]
fn received_primitive_data_view_constructor_getter() {
    primitive_view_getter("DataView");
}
#[test]
fn received_primitive_typed_array_constructor_getter() {
    primitive_view_getter("Int16Array");
}

extern "C" fn received_second_null_constructor(
    _closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    let call = RECEIVED_CONSTRUCTOR_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if call % 2 == 0 {
        1.0
    } else {
        f64::from_bits(crate::value::TAG_NULL)
    }
}

extern "C" fn received_second_undefined_constructor(
    _closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    let call = RECEIVED_CONSTRUCTOR_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if call % 2 == 0 {
        1.0
    } else {
        f64::from_bits(crate::value::TAG_UNDEFINED)
    }
}

#[test]
fn received_primitive_constructor_second_nullish_read() {
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc(0, 0) as i64,
    ));
    for (info, rhs) in [
        (crate::fn_info!(received_second_null_constructor, 0), "null"),
        (
            crate::fn_info!(received_second_undefined_constructor, 0),
            "undefined",
        ),
    ] {
        install_received_constructor_getter(value.get_nanbox_f64(), info);
        RECEIVED_CONSTRUCTOR_CALLS.store(0, std::sync::atomic::Ordering::Relaxed);
        assert_received_in_error(value.get_nanbox_f64(), rhs);
        assert_eq!(
            RECEIVED_CONSTRUCTOR_CALLS.load(std::sync::atomic::Ordering::Relaxed),
            4
        );
    }
}

static RECEIVED_TRANSITION_COLLECT: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
static RECEIVED_THIRD_KIND: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
extern "C" fn received_transition_constructor(
    _closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    let call = RECEIVED_CONSTRUCTOR_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % 3;
    let kind = RECEIVED_THIRD_KIND.load(std::sync::atomic::Ordering::Relaxed);
    if RECEIVED_TRANSITION_COLLECT.load(std::sync::atomic::Ordering::Relaxed) {
        crate::gc::js_gc_collect();
    }
    if call == 0 && kind == 10 {
        return f64::from_bits(crate::value::TAG_FALSE);
    }
    if call == 2 {
        match kind {
            0 => return f64::from_bits(crate::value::TAG_NULL),
            1 => return f64::from_bits(crate::value::TAG_UNDEFINED),
            3 => return 0.0,
            4 => return f64::from_bits(crate::value::TAG_FALSE),
            5 => return text(""),
            _ => {}
        }
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let ctor = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc(0, 0) as i64,
    ));
    set(
        ctor.get_nanbox_f64(),
        "name",
        text(if call == 2 { "ThirdObject" } else { "Second" }),
    );
    ctor.get_nanbox_f64()
}

fn assert_received_bare_error(value: f64, message: &str) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    let error = crate::exception::catch_js_throw(|| {
        validate_function("cb", value.get_nanbox_f64());
    })
    .expect_err("metadata evaluation must throw");
    let error = scope.root_nanbox_f64(error);
    assert_eq!(
        read_js_string_pub(property(error.get_nanbox_f64(), "name")),
        "TypeError"
    );
    assert_eq!(
        read_js_string_pub(property(error.get_nanbox_f64(), "message")),
        message
    );
    assert_eq!(
        property(error.get_nanbox_f64(), "code").to_bits(),
        crate::value::TAG_UNDEFINED
    );
}

#[test]
fn received_third_constructor_nullish_getv() {
    RECEIVED_TRANSITION_COLLECT.store(false, std::sync::atomic::Ordering::Relaxed);
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc(0, 0) as i64,
    ));
    install_received_constructor_getter(
        value.get_nanbox_f64(),
        crate::fn_info!(received_transition_constructor, 0),
    );
    for (kind, label) in [(0, "null"), (1, "undefined")] {
        RECEIVED_THIRD_KIND.store(kind, std::sync::atomic::Ordering::Relaxed);
        RECEIVED_CONSTRUCTOR_CALLS.store(0, std::sync::atomic::Ordering::Relaxed);
        assert_received_bare_error(
            value.get_nanbox_f64(),
            &format!("Cannot read properties of {label} (reading 'name')"),
        );
        assert_eq!(
            RECEIVED_CONSTRUCTOR_CALLS.load(std::sync::atomic::Ordering::Relaxed),
            3
        );
    }
    RECEIVED_THIRD_KIND.store(2, std::sync::atomic::Ordering::Relaxed);
    RECEIVED_CONSTRUCTOR_CALLS.store(0, std::sync::atomic::Ordering::Relaxed);
    assert_eq!(
        describe_received(value.get_nanbox_f64()),
        "an instance of ThirdObject"
    );
    assert_eq!(
        RECEIVED_CONSTRUCTOR_CALLS.load(std::sync::atomic::Ordering::Relaxed),
        3
    );
    RECEIVED_THIRD_KIND.store(10, std::sync::atomic::Ordering::Relaxed);
    RECEIVED_CONSTRUCTOR_CALLS.store(0, std::sync::atomic::Ordering::Relaxed);
    assert_eq!(describe_received(value.get_nanbox_f64()), "[Object]");
    assert_eq!(
        RECEIVED_CONSTRUCTOR_CALLS.load(std::sync::atomic::Ordering::Relaxed),
        1
    );
}

static RECEIVED_PRIMITIVE_THIS: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
extern "C" fn received_primitive_name(
    _closure: *const ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    let receiver = JSValue::from_bits(this.as_f64().to_bits());
    let matches = match RECEIVED_THIRD_KIND.load(std::sync::atomic::Ordering::Relaxed) {
        3 => receiver.is_number() && this.as_f64() == 0.0,
        4 => receiver.is_bool() && !receiver.as_bool(),
        5 => receiver.is_any_string() && read_js_string_pub(this.as_f64()).is_empty(),
        _ => false,
    };
    RECEIVED_PRIMITIVE_THIS.store(matches, std::sync::atomic::Ordering::Relaxed);
    crate::gc::js_gc_collect();
    text("PrimitiveName")
}

#[test]
fn received_third_constructor_primitive_getv() {
    RECEIVED_TRANSITION_COLLECT.store(true, std::sync::atomic::Ordering::Relaxed);
    let _lock = crate::gc::global_side_table_test_lock();
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc(0, 0) as i64,
    ));
    install_received_constructor_getter(
        value.get_nanbox_f64(),
        crate::fn_info!(received_transition_constructor, 0),
    );
    for (kind, brand) in [(3, "Number"), (4, "Boolean"), (5, "String")] {
        let proto = scope.root_nanbox_f64(crate::object::builtin_prototype_value(brand));
        let key = scope.root_nanbox_f64(text("name"));
        let original = scope.root_nanbox_f64(crate::object::js_object_get_own_property_descriptor(
            proto.get_nanbox_f64(),
            key.get_nanbox_f64(),
        ));
        let descriptor = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::object::js_object_alloc(0, 0) as i64,
        ));
        let getter = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::closure::js_closure_alloc(crate::fn_info!(received_primitive_name, 0), 0) as i64,
        ));
        set(descriptor.get_nanbox_f64(), "get", getter.get_nanbox_f64());
        set(
            descriptor.get_nanbox_f64(),
            "configurable",
            f64::from_bits(crate::value::TAG_TRUE),
        );
        crate::object::js_object_define_property(
            proto.get_nanbox_f64(),
            key.get_nanbox_f64(),
            descriptor.get_nanbox_f64(),
        );
        RECEIVED_THIRD_KIND.store(kind, std::sync::atomic::Ordering::Relaxed);
        RECEIVED_CONSTRUCTOR_CALLS.store(0, std::sync::atomic::Ordering::Relaxed);
        let result = describe_received(value.get_nanbox_f64());
        if original.get_nanbox_u64() == crate::value::TAG_UNDEFINED {
            crate::object::js_object_delete_dynamic_value(
                proto.get_nanbox_f64(),
                key.get_nanbox_f64(),
            );
        } else {
            crate::object::js_object_define_property(
                proto.get_nanbox_f64(),
                key.get_nanbox_f64(),
                original.get_nanbox_f64(),
            );
        }
        assert_eq!(result, "an instance of PrimitiveName", "{brand}");
        assert_eq!(
            RECEIVED_CONSTRUCTOR_CALLS.load(std::sync::atomic::Ordering::Relaxed),
            3
        );
        assert!(
            RECEIVED_PRIMITIVE_THIS.load(std::sync::atomic::Ordering::Relaxed),
            "{brand} receiver must be the original primitive"
        );
    }
}

macro_rules! native_constructor_tests {
    ($data:ident, $getter:ident, $throw:ident, $collect:ident, $brand:literal) => {
        #[test]
        fn $data() {
            inherited_view_data($brand);
        }
        #[test]
        fn $getter() {
            inherited_view_getter($brand, false);
        }
        #[test]
        fn $throw() {
            inherited_view_throw($brand);
        }
        #[test]
        fn $collect() {
            inherited_view_getter($brand, true);
        }
    };
}
native_constructor_tests!(
    received_native_ab_data,
    received_native_ab_getter,
    received_native_ab_throw,
    received_native_ab_collect,
    "ArrayBuffer"
);
native_constructor_tests!(
    received_native_sab_data,
    received_native_sab_getter,
    received_native_sab_throw,
    received_native_sab_collect,
    "SharedArrayBuffer"
);
native_constructor_tests!(
    received_native_date_data,
    received_native_date_getter,
    received_native_date_throw,
    received_native_date_collect,
    "Date"
);

fn native_constructor_absent_fallback(name: &str) {
    with_received_intrinsic_constructor(name, |value, _| {
        let scope = crate::gc::RuntimeHandleScope::new();
        let value = scope.root_nanbox_f64(value);
        set(
            value.get_nanbox_f64(),
            "constructor",
            f64::from_bits(crate::value::TAG_UNDEFINED),
        );
        check(
            value.get_nanbox_f64(),
            &if name == "Date" {
                "1970-01-01T00:00:00.000Z".to_string()
            } else {
                format!("[{name}]")
            },
        );
        let key = scope.root_nanbox_f64(text("constructor"));
        crate::object::js_object_delete_dynamic_value(value.get_nanbox_f64(), key.get_nanbox_f64());
        crate::object::js_object_set_prototype_of(
            value.get_nanbox_f64(),
            f64::from_bits(crate::value::TAG_NULL),
        );
        check(
            value.get_nanbox_f64(),
            &if name == "Date" {
                "[Date: null prototype] 1970-01-01T00:00:00.000Z".to_string()
            } else {
                format!("[{name}: null prototype]")
            },
        );
        let proto = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::object::js_object_alloc_null_proto(0, 0) as i64,
        ));
        crate::object::js_object_set_prototype_of(value.get_nanbox_f64(), proto.get_nanbox_f64());
        check(
            value.get_nanbox_f64(),
            &if name == "Date" {
                "Date <Complex prototype> 1970-01-01T00:00:00.000Z".to_string()
            } else {
                format!("[{name} <Complex prototype>]")
            },
        );
        crate::object::js_object_set_prototype_of(
            value.get_nanbox_f64(),
            crate::object::builtin_prototype_value(name),
        );
    });
}

macro_rules! native_fallback_tests {
    ($test:ident, $brand:literal) => {
        #[test]
        fn $test() {
            native_constructor_absent_fallback($brand);
        }
    };
}
native_fallback_tests!(received_native_ab_fallback, "ArrayBuffer");
native_fallback_tests!(received_native_sab_fallback, "SharedArrayBuffer");
native_fallback_tests!(received_native_date_fallback, "Date");
native_fallback_tests!(received_native_int16_fallback, "Int16Array");
native_fallback_tests!(received_native_data_view_fallback, "DataView");
native_fallback_tests!(received_native_uint8_fallback, "Uint8Array");

#[test]
fn received_native_null_born_typed_array() {
    with_received_intrinsic_constructor("Int16Array", |value, _| {
        let scope = crate::gc::RuntimeHandleScope::new();
        let value = scope.root_nanbox_f64(value);
        let addr = crate::value::addr_class::object_ref_addr(value.get_nanbox_f64());
        assert!(crate::object::prototype_chain::object_static_prototype(addr).is_none());
        let header = unsafe { crate::value::addr_class::try_read_tracked_gc_header(addr) }
            .expect("fixture needs a tracked arena header");
        let original = unsafe { (*header.as_ptr())._reserved };
        unsafe {
            (*header.as_ptr())._reserved |= crate::gc::OBJ_FLAG_NULL_PROTO;
        }
        let result = describe_received(value.get_nanbox_f64());
        let addr = crate::value::addr_class::object_ref_addr(value.get_nanbox_f64());
        let header = unsafe { crate::value::addr_class::try_read_tracked_gc_header(addr) }.unwrap();
        unsafe {
            (*header.as_ptr())._reserved = original;
        }
        assert_eq!(result, "[Int16Array: null prototype]");
    });
}

#[test]
fn received_native_missing_and_undefined_intrinsic_typed_constructor() {
    with_received_intrinsic_constructor("Int16Array", |value, prototype| {
        set(
            prototype,
            "constructor",
            f64::from_bits(crate::value::TAG_UNDEFINED),
        );
        check(value, "[TypedArray [Int16Array]]");
        let key = text("constructor");
        crate::object::js_object_delete_dynamic_value(prototype, key);
        check(value, "an instance of TypedArray");
    });
}

// Native prototype-chain fallback coverage: classification must inspect link
// authority (never a user-visible `constructor` Get, accessor execution or
// Proxy trap), and the Buffer payload arm tolerates exactly one additional
// constructor Get even when that getter throws.

extern "C" fn received_chain_value_getter(
    _closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    RECEIVED_CONSTRUCTOR_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

// The second (payload) read throws; the render must survive it.
extern "C" fn received_chain_throwing_value_getter(
    _closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    let call = RECEIVED_CONSTRUCTOR_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if call % 2 == 1 {
        crate::exception::js_throw(text("constructor sentinel"));
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

// A chain-link constructor accessor must never execute during a render.
extern "C" fn received_chain_link_getter(
    _closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    RECEIVED_CHAIN_LINK_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

fn install_received_chain_value_getter(value: f64, throwing: bool) {
    install_received_constructor_getter(
        value,
        if throwing {
            crate::fn_info!(received_chain_throwing_value_getter, 0)
        } else {
            crate::fn_info!(received_chain_value_getter, 0)
        },
    );
}

fn received_chain_link(prototype: f64) -> f64 {
    let link = crate::value::js_nanbox_pointer(crate::object::js_object_alloc(0, 0) as i64);
    crate::object::js_object_set_prototype_of(link, prototype);
    link
}

// Each expected render must produce the same label on both the Rust and ABI
// paths while firing the value accessor exactly `expected_reads` times and
// never executing a chain-link accessor.
fn received_chain_render_check(value: f64, expected: &str, expected_reads: usize) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    RECEIVED_CONSTRUCTOR_CALLS.store(0, std::sync::atomic::Ordering::Relaxed);
    RECEIVED_CHAIN_LINK_CALLS.store(0, std::sync::atomic::Ordering::Relaxed);
    assert_eq!(describe_received(value.get_nanbox_f64()), expected);
    assert_eq!(
        RECEIVED_CONSTRUCTOR_CALLS.load(std::sync::atomic::Ordering::Relaxed),
        expected_reads,
        "Rust render constructor reads for {expected}"
    );
    assert_eq!(
        RECEIVED_CHAIN_LINK_CALLS.load(std::sync::atomic::Ordering::Relaxed),
        0,
        "chain-link accessor executed (Rust render)"
    );
    RECEIVED_CONSTRUCTOR_CALLS.store(0, std::sync::atomic::Ordering::Relaxed);
    RECEIVED_CHAIN_LINK_CALLS.store(0, std::sync::atomic::Ordering::Relaxed);
    let result = crate::validators::js_runtime_describe_received(value.get_nanbox_f64());
    assert_eq!(read_js_string_pub(result), expected);
    assert_eq!(
        RECEIVED_CONSTRUCTOR_CALLS.load(std::sync::atomic::Ordering::Relaxed),
        expected_reads,
        "ABI render constructor reads for {expected}"
    );
    assert_eq!(
        RECEIVED_CHAIN_LINK_CALLS.load(std::sync::atomic::Ordering::Relaxed),
        0,
        "chain-link accessor executed (ABI render)"
    );
}

// Wraps the intrinsic fixture and restores the received value's own
// constructor property and prototype even when an assertion panics.
fn with_received_chain_value(name: &str, test: impl FnOnce(f64, f64)) {
    with_received_intrinsic_constructor(name, |value, prototype| {
        let scope = crate::gc::RuntimeHandleScope::new();
        let value = scope.root_nanbox_f64(value);
        let prototype = scope.root_nanbox_f64(prototype);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            test(value.get_nanbox_f64(), prototype.get_nanbox_f64());
        }));
        let key = scope.root_nanbox_f64(text("constructor"));
        crate::object::js_object_delete_dynamic_value(value.get_nanbox_f64(), key.get_nanbox_f64());
        crate::object::js_object_set_prototype_of(
            value.get_nanbox_f64(),
            prototype.get_nanbox_f64(),
        );
        if let Err(panic) = result {
            std::panic::resume_unwind(panic);
        }
    });
}

#[test]
fn received_chain_ab_native() {
    with_received_chain_value("ArrayBuffer", |value, prototype| {
        let scope = crate::gc::RuntimeHandleScope::new();
        let value = scope.root_nanbox_f64(value);
        // Native: one replacement link that inherits the intrinsic prototype.
        let replacement = scope.root_nanbox_f64(received_chain_link(prototype));
        install_received_constructor_getter(
            replacement.get_nanbox_f64(),
            crate::fn_info!(received_chain_link_getter, 0),
        );
        install_received_chain_value_getter(value.get_nanbox_f64(), false);
        crate::object::js_object_set_prototype_of(
            value.get_nanbox_f64(),
            replacement.get_nanbox_f64(),
        );
        received_chain_render_check(value.get_nanbox_f64(), "[ArrayBuffer]", 1);
        // Deep native: an extra ordinary link still ends on the intrinsic.
        let deep = scope.root_nanbox_f64(received_chain_link(replacement.get_nanbox_f64()));
        crate::object::js_object_set_prototype_of(value.get_nanbox_f64(), deep.get_nanbox_f64());
        received_chain_render_check(value.get_nanbox_f64(), "[ArrayBuffer]", 1);
    });
}

fn received_chain_deep_null_setup(value: f64) -> f64 {
    // value -> replacement -> null_link -> null (a chain ending in a
    // null-born prototype keeps the complex native label).
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(value);
    let null_link = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
        crate::object::js_object_alloc(0, 0) as i64,
    ));
    crate::object::js_object_set_prototype_of(
        null_link.get_nanbox_f64(),
        f64::from_bits(crate::value::TAG_NULL),
    );
    set(
        null_link.get_nanbox_f64(),
        "constructor",
        f64::from_bits(crate::value::TAG_UNDEFINED),
    );
    let replacement = scope.root_nanbox_f64(received_chain_link(null_link.get_nanbox_f64()));
    install_received_constructor_getter(
        replacement.get_nanbox_f64(),
        crate::fn_info!(received_chain_link_getter, 0),
    );
    install_received_chain_value_getter(value.get_nanbox_f64(), false);
    crate::object::js_object_set_prototype_of(value.get_nanbox_f64(), replacement.get_nanbox_f64());
    replacement.get_nanbox_f64()
}

#[test]
fn received_chain_ab_deep_null() {
    with_received_chain_value("ArrayBuffer", |value, _| {
        received_chain_deep_null_setup(value);
        received_chain_render_check(value, "[ArrayBuffer <Complex prototype>]", 1);
    });
}

#[test]
fn received_chain_int16_native() {
    with_received_chain_value("Int16Array", |value, prototype| {
        let scope = crate::gc::RuntimeHandleScope::new();
        let value = scope.root_nanbox_f64(value);
        let replacement = scope.root_nanbox_f64(received_chain_link(prototype));
        install_received_constructor_getter(
            replacement.get_nanbox_f64(),
            crate::fn_info!(received_chain_link_getter, 0),
        );
        install_received_chain_value_getter(value.get_nanbox_f64(), false);
        crate::object::js_object_set_prototype_of(
            value.get_nanbox_f64(),
            replacement.get_nanbox_f64(),
        );
        received_chain_render_check(value.get_nanbox_f64(), "[Int16Array]", 1);
        let deep = scope.root_nanbox_f64(received_chain_link(replacement.get_nanbox_f64()));
        crate::object::js_object_set_prototype_of(value.get_nanbox_f64(), deep.get_nanbox_f64());
        received_chain_render_check(value.get_nanbox_f64(), "[Int16Array]", 1);
    });
}

#[test]
fn received_chain_int16_deep_null() {
    with_received_chain_value("Int16Array", |value, _| {
        received_chain_deep_null_setup(value);
        received_chain_render_check(value, "[Int16Array <Complex prototype>]", 1);
    });
}

#[test]
fn received_chain_date_native() {
    with_received_chain_value("Date", |value, prototype| {
        let scope = crate::gc::RuntimeHandleScope::new();
        let value = scope.root_nanbox_f64(value);
        let replacement = scope.root_nanbox_f64(received_chain_link(prototype));
        install_received_constructor_getter(
            replacement.get_nanbox_f64(),
            crate::fn_info!(received_chain_link_getter, 0),
        );
        install_received_chain_value_getter(value.get_nanbox_f64(), false);
        crate::object::js_object_set_prototype_of(
            value.get_nanbox_f64(),
            replacement.get_nanbox_f64(),
        );
        received_chain_render_check(value.get_nanbox_f64(), "1970-01-01T00:00:00.000Z", 1);
        let deep = scope.root_nanbox_f64(received_chain_link(replacement.get_nanbox_f64()));
        crate::object::js_object_set_prototype_of(value.get_nanbox_f64(), deep.get_nanbox_f64());
        received_chain_render_check(value.get_nanbox_f64(), "1970-01-01T00:00:00.000Z", 1);
    });
}

#[test]
fn received_chain_date_deep_null() {
    with_received_chain_value("Date", |value, _| {
        received_chain_deep_null_setup(value);
        received_chain_render_check(
            value,
            "Date <Complex prototype> 1970-01-01T00:00:00.000Z",
            1,
        );
    });
}

#[test]
fn received_chain_data_view_native() {
    with_received_chain_value("DataView", |value, prototype| {
        let scope = crate::gc::RuntimeHandleScope::new();
        let value = scope.root_nanbox_f64(value);
        let replacement = scope.root_nanbox_f64(received_chain_link(prototype));
        install_received_constructor_getter(
            replacement.get_nanbox_f64(),
            crate::fn_info!(received_chain_link_getter, 0),
        );
        install_received_chain_value_getter(value.get_nanbox_f64(), false);
        crate::object::js_object_set_prototype_of(
            value.get_nanbox_f64(),
            replacement.get_nanbox_f64(),
        );
        received_chain_render_check(value.get_nanbox_f64(), "[DataView]", 1);
        let deep = scope.root_nanbox_f64(received_chain_link(replacement.get_nanbox_f64()));
        crate::object::js_object_set_prototype_of(value.get_nanbox_f64(), deep.get_nanbox_f64());
        received_chain_render_check(value.get_nanbox_f64(), "[DataView]", 1);
    });
}

#[test]
fn received_chain_data_view_deep_null() {
    with_received_chain_value("DataView", |value, _| {
        received_chain_deep_null_setup(value);
        received_chain_render_check(value, "[DataView <Complex prototype>]", 1);
    });
}

#[test]
fn received_chain_buffer_native() {
    with_received_chain_value("Buffer", |_original, prototype| {
        let scope = crate::gc::RuntimeHandleScope::new();
        // Buffer.alloc(1): the payload arm renders the buffer's bytes, so the
        // received fixture needs the user-facing zero-filled length-1 buffer,
        // not the helper's zero-length allocation cell.
        let value = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::buffer::js_buffer_alloc(1, 0) as i64,
        ));
        let replacement = scope.root_nanbox_f64(received_chain_link(prototype));
        install_received_constructor_getter(
            replacement.get_nanbox_f64(),
            crate::fn_info!(received_chain_link_getter, 0),
        );
        install_received_chain_value_getter(value.get_nanbox_f64(), false);
        crate::object::js_object_set_prototype_of(
            value.get_nanbox_f64(),
            replacement.get_nanbox_f64(),
        );
        // Node's native Buffer payload performs exactly one extra tolerated read.
        received_chain_render_check(value.get_nanbox_f64(), "<Buffer 00>", 2);
        // The payload read stays tolerated even when that getter throws.
        install_received_chain_value_getter(value.get_nanbox_f64(), true);
        received_chain_render_check(value.get_nanbox_f64(), "<Buffer 00>", 2);
        // Deep native chains keep the payload arm and the same read budget.
        let deep = scope.root_nanbox_f64(received_chain_link(replacement.get_nanbox_f64()));
        crate::object::js_object_set_prototype_of(value.get_nanbox_f64(), deep.get_nanbox_f64());
        received_chain_render_check(value.get_nanbox_f64(), "<Buffer 00>", 2);
    });
}

#[test]
fn received_chain_buffer_deep_null() {
    with_received_chain_value("Buffer", |value, _| {
        received_chain_deep_null_setup(value);
        received_chain_render_check(value, "[Uint8Array <Complex prototype>]", 1);
    });
}

#[test]
fn received_chain_cyclic_is_bounded_without_user_get() {
    with_received_chain_value("ArrayBuffer", |value, _| {
        let scope = crate::gc::RuntimeHandleScope::new();
        let value = scope.root_nanbox_f64(value);
        let a = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::object::js_object_alloc(0, 0) as i64,
        ));
        let b = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::object::js_object_alloc(0, 0) as i64,
        ));
        // Spec-level relinking rejects cycles, so record the corrupt edges
        // directly the way a corrupted/foreign link would appear.
        crate::object::prototype_chain::object_set_static_prototype(
            crate::value::addr_class::object_ref_addr(a.get_nanbox_f64()),
            b.get_nanbox_u64(),
        );
        crate::object::prototype_chain::object_set_static_prototype(
            crate::value::addr_class::object_ref_addr(b.get_nanbox_f64()),
            a.get_nanbox_u64(),
        );
        install_received_constructor_getter(
            a.get_nanbox_f64(),
            crate::fn_info!(received_chain_link_getter, 0),
        );
        install_received_constructor_getter(
            b.get_nanbox_f64(),
            crate::fn_info!(received_chain_link_getter, 0),
        );
        install_received_chain_value_getter(value.get_nanbox_f64(), false);
        crate::object::js_object_set_prototype_of(value.get_nanbox_f64(), a.get_nanbox_f64());
        // A corrupt/cyclic chain is bounded and renders ordinary without
        // invoking any user getter.
        received_chain_render_check(value.get_nanbox_f64(), "{}", 1);
    });
}

#[test]
fn received_chain_ordinary_and_recorded_null_controls() {
    with_received_chain_value("ArrayBuffer", |value, _prototype| {
        let scope = crate::gc::RuntimeHandleScope::new();
        let value = scope.root_nanbox_f64(value);
        // A chain that reaches Object.prototype renders ordinary.
        let ordinary = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::object::js_object_alloc(0, 0) as i64,
        ));
        crate::object::js_object_set_prototype_of(
            ordinary.get_nanbox_f64(),
            crate::object::builtin_prototype_value("Object"),
        );
        install_received_constructor_getter(
            ordinary.get_nanbox_f64(),
            crate::fn_info!(received_chain_link_getter, 0),
        );
        install_received_chain_value_getter(value.get_nanbox_f64(), false);
        crate::object::js_object_set_prototype_of(
            value.get_nanbox_f64(),
            ordinary.get_nanbox_f64(),
        );
        received_chain_render_check(value.get_nanbox_f64(), "{}", 1);
        // An explicitly recorded null prototype keeps the existing rendering.
        crate::object::js_object_set_prototype_of(
            value.get_nanbox_f64(),
            f64::from_bits(crate::value::TAG_NULL),
        );
        received_chain_render_check(value.get_nanbox_f64(), "[ArrayBuffer: null prototype]", 1);
    });
}
