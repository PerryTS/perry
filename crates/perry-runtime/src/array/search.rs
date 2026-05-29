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

/// If `arr` is actually a TypedArray, return `(typed_header, length)`. Their
/// elements are raw numbers in the typed backing store (not NaN-boxed
/// `JSValue`s), so the generic element-walk below would feed garbage bit
/// patterns to the comparison — read them via `js_typed_array_get` instead.
#[inline]
fn as_typed_array(
    arr: *const ArrayHeader,
) -> Option<(*const crate::typedarray::TypedArrayHeader, i32)> {
    if crate::typedarray::lookup_typed_array_kind(arr as usize).is_some() {
        let ta = arr as *const crate::typedarray::TypedArrayHeader;
        Some((ta, crate::typedarray::js_typed_array_length(ta)))
    } else {
        None
    }
}

/// indexOf for arrays, using jsvalue comparison (handles NaN-boxed strings correctly)
#[no_mangle]
pub extern "C" fn js_array_indexOf_jsvalue(arr: *const ArrayHeader, value: f64) -> i32 {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return -1;
    }
    // TypedArray: strict-equality numeric search over the typed store.
    if let Some((ta, len)) = as_typed_array(arr) {
        for i in 0..len {
            if crate::typedarray::js_typed_array_get(ta, i) == value {
                return i;
            }
        }
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

/// `Array.prototype.lastIndexOf` (ECMA-262 §23.1.3.20): search backward for
/// `value`, returning the highest matching index or -1. `has_from == 0` means
/// no `fromIndex` argument (default: `length - 1`); otherwise `from_index` is
/// the caller's `fromIndex` with the spec's clamping. Uses `jsvalue` equality
/// so SSO/heap string elements compare by content (mirrors `indexOf`).
#[no_mangle]
pub extern "C" fn js_array_last_index_of_jsvalue(
    arr: *const ArrayHeader,
    value: f64,
    from_index: f64,
    has_from: i32,
) -> i32 {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return -1;
    }
    // NB: TypedArray `lastIndexOf` does NOT reach here — the HIR lowers it to
    // the string `lastIndexOf` path (`js_string_last_index_of`), so a typed
    // branch would be dead code. Tracked as a separate codegen-routing fix.
    unsafe {
        let length = (*arr).length as i64;
        if length == 0 {
            return -1;
        }
        let elements_ptr = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;

        // Determine the start index. Without an explicit fromIndex, start at
        // the last element. With one, apply ToIntegerOrInfinity + clamping
        // while avoiding i64 overflow for ±Infinity / out-of-range values.
        let start: i64 = if has_from == 0 {
            length - 1
        } else {
            let n = if from_index.is_nan() {
                0.0
            } else {
                from_index.trunc()
            };
            if n >= length as f64 {
                length - 1
            } else if n >= 0.0 {
                n as i64
            } else if n >= -(length as f64) {
                length + (n as i64) // n negative: count from the end
            } else {
                return -1; // fromIndex < -length: nothing to search
            }
        };

        let mut i = start;
        while i >= 0 {
            let element = *elements_ptr.add(i as usize);
            if crate::value::js_jsvalue_equals(element, value) == 1 {
                return i as i32;
            }
            i -= 1;
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
    // TypedArray: SameValueZero numeric search (so includes(NaN) is true for
    // float typed arrays).
    if let Some((ta, len)) = as_typed_array(arr) {
        for i in 0..len {
            let e = crate::typedarray::js_typed_array_get(ta, i);
            if crate::value::js_jsvalue_same_value_zero(e, value) == 1 {
                return 1;
            }
        }
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

#[cfg(test)]
mod typed_search_tests {
    use super::*;
    use crate::typedarray::{
        js_typed_array_new_empty, js_typed_array_set, KIND_FLOAT64, KIND_INT32,
    };

    /// `indexOf` / `includes` on a registered TypedArray must read the typed
    /// backing store (via `js_typed_array_get`) rather than reinterpreting the
    /// raw element bytes as NaN-boxed `JSValue`s — the latter returned garbage
    /// (-1 / false) for every TypedArray search.
    #[test]
    fn typed_array_indexof_includes() {
        // Int32Array([1, 2, 3, 2, 1])
        let ta = js_typed_array_new_empty(KIND_INT32 as i32, 5);
        for (i, v) in [1.0, 2.0, 3.0, 2.0, 1.0].iter().enumerate() {
            js_typed_array_set(ta, i as i32, *v);
        }
        let arr = ta as *const ArrayHeader;
        assert_eq!(js_array_indexOf_jsvalue(arr, 2.0), 1);
        assert_eq!(js_array_indexOf_jsvalue(arr, 3.0), 2);
        assert_eq!(js_array_indexOf_jsvalue(arr, 9.0), -1);
        assert_eq!(js_array_includes_jsvalue(arr, 3.0), 1);
        assert_eq!(js_array_includes_jsvalue(arr, 9.0), 0);
        // indexOf uses strict equality → NaN never matches.
        assert_eq!(js_array_indexOf_jsvalue(arr, f64::NAN), -1);
    }

    /// SameValueZero: `includes(NaN)` is true for a Float64Array holding NaN,
    /// while `indexOf(NaN)` stays -1.
    #[test]
    fn typed_array_includes_nan() {
        let ta = js_typed_array_new_empty(KIND_FLOAT64 as i32, 3);
        js_typed_array_set(ta, 0, 1.5);
        js_typed_array_set(ta, 1, f64::NAN);
        js_typed_array_set(ta, 2, 2.5);
        let arr = ta as *const ArrayHeader;
        assert_eq!(js_array_includes_jsvalue(arr, f64::NAN), 1);
        assert_eq!(js_array_indexOf_jsvalue(arr, f64::NAN), -1);
        assert_eq!(js_array_indexOf_jsvalue(arr, 2.5), 2);
    }
}
