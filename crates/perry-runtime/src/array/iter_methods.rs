//! Higher-order array methods.
use super::*;
use crate::closure::{js_closure_call3, js_closure_call4, ClosureHeader};
use std::ptr;

/// NaN-box an array header pointer as the JS `array` receiver value passed as
/// the 3rd/4th callback argument (`(element, index, array)` /
/// `(accumulator, currentValue, currentIndex, array)`). Per spec the callback
/// observes the original receiver object.
#[inline(always)]
fn array_receiver_value(arr: *const ArrayHeader) -> f64 {
    f64::from_bits(crate::value::JSValue::pointer(arr as *const u8).bits())
}

/// forEach - call callback(element, index) for each element
/// Returns nothing (void)
#[no_mangle]
pub extern "C" fn js_array_forEach(arr: *const ArrayHeader, callback: *const ClosureHeader) {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return;
    }
    if crate::typedarray::lookup_typed_array_kind(arr as usize).is_some() {
        crate::typedarray::js_typed_array_for_each(
            arr as *const crate::typedarray::TypedArrayHeader,
            callback,
        );
        return;
    }
    unsafe {
        let length = (*arr).length;
        let elements_ptr = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;

        let arr_value = array_receiver_value(arr);
        for i in 0..length as usize {
            let element = *elements_ptr.add(i);
            // JS forEach passes (element, index, array). The callback
            // dispatch path supports call3 safely, so bound native
            // methods like `array.forEach(console.log)` can observe the
            // source array just like Node.
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
    if crate::typedarray::lookup_typed_array_kind(arr as usize).is_some() {
        // Typed-array receiver: read elements per element-kind and return a
        // same-kind TypedArray (mirrors the sort/at/findLast delegation).
        return crate::typedarray::js_typed_array_map(
            arr as *const crate::typedarray::TypedArrayHeader,
            callback,
        ) as *mut ArrayHeader;
    }
    unsafe {
        let length = (*arr).length;
        let elements_ptr = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;

        // Allocate result array with same capacity
        let result = js_array_alloc(length);
        let result_elements =
            (result as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
        let arr_value = array_receiver_value(arr);

        for i in 0..length as usize {
            let element = *elements_ptr.add(i);
            // JS .map() callback receives (element, index, array).
            let mapped = js_closure_call3(callback, element, i as f64, arr_value);
            // GC_STORE_AUDIT(INIT): map result is unpublished; slot layout is noted immediately below.
            ptr::write(result_elements.add(i), mapped);
            let mapped_bits = mapped.to_bits();
            if length <= 64 {
                // Fast path: skip the generational write barrier.
                // `result` was just allocated; for length ≤ 64 it stays
                // in the nursery for the whole loop in practice, so the
                // young→old barrier is redundant — only the layout slot
                // metadata is needed for GC tracing. If a future GC
                // policy starts tenuring nursery objects mid-loop
                // (e.g. aggressive evacuation under
                // `PERRY_GC_FORCE_EVACUATE=1` triggered by the callback
                // allocating), this path needs the full barrier helper
                // because subsequent stores would miss the remembered
                // set. The 64-element cap keeps that probability low.
                note_array_slot_layout_only(result, i, mapped_bits);
            } else {
                note_array_slot(result, i, mapped_bits);
            }
            (*result).length = (i + 1) as u32;
        }
        (*result).length = length;

        result
    }
}

/// map for an unused result: preserve callback evaluation order and side
/// effects without allocating or filling the result array.
#[no_mangle]
pub extern "C" fn js_array_map_discard(arr: *const ArrayHeader, callback: *const ClosureHeader) {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return;
    }
    unsafe {
        let length = (*arr).length;
        let elements_ptr = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
        let arr_value = array_receiver_value(arr);

        for i in 0..length as usize {
            let element = *elements_ptr.add(i);
            let _ = js_closure_call3(callback, element, i as f64, arr_value);
        }
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
    if crate::typedarray::lookup_typed_array_kind(arr as usize).is_some() {
        return crate::typedarray::js_typed_array_filter(
            arr as *const crate::typedarray::TypedArrayHeader,
            callback,
        ) as *mut ArrayHeader;
    }
    unsafe {
        let length = (*arr).length;
        let elements_ptr = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;

        // Allocate result array with same capacity (might be smaller)
        let mut result = js_array_alloc(length);
        let arr_value = array_receiver_value(arr);
        // #854: `js_array_push_f64` already maintains `(*result).length`, so the
        // separate `result_len` counter that used to live here was dead.

        for i in 0..length as usize {
            let element = *elements_ptr.add(i);
            let keep = js_closure_call3(callback, element, i as f64, arr_value);
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
    if crate::typedarray::lookup_typed_array_kind(arr as usize).is_some() {
        return crate::typedarray::js_typed_array_find(
            arr as *const crate::typedarray::TypedArrayHeader,
            callback,
        );
    }
    unsafe {
        let length = (*arr).length;
        let elements_ptr = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
        let arr_value = array_receiver_value(arr);

        for i in 0..length as usize {
            let element = *elements_ptr.add(i);
            let result = js_closure_call3(callback, element, i as f64, arr_value);
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
    if crate::typedarray::lookup_typed_array_kind(arr as usize).is_some() {
        return crate::typedarray::js_typed_array_find_index(
            arr as *const crate::typedarray::TypedArrayHeader,
            callback,
        ) as i32;
    }
    unsafe {
        let length = (*arr).length;
        let elements_ptr = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
        let arr_value = array_receiver_value(arr);

        for i in 0..length as usize {
            let element = *elements_ptr.add(i);
            let result = js_closure_call3(callback, element, i as f64, arr_value);
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
        let arr_value = array_receiver_value(arr);
        for i in (0..length).rev() {
            let element = *elements_ptr.add(i);
            let result = js_closure_call3(callback, element, i as f64, arr_value);
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
        let arr_value = array_receiver_value(arr);
        for i in (0..length).rev() {
            let element = *elements_ptr.add(i);
            let result = js_closure_call3(callback, element, i as f64, arr_value);
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
    if crate::typedarray::lookup_typed_array_kind(arr as usize).is_some() {
        return crate::typedarray::js_typed_array_some(
            arr as *const crate::typedarray::TypedArrayHeader,
            callback,
        );
    }
    unsafe {
        let length = (*arr).length;
        let elements_ptr = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
        let arr_value = array_receiver_value(arr);

        for i in 0..length as usize {
            let element = *elements_ptr.add(i);
            let result = js_closure_call3(callback, element, i as f64, arr_value);
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
    if crate::typedarray::lookup_typed_array_kind(arr as usize).is_some() {
        return crate::typedarray::js_typed_array_every(
            arr as *const crate::typedarray::TypedArrayHeader,
            callback,
        );
    }
    unsafe {
        let length = (*arr).length;
        let elements_ptr = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
        let arr_value = array_receiver_value(arr);

        for i in 0..length as usize {
            let element = *elements_ptr.add(i);
            let result = js_closure_call3(callback, element, i as f64, arr_value);
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
        let arr_value = array_receiver_value(arr);

        for i in 0..length as usize {
            let element = *elements_ptr.add(i);
            let mapped = js_closure_call3(callback, element, i as f64, arr_value);
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
        if has_initial != 0 {
            return initial;
        }
        throw_reduce_of_empty();
    }
    // Typed-array receiver: read elements per element-kind (raw int/float
    // storage is NOT NaN-boxed f64, so the generic ArrayHeader path below would
    // read garbage). Issue #2799.
    if crate::typedarray::lookup_typed_array_kind(arr as usize).is_some() {
        return crate::typedarray::js_typed_array_reduce(
            arr as *const crate::typedarray::TypedArrayHeader,
            callback,
            has_initial,
            initial,
        );
    }
    unsafe {
        let length = (*arr).length;
        let elements_ptr = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;

        if length == 0 {
            if has_initial != 0 {
                return initial;
            }
            // Per spec (ES2015 §22.1.3.18): empty array with no initial value
            // throws `TypeError: Reduce of empty array with no initial value`.
            throw_reduce_of_empty();
        }

        let (mut accumulator, start_idx) = if has_initial != 0 {
            (initial, 0)
        } else {
            // Use first element as initial
            (*elements_ptr, 1)
        };

        let arr_value = array_receiver_value(arr);
        for i in start_idx..length as usize {
            let element = *elements_ptr.add(i);
            // Spec callback is `(accumulator, currentValue, currentIndex, array)`.
            accumulator = js_closure_call4(callback, accumulator, element, i as f64, arr_value);
        }

        accumulator
    }
}

/// Throw `TypeError: Reduce of empty array with no initial value` (ES §22.1.3.18 /
/// §22.2.3.20). Routed through Perry's exception machinery so it can be caught.
pub(crate) fn throw_reduce_of_empty() -> ! {
    let msg = "Reduce of empty array with no initial value";
    let msg_str = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err_ptr = crate::error::js_typeerror_new(msg_str);
    let err_value = crate::value::JSValue::pointer(err_ptr as *const u8).bits();
    crate::exception::js_throw(f64::from_bits(err_value))
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
                let str_ptr = jsvalue.as_string_ptr();
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
                // POINTER_TAG. Two cases:
                //  1. A genuine string NaN-boxed with POINTER_TAG instead of
                //     STRING_TAG (a cross-module mis-tag) — read its bytes.
                //  2. A real heap object/array/error/buffer — these must go
                //     through the spec `ToString` (`js_jsvalue_to_string`):
                //     Array→nested join, Error→"name: message" (#2135), an
                //     object with a custom `toString`→that result, buffers,
                //     etc. The old code read *every* pointer as a
                //     `StringHeader`, so a non-string's garbage `byte_len`
                //     produced corrupted output (`[err].join()` → empty).
                //     Distinguish via the GcHeader type tag, excluding the
                //     headerless buffer/symbol pointers first.
                let ptr_addr = (element_bits & 0x0000_FFFF_FFFF_FFFF) as usize;
                if ptr_addr >= 0x1000 {
                    let is_string_obj = !crate::buffer::is_registered_buffer(ptr_addr)
                        && !crate::symbol::is_registered_symbol(ptr_addr)
                        && {
                            let gc_header = (ptr_addr as *const u8).sub(crate::gc::GC_HEADER_SIZE)
                                as *const crate::gc::GcHeader;
                            (*gc_header).obj_type == crate::gc::GC_TYPE_STRING
                        };
                    let s_ptr = if is_string_obj {
                        ptr_addr as *const StringHeader
                    } else {
                        crate::value::js_jsvalue_to_string(f64::from_bits(element_bits))
                    };
                    if !s_ptr.is_null() {
                        let str_len = (*s_ptr).byte_len as usize;
                        let str_data =
                            (s_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                        result.push_str(std::str::from_utf8_unchecked(std::slice::from_raw_parts(
                            str_data, str_len,
                        )));
                    }
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

#[no_mangle]
pub extern "C" fn js_array_join_value(
    arr: *const ArrayHeader,
    separator_value: f64,
) -> *mut crate::string::StringHeader {
    let separator = if separator_value.to_bits() == crate::value::TAG_UNDEFINED {
        ptr::null()
    } else {
        crate::value::js_jsvalue_to_string(separator_value) as *const crate::string::StringHeader
    };
    js_array_join(arr, separator)
}

// Symbol retention: codegen lowers `arr.join(sep)` to a call to
// `js_array_join_value`, but its only in-crate caller sits behind a dispatch
// path the auto-optimize whole-program-bitcode build can prove unreachable and
// dead-strip — which broke the default `perry file.ts -o out` link with
// `undefined _js_array_join_value`. The `#[used]` static pins the symbol so it
// survives every link mode. Same pattern as `node_stream_keepalive.rs`.
#[used]
static KEEP_ARRAY_JOIN_VALUE: extern "C" fn(
    *const ArrayHeader,
    f64,
) -> *mut crate::string::StringHeader = js_array_join_value;

/// `arr.toLocaleString(locales?, options?)` (#2808).
///
/// Per the ECMAScript `Array.prototype.toLocaleString` algorithm: walk the
/// array from `0` to `length - 1`, render `null` / `undefined` elements as the
/// empty string, and for every other element call its own
/// `toLocaleString(locales, options)` method, stringify the result, and join
/// the per-element strings with `","` separators. `locales` / `options` are
/// forwarded verbatim to each element method (omitted args are passed as
/// `undefined`).
#[no_mangle]
pub extern "C" fn js_array_to_locale_string(
    arr: *const ArrayHeader,
    locales: f64,
    options: f64,
) -> *mut crate::string::StringHeader {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return crate::string::js_string_from_bytes(b"".as_ptr(), 0);
    }
    let len = unsafe { (*arr).length as usize };
    // Forward (locales, options) to each element's toLocaleString. Both are
    // always passed (undefined when omitted by the caller) so element methods
    // that branch on `arguments.length` still observe two slots, matching V8.
    let elem_args: [f64; 2] = [locales, options];
    let method = b"toLocaleString";
    let mut out = String::new();
    for i in 0..len {
        if i > 0 {
            out.push(',');
        }
        let elem = js_array_get(arr, i as u32);
        if elem.is_null() || elem.is_undefined() {
            // Nullish / hole -> empty field.
            continue;
        }
        let elem_f64 = f64::from_bits(elem.bits());
        let result = unsafe {
            crate::object::js_native_call_method(
                elem_f64,
                method.as_ptr() as *const i8,
                method.len(),
                elem_args.as_ptr(),
                elem_args.len(),
            )
        };
        let sp = crate::value::js_jsvalue_to_string(result);
        if !sp.is_null() {
            unsafe {
                let header = &*(sp as *const crate::string::StringHeader);
                let bytes_ptr =
                    (sp as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
                let slice = std::slice::from_raw_parts(bytes_ptr, header.byte_len as usize);
                out.push_str(std::str::from_utf8(slice).unwrap_or(""));
            }
        }
    }
    crate::string::js_string_from_bytes(out.as_ptr(), out.len() as u32)
}

#[used]
static KEEP_ARRAY_TO_LOCALE_STRING: extern "C" fn(
    *const ArrayHeader,
    f64,
    f64,
) -> *mut crate::string::StringHeader = js_array_to_locale_string;
