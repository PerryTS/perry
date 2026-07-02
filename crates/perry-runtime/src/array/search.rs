//! indexOf / includes — both f64 and JSValue variants.
use super::*;

#[inline(always)]
unsafe fn array_elements_ptr(arr: *const ArrayHeader) -> *const f64 {
    (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64
}

#[inline(always)]
fn undefined_value() -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

#[inline(always)]
unsafe fn present_array_element(elements_ptr: *const f64, index: usize) -> Option<f64> {
    let element = *elements_ptr.add(index);
    (element.to_bits() != crate::value::TAG_HOLE).then_some(element)
}

/// ECMA-262 ToIntegerOrInfinity applied to a possibly non-numeric `fromIndex`
/// argument. The codegen passes the raw (NaN-boxed) value through, so a string
/// / boolean / null / object `fromIndex` (`"3E0"`, `"0x0003"`, `true`, `null`,
/// `{valueOf}`) must be ToNumber-coerced first — the previous `from.is_nan()`
/// shortcut treated every NaN-boxed value as NaN→0. Returns the truncated
/// integer (or ±Infinity, whose `trunc()` is itself).
#[inline]
pub(crate) fn from_index_to_integer(from: f64) -> f64 {
    let n = crate::builtins::js_number_coerce(from);
    if n.is_nan() {
        0.0
    } else {
        n.trunc()
    }
}

#[inline(always)]
unsafe fn array_element_get_value(elements_ptr: *const f64, index: usize) -> f64 {
    let element = *elements_ptr.add(index);
    if element.to_bits() == crate::value::TAG_HOLE {
        undefined_value()
    } else {
        element
    }
}

#[no_mangle]
pub extern "C" fn js_array_indexOf_f64(arr: *const ArrayHeader, value: f64) -> i32 {
    let arr = normalize_array_receiver(arr);
    if arr.is_null() {
        return -1;
    }
    unsafe {
        let length = (*arr).length;
        let elements_ptr = array_elements_ptr(arr);

        for i in 0..length as usize {
            let Some(element) = present_array_element(elements_ptr, i) else {
                continue;
            };
            if element == value {
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

/// Resolve a forward-search `fromIndex` (ECMA-262 ToIntegerOrInfinity +
/// clamping) into the first index to inspect. Returns `None` when nothing can
/// match (e.g. `fromIndex >= length`, including `+Infinity`). `has_from == 0`
/// means no argument was supplied → start at 0. `NaN` coerces to 0; negative
/// values count from the end and clamp at 0.
#[inline]
fn forward_start_index(length: i64, from_index: f64, has_from: i32) -> Option<i64> {
    if has_from == 0 {
        return Some(0);
    }
    let n = from_index_to_integer(from_index);
    if n >= length as f64 {
        // Covers +Infinity and any fromIndex past the end.
        None
    } else if n >= 0.0 {
        Some(n as i64)
    } else if n >= -(length as f64) {
        Some(length + n as i64)
    } else {
        // fromIndex <= -length (including -Infinity): clamp to 0.
        Some(0)
    }
}

/// indexOf for arrays, using jsvalue comparison (handles NaN-boxed strings
/// correctly). `from_index` / `has_from` implement the optional ECMA-262
/// `fromIndex` argument (#2804); `has_from == 0` searches from index 0.
#[no_mangle]
pub extern "C" fn js_array_indexOf_jsvalue(
    arr: *const ArrayHeader,
    value: f64,
    from_index: f64,
    has_from: i32,
) -> i64 {
    let arr = normalize_array_receiver(arr);
    if arr.is_null() {
        return -1;
    }
    // TypedArray: strict-equality search over the typed store. `len == 0`
    // returns BEFORE ToIntegerOrInfinity(fromIndex) per spec (an observable /
    // throwing coercion never runs on an empty array). `js_jsvalue_equals`
    // (not `==`) so BigInt elements compare by value — `js_typed_array_get`
    // on a BigInt64Array returns a freshly boxed BigInt whose bit pattern
    // never raw-equals the search value.
    if let Some((ta, len)) = as_typed_array(arr) {
        if len == 0 {
            return -1;
        }
        let start = match forward_start_index(len as i64, from_index, has_from) {
            Some(s) => s as i32,
            None => return -1,
        };
        for i in start..len {
            if crate::value::js_jsvalue_equals(crate::typedarray::js_typed_array_get(ta, i), value)
                == 1
            {
                return i as i64;
            }
        }
        return -1;
    }
    unsafe {
        let length = (*arr).length as i64;
        // ECMA-262 §23.1.3.13 step 3: `len == 0 → -1` is checked BEFORE
        // ToIntegerOrInfinity(fromIndex), so a throwing/observable `fromIndex`
        // coercion never runs on an empty array.
        if length == 0 {
            return -1;
        }
        let start = match forward_start_index(length, from_index, has_from) {
            Some(s) => s,
            None => return -1,
        };
        let elements_ptr = array_elements_ptr(arr);
        let exotic = crate::array::array_iteration_is_exotic(arr);
        for i in start..length {
            let element = if exotic {
                if !crate::array::array_spec_has_index(arr, i as u32) {
                    continue;
                }
                crate::array::array_spec_get(arr, i as u32)
            } else {
                match present_array_element(elements_ptr, i as usize) {
                    Some(e) => e,
                    None => continue,
                }
            };
            if crate::value::js_jsvalue_equals(element, value) == 1 {
                return i;
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
) -> i64 {
    let arr = normalize_array_receiver(arr);
    if arr.is_null() {
        return -1;
    }
    // TypedArray: backward strict-equality numeric search over the typed
    // store (#2457 routes `Int32Array(..).lastIndexOf` here via the new
    // `Expr::ArrayLastIndexOf` lowering). Mirrors the `indexOf` typed branch.
    if let Some((ta, tlen)) = as_typed_array(arr) {
        let length = tlen as i64;
        if length == 0 {
            return -1;
        }
        let start: i64 = if has_from == 0 {
            length - 1
        } else {
            let n = from_index_to_integer(from_index);
            if n >= length as f64 {
                length - 1
            } else if n >= 0.0 {
                n as i64
            } else if n >= -(length as f64) {
                length + (n as i64)
            } else {
                return -1;
            }
        };
        let mut i = start;
        while i >= 0 {
            if crate::value::js_jsvalue_equals(
                crate::typedarray::js_typed_array_get(ta, i as i32),
                value,
            ) == 1
            {
                return i as i64;
            }
            i -= 1;
        }
        return -1;
    }
    unsafe {
        let length = (*arr).length as i64;
        if length == 0 {
            return -1;
        }
        let elements_ptr = array_elements_ptr(arr);

        // Determine the start index. Without an explicit fromIndex, start at
        // the last element. With one, apply ToIntegerOrInfinity + clamping
        // while avoiding i64 overflow for ±Infinity / out-of-range values.
        let start: i64 = if has_from == 0 {
            length - 1
        } else {
            let n = from_index_to_integer(from_index);
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

        let exotic = crate::array::array_iteration_is_exotic(arr);
        let mut i = start;
        while i >= 0 {
            let element = if exotic {
                if !crate::array::array_spec_has_index(arr, i as u32) {
                    i -= 1;
                    continue;
                }
                crate::array::array_spec_get(arr, i as u32)
            } else {
                match present_array_element(elements_ptr, i as usize) {
                    Some(e) => e,
                    None => {
                        i -= 1;
                        continue;
                    }
                }
            };
            if crate::value::js_jsvalue_equals(element, value) == 1 {
                return i;
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
/// `from_index` / `has_from` implement the optional ECMA-262 `fromIndex`
/// argument (#2804); `has_from == 0` searches from index 0.
/// Returns 1 if found, 0 if not.
#[no_mangle]
pub extern "C" fn js_array_includes_jsvalue(
    arr: *const ArrayHeader,
    value: f64,
    from_index: f64,
    has_from: i32,
) -> i32 {
    let arr = normalize_array_receiver(arr);
    if arr.is_null() {
        return 0;
    }
    // TypedArray: SameValueZero numeric search (so includes(NaN) is true for
    // float typed arrays).
    if let Some((ta, len)) = as_typed_array(arr) {
        if len == 0 {
            return 0;
        }
        let start = match forward_start_index(len as i64, from_index, has_from) {
            Some(s) => s as i32,
            None => return 0,
        };
        for i in start..len {
            let e = crate::typedarray::js_typed_array_get(ta, i);
            if crate::value::js_jsvalue_same_value_zero(e, value) == 1 {
                return 1;
            }
        }
        return 0;
    }
    unsafe {
        let length = (*arr).length as i64;
        // §23.1.3.16 step 3: `len == 0 → false` BEFORE ToIntegerOrInfinity
        // (fromIndex), mirroring `indexOf`.
        if length == 0 {
            return 0;
        }
        let start = match forward_start_index(length, from_index, has_from) {
            Some(s) => s,
            None => return 0,
        };
        let elements_ptr = array_elements_ptr(arr);

        // `Array.prototype.includes` uses SameValueZero (ECMA-262 §23.1.3.16),
        // which differs from === in one place: NaN equals NaN. Routing
        // through `js_jsvalue_same_value_zero` preserves the `indexOf(NaN) ===
        // -1` / `includes(NaN) === true` split.
        for i in start..length {
            let element = array_element_get_value(elements_ptr, i as usize);
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
        assert_eq!(js_array_indexOf_jsvalue(arr, 2.0, 0.0, 0), 1);
        assert_eq!(js_array_indexOf_jsvalue(arr, 3.0, 0.0, 0), 2);
        assert_eq!(js_array_indexOf_jsvalue(arr, 9.0, 0.0, 0), -1);
        assert_eq!(js_array_includes_jsvalue(arr, 3.0, 0.0, 0), 1);
        assert_eq!(js_array_includes_jsvalue(arr, 9.0, 0.0, 0), 0);
        // indexOf uses strict equality → NaN never matches.
        assert_eq!(js_array_indexOf_jsvalue(arr, f64::NAN, 0.0, 0), -1);
        // fromIndex on a TypedArray: search from index 2 onward.
        assert_eq!(js_array_indexOf_jsvalue(arr, 1.0, 2.0, 1), 4);
        // negative fromIndex counts from end: -2 → index 3.
        assert_eq!(js_array_indexOf_jsvalue(arr, 2.0, -2.0, 1), 3);
        // +Infinity fromIndex never matches forward search.
        assert_eq!(js_array_indexOf_jsvalue(arr, 1.0, f64::INFINITY, 1), -1);
        assert_eq!(js_array_includes_jsvalue(arr, 1.0, f64::INFINITY, 1), 0);
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
        assert_eq!(js_array_includes_jsvalue(arr, f64::NAN, 0.0, 0), 1);
        assert_eq!(js_array_indexOf_jsvalue(arr, f64::NAN, 0.0, 0), -1);
        assert_eq!(js_array_indexOf_jsvalue(arr, 2.5, 0.0, 0), 2);
    }

    /// `lastIndexOf` on a registered TypedArray scans the typed backing store
    /// backward (#2457). Without a fromIndex it starts at the last element;
    /// with one it clamps like the array path.
    #[test]
    fn typed_array_last_index_of() {
        // Int32Array([1, 2, 3, 2, 1])
        let ta = js_typed_array_new_empty(KIND_INT32 as i32, 5);
        for (i, v) in [1.0, 2.0, 3.0, 2.0, 1.0].iter().enumerate() {
            js_typed_array_set(ta, i as i32, *v);
        }
        let arr = ta as *const ArrayHeader;
        // no fromIndex (has_from = 0): highest match.
        assert_eq!(js_array_last_index_of_jsvalue(arr, 2.0, 0.0, 0), 3);
        assert_eq!(js_array_last_index_of_jsvalue(arr, 9.0, 0.0, 0), -1);
        // fromIndex = 2: search backward from index 2.
        assert_eq!(js_array_last_index_of_jsvalue(arr, 2.0, 2.0, 1), 1);
        // strict equality → NaN never matches.
        assert_eq!(js_array_last_index_of_jsvalue(arr, f64::NAN, 0.0, 0), -1);
    }
}
