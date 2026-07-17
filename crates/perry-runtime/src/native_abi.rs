//! Strict runtime validators for manifest-declared native-library ABI calls.
//!
//! These helpers are intentionally narrower than the legacy conversion helpers
//! used by the rest of the runtime. Manifest lowering calls them before handing
//! raw scalars, pointers, buffer spans, strings, or promises to native code.

use crate::buffer::{is_registered_buffer, resolve_span_data_ptr, BufferHeader};
use crate::object::ObjectHeader;
use crate::promise::Promise;
use crate::value::{JSValue, POINTER_MASK, TAG_FALSE, TAG_TRUE};

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

/// Guard for internal typed-f64 Perry function clones.
///
/// Unlike `js_native_abi_check_f64`, this does not throw. A failed guard means
/// codegen must call the generic JSValue wrapper instead of the typed clone.
#[no_mangle]
pub extern "C" fn js_typed_f64_arg_guard(value: f64) -> i32 {
    let js_value = JSValue::from_bits(value.to_bits());
    (js_value.is_number() || js_value.is_int32()) as i32
}

/// Convert an already-guarded JS number/int32 argument to the raw f64 ABI used
/// by internal typed-f64 clones.
#[no_mangle]
pub extern "C" fn js_typed_f64_arg_to_raw(value: f64) -> f64 {
    crate::builtins::js_number_coerce(value)
}

/// Guard for internal typed-i32 Perry function clones.
///
/// This is intentionally non-throwing. Tagged JS int32 values are accepted
/// directly; plain JS numbers are accepted only when they are finite, integral,
/// and in the signed 32-bit range. Everything else must use the generic
/// JSValue body.
#[no_mangle]
pub extern "C" fn js_typed_i32_arg_guard(value: f64) -> i32 {
    let js_value = JSValue::from_bits(value.to_bits());
    if js_value.is_int32() {
        return 1;
    }
    if !js_value.is_number() {
        return 0;
    }
    let number = js_value.as_number();
    (number.is_finite()
        && number.fract() == 0.0
        && number >= i32::MIN as f64
        && number <= i32::MAX as f64) as i32
}

/// Convert an already-guarded JS number/int32 argument to the raw i32 ABI used
/// by internal typed-i32 parameter slots.
#[no_mangle]
pub extern "C" fn js_typed_i32_arg_to_raw(value: f64) -> i32 {
    let js_value = JSValue::from_bits(value.to_bits());
    if js_value.is_int32() {
        js_value.as_int32()
    } else {
        js_value.as_number() as i32
    }
}

/// Guard for internal typed-i1 Perry function clones.
///
/// This deliberately accepts only the exact JS boolean singleton tags. Truthy
/// numbers, strings, objects, null, and undefined must fall back to the generic
/// JSValue function body.
#[no_mangle]
pub extern "C" fn js_typed_i1_arg_guard(value: f64) -> i32 {
    matches!(value.to_bits(), TAG_TRUE | TAG_FALSE) as i32
}

/// Convert an already-guarded JS boolean argument to an integer bit. Codegen
/// narrows this to LLVM `i1` before calling the typed-i1 clone.
#[no_mangle]
pub extern "C" fn js_typed_i1_arg_to_raw(value: f64) -> i32 {
    (value.to_bits() == TAG_TRUE) as i32
}

/// Guard for internal typed-string Perry function clones.
///
/// This is intentionally narrower than `js_get_string_pointer_unified`: it
/// accepts only actual heap/short JS strings and must not perform property-key
/// coercions such as number-to-string. Failed guards route to the generic
/// JSValue body.
#[no_mangle]
pub extern "C" fn js_typed_string_arg_guard(value: f64) -> i32 {
    let js_value = JSValue::from_bits(value.to_bits());
    (js_value.is_string() || js_value.is_short_string()) as i32
}

/// Convert an already-guarded JS string argument to the raw `StringHeader*`
/// ABI used by internal typed-string clones.
#[no_mangle]
pub extern "C" fn js_typed_string_arg_to_raw(value: f64) -> i64 {
    crate::value::js_get_string_pointer_unified(value)
}

// Codegen calls these helpers from generated LLVM IR when it selects an
// internal typed clone. They have no Rust call sites, so keep explicit
// function-pointer references to prevent whole-program LTO/dead-strip from
// removing the exported symbols.
#[used]
static KEEP_JS_TYPED_F64_ARG_GUARD: extern "C" fn(f64) -> i32 = js_typed_f64_arg_guard;
#[used]
static KEEP_JS_TYPED_F64_ARG_TO_RAW: extern "C" fn(f64) -> f64 = js_typed_f64_arg_to_raw;
#[used]
static KEEP_JS_TYPED_I32_ARG_GUARD: extern "C" fn(f64) -> i32 = js_typed_i32_arg_guard;
#[used]
static KEEP_JS_TYPED_I32_ARG_TO_RAW: extern "C" fn(f64) -> i32 = js_typed_i32_arg_to_raw;
#[used]
static KEEP_JS_TYPED_I1_ARG_GUARD: extern "C" fn(f64) -> i32 = js_typed_i1_arg_guard;
#[used]
static KEEP_JS_TYPED_I1_ARG_TO_RAW: extern "C" fn(f64) -> i32 = js_typed_i1_arg_to_raw;
#[used]
static KEEP_JS_TYPED_STRING_ARG_GUARD: extern "C" fn(f64) -> i32 = js_typed_string_arg_guard;
#[used]
static KEEP_JS_TYPED_STRING_ARG_TO_RAW: extern "C" fn(f64) -> i64 = js_typed_string_arg_to_raw;

// Static-name and static-method lowering emits these by-id wrappers directly
// from generated LLVM IR. Keep roots here so LTO cannot strip the symbols just
// because the Rust crate graph has no ordinary caller.
#[used]
static KEEP_JS_OBJECT_GET_FIELD_BY_PROPERTY_ID_F64: extern "C" fn(*const ObjectHeader, i64) -> f64 =
    crate::object::js_object_get_field_by_property_id_f64;
#[used]
static KEEP_JS_OBJECT_SET_FIELD_BY_PROPERTY_ID: extern "C" fn(*mut ObjectHeader, i64, f64) =
    crate::object::js_object_set_field_by_property_id;
#[used]
static KEEP_JS_NATIVE_CALL_METHOD_BY_ID: unsafe extern "C" fn(f64, i64, *const f64, usize) -> f64 =
    crate::object::js_native_call_method_by_id;
#[used]
static KEEP_JS_NATIVE_CALL_METHOD_APPLY_BY_ID: unsafe extern "C" fn(f64, i64, i64) -> f64 =
    crate::object::js_native_call_method_apply_by_id;

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
///
/// Resolves a registered view (`new Uint8Array(arrayBuffer)`, `slice`,
/// `subarray`) to `backing_data + byteOffset` — the same storage JS reads and
/// writes go through — so a native callee that writes `len` bytes touches the
/// bytes the script observes, not the view's stale local copy (#6515).
#[no_mangle]
pub extern "C" fn js_native_abi_check_buffer_data_ptr(value: f64) -> *const u8 {
    unsafe { resolve_span_data_ptr(strict_buffer_from_value(value)) }
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
    fn typed_f64_arg_guard_is_non_throwing_and_numeric_only() {
        assert_eq!(js_typed_f64_arg_guard(12.5), 1);
        assert_eq!(js_typed_f64_arg_to_raw(12.5), 12.5);

        let int32 = f64::from_bits(crate::value::JSValue::int32(-7).bits());
        assert_eq!(js_typed_f64_arg_guard(int32), 1);
        assert_eq!(js_typed_f64_arg_to_raw(int32), -7.0);

        let s = crate::string::js_string_from_bytes(b"no".as_ptr(), 2);
        let string = f64::from_bits(JSValue::string_ptr(s).bits());
        assert_eq!(js_typed_f64_arg_guard(string), 0);
    }

    #[test]
    fn typed_i32_arg_guard_is_non_throwing_and_int32_only() {
        let tagged = f64::from_bits(crate::value::JSValue::int32(-7).bits());
        assert_eq!(js_typed_i32_arg_guard(tagged), 1);
        assert_eq!(js_typed_i32_arg_to_raw(tagged), -7);

        assert_eq!(js_typed_i32_arg_guard(12.0), 1);
        assert_eq!(js_typed_i32_arg_to_raw(12.0), 12);
        assert_eq!(js_typed_i32_arg_guard(12.5), 0);
        assert_eq!(js_typed_i32_arg_guard(f64::NAN), 0);
        assert_eq!(js_typed_i32_arg_guard(i32::MAX as f64 + 1.0), 0);
        assert_eq!(js_typed_i32_arg_guard(i32::MIN as f64 - 1.0), 0);
        assert_eq!(js_typed_i32_arg_guard(f64::from_bits(TAG_TRUE)), 0);

        let s = crate::string::js_string_from_bytes(b"no".as_ptr(), 2);
        let string = f64::from_bits(JSValue::string_ptr(s).bits());
        assert_eq!(js_typed_i32_arg_guard(string), 0);
    }

    #[test]
    fn typed_i1_arg_guard_is_non_throwing_and_boolean_only() {
        let t = f64::from_bits(TAG_TRUE);
        let f = f64::from_bits(TAG_FALSE);
        assert_eq!(js_typed_i1_arg_guard(t), 1);
        assert_eq!(js_typed_i1_arg_to_raw(t), 1);
        assert_eq!(js_typed_i1_arg_guard(f), 1);
        assert_eq!(js_typed_i1_arg_to_raw(f), 0);

        assert_eq!(js_typed_i1_arg_guard(1.0), 0);
        assert_eq!(
            js_typed_i1_arg_guard(f64::from_bits(JSValue::int32(1).bits())),
            0
        );
        let s = crate::string::js_string_from_bytes(b"yes".as_ptr(), 3);
        let string = f64::from_bits(JSValue::string_ptr(s).bits());
        assert_eq!(js_typed_i1_arg_guard(string), 0);
    }

    #[test]
    fn typed_string_arg_guard_is_non_throwing_and_string_only() {
        let heap = crate::string::js_string_from_bytes(b"heap".as_ptr(), 4);
        let heap_boxed = f64::from_bits(JSValue::string_ptr(heap).bits());
        assert_eq!(js_typed_string_arg_guard(heap_boxed), 1);
        assert_eq!(js_typed_string_arg_to_raw(heap_boxed), heap as i64);

        let short = f64::from_bits(JSValue::try_short_string(b"id").unwrap().bits());
        assert_eq!(js_typed_string_arg_guard(short), 1);
        assert_ne!(js_typed_string_arg_to_raw(short), 0);

        assert_eq!(js_typed_string_arg_guard(42.0), 0);
        assert_eq!(
            js_typed_string_arg_guard(f64::from_bits(JSValue::int32(7).bits())),
            0
        );
        assert_eq!(js_typed_string_arg_guard(f64::from_bits(TAG_TRUE)), 0);
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

    // #6515: a `Uint8Array` view over an `ArrayBuffer` is a registered view
    // whose own storage is a copy; the backing ArrayBuffer is what JS reads
    // and writes go through. The `buffer+len` data pointer must resolve to the
    // backing window, otherwise a native write lands in the view's stale copy
    // and the script never observes it (silent corruption).
    #[test]
    fn buffer_data_ptr_resolves_view_over_arraybuffer_to_backing() {
        let ab = crate::buffer::js_array_buffer_new(64);
        let view = crate::buffer::js_uint8array_new(boxed_ptr(ab));
        assert_ne!(view as usize, ab as usize, "view must be a distinct header");

        // Resolves to the backing's storage — NOT the view's own local copy.
        let resolved = js_native_abi_check_buffer_data_ptr(boxed_ptr(view));
        assert_eq!(resolved, crate::buffer::buffer_data(ab));
        assert_ne!(resolved, crate::buffer::buffer_data(view));
        assert_eq!(js_native_abi_check_buffer_byte_len(boxed_ptr(view)), 64);

        // End-to-end: a native write through the resolved pointer is observed
        // by a JS-value index read of the view. Before the fix this read 0 —
        // the byte landed in the view's copy while the read came from `ab`.
        unsafe {
            *(resolved as *mut u8) = 0xAB;
        }
        assert_eq!(
            crate::buffer::js_buffer_index_get_value(view, 0),
            0xAB as f64
        );

        // Offset views (`new Uint8Array(ab, 16, 8)`) resolve to
        // `backing_data + byteOffset`, and the write is visible at the
        // absolute backing offset too.
        let with_offset = crate::buffer::js_uint8array_view(boxed_ptr(ab), 16.0, 8.0);
        let resolved_off = js_native_abi_check_buffer_data_ptr(boxed_ptr(with_offset));
        assert_eq!(resolved_off, unsafe {
            crate::buffer::buffer_data(ab).add(16)
        });
        assert_eq!(
            js_native_abi_check_buffer_byte_len(boxed_ptr(with_offset)),
            8
        );
        unsafe {
            *(resolved_off as *mut u8) = 0xCD;
        }
        assert_eq!(
            crate::buffer::js_buffer_index_get_value(with_offset, 0),
            0xCD as f64
        );
        assert_eq!(
            crate::buffer::js_buffer_index_get_value(ab, 16),
            0xCD as f64
        );

        // A standalone `new Uint8Array(64)` is not a view: it keeps resolving
        // to its own storage (the working case in the report).
        let standalone = crate::buffer::js_uint8array_alloc(64);
        assert_eq!(
            js_native_abi_check_buffer_data_ptr(boxed_ptr(standalone)),
            crate::buffer::buffer_data(standalone)
        );

        // Hardening: if the backing shrinks after the view is registered so the
        // recorded window no longer fits, resolution falls back to the view's
        // own (correctly-sized) storage rather than hand back a backing pointer
        // that a `len`-byte native write would overrun.
        unsafe {
            (*ab).length = 8;
        }
        assert_eq!(
            js_native_abi_check_buffer_data_ptr(boxed_ptr(view)),
            crate::buffer::buffer_data(view)
        );
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
