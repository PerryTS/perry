//! Type-erased array dispatchers (get / length / find / findIndex) that
//! choose between JS-handle arrays, registry-backed buffers, and the
//! native `ArrayHeader` at runtime.

use super::*;
use std::sync::atomic::Ordering;

/// Unified index access that handles strings, arrays, and JS handles.
/// This is called from compiled code when the value type is not known at compile time.
/// For strings, returns the character at the given index as a NaN-boxed string.
/// For arrays, returns the element at the given index.
#[no_mangle]
pub extern "C" fn js_dynamic_array_get(value: f64, index: i32) -> f64 {
    let bits = value.to_bits();
    let jsval = JSValue::from_bits(bits);

    // Check if this is a NaN-boxed string
    if jsval.is_string() {
        // String character access
        let str_ptr = jsval.as_string_ptr();
        if !str_ptr.is_null() && index >= 0 {
            let result_ptr = crate::string::js_string_char_at(str_ptr, index);
            if !result_ptr.is_null() {
                // NaN-box the result string pointer
                return f64::from_bits(STRING_TAG | (result_ptr as u64 & POINTER_MASK));
            }
        }
        // Return empty string for invalid index
        let empty = crate::string::js_string_from_bytes(std::ptr::null(), 0);
        return f64::from_bits(STRING_TAG | (empty as u64 & POINTER_MASK));
    }

    // Check if this is a JS handle
    if is_js_handle(value) {
        // Try to use the JS runtime function if it's been registered
        let func_ptr = JS_HANDLE_ARRAY_GET.load(Ordering::SeqCst);
        if !func_ptr.is_null() {
            let func: JsHandleArrayGetFn = unsafe { std::mem::transmute(func_ptr) };
            return func(value, index);
        }
        // JS runtime not available - return undefined
        return f64::from_bits(TAG_UNDEFINED);
    }

    // Not a JS handle - it's a native array/buffer pointer
    let ptr = js_nanbox_get_pointer(value);
    if ptr == 0 {
        // Invalid pointer - return undefined
        return f64::from_bits(TAG_UNDEFINED);
    }

    // Check if this is a buffer (Uint8Array) - read individual bytes, not f64 values
    if crate::buffer::is_registered_buffer(ptr as usize) {
        let byte_val =
            crate::buffer::js_buffer_get(ptr as *const crate::buffer::BufferHeader, index);
        return byte_val as f64;
    }

    // Call the native array get function
    let result_bits =
        crate::array::js_array_get_jsvalue(ptr as *const crate::array::ArrayHeader, index as u32);
    let _result_top16 = result_bits >> 48;
    // debug: DYNAMIC-ARRAY-GET-DEBUG disabled
    f64::from_bits(result_bits)
}

/// Unified array length access that handles both JS handle arrays and native arrays.
#[no_mangle]
pub extern "C" fn js_dynamic_array_length(arr_value: f64) -> i32 {
    let bits = arr_value.to_bits();
    let _top16 = bits >> 48;

    // Check if this is a JS handle
    if is_js_handle(arr_value) {
        let func_ptr = JS_HANDLE_ARRAY_LENGTH.load(Ordering::SeqCst);
        if !func_ptr.is_null() {
            let func: JsHandleArrayLengthFn = unsafe { std::mem::transmute(func_ptr) };
            return func(arr_value);
        }
        return 0;
    }

    // Not a JS handle - extract the pointer
    let ptr = js_nanbox_get_pointer(arr_value);
    if ptr == 0 {
        return 0;
    }

    crate::array::js_array_length(ptr as *const crate::array::ArrayHeader) as i32
}

/// Dynamic array find that handles both JS handle arrays and native arrays.
/// Takes the array as f64 (may be NaN-boxed or JS handle) and a callback closure.
/// Returns the found element as f64, or NaN (undefined) if not found.
#[no_mangle]
pub extern "C" fn js_dynamic_array_find(
    arr_value: f64,
    callback: *const crate::closure::ClosureHeader,
) -> f64 {
    // Check if callback is null
    if callback.is_null() {
        return f64::NAN;
    }

    // Check if this is a JS handle array
    if is_js_handle(arr_value) {
        // For JS handle arrays, iterate using dynamic access
        let length = js_dynamic_array_length(arr_value);
        for i in 0..length {
            let element = js_dynamic_array_get(arr_value, i);
            let result = crate::closure::js_closure_call1(callback, element);
            // Proper truthy check: handles NaN-boxed booleans
            if js_is_truthy(result) != 0 {
                return element;
            }
        }
        // Not found - return undefined (NaN)
        return f64::NAN;
    }

    // Not a JS handle - extract the native array pointer
    let ptr = js_nanbox_get_pointer(arr_value);
    if ptr == 0 {
        return f64::NAN;
    }

    // Use the native array find
    crate::array::js_array_find(ptr as *const crate::array::ArrayHeader, callback)
}

/// Dynamic array findIndex that handles both JS handle arrays and native arrays.
/// Takes the array as f64 (may be NaN-boxed or JS handle) and a callback closure.
/// Returns the index as f64 (-1.0 if not found).
#[no_mangle]
#[allow(non_snake_case)]
pub extern "C" fn js_dynamic_array_findIndex(
    arr_value: f64,
    callback: *const crate::closure::ClosureHeader,
) -> f64 {
    // Check if this is a JS handle array
    if is_js_handle(arr_value) {
        // For JS handle arrays, iterate using dynamic access
        let length = js_dynamic_array_length(arr_value);
        for i in 0..length {
            let element = js_dynamic_array_get(arr_value, i);
            let result = crate::closure::js_closure_call1(callback, element);
            // Proper truthy check: handles NaN-boxed booleans
            if js_is_truthy(result) != 0 {
                return i as f64;
            }
        }
        // Not found
        return -1.0;
    }

    // Not a JS handle - extract the native array pointer
    let ptr = js_nanbox_get_pointer(arr_value);
    if ptr == 0 {
        return -1.0;
    }

    // Use the native array findIndex and convert to f64
    crate::array::js_array_findIndex(ptr as *const crate::array::ArrayHeader, callback) as f64
}
