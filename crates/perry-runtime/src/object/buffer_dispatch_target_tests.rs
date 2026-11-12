//! The `Buffer.prototype.copy/compare/equals` target predicate. `compare` and
//! `equals` take node's `isUint8Array` set; `copy` takes every
//! `ArrayBufferView`. Neither takes an `ArrayBuffer`. The throwing path goes
//! through `js_throw` and is covered end to end by the equals fixtures.
use super::is_buffer_target;
use crate::typedarray::{
    js_typed_array_new, KIND_FLOAT64, KIND_INT8, KIND_UINT16, KIND_UINT8, KIND_UINT8_CLAMPED,
};
use crate::value::JSValue;

fn boxed<T>(ptr: *mut T) -> f64 {
    f64::from_bits(JSValue::pointer(ptr as *mut u8).bits())
}

fn typed(kind: u8) -> f64 {
    boxed(js_typed_array_new(kind as i32, 16.0))
}

#[test]
fn a_buffer_and_every_uint8array_form_are_targets_of_all_three() {
    for any_view in [false, true] {
        assert!(is_buffer_target(
            boxed(crate::buffer::buffer_alloc(16)),
            any_view
        ));
        assert!(is_buffer_target(
            boxed(crate::buffer::js_uint8array_alloc(16)),
            any_view
        ));
        assert!(is_buffer_target(typed(KIND_UINT8), any_view));
    }
}

#[test]
fn other_typed_arrays_are_copy_targets_only() {
    for kind in [KIND_UINT8_CLAMPED, KIND_INT8, KIND_UINT16, KIND_FLOAT64] {
        assert!(
            !is_buffer_target(typed(kind), false),
            "typed array kind {kind} is not a Uint8Array"
        );
        assert!(
            is_buffer_target(typed(kind), true),
            "typed array kind {kind} is an ArrayBufferView"
        );
    }
}

#[test]
fn array_buffers_and_primitives_are_no_targets() {
    for any_view in [false, true] {
        assert!(!is_buffer_target(
            boxed(crate::buffer::js_array_buffer_new(16)),
            any_view
        ));
        assert!(!is_buffer_target(16.0, any_view));
        assert!(!is_buffer_target(
            f64::from_bits(crate::value::TAG_UNDEFINED),
            any_view
        ));
    }
}
