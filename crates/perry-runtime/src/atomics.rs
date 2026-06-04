//! Minimal `Atomics` namespace operations for integer TypedArray views.

use crate::closure::ClosureHeader;
use crate::typedarray::{
    clean_ta_ptr, js_typed_array_get, js_typed_array_length, js_typed_array_set,
    lookup_typed_array_kind, TypedArrayHeader, KIND_BIGINT64, KIND_BIGUINT64, KIND_INT16,
    KIND_INT32, KIND_INT8, KIND_UINT16, KIND_UINT32, KIND_UINT8,
};
use crate::value::JSValue;

fn nanbox_bool(value: bool) -> f64 {
    f64::from_bits(JSValue::bool(value).bits())
}

fn string_value(bytes: &[u8]) -> f64 {
    let ptr = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    crate::value::js_nanbox_string(ptr as i64)
}

fn object_key(bytes: &[u8]) -> *const crate::StringHeader {
    crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

fn throw_type_error(message: &[u8]) -> ! {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

fn throw_type_error_string(message: String) -> ! {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

fn throw_range_error(message: &[u8]) -> ! {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_rangeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

fn numeric_integer_kind(kind: u8) -> bool {
    matches!(
        kind,
        KIND_INT8 | KIND_UINT8 | KIND_INT16 | KIND_UINT16 | KIND_INT32 | KIND_UINT32
    )
}

fn bigint_integer_kind(kind: u8) -> bool {
    matches!(kind, KIND_BIGINT64 | KIND_BIGUINT64)
}

fn supported_integer_kind(kind: u8) -> bool {
    numeric_integer_kind(kind) || bigint_integer_kind(kind)
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

    fn is_bigint(&self) -> bool {
        bigint_integer_kind(self.kind())
    }

    fn has_shared_backing(&self) -> bool {
        match self {
            AtomicView::TypedArray { ptr, .. } => {
                crate::typedarray::typed_array_has_shared_backing(*ptr)
            }
            AtomicView::Uint8ArrayBuffer(ptr) => {
                crate::buffer::is_shared_array_buffer(*ptr as usize)
            }
        }
    }

    fn length(&self) -> i32 {
        match self {
            AtomicView::TypedArray { ptr, .. } => js_typed_array_length(*ptr),
            AtomicView::Uint8ArrayBuffer(ptr) => crate::buffer::js_buffer_length(*ptr),
        }
    }

    fn get_numeric(&self, index: i32) -> f64 {
        match self {
            AtomicView::TypedArray { ptr, .. } => js_typed_array_get(*ptr, index),
            AtomicView::Uint8ArrayBuffer(ptr) => {
                crate::buffer::js_buffer_get(*ptr as *const crate::buffer::BufferHeader, index)
                    as f64
            }
        }
    }

    fn set_numeric(&self, index: i32, value: f64) {
        match self {
            AtomicView::TypedArray { ptr, .. } => js_typed_array_set(*ptr, index, value),
            AtomicView::Uint8ArrayBuffer(ptr) => {
                crate::buffer::js_buffer_set(*ptr, index, value as i32);
            }
        }
    }

    fn get_bigint_bits(&self, index: i32) -> u64 {
        match self {
            AtomicView::TypedArray { ptr, .. } => typed_array_bigint_bits(*ptr, index),
            AtomicView::Uint8ArrayBuffer(_) => {
                throw_type_error(b"Atomics operation requires a BigInt typed array")
            }
        }
    }

    fn set_bigint_bits(&self, index: i32, value: u64) {
        match self {
            AtomicView::TypedArray { ptr, .. } => typed_array_set_bigint_bits(*ptr, index, value),
            AtomicView::Uint8ArrayBuffer(_) => {
                throw_type_error(b"Atomics operation requires a BigInt typed array")
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

fn atomics_int32_view_arg(value: f64) -> AtomicView {
    let js = JSValue::from_bits(value.to_bits());
    if !js.is_pointer() {
        throw_type_error(b"Atomics wait/notify requires an Int32Array");
    }
    let raw = clean_ta_ptr(js.as_pointer::<TypedArrayHeader>()) as usize;
    if raw == 0 {
        throw_type_error(b"Atomics wait/notify requires an Int32Array");
    }
    if lookup_typed_array_kind(raw) == Some(KIND_INT32) {
        return AtomicView::TypedArray {
            ptr: raw as *mut TypedArrayHeader,
            kind: KIND_INT32,
        };
    }
    throw_type_error(b"Atomics wait/notify requires an Int32Array");
}

fn atomics_wait_notify_view_arg(value: f64) -> AtomicView {
    let js = JSValue::from_bits(value.to_bits());
    if !js.is_pointer() {
        throw_type_error(b"Atomics wait/notify requires an Int32Array or BigInt64Array");
    }
    let raw = clean_ta_ptr(js.as_pointer::<TypedArrayHeader>()) as usize;
    if raw == 0 {
        throw_type_error(b"Atomics wait/notify requires an Int32Array or BigInt64Array");
    }
    match lookup_typed_array_kind(raw) {
        Some(kind @ (KIND_INT32 | KIND_BIGINT64)) => AtomicView::TypedArray {
            ptr: raw as *mut TypedArrayHeader,
            kind,
        },
        _ => throw_type_error(b"Atomics wait/notify requires an Int32Array or BigInt64Array"),
    }
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

fn number_arg(value: f64) -> f64 {
    let js = JSValue::from_bits(value.to_bits());
    if js.is_bigint() {
        throw_type_error(b"Cannot convert a BigInt value to a number");
    }
    if js.is_any_string() {
        let ptr = crate::value::js_get_string_pointer_unified(value)
            as *const crate::string::StringHeader;
        if ptr.is_null() {
            f64::NAN
        } else {
            unsafe {
                let len = (*ptr).byte_len as usize;
                let data =
                    (ptr as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
                match std::str::from_utf8(std::slice::from_raw_parts(data, len)) {
                    Ok(s) => match s.trim() {
                        "Infinity" | "+Infinity" => f64::INFINITY,
                        "-Infinity" => f64::NEG_INFINITY,
                        other => other.parse::<f64>().unwrap_or(f64::NAN),
                    },
                    Err(_) => f64::NAN,
                }
            }
        }
    } else {
        js.to_number()
    }
}

fn numeric_arg(value: f64) -> f64 {
    let n = number_arg(value);
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

fn format_number_label(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_string();
    }
    if value.is_infinite() {
        return if value.is_sign_negative() {
            "-Infinity".to_string()
        } else {
            "Infinity".to_string()
        };
    }
    if value == 0.0 {
        return "0".to_string();
    }
    if value.fract() == 0.0 && value.abs() < 1e21 {
        return format!("{value:.0}");
    }
    format!("{value}")
}

fn throw_number_to_bigint_error(value: f64, js: JSValue) -> ! {
    let label = if js.is_int32() {
        js.as_int32().to_string()
    } else {
        format_number_label(value)
    };
    throw_type_error_string(format!("Cannot convert {label} to a BigInt"))
}

fn bigint_value(value: f64) -> f64 {
    let js = JSValue::from_bits(value.to_bits());
    if js.is_bigint() {
        return value;
    }
    if js.is_int32() || js.is_number() {
        throw_number_to_bigint_error(value, js);
    }
    if js.is_bool() || js.is_any_string() {
        let ptr = crate::bigint::js_bigint_from_f64(value);
        return f64::from_bits(JSValue::bigint_ptr(ptr).bits());
    }
    if js.is_undefined() {
        throw_type_error(b"Cannot convert undefined to a BigInt");
    }
    if js.is_null() {
        throw_type_error(b"Cannot convert null to a BigInt");
    }
    throw_type_error(b"Cannot convert value to a BigInt");
}

fn bigint_bits(value: f64) -> u64 {
    let coerced = bigint_value(value);
    let ptr = JSValue::from_bits(coerced.to_bits()).as_bigint_ptr();
    let ptr = crate::bigint::clean_bigint_ptr(ptr);
    if ptr.is_null() {
        return 0;
    }
    unsafe { (*ptr).limbs[0] }
}

fn bigint_result_for_kind(kind: u8, bits: u64) -> f64 {
    let ptr = match kind {
        KIND_BIGINT64 => crate::bigint::js_bigint_from_i64(bits as i64),
        KIND_BIGUINT64 => crate::bigint::js_bigint_from_u64(bits),
        _ => crate::bigint::js_bigint_from_i64(bits as i64),
    };
    f64::from_bits(JSValue::bigint_ptr(ptr).bits())
}

fn typed_array_bigint_bits(ta: *const TypedArrayHeader, index: i32) -> u64 {
    let ta = clean_ta_ptr(ta);
    if ta.is_null() || index < 0 {
        return 0;
    }
    unsafe {
        if crate::native_arena::is_native_typed_view(ta) {
            crate::native_arena::validate_view_alive(
                crate::native_arena::native_view_from_typed_array(ta),
            );
        }
        if index as u32 >= (*ta).length {
            return 0;
        }
        let data = crate::typedarray::typed_array_bytes(ta).unwrap_or(&[]);
        let off = (index as usize).saturating_mul((*ta).elem_size as usize);
        let bytes = data.get(off..off + 8).unwrap_or(&[]);
        if bytes.len() != 8 {
            return 0;
        }
        u64::from_ne_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ])
    }
}

fn typed_array_set_bigint_bits(ta: *mut TypedArrayHeader, index: i32, value: u64) {
    let ta = clean_ta_ptr(ta) as *mut TypedArrayHeader;
    if ta.is_null() || index < 0 {
        return;
    }
    unsafe {
        if crate::native_arena::is_native_typed_view(ta as *const TypedArrayHeader) {
            crate::native_arena::validate_view_alive(
                crate::native_arena::native_view_from_typed_array(ta as *const TypedArrayHeader),
            );
        }
        if index as u32 >= (*ta).length {
            return;
        }
        let Some(data) = crate::typedarray::typed_array_bytes_mut(ta) else {
            return;
        };
        let off = (index as usize).saturating_mul((*ta).elem_size as usize);
        if let Some(slot) = data.get_mut(off..off + 8) {
            slot.copy_from_slice(&value.to_ne_bytes());
        }
    }
}

fn slot(view: f64, index: f64) -> (AtomicView, i32) {
    let view = atomics_view_arg(view);
    let idx = atomics_to_index(index, view.length());
    (view, idx)
}

fn int32_slot(view: f64, index: f64) -> (AtomicView, i32) {
    let view = atomics_int32_view_arg(view);
    let idx = atomics_to_index(index, view.length());
    (view, idx)
}

fn wait_notify_slot(view: f64, index: f64) -> (AtomicView, i32) {
    let view = atomics_wait_notify_view_arg(view);
    let idx = atomics_to_index(index, view.length());
    (view, idx)
}

fn wait_async_result(async_value: bool, value: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj = crate::object::js_object_alloc(0, 2);
    let obj_handle = scope.root_raw_mut_ptr(obj);
    let value_handle = scope.root_nanbox_f64(value);

    let async_key = object_key(b"async");
    let async_key_handle = scope.root_string_ptr(async_key);
    crate::object::js_object_set_field_by_name(
        obj_handle.get_raw_mut_ptr(),
        async_key_handle.get_raw_const_ptr(),
        nanbox_bool(async_value),
    );

    let value_key = object_key(b"value");
    let value_key_handle = scope.root_string_ptr(value_key);
    crate::object::js_object_set_field_by_name(
        obj_handle.get_raw_mut_ptr(),
        value_key_handle.get_raw_const_ptr(),
        value_handle.get_nanbox_f64(),
    );

    crate::value::js_nanbox_pointer(
        obj_handle.get_raw_mut_ptr::<crate::object::ObjectHeader>() as i64
    )
}

fn atomics_bitwise(view: f64, index: f64, value: f64, op: impl FnOnce(u64, u64) -> u64) -> f64 {
    let (view, idx) = slot(view, index);
    if view.is_bigint() {
        let kind = view.kind();
        let previous = view.get_bigint_bits(idx);
        let result = op(previous, bigint_bits(value));
        view.set_bigint_bits(idx, result);
        return bigint_result_for_kind(kind, previous);
    }
    let kind = view.kind();
    let previous = view.get_numeric(idx);
    let result = op(
        to_uint32_bits(previous) as u64,
        to_uint32_bits(value) as u64,
    );
    view.set_numeric(idx, bitwise_result_for_kind(kind, result as u32));
    previous
}

#[no_mangle]
pub extern "C" fn js_atomics_load(_closure: *const ClosureHeader, view: f64, index: f64) -> f64 {
    let (view, idx) = slot(view, index);
    if view.is_bigint() {
        return bigint_result_for_kind(view.kind(), view.get_bigint_bits(idx));
    }
    view.get_numeric(idx)
}

#[no_mangle]
pub extern "C" fn js_atomics_is_lock_free(_closure: *const ClosureHeader, size: f64) -> f64 {
    let n = number_arg(size);
    nanbox_bool(n.is_finite() && n.trunc() == n && matches!(n as i32, 1 | 2 | 4 | 8))
}

#[no_mangle]
pub extern "C" fn js_atomics_store(
    _closure: *const ClosureHeader,
    view: f64,
    index: f64,
    value: f64,
) -> f64 {
    let (view, idx) = slot(view, index);
    if view.is_bigint() {
        let stored = bigint_value(value);
        view.set_bigint_bits(idx, bigint_bits(stored));
        return stored;
    }
    view.set_numeric(idx, coerce_for_kind(view.kind(), value));
    view.get_numeric(idx)
}

#[no_mangle]
pub extern "C" fn js_atomics_add(
    _closure: *const ClosureHeader,
    view: f64,
    index: f64,
    value: f64,
) -> f64 {
    let (view, idx) = slot(view, index);
    if view.is_bigint() {
        let kind = view.kind();
        let previous = view.get_bigint_bits(idx);
        view.set_bigint_bits(idx, previous.wrapping_add(bigint_bits(value)));
        return bigint_result_for_kind(kind, previous);
    }
    let previous = view.get_numeric(idx);
    view.set_numeric(
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
    if view.is_bigint() {
        let kind = view.kind();
        let previous = view.get_bigint_bits(idx);
        view.set_bigint_bits(idx, previous.wrapping_sub(bigint_bits(value)));
        return bigint_result_for_kind(kind, previous);
    }
    let previous = view.get_numeric(idx);
    view.set_numeric(
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
    if view.is_bigint() {
        let kind = view.kind();
        let previous = view.get_bigint_bits(idx);
        view.set_bigint_bits(idx, bigint_bits(value));
        return bigint_result_for_kind(kind, previous);
    }
    let previous = view.get_numeric(idx);
    view.set_numeric(idx, coerce_for_kind(view.kind(), value));
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
    if view.is_bigint() {
        let kind = view.kind();
        let previous = view.get_bigint_bits(idx);
        let expected = bigint_bits(expected);
        let replacement = bigint_bits(replacement);
        if previous == expected {
            view.set_bigint_bits(idx, replacement);
        }
        return bigint_result_for_kind(kind, previous);
    }
    let previous = view.get_numeric(idx);
    let expected = coerce_for_kind(view.kind(), expected);
    let replacement = coerce_for_kind(view.kind(), replacement);
    if previous == expected {
        view.set_numeric(idx, replacement);
    }
    previous
}

#[no_mangle]
pub extern "C" fn js_atomics_notify(
    _closure: *const ClosureHeader,
    view: f64,
    index: f64,
    count: f64,
) -> f64 {
    let (view, _idx) = wait_notify_slot(view, index);
    let _ = numeric_arg(count);
    if !view.has_shared_backing() {
        return 0.0;
    }
    0.0
}

#[no_mangle]
pub extern "C" fn js_atomics_wait(
    _closure: *const ClosureHeader,
    view: f64,
    index: f64,
    expected: f64,
    timeout: f64,
) -> f64 {
    let (view, idx) = wait_notify_slot(view, index);
    if !view.has_shared_backing() {
        throw_type_error(b"Atomics.wait requires a shared typed array");
    }
    if view.kind() == KIND_BIGINT64 {
        let expected = bigint_bits(expected);
        if view.get_bigint_bits(idx) != expected {
            return string_value(b"not-equal");
        }
    } else {
        let expected = coerce_for_kind(KIND_INT32, expected);
        if view.get_numeric(idx) != expected {
            return string_value(b"not-equal");
        }
    }

    let _ = number_arg(timeout);
    string_value(b"timed-out")
}

#[no_mangle]
pub extern "C" fn js_atomics_wait_async(
    _closure: *const ClosureHeader,
    view: f64,
    index: f64,
    expected: f64,
    timeout: f64,
) -> f64 {
    let (view, idx) = int32_slot(view, index);
    let expected = coerce_for_kind(KIND_INT32, expected);
    let timeout = number_arg(timeout);
    if view.get_numeric(idx) != expected {
        return wait_async_result(false, string_value(b"not-equal"));
    }
    if timeout <= 0.0 {
        return wait_async_result(false, string_value(b"timed-out"));
    }

    let scope = crate::gc::RuntimeHandleScope::new();
    let timed_out = scope.root_nanbox_f64(string_value(b"timed-out"));
    let promise = crate::promise::js_promise_resolved(timed_out.get_nanbox_f64());
    wait_async_result(true, crate::value::js_nanbox_pointer(promise as i64))
}
