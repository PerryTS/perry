//! Minimal `Atomics` namespace operations for integer TypedArray views.

use crate::closure::ClosureHeader;
use crate::typedarray::{
    clean_ta_ptr, js_typed_array_get, js_typed_array_length, js_typed_array_set,
    lookup_typed_array_kind, TypedArrayHeader, KIND_INT16, KIND_INT32, KIND_INT8, KIND_UINT16,
    KIND_UINT32, KIND_UINT8,
};
use crate::value::JSValue;

fn throw_type_error(message: &[u8]) -> ! {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

fn throw_range_error(message: &[u8]) -> ! {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_rangeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

fn supported_integer_kind(kind: u8) -> bool {
    matches!(
        kind,
        KIND_INT8 | KIND_UINT8 | KIND_INT16 | KIND_UINT16 | KIND_INT32 | KIND_UINT32
    )
}

fn typed_array_arg(value: f64) -> (*mut TypedArrayHeader, u8) {
    let js = JSValue::from_bits(value.to_bits());
    if !js.is_pointer() {
        throw_type_error(b"Atomics operation requires an integer typed array");
    }
    let ta = clean_ta_ptr(js.as_pointer::<TypedArrayHeader>());
    if ta.is_null() {
        throw_type_error(b"Atomics operation requires an integer typed array");
    }
    let Some(kind) = lookup_typed_array_kind(ta as usize) else {
        throw_type_error(b"Atomics operation requires an integer typed array");
    };
    if !supported_integer_kind(kind) {
        throw_type_error(b"Atomics operation requires an integer typed array");
    }
    (ta as *mut TypedArrayHeader, kind)
}

fn atomics_to_index(index: f64, length: i32) -> i32 {
    let mut n = JSValue::from_bits(index.to_bits()).to_number();
    if n.is_nan() {
        n = 0.0;
    }
    if n < 0.0 || n > 9_007_199_254_740_991.0 {
        throw_range_error(b"Invalid atomic access index");
    }
    let i = n.trunc();
    if i >= length as f64 {
        throw_range_error(b"Invalid atomic access index");
    }
    i as i32
}

fn numeric_arg(value: f64) -> f64 {
    let js = JSValue::from_bits(value.to_bits());
    let n = if js.is_any_string() {
        let ptr = crate::value::js_get_string_pointer_unified(value)
            as *const crate::string::StringHeader;
        if ptr.is_null() {
            f64::NAN
        } else {
            unsafe {
                let len = (*ptr).byte_len as usize;
                let data =
                    (ptr as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
                std::str::from_utf8(std::slice::from_raw_parts(data, len))
                    .ok()
                    .and_then(|s| s.trim().parse::<f64>().ok())
                    .unwrap_or(f64::NAN)
            }
        }
    } else {
        js.to_number()
    };
    if n.is_finite() {
        n.trunc()
    } else {
        0.0
    }
}

fn coerce_for_kind(kind: u8, value: f64) -> f64 {
    let n = numeric_arg(value);
    match kind {
        KIND_INT8 => (n as i32 as i8) as f64,
        KIND_UINT8 => (n as i64).rem_euclid(256) as f64,
        KIND_INT16 => (n as i32 as i16) as f64,
        KIND_UINT16 => (n as i64).rem_euclid(65_536) as f64,
        KIND_INT32 => (n as i32) as f64,
        KIND_UINT32 => (n as i64 as u32) as f64,
        _ => n,
    }
}

fn slot(view: f64, index: f64) -> (*mut TypedArrayHeader, u8, i32) {
    let (ta, kind) = typed_array_arg(view);
    let idx = atomics_to_index(index, js_typed_array_length(ta));
    (ta, kind, idx)
}

#[no_mangle]
pub extern "C" fn js_atomics_load(_closure: *const ClosureHeader, view: f64, index: f64) -> f64 {
    let (ta, _, idx) = slot(view, index);
    js_typed_array_get(ta, idx)
}

#[no_mangle]
pub extern "C" fn js_atomics_store(
    _closure: *const ClosureHeader,
    view: f64,
    index: f64,
    value: f64,
) -> f64 {
    let (ta, kind, idx) = slot(view, index);
    js_typed_array_set(ta, idx, coerce_for_kind(kind, value));
    js_typed_array_get(ta, idx)
}

#[no_mangle]
pub extern "C" fn js_atomics_add(
    _closure: *const ClosureHeader,
    view: f64,
    index: f64,
    value: f64,
) -> f64 {
    let (ta, kind, idx) = slot(view, index);
    let previous = js_typed_array_get(ta, idx);
    js_typed_array_set(
        ta,
        idx,
        coerce_for_kind(kind, previous + numeric_arg(value)),
    );
    previous
}

#[no_mangle]
pub extern "C" fn js_atomics_sub(
    _closure: *const ClosureHeader,
    view: f64,
    index: f64,
    value: f64,
) -> f64 {
    let (ta, kind, idx) = slot(view, index);
    let previous = js_typed_array_get(ta, idx);
    js_typed_array_set(
        ta,
        idx,
        coerce_for_kind(kind, previous - numeric_arg(value)),
    );
    previous
}

#[no_mangle]
pub extern "C" fn js_atomics_exchange(
    _closure: *const ClosureHeader,
    view: f64,
    index: f64,
    value: f64,
) -> f64 {
    let (ta, kind, idx) = slot(view, index);
    let previous = js_typed_array_get(ta, idx);
    js_typed_array_set(ta, idx, coerce_for_kind(kind, value));
    previous
}

#[no_mangle]
pub extern "C" fn js_atomics_compare_exchange(
    _closure: *const ClosureHeader,
    view: f64,
    index: f64,
    expected: f64,
    replacement: f64,
) -> f64 {
    let (ta, kind, idx) = slot(view, index);
    let previous = js_typed_array_get(ta, idx);
    if previous == coerce_for_kind(kind, expected) {
        js_typed_array_set(ta, idx, coerce_for_kind(kind, replacement));
    }
    previous
}
