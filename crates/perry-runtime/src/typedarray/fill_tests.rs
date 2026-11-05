//! %TypedArray%.prototype.fill: one conversion, spec clamping, own element
//! width, ValidateTypedArray before and after the conversions.
use super::*;
use crate::typedarray_view::js_typed_array_view;

fn num(x: f64) -> f64 {
    x
}

fn undefined() -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

fn fill(ta: *mut TypedArrayHeader, v: f64, range: &[f64]) -> *mut TypedArrayHeader {
    let (hs, s) = range.first().map_or((0, 0.0), |s| (1, *s));
    let (he, e) = range.get(1).map_or((0, 0.0), |e| (1, *e));
    js_typed_array_fill(ta, v, hs, s, he, e)
}

fn read(ta: *mut TypedArrayHeader, n: usize) -> Vec<f64> {
    (0..n).map(|i| js_typed_array_get(ta, i as i32)).collect()
}

#[test]
fn fill_clamps_range_and_converts_per_kind() {
    let a = typed_array_alloc(KIND_INT16, 5);
    assert_eq!(fill(a, 70000.0, &[num(-3.0), num(-1.0)]), a);
    assert_eq!(read(a, 5), [0.0, 0.0, 4464.0, 4464.0, 0.0]);
    fill(a, 1.0, &[num(1.9), num(f64::INFINITY)]);
    assert_eq!(read(a, 5), [0.0, 1.0, 1.0, 1.0, 1.0]);
    fill(a, 2.0, &[num(4.0), num(2.0)]);
    assert_eq!(read(a, 5), [0.0, 1.0, 1.0, 1.0, 1.0]);
    let c = typed_array_alloc(KIND_UINT8_CLAMPED, 3);
    fill(c, 2.5, &[]);
    assert_eq!(read(c, 3), [2.0; 3]);
    fill(c, 300.0, &[undefined(), undefined()]);
    assert_eq!(read(c, 3), [255.0; 3]);
    let f = typed_array_alloc(KIND_FLOAT32, 2);
    fill(f, -0.0, &[]);
    assert!(read(f, 2)
        .iter()
        .all(|x| x.to_bits() == (-0.0f64).to_bits()));
    let b = typed_array_alloc(KIND_FLOAT64, 4);
    fill(b, 1.5, &[num(1.0)]);
    assert_eq!(read(b, 4), [0.0, 1.5, 1.5, 1.5]);
}

#[test]
fn fill_number_into_bigint_kind_and_bigint_into_number_kind_throw() {
    let big = typed_array_alloc(KIND_BIGINT64, 2);
    assert!(crate::exception::catch_js_throw(|| fill(big, 1.0, &[])).is_err());
    let int = typed_array_alloc(KIND_INT32, 2);
    let one = crate::value::js_nanbox_bigint(crate::bigint_hooks::js_bigint_from_i64(1) as i64);
    assert!(crate::exception::catch_js_throw(|| fill(int, one, &[])).is_err());
    assert_eq!(read(int, 2), [0.0, 0.0]);
    assert!(crate::exception::catch_js_throw(|| fill(big, one, &[])).is_ok());
}

#[test]
fn fill_validates_the_receiver_and_reclamps_after_a_resize() {
    let buf = crate::buffer::alloc_resizable_array_buffer(8, 16);
    let boxed = f64::from_bits(crate::value::JSValue::pointer(buf as *const u8).bits());
    let view = js_typed_array_view(KIND_UINT8 as i32, boxed, 0.0, undefined());
    assert_eq!(read(view, 8), [0.0; 8]);
    // Shrunk below the length-tracking view: only the live prefix is written.
    crate::buffer::array_buffer_resize(buf as usize, &[4.0]);
    fill(view, 9.0, &[num(0.0), num(8.0)]);
    crate::buffer::array_buffer_resize(buf as usize, &[8.0]);
    assert_eq!(read(view, 8), [9.0, 9.0, 9.0, 9.0, 0.0, 0.0, 0.0, 0.0]);
    let fixed = crate::buffer::js_array_buffer_new(8);
    let boxed = f64::from_bits(crate::value::JSValue::pointer(fixed as *const u8).bits());
    let f32v = js_typed_array_view(KIND_FLOAT32 as i32, boxed, 0.0, undefined());
    crate::buffer::detach_array_buffer(fixed as usize);
    assert!(crate::exception::catch_js_throw(|| fill(f32v, 1.0, &[])).is_err());
}
