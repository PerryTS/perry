use super::*;

#[test]
fn rest_preserves_undefined_and_reads_live_overflow_positions() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    let key = |name: &[u8]| crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let omit = key(b"omit");
    let undefined = key(b"u");
    let deleted = key(b"deleted");
    let tail = key(b"tail");
    let source = js_object_alloc(0, 4);
    js_object_set_field_by_name(source, omit, 1.0);
    js_object_set_field_by_name(
        source,
        undefined,
        f64::from_bits(crate::value::TAG_UNDEFINED),
    );
    js_object_set_field_by_name(source, deleted, 2.0);
    js_object_set_field_by_name(source, tail, 42.0);
    assert_eq!(js_object_delete_field(source, deleted), 1);
    let exclude = crate::array::js_array_alloc_with_length(1);
    crate::array::js_array_set(exclude, 0, crate::JSValue::string_ptr(omit));
    let rest = js_object_rest(source, exclude);
    let keys = crate::object::js_object_keys(rest);
    assert_eq!(crate::array::js_array_length(keys), 2);
    unsafe {
        assert!(crate::string::js_string_key_matches(
            crate::array::js_array_get(keys, 0),
            undefined
        ));
        assert!(crate::string::js_string_key_matches(
            crate::array::js_array_get(keys, 1),
            tail
        ));
    }
    assert!(js_object_get_field_by_name(rest, undefined).is_undefined());
    assert_eq!(
        js_object_get_field_by_name(rest, tail).bits(),
        42.0f64.to_bits()
    );
}
