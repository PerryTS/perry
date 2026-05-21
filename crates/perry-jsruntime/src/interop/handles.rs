//! Handle-based FFIs: array index/length lookup, object property
//! get/set, `to_string`, and the `typeof` discriminator probe.

use super::*;

use deno_core::v8;
use std::ffi::{c_char, CStr};

/// Probe a V8 handle's `typeof` discriminator. Returns 1 for callables (functions),
/// 0 for everything else. Wired into `js_value_typeof` so user-visible `typeof gp`
/// returns `"function"` when `gp` is a V8 callable handle. (Issue #258.)
pub(crate) unsafe extern "C" fn js_handle_typeof(value: f64) -> i32 {
    bump_v8_entry(V8EntryKind::TypeofProbe);
    with_runtime(|state| {
        deno_core::scope!(scope, &mut state.runtime);
        let v = native_to_v8(scope, value);
        if v.is_function() {
            1
        } else {
            0
        }
    })
}

/// Get an element from a JavaScript array by index
/// array_handle: NaN-boxed value containing a JS handle to an array
/// index: The array index
/// Returns the element value as a NaN-boxed f64
#[no_mangle]
pub extern "C" fn js_handle_array_get(array_handle: f64, index: i32) -> f64 {
    bump_v8_entry(V8EntryKind::ArrayGet);
    with_runtime(|state| {
        deno_core::scope!(scope, &mut state.runtime);

        // Convert the handle to a V8 value
        let arr_val = native_to_v8(scope, array_handle);

        // Use Object::get_index which works for both arrays and array-like objects
        // (e.g., ethers.js Result extends Array but V8 is_array() returns false)
        if arr_val.is_object() {
            let obj = v8::Local::<v8::Object>::try_from(arr_val).unwrap();
            let elem = match obj.get_index(scope, index as u32) {
                Some(v) => v,
                None => return f64::from_bits(0x7FFC_0000_0000_0001),
            };
            return v8_to_native(scope, elem);
        }

        // Fallback for non-objects
        f64::from_bits(0x7FFC_0000_0000_0001) // undefined
    })
}

/// Get the length of a JavaScript array
/// array_handle: NaN-boxed value containing a JS handle to an array
/// Returns the length as i32
#[no_mangle]
pub extern "C" fn js_handle_array_length(array_handle: f64) -> i32 {
    bump_v8_entry(V8EntryKind::ArrayLength);
    with_runtime(|state| {
        deno_core::scope!(scope, &mut state.runtime);

        // Convert the handle to a V8 value
        let arr_val = native_to_v8(scope, array_handle);

        // For actual arrays, use Array::length()
        if arr_val.is_array() {
            let arr = v8::Local::<v8::Array>::try_from(arr_val).unwrap();
            return arr.length() as i32;
        }

        // For array-like objects (e.g., ethers.js Result), get the "length" property
        if arr_val.is_object() {
            let obj = v8::Local::<v8::Object>::try_from(arr_val).unwrap();
            let key = v8::String::new(scope, "length").unwrap();
            if let Some(length_val) = obj.get(scope, key.into()) {
                if length_val.is_number() {
                    return length_val.number_value(scope).unwrap_or(0.0) as i32;
                }
            }
        }

        0
    })
}

/// Get a property from a JavaScript object (for JS handle objects)
/// This is called by js_dynamic_object_get_property in perry-runtime when a JS handle is detected
/// object_ptr: NaN-boxed value containing a JS handle
/// Returns the property value as a NaN-boxed f64
#[no_mangle]
pub extern "C" fn js_handle_object_get_property(
    object_ptr: f64,
    property_name_ptr: *const i8,
    property_name_len: usize,
) -> f64 {
    bump_v8_entry(V8EntryKind::ObjectPropertyGet);
    let name_slice = if property_name_ptr.is_null() {
        return f64::from_bits(0x7FFC_0000_0000_0001); // undefined
    } else if property_name_len > 0 {
        unsafe { std::slice::from_raw_parts(property_name_ptr as *const u8, property_name_len) }
    } else {
        unsafe { CStr::from_ptr(property_name_ptr as *const c_char).to_bytes() }
    };

    let property_name = match std::str::from_utf8(name_slice) {
        Ok(s) => s,
        Err(_) => return f64::from_bits(0x7FFC_0000_0000_0001),
    };

    // Issue #255: when called from inside a V8 callback trampoline,
    // reuse the trampoline's scope rather than creating a new one via
    // `state.runtime.handle_scope()`. The latter clashes with V8's
    // scope-stack tracking under deno_core (panics with "active scope
    // can't be dropped" when the inner scope drops). The trampoline
    // stashes its scope ptr in REENTRY_SCOPE_PTR; this branch picks
    // it up. Outside a callback, fall through to the normal path.
    if let Some(scope) = unsafe { crate::try_trampoline_scope() } {
        return get_property_with_scope(scope, object_ptr, property_name);
    }

    with_runtime(|state| {
        deno_core::scope!(scope, &mut state.runtime);
        get_property_with_scope(scope, object_ptr, property_name)
    })
}

/// Shared body of `js_handle_object_get_property` parameterized over the
/// V8 scope to use — extracted so both the normal path (creates a scope
/// from the runtime) and the trampoline-reuse path (issue #255) share
/// the same logic.
fn get_property_with_scope(
    scope: &mut v8::PinScope<'_, '_>,
    object_ptr: f64,
    property_name: &str,
) -> f64 {
    let obj_val = native_to_v8(scope, object_ptr);
    if !obj_val.is_object() {
        eprintln!("[js_handle_object_get_property] value is not an object!");
        return f64::from_bits(0x7FFC_0000_0000_0001);
    }

    let obj = obj_val.to_object(scope).unwrap();

    let key = match v8::String::new(scope, property_name) {
        Some(k) => k,
        None => return f64::from_bits(0x7FFC_0000_0000_0001),
    };

    let prop_val = match obj.get(scope, key.into()) {
        Some(v) => v,
        None => return f64::from_bits(0x7FFC_0000_0000_0001),
    };

    v8_to_native(scope, prop_val)
}

/// Convert a JavaScript handle value to a native string
/// handle: NaN-boxed value containing a JS handle
/// Returns a pointer to a native StringHeader
#[no_mangle]
pub extern "C" fn js_handle_to_string(handle: f64) -> *mut perry_runtime::string::StringHeader {
    bump_v8_entry(V8EntryKind::HandleToString);
    with_runtime(|state| {
        deno_core::scope!(scope, &mut state.runtime);

        // Convert the handle to a V8 value
        let v8_val = native_to_v8(scope, handle);

        // Convert to string
        let str_val = match v8_val.to_string(scope) {
            Some(s) => s,
            None => {
                // Return empty string on failure
                return perry_runtime::string::js_string_from_bytes(b"".as_ptr(), 0);
            }
        };

        // Get the UTF-8 bytes
        let len = str_val.utf8_length(scope);
        let mut buffer = vec![0u8; len];
        str_val.write_utf8_v2(scope, &mut buffer, v8::WriteFlags::empty(), None);

        // Create a native string
        perry_runtime::string::js_string_from_bytes(buffer.as_ptr(), buffer.len() as u32)
    })
}

/// Set a property on a JavaScript object
/// object_ptr: NaN-boxed value containing a JS handle
/// value: NaN-boxed value to set
#[no_mangle]
pub unsafe extern "C" fn js_set_property(
    object_ptr: f64,
    property_name_ptr: *const i8,
    property_name_len: usize,
    value: f64,
) {
    bump_v8_entry(V8EntryKind::PropertySet);
    let name_slice = if property_name_ptr.is_null() {
        return;
    } else if property_name_len > 0 {
        std::slice::from_raw_parts(property_name_ptr as *const u8, property_name_len)
    } else {
        CStr::from_ptr(property_name_ptr as *const c_char).to_bytes()
    };

    let property_name = match std::str::from_utf8(name_slice) {
        Ok(s) => s,
        Err(_) => return,
    };

    with_runtime(|state| {
        deno_core::scope!(scope, &mut state.runtime);

        // Convert the object pointer to a V8 object
        let obj_val = native_to_v8(scope, object_ptr);
        if !obj_val.is_object() {
            log::error!("Value is not an object");
            return;
        }

        let obj = obj_val.to_object(scope).unwrap();

        // Set the property
        let key = match v8::String::new(scope, property_name) {
            Some(k) => k,
            None => return,
        };

        let v8_value = native_to_v8(scope, value);
        obj.set(scope, key.into(), v8_value);
    })
}
