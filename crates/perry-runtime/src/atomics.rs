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

enum AtomicView {
    TypedArray {
        ptr: *mut TypedArrayHeader,
        kind: u8,
    },
    Uint8ArrayBuffer(*mut crate::buffer::BufferHeader),
}

impl AtomicView {
    fn kind(&self) -> u8 {
        match self {
            AtomicView::TypedArray { kind, .. } => *kind,
            AtomicView::Uint8ArrayBuffer(_) => KIND_UINT8,
        }
    }

    fn length(&self) -> i32 {
        match self {
            AtomicView::TypedArray { ptr, .. } => js_typed_array_length(*ptr),
            AtomicView::Uint8ArrayBuffer(ptr) => crate::buffer::js_buffer_length(*ptr),
        }
    }

    fn get(&self, index: i32) -> f64 {
        match self {
            AtomicView::TypedArray { ptr, .. } => js_typed_array_get(*ptr, index),
            AtomicView::Uint8ArrayBuffer(ptr) => {
                crate::buffer::js_buffer_get(*ptr as *const crate::buffer::BufferHeader, index)
                    as f64
            }
        }
    }

    fn set(&self, index: i32, value: f64) {
        match self {
            AtomicView::TypedArray { ptr, .. } => js_typed_array_set(*ptr, index, value),
            AtomicView::Uint8ArrayBuffer(ptr) => {
                crate::buffer::js_buffer_set(*ptr, index, value as i32);
            }
        }
    }
}

fn atomics_view_arg(value: f64) -> AtomicView {
    let js = JSValue::from_bits(value.to_bits());
    if !js.is_pointer() {
        throw_type_error(b"Atomics operation requires an integer typed array");
    }
    let raw = clean_ta_ptr(js.as_pointer::<TypedArrayHeader>()) as usize;
    if raw == 0 {
        throw_type_error(b"Atomics operation requires an integer typed array");
    }
    if let Some(kind) = lookup_typed_array_kind(raw) {
        if !supported_integer_kind(kind) {
            throw_type_error(b"Atomics operation requires an integer typed array");
        }
        return AtomicView::TypedArray {
            ptr: raw as *mut TypedArrayHeader,
            kind,
        };
    }
    if crate::buffer::is_registered_buffer(raw) && crate::buffer::is_uint8array_buffer(raw) {
        return AtomicView::Uint8ArrayBuffer(raw as *mut crate::buffer::BufferHeader);
    }
    throw_type_error(b"Atomics operation requires an integer typed array");
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

fn to_uint32_bits(value: f64) -> u32 {
    numeric_arg(value).rem_euclid(4_294_967_296.0) as u32
}

fn bitwise_result_for_kind(kind: u8, bits: u32) -> f64 {
    match kind {
        KIND_INT8 => (bits as u8 as i8) as f64,
        KIND_UINT8 => (bits as u8) as f64,
        KIND_INT16 => (bits as u16 as i16) as f64,
        KIND_UINT16 => (bits as u16) as f64,
        KIND_INT32 => (bits as i32) as f64,
        KIND_UINT32 => bits as f64,
        _ => bits as f64,
    }
}

fn slot(view: f64, index: f64) -> (AtomicView, i32) {
    let view = atomics_view_arg(view);
    let idx = atomics_to_index(index, view.length());
    (view, idx)
}

fn atomics_bitwise(view: f64, index: f64, value: f64, op: impl FnOnce(u32, u32) -> u32) -> f64 {
    let (view, idx) = slot(view, index);
    let kind = view.kind();
    let previous = view.get(idx);
    let result = op(to_uint32_bits(previous), to_uint32_bits(value));
    view.set(idx, bitwise_result_for_kind(kind, result));
    previous
}

#[no_mangle]
pub extern "C" fn js_atomics_load(_closure: *const ClosureHeader, view: f64, index: f64) -> f64 {
    let (view, idx) = slot(view, index);
    view.get(idx)
}

#[no_mangle]
pub extern "C" fn js_atomics_store(
    _closure: *const ClosureHeader,
    view: f64,
    index: f64,
    value: f64,
) -> f64 {
    let (view, idx) = slot(view, index);
    view.set(idx, coerce_for_kind(view.kind(), value));
    view.get(idx)
}

#[no_mangle]
pub extern "C" fn js_atomics_add(
    _closure: *const ClosureHeader,
    view: f64,
    index: f64,
    value: f64,
) -> f64 {
    let (view, idx) = slot(view, index);
    let previous = view.get(idx);
    view.set(
        idx,
        coerce_for_kind(view.kind(), previous + numeric_arg(value)),
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
    let (view, idx) = slot(view, index);
    let previous = view.get(idx);
    view.set(
        idx,
        coerce_for_kind(view.kind(), previous - numeric_arg(value)),
    );
    previous
}

#[no_mangle]
pub extern "C" fn js_atomics_and(
    _closure: *const ClosureHeader,
    view: f64,
    index: f64,
    value: f64,
) -> f64 {
    atomics_bitwise(view, index, value, |a, b| a & b)
}

#[no_mangle]
pub extern "C" fn js_atomics_or(
    _closure: *const ClosureHeader,
    view: f64,
    index: f64,
    value: f64,
) -> f64 {
    atomics_bitwise(view, index, value, |a, b| a | b)
}

#[no_mangle]
pub extern "C" fn js_atomics_xor(
    _closure: *const ClosureHeader,
    view: f64,
    index: f64,
    value: f64,
) -> f64 {
    atomics_bitwise(view, index, value, |a, b| a ^ b)
}

#[no_mangle]
pub extern "C" fn js_atomics_exchange(
    _closure: *const ClosureHeader,
    view: f64,
    index: f64,
    value: f64,
) -> f64 {
    let (view, idx) = slot(view, index);
    let previous = view.get(idx);
    view.set(idx, coerce_for_kind(view.kind(), value));
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
    let (view, idx) = slot(view, index);
    let previous = view.get(idx);
    if previous == coerce_for_kind(view.kind(), expected) {
        view.set(idx, coerce_for_kind(view.kind(), replacement));
    }
    previous
}
