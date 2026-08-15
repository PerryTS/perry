//! Tests for the object module (extracted from mod.rs to keep it under the 2000-line cap).
#![cfg(test)]

use super::*;
use std::os::raw::c_int;

fn test_global_this_builtin_constructor_value(name: &str) -> f64 {
    let closure_ptr = crate::closure::js_closure_alloc(
        crate::object::global_this_builtin_noop_thunk as *const u8,
        0,
    );
    if closure_ptr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    super::native_module::set_bound_native_closure_name(closure_ptr, name);
    if let Some(len) = crate::object::builtin_constructor_spec_length(name) {
        super::native_module::set_builtin_closure_length(closure_ptr as usize, len);
    }
    let proto_key = crate::string::js_string_from_bytes(b"prototype".as_ptr(), 9);
    let proto_obj = js_object_alloc(0, 0);
    if !proto_obj.is_null() {
        let proto_value = crate::value::js_nanbox_pointer(proto_obj as i64);
        js_object_set_field_by_name(closure_ptr as *mut ObjectHeader, proto_key, proto_value);
        let constructor_key = crate::string::js_string_from_bytes(b"constructor".as_ptr(), 11);
        let constructor_value = crate::value::js_nanbox_pointer(closure_ptr as i64);
        js_object_set_field_by_name(proto_obj, constructor_key, constructor_value);
    }
    crate::value::js_nanbox_pointer(closure_ptr as i64)
}

fn js_string_to_rust(value: JSValue) -> String {
    assert!(
        value.is_string(),
        "expected JS string, got bits={:#x}",
        value.bits()
    );
    let ptr = value.as_string_ptr();
    assert!(!ptr.is_null());
    unsafe {
        let data = (ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        let bytes = std::slice::from_raw_parts(data, (*ptr).byte_len as usize);
        std::str::from_utf8(bytes).unwrap().to_string()
    }
}

fn catch_js<F: FnOnce() -> f64>(f: F) -> Result<f64, f64> {
    let env = crate::exception::js_try_push();
    let jumped = unsafe { crate::ffi::setjmp::setjmp(env as *mut c_int) };
    if jumped == 0 {
        let result = f();
        crate::exception::js_try_end();
        Ok(result)
    } else {
        crate::exception::js_try_end();
        let err = crate::exception::js_get_exception();
        crate::exception::js_clear_exception();
        Err(err)
    }
}

unsafe fn installed_builtin_method(ctor_name: &str, method_name: &str) -> f64 {
    let global_ptr = js_object_alloc(0, 0);
    super::global_this::populate_global_this_builtins(global_ptr);
    let ctor_key = crate::string::js_string_from_bytes(ctor_name.as_ptr(), ctor_name.len() as u32);
    let ctor = js_object_get_field_by_name(global_ptr, ctor_key);
    assert!(
        ctor.is_pointer(),
        "{ctor_name} constructor should be installed"
    );

    let prototype_key = crate::string::js_string_from_bytes(b"prototype".as_ptr(), 9);
    let prototype = js_object_get_field_by_name(
        ctor.as_pointer::<crate::closure::ClosureHeader>() as *const ObjectHeader,
        prototype_key,
    );
    assert!(
        prototype.is_pointer(),
        "{ctor_name}.prototype should be installed"
    );

    let method_key =
        crate::string::js_string_from_bytes(method_name.as_ptr(), method_name.len() as u32);
    let method = js_object_get_field_by_name(prototype.as_pointer::<ObjectHeader>(), method_key);
    assert!(
        method.is_pointer(),
        "{ctor_name}.prototype.{method_name} should be a function value"
    );
    f64::from_bits(method.bits())
}

extern "C" fn symbol_to_primitive_nan(
    _closure: *const crate::closure::ClosureHeader,
    hint: f64,
) -> f64 {
    let hint_value = JSValue::from_bits(hint.to_bits());
    assert_eq!(js_string_to_rust(hint_value), "number");
    f64::NAN
}

extern "C" fn value_of_finite(_closure: *const crate::closure::ClosureHeader) -> f64 {
    1.0
}

extern "C" fn symbol_to_primitive_this_object(
    _closure: *const crate::closure::ClosureHeader,
    hint: f64,
) -> f64 {
    let hint_value = JSValue::from_bits(hint.to_bits());
    assert_eq!(js_string_to_rust(hint_value), "number");
    crate::object::js_implicit_this_get()
}

extern "C" fn to_iso_string_sentinel(_closure: *const crate::closure::ClosureHeader) -> f64 {
    let string = crate::string::js_string_from_bytes(b"iso".as_ptr(), 3);
    crate::value::js_nanbox_string(string as i64)
}

#[test]
fn date_to_json_number_hint_honors_symbol_to_primitive() {
    // The @@toPrimitive install lands in the PROCESS-global SYMBOL_PROPERTIES
    // table, which the gc test guards' state reset wipes from parallel test
    // threads (#6965). Hold the global side-table lock.
    let _global = crate::gc::global_side_table_test_lock();
    unsafe {
        let receiver = js_object_alloc(0, 0);
        let receiver_value = crate::value::js_nanbox_pointer(receiver as i64);

        let to_primitive =
            crate::closure::js_closure_alloc(symbol_to_primitive_nan as *const u8, 0);
        crate::closure::js_register_closure_arity(symbol_to_primitive_nan as *const u8, 1);
        let sym = crate::symbol::well_known_symbol("toPrimitive");
        let sym_value =
            f64::from_bits(crate::value::POINTER_TAG | (sym as u64 & crate::value::POINTER_MASK));
        crate::symbol::js_object_set_symbol_property(
            receiver_value,
            sym_value,
            crate::value::js_nanbox_pointer(to_primitive as i64),
        );

        let value_of = crate::closure::js_closure_alloc(value_of_finite as *const u8, 0);
        crate::closure::js_register_closure_arity(value_of_finite as *const u8, 0);
        let value_of_key = crate::string::js_string_from_bytes(b"valueOf".as_ptr(), 7);
        js_object_set_field_by_name(
            receiver,
            value_of_key,
            crate::value::js_nanbox_pointer(value_of as i64),
        );

        let prev_this = js_implicit_this_set(receiver_value);
        let result = catch_js(crate::object::date_proto_thunks::test_date_to_json_current_this);
        js_implicit_this_set(prev_this);

        let result = result.expect("Date.prototype.toJSON should not throw");
        assert!(
            JSValue::from_bits(result.to_bits()).is_null(),
            "@@toPrimitive returning NaN must make Date.prototype.toJSON return null"
        );
    }
}

#[test]
fn date_to_json_symbol_to_primitive_object_result_throws() {
    // See date_to_json_number_hint_honors_symbol_to_primitive (#6965).
    let _global = crate::gc::global_side_table_test_lock();
    unsafe {
        let receiver = js_object_alloc(0, 0);
        let receiver_value = crate::value::js_nanbox_pointer(receiver as i64);

        let to_primitive =
            crate::closure::js_closure_alloc(symbol_to_primitive_this_object as *const u8, 0);
        crate::closure::js_register_closure_arity(symbol_to_primitive_this_object as *const u8, 1);
        let sym = crate::symbol::well_known_symbol("toPrimitive");
        let sym_value =
            f64::from_bits(crate::value::POINTER_TAG | (sym as u64 & crate::value::POINTER_MASK));
        crate::symbol::js_object_set_symbol_property(
            receiver_value,
            sym_value,
            crate::value::js_nanbox_pointer(to_primitive as i64),
        );

        let to_iso = crate::closure::js_closure_alloc(to_iso_string_sentinel as *const u8, 0);
        crate::closure::js_register_closure_arity(to_iso_string_sentinel as *const u8, 0);
        let to_iso_key = crate::string::js_string_from_bytes(b"toISOString".as_ptr(), 11);
        js_object_set_field_by_name(
            receiver,
            to_iso_key,
            crate::value::js_nanbox_pointer(to_iso as i64),
        );

        let prev_this = js_implicit_this_set(receiver_value);
        let result = catch_js(crate::object::date_proto_thunks::test_date_to_json_current_this);
        js_implicit_this_set(prev_this);

        assert!(
            result.is_err(),
            "@@toPrimitive returning an object must throw before toISOString"
        );
    }
}

#[test]
fn builtin_prototype_methods_reject_dynamic_new() {
    // `installed_builtin_method` reads each constructor's `prototype` off a
    // closure — a PROCESS-global `CLOSURE_PROPS` entry the gc test guards'
    // state reset wipes from parallel test threads (#6965). Hold the global
    // side-table lock across the populate-then-assert.
    let _global = crate::gc::global_side_table_test_lock();
    unsafe {
        for (ctor, method) in [
            ("Date", "toJSON"),
            ("Array", "map"),
            ("Object", "hasOwnProperty"),
        ] {
            let method_value = installed_builtin_method(ctor, method);
            let result = catch_js(|| js_new_function_construct(method_value, std::ptr::null(), 0));
            assert!(
                result.is_err(),
                "{ctor}.prototype.{method} should not be constructable"
            );

            let args = crate::array::js_array_alloc(0);
            let args_value = crate::value::js_nanbox_pointer(args as i64);
            let result = catch_js(|| {
                crate::proxy::js_reflect_construct(
                    method_value,
                    args_value,
                    f64::from_bits(crate::value::TAG_UNDEFINED),
                )
            });
            assert!(
                result.is_err(),
                "{ctor}.prototype.{method} should not be a Reflect.construct target"
            );
        }

        let ordinary = crate::closure::js_closure_alloc(value_of_finite as *const u8, 0);
        crate::closure::js_register_closure_arity(value_of_finite as *const u8, 0);
        let ordinary_value = crate::value::js_nanbox_pointer(ordinary as i64);
        let result = catch_js(|| js_new_function_construct(ordinary_value, std::ptr::null(), 0));
        assert!(result.is_ok(), "ordinary closures remain constructable");

        let args = crate::array::js_array_alloc(0);
        let args_value = crate::value::js_nanbox_pointer(args as i64);
        let result = catch_js(|| {
            crate::proxy::js_reflect_construct(
                ordinary_value,
                args_value,
                f64::from_bits(crate::value::TAG_UNDEFINED),
            )
        });
        assert!(
            result.is_ok(),
            "ordinary closures remain Reflect.construct targets"
        );
    }
}

#[test]
fn recorded_prototype_constructor_overrides_plain_object_constructor() {
    unsafe {
        let prototype = js_object_alloc(0, 1);
        let class_id = 9_999_999;
        js_register_class_id(class_id);
        let instance = js_object_alloc(class_id, 0);
        let constructor_key = crate::string::js_string_from_bytes(b"constructor".as_ptr(), 11);
        js_object_set_field_by_name(prototype, constructor_key, 42.0);
        crate::object::prototype_chain::object_set_static_prototype(
            instance as usize,
            crate::value::js_nanbox_pointer(prototype as i64).to_bits(),
        );

        assert_eq!(
            js_object_get_field_by_name(instance, constructor_key).as_number(),
            42.0
        );
    }
}

#[test]
fn closure_name_and_length_ignore_plain_assignment() {
    // The closure side tables are PROCESS-global: the clear below must not
    // land mid-test in a parallel lock-holder's populate-then-assert window,
    // and this test's own populate-then-assert must not be wiped by the gc
    // test guards' state reset (#6965). Hold the global side-table lock.
    let _global = crate::gc::global_side_table_test_lock();
    crate::closure::test_clear_closure_side_tables();
    {
        let closure = crate::closure::js_closure_alloc(
            crate::object::global_this_builtin_noop_thunk as *const u8,
            0,
        );
        assert!(!closure.is_null());
        super::native_module::set_bound_native_closure_name(closure, "fn");
        super::native_module::set_builtin_closure_length(closure as usize, 2);

        let name_key = crate::string::js_string_from_bytes(b"name".as_ptr(), 4);
        let length_key = crate::string::js_string_from_bytes(b"length".as_ptr(), 6);
        let custom_key = crate::string::js_string_from_bytes(b"custom".as_ptr(), 6);
        let replacement = crate::string::js_string_from_bytes(b"changed".as_ptr(), 7);
        let replacement_value = f64::from_bits(JSValue::string_ptr(replacement).bits());
        let closure_obj = closure as *mut ObjectHeader;

        js_object_set_field_by_name(closure_obj, name_key, replacement_value);
        let name = js_object_get_field_by_name(closure_obj, name_key);
        assert_eq!(js_string_to_rust(name), "fn");

        js_object_set_field_by_name(closure_obj, length_key, 99.0);
        let length = js_object_get_field_by_name(closure_obj, length_key);
        assert!(length.is_number());
        assert_eq!(length.as_number(), 2.0);

        js_object_set_field_by_name(closure_obj, custom_key, replacement_value);
        let custom = js_object_get_field_by_name(closure_obj, custom_key);
        assert_eq!(js_string_to_rust(custom), "changed");
    }
}

#[test]
fn closure_name_can_be_redefined_with_define_property() {
    // See closure_name_and_length_ignore_plain_assignment: the clear and the
    // populate-then-assert both need the global side-table lock (#6965).
    let _global = crate::gc::global_side_table_test_lock();
    crate::closure::test_clear_closure_side_tables();
    {
        let closure = crate::closure::js_closure_alloc(
            crate::object::global_this_builtin_noop_thunk as *const u8,
            0,
        );
        assert!(!closure.is_null());
        super::native_module::set_bound_native_closure_name(closure, "fn");

        let name_key = crate::string::js_string_from_bytes(b"name".as_ptr(), 4);
        let value_key = crate::string::js_string_from_bytes(b"value".as_ptr(), 5);
        let writable_key = crate::string::js_string_from_bytes(b"writable".as_ptr(), 8);
        let enumerable_key = crate::string::js_string_from_bytes(b"enumerable".as_ptr(), 10);
        let configurable_key = crate::string::js_string_from_bytes(b"configurable".as_ptr(), 12);
        let replacement = crate::string::js_string_from_bytes(b"require".as_ptr(), 7);

        let descriptor = js_object_alloc(0, 0);
        assert!(!descriptor.is_null());
        js_object_set_field_by_name(
            descriptor,
            value_key,
            f64::from_bits(JSValue::string_ptr(replacement).bits()),
        );
        js_object_set_field_by_name(
            descriptor,
            writable_key,
            f64::from_bits(crate::value::TAG_FALSE),
        );
        js_object_set_field_by_name(
            descriptor,
            enumerable_key,
            f64::from_bits(crate::value::TAG_FALSE),
        );
        js_object_set_field_by_name(
            descriptor,
            configurable_key,
            f64::from_bits(crate::value::TAG_TRUE),
        );

        let closure_value = crate::value::js_nanbox_pointer(closure as i64);
        let name_value = f64::from_bits(JSValue::string_ptr(name_key).bits());
        let descriptor_value = crate::value::js_nanbox_pointer(descriptor as i64);
        js_object_define_property(closure_value, name_value, descriptor_value);

        let name = js_object_get_field_by_name(closure as *const ObjectHeader, name_key);
        assert_eq!(js_string_to_rust(name), "require");

        let own_descriptor = js_object_get_own_property_descriptor(closure_value, name_value);
        let own_descriptor_obj = crate::value::js_nanbox_get_pointer(own_descriptor)
            as *const crate::object::ObjectHeader;
        assert_eq!(
            js_object_get_field_by_name(own_descriptor_obj, value_key).bits(),
            JSValue::string_ptr(replacement).bits()
        );
        assert_eq!(
            js_object_get_field_by_name(own_descriptor_obj, writable_key).bits(),
            crate::value::TAG_FALSE
        );
        assert_eq!(
            js_object_get_field_by_name(own_descriptor_obj, enumerable_key).bits(),
            crate::value::TAG_FALSE
        );
        assert_eq!(
            js_object_get_field_by_name(own_descriptor_obj, configurable_key).bits(),
            crate::value::TAG_TRUE
        );
    }
}

extern "C" fn closure_accessor_getter(_closure: *const crate::closure::ClosureHeader) -> f64 {
    4.0
}

#[test]
fn closure_accessor_define_property_is_own_and_invoked() {
    // See closure_name_and_length_ignore_plain_assignment: the clear and the
    // populate-then-assert both need the global side-table lock (#6965).
    let _global = crate::gc::global_side_table_test_lock();
    crate::closure::test_clear_closure_side_tables();
    let closure = crate::closure::js_closure_alloc(
        crate::object::global_this_builtin_noop_thunk as *const u8,
        0,
    );
    assert!(!closure.is_null());
    let getter = crate::closure::js_closure_alloc(closure_accessor_getter as *const u8, 0);
    assert!(!getter.is_null());

    let caller_key = crate::string::js_string_from_bytes(b"caller".as_ptr(), 6);
    let get_key = crate::string::js_string_from_bytes(b"get".as_ptr(), 3);
    let configurable_key = crate::string::js_string_from_bytes(b"configurable".as_ptr(), 12);
    let descriptor = js_object_alloc(0, 0);
    assert!(!descriptor.is_null());
    js_object_set_field_by_name(
        descriptor,
        get_key,
        crate::value::js_nanbox_pointer(getter as i64),
    );
    js_object_set_field_by_name(
        descriptor,
        configurable_key,
        f64::from_bits(crate::value::TAG_TRUE),
    );

    let closure_value = crate::value::js_nanbox_pointer(closure as i64);
    let key_value = f64::from_bits(JSValue::string_ptr(caller_key).bits());
    let descriptor_value = crate::value::js_nanbox_pointer(descriptor as i64);
    js_object_define_property(closure_value, key_value, descriptor_value);

    assert!(super::has_own_helpers::closure_own_key_present(
        closure as usize,
        "caller"
    ));
    let value = js_object_get_field_by_name(closure as *const ObjectHeader, caller_key);
    assert!(value.is_number());
    assert_eq!(value.as_number(), 4.0);

    let own_descriptor = js_object_get_own_property_descriptor(closure_value, key_value);
    let own_descriptor_obj =
        crate::value::js_nanbox_get_pointer(own_descriptor) as *const crate::object::ObjectHeader;
    assert_eq!(
        js_object_get_field_by_name(own_descriptor_obj, get_key).bits(),
        crate::value::js_nanbox_pointer(getter as i64).to_bits()
    );
    assert_eq!(
        js_object_get_field_by_name(own_descriptor_obj, configurable_key).bits(),
        crate::value::TAG_TRUE
    );
}

#[test]
fn symbol_define_property_attrs_round_trip_descriptor() {
    // The symbol side tables are PROCESS-global: the clear below and the
    // populate-then-assert both need the global side-table lock (#6965).
    let _global = crate::gc::global_side_table_test_lock();
    crate::symbol::test_clear_symbol_side_table_roots();
    unsafe {
        let obj = js_object_alloc(0, 0);
        assert!(!obj.is_null());
        let obj_value = crate::value::js_nanbox_pointer(obj as i64);
        let symbol_key = crate::symbol::js_symbol_new_empty();
        let symbol_ptr = crate::symbol::sym_key_from_f64(symbol_key);
        assert_ne!(symbol_ptr, 0);

        let value_key = crate::string::js_string_from_bytes(b"value".as_ptr(), 5);
        let writable_key = crate::string::js_string_from_bytes(b"writable".as_ptr(), 8);
        let enumerable_key = crate::string::js_string_from_bytes(b"enumerable".as_ptr(), 10);
        let configurable_key = crate::string::js_string_from_bytes(b"configurable".as_ptr(), 12);

        let descriptor = js_object_alloc(0, 0);
        assert!(!descriptor.is_null());
        js_object_set_field_by_name(descriptor, value_key, 42.0);
        js_object_set_field_by_name(
            descriptor,
            writable_key,
            f64::from_bits(crate::value::TAG_FALSE),
        );
        js_object_set_field_by_name(
            descriptor,
            enumerable_key,
            f64::from_bits(crate::value::TAG_FALSE),
        );
        js_object_set_field_by_name(
            descriptor,
            configurable_key,
            f64::from_bits(crate::value::TAG_TRUE),
        );

        let descriptor_value = crate::value::js_nanbox_pointer(descriptor as i64);
        js_object_define_property(obj_value, symbol_key, descriptor_value);

        assert_eq!(
            crate::symbol::symbol_property_root_bits(obj as usize, symbol_ptr),
            Some(42.0f64.to_bits())
        );
        assert!(!crate::symbol::symbol_property_is_enumerable(
            obj as usize,
            symbol_ptr
        ));

        let own_descriptor = js_object_get_own_property_descriptor(obj_value, symbol_key);
        let own_descriptor_obj =
            crate::value::js_nanbox_get_pointer(own_descriptor) as *const ObjectHeader;
        assert!(!own_descriptor_obj.is_null());
        let value = js_object_get_field_by_name(own_descriptor_obj, value_key);
        assert!(value.is_number());
        assert_eq!(value.as_number(), 42.0);
        assert_eq!(
            js_object_get_field_by_name(own_descriptor_obj, writable_key).bits(),
            crate::value::TAG_FALSE
        );
        assert_eq!(
            js_object_get_field_by_name(own_descriptor_obj, enumerable_key).bits(),
            crate::value::TAG_FALSE
        );
        assert_eq!(
            js_object_get_field_by_name(own_descriptor_obj, configurable_key).bits(),
            crate::value::TAG_TRUE
        );

        let attr_descriptor = js_object_alloc(0, 0);
        assert!(!attr_descriptor.is_null());
        js_object_set_field_by_name(
            attr_descriptor,
            enumerable_key,
            f64::from_bits(crate::value::TAG_TRUE),
        );
        let attr_descriptor_value = crate::value::js_nanbox_pointer(attr_descriptor as i64);
        js_object_define_property(obj_value, symbol_key, attr_descriptor_value);
        assert_eq!(
            crate::symbol::symbol_property_root_bits(obj as usize, symbol_ptr),
            Some(42.0f64.to_bits())
        );
        assert!(crate::symbol::symbol_property_is_enumerable(
            obj as usize,
            symbol_ptr
        ));
    }
}

#[test]
fn symbol_keys_keep_creation_order_across_accessor_redefine() {
    // `[[OwnPropertyKeys]]` reports symbol keys in property-CREATION order. A
    // data→accessor redefine must not move the key to the end (test262
    // getOwnPropertySymbols/order-after-define-property), and an accessor
    // installed BETWEEN two data installs must enumerate at its install
    // position — both rest on the order-preserving placeholder that
    // `set_symbol_accessor_property` leaves in `SYMBOL_PROPERTIES`.
    let _global = crate::gc::global_side_table_test_lock();
    crate::symbol::test_clear_symbol_side_table_roots();
    unsafe {
        let own_symbol_order = |obj_value: f64| -> Vec<usize> {
            let arr = crate::symbol::js_object_get_own_property_symbols(obj_value)
                as *const crate::array::ArrayHeader;
            assert!(!arr.is_null());
            let n = crate::array::js_array_length(arr);
            (0..n)
                .map(|i| {
                    (crate::array::js_array_get(arr, i).bits() & crate::value::POINTER_MASK)
                        as usize
                })
                .collect()
        };
        let getter_descriptor = || -> f64 {
            let getter = crate::closure::js_closure_alloc(closure_accessor_getter as *const u8, 0);
            assert!(!getter.is_null());
            let get_key = crate::string::js_string_from_bytes(b"get".as_ptr(), 3);
            let descriptor = js_object_alloc(0, 0);
            assert!(!descriptor.is_null());
            js_object_set_field_by_name(
                descriptor,
                get_key,
                crate::value::js_nanbox_pointer(getter as i64),
            );
            crate::value::js_nanbox_pointer(descriptor as i64)
        };

        // Data → accessor redefine keeps the key's position.
        let obj = js_object_alloc(0, 0);
        assert!(!obj.is_null());
        let obj_value = crate::value::js_nanbox_pointer(obj as i64);
        let sym_a = crate::symbol::js_symbol_new_empty();
        let sym_b = crate::symbol::js_symbol_new_empty();
        let a_ptr = crate::symbol::sym_key_from_f64(sym_a);
        let b_ptr = crate::symbol::sym_key_from_f64(sym_b);
        crate::symbol::js_object_set_symbol_property(obj_value, sym_a, 1.0);
        crate::symbol::js_object_set_symbol_property(obj_value, sym_b, 2.0);
        js_object_define_property(obj_value, sym_a, getter_descriptor());
        assert_eq!(
            own_symbol_order(obj_value),
            vec![a_ptr, b_ptr],
            "data→accessor redefine moved the key out of creation order"
        );
        // The placeholder must never serve as the value — the read goes
        // through the accessor table and runs the getter.
        let read = crate::symbol::js_object_get_symbol_property(obj_value, sym_a);
        assert_eq!(read.to_bits(), 4.0f64.to_bits());

        // Accessor installed between two data installs enumerates in place.
        let obj2 = js_object_alloc(0, 0);
        assert!(!obj2.is_null());
        let obj2_value = crate::value::js_nanbox_pointer(obj2 as i64);
        let sym_c = crate::symbol::js_symbol_new_empty();
        let sym_d = crate::symbol::js_symbol_new_empty();
        let sym_e = crate::symbol::js_symbol_new_empty();
        let c_ptr = crate::symbol::sym_key_from_f64(sym_c);
        let d_ptr = crate::symbol::sym_key_from_f64(sym_d);
        let e_ptr = crate::symbol::sym_key_from_f64(sym_e);
        crate::symbol::js_object_set_symbol_property(obj2_value, sym_c, 1.0);
        js_object_define_property(obj2_value, sym_d, getter_descriptor());
        crate::symbol::js_object_set_symbol_property(obj2_value, sym_e, 3.0);
        assert_eq!(
            own_symbol_order(obj2_value),
            vec![c_ptr, d_ptr, e_ptr],
            "interleaved accessor install enumerated out of creation order"
        );
    }
}

/// #7916 / #8113: the per-object footprint accounting this issue is about,
/// pinned as an executable fact rather than a comment.
///
/// A two-field object literal is `GcHeader (8) + ObjectHeader (24) + 8 *
/// max(live_inline_slot_count, INLINE_SLOT_FLOOR)`. It was 72 bytes at floor 4
/// (#7916 took it to 56 by lowering the floor to 2) and #8113 took it to **48**
/// by deleting the header's two derivable words. 48 bytes to store 16 bytes of
/// payload; `gc-handoff/bench/retain.ts` writes 3x its data volume, down from
/// 4.5x.
///
/// This reads the size the ALLOCATOR recorded (`GcHeader::size`), not a
/// recomputation of the same formula, so it fails if any allocation path
/// silently stops honouring the floor.
#[test]
fn two_field_literal_footprint_is_exactly_accounted() {
    assert_eq!(
        std::mem::size_of::<ObjectHeader>(),
        24,
        "the ObjectHeader half of the accounting: 2 u32 + 2 pointers (#8113 \
         removed `object_type` and `field_count`; either alone saved nothing \
         because the struct re-padded, both together saved 8 bytes)"
    );
    assert_eq!(crate::gc::GC_HEADER_SIZE, 8);

    let keys = b"a\0b\0";
    let obj = js_object_alloc_with_shape(0x7916_0001, 2, keys.as_ptr(), keys.len() as u32);
    assert!(!obj.is_null());
    let recorded = unsafe {
        // #7928 added this probe with a bare `as *const GcHeader`, which the
        // addr-class ratchet rejects (and which turned required `lint` red on
        // `main`). `try_read_gc_header` is the approved accessor: it takes the
        // OBJECT address and does the header arithmetic itself, behind the
        // plausibility and slab checks.
        crate::value::addr_class::try_read_gc_header(obj as usize)
            .expect("a freshly allocated object must carry a readable GcHeader")
            .size as usize
    };
    let expected = crate::gc::GC_HEADER_SIZE
        + std::mem::size_of::<ObjectHeader>()
        + 8 * std::cmp::max(2, crate::object::INLINE_SLOT_FLOOR);
    assert_eq!(
        recorded, expected,
        "a 2-field literal must occupy exactly {expected} bytes"
    );
    assert_eq!(
        recorded, 48,
        "#8113: the 2-field literal footprint is 48 bytes (56 before the header \
         shrink, 72 at floor 4). Raising INLINE_SLOT_FLOOR back to 4 re-adds 16 \
         bytes of unusable slots to every small object; re-adding a header word \
         re-adds 8 to every object regardless of width"
    );

    // #8113 acceptance: the WIDE case too. The floor does not apply at 8
    // fields, so this isolates the header term from the padding term — it is
    // the number that says the saving is per-OBJECT, not per-small-object.
    let wide_keys = b"a\0b\0c\0d\0e\0f\0g\0h\0";
    let wide =
        js_object_alloc_with_shape(0x8113_0008, 8, wide_keys.as_ptr(), wide_keys.len() as u32);
    assert!(!wide.is_null());
    let wide_recorded = unsafe {
        crate::value::addr_class::try_read_gc_header(wide as usize)
            .expect("a freshly allocated object must carry a readable GcHeader")
            .size as usize
    };
    assert_eq!(
        wide_recorded, 96,
        "#8113: the 8-slot footprint is 96 bytes (104 before the header shrink)"
    );
}

/// #8113 acceptance, spelled as offsets rather than a total so a failure names
/// the field that moved. `GcHeader` staying 8 bytes is part of the contract:
/// the whole 8-byte saving is the header's, not a GcHeader change.
#[test]
fn object_header_is_two_words_plus_two_pointers() {
    use std::mem::{align_of, offset_of, size_of};
    assert_eq!(crate::gc::GC_HEADER_SIZE, 8);
    assert_eq!(size_of::<crate::gc::GcHeader>(), 8);
    assert_eq!(align_of::<ObjectHeader>(), size_of::<*const u8>());
    assert_eq!(offset_of!(ObjectHeader, class_id), 0);
    assert_eq!(offset_of!(ObjectHeader, parent_class_id), 4);
    assert_eq!(offset_of!(ObjectHeader, keys_array), size_of::<*const u8>());
    assert_eq!(offset_of!(ObjectHeader, meta), 2 * size_of::<*const u8>());
    assert_eq!(size_of::<ObjectHeader>(), 3 * size_of::<*const u8>());
    // The emitted-IR offsets in perry-codegen are literals; these two are the
    // ones `class_field_inline_guard` / `proxy_reflect` / `generic_dispatch`
    // splice in, and `stmt/loops.rs` + `expr/proxy_reflect.rs` used to divide
    // the size by 8 for a word index.
    assert_eq!(size_of::<ObjectHeader>() % 8, 0);
}

/// Paired with `inline_slot_floor_matches_runtime` in
/// `perry-codegen/src/target_layout.rs` (#7916).
///
/// perry-codegen cannot depend on perry-runtime, so it carries its own copy of
/// this constant and uses it to size the inline-`new` bump allocation, which
/// must match the floor every runtime bounds check applies. The two failure
/// modes point in opposite directions (codegen too small under-allocates;
/// codegen too large over-reads), so the values must be exactly equal — pin the
/// number on both sides.
#[test]
fn inline_slot_floor_matches_codegen() {
    assert_eq!(
        crate::object::INLINE_SLOT_FLOOR,
        2,
        "perry-codegen's target_layout::INLINE_SLOT_FLOOR is 2; update both sides together"
    );
}

/// #7916: lowering the floor must not change what `{}` + by-name growth does,
/// only where the inline/overflow boundary sits. Fields placed past the
/// boundary go to overflow storage and must still read back — the property
/// that makes the floor a footprint dial rather than a correctness one.
#[test]
fn by_name_growth_past_the_floor_reads_back() {
    let obj = js_object_alloc(0, 0);
    assert!(!obj.is_null());
    let names: [&[u8]; 6] = [b"k0", b"k1", b"k2", b"k3", b"k4", b"k5"];
    for (i, n) in names.iter().enumerate() {
        let key = crate::string::js_string_from_bytes(n.as_ptr(), n.len() as u32);
        js_object_set_field_by_name(obj, key, i as f64);
    }
    for (i, n) in names.iter().enumerate() {
        let key = crate::string::js_string_from_bytes(n.as_ptr(), n.len() as u32);
        let got = js_object_get_field_by_name(obj, key);
        assert!(
            got.is_number() && got.as_number() == i as f64,
            "field {} ({}) read back as {:#x}; the inline/overflow boundary \
             must be invisible to reads",
            i,
            std::str::from_utf8(n).unwrap(),
            got.bits()
        );
    }
}

#[test]
fn test_object_alloc_and_fields() {
    let obj = js_object_alloc(1, 3);

    // Check header
    assert_eq!(js_object_get_class_id(obj), 1);

    // Fields should be undefined initially
    let f0 = js_object_get_field(obj, 0);
    assert!(f0.is_undefined());

    // Set and get a field
    js_object_set_field(obj, 0, JSValue::number(42.0));
    let f0 = js_object_get_field(obj, 0);
    assert!(f0.is_number());
    assert_eq!(f0.as_number(), 42.0);

    // Set another field
    js_object_set_field(obj, 2, JSValue::bool(true));
    let f2 = js_object_get_field(obj, 2);
    assert!(f2.is_bool());
    assert!(f2.as_bool());

    // Clean up
    js_object_free(obj);
}

#[test]
fn test_object_to_value_roundtrip() {
    let obj = js_object_alloc(5, 2);
    js_object_set_field(obj, 0, JSValue::number(123.0));

    let value = js_object_to_value(obj);
    assert!(value.is_pointer());

    let obj2 = js_value_to_object(value);
    assert_eq!(js_object_get_class_id(obj2), 5);

    let f0 = js_object_get_field(obj2, 0);
    assert_eq!(f0.as_number(), 123.0);

    js_object_free(obj);
}

#[test]
fn text_encoding_stream_globals_construct_readable_writable_shape() {
    // Constructing the stream globals reads their `prototype` slots out of
    // the PROCESS-global CLOSURE_PROPS table (#6965). Hold the global
    // side-table lock across the populate-then-construct.
    let _global = crate::gc::global_side_table_test_lock();
    unsafe {
        let global_ptr = js_object_alloc(0, 0);
        super::global_this::populate_global_this_builtins(global_ptr);
        assert!(!global_ptr.is_null());

        for ctor_name in ["TextEncoderStream", "TextDecoderStream"] {
            let ctor_raw = test_global_this_builtin_constructor_value(ctor_name);
            let ctor = JSValue::from_bits(ctor_raw.to_bits());
            assert!(
                ctor.is_pointer(),
                "{ctor_name} should be a closure-backed global"
            );

            let ctor_ptr = ctor.as_pointer::<crate::closure::ClosureHeader>();
            assert_eq!((*ctor_ptr).type_tag, crate::closure::CLOSURE_MAGIC);

            let class_id = match ctor_name {
                "TextEncoderStream" => crate::object::class_registry::CLASS_ID_TEXT_ENCODER_STREAM,
                "TextDecoderStream" => crate::object::class_registry::CLASS_ID_TEXT_DECODER_STREAM,
                _ => unreachable!(),
            };
            let instance =
                crate::object::test_text_encoding_stream_new_with_constructor(ctor_raw, class_id);
            for field in ["readable", "writable"] {
                let key = crate::string::js_string_from_bytes(field.as_ptr(), field.len() as u32);
                let key_box = f64::from_bits(JSValue::string_ptr(key).bits());
                let present = js_object_has_property(instance, key_box);
                assert_ne!(
                    crate::value::js_is_truthy(present),
                    0,
                    "{ctor_name} instance should expose {field}"
                );
            }

            let constructor_key = crate::string::js_string_from_bytes(b"constructor".as_ptr(), 11);
            let constructor = js_object_get_field_by_name(
                crate::value::js_nanbox_get_pointer(instance) as *const ObjectHeader,
                constructor_key,
            );
            assert_eq!(
                constructor.bits(),
                ctor.bits(),
                "{ctor_name} instance should point back to its constructor"
            );
        }
    }
}

#[test]
fn navigator_global_constructor_identity_shape() {
    {
        // The constructor's `prototype` read below goes through the
        // PROCESS-global CLOSURE_PROPS table (#6965). Hold the global
        // side-table lock across the populate-then-assert.
        let _global = crate::gc::global_side_table_test_lock();
        let ctor_raw = test_global_this_builtin_constructor_value("Navigator");
        let ctor = JSValue::from_bits(ctor_raw.to_bits());
        assert!(ctor.is_pointer());

        let navigator_raw = crate::navigator::test_navigator_object_with_constructor(ctor_raw);
        let navigator = JSValue::from_bits(navigator_raw.to_bits());
        assert!(navigator.is_pointer());
        let navigator_ptr = navigator.as_pointer::<ObjectHeader>();
        assert_eq!(
            js_object_get_class_id(navigator_ptr),
            crate::navigator::NAVIGATOR_CLASS_ID
        );

        let constructor_key = crate::string::js_string_from_bytes(b"constructor".as_ptr(), 11);
        let actual = js_object_get_field_by_name(navigator_ptr, constructor_key);
        assert_eq!(actual.bits(), ctor.bits());

        let prototype_key = crate::string::js_string_from_bytes(b"prototype".as_ptr(), 9);
        let prototype = js_object_get_field_by_name(
            ctor.as_pointer::<crate::closure::ClosureHeader>() as *const ObjectHeader,
            prototype_key,
        );
        assert!(prototype.is_pointer());
    }
}

#[test]
fn transition_cache_lookup_rejects_mutated_edge_target() {
    let key = crate::string::js_string_from_bytes(b"id".as_ptr(), 2);
    let keys = crate::array::js_array_alloc(4);
    let keys = crate::array::js_array_push(keys, JSValue::string_ptr(key));
    let keys = crate::array::js_array_push(keys, JSValue::string_ptr(key));

    transition_cache_insert(0, key, keys as usize, 0);

    assert!(
        transition_cache_lookup(0, key).is_none(),
        "slot 0 cache edge must not hit after its keys array grows past length 1"
    );

    let slot = transition_cache_slot(0, key as usize);
    with_transition_cache(|t| unsafe {
        // GC_STORE_AUDIT(ROOT): test cleanup writes non-pointer sentinels into scanned TRANSITION_CACHE_GLOBAL roots.
        (*t)[slot] = TransitionEntry {
            prev_keys: 0,
            key_ptr: 0,
            next_keys: 0,
            slot_idx: 0,
            target_len: 0,
        };
    });
}

#[test]
fn transition_cache_lookup_rejects_slot_key_mismatch() {
    // #6006: `prev_keys` / `key_ptr` are raw addresses that GC does not
    // relocate. When GC frees a keys_array and recycles its address into an
    // unrelated array, a stale entry can pointer-match a *different* shape —
    // one where `next_keys[slot_idx]` is a DIFFERENT key. Adopting that edge
    // would store the value at the wrong slot (keys_array looks right but the
    // read returns undefined). The content check must reject such an edge so
    // the caller falls back to the correct slow path.
    let want = crate::string::js_string_from_bytes(b"alpha".as_ptr(), 5);
    let other = crate::string::js_string_from_bytes(b"beta".as_ptr(), 4);

    // A target shape whose slot 0 holds `beta`, not `alpha`.
    let keys = crate::array::js_array_alloc(4);
    let keys = crate::array::js_array_push(keys, JSValue::string_ptr(other));

    // Insert an edge keyed on (prev=0, `alpha`) but targeting the `beta` shape,
    // mirroring a recycled-address false match (target_len is set because the
    // length matches slot_idx+1, so only the content check can catch it).
    transition_cache_insert(0, want, keys as usize, 0);

    assert!(
        transition_cache_lookup(0, want).is_none(),
        "a cache edge whose target slot holds a different key must be rejected (#6006)"
    );

    // Sanity: an edge whose target slot DOES hold the key still hits.
    let good_keys = crate::array::js_array_alloc(4);
    let good_keys = crate::array::js_array_push(good_keys, JSValue::string_ptr(want));
    transition_cache_insert(0, want, good_keys as usize, 0);
    assert!(
        transition_cache_lookup(0, want).is_some(),
        "a genuine edge (target slot holds the key) must still hit (#6006)"
    );

    let slot = transition_cache_slot(0, want as usize);
    with_transition_cache(|t| unsafe {
        // GC_STORE_AUDIT(ROOT): test cleanup writes non-pointer sentinels into scanned TRANSITION_CACHE_GLOBAL roots.
        (*t)[slot] = TransitionEntry {
            prev_keys: 0,
            key_ptr: 0,
            next_keys: 0,
            slot_idx: 0,
            target_len: 0,
        };
    });
}

#[test]
fn transition_cache_lookup_rejects_grown_shared_target() {
    // #6006: a cached edge's `target_len` is a snapshot. The shared target
    // keys_array can grow IN PLACE after caching (a later object extends the
    // same shape), so `target_len == slot_idx + 1` still passes while the
    // actual array is now longer. Adopting it would give the object a
    // keys_array with more keys than field_count tracks — keys present, values
    // undefined. The exact-length content check must catch the grown array.
    let key = crate::string::js_string_from_bytes(b"gamma".as_ptr(), 5);
    let extra = crate::string::js_string_from_bytes(b"delta".as_ptr(), 5);

    // A 1-key target with spare capacity, cached as a slot-0 edge (target_len=1).
    let keys = crate::array::js_array_alloc(4);
    let keys = crate::array::js_array_push(keys, JSValue::string_ptr(key));
    transition_cache_insert(0, key, keys as usize, 0);
    assert!(
        transition_cache_lookup(0, key).is_some(),
        "sanity: a genuine 1-key edge hits before the target grows (#6006)"
    );

    // Grow the SAME array in place to length 2 (as a sibling object would).
    let keys2 = crate::array::js_array_push(keys, JSValue::string_ptr(extra));
    // `js_array_push` grows in place when capacity allows (cap was 4), so the
    // cached `next_keys` pointer still points at the now-length-2 array.
    assert_eq!(
        keys2, keys,
        "test setup: push must grow in place, not realloc"
    );

    assert!(
        transition_cache_lookup(0, key).is_none(),
        "a cache edge whose shared target grew past slot_idx+1 must be rejected (#6006)"
    );

    let slot = transition_cache_slot(0, key as usize);
    with_transition_cache(|t| unsafe {
        // GC_STORE_AUDIT(ROOT): test cleanup writes non-pointer sentinels into scanned TRANSITION_CACHE_GLOBAL roots.
        (*t)[slot] = TransitionEntry {
            prev_keys: 0,
            key_ptr: 0,
            next_keys: 0,
            slot_idx: 0,
            target_len: 0,
        };
    });
}

#[test]
fn entries_and_values_skip_non_enumerable_descriptor_slots() {
    // #5046: Object.defineProperty(o, 'hidden', { value: 1 }) defaults to
    // enumerable: false. Object.keys filtered it; entries/values did not.
    {
        let obj = js_object_alloc(0, 0);
        let hidden_key = crate::string::js_string_from_bytes(b"hidden".as_ptr(), 6);
        let shown_key = crate::string::js_string_from_bytes(b"shown".as_ptr(), 5);
        let value_key = crate::string::js_string_from_bytes(b"value".as_ptr(), 5);

        let descriptor = js_object_alloc(0, 0);
        js_object_set_field_by_name(descriptor, value_key, 1.0);

        let obj_value = crate::value::js_nanbox_pointer(obj as i64);
        let hidden_value = f64::from_bits(JSValue::string_ptr(hidden_key).bits());
        let descriptor_value = crate::value::js_nanbox_pointer(descriptor as i64);
        js_object_define_property(obj_value, hidden_value, descriptor_value);
        js_object_set_field_by_name(obj as *mut ObjectHeader, shown_key, 2.0);

        let keys = js_object_keys(obj);
        assert_eq!(crate::array::js_array_length(keys), 1);
        assert_eq!(
            js_string_to_rust(crate::array::js_array_get(keys, 0).into()),
            "shown"
        );

        let values = js_object_values(obj);
        assert_eq!(crate::array::js_array_length(values), 1);
        assert_eq!(
            crate::array::js_array_get(values, 0).bits(),
            2.0f64.to_bits()
        );

        let entries = js_object_entries(obj);
        assert_eq!(crate::array::js_array_length(entries), 1);
        let pair = crate::value::js_nanbox_get_pointer(f64::from_bits(
            crate::array::js_array_get(entries, 0).bits(),
        )) as *const crate::array::ArrayHeader;
        assert_eq!(
            js_string_to_rust(crate::array::js_array_get(pair, 0).into()),
            "shown"
        );
        assert_eq!(crate::array::js_array_get(pair, 1).bits(), 2.0f64.to_bits());
    }
}

/// #5054: wide objects (≥257 keys) read through the validated key→index map;
/// the dynamic-write fast path must still respect descriptors installed later.
#[test]
fn wide_object_index_reads_and_descriptor_writes() {
    {
        let obj = js_object_alloc(0, 0);
        let n = 600u32;
        for i in 0..n {
            let name = format!("w{}", i);
            let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            js_object_set_field_by_name(obj, key, i as f64);
        }
        for i in 0..n {
            let name = format!("w{}", i);
            let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            let v = js_object_get_field_by_name(obj as *const ObjectHeader, key);
            assert_eq!(f64::from_bits(v.bits()), i as f64, "read-back of {}", name);
        }
        // Missing key stays undefined (index miss → scan → not found).
        let missing = crate::string::js_string_from_bytes(b"nope".as_ptr(), 4);
        assert!(crate::value::JSValue::from_bits(
            js_object_get_field_by_name(obj as *const ObjectHeader, missing).bits()
        )
        .is_undefined());

        // Install a non-writable descriptor on one key; the put_value_set
        // fast path must bail to the descriptor-aware walk and reject the
        // write (sloppy mode: value unchanged, no throw).
        let obj_value = crate::value::js_nanbox_pointer(obj as i64);
        let target_name = b"w42";
        let target_key = crate::string::js_string_from_bytes(target_name.as_ptr(), 3);
        let value_key = crate::string::js_string_from_bytes(b"value".as_ptr(), 5);
        let writable_key = crate::string::js_string_from_bytes(b"writable".as_ptr(), 8);
        let descriptor = js_object_alloc(0, 0);
        js_object_set_field_by_name(descriptor, value_key, 42.0);
        js_object_set_field_by_name(
            descriptor,
            writable_key,
            f64::from_bits(crate::value::TAG_FALSE),
        );
        crate::object::object_ops::js_object_define_property(
            obj_value,
            f64::from_bits(JSValue::string_ptr(target_key).bits()),
            crate::value::js_nanbox_pointer(descriptor as i64),
        );
        crate::proxy::js_put_value_set(
            obj_value,
            f64::from_bits(JSValue::string_ptr(target_key).bits()),
            777.0,
            obj_value,
            0,
        );
        let after = js_object_get_field_by_name(obj as *const ObjectHeader, target_key);
        assert_eq!(f64::from_bits(after.bits()), 42.0);

        // Writes to other keys still go through (fast path off for this
        // object now — but correctness preserved either way).
        let other_key = crate::string::js_string_from_bytes(b"w43".as_ptr(), 3);
        crate::proxy::js_put_value_set(
            obj_value,
            f64::from_bits(JSValue::string_ptr(other_key).bits()),
            4343.0,
            obj_value,
            0,
        );
        let v43 = js_object_get_field_by_name(obj as *const ObjectHeader, other_key);
        assert_eq!(f64::from_bits(v43.bits()), 4343.0);
    }
}

#[test]
fn sloppy_put_value_rejects_disposable_stack_getter_without_own_shadow() {
    unsafe {
        let stack = crate::disposable::js_disposable_stack_new();
        let key = crate::string::js_string_from_bytes(b"disposed".as_ptr(), 8);
        let stack_value = crate::value::js_nanbox_pointer(stack as i64);
        let key_value = f64::from_bits(JSValue::string_ptr(key).bits());

        crate::proxy::js_put_value_set(stack_value, key_value, 1.0, stack_value, 0);

        assert_eq!(
            crate::disposable::js_disposable_stack_disposed(stack).to_bits(),
            crate::value::TAG_FALSE
        );
        assert!(
            !own_key_present(stack, key),
            "a sloppy write to the inherited getter-only accessor must be a silent no-op"
        );
    }
}

/// #5736: `own_key_present` on a wide object (≥257 keys — e.g. a barrel
/// `export *` namespace) must use the O(1) wide-key index rather than an O(n)
/// keys_array scan, so `Object.values`/`Object.entries` (which re-check every
/// own key) don't degrade to O(n²). Correctness must be preserved: present keys
/// resolve, absent keys don't, and `Object.values` yields every value.
#[test]
fn wide_object_own_key_present_uses_index_and_object_values_is_complete() {
    unsafe {
        let obj = js_object_alloc(0, 0);
        let n = 600u32;
        for i in 0..n {
            let name = format!("w{}", i);
            let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            js_object_set_field_by_name(obj, key, i as f64);
        }
        // Every present key is found through the wide-index probe.
        for i in [0u32, 1, 42, 256, 257, 300, 599] {
            let name = format!("w{}", i);
            let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            assert!(
                own_key_present(obj, key),
                "present key {name} must be found"
            );
        }
        // Absent keys fall through the index miss to the linear scan → false.
        for name in ["nope", "w600", "w-1", ""] {
            let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            assert!(
                !own_key_present(obj, key),
                "absent key {name:?} must not be found"
            );
        }
        // `Object.values` must still enumerate every value exactly once.
        let values = crate::object::js_object_values(obj as *const ObjectHeader);
        assert_eq!(
            crate::array::js_array_length(values),
            n,
            "Object.values must yield one value per key"
        );
        // Track each payload so a balanced duplicate/omission can't slip past a
        // length+sum check: every value 0..n must appear exactly once.
        let mut seen = vec![false; n as usize];
        for i in 0..n {
            let v = crate::array::js_array_get(values, i);
            let num = f64::from_bits(v.bits());
            let idx = num as usize;
            assert_eq!(num, idx as f64, "Object.values must yield integer payloads");
            assert!(
                (idx as u32) < n,
                "Object.values yielded out-of-range value {num}"
            );
            assert!(!seen[idx], "Object.values yielded duplicate value {idx}");
            seen[idx] = true;
        }
        assert!(
            seen.into_iter().all(|hit| hit),
            "Object.values missed at least one value"
        );
    }
}

/// `js_object_to_string` must NOT dereference a handle-band value (a Web Fetch
/// `Headers`/`Request`/`Response`/`Blob` registry id, or any other small native
/// handle) as a heap pointer. Such ids are NaN-boxed as `POINTER_TAG` values but
/// are not `GcHeader`-prefixed objects; reading the GC type byte at `id - 8` (or
/// `(*ObjectHeader).class_id` at `id`) faults on unmapped low memory. This is
/// the `claude -p` SIGSEGV (`EXC_BAD_ACCESS` at `0x3FFFB` == `0x40003 - 8`),
/// where the SDK coerced a `Headers` handle to a string while building a
/// request. The brand must fall through to the generic `[object Object]` tag.
#[test]
fn object_to_string_rejects_handle_band_ids() {
    use crate::value::addr_class;
    for &id in &[
        addr_class::FETCH_HANDLE_BAND_START,     // 0x40000
        addr_class::FETCH_HANDLE_BAND_START + 3, // the 0x40003 from the crash
        addr_class::HANDLE_BAND_MAX - 1,         // 0xFFFFF
        1usize,                                  // common native handle
    ] {
        assert!(addr_class::is_handle_band(id));
        let handle = crate::value::js_nanbox_pointer(id as i64);
        // Must return a string brand without dereferencing the bogus pointer.
        let result = unsafe { js_object_to_string(handle) };
        let s = js_string_to_rust(JSValue::from_bits(result.to_bits()));
        assert_eq!(
            s, "[object Object]",
            "handle-band id {id:#x} must brand as [object Object], got {s:?}"
        );
    }
}

/// #5437 — captured-`undefined` tag-loss on Next.js dynamic/API routes.
///
/// `js_class_capture_value_or` must NOT replace a snapshot whose slot is a
/// genuinely-undefined capture (`TAG_UNDEFINED`) with a tag-stripped/mis-boxed
/// raw-word `fallback` (`0x0000_0000_0000_0001` — `TAG_UNDEFINED` with its
/// `0x7FFC` NaN-box tag stripped). The bundle's `let t_ = cond ? fn : void 0`
/// debug logger is `undefined`; at giant-module scale the `new`-site appended
/// fallback for it materialized as `0x1`, so the snapshot's correct `undefined`
/// was discarded → `t_` became `0x1` → `null == t_` false → `t_(…)` called →
/// "value is not a function" → route 500.
#[test]
fn class_capture_value_or_rejects_tag_stripped_fallback() {
    const TAG_UNDEFINED: u64 = crate::value::TAG_UNDEFINED; // 0x7FFC_0000_0000_0001
    const STRIPPED: u64 = 0x0000_0000_0000_0001; // tag-stripped undefined
    let undef = f64::from_bits(TAG_UNDEFINED);
    let stripped = f64::from_bits(STRIPPED);

    // Case 1 (THE BUG): snapshot slot is genuinely `undefined`, fallback is the
    // tag-stripped `0x1`. Must return `undefined`, NOT the corrupt fallback.
    let cid_a: u32 = 0x5437_0001;
    let snap_a = [TAG_UNDEFINED, TAG_UNDEFINED, TAG_UNDEFINED];
    unsafe {
        js_class_register_capture_values(cid_a, snap_a.as_ptr() as *const f64, snap_a.len());
    }
    let got = js_class_capture_value_or(cid_a, 1, stripped).to_bits();
    assert_eq!(
        got, TAG_UNDEFINED,
        "undefined snapshot + tag-stripped fallback must yield undefined, got {got:#018x}"
    );

    // Case 2 (W6 — snapshot wins): a real pointer in the snapshot stays
    // authoritative even when the fallback is a (different) real value.
    let cid_b: u32 = 0x5437_0002;
    let real_ptr = crate::value::POINTER_TAG | 0x1234_5678;
    let snap_b = [real_ptr];
    unsafe {
        js_class_register_capture_values(cid_b, snap_b.as_ptr() as *const f64, snap_b.len());
    }
    let other = f64::from_bits(crate::value::POINTER_TAG | 0xDEAD);
    let got_b = js_class_capture_value_or(cid_b, 0, other).to_bits();
    assert_eq!(
        got_b, real_ptr,
        "non-undefined snapshot slot must win over the fallback (W6), got {got_b:#018x}"
    );

    // Case 3 (#5437 hoisted-class/TDZ — VALID fallback over undefined snapshot
    // still wins): snapshot slot is `undefined` (class decl hoisted above the
    // local's assignment) but the fallback is a legitimate NaN-boxed value.
    let cid_c: u32 = 0x5437_0003;
    let snap_c = [TAG_UNDEFINED];
    unsafe {
        js_class_register_capture_values(cid_c, snap_c.as_ptr() as *const f64, snap_c.len());
    }
    let valid_fb = crate::value::POINTER_TAG | 0xCAFE;
    let got_c = js_class_capture_value_or(cid_c, 0, f64::from_bits(valid_fb)).to_bits();
    assert_eq!(
        got_c, valid_fb,
        "undefined snapshot + VALID fallback must keep the fallback (TDZ fix), got {got_c:#018x}"
    );

    // Case 4 (no snapshot + tag-stripped fallback): with no registered snapshot
    // a corrupt `0x1` fallback is not callable, so resolve to `undefined`.
    let cid_d: u32 = 0x5437_0004; // never registered
    let got_d = js_class_capture_value_or(cid_d, 0, stripped).to_bits();
    assert_eq!(
        got_d, TAG_UNDEFINED,
        "no snapshot + tag-stripped fallback must yield undefined, got {got_d:#018x}"
    );

    // Case 5 (no snapshot + valid fallback): the appended cap value is used
    // (getSpan/require-derived-capture path preserved).
    let cid_e: u32 = 0x5437_0005; // never registered
    let valid2 = crate::value::POINTER_TAG | 0xBEEF;
    let got_e = js_class_capture_value_or(cid_e, 0, f64::from_bits(valid2)).to_bits();
    assert_eq!(
        got_e, valid2,
        "no snapshot + valid fallback must use the fallback, got {got_e:#018x}"
    );

    // Sanity: `0.0` (the number zero) is a legitimate captured value and must
    // NOT be treated as a tag-stripped word.
    assert!(!fallback_is_tag_stripped(0.0_f64));
    assert!(fallback_is_tag_stripped(stripped));
    assert!(!fallback_is_tag_stripped(undef));
}

/// Reading `.size` on a `Map` *by name* — the shape a minified bundle produces
/// when the receiver's `Map` type is erased to `any` (`map.size` dispatched
/// through `js_object_get_field_by_name`) — reaches the `.size` fast path,
/// which calls `own_key_present(map, "size")`. A `MapHeader` is 16 bytes
/// (`size`/`capacity`/`entries`) with no `keys_array` field at offset 16, so
/// `(*obj).keys_array` used to read 8 bytes past the header into the adjacent
/// allocation; that stray word cleared the keys-pointer alignment/range guard
/// and then SIGBUS'd on the `[keys-8]` GC-type-tag load. `own_key_present` now
/// answers `false` for a non-`GC_TYPE_OBJECT` receiver, so the read falls
/// through to the `Map.size` tail instead of dereferencing garbage.
#[test]
fn map_size_by_name_does_not_oob_read_keys_array() {
    unsafe {
        let size_key = crate::string::js_string_from_bytes(b"size".as_ptr(), 4);

        // Empty Map — the exact shape observed crashing (size 0).
        let empty = crate::map::js_map_alloc(4);
        assert!(!empty.is_null());
        // The precise frame that faulted: a Map is not an object, so it has no
        // own string key. This must answer false without dereferencing
        // `[obj+16]` past the 16-byte MapHeader.
        assert!(!own_key_present(empty as *mut ObjectHeader, size_key));
        let v0 = crate::object::js_object_get_field_by_name(empty as *const ObjectHeader, size_key);
        assert!(v0.is_number(), "empty Map .size must be a number");
        assert_eq!(v0.as_number(), 0.0, "empty Map .size");

        // Populated Map — `.size` by name must still return the real size.
        let m = crate::map::js_map_alloc(4);
        crate::map::js_map_set(m, 10.0, 100.0);
        crate::map::js_map_set(m, 20.0, 200.0);
        assert!(!own_key_present(m as *mut ObjectHeader, size_key));
        let v2 = crate::object::js_object_get_field_by_name(m as *const ObjectHeader, size_key);
        assert!(v2.is_number(), "populated Map .size must be a number");
        assert_eq!(v2.as_number(), 2.0, "populated Map .size");
    }
}

/// #7518: a `globalThis` built-in CONSTRUCTOR reached as a VALUE must never be
/// re-dispatched as a method name on `IMPLICIT_THIS`.
///
/// `try_dispatch_value_called_proto_method` exists for the #3716 uncurry-this
/// idiom: a built-in *prototype method* invoked as a value arrives backed by the
/// shared `global_this_builtin_noop_thunk`, so the helper recovers its recorded
/// `name` and re-dispatches `IMPLICIT_THIS.<name>(…)` through the real by-name
/// tower. Global constructors share that same no-op thunk, and the only thing
/// keeping them out was incidental — they recorded no builtin `.length`.
///
/// c6ed8175d (#6853) added `EventTarget` to `builtin_constructor_spec_length` so
/// `EventTarget.length` reads `0` like Node. That gave the EventTarget global a
/// recorded length, opened the gate, and re-broke #6301: `class Bus extends
/// EventTarget {}` has no static parent class id, so its `super()` runs the
/// parent VALUE through `js_fetch_or_value_super` — which binds `IMPLICIT_THIS`
/// to the new instance before the value call — and the helper turned that into
/// `bus.EventTarget()`, whose miss throws `TypeError: EventTarget is not a
/// function`. `parity` is tag-gated, so the gap test that covers this sat red on
/// `main` for a week unnoticed; this assertion lives in the per-PR `cargo-test`
/// tier instead.
///
/// Walks the whole table so a future `builtin_constructor_spec_length` addition
/// cannot silently re-open the hole for a different name.
/// `test_global_this_builtin_constructor_value` builds the no-op-thunk shape for
/// every name, including the ones `populate_global_this_builtins` currently gives
/// a dedicated thunk — deliberately: the assertion is about the helper's contract
/// for a constructor NAME, so it stays meaningful if a name is later moved onto
/// the shared thunk. Reverting the exclusion fails this on all ~70 entries.
#[test]
fn global_builtin_constructor_values_are_not_redispatched_by_name() {
    // The closure `name` / `length` props these assertions read live in the
    // PROCESS-global CLOSURE_PROPS table (#6965).
    let _global = crate::gc::global_side_table_test_lock();
    // Give the pre-fix failure mode a real receiver to miss on, so a regression
    // surfaces as a clean catchable throw rather than a dispatch on whatever
    // `IMPLICIT_THIS` happened to hold.
    let receiver = crate::value::js_nanbox_pointer(js_object_alloc(0, 0) as i64);
    let prev_this = crate::object::js_implicit_this_set(receiver);

    let mut with_recorded_length = 0usize;
    let mut offenders: Vec<String> = Vec::new();
    for name in GLOBAL_THIS_BUILTIN_CONSTRUCTORS.iter().copied() {
        let ctor_raw = test_global_this_builtin_constructor_value(name);
        let ctor = JSValue::from_bits(ctor_raw.to_bits());
        assert!(
            ctor.is_pointer(),
            "{name} should be a closure-backed global"
        );
        let closure = ctor.as_pointer::<crate::closure::ClosureHeader>();
        if super::native_module::builtin_closure_length(closure as usize).is_some() {
            with_recorded_length += 1;
        }
        let verdict = catch_js(|| {
            match unsafe {
                crate::object::try_dispatch_value_called_proto_method(closure, std::ptr::null(), 0)
            } {
                None => 1.0,
                Some(_) => 0.0,
            }
        });
        match verdict {
            Ok(v) if v == 1.0 => {}
            Ok(_) => offenders.push(format!("{name} (re-dispatched by name)")),
            Err(_) => offenders.push(format!("{name} (threw)")),
        }
    }

    crate::object::js_implicit_this_set(prev_this);

    assert!(
        offenders.is_empty(),
        "globalThis built-in constructors must not be value-dispatched as \
         `IMPLICIT_THIS.<Name>(…)`; offenders: {offenders:?}"
    );
    // Non-vacuity: the bug needs the no-op thunk PLUS a recorded spec `.length`.
    // If nothing in the table carries a length, every entry above declined at the
    // `.length` gate and this test proved nothing about the exclusion.
    assert!(
        with_recorded_length > 0,
        "no globalThis built-in constructor carries a recorded builtin `.length` — \
         this test can no longer reach the shape it guards"
    );
    // And pin the specific input that regressed: the fix is the explicit
    // constructor exclusion, NOT dropping the Node-parity `.length` #6853 added.
    assert!(
        crate::object::builtin_constructor_spec_length("EventTarget").is_some(),
        "#7518: EventTarget must keep its spec `.length`"
    );
}

/// #7548: the array branches of `Object.*` reinterpreted the caller's
/// `ObjectHeader` pointer as an `ArrayHeader` with a bare cast. When a JS
/// binding still holds an array's PRE-GROW address, that pointer is a #233
/// forwarding stub whose first 8 bytes — exactly `length` and `capacity` —
/// have been overwritten with the forwarding POINTER, so `(*arr).length` read
/// back a heap address (~6·10^8 in the wild) instead of the real length.
///
/// Two loops are driven by that value and became bounded-but-unreachable
/// walks — one `to_string()` plus a side-table probe per index, hundreds of
/// millions of iterations, which presents as a hang:
///   * `mark_all_array_props`          — `Object.freeze` / `Object.seal`.
///   * `array_set_length_from_descriptor` — ArraySetLength's shrink walk, the
///     `Set(receiver, "length", …)` tail of a Proxy-receiver `splice` that grows.
///
/// The header read is asserted FIRST and directly: a regression must fail fast
/// here rather than hang the suite in one of the walks below.
#[test]
fn stale_pre_grow_array_pointer_reads_the_real_length_in_object_ops() {
    let mut arr = crate::array::js_array_alloc(0);
    let stale = arr;
    let capacity = unsafe { (*arr).capacity };
    for i in 0..capacity {
        arr = crate::array::js_array_push_f64(arr, i as f64);
    }
    // One more push exceeds the dense capacity: the array is reallocated and a
    // forwarding stub is left behind at `stale`.
    let grown = crate::array::js_array_push_f64(arr, capacity as f64);
    assert_ne!(
        grown as usize, stale as usize,
        "pushing past capacity must reallocate — otherwise no stub exists and \
         this test proves nothing"
    );
    let real_len = crate::array::js_array_length(grown);
    assert_eq!(real_len, capacity + 1);

    // Non-vacuity: the stub's payload must really be clobbered, or the bare
    // cast below would have been harmless all along.
    let raw_len = unsafe { (*(stale as *const crate::array::ArrayHeader)).length };
    assert_ne!(
        raw_len, real_len,
        "the forwarding stub's length word must be clobbered for this test to \
         exercise #7548"
    );

    // The fix: resolve the chain before reading the header.
    let resolved = unsafe { super::array_object_ops::array_header(stale as *const ObjectHeader) };
    assert_eq!(
        unsafe { (*resolved).length },
        real_len,
        "#7548: a stale pre-grow array pointer must resolve to the array's \
         current home before its length is read"
    );

    // `Object.freeze`'s index walk now terminates at the real length: it must
    // record attrs for every real index and none beyond it.
    unsafe {
        super::array_object_ops::mark_all_array_props(stale as *mut ObjectHeader, true, true);
    }
    let addr = stale as usize;
    assert!(
        crate::object::get_property_attrs(addr, &(real_len - 1).to_string()).is_some(),
        "the freeze walk must reach the array's last real index"
    );
    assert!(
        crate::object::get_property_attrs(addr, &real_len.to_string()).is_none(),
        "the freeze walk must not run past the array's real length"
    );
}

/// #7563: an ARRAY receiver must never be read back as a class instance.
///
/// `ObjectHeader` is `{ class_id: u32, parent_class_id: u32, … }` and
/// `ArrayHeader` is `{ length: u32, capacity: u32 }`, so the two u32s at offset
/// 0 alias — an array read as an `ObjectHeader` reports its **length** as a
/// `class_id`. (#8113 moved this from offset 4 / `capacity` when it deleted the
/// leading `object_type` word. Note that makes the collision DENSER, not
/// sparser: array lengths are small and consecutive, and so are class ids.)
///
/// That mattered because `arr[Symbol.iterator]` resolves through
/// `js_class_method_bind(arr, "values")`, whose receiver→class step used a bare
/// `(*obj).class_id` read instead of the guarded `js_object_get_class_id`. Any
/// class whose id equalled the array's capacity and which owned a `values`
/// method therefore captured the array's iterator. When that class was the
/// *calling* class — `class C { values() { return [x][Symbol.iterator](); } }`
/// — `values` re-entered `values` until the stack guard page, i.e. a SIGSEGV
/// with no `Map` anywhere in the program.
#[test]
fn array_receiver_is_never_read_as_a_class_id() {
    let arr = crate::array::js_array_alloc(3);
    assert!(!arr.is_null());
    crate::array::js_array_push(arr, crate::JSValue::from_bits(1.0f64.to_bits()));
    // Impersonate exactly the class id this array's bytes would have yielded.
    // #8113: that is `length`, at offset 0, not `capacity`.
    let impersonated = unsafe { (*arr).length };
    assert_ne!(
        impersonated, 0,
        "the test is vacuous unless the length is a non-zero (i.e. lookup-able) class id"
    );

    let arr_value = crate::value::js_nanbox_pointer(arr as i64);
    assert_eq!(
        super::native_module::class_id_from_method_receiver(arr_value),
        None,
        "an array is not a class instance: its capacity must not be read as a class id"
    );

    // The guard must not over-narrow. A genuine class instance carrying the
    // very same id still resolves, so the bound-method identity path (#446)
    // keeps working.
    let obj = js_object_alloc(impersonated, 0);
    assert!(!obj.is_null());
    let obj_value = crate::value::js_nanbox_pointer(obj as i64);
    assert_eq!(
        super::native_module::class_id_from_method_receiver(obj_value),
        Some(impersonated),
        "a real class instance must still resolve to its class id"
    );
}

/// #7689: `const f = C.m; f(...)` — a method value read off a CONSTRUCTOR
/// class ref — must invoke the STATIC method when the class declares both a
/// static and an instance method of the same name.
///
/// `js_class_method_bind`'s #446 method-identity canonicalization resolved
/// the name against the INSTANCE vtable (`class_id_from_method_receiver`
/// treats a class ref like an instance receiver), so the extracted value was
/// the prototype method. marked's `Lexer` has exactly this collision
/// (`static lex` + instance `lex`): `const lexer2 = _Lexer.lex;
/// lexer2(src, opt)` ran the instance `lex` with no constructed receiver and
/// every `marked.parse` threw "Cannot read properties of undefined (reading
/// 'pedantic')".
#[test]
fn constructor_ref_method_value_resolves_static_over_instance_method() {
    // Unique id so the process-global registries don't collide with other tests.
    const LEX_METHOD_TEST_CLASS_ID: u32 = 0x7689;
    const NAME: &[u8] = b"lex";

    extern "C" fn static_lex_7689() -> f64 {
        42.0
    }
    extern "C" fn instance_lex_7689(_this: f64) -> f64 {
        7.0
    }

    unsafe {
        super::class_registry::js_register_class_method(
            LEX_METHOD_TEST_CLASS_ID as i64,
            NAME.as_ptr(),
            NAME.len() as i64,
            instance_lex_7689 as *const () as usize as i64,
            0,
            0,
            0,
        );
        super::class_registry::js_register_class_static_method(
            LEX_METHOD_TEST_CLASS_ID as i64,
            NAME.as_ptr(),
            NAME.len() as i64,
            static_lex_7689 as *const () as usize as i64,
            0,
            0,
        );
    }

    let class_ref = super::native_module::class_constructor_ref_value(LEX_METHOD_TEST_CLASS_ID);
    let bound = super::native_module::js_class_method_bind(class_ref, NAME.as_ptr(), NAME.len());
    let result = unsafe { crate::closure::js_native_call_value(bound, std::ptr::null(), 0) };
    assert_eq!(
        result, 42.0,
        "a method value extracted off the CONSTRUCTOR ref must dispatch the \
         static `lex`, not the same-named instance method"
    );

    // The guard must not over-narrow: the PROTOTYPE ref names the instance
    // method, and an extracted `C.prototype.lex` must keep resolving it.
    let proto_ref = super::native_module::class_prototype_ref_value(LEX_METHOD_TEST_CLASS_ID);
    let bound_proto =
        super::native_module::js_class_method_bind(proto_ref, NAME.as_ptr(), NAME.len());
    let result_proto =
        unsafe { crate::closure::js_native_call_value(bound_proto, std::ptr::null(), 0) };
    assert_eq!(
        result_proto, 7.0,
        "a method value extracted off the PROTOTYPE ref must still dispatch \
         the instance `lex`"
    );
}

/// #8117: a `Buffer` / `DataView` receiver must not reach the ordinary
/// `ObjectHeader` walk in `obj_value_has_own_key`.
///
/// A buffer is a `BufferHeader` — no `class_id`, no `keys_array`. With no arm
/// of its own it fell through to the ordinary arm, which read
/// `(*obj).keys_array` out of the bytes that follow a buffer header and handed
/// that to `js_array_length`, whose lazy-array probe dereferences `addr - 8`.
///
/// The two platforms fail differently, which is why this test asserts the
/// ANSWER rather than merely "did not crash":
///
/// * Linux: the payload bytes clear the old `< 0x10000` magnitude floor and the
///   dereference is a SIGSEGV. `b.readUInt8 = fn` reached through the dynamic
///   `[[Set]]` (`js_put_value_set_dyn_ic_miss` -> `proxy::ordinary_set_with_
///   receiver` -> `proxy::own_set_descriptor`) crashed 10/10.
/// * macOS: the heap floor is high enough that the garbage usually reads as
///   null, so it silently answered "no own key" for a property the buffer
///   really owns.
///
/// The first assertion below fails on BOTH.
#[test]
fn buffer_own_key_comes_from_the_expando_table_not_the_object_walk() {
    let addr = crate::buffer::buffer_alloc(8) as usize;
    crate::buffer::buffer_set_own_prop(addr, "myFlag", 42.0);
    let receiver = crate::value::js_nanbox_pointer(addr as i64);

    let present = crate::string::js_string_from_bytes(b"myFlag".as_ptr(), 6);
    let present_key = crate::value::js_nanbox_string(present as i64);
    assert!(
        obj_value_has_own_key(receiver, present_key),
        "a buffer's own expando property must be reported as an own key"
    );

    // A `Buffer.prototype` method is INHERITED, not own. That is what lets
    // `buf.readUInt8 = fn` install a shadowing own property instead of
    // being treated as the redefinition of an existing one.
    let inherited = crate::string::js_string_from_bytes(b"readUInt8".as_ptr(), 9);
    let inherited_key = crate::value::js_nanbox_string(inherited as i64);
    assert!(
        !obj_value_has_own_key(receiver, inherited_key),
        "a Buffer.prototype method is inherited, not an own key"
    );

    // And a key the buffer has never seen.
    let absent = crate::string::js_string_from_bytes(b"nope".as_ptr(), 4);
    let absent_key = crate::value::js_nanbox_string(absent as i64);
    assert!(
        !obj_value_has_own_key(receiver, absent_key),
        "an unknown key is not an own key"
    );
}
// ---------------------------------------------------------------------------
// #8113 — the trap this header shrink had to disarm.
//
// `ObjectHeader` used to open with `object_type: u32`, prefix-punned against
// `error::ErrorHeader`'s first word, and NINE sites read raw offset 0 to answer
// "is this an Error?". Deleting the word makes offset 0 `class_id` — and
// `OBJECT_TYPE_ERROR` is **2**, while class ids are handed out from 1, densely,
// in source-declaration order. So a surviving raw read reclassifies every
// instance of the SECOND class a program declares as an `ErrorHeader` and reads
// `message`/`name`/`stack`/`errors` out of its field slots: a silent wrong
// answer of exactly the #8100 shape.
//
// These tests are SABOTAGE-SHAPED. Each first asserts that the confusable value
// really is sitting at offset 0 — so a green run proves the GcHeader-kind test
// fired, not that the fixture happened to look harmless.
// ---------------------------------------------------------------------------

/// The premise: an ordinary object CAN carry `class_id == OBJECT_TYPE_ERROR`,
/// and that value really is the first word of its header.
#[test]
fn an_ordinary_object_can_carry_the_error_type_tag_as_its_class_id() {
    let obj = js_object_alloc(crate::error::OBJECT_TYPE_ERROR, 2);
    assert!(!obj.is_null());
    unsafe {
        assert_eq!((*obj).class_id, crate::error::OBJECT_TYPE_ERROR);
        // Offset 0, read the way the retired discriminators read it.
        let raw_word_0 = std::ptr::read(obj as *const u32);
        assert_eq!(
            raw_word_0,
            crate::error::OBJECT_TYPE_ERROR,
            "test premise: the pre-#8113 raw offset-0 read now yields \
             OBJECT_TYPE_ERROR for an ordinary object"
        );
    }
}

/// `Error.isError()` must not be fooled by it. (`error.rs:750`.)
#[test]
fn error_is_error_rejects_an_object_whose_class_id_equals_the_error_tag() {
    let obj = js_object_alloc(crate::error::OBJECT_TYPE_ERROR, 2);
    let value = crate::value::js_nanbox_pointer(obj as i64);
    assert_eq!(
        crate::error::js_error_is_error(value).to_bits(),
        crate::value::TAG_FALSE,
        "class_id == OBJECT_TYPE_ERROR must not read as a native Error"
    );

    // Not over-narrowed: a real Error still answers true.
    let real = crate::error::js_error_new_with_message(crate::string::js_string_from_bytes(
        b"boom".as_ptr(),
        4,
    ));
    let real_value = crate::value::js_nanbox_pointer(real as i64);
    assert_eq!(
        crate::error::js_error_is_error(real_value).to_bits(),
        crate::value::TAG_TRUE,
        "a genuine ErrorHeader must still classify as an Error"
    );
}

/// `js_error_get_errors` must resolve `.errors` GENERICALLY for it rather than
/// returning the fixed `ErrorHeader.errors` slot. (`error.rs:1542`; the doc
/// there records the for-of corruption the fixed-slot read caused.)
#[test]
fn error_get_errors_does_not_read_a_fixed_slot_off_a_colliding_class_id() {
    let obj = js_object_alloc(crate::error::OBJECT_TYPE_ERROR, 2);
    unsafe {
        assert_eq!((*obj).class_id, crate::error::OBJECT_TYPE_ERROR);
        // Poison the slot the ErrorHeader layout would call `errors`.
        let key = crate::string::js_string_from_bytes(b"errors".as_ptr(), 6);
        let arr = crate::array::js_array_alloc(1);
        crate::object::js_object_set_field_by_name(
            obj,
            key,
            f64::from_bits(crate::value::js_nanbox_pointer(arr as i64).to_bits()),
        );
        let got = crate::error::js_error_get_errors(obj as *mut crate::error::ErrorHeader);
        assert_eq!(
            got as usize, arr as usize,
            "`.errors` on a class_id == 2 object must resolve as an ordinary \
             own property, not as ErrorHeader's fixed slot"
        );
    }
}

/// `js_dynamic_object_keys` must return the object's real keys, not the Error
/// triple. (`value/dynamic_object.rs:728`.)
#[test]
fn dynamic_object_keys_are_not_the_error_triple_for_a_colliding_class_id() {
    let obj = js_object_alloc(crate::error::OBJECT_TYPE_ERROR, 2);
    unsafe {
        let key = crate::string::js_string_from_bytes(b"kk8113".as_ptr(), 6);
        crate::object::js_object_set_field_by_name(obj, key, 1.0);
        let keys = crate::value::js_dynamic_object_keys(obj as i64);
        assert_eq!(
            crate::array::js_array_length(keys),
            1,
            "a class_id == 2 object must enumerate its OWN keys, not \
             [message, name, stack]"
        );
    }
}

/// The #6595 half: the store-plan gate must stay FALSE for a heap class object.
/// `object_is_regular` is the replacement for the deleted
/// `object_type == OBJECT_TYPE_REGULAR` read at `proxy.rs:1523`, and it is only
/// a valid one because it means `descriptor.object_kind == Ordinary` — not the
/// weaker "is an ObjectHeader".
#[test]
fn object_is_regular_excludes_a_heap_class_object() {
    let obj = js_object_alloc(0x8113_0001, 1);
    unsafe {
        assert!(
            crate::object::object_is_regular(obj),
            "a fresh ordinary object is regular"
        );
        crate::object::class_registry::js_object_mark_class(obj as i64);
        assert!(
            !crate::object::object_is_regular(obj),
            "#6595: a heap class object must NOT be 'regular' — the store-plan \
             gate at proxy.rs keys off exactly this"
        );
    }
}
