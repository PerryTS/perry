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
fn closure_name_and_length_ignore_plain_assignment() {
    crate::closure::test_clear_closure_side_tables();
    unsafe {
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
    crate::closure::test_clear_closure_side_tables();
    unsafe {
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
    unsafe {
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
