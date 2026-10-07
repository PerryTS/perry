//! String conversion regressions for BufferHeader-backed values.

use super::*;

fn text(ptr: *mut crate::string::StringHeader) -> String {
    unsafe { crate::object::has_own_helpers::str_from_string_header(ptr) }
        .expect("string conversion returns UTF-8")
        .to_string()
}

fn boxed(ptr: *mut crate::buffer::BufferHeader) -> f64 {
    f64::from_bits(JSValue::pointer(ptr as *const u8).bits())
}

fn with_printable_bytes(ptr: *mut crate::buffer::BufferHeader) -> f64 {
    unsafe {
        crate::buffer::bytes::no_gc(|scope| {
            crate::buffer::bytes::bytes_mut(crate::value::js_nanbox_pointer(ptr as i64), scope)
                .unwrap()
                .copy_from_slice(b"hi")
        });
    }
    boxed(ptr)
}

#[test]
fn array_buffer_backed_values_use_their_object_tags() {
    let ab = with_printable_bytes(crate::buffer::js_array_buffer_new(2));
    let sab = with_printable_bytes(crate::buffer::js_shared_array_buffer_new(2));
    let value = crate::buffer::bytes::from_slice(crate::buffer::bytes::Brand::DataView, b"hi");
    let dv = JSValue::from_bits(value.to_bits())
        .as_pointer::<crate::buffer::BufferHeader>()
        .cast_mut();
    let dv = with_printable_bytes(dv);

    for (value, expected) in [
        (ab, "[object ArrayBuffer]"),
        (sab, "[object SharedArrayBuffer]"),
        (dv, "[object DataView]"),
    ] {
        assert_eq!(text(js_jsvalue_to_string(value)), expected);
        assert_eq!(text(js_jsvalue_to_string_method(value)), expected);
        assert_eq!(
            text(crate::buffer::js_value_to_string_with_encoding(value, 1)),
            expected
        );
    }

    let buffer = with_printable_bytes(crate::buffer::js_buffer_alloc(2, 0));
    assert_eq!(text(js_jsvalue_to_string(buffer)), "hi");
    assert_eq!(
        text(crate::buffer::js_value_to_string_with_encoding(buffer, 1)),
        "6869"
    );
}
