use super::super::*;

fn append_preserves_read_probe(warm: bool) {
    let _lock = crate::gc::global_side_table_test_lock();
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _ = crate::object::js_get_global_this();
    unsafe {
        let obj = crate::object::js_object_alloc(0, 0);
        crate::object::mark_object_plain_ordinary(obj);
        for i in 0..48 {
            let name = format!("storeProbe{warm}Field{i}");
            let key = crate::string::canonical_key(name.as_bytes());
            crate::object::js_object_set_field_by_name(obj, key, i as f64);
        }
        let predecessor = record_of(test_dir(), LayoutWord((*obj).parent_class_id));
        assert!(!predecessor.is_null());
        assert_eq!((*predecessor).logical_key_count, 48);
        assert_eq!((*predecessor).probe, 0, "the final layout is cold");
        let expected_probe = if warm { probe_word(predecessor) } else { 0 };
        if warm {
            assert_ne!(expected_probe, 0, "exercise an existing read index");
        }
        let key = crate::string::canonical_key(b"storeProbeAppendWitness");
        let owner = crate::value::js_nanbox_pointer(obj as i64);
        let boxed_key = crate::value::js_nanbox_string(key as i64);
        crate::proxy::js_put_value_set(owner, boxed_key, 91.0, owner, 1);
        assert_eq!(
            (*predecessor).probe,
            expected_probe,
            "adding a key must not derive the predecessor's read index"
        );
        assert_eq!(
            crate::object::js_object_get_field_by_name_f64(obj, key),
            91.0
        );
        assert!(crate::object::obj_value_has_own_key(owner, boxed_key));
        let successor = record_of(test_dir(), LayoutWord((*obj).parent_class_id));
        assert_eq!((*successor).logical_key_count, 49);
        assert_eq!(test_list_word(obj, 48), boxed_key.to_bits());
        for i in 0..48 {
            let name = format!("storeProbe{warm}Field{i}");
            let key = crate::string::canonical_key(name.as_bytes());
            assert_eq!(
                crate::object::js_object_get_field_by_name_f64(obj, key),
                i as f64
            );
            assert_eq!(
                test_list_word(obj, i),
                crate::value::js_nanbox_string(key as i64).to_bits()
            );
        }
    }
}

#[test]
fn cold_wide_append_does_not_derive_a_read_probe() {
    append_preserves_read_probe(false);
}

#[test]
fn warm_wide_append_preserves_the_existing_read_probe() {
    append_preserves_read_probe(true);
}
