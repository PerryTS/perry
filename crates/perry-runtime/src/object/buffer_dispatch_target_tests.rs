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

#[test]
fn a_view_class_object_without_view_storage_is_no_target() {
    // A plain object whose class extends a view: `ArrayBuffer.isView` still
    // classifies it, but it has no bytes, so copy/compare/equals throw
    // instead of resolving nothing (a silent 0 from `copy`).
    const TYPED: u32 = 61_301;
    const VIEW: u32 = 61_302;
    crate::object::data_view_registry::js_register_class_extends_typed_array(TYPED);
    crate::object::data_view_registry::js_register_class_extends_data_view(VIEW);
    for class_id in [TYPED, VIEW] {
        let object = boxed(crate::object::js_object_alloc(class_id, 0));
        let brand = crate::object::view_brand::view_brand(object).unwrap();
        assert!(!brand.has_storage());
        assert_eq!(brand.is_data_view(), class_id == VIEW);
        for any_view in [false, true] {
            assert!(!is_buffer_target(object, any_view));
        }
    }
}

#[test]
fn subclass_instances_built_by_the_base_constructor_are_targets() {
    // `class X extends Uint8Array` / `extends Float64Array` instances are
    // typed-array cells (the base constructor builds them), so the storage
    // classification and the byte resolution see the same cell.
    let u8 = typed(KIND_UINT8);
    let f64v = typed(KIND_FLOAT64);
    assert!(is_buffer_target(u8, false));
    assert!(crate::buffer::bytes::span(u8, true).is_ok());
    assert!(!is_buffer_target(f64v, false));
    assert!(is_buffer_target(f64v, true));
    assert!(crate::buffer::bytes::span(f64v, true).is_ok());
}
