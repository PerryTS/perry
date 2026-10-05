//! The rollback edge (`shapes_last_key_rollback`) through `delete`: deleting
//! the key a plain object added last returns it to the shape of its list
//! without that key, and adding the key back reaches the shape it left.

use super::super::{
    js_object_alloc_null_proto, js_object_delete_field, js_object_get_field_by_name,
    js_object_set_field_by_name, shapes, ObjectHeader,
};

unsafe fn key(name: &str) -> *mut crate::StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

unsafe fn key_count(obj: *const ObjectHeader) -> u32 {
    shapes::shape_record_by_id(shapes::object_shape_stamp(obj))
        .expect("a shaped receiver")
        .logical_key_count()
}

#[test]
fn deleting_the_last_added_key_rolls_back_and_a_re_add_returns() {
    let _global = crate::gc::global_side_table_test_lock();
    let _no_move = crate::gc::GcSuppressScope::new();
    unsafe {
        let obj = js_object_alloc_null_proto(0, 0);
        js_object_set_field_by_name(obj, key("rollback_a"), 1.0);
        let exact_parent = shapes::object_shape_stamp(obj);
        js_object_set_field_by_name(obj, key("rollback_b"), 2.0);
        let with_both = shapes::object_shape_stamp(obj);
        assert_eq!(key_count(obj), 2);

        assert_eq!(js_object_delete_field(obj, key("rollback_b")), 1);
        let rolled_back = shapes::object_shape_stamp(obj);
        assert_eq!(rolled_back, exact_parent, "the exact key-add parent");
        assert_ne!(rolled_back, with_both, "a delete must move the shape word");
        assert_eq!(key_count(obj), 1, "the list without its last key");
        let header = crate::value::addr_class::try_read_gc_header(obj as usize).unwrap();
        assert_eq!(
            header._reserved & crate::gc::OBJ_FLAG_STABLE_TOMBSTONES,
            0,
            "a rollback forks no private tombstone list"
        );
        assert_eq!(
            js_object_get_field_by_name(obj, key("rollback_b")).bits(),
            crate::value::TAG_UNDEFINED
        );
        assert_eq!(
            js_object_get_field_by_name(obj, key("rollback_a")).as_number(),
            1.0
        );

        js_object_set_field_by_name(obj, key("rollback_b"), 3.0);
        assert_eq!(
            shapes::object_shape_stamp(obj),
            with_both,
            "the key-add edge leads back to the shape the delete left"
        );
        assert_eq!(
            js_object_get_field_by_name(obj, key("rollback_b")).as_number(),
            3.0
        );

        // The next cycle takes the learned edge to the same shape.
        assert_eq!(js_object_delete_field(obj, key("rollback_b")), 1);
        assert_eq!(shapes::object_shape_stamp(obj), rolled_back);
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
