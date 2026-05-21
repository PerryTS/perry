//! indexOf / includes — both f64 and JSValue variants.
use super::*;

#[no_mangle]
pub extern "C" fn js_array_indexOf_f64(arr: *const ArrayHeader, value: f64) -> i32 {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return -1;
    }
    unsafe {
        let length = (*arr).length;
        let elements_ptr = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;

        for i in 0..length as usize {
            if *elements_ptr.add(i) == value {
                return i as i32;
            }
        }
        -1
    }
}

/// indexOf for arrays, using jsvalue comparison (handles NaN-boxed strings correctly)
#[no_mangle]
pub extern "C" fn js_array_indexOf_jsvalue(arr: *const ArrayHeader, value: f64) -> i32 {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return -1;
    }
    unsafe {
        let length = (*arr).length;
        let elements_ptr = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
        for i in 0..length as usize {
            let element = *elements_ptr.add(i);
            if crate::value::js_jsvalue_equals(element, value) == 1 {
                return i as i32;
            }
        }
        -1
    }
}

/// Check if an array includes a value
/// Returns 1 if found, 0 if not
#[no_mangle]
pub extern "C" fn js_array_includes_f64(arr: *const ArrayHeader, value: f64) -> i32 {
    if js_array_indexOf_f64(arr, value) >= 0 {
        1
    } else {
        0
    }
}

/// Check if an array includes a value using deep equality comparison.
/// This handles NaN-boxed strings by comparing string contents.
/// Returns 1 if found, 0 if not.
#[no_mangle]
pub extern "C" fn js_array_includes_jsvalue(arr: *const ArrayHeader, value: f64) -> i32 {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return 0;
    }
    unsafe {
        let length = (*arr).length;
        let elements_ptr = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;

        // `Array.prototype.includes` uses SameValueZero (ECMA-262 §23.1.3.16),
        // which differs from === in one place: NaN equals NaN. Routing
        // through `js_jsvalue_same_value_zero` preserves the `indexOf(NaN) ===
        // -1` / `includes(NaN) === true` split.
        for i in 0..length as usize {
            let element = *elements_ptr.add(i);
            if crate::value::js_jsvalue_same_value_zero(element, value) == 1 {
                return 1;
            }
        }
        0
    }
}
