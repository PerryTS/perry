//! #6759 C3c: `ObjectHeader.parent_class_id` carries TWO different things —
//! a real parent class id for a class instance, and the runtime ShapeId
//! stamp for a plain object (`class_id == 0`). `serialize_object` used to
//! copy the raw word, and `deserialize` feeds it to
//! `js_object_alloc_with_parent`, which registers it as a class-parent
//! edge. These tests pin that the serializer reads the class-parent
//! REGISTRY instead, which is (a) correct for both kinds and (b) the last
//! thing that had to stop reading the header word before C3 can re-purpose
//! it as a uniform shape word.
use super::*;
use crate::object::shapes::is_shape_id;

fn key(name: &str) -> *mut crate::StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

fn serialized_parent(obj: *mut crate::object::ObjectHeader) -> u32 {
    let bits = crate::value::js_nanbox_pointer(obj as i64).to_bits();
    match unsafe { serialize_nanbox_for_thread(bits) } {
        SerializedValue::Object {
            parent_class_id, ..
        } => parent_class_id,
        other => panic!("expected SerializedValue::Object, got {other:?}"),
    }
}

/// The discriminating quantity. A plain object that has been READ once
/// carries a ShapeId in `parent_class_id`; before this fix that id was
/// serialized and replayed as a class-parent edge, so the receiving thread
/// ran `register_class(0, <shape id>)`. The test asserts BOTH halves: the
/// stamp is really there (so the fixture is not vacuous) and it does not
/// reach the wire.
#[test]
fn a_plain_objects_shape_stamp_is_not_serialized_as_a_parent_class_id() {
    let _lock = crate::gc::global_side_table_test_lock();
    unsafe {
        let obj = crate::object::js_object_alloc(0, 8);
        for name in ["thr6759_a", "thr6759_b", "thr6759_c"] {
            crate::object::js_object_set_field_by_name(obj, key(name), 1.0);
        }
        let _ = crate::object::js_object_get_field_by_name(obj, key("thr6759_b"));

        let stamp = (*obj).parent_class_id;
        assert!(
            is_shape_id(stamp),
            "fixture is vacuous — the object carries no shape stamp to leak (got {stamp:#x})"
        );
        assert_eq!(
            serialized_parent(obj),
            0,
            "a ShapeId ({stamp:#x}) reached the worker as a class-parent edge; \
             deserialization would call register_class(0, {stamp:#x})"
        );
    }
}

/// The other half: the fix must not be "always send 0". A class instance's
/// parent edge still round-trips — and it comes from the REGISTRY, so it
/// survives the header word being overwritten (which is exactly what C3's
/// uniform shape stamp will do).
#[test]
fn a_class_instances_parent_comes_from_the_registry_not_the_header() {
    let _lock = crate::gc::global_side_table_test_lock();
    const CHILD: u32 = 0x0C3C_6759;
    const PARENT: u32 = 0x0C3C_675A;
    unsafe {
        let obj = crate::object::js_object_alloc_with_parent(CHILD, PARENT, 2);
        assert_eq!(serialized_parent(obj), PARENT, "parent edge lost");

        // Simulate the C3 end state: the header word now holds a shape
        // stamp. The registry is unchanged, so the wire value must be too.
        (*obj).parent_class_id = crate::object::shapes::SHAPE_ID_BASE + 7;
        assert_eq!(
            serialized_parent(obj),
            PARENT,
            "the serializer is still reading the header word, not the registry"
        );
    }
}
