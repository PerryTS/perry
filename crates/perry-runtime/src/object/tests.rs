//! Tests for the object module (extracted from mod.rs to keep it under the 2000-line cap).
#![cfg(test)]

use super::*;

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
