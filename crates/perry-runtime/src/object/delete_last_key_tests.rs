//! Deleting the key a plain object added last. Object model step 4: every
//! delete goes through the object's overflow, the last-added key included.

use super::super::{
    js_object_alloc_null_proto, js_object_delete_field, js_object_get_field_by_name,
    js_object_set_field_by_name, shapes, ObjectHeader,
};

unsafe fn key(name: &str) -> *mut crate::StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

unsafe fn key_count(obj: *const ObjectHeader) -> u32 {
    shapes::shape_record_for_word(shapes::object_shape_stamp(obj))
        .expect("a shaped receiver")
        .logical_key_count()
}

/// Object model step 4: `delete` no longer takes the rollback edge; deleting
/// the last added key goes through the overflow like any delete (§3.2). The
/// layout keeps its keys, the key reads absent, and a re-add appends to the
/// overflow and meets the key's fixed slot, cycle after cycle.
#[test]
fn deleting_the_last_added_key_goes_through_the_overflow_and_a_re_add_returns() {
    let _global = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        let obj = js_object_alloc_null_proto(0, 0);
        js_object_set_field_by_name(obj, key("rollback_a"), 1.0);
        js_object_set_field_by_name(obj, key("rollback_b"), 2.0);
        assert_eq!(key_count(obj), 2);

        for value in [3.0, 4.0] {
            assert_eq!(js_object_delete_field(obj, key("rollback_b")), 1);
            assert!(crate::object::overflow::has_overflow(obj));
            assert_eq!(key_count(obj), 2, "a delete keeps the layout's keys");
            assert_eq!(
                js_object_get_field_by_name(obj, key("rollback_b")).bits(),
                crate::value::TAG_UNDEFINED
            );
            assert_eq!(
                js_object_get_field_by_name(obj, key("rollback_a")).as_number(),
                1.0
            );
            js_object_set_field_by_name(obj, key("rollback_b"), value);
            assert_eq!(
                js_object_get_field_by_name(obj, key("rollback_b")).as_number(),
                value
            );
            assert_eq!(
                key_count(obj),
                2,
                "a re-add of a layout key meets its fixed slot"
            );
        }
    }
}

#[test]
fn deleting_an_earlier_key_is_not_a_rollback() {
    let _global = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        let obj = js_object_alloc_null_proto(0, 0);
        js_object_set_field_by_name(obj, key("earlier_a"), 1.0);
        js_object_set_field_by_name(obj, key("earlier_b"), 2.0);
        let before = shapes::object_shape_stamp(obj);
        assert_eq!(js_object_delete_field(obj, key("earlier_a")), 1);
        assert_ne!(shapes::object_shape_stamp(obj), before);
        assert_eq!(
            js_object_get_field_by_name(obj, key("earlier_a")).bits(),
            crate::value::TAG_UNDEFINED
        );
        assert_eq!(
            js_object_get_field_by_name(obj, key("earlier_b")).as_number(),
            2.0
        );
    }
}
