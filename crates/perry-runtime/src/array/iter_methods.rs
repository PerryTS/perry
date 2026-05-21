//! Higher-order array methods.
use super::*;
use crate::closure::{js_closure_call2, js_closure_call3, ClosureHeader};
use std::ptr;

/// forEach - call callback(element, index) for each element
/// Returns nothing (void)
#[no_mangle]
pub extern "C" fn js_array_forEach(arr: *const ArrayHeader, callback: *const ClosureHeader) {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return;
    }
    unsafe {
        let length = (*arr).length;
        let elements_ptr = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;

        for i in 0..length as usize {
            let element = *elements_ptr.add(i);
            // JS forEach passes (element, index, array). The callback
            // dispatch path now supports call3 safely, so bound native
            // methods like `array.forEach(console.log)` can observe the
            // source array just like Node.
            let arr_value = f64::from_bits(crate::value::JSValue::pointer(arr as *const u8).bits());
            js_closure_call3(callback, element, i as f64, arr_value);
        }
    }
}

/// map - create new array by calling callback(element) on each element
/// Returns pointer to new array
#[no_mangle]
pub extern "C" fn js_array_map(
    arr: *const ArrayHeader,
    callback: *const ClosureHeader,
) -> *mut ArrayHeader {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return js_array_alloc(0);
    }
    unsafe {
        let length = (*arr).length;
        let elements_ptr = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;

        // Allocate result array with same capacity
        let result = js_array_alloc(length);
        let result_elements =
            (result as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;

        for i in 0..length as usize {
            let element = *elements_ptr.add(i);
            // Pass both element and index — JS .map() callback receives (element, index, array).
            // Using call2 ensures the index parameter is defined instead of garbage from registers,
            // which caused SIGSEGV on x86_64 when callbacks used the index (e.g., (_, i) => obj[i]).
            let mapped = js_closure_call2(callback, element, i as f64);
            ptr::write(result_elements.add(i), mapped);
            note_array_slot(result, i, mapped.to_bits());
            (*result).length = (i + 1) as u32;
        }
        (*result).length = length;

        result
    }
}

/// filter - create new array with elements where callback(element) returns truthy
/// Returns pointer to new array
#[no_mangle]
pub extern "C" fn js_array_filter(
    arr: *const ArrayHeader,
    callback: *const ClosureHeader,
) -> *mut ArrayHeader {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return js_array_alloc(0);
    }
    unsafe {
        let length = (*arr).length;
        let elements_ptr = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;

        // Allocate result array with same capacity (might be smaller)
        let mut result = js_array_alloc(length);
        // #854: `js_array_push_f64` already maintains `(*result).length`, so the
        // separate `result_len` counter that used to live here was dead.

        for i in 0..length as usize {
            let element = *elements_ptr.add(i);
            let keep = js_closure_call2(callback, element, i as f64);
            // Proper truthy check: handles NaN-boxed booleans (TAG_FALSE != 0.0 but is falsy)
            if crate::value::js_is_truthy(keep) != 0 {
                result = js_array_push_f64(result, element);
            }
        }

        result
    }
}

/// find - find first element that matches callback(element) => true
/// Returns the element as f64, or f64::NAN (undefined) if not found
#[no_mangle]
pub extern "C" fn js_array_find(arr: *const ArrayHeader, callback: *const ClosureHeader) -> f64 {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    unsafe {
        let length = (*arr).length;
        let elements_ptr = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;

        for i in 0..length as usize {
            let element = *elements_ptr.add(i);
            let result = js_closure_call2(callback, element, i as f64);
            // Proper truthy check: handles NaN-boxed booleans
            if crate::value::js_is_truthy(result) != 0 {
                return element;
            }
        }

        // Not found - return undefined (NaN)
        f64::NAN
    }
}

/// findIndex - find index of first element that matches callback(element) => true
/// Returns the index as i32, or -1 if not found
#[no_mangle]
pub extern "C" fn js_array_findIndex(
    arr: *const ArrayHeader,
    callback: *const ClosureHeader,
) -> i32 {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return -1;
    }
    unsafe {
        let length = (*arr).length;
        let elements_ptr = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;

        for i in 0..length as usize {
            let element = *elements_ptr.add(i);
            let result = js_closure_call2(callback, element, i as f64);
            // Proper truthy check: handles NaN-boxed booleans
            if crate::value::js_is_truthy(result) != 0 {
                return i as i32;
            }
        }

        // Not found
        -1
    }
}

/// findLast - like find but iterates from the end
#[no_mangle]
pub extern "C" fn js_array_find_last(
    arr: *const ArrayHeader,
    callback: *const ClosureHeader,
) -> f64 {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    if crate::typedarray::lookup_typed_array_kind(arr as usize).is_some() {
        return crate::typedarray::js_typed_array_find_last(
            arr as *const crate::typedarray::TypedArrayHeader,
            callback,
        );
    }
    unsafe {
        let length = (*arr).length as usize;
        let elements_ptr = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
        for i in (0..length).rev() {
            let element = *elements_ptr.add(i);
            let result = js_closure_call2(callback, element, i as f64);
            if crate::value::js_is_truthy(result) != 0 {
                return element;
            }
        }
        f64::from_bits(crate::value::TAG_UNDEFINED)
    }
}

/// findLastIndex - like findIndex but iterates from the end
#[no_mangle]
pub extern "C" fn js_array_find_last_index(
    arr: *const ArrayHeader,
    callback: *const ClosureHeader,
) -> i32 {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return -1;
    }
    if crate::typedarray::lookup_typed_array_kind(arr as usize).is_some() {
        let r = crate::typedarray::js_typed_array_find_last_index(
            arr as *const crate::typedarray::TypedArrayHeader,
            callback,
        );
        return r as i32;
    }
    unsafe {
        let length = (*arr).length as usize;
        let elements_ptr = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
        for i in (0..length).rev() {
            let element = *elements_ptr.add(i);
            let result = js_closure_call2(callback, element, i as f64);
            if crate::value::js_is_truthy(result) != 0 {
                return i as i32;
            }
        }
        -1
    }
}

/// at - element access supporting negative indices (arr.at(-1) = last)
#[no_mangle]
pub extern "C" fn js_array_at(arr: *const ArrayHeader, index: f64) -> f64 {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    // If this pointer is actually a typed-array, dispatch there. Typed arrays
    // and Uint8Array/Buffer have different layouts than ArrayHeader, and the
    // codegen happily routes their `.at(i)` through this generic helper.
    let addr = arr as usize;
    if crate::typedarray::lookup_typed_array_kind(addr).is_some() {
        return crate::typedarray::js_typed_array_at(
            addr as *const crate::typedarray::TypedArrayHeader,
            index,
        );
    }
    if crate::buffer::is_registered_buffer(addr) {
        let buf = addr as *const crate::buffer::BufferHeader;
        unsafe {
            let length = (*buf).length as i64;
            let mut idx = index as i64;
            if idx < 0 {
                idx += length;
            }
            if idx < 0 || idx >= length {
                return f64::from_bits(crate::value::TAG_UNDEFINED);
            }
            let data = (buf as *const u8).add(std::mem::size_of::<crate::buffer::BufferHeader>());
            return *data.add(idx as usize) as f64;
        }
    }
    unsafe {
        let length = (*arr).length as i64;
        let mut idx = index as i64;
        if idx < 0 {
            idx += length;
        }
        if idx < 0 || idx >= length {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }
        let elements_ptr = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
        *elements_ptr.add(idx as usize)
    }
}

/// some - returns true if any element matches callback(element) => true
/// Returns TAG_TRUE or TAG_FALSE as f64
#[no_mangle]
pub extern "C" fn js_array_some(arr: *const ArrayHeader, callback: *const ClosureHeader) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return f64::from_bits(TAG_FALSE);
    }
    unsafe {
        let length = (*arr).length;
        let elements_ptr = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;

        for i in 0..length as usize {
            let element = *elements_ptr.add(i);
            let result = js_closure_call2(callback, element, i as f64);
            if crate::value::js_is_truthy(result) != 0 {
                return f64::from_bits(TAG_TRUE);
            }
        }

        f64::from_bits(TAG_FALSE)
    }
}

/// every - returns true if all elements match callback(element) => true
/// Returns TAG_TRUE or TAG_FALSE as f64
#[no_mangle]
pub extern "C" fn js_array_every(arr: *const ArrayHeader, callback: *const ClosureHeader) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return f64::from_bits(TAG_TRUE);
    }
    unsafe {
        let length = (*arr).length;
        let elements_ptr = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;

        for i in 0..length as usize {
            let element = *elements_ptr.add(i);
            let result = js_closure_call2(callback, element, i as f64);
            if crate::value::js_is_truthy(result) == 0 {
                return f64::from_bits(TAG_FALSE);
            }
        }

        f64::from_bits(TAG_TRUE)
    }
}

/// flatMap - map each element to an array, then flatten one level
/// Returns pointer to new array
#[no_mangle]
pub extern "C" fn js_array_flatMap(
    arr: *const ArrayHeader,
    callback: *const ClosureHeader,
) -> *mut ArrayHeader {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return js_array_alloc(0);
    }
    unsafe {
        let length = (*arr).length;
        let elements_ptr = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;

        let mut result = js_array_alloc(length);

        for i in 0..length as usize {
            let element = *elements_ptr.add(i);
            let mapped = js_closure_call2(callback, element, i as f64);
            // Check if the mapped value is an array (pointer-tagged)
            let bits = mapped.to_bits();
            let top16 = bits >> 48;
            if top16 == 0x7FFD {
                // NaN-boxed pointer — likely an array
                let sub_arr = (bits & 0x0000_FFFF_FFFF_FFFF) as *const ArrayHeader;
                if !sub_arr.is_null() {
                    let sub_len = (*sub_arr).length;
                    let sub_elements = (sub_arr as *const u8)
                        .add(std::mem::size_of::<ArrayHeader>())
                        as *const f64;
                    for j in 0..sub_len as usize {
                        let sub_element = *sub_elements.add(j);
                        result = js_array_push_f64(result, sub_element);
                    }
                }
            } else {
                // Not an array — push as single element
                result = js_array_push_f64(result, mapped);
            }
        }

        result
    }
}

/// reduce - accumulate values using callback(accumulator, element)
/// initial_ptr is pointer to f64 initial value (null if not provided)
/// Returns the final accumulated value
#[no_mangle]
pub extern "C" fn js_array_reduce(
    arr: *const ArrayHeader,
    callback: *const ClosureHeader,
    has_initial: i32,
    initial: f64,
) -> f64 {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return if has_initial != 0 { initial } else { f64::NAN };
    }
    unsafe {
        let length = (*arr).length;
        let elements_ptr = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;

        if length == 0 {
            if has_initial != 0 {
                return initial;
            } else {
                // TypeError in JS, but we return NaN for simplicity
                return f64::NAN;
            }
        }

        let (mut accumulator, start_idx) = if has_initial != 0 {
            (initial, 0)
        } else {
            // Use first element as initial
            (*elements_ptr, 1)
        };

        for i in start_idx..length as usize {
            let element = *elements_ptr.add(i);
            // Refs #488 drizzle-sqlite: spec says callback is
            // `(accumulator, currentValue, currentIndex, array)`. Pre-fix
            // we only passed 2 args, so callbacks like drizzle's
            // `mapResultRow`'s `(result, {path, field}, columnIndex)` got
            // `columnIndex === undefined` and ended up reading `row[undefined]`
            // (which perry returns as `row[0]`) — every column projection
            // collapsed onto the first column's value (alice.age = 1
            // instead of 30). We now pass the index as the 3rd arg.
            // (The 4th `array` arg is intentionally omitted — drizzle and
            // most real callbacks ignore it; adding it would require a
            // call4 path and another NaN-box of the array handle.)
            accumulator = js_closure_call3(callback, accumulator, element, i as f64);
        }

        accumulator
    }
}

/// join - Join array elements into a string with a separator
/// Returns pointer to new StringHeader
#[no_mangle]
pub extern "C" fn js_array_join(
    arr: *const ArrayHeader,
    separator: *const crate::string::StringHeader,
) -> *mut crate::string::StringHeader {
    use crate::string::{js_string_from_bytes, StringHeader};
    use crate::value::JSValue;

    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return crate::string::js_string_from_bytes(b"".as_ptr(), 0);
    }
    unsafe {
        let length = (*arr).length;

        // Empty array returns empty string
        if length == 0 {
            return js_string_from_bytes(ptr::null(), 0);
        }

        let elements_ptr = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;

        // Get separator string
        let sep_str = if separator.is_null() {
            ","
        } else {
            let sep_len = (*separator).byte_len as usize;
            let sep_data = (separator as *const u8).add(std::mem::size_of::<StringHeader>());
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(sep_data, sep_len))
        };

        // Build result string
        let mut result = String::new();
        for i in 0..length as usize {
            if i > 0 {
                result.push_str(sep_str);
            }
            let element_bits = (*elements_ptr.add(i)).to_bits();
            let jsvalue = JSValue::from_bits(element_bits);

            // Issue #907: `Array(n)` initializes slots to TAG_HOLE
            // (see `js_array_alloc_with_length`). Per ES2015 §22.1.3.13
            // (Array.prototype.join), holes go through Get which returns
            // undefined → the spec's ToString step turns them into the
            // empty string. Without this check the catch-all below
            // emitted "[object Object]", so `Array(3).join("0")` returned
            // `"[object Object]0[object Object]0[object Object]"` instead
            // of `"00"`. dayjs's `m(t,e,n)` pad utility builds the UTC
            // offset string via `Array(e+1-r.length).join(n)` and the
            // result silently corrupted `b.z(this)` (the format `i`
            // capture), which downstream triggered
            // `TypeError: (number).replace is not a function` once the
            // catch-all fallthrough reached `i.replace(":","")`.
            if element_bits == crate::value::TAG_HOLE {
                // hole → empty string per spec
                continue;
            }

            // Convert element to string based on its type
            if jsvalue.is_string() {
                let str_ptr = jsvalue.as_pointer() as *const StringHeader;
                let str_len = (*str_ptr).byte_len as usize;
                let str_data = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                let s =
                    std::str::from_utf8_unchecked(std::slice::from_raw_parts(str_data, str_len));
                result.push_str(s);
            } else if jsvalue.is_short_string() {
                // v0.5.214 SSO — decode inline into a stack buffer
                // and push bytes. No heap roundtrip via
                // materialize_to_heap.
                let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
                let n = jsvalue.short_string_to_buf(&mut scratch);
                let s = std::str::from_utf8_unchecked(&scratch[..n]);
                result.push_str(s);
            } else if jsvalue.is_pointer() {
                // POINTER_TAG — may be a string stored with the wrong tag (cross-module)
                let ptr = (element_bits & 0x0000_FFFF_FFFF_FFFF) as *const StringHeader;
                if !ptr.is_null() && (ptr as usize) >= 0x1000 {
                    let str_len = (*ptr).byte_len as usize;
                    let str_data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                    let s = std::str::from_utf8_unchecked(std::slice::from_raw_parts(
                        str_data, str_len,
                    ));
                    result.push_str(s);
                } else {
                    result.push_str("[object Object]");
                }
            } else if jsvalue.is_number() {
                let n = jsvalue.as_number();
                if n.is_nan() {
                    result.push_str("NaN");
                } else if n.is_infinite() {
                    result.push_str(if n > 0.0 { "Infinity" } else { "-Infinity" });
                } else if n == 0.0 {
                    result.push('0');
                } else if n.fract() == 0.0 && n.abs() < 1e15 {
                    result.push_str(&format!("{}", n as i64));
                } else {
                    result.push_str(&format!("{}", n));
                }
            } else if jsvalue.is_null() {
                // null stringifies to empty string in join
            } else if jsvalue.is_undefined() {
                // undefined stringifies to empty string in join
            } else if jsvalue.is_bool() {
                result.push_str(if jsvalue.as_bool() { "true" } else { "false" });
            } else if element_bits > 0x1000
                && element_bits < 0x0001_0000_0000_0000
                && (element_bits & 0x3) == 0
            {
                // Raw pointer fallback — string stored without NaN-box tag
                let str_ptr = element_bits as *const StringHeader;
                let str_len = (*str_ptr).byte_len as usize;
                if str_len < 10_000_000 {
                    let str_data = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                    let s = std::str::from_utf8_unchecked(std::slice::from_raw_parts(
                        str_data, str_len,
                    ));
                    result.push_str(s);
                } else {
                    result.push_str("[object Object]");
                }
            } else {
                // For objects/arrays, just use placeholder
                result.push_str("[object Object]");
            }
        }

        // Create result string - extract ptr/len before passing to avoid
        // potential LLVM reordering of String drop vs copy_nonoverlapping
        let result_ptr = result.as_ptr();
        let result_len = result.len() as u32;
        let ret = js_string_from_bytes(result_ptr, result_len);
        // Ensure result String stays alive until after the copy completes
        std::hint::black_box(&result);
        drop(result);
        ret
    }
}
