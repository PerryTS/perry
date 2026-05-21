//! JSValue-typed convenience wrappers.
use super::*;
use crate::value::JSValue;
use std::ptr;

/// Set an element using JSValue
#[no_mangle]
pub extern "C" fn js_array_set(arr: *mut ArrayHeader, index: u32, value: JSValue) {
    // Convert JSValue bits to f64 for storage
    let bits_as_f64 = f64::from_bits(value.bits());
    js_array_set_f64(arr, index, bits_as_f64);
}

/// Get an element as JSValue
#[no_mangle]
pub extern "C" fn js_array_get(arr: *const ArrayHeader, index: u32) -> JSValue {
    let bits_as_f64 = js_array_get_f64(arr, index);
    JSValue::from_bits(bits_as_f64.to_bits())
}

/// Push a JSValue to the array
#[no_mangle]
pub extern "C" fn js_array_push(arr: *mut ArrayHeader, value: JSValue) -> *mut ArrayHeader {
    let bits_as_f64 = f64::from_bits(value.bits());
    js_array_push_f64(arr, bits_as_f64)
}

/// Allocate and initialize an array from a list of JSValue (stored as u64 bits)
/// This is used for mixed-type arrays where elements can be numbers, strings, objects, etc.
#[no_mangle]
pub extern "C" fn js_array_from_jsvalue(elements: *const u64, count: u32) -> *mut ArrayHeader {
    let arr = js_array_alloc(count);
    unsafe {
        (*arr).length = count;
        let arr_elements = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
        // Each u64 contains NaN-boxed JSValue bits, store as f64 bits
        for i in 0..count as usize {
            let bits = *elements.add(i);
            ptr::write(arr_elements.add(i), f64::from_bits(bits));
        }
        rebuild_array_layout(arr);
    }
    arr
}

/// Get an element from a mixed-type array (returns raw u64 bits for JSValue)
#[no_mangle]
pub extern "C" fn js_array_get_jsvalue(arr: *const ArrayHeader, index: u32) -> u64 {
    let bits_as_f64 = js_array_get_f64(arr, index);
    bits_as_f64.to_bits()
}

/// Set an element in a mixed-type array (value is raw u64 bits for JSValue)
#[no_mangle]
pub extern "C" fn js_array_set_jsvalue(arr: *mut ArrayHeader, index: u32, value: u64) {
    let bits_as_f64 = f64::from_bits(value);
    js_array_set_f64(arr, index, bits_as_f64);
}

/// Set an element in a mixed-type array, extending the array if needed.
/// Returns the (possibly reallocated) array pointer.
#[no_mangle]
pub extern "C" fn js_array_set_jsvalue_extend(
    arr: *mut ArrayHeader,
    index: u32,
    value: u64,
) -> *mut ArrayHeader {
    let bits_as_f64 = f64::from_bits(value);
    js_array_set_f64_extend(arr, index, bits_as_f64)
}

/// Push a JSValue (as u64 bits) to a mixed-type array
#[no_mangle]
pub extern "C" fn js_array_push_jsvalue(arr: *mut ArrayHeader, value: u64) -> *mut ArrayHeader {
    let bits_as_f64 = f64::from_bits(value);
    js_array_push_f64(arr, bits_as_f64)
}
