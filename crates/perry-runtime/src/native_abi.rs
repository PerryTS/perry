//! Strict runtime validators for manifest-declared native-library ABI calls.
//!
//! These helpers are intentionally narrower than the legacy conversion helpers
//! used by the rest of the runtime. Manifest lowering calls them before handing
//! raw scalars, pointers, buffer spans, strings, or promises to native code.

use crate::buffer::{buffer_data, is_registered_buffer, BufferHeader};
use crate::object::ObjectHeader;
use crate::promise::Promise;
use crate::value::{JSValue, POINTER_MASK};

const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
const MIN_SAFE_INTEGER: f64 = -9_007_199_254_740_991.0;

#[cold]
fn throw_type_error(message: &str) -> ! {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

fn strict_number(value: f64, message: &str) -> f64 {
    let js_value = JSValue::from_bits(value.to_bits());
    if js_value.is_int32() {
        js_value.as_int32() as f64
    } else if js_value.is_number() {
        js_value.as_number()
    } else {
        throw_type_error(message)
    }
}

fn strict_integer(value: f64, message: &str) -> f64 {
    let number = strict_number(value, message);
    if !number.is_finite() || number.fract() != 0.0 {
        throw_type_error(message);
    }
    number
}

fn strict_safe_integer(value: f64, message: &str) -> f64 {
    let number = strict_integer(value, message);
    if !(MIN_SAFE_INTEGER..=MAX_SAFE_INTEGER).contains(&number) {
        throw_type_error(message);
    }
    number
}

fn strict_buffer_from_value(value: f64) -> *const BufferHeader {
    let bits = value.to_bits();
    let js_value = JSValue::from_bits(bits);
    let raw_ptr = if js_value.is_pointer() || js_value.is_string() {
        (bits & POINTER_MASK) as usize
    } else if !value.is_nan() && (0x1000..0x0001_0000_0000_0000).contains(&bits) {
        bits as usize
    } else {
        0
    };
    if raw_ptr != 0 && is_registered_buffer(raw_ptr) {
        raw_ptr as *const BufferHeader
    } else {
        throw_type_error("Expected a Buffer or Uint8Array for native buffer span")
    }
}

/// Validate that a manifest `f64` parameter is a JavaScript number.
#[no_mangle]
pub extern "C" fn js_native_abi_check_f64(value: f64) -> f64 {
    strict_number(value, "Expected number for native f64 parameter")
}

/// Validate and lower a manifest `f32` parameter.
#[no_mangle]
pub extern "C" fn js_native_abi_check_f32(value: f64) -> f32 {
    let number = strict_number(value, "Expected number for native f32 parameter");
    if number.is_finite() && (number < f32::MIN as f64 || number > f32::MAX as f64) {
        throw_type_error("Native f32 parameter is out of range");
    }
    number as f32
}

/// Validate and lower a manifest `i32` parameter.
#[no_mangle]
pub extern "C" fn js_native_abi_check_i32(value: f64) -> i32 {
    let number = strict_integer(
        value,
        "Expected int32-compatible number for native i32 parameter",
    );
    if number < i32::MIN as f64 || number > i32::MAX as f64 {
        throw_type_error("Native i32 parameter is out of range");
    }
    number as i32
}

/// Validate and lower a manifest `i64` parameter.
#[no_mangle]
pub extern "C" fn js_native_abi_check_i64(value: f64) -> i64 {
    let number = strict_safe_integer(value, "Expected safe integer for native i64 parameter");
    number as i64
}

/// Validate and lower a manifest `u32` or standalone `buffer_len` parameter.
#[no_mangle]
pub extern "C" fn js_native_abi_check_u32(value: f64) -> u32 {
    let number = strict_integer(
        value,
        "Expected uint32-compatible number for native u32 parameter",
    );
    if number < 0.0 || number > u32::MAX as f64 {
        throw_type_error("Native u32 parameter is out of range");
    }
    number as u32
}

/// Validate and lower a manifest `u64` parameter.
#[no_mangle]
pub extern "C" fn js_native_abi_check_u64(value: f64) -> u64 {
    let number = strict_safe_integer(value, "Expected safe integer for native u64 parameter");
    if number < 0.0 {
        throw_type_error("Native u64 parameter is out of range");
    }
    number as u64
}

/// Validate and lower a manifest `usize` parameter on 64-bit native targets.
#[no_mangle]
pub extern "C" fn js_native_abi_check_usize(value: f64) -> usize {
    let number = strict_safe_integer(value, "Expected safe integer for native usize parameter");
    if number < 0.0 {
        throw_type_error("Native usize parameter is out of range");
    }
    number as usize
}

/// Validate a manifest `string` parameter and return a raw StringHeader pointer.
#[no_mangle]
pub extern "C" fn js_native_abi_check_string_ptr(value: f64) -> i64 {
    let js_value = JSValue::from_bits(value.to_bits());
    if js_value.is_string() || js_value.is_short_string() {
        let ptr = crate::value::js_get_string_pointer_unified(value);
        if ptr != 0 {
            return ptr;
        }
    }
    throw_type_error("Expected string for native string parameter")
}

/// Validate a manifest `ptr` parameter and return the raw payload.
#[no_mangle]
pub extern "C" fn js_native_abi_check_ptr(value: f64) -> i64 {
    let bits = value.to_bits();
    let js_value = JSValue::from_bits(bits);
    if js_value.is_pointer() || js_value.is_string() {
        return (bits & POINTER_MASK) as i64;
    }
    if !value.is_nan() && (0x10000..0x0001_0000_0000_0000).contains(&bits) && (bits & 0x7) == 0 {
        return bits as i64;
    }
    throw_type_error("Expected pointer-compatible value for native ptr parameter")
}

/// Validate and lower the data pointer half of a manifest `buffer+len` span.
#[no_mangle]
pub extern "C" fn js_native_abi_check_buffer_data_ptr(value: f64) -> *const u8 {
    buffer_data(strict_buffer_from_value(value))
}

/// Validate and lower the byte-length half of a manifest `buffer+len` span.
#[no_mangle]
pub extern "C" fn js_native_abi_check_buffer_byte_len(value: f64) -> usize {
    let buffer = strict_buffer_from_value(value);
    unsafe { (*buffer).length as usize }
}

/// Validate and unwrap a manifest `promise` parameter.
#[no_mangle]
pub extern "C" fn js_native_abi_check_promise(value: f64) -> i64 {
    if crate::promise::js_value_is_promise(value) == 0 {
        throw_type_error("Expected Promise for native promise parameter");
    }
    let ptr = JSValue::from_bits(value.to_bits()).as_pointer::<Promise>();
    ptr as i64
}

/// Validate a manifest `pod` fallback object and return its ObjectHeader pointer.
#[no_mangle]
pub extern "C" fn js_native_abi_check_pod_object(value: f64) -> i64 {
    let js_value = JSValue::from_bits(value.to_bits());
    if !js_value.is_pointer() {
        throw_type_error("Expected object for native pod parameter");
    }
    let obj = js_value.as_pointer::<ObjectHeader>();
    if obj.is_null() || (obj as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        throw_type_error("Expected object for native pod parameter");
    }
    unsafe {
        let gc_header =
            (obj as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        let is_gc_object = (*gc_header).obj_type == crate::gc::GC_TYPE_OBJECT;
        let is_regular = (*obj).object_type == crate::error::OBJECT_TYPE_REGULAR;
        if !is_gc_object || !is_regular {
            throw_type_error("Expected object for native pod parameter");
        }
    }
    obj as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::raw::c_int;

    fn catch_runtime_throw(f: impl FnOnce()) -> bool {
        let env = crate::exception::js_try_push();
        let jumped = unsafe { crate::ffi::setjmp::setjmp(env as *mut c_int) };
        if jumped == 0 {
            f();
            crate::exception::js_try_end();
            false
        } else {
            crate::exception::js_try_end();
            crate::exception::js_clear_exception();
            true
        }
    }

    fn boxed_ptr<T>(ptr: *const T) -> f64 {
        crate::value::js_nanbox_pointer(ptr as i64)
    }

    #[test]
    fn scalar_guards_reject_incompatible_js_values() {
        assert_eq!(js_native_abi_check_i32(12.0), 12);
        assert_eq!(js_native_abi_check_u32(4_000_000_000.0), 4_000_000_000);
        assert_eq!(js_native_abi_check_f32(6.25), 6.25f32);

        assert!(catch_runtime_throw(|| {
            js_native_abi_check_i32(1.5);
        }));
        assert!(catch_runtime_throw(|| {
            js_native_abi_check_u32(-1.0);
        }));
        assert!(catch_runtime_throw(|| {
            js_native_abi_check_i64(MAX_SAFE_INTEGER + 2.0);
        }));
        assert!(catch_runtime_throw(|| {
            let s = crate::string::js_string_from_bytes(b"no".as_ptr(), 2);
            js_native_abi_check_f64(f64::from_bits(JSValue::string_ptr(s).bits()));
        }));
    }

    #[test]
    fn string_guard_requires_actual_js_string() {
        let s = crate::string::js_string_from_bytes(b"ok".as_ptr(), 2);
        let boxed = f64::from_bits(JSValue::string_ptr(s).bits());
        assert_eq!(js_native_abi_check_string_ptr(boxed), s as i64);

        let short = f64::from_bits(JSValue::try_short_string(b"id").unwrap().bits());
        assert_ne!(js_native_abi_check_string_ptr(short), 0);

        assert!(catch_runtime_throw(|| {
            js_native_abi_check_string_ptr(42.0);
        }));
    }

    #[test]
    fn buffer_span_guards_require_registered_buffer() {
        let buf = crate::buffer::js_buffer_alloc(3, 0);
        let boxed = boxed_ptr(buf);
        assert_eq!(
            js_native_abi_check_buffer_data_ptr(boxed),
            crate::buffer::buffer_data(buf)
        );
        assert_eq!(js_native_abi_check_buffer_byte_len(boxed), 3);

        let object = crate::object::js_object_alloc(0, 0);
        assert!(catch_runtime_throw(|| {
            js_native_abi_check_buffer_data_ptr(boxed_ptr(object));
        }));
        assert!(catch_runtime_throw(|| {
            js_native_abi_check_buffer_byte_len(42.0);
        }));
    }

    #[test]
    fn promise_guard_rejects_non_promises() {
        let promise = crate::promise::js_promise_new();
        let boxed = boxed_ptr(promise);
        assert_eq!(js_native_abi_check_promise(boxed), promise as i64);

        let object = crate::object::js_object_alloc(0, 0);
        assert!(catch_runtime_throw(|| {
            js_native_abi_check_promise(boxed_ptr(object));
        }));
        assert!(catch_runtime_throw(|| {
            js_native_abi_check_promise(0.0);
        }));
    }

    #[test]
    fn pod_object_guard_rejects_non_objects() {
        let object = crate::object::js_object_alloc(0, 1);
        let boxed = boxed_ptr(object);
        assert_eq!(js_native_abi_check_pod_object(boxed), object as i64);

        let buffer = crate::buffer::js_buffer_alloc(3, 0);
        assert!(catch_runtime_throw(|| {
            js_native_abi_check_pod_object(boxed_ptr(buffer));
        }));
        assert!(catch_runtime_throw(|| {
            js_native_abi_check_pod_object(42.0);
        }));
    }
}
