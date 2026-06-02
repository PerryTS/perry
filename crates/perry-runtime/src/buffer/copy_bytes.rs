use super::*;

use crate::value::JSValue;

const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

fn value_addr(value: f64) -> usize {
    let bits = value.to_bits();
    if (bits >> 48) >= 0x7FF8 {
        (bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else {
        bits as usize
    }
}

fn format_received_number(n: f64) -> String {
    if n.is_nan() {
        "NaN".to_string()
    } else if n.is_infinite() {
        if n.is_sign_negative() {
            "-Infinity".to_string()
        } else {
            "Infinity".to_string()
        }
    } else if n.fract() == 0.0 && n.abs() < 1e21 && n >= i64::MIN as f64 && n <= i64::MAX as f64 {
        format!("{}", n as i64)
    } else {
        format!("{}", n)
    }
}

fn optional_index(value: f64, name: &str) -> Option<usize> {
    let js_value = JSValue::from_bits(value.to_bits());
    if js_value.is_undefined() {
        return None;
    }
    if !crate::fs::validate::is_numeric(js_value) {
        let message = format!(
            "The \"{}\" argument must be of type number. Received {}",
            name,
            crate::fs::validate::describe_received(value)
        );
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    }
    let number = if js_value.is_int32() {
        js_value.as_int32() as f64
    } else {
        js_value.as_number()
    };
    if !number.is_finite() || number.fract() != 0.0 {
        let message = format!(
            "The value of \"{}\" is out of range. It must be an integer. Received {}",
            name,
            format_received_number(number)
        );
        crate::fs::validate::throw_range_error_with_code(&message);
    }
    if !(0.0..=MAX_SAFE_INTEGER).contains(&number) {
        let message = format!(
            "The value of \"{}\" is out of range. It must be >= 0 && <= 9007199254740991. Received {}",
            name,
            format_received_number(number)
        );
        crate::fs::validate::throw_range_error_with_code(&message);
    }
    Some(number as usize)
}

fn throw_invalid_view(value: f64) -> ! {
    let message = format!(
        "The \"view\" argument must be an instance of TypedArray. Received {}",
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
}

unsafe fn buffer_from_slice(bytes: &[u8]) -> *mut BufferHeader {
    let len = bytes.len().min(u32::MAX as usize);
    let buf = buffer_alloc(len as u32);
    (*buf).length = len as u32;
    if len > 0 {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), buffer_data_mut(buf), len);
    }
    buf
}

unsafe fn copy_registered_buffer(
    source: *const BufferHeader,
    offset_value: f64,
    length_value: f64,
) -> *mut BufferHeader {
    let element_len = (*source).length as usize;
    let offset = optional_index(offset_value, "offset").unwrap_or(0);
    let start = offset.min(element_len);
    let available = element_len.saturating_sub(start);
    let requested = optional_index(length_value, "length").unwrap_or(available);
    let take = requested.min(available);
    let out = buffer_alloc(take as u32);
    (*out).length = take as u32;
    let dst = buffer_data_mut(out);
    for i in 0..take {
        *dst.add(i) = js_buffer_get(source, (start + i) as i32) as u8;
    }
    out
}

unsafe fn copy_typed_array(
    source: *const crate::typedarray::TypedArrayHeader,
    offset_value: f64,
    length_value: f64,
) -> *mut BufferHeader {
    let source = crate::typedarray::clean_ta_ptr(source);
    if source.is_null() {
        throw_invalid_view(f64::from_bits(JSValue::undefined().bits()));
    }
    let element_len = (*source).length as usize;
    let element_size = (*source).elem_size as usize;
    let offset = optional_index(offset_value, "offset").unwrap_or(0);
    let start = offset.min(element_len);
    let available = element_len.saturating_sub(start);
    let requested = optional_index(length_value, "length").unwrap_or(available);
    let take = requested.min(available);
    let Some(bytes) = crate::typedarray::typed_array_bytes(source) else {
        return buffer_alloc(0);
    };
    let byte_start = start.saturating_mul(element_size).min(bytes.len());
    let byte_len = take
        .saturating_mul(element_size)
        .min(bytes.len().saturating_sub(byte_start));
    buffer_from_slice(&bytes[byte_start..byte_start + byte_len])
}

/// `Buffer.copyBytesFrom(view[, offset[, length]])`.
///
/// Node accepts TypedArray instances, including Buffer, and copies raw bytes
/// from an element range. DataView and ArrayBuffer are intentionally rejected.
#[no_mangle]
pub extern "C" fn js_buffer_copy_bytes_from(
    view: f64,
    offset_value: f64,
    length_value: f64,
) -> *mut BufferHeader {
    let addr = value_addr(view);
    if addr < 0x1000 {
        throw_invalid_view(view);
    }

    unsafe {
        if is_registered_buffer(addr) {
            if is_any_array_buffer(addr) || is_data_view(addr) {
                throw_invalid_view(view);
            }
            return copy_registered_buffer(addr as *const BufferHeader, offset_value, length_value);
        }

        if crate::typedarray::lookup_typed_array_kind(addr).is_some() {
            return copy_typed_array(
                addr as *const crate::typedarray::TypedArrayHeader,
                offset_value,
                length_value,
            );
        }
    }

    throw_invalid_view(view);
}
