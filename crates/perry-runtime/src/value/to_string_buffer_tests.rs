//! String conversion regressions for BufferHeader-backed values.

use super::*;

fn text(ptr: *const crate::string::StringHeader) -> String {
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

#[test]
fn buffer_dispatch_to_string_uses_object_tags_for_non_indexed_views() {
    use crate::buffer::BufferHeader;
    use crate::gc::RuntimeHandleScope;

    let scope = RuntimeHandleScope::new();
    let ab = scope.root_nanbox_f64(boxed(crate::buffer::js_array_buffer_new(2)));
    crate::buffer::bytes::no_gc(|scope| {
        crate::buffer::bytes::bytes_mut(
            crate::value::js_nanbox_pointer(ab.get_nanbox_f64().to_bits() as i64),
            scope,
        )
        .unwrap()
        .copy_from_slice(b"hi");
    });

    let sab = scope.root_nanbox_f64(boxed(crate::buffer::js_shared_array_buffer_new(2)));
    crate::buffer::bytes::no_gc(|scope| {
        crate::buffer::bytes::bytes_mut(
            crate::value::js_nanbox_pointer(sab.get_nanbox_f64().to_bits() as i64),
            scope,
        )
        .unwrap()
        .copy_from_slice(b"hi");
    });

    let value = crate::buffer::bytes::from_slice(crate::buffer::bytes::Brand::DataView, b"hi");
    let dv_ptr = JSValue::from_bits(value.to_bits())
        .as_pointer::<BufferHeader>()
        .cast_mut();
    let dv = scope.root_nanbox_f64(boxed(dv_ptr));

    let short_hex = scope.root_nanbox_f64(f64::from_bits(
        JSValue::short_string_unchecked(b"hex").bits(),
    ));
    let heap_hex = scope.root_nanbox_f64(f64::from_bits(
        JSValue::string_ptr(crate::string::js_string_from_bytes(b"hex".as_ptr(), 3)).bits(),
    ));

    // The short encoding and the literal both must be ignored for the three
    // non-indexed byte-family receivers.
    for (value, expected) in [
        (&ab, "[object ArrayBuffer]"),
        (&sab, "[object SharedArrayBuffer]"),
        (&dv, "[object DataView]"),
    ] {
        for encoding in [&short_hex, &heap_hex] {
            let current = value.get_nanbox_f64();
            let addr = JSValue::from_bits(current.to_bits()).as_pointer::<u8>() as usize;
            let argument = encoding.get_nanbox_f64();
            let result =
                unsafe { crate::object::dispatch_buffer_method(addr, "toString", &argument, 1) };
            let result = scope.root_nanbox_f64(result);
            assert_eq!(
                text(
                    crate::value::js_get_string_pointer_unified(result.get_nanbox_f64())
                        as *const _
                ),
                expected
            );
        }
    }

    // Control: Buffer.toString continues to consume the same short encoding.
    let buffer = scope.root_nanbox_f64(boxed(crate::buffer::js_buffer_alloc(2, 0)));
    crate::buffer::bytes::no_gc(|scope| {
        crate::buffer::bytes::bytes_mut(
            crate::value::js_nanbox_pointer(buffer.get_nanbox_f64().to_bits() as i64),
            scope,
        )
        .unwrap()
        .copy_from_slice(b"hi");
    });
    let current = buffer.get_nanbox_f64();
    let addr = JSValue::from_bits(current.to_bits()).as_pointer::<u8>() as usize;
    let argument = short_hex.get_nanbox_f64();
    let result = unsafe { crate::object::dispatch_buffer_method(addr, "toString", &argument, 1) };
    let result = scope.root_nanbox_f64(result);
    assert_eq!(
        text(crate::value::js_get_string_pointer_unified(result.get_nanbox_f64()) as *const _),
        "6869"
    );
}
