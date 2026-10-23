//! `Array.prototype.fill` over an array that holds element descriptors is the
//! spec's `Set(O, k, value, true)` per index: a read-only element is not
//! overwritten and the strict Set throws.

use super::*;

fn define_read_only(arr: *mut ArrayHeader, index: u32, value: f64) {
    let descriptor = crate::object::js_object_alloc_null_proto(0, 0);
    crate::object::js_object_set_field_by_name(
        descriptor,
        crate::string::intern_ascii_literal(b"value"),
        value,
    );
    crate::object::js_object_set_field_by_name(
        descriptor,
        crate::string::intern_ascii_literal(b"writable"),
        f64::from_bits(crate::value::TAG_FALSE),
    );
    let key = index.to_string();
    let key = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
    crate::object::js_object_define_property(
        crate::value::js_nanbox_pointer(arr as i64),
        crate::value::js_nanbox_string(key as i64),
        crate::value::js_nanbox_pointer(descriptor as i64),
    );
}

fn three() -> *mut ArrayHeader {
    let mut arr = js_array_alloc(3);
    for v in [1.0, 2.0, 3.0] {
        arr = js_array_push_f64(arr, v);
    }
    arr
}

#[test]
fn fill_over_a_read_only_element_throws_instead_of_writing() {
    let arr = three();
    define_read_only(arr, 1, 20.0);
    let thrown = crate::exception::catch_js_throw(|| js_array_fill(arr, 7.0));
    assert!(thrown.is_err(), "Set on a read-only element throws");
    let live = clean_arr_ptr_mut(arr);
    assert_eq!(
        js_array_get_f64(live, 0),
        7.0,
        "the index before it was set"
    );
    assert_eq!(
        js_array_get_f64(live, 1),
        20.0,
        "the read-only element keeps its value"
    );
    assert_eq!(js_array_get_f64(live, 2), 3.0, "the throw stops the fill");
}

#[test]
fn fill_range_over_a_read_only_element_throws_only_when_it_reaches_it() {
    let arr = three();
    define_read_only(arr, 2, 30.0);
    let arr = js_array_fill_range(arr, 9.0, 0.0, 2.0);
    let live = clean_arr_ptr_mut(arr);
    assert_eq!(js_array_get_f64(live, 0), 9.0);
    assert_eq!(js_array_get_f64(live, 1), 9.0);
    assert_eq!(js_array_get_f64(live, 2), 30.0, "outside the range");
    let thrown = crate::exception::catch_js_throw(|| js_array_fill_range(arr, 5.0, 1.0, 3.0));
    assert!(thrown.is_err(), "a range reaching the read-only key throws");
    let live = clean_arr_ptr_mut(arr);
    assert_eq!(js_array_get_f64(live, 1), 5.0, "set before the throw");
    assert_eq!(js_array_get_f64(live, 2), 30.0, "read-only keeps its value");
}
