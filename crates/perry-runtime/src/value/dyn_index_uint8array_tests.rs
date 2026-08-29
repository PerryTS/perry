//! Dynamic-index regression coverage for Perry's Buffer-backed Uint8Array.

use super::{js_dyn_index_get, js_dyn_index_set_strict};

fn boxed_i32(value: i32) -> f64 {
    f64::from_bits(crate::value::INT32_TAG | u64::from(value as u32))
}

#[test]
fn dynamic_uint8array_set_coerces_nanboxed_values_before_narrowing() {
    let array = crate::buffer::js_uint8array_alloc(2);
    let receiver = crate::value::js_nanbox_pointer(array as i64);
    let index_zero = boxed_i32(0);
    let index_one = boxed_i32(1);

    let wrapped_one = boxed_i32(257);
    assert_eq!(
        js_dyn_index_set_strict(receiver, index_zero, wrapped_one, 1).to_bits(),
        wrapped_one.to_bits(),
        "an assignment expression still yields its original boxed value"
    );
    assert_eq!(js_dyn_index_get(receiver, index_zero), 1.0);

    js_dyn_index_set_strict(receiver, index_one, boxed_i32(-1), 1);
    assert_eq!(js_dyn_index_get(receiver, index_one), 255.0);
}
