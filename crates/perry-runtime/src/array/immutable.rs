//! ES2023 immutable methods + copyWithin.
use super::*;
use crate::closure::ClosureHeader;

/// Throw a Node-compatible `RangeError("Invalid index : <idx>")` used by
/// `Array.prototype.with` for out-of-range / non-finite indexes.
#[cold]
fn throw_invalid_index(index: f64) -> ! {
    let body = if index.is_nan() {
        "NaN".to_string()
    } else if index.is_infinite() {
        if index > 0.0 {
            "Infinity".to_string()
        } else {
            "-Infinity".to_string()
        }
    } else if index == index.trunc() && index.abs() < 9.007_199_254_740_992e15 {
        // Integer-valued: print without a fractional part.
        format!("{}", index as i64)
    } else {
        format!("{}", index)
    };
    let message = format!("Invalid index : {}", body);
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let error = crate::error::js_rangeerror_new(msg);
    let bits = crate::value::JSValue::pointer(error as *const u8).bits();
    crate::exception::js_throw(f64::from_bits(bits))
}

/// `arr.toReversed()` — return a new reversed copy (immutable)
#[no_mangle]
pub extern "C" fn js_array_to_reversed(arr: *const ArrayHeader) -> *mut ArrayHeader {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return js_array_alloc(0);
    }
    if crate::typedarray::lookup_typed_array_kind(arr as usize).is_some() {
        return crate::typedarray::js_typed_array_to_reversed(
            arr as *const crate::typedarray::TypedArrayHeader,
        ) as *mut ArrayHeader;
    }
    unsafe {
        let len = (*arr).length as usize;
        let new_arr = js_array_alloc(len as u32);
        (*new_arr).length = len as u32;
        let src = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
        let dst = (new_arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
        for i in 0..len {
            // GC_STORE_AUDIT(BARRIERED): reversed copy initializes a fresh array rebuilt below.
            *dst.add(i) = *src.add(len - 1 - i);
        }
        rebuild_array_layout(new_arr);
        new_arr
    }
}

/// `arr.toSorted()` — return a new sorted copy (default string sort, immutable)
#[no_mangle]
pub extern "C" fn js_array_to_sorted_default(arr: *const ArrayHeader) -> *mut ArrayHeader {
    let arr = clean_arr_ptr(arr);
    if !arr.is_null() && crate::typedarray::lookup_typed_array_kind(arr as usize).is_some() {
        return crate::typedarray::js_typed_array_to_sorted_default(
            arr as *const crate::typedarray::TypedArrayHeader,
        ) as *mut ArrayHeader;
    }
    if arr.is_null() {
        return js_array_alloc(0);
    }
    unsafe {
        let len = (*arr).length as usize;
        // Clone the array
        let new_arr = js_array_alloc(len as u32);
        (*new_arr).length = len as u32;
        let src = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
        let dst = (new_arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
        // GC_STORE_AUDIT(BARRIERED): sorted clone copy initializes a fresh array rebuilt below.
        std::ptr::copy_nonoverlapping(src, dst, len);
        rebuild_array_layout(new_arr);
        // Sort the copy in-place using default sort
        js_array_sort_default(new_arr);
        new_arr
    }
}

/// `arr.toSorted(comparator)` — return a new sorted copy with comparator (immutable)
#[no_mangle]
pub extern "C" fn js_array_to_sorted_with_comparator(
    arr: *const ArrayHeader,
    comparator: *const ClosureHeader,
) -> *mut ArrayHeader {
    // #2796: a null comparator (validated `undefined`, or absent) means
    // "use the default sort path".
    if comparator.is_null() {
        return js_array_to_sorted_default(arr);
    }
    let arr = clean_arr_ptr(arr);
    if !arr.is_null() && crate::typedarray::lookup_typed_array_kind(arr as usize).is_some() {
        return crate::typedarray::js_typed_array_to_sorted_with_comparator(
            arr as *const crate::typedarray::TypedArrayHeader,
            comparator,
        ) as *mut ArrayHeader;
    }
    if arr.is_null() {
        return js_array_alloc(0);
    }
    unsafe {
        let len = (*arr).length as usize;
        // Clone the array
        let new_arr = js_array_alloc(len as u32);
        (*new_arr).length = len as u32;
        let src = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
        let dst = (new_arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
        // GC_STORE_AUDIT(BARRIERED): comparator sorted clone copy initializes a fresh array rebuilt below.
        std::ptr::copy_nonoverlapping(src, dst, len);
        rebuild_array_layout(new_arr);
        // Sort the copy in-place
        js_array_sort_with_comparator(new_arr, comparator);
        new_arr
    }
}

/// `arr.toSpliced(start, deleteCount, ...items)` — return a new array with splice applied (immutable)
#[no_mangle]
pub extern "C" fn js_array_to_spliced(
    arr: *const ArrayHeader,
    start: f64,
    delete_count: f64,
    items: *const f64,
    items_count: u32,
) -> *mut ArrayHeader {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return js_array_alloc(0);
    }
    unsafe {
        let len = (*arr).length as isize;
        let src = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;

        // Normalize start index (ECMA ToIntegerOrInfinity). NaN -> 0,
        // +Infinity -> len, -Infinity -> 0. Avoid `f as isize` on non-finite.
        let start_int = if start.is_nan() { 0.0 } else { start };
        let mut s: isize = if !start_int.is_finite() {
            if start_int > 0.0 {
                len
            } else {
                0
            }
        } else if start_int < 0.0 {
            (start_int + len as f64).max(0.0) as isize
        } else {
            (start_int.min(len as f64)) as isize
        };
        if s < 0 {
            s = 0;
        }
        if s > len {
            s = len;
        }

        // Normalize delete count (ECMA ToIntegerOrInfinity). NaN/undefined
        // coerce to 0; +Infinity deletes through the end.
        let dc_int = if delete_count.is_nan() {
            0.0
        } else {
            delete_count
        };
        let mut dc: isize = if !dc_int.is_finite() {
            if dc_int > 0.0 {
                len - s
            } else {
                0
            }
        } else if dc_int <= 0.0 {
            0
        } else {
            (dc_int.min((len - s) as f64)) as isize
        };
        if dc < 0 {
            dc = 0;
        }
        if dc > len - s {
            dc = len - s;
        }

        let new_len = (len - dc + items_count as isize) as usize;
        let new_arr = js_array_alloc(new_len as u32);
        (*new_arr).length = new_len as u32;
        let dst = (new_arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;

        // Copy elements before start
        // GC_STORE_AUDIT(BARRIERED): toSpliced result writes are followed by layout/barrier rebuild.
        for i in 0..s as usize {
            *dst.add(i) = *src.add(i);
        }
        // Copy inserted items
        // GC_STORE_AUDIT(BARRIERED): inserted toSpliced items are included in the rebuild below.
        for i in 0..items_count as usize {
            *dst.add(s as usize + i) = *items.add(i);
        }
        // Copy elements after deleted range
        let after_start = (s + dc) as usize;
        // GC_STORE_AUDIT(BARRIERED): toSpliced tail copy is included in the rebuild below.
        for i in after_start..len as usize {
            *dst.add(s as usize + items_count as usize + i - after_start) = *src.add(i);
        }

        rebuild_array_layout(new_arr);
        new_arr
    }
}

/// `arr.with(index, value)` — return a new array with one element replaced (immutable)
#[no_mangle]
pub extern "C" fn js_array_with(
    arr: *const ArrayHeader,
    index: f64,
    value: f64,
) -> *mut ArrayHeader {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return js_array_alloc(0);
    }
    if crate::typedarray::lookup_typed_array_kind(arr as usize).is_some() {
        return crate::typedarray::js_typed_array_with(
            arr as *const crate::typedarray::TypedArrayHeader,
            index,
            value,
        ) as *mut ArrayHeader;
    }
    unsafe {
        let len = (*arr).length as isize;
        // ECMA ToIntegerOrInfinity: NaN coerces to 0, ±Infinity stay infinite.
        // Resolve the relative index against `len`; any out-of-range or
        // non-finite index throws RangeError (Node parity, #2792).
        let rel = if index.is_nan() { 0.0 } else { index };
        // Reject non-finite indexes (Infinity / -Infinity) — always OOB.
        if !rel.is_finite() {
            throw_invalid_index(index);
        }
        let resolved = if rel < 0.0 { rel + len as f64 } else { rel };
        if resolved < 0.0 || resolved >= len as f64 {
            throw_invalid_index(index);
        }
        let idx = resolved as isize;
        let src = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
        let new_arr = js_array_alloc(len as u32);
        (*new_arr).length = len as u32;
        let dst = (new_arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
        // GC_STORE_AUDIT(BARRIERED): with() clone and replacement are followed by layout/barrier rebuild.
        std::ptr::copy_nonoverlapping(src, dst, len as usize);
        *dst.add(idx as usize) = value;
        rebuild_array_layout(new_arr);
        new_arr
    }
}

/// `arr.copyWithin(target, start, end?)` — copy a sequence of elements within the array (in-place)
#[no_mangle]
pub extern "C" fn js_array_copy_within(
    arr: *mut ArrayHeader,
    target: f64,
    start: f64,
    has_end: i32,
    end: f64,
) -> *mut ArrayHeader {
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        return arr;
    }
    unsafe {
        let len = (*arr).length as isize;
        let elements = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;

        // Normalize target
        let mut t = target as isize;
        if t < 0 {
            t += len;
        }
        if t < 0 {
            t = 0;
        }

        // Normalize start
        let mut s = start as isize;
        if s < 0 {
            s += len;
        }
        if s < 0 {
            s = 0;
        }

        // Normalize end
        let mut e = if has_end != 0 { end as isize } else { len };
        if e < 0 {
            e += len;
        }
        if e < 0 {
            e = 0;
        }
        if e > len {
            e = len;
        }

        let count = (e - s).min(len - t);
        if count <= 0 {
            return arr;
        }

        // Use memmove semantics (handles overlapping regions)
        // GC_STORE_AUDIT(BARRIERED): copyWithin mutates array slots and rebuilds layout/barriers below.
        std::ptr::copy(
            elements.add(s as usize),
            elements.add(t as usize),
            count as usize,
        );
        rebuild_array_layout(arr);
        arr
    }
}
