use crate::object::*;
use crate::value::{JSValue, TAG_NULL};

fn key() -> *mut crate::StringHeader {
    crate::string::js_string_from_bytes(b"constructor".as_ptr(), 11)
}

fn view() -> *mut ObjectHeader {
    let backing = crate::buffer::js_array_buffer_new(4);
    let value =
        crate::buffer::js_data_view_new(crate::value::js_nanbox_pointer(backing as i64), 0.0, 4.0);
    JSValue::from_bits(value.to_bits()).as_pointer::<ObjectHeader>() as *mut ObjectHeader
}

#[test]
fn data_view_constructor_reads_the_recorded_prototype() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _suppress = crate::gc::GcSuppressScope::new();
    let view = view();
    let key = key();
    let proto = js_object_alloc(0, 0);
    js_object_set_field_by_name(proto, key, 42.0);
    prototype_chain::object_set_static_prototype(
        view as usize,
        JSValue::pointer(proto as *mut u8).bits(),
    );
    assert_eq!(
        js_object_get_field_by_name(view, key).bits(),
        42.0f64.to_bits()
    );
}

#[test]
fn data_view_constructor_respects_a_null_or_missing_prototype_property() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _suppress = crate::gc::GcSuppressScope::new();
    let view = view();
    let key = key();
    prototype_chain::object_set_static_prototype(view as usize, TAG_NULL);
    assert!(js_object_get_field_by_name(view, key).is_undefined());
    let proto = js_object_alloc(0, 0);
    prototype_chain::object_set_static_prototype(proto as usize, TAG_NULL);
    prototype_chain::object_set_static_prototype(
        view as usize,
        JSValue::pointer(proto as *mut u8).bits(),
    );
    assert!(js_object_get_field_by_name(view, key).is_undefined());
}

#[test]
fn data_view_constructor_own_property_shadows_the_prototype() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _suppress = crate::gc::GcSuppressScope::new();
    let view = view();
    let key = key();
    let proto = js_object_alloc(0, 0);
    js_object_set_field_by_name(proto, key, 42.0);
    prototype_chain::object_set_static_prototype(
        view as usize,
        JSValue::pointer(proto as *mut u8).bits(),
    );
    crate::buffer::buffer_set_own_prop(view as usize, "constructor", 7.0);
    assert_eq!(
        js_object_get_field_by_name(view, key).bits(),
        7.0f64.to_bits()
    );
}

#[test]
fn data_view_constructor_uses_the_intrinsic_without_a_recorded_prototype() {
    let _lock = crate::gc::global_side_table_test_lock();
    let _suppress = crate::gc::GcSuppressScope::new();
    let view = view();
    assert!(prototype_chain::object_static_prototype(view as usize).is_none());
    let expected = js_get_global_this_builtin_value(b"DataView".as_ptr(), 8);
    assert_eq!(
        js_object_get_field_by_name(view, key()).bits(),
        expected.to_bits()
    );
}
