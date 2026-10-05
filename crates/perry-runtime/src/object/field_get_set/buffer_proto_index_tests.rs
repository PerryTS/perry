use crate::{buffer, object, value};
fn key(s: &str) -> *mut crate::StringHeader {
    crate::string::canonical_key(s.as_bytes())
}
#[test]
fn buffer_proto_indices_use_holder_bytes_and_stop_at_invalid_indices() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    let buf = buffer::js_buffer_alloc(2, 0);
    buffer::js_buffer_set(buf, 0, 21);
    buffer::js_buffer_set(buf, 1, 22);
    let middle = object::js_object_alloc(0, 0);
    let receiver = object::js_object_alloc(0, 0);
    object::prototype_chain::object_set_static_prototype(
        middle as usize,
        value::js_nanbox_pointer(buf as i64).to_bits(),
    );
    object::prototype_chain::object_set_static_prototype(
        receiver as usize,
        value::js_nanbox_pointer(middle as i64).to_bits(),
    );
    for (name, expected) in [("0", 21.0f64), ("1", 22.0)] {
        assert_eq!(
            object::js_object_get_field_by_name(receiver, key(name)).bits(),
            expected.to_bits(),
            "{name}"
        );
    }
    for name in ["2", "-0", "-1", "1.5", "NaN", "Infinity"] {
        assert!(
            object::js_object_get_field_by_name(receiver, key(name)).is_undefined(),
            "{name}"
        );
    }
    buffer::buffer_set_own_prop(buf as usize, "01", 61.0);
    assert_eq!(
        object::js_object_get_field_by_name(receiver, key("01")).bits(),
        61.0f64.to_bits()
    );
    object::js_object_set_field_by_name(receiver, key("0"), 53.0);
    assert_eq!(
        object::js_object_get_field_by_name(receiver, key("0")).bits(),
        53.0f64.to_bits()
    );
    let ab = buffer::js_array_buffer_new(2);
    let nonindexed = object::js_object_alloc(0, 0);
    object::prototype_chain::object_set_static_prototype(
        nonindexed as usize,
        value::js_nanbox_pointer(ab as i64).to_bits(),
    );
    assert!(object::js_object_get_field_by_name(nonindexed, key("0")).is_undefined());
}

#[test]
fn buffer_index_classification_does_not_allocate_while_borrowing_a_key() {
    let _no_gc = crate::gc::GcSuppressScope::new();
    let buf = buffer::js_buffer_alloc(2, 0);
    let before = crate::arena::arena_live_allocated_bytes();
    unsafe {
        assert!(
            crate::typedarray_props::byte_buffer_index_get_by_name(buf as usize, "000123.45")
                .is_none()
        );
    }
    assert_eq!(crate::arena::arena_live_allocated_bytes(), before);
}
