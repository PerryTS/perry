//! #11239: every view predicate answers from [`view_brand`], so they agree.

use super::*;
use crate::buffer::{self, BufferHeader};
use crate::typedarray::{KIND_INT8, KIND_UINT8};
use crate::value::TAG_TRUE;

fn boxed(ptr: *mut BufferHeader) -> f64 {
    f64::from_bits(JSValue::pointer(ptr.cast()).bits())
}

fn is_true(v: f64) -> bool {
    v.to_bits() == TAG_TRUE
}

/// One row of the predicate matrix, read through every public entry point.
#[derive(Debug, PartialEq, Eq)]
struct Row {
    is_view_direct: bool,
    is_view_value: bool,
    typed: bool,
    uint8: bool,
    int8: bool,
    data_view: bool,
    instanceof_uint8: bool,
    instanceof_buffer: bool,
    is_buffer: bool,
}

fn row(value: f64) -> Row {
    use crate::object::global_this::array_buffer_is_view_thunk;
    use crate::object::js_instanceof;
    Row {
        is_view_direct: is_true(crate::object::js_util_types_is_array_buffer_view(value)),
        is_view_value: is_true(array_buffer_is_view_thunk(std::ptr::null(), value)),
        typed: is_true(crate::object::js_util_types_is_typed_array(value)),
        uint8: is_true(crate::object::js_util_types_is_uint8_array(value)),
        int8: is_true(crate::object::js_util_types_is_int8_array(value)),
        data_view: is_true(crate::object::js_util_types_is_data_view(value)),
        instanceof_uint8: is_true(js_instanceof(value, buffer::BUFFER_TYPE_ID)),
        instanceof_buffer: is_true(js_instanceof(value, buffer::NODE_BUFFER_CLASS_ID)),
        is_buffer: buffer::js_buffer_is_node_buffer(value.to_bits() as i64) == 1,
    }
}

const NOT_A_VIEW: Row = Row {
    is_view_direct: false,
    is_view_value: false,
    typed: false,
    uint8: false,
    int8: false,
    data_view: false,
    instanceof_uint8: false,
    instanceof_buffer: false,
    is_buffer: false,
};

#[test]
fn node_buffer_is_a_uint8_array_view() {
    let value = boxed(buffer::buffer_alloc(4));
    assert_eq!(view_brand(value), Some(ViewBrand::NodeBuffer));
    assert_eq!(
        row(value),
        Row {
            is_view_direct: true,
            is_view_value: true,
            typed: true,
            uint8: true,
            instanceof_uint8: true,
            instanceof_buffer: true,
            is_buffer: true,
            ..NOT_A_VIEW
        }
    );
}

#[test]
fn plain_uint8_array_is_not_a_buffer() {
    let value = boxed(buffer::js_uint8array_alloc(4));
    assert_eq!(view_brand(value), Some(ViewBrand::TypedArray(KIND_UINT8)));
    assert_eq!(
        row(value),
        Row {
            is_view_direct: true,
            is_view_value: true,
            typed: true,
            uint8: true,
            instanceof_uint8: true,
            ..NOT_A_VIEW
        }
    );
}

#[test]
fn registry_typed_array_keeps_its_kind() {
    let ta = crate::typedarray::js_typed_array_new_empty(KIND_INT8 as i32, 4);
    let value = f64::from_bits(JSValue::pointer(ta.cast()).bits());
    assert_eq!(view_brand(value), Some(ViewBrand::TypedArray(KIND_INT8)));
    assert_eq!(
        row(value),
        Row {
            is_view_direct: true,
            is_view_value: true,
            typed: true,
            int8: true,
            ..NOT_A_VIEW
        }
    );
}

#[test]
fn data_view_is_a_view_but_not_a_typed_array_or_buffer() {
    let value = boxed(buffer::buffer_alloc(4));
    buffer::mark_as_data_view(crate::value::addr_class::object_ref_addr(value));
    assert_eq!(view_brand(value), Some(ViewBrand::DataView));
    assert_eq!(
        row(value),
        Row {
            is_view_direct: true,
            is_view_value: true,
            data_view: true,
            ..NOT_A_VIEW
        }
    );
}

#[test]
fn backing_stores_and_key_material_are_not_views() {
    let marks: [(&str, fn(usize)); 5] = [
        ("ArrayBuffer", buffer::mark_as_array_buffer),
        ("SharedArrayBuffer", buffer::mark_as_shared_array_buffer),
        ("secret key", buffer::mark_as_secret_key),
        ("CryptoKey", |addr| {
            buffer::mark_as_crypto_key(addr, 0, 0, 0)
        }),
        ("asymmetric key", |addr| {
            buffer::mark_as_asymmetric_key(addr, 0, 0)
        }),
    ];
    for (label, mark) in marks {
        let buf = buffer::buffer_alloc(4);
        mark(buf as usize);
        let value = boxed(buf);
        assert_eq!(view_brand(value), None, "{label}");
        assert_eq!(row(value), NOT_A_VIEW, "{label}");
    }
}

#[test]
fn primitives_are_not_views() {
    for bits in [
        crate::value::TAG_UNDEFINED,
        crate::value::TAG_NULL,
        crate::value::TAG_TRUE,
        16.0f64.to_bits(),
    ] {
        let value = f64::from_bits(bits);
        assert_eq!(view_brand(value), None);
        assert_eq!(row(value), NOT_A_VIEW);
    }
}

#[test]
fn uint8_view_shortcut_agrees_with_the_full_brand() {
    let fixtures: [(&str, fn(usize)); 7] = [
        ("Buffer", |_| {}),
        ("Uint8Array", buffer::mark_as_uint8array),
        ("DataView", buffer::mark_as_data_view),
        ("ArrayBuffer", buffer::mark_as_array_buffer),
        ("SharedArrayBuffer", buffer::mark_as_shared_array_buffer),
        ("secret key", buffer::mark_as_secret_key),
        ("asymmetric key", |addr| {
            buffer::mark_as_asymmetric_key(addr, 0, 0)
        }),
    ];
    for (label, mark) in fixtures {
        let addr = buffer::buffer_alloc(4) as usize;
        mark(addr);
        assert_eq!(
            buffer::is_uint8_view_buffer(addr),
            buffer::buffer_brand(addr).is_some_and(buffer::BufferBrand::is_uint8_array),
            "{label}"
        );
    }
    // A live heap object that was never registered as a buffer.
    let plain = crate::object::js_object_alloc(0, 0) as usize;
    assert!(!buffer::is_uint8_view_buffer(plain));
    assert_eq!(buffer::buffer_brand(plain), None);
}
