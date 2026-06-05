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
    // #3148: TypedArray receiver — copy over element-typed storage. The typed
    // impl treats an undefined `end` as "to length", so pass TAG_UNDEFINED
    // when no end argument was provided.
    if crate::typedarray::lookup_typed_array_kind(arr as usize).is_some() {
        let end_value = if has_end != 0 {
            end
        } else {
            f64::from_bits(crate::value::TAG_UNDEFINED)
        };
        return crate::typedarray::js_typed_array_copy_within(
            arr as *mut crate::typedarray::TypedArrayHeader,
            target,
            start,
            end_value,
        ) as *mut ArrayHeader;
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

#[cold]
fn throw_copy_within_type_error(message: &[u8]) -> ! {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

fn copy_within_to_integer_or_infinity(value: f64) -> f64 {
    let number = crate::builtins::js_number_coerce(value);
    if number.is_nan() || number == 0.0 {
        0.0
    } else if number.is_infinite() {
        number
    } else {
        number.trunc()
    }
}

fn copy_within_to_length(value: f64) -> u32 {
    let number = crate::builtins::js_number_coerce(value);
    if number.is_nan() || number <= 0.0 {
        0
    } else if number.is_infinite() {
        if number.is_sign_positive() {
            u32::MAX
        } else {
            0
        }
    } else {
        number.trunc().min(u32::MAX as f64) as u32
    }
}

fn copy_within_relative_index(value: f64, len: i64) -> i64 {
    let integer = copy_within_to_integer_or_infinity(value);
    if integer == f64::NEG_INFINITY {
        0
    } else if integer < 0.0 {
        (len as f64 + integer).max(0.0) as i64
    } else {
        integer.min(len as f64) as i64
    }
}

fn copy_within_length_of_array_like(receiver: f64) -> u32 {
    let length = unsafe {
        crate::value::js_dynamic_object_get_property(receiver, b"length".as_ptr() as *const i8, 6)
    };
    copy_within_to_length(length)
}

fn copy_within_has_property(receiver: f64, index: i64) -> bool {
    crate::value::js_is_truthy(crate::object::js_object_has_own(receiver, index as f64)) != 0
}

/// Generic `Array.prototype.copyWithin.call(arrayLike, target, start?, end?)`.
///
/// This keeps the original `this` value rather than materializing an Array:
/// the spec mutates the receiver's indexed properties and returns that same
/// receiver after ToObject coercion.
#[no_mangle]
pub extern "C" fn js_array_copy_within_value(
    receiver: f64,
    target: f64,
    start: f64,
    has_end: i32,
    end: f64,
) -> f64 {
    let receiver_value = crate::value::JSValue::from_bits(receiver.to_bits());
    if receiver_value.is_null() || receiver_value.is_undefined() {
        throw_copy_within_type_error(b"Cannot convert undefined or null to object");
    }

    let receiver = crate::object::js_object_coerce(receiver);
    let len = copy_within_length_of_array_like(receiver) as i64;
    let to = copy_within_relative_index(target, len);
    let from = copy_within_relative_index(start, len);
    let final_index = if has_end != 0 {
        copy_within_relative_index(end, len)
    } else {
        len
    };
    let count = (final_index - from).min(len - to).max(0);
    if count <= 0 {
        return receiver;
    }

    if crate::builtins::boxed_primitive_to_string_tag(receiver) == Some("String") {
        throw_copy_within_type_error(b"Cannot assign to read only property");
    }

    let mut from_idx = from;
    let mut to_idx = to;
    let direction = if from < to && to < from + count {
        from_idx += count - 1;
        to_idx += count - 1;
        -1
    } else {
        1
    };

    let receiver_bits = receiver.to_bits() as i64;
    for _ in 0..count {
        if copy_within_has_property(receiver, from_idx) {
            let value =
                crate::object::js_object_get_index_polymorphic(receiver_bits, from_idx as f64);
            crate::object::js_object_set_index_polymorphic(receiver_bits, to_idx as f64, value);
        } else {
            let obj = unsafe { crate::object::extract_obj_ptr(receiver) };
            if !obj.is_null() && crate::object::js_object_delete_dynamic(obj, to_idx as f64) == 0 {
                throw_copy_within_type_error(b"Cannot delete property");
            }
        }
        from_idx += direction;
        to_idx += direction;
    }

    receiver
}
