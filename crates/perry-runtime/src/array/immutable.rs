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
        // toSorted reads via Get (no HasProperty skip): holes become present
        // `undefined` elements in the dense copy (ECMA-262 §23.1.3.34).
        for i in 0..len {
            let v = *src.add(i);
            *dst.add(i) = if v.to_bits() == crate::value::TAG_HOLE {
                f64::from_bits(crate::value::TAG_UNDEFINED)
            } else {
                v
            };
        }
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
        // toSorted reads via Get (no HasProperty skip): holes become present
        // `undefined` elements in the dense copy (ECMA-262 §23.1.3.34).
        for i in 0..len {
            let v = *src.add(i);
            *dst.add(i) = if v.to_bits() == crate::value::TAG_HOLE {
                f64::from_bits(crate::value::TAG_UNDEFINED)
            } else {
                v
            };
        }
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
    // #5552: the replacement `value` is stored into the new array's slot — demote
    // a uniquely-owned (refcount==1) string to shared so a later `s += x` on the
    // source local can't corrupt it. No-op for SSO / non-string. The cloned
    // elements come from `arr` and are already shared. (Mirrors #5548.)
    crate::string::js_string_addref_if_heap_string(value);
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
    // Spec order (ECMA-262 §23.1.3.4): ToIntegerOrInfinity(target), then
    // (start), then (end). Each coerces via ToNumber → fires `valueOf` /
    // `Symbol.toPrimitive` and propagates abrupt completions (test262
    // copyWithin/return-abrupt-from-target/start/end). The previous `as isize`
    // raw cast on a NaN-boxed object argument silently produced garbage and
    // never threw.
    let len_i64 = unsafe { (*arr).length as i64 };
    let t = copy_within_relative_index(target, len_i64);
    let s = copy_within_relative_index(start, len_i64);
    let e = if has_end != 0 && !crate::value::JSValue::from_bits(end.to_bits()).is_undefined() {
        copy_within_relative_index(end, len_i64)
    } else {
        len_i64
    };
    unsafe {
        // A side-effecting `valueOf` in the index coercions above may have
        // mutated the array (e.g. shrunk `length` — test262
        // copyWithin/coerced-values-start-change-*). The raw memmove below
        // would then copy stale storage; run the per-index spec loop
        // (HasProperty / Get / Set / Delete against the CURRENT state, with
        // the ORIGINAL captured len driving the bounds) instead.
        if (*arr).length as i64 != len_i64 {
            copy_within_spec_dense(arr, len_i64, t, s, e);
            return arr;
        }
        let len = len_i64 as isize;
        let (t, s, e) = (t as isize, s as isize, e as isize);
        let elements = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;

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

/// ECMA-262 §23.1.3.4 steps 11-12 over a real-array receiver whose state
/// changed during index coercion: per-index HasProperty (own + inherited) /
/// Get / Set / Delete against the live array, bounds driven by the originally
/// captured `len`.
unsafe fn copy_within_spec_dense(arr: *mut ArrayHeader, len: i64, t: i64, s: i64, e: i64) {
    let count = (e - s).min(len - t);
    if count <= 0 {
        return;
    }
    let (mut from, mut to, dir) = if s < t && t < s + count {
        (s + count - 1, t + count - 1, -1i64)
    } else {
        (s, t, 1i64)
    };
    let mut cur = arr;
    let mut remaining = count;
    while remaining > 0 {
        let from_present = from >= 0
            && from <= u32::MAX as i64
            && crate::array::array_spec_has_index(cur, from as u32);
        if from_present {
            let v = crate::array::array_spec_get(cur, from as u32);
            cur = js_array_set_f64_extend(cur, to as u32, v);
        } else if to >= 0 && to <= u32::MAX as i64 {
            crate::array::js_array_delete(cur, to as u32);
        }
        from += dir;
        to += dir;
        remaining -= 1;
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

fn copy_within_to_length(value: f64) -> i64 {
    // `ToLength` clamps to 2^53-1, not u32::MAX — an array-like with
    // `length: Number.MAX_SAFE_INTEGER` keeps near-limit indices addressable
    // (test262 copyWithin/length-near-integer-limit).
    const MAX_LENGTH: f64 = 9_007_199_254_740_991.0;
    let number = crate::builtins::js_number_coerce(value);
    if number.is_nan() || number <= 0.0 {
        0
    } else if number.is_infinite() {
        if number.is_sign_positive() {
            MAX_LENGTH as i64
        } else {
            0
        }
    } else {
        number.trunc().min(MAX_LENGTH) as i64
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

fn copy_within_length_of_array_like(receiver: f64) -> i64 {
    // A Proxy receiver resolves `length` through its `get` trap (falling back
    // to the target when untrapped) — the raw dynamic read doesn't route
    // proxies (test262 copyWithin/return-abrupt-from-has-start needs the
    // subsequent HasProperty trap to fire, which requires a real `len` here).
    let length = if crate::proxy::js_proxy_is_proxy(receiver) != 0 {
        let key = crate::string::js_string_from_bytes(b"length".as_ptr(), 6);
        let key_v = f64::from_bits(crate::value::JSValue::string_ptr(key).bits());
        crate::proxy::js_proxy_get(receiver, key_v)
    } else {
        unsafe {
            crate::value::js_dynamic_object_get_property(
                receiver,
                b"length".as_ptr() as *const i8,
                6,
            )
        }
    };
    copy_within_to_length(length)
}

fn copy_within_index_key(index: i64) -> f64 {
    let s = index.to_string();
    let key = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
    f64::from_bits(crate::value::JSValue::string_ptr(key).bits())
}

fn copy_within_has_property(receiver: f64, index: i64) -> bool {
    // `HasProperty(O, fromKey)` fires a Proxy `has` trap, propagating its
    // throw (test262 copyWithin/return-abrupt-from-has-start).
    if crate::proxy::js_proxy_is_proxy(receiver) != 0 {
        let v = crate::proxy::js_proxy_has(receiver, copy_within_index_key(index));
        return crate::value::js_is_truthy(v) != 0;
    }
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
    let len = copy_within_length_of_array_like(receiver);
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
    let is_proxy = crate::proxy::js_proxy_is_proxy(receiver) != 0;
    for _ in 0..count {
        if copy_within_has_property(receiver, from_idx) {
            let value = if is_proxy {
                crate::proxy::js_proxy_get(receiver, copy_within_index_key(from_idx))
            } else {
                crate::object::js_object_get_index_polymorphic(receiver_bits, from_idx as f64)
            };
            if is_proxy {
                crate::proxy::js_proxy_set(receiver, copy_within_index_key(to_idx), value);
            } else {
                crate::object::js_object_set_index_polymorphic(receiver_bits, to_idx as f64, value);
            }
        } else if is_proxy {
            // `DeletePropertyOrThrow` through the Proxy `deleteProperty` trap
            // (test262 copyWithin/return-abrupt-from-delete-proxy-target).
            let ok = crate::proxy::js_proxy_delete(receiver, copy_within_index_key(to_idx));
            if crate::value::js_is_truthy(ok) == 0 {
                throw_copy_within_type_error(b"Cannot delete property");
            }
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
