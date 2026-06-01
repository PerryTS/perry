//! Tests for the object module (extracted from mod.rs to keep it under the 2000-line cap).
#![cfg(test)]

use super::*;

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
            let ctor_key =
                crate::string::js_string_from_bytes(ctor_name.as_ptr(), ctor_name.len() as u32);
            let ctor = js_object_get_field_by_name(global_ptr, ctor_key);
            assert!(
                ctor.is_pointer(),
                "{ctor_name} should be a closure-backed global"
            );

            let ctor_ptr = ctor.as_pointer::<crate::closure::ClosureHeader>();
            assert_eq!((*ctor_ptr).type_tag, crate::closure::CLOSURE_MAGIC);

            let instance =
                js_new_function_construct(f64::from_bits(ctor.bits()), std::ptr::null(), 0);
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
        }
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
