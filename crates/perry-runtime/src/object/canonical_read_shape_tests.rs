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

#[test]
fn remembered_prediction_roots_its_keys_without_a_receiver() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let packed = b"remembered_read_key\0";
        let id = js_canonical_read_shape(packed.as_ptr(), packed.len() as u32);
        let descriptor = super::super::shapes::shape_descriptor_by_id(id).unwrap();
        assert!(
            descriptor.cache_carrier,
            "a remembered id needs an external carrier"
        );
        let mut roots = Vec::new();
        super::super::shapes::scan_shape_table_rekey_mut(
            &mut crate::gc::RuntimeRootVisitor::for_copy(&mut |v| roots.push(v.to_bits())),
        );
        assert!(
            roots
                .iter()
                .any(|bits| bits & crate::value::POINTER_MASK == descriptor.keys),
            "the shape scanner must strongly visit the keys with no receiver"
        );
        super::super::shapes::prune_uncarried_shape_descriptors_after_full_trace();
        assert!(
            super::super::shapes::shape_descriptor_by_id(id).is_some(),
            "external expectations must survive descriptor retirement"
        );
        prune_dead_canonical_keys(&|addr| {
            !roots
                .iter()
                .any(|bits| bits & crate::value::POINTER_MASK == addr as u64)
        });
        assert_eq!(
            js_canonical_read_shape(packed.as_ptr(), packed.len() as u32),
            id,
            "weak trie pruning must retain the rooted prediction"
        );
    }
}
