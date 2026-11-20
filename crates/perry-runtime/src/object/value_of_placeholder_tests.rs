use super::*;

struct Scope {
    _lock: std::sync::MutexGuard<'static, ()>,
    _no_gc: crate::gc::GcSuppressScope,
}

impl Scope {
    fn new() -> Self {
        Self {
            _lock: crate::gc::global_side_table_test_lock(),
            _no_gc: crate::gc::GcSuppressScope::new(),
        }
    }
}

unsafe fn builtin_prototype_method(
    global: *mut ObjectHeader,
    constructor_name: &str,
    method_name: &str,
) -> f64 {
    let constructor_key = crate::string::js_string_from_bytes(
        constructor_name.as_ptr(),
        constructor_name.len() as u32,
    );
    let constructor = js_object_get_field_by_name(global, constructor_key);
    assert!(
        constructor.is_pointer(),
        "{constructor_name} constructor exists"
    );

    let prototype_key = crate::string::js_string_from_bytes(b"prototype".as_ptr(), 9);
    let prototype = js_object_get_field_by_name(
        constructor.as_pointer::<crate::closure::ClosureHeader>() as *const ObjectHeader,
        prototype_key,
    );
    assert!(
        prototype.is_pointer(),
        "{constructor_name}.prototype exists"
    );

    let method_key =
        crate::string::js_string_from_bytes(method_name.as_ptr(), method_name.len() as u32);
    let method = js_object_get_field_by_name(prototype.as_pointer::<ObjectHeader>(), method_key);
    assert!(
        method.is_pointer(),
        "{constructor_name}.prototype.{method_name} is installed"
    );
    f64::from_bits(method.bits())
}

unsafe fn call_value_of(method: f64, receiver: f64) -> f64 {
    crate::closure::native_call_value_this(
        method,
        crate::closure::JsThis::from_f64(receiver),
        std::ptr::null(),
        0,
    )
}

unsafe fn set_own_value_of(receiver: f64) {
    let key_ptr = crate::string::js_string_from_bytes(b"valueOf".as_ptr(), 7);
    let key = crate::value::js_nanbox_string(key_ptr as i64);
    let result = crate::proxy::js_reflect_set(receiver, key, 99.0, receiver);
    assert_eq!(
        crate::value::JSValue::from_bits(result.to_bits()).bits(),
        crate::value::TAG_TRUE,
        "the sentinel valueOf property should be installed on the receiver"
    );
}

extern "C" fn value_of_test_function(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    1.0
}

#[test]
fn borrowed_builtin_value_of_ignores_own_sentinels() {
    let _scope = Scope::new();
    unsafe {
        let global = js_object_alloc(0, 0);
        super::global_this::populate_global_this_builtins(global);
        let receivers = [
            (
                "Array",
                crate::value::js_nanbox_pointer(crate::array::js_array_alloc(0) as i64),
            ),
            (
                "Map",
                crate::value::js_nanbox_pointer(crate::map::js_map_alloc(0) as i64),
            ),
            (
                "Set",
                crate::value::js_nanbox_pointer(crate::set::js_set_alloc(0) as i64),
            ),
            (
                "Function",
                crate::value::js_nanbox_pointer(crate::closure::js_closure_alloc(
                    crate::fn_info!(value_of_test_function, 0; with_declared(0)),
                    0,
                ) as i64),
            ),
        ];

        for (builtin, receiver) in receivers {
            let method = builtin_prototype_method(global, builtin, "valueOf");
            set_own_value_of(receiver);
            assert_eq!(
                call_value_of(method, receiver).to_bits(),
                receiver.to_bits(),
                "borrowed {builtin}.prototype.valueOf returns the receiver despite its own sentinel"
            );
        }
    }
}

#[test]
fn borrowed_builtin_value_of_boxes_number_and_rejects_nullish_receivers() {
    let _scope = Scope::new();
    unsafe {
        let global = js_object_alloc(0, 0);
        super::global_this::populate_global_this_builtins(global);
        let object_value_of = builtin_prototype_method(global, "Object", "valueOf");

        let boxed = call_value_of(object_value_of, 42.0);
        assert!(
            crate::value::JSValue::from_bits(boxed.to_bits()).is_pointer(),
            "Object.prototype.valueOf called with a number boxes it"
        );
        let (_, payload) = crate::builtins::boxed_primitive_payload(boxed)
            .expect("the result is a Number wrapper");
        assert_eq!(payload, 42.0);

        for receiver in [
            f64::from_bits(crate::value::TAG_NULL),
            f64::from_bits(crate::value::TAG_UNDEFINED),
        ] {
            assert!(
                crate::exception::catch_js_throw(|| call_value_of(object_value_of, receiver))
                    .is_err(),
                "Object.prototype.valueOf must throw for null and undefined"
            );
        }
    }
}

#[test]
fn borrowed_builtin_value_of_keeps_number_string_and_date_brand_checks() {
    let _scope = Scope::new();
    unsafe {
        let global = js_object_alloc(0, 0);
        super::global_this::populate_global_this_builtins(global);
        let number_value_of = builtin_prototype_method(global, "Number", "valueOf");
        let string_value_of = builtin_prototype_method(global, "String", "valueOf");
        let date_value_of = builtin_prototype_method(global, "Date", "valueOf");
        let plain = crate::value::js_nanbox_pointer(js_object_alloc(0, 0) as i64);

        for (name, method) in [
            ("Number", number_value_of),
            ("String", string_value_of),
            ("Date", date_value_of),
        ] {
            assert!(
                crate::exception::catch_js_throw(|| call_value_of(method, plain)).is_err(),
                "{name}.prototype.valueOf retains its incompatible-receiver brand check"
            );
        }

        let number = crate::builtins::js_boxed_number_new(42.0);
        assert_eq!(call_value_of(number_value_of, number), 42.0);

        let string_ptr = crate::string::js_string_from_bytes(b"value".as_ptr(), 5);
        let string = crate::builtins::js_boxed_string_new(
            crate::value::js_nanbox_string(string_ptr as i64),
            1,
        );
        assert_eq!(
            call_value_of(string_value_of, string).to_bits(),
            crate::value::js_nanbox_string(string_ptr as i64).to_bits()
        );

        let date = crate::date_hook_slots::js_date_new_from_timestamp(1234.0);
        assert_eq!(call_value_of(date_value_of, date), 1234.0);
    }
}
