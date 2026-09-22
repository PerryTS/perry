use super::*;

#[test]
fn predicted_canonical_shape_is_shared_by_literal_and_growth_paths() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let packed = b"licm_a\0licm_b\0";
        let expected = js_canonical_read_shape(packed.as_ptr(), packed.len() as u32);
        let keys = crate::object::js_build_class_keys_array(
            987654,
            2,
            packed.as_ptr(),
            packed.len() as u32,
        );
        assert_eq!(
            crate::object::shapes::js_object_shape_id_for_keys(keys as u64, 2),
            expected
        );
        let scope = crate::gc::RuntimeHandleScope::new();
        let object = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
        for (name, value) in [(b"licm_a", 1.0), (b"licm_b", 2.0)] {
            let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            object.with_mut_ptr(|o| crate::object::js_object_set_field_by_name(o, key, value));
        }
        let stamp = object.with_const_ptr(|o| crate::object::shapes::object_shape_stamp(o));
        assert_eq!(
            stamp, expected,
            "both actual producers must hit the same prediction"
        );
        assert_eq!(
            crate::object::shapes::js_shape_ordinary_inline_slot_for_key(
                expected,
                crate::value::js_nanbox_string(crate::string::js_string_from_bytes(
                    b"licm_b".as_ptr(),
                    6
                ) as i64)
                .to_bits()
            ),
            1
        );
        assert!(object.with_mut_ptr(|o| crate::object::dictionary::latch_object_to_dictionary(o)));
        let dictionary_stamp =
            object.with_const_ptr(|o| crate::object::shapes::object_shape_stamp(o));
        assert_ne!(
            dictionary_stamp, expected,
            "dictionary must never enter the canonical loop"
        );
    }
}
