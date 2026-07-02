//! concat / reverse / fill.
use super::*;
use crate::JSValue;
use std::ptr;

fn fill_to_number(value: f64) -> f64 {
    let jsval = JSValue::from_bits(value.to_bits());
    if jsval.is_any_string() {
        let ptr = crate::value::js_get_string_pointer_unified(value) as *const crate::StringHeader;
        if ptr.is_null() || (ptr as usize) < 0x1000 {
            return f64::NAN;
        }
        let s = crate::string::string_as_str(ptr).trim();
        if s.is_empty() {
            0.0
        } else {
            s.parse::<f64>().unwrap_or(f64::NAN)
        }
    } else {
        jsval.to_number()
    }
}

fn fill_to_length(value: f64) -> i64 {
    // `ToLength` clamps to 2^53-1 (NOT u32::MAX — an array-like with
    // `length: Number.MAX_SAFE_INTEGER` must keep its near-limit indices
    // addressable; test262 fill/length-near-integer-limit).
    const MAX_LENGTH: f64 = 9_007_199_254_740_991.0;
    // Real ToNumber: fires `valueOf` on an object-valued length and throws
    // TypeError for Symbol/BigInt (fill/return-abrupt-from-this-length*).
    let number = crate::builtins::js_number_coerce(value);
    if number.is_nan() || number <= 0.0 {
        0
    } else if !number.is_finite() || number > MAX_LENGTH {
        MAX_LENGTH as i64
    } else {
        number as i64
    }
}

fn fill_relative_index(value: f64, len: i64, default_value: i64) -> i64 {
    let jsval = JSValue::from_bits(value.to_bits());
    if jsval.is_undefined() {
        return default_value;
    }
    let number = fill_to_number(value);
    if number.is_nan() {
        return 0;
    }
    let mut index = if number.is_infinite() {
        if number > 0.0 {
            len
        } else {
            -len
        }
    } else {
        number as i64
    };
    if index < 0 {
        index += len;
        if index < 0 {
            return 0;
        }
    }
    if index > len {
        len
    } else {
        index
    }
}

fn throw_fill_nullish_receiver() -> ! {
    crate::collection_iter::throw_type_error("Cannot convert undefined or null to object")
}

/// Append all elements from source array to destination array
/// Returns the (possibly reallocated) destination array pointer
#[no_mangle]
pub extern "C" fn js_array_concat(
    dest: *mut ArrayHeader,
    src: *const ArrayHeader,
) -> *mut ArrayHeader {
    let src = clean_arr_ptr(src);
    if src.is_null() {
        return dest;
    }
    // Detect non-array sources: Sets register themselves in
    // SET_REGISTRY; convert to array first so spread-into-array
    // `[...new Set(...)]` reads the right elements instead of the
    // SetHeader's raw memory.
    if crate::set::is_registered_set(src as usize) {
        let arr = crate::set::js_set_to_array(src as *const crate::set::SetHeader);
        return js_array_concat(dest, arr);
    }
    // Same treatment for Maps — `[...map]` materializes [key, value]
    // pair Arrays. Without this branch, the loop below reads the
    // MapHeader's `size` field as `length` and pulls keys/values out of
    // the wrong offsets, producing garbage f64s (issue #540). The
    // companion `Array.from(map)` path goes through `js_array_clone`
    // which already has the matching Map arm.
    if crate::map::is_registered_map(src as usize) {
        let arr = crate::map::js_map_entries(src as *const crate::map::MapHeader);
        return js_array_concat(dest, arr);
    }
    // Issue #578: typed-array source — materialize through the per-kind
    // accessor so `[...new Uint8Array([1,2,3])]` and `arr.concat(typedArr)`
    // see the byte values, not the byte buffer reinterpreted as f64.
    if crate::typedarray::lookup_typed_array_kind(src as usize).is_some() {
        let arr = crate::typedarray::typed_array_to_array(
            src as *const crate::typedarray::TypedArrayHeader,
        );
        return js_array_concat(dest, arr);
    }
    // Uint8Array (legacy Buffer-backed) source — materialize byte values.
    if crate::buffer::is_registered_buffer(src as usize) {
        let arr = crate::buffer::buffer_to_array(src as *const crate::buffer::BufferHeader);
        return js_array_concat(dest, arr);
    }
    unsafe {
        let src_len = (*src).length;
        if src_len == 0 {
            return dest;
        }

        let src_elements = (src as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;

        // Bulk-copy fast path: pre-grow once to fit dest_len+src_len,
        // then memcpy the source elements into the dest tail and update
        // length once. Replaces N individual `js_array_push_f64` calls
        // (each doing a forwarding-chain follow + capacity check). The
        // alias case (dest == src) is rare but possible — fall back to
        // the per-element loop for that, since growing dest invalidates
        // the src_elements pointer.
        let dest_resolved = clean_arr_ptr_mut(dest);
        if !dest_resolved.is_null() && !std::ptr::eq(dest_resolved, src) {
            let dest_len = (*dest_resolved).length;
            let new_len = dest_len + src_len;
            let result = if new_len > (*dest_resolved).capacity {
                js_array_grow(dest_resolved, new_len)
            } else {
                dest_resolved
            };
            let dst_elements =
                (result as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
            // GC_STORE_AUDIT(BARRIERED): concat bulk copy is followed by exact layout/barrier rebuild.
            ptr::copy_nonoverlapping(
                src_elements,
                dst_elements.add(dest_len as usize),
                src_len as usize,
            );
            (*result).length = new_len;
            rebuild_array_layout_exact(result);
            return result;
        }

        // Fallback: per-element push (handles aliasing + null dest).
        let mut result = dest;
        for i in 0..src_len as usize {
            let element = *src_elements.add(i);
            result = js_array_push_f64(result, element);
        }
        result
    }
}

/// JS-semantic `Array.prototype.concat`: returns a NEW array with the
/// elements of both `arr` and `other`. Neither input is mutated. This is
/// what users get when they call `a.concat(b)`. `js_array_concat` above
/// mutates its first argument and is reserved for the internal
/// push-spread desugaring path.
#[no_mangle]
pub extern "C" fn js_array_concat_new(
    arr: *const ArrayHeader,
    other: *const ArrayHeader,
) -> *mut ArrayHeader {
    let a = clean_arr_ptr(arr);
    let b = clean_arr_ptr(other);
    unsafe {
        let a_len = if a.is_null() { 0 } else { (*a).length };
        let b_len = if b.is_null() { 0 } else { (*b).length };
        let total = a_len + b_len;

        let mut result = js_array_alloc(total);
        if !a.is_null() && a_len > 0 {
            let src = (a as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
            for i in 0..a_len as usize {
                result = js_array_push_f64(result, *src.add(i));
            }
        }
        if !b.is_null() && b_len > 0 {
            let src = (b as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
            for i in 0..b_len as usize {
                result = js_array_push_f64(result, *src.add(i));
            }
        }
        result
    }
}

/// `Array.prototype.reverse` — reverses in place and returns the same pointer.
#[no_mangle]
pub extern "C" fn js_array_reverse(arr: *mut ArrayHeader) -> *mut ArrayHeader {
    // Borrowed array-like receiver (`obj.reverse = Array.prototype.reverse;
    // obj.reverse()`): the thunk hands this dense helper the plain object
    // pointer. `js_array_reverse_value` already implements the spec-generic
    // (live `Get`/`Set`/`Delete`) reversal, so delegate and return the original
    // pointer (reverse returns its receiver).
    if crate::array::plain_object_value(arr).is_some() {
        let recv = f64::from_bits(crate::value::JSValue::pointer(arr as *const u8).bits());
        crate::array::js_array_reverse_value(recv);
        return arr;
    }
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        return arr;
    }
    // #3148: TypedArray receiver — reverse over element-typed storage.
    if crate::typedarray::lookup_typed_array_kind(arr as usize).is_some() {
        return crate::typedarray::js_typed_array_reverse(
            arr as *mut crate::typedarray::TypedArrayHeader,
        ) as *mut ArrayHeader;
    }
    unsafe {
        let len = (*arr).length as usize;
        if len <= 1 {
            return arr;
        }
        let elements = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
        let mut i = 0usize;
        let mut j = len - 1;
        while i < j {
            let tmp = *elements.add(i);
            // GC_STORE_AUDIT(BARRIERED): reverse slot swap is followed by layout/barrier rebuild.
            *elements.add(i) = *elements.add(j);
            *elements.add(j) = tmp;
            i += 1;
            j -= 1;
        }
        rebuild_array_layout(arr);
        arr
    }
}

/// `Array.prototype.reverse.call(value)` for generic array-like receivers.
#[no_mangle]
pub extern "C" fn js_array_reverse_value(receiver: f64) -> f64 {
    let receiver_js = crate::value::JSValue::from_bits(receiver.to_bits());
    if receiver_js.is_null() || receiver_js.is_undefined() {
        reverse_throw_type_error(b"Cannot convert undefined or null to object");
    }

    let object = crate::object::js_object_coerce(receiver);
    let len = reverse_length_of_array_like(object);
    if reverse_is_boxed_string(object) && len > 1 {
        reverse_throw_type_error(b"Cannot assign to read only property");
    }
    if len <= 1 {
        return object;
    }

    let handle = object.to_bits() as i64;
    let object_ptr =
        crate::value::js_nanbox_get_pointer(object) as *mut crate::object::ObjectHeader;
    let mut lower = 0u64;
    let mut upper = len - 1;
    while lower < upper {
        let lower_exists = reverse_has_own_index(object, lower);
        let upper_exists = reverse_has_own_index(object, upper);
        let lower_value = if lower_exists {
            crate::object::js_object_get_index_polymorphic(handle, lower as f64)
        } else {
            f64::from_bits(crate::value::TAG_UNDEFINED)
        };
        let upper_value = if upper_exists {
            crate::object::js_object_get_index_polymorphic(handle, upper as f64)
        } else {
            f64::from_bits(crate::value::TAG_UNDEFINED)
        };

        match (lower_exists, upper_exists) {
            (true, true) => {
                crate::object::js_object_set_index_polymorphic(handle, lower as f64, upper_value);
                crate::object::js_object_set_index_polymorphic(handle, upper as f64, lower_value);
            }
            (false, true) => {
                crate::object::js_object_set_index_polymorphic(handle, lower as f64, upper_value);
                reverse_delete_index(object_ptr, upper);
            }
            (true, false) => {
                reverse_delete_index(object_ptr, lower);
                crate::object::js_object_set_index_polymorphic(handle, upper as f64, lower_value);
            }
            (false, false) => {}
        }

        lower += 1;
        upper -= 1;
    }
    object
}

fn reverse_length_of_array_like(object: f64) -> u64 {
    let key = crate::string::js_string_from_bytes(b"length".as_ptr(), 6);
    let object_ptr =
        crate::value::js_nanbox_get_pointer(object) as *const crate::object::ObjectHeader;
    if object_ptr.is_null() {
        return 0;
    }
    reverse_to_length(crate::object::js_object_get_field_by_name_f64(
        object_ptr, key,
    ))
}

fn reverse_to_length(value: f64) -> u64 {
    let n = crate::builtins::js_number_coerce(value);
    if n.is_nan() || n <= 0.0 {
        0
    } else if n.is_infinite() || n >= (1u64 << 53) as f64 {
        (1u64 << 53) - 1
    } else {
        n.trunc() as u64
    }
}

fn reverse_is_boxed_string(object: f64) -> bool {
    crate::builtins::boxed_primitive_to_string_tag(object) == Some("String")
}

fn reverse_has_own_index(object: f64, index: u64) -> bool {
    let present = crate::object::js_object_has_own(object, index as f64);
    crate::value::js_is_truthy(present) != 0
}

fn reverse_delete_index(obj: *mut crate::object::ObjectHeader, index: u64) {
    if crate::object::js_object_delete_dynamic(obj, index as f64) == 0 {
        reverse_throw_type_error(b"Cannot delete property");
    }
}

fn reverse_throw_type_error(message: &[u8]) -> ! {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

/// `Array.prototype.fill(value)` — fills every element (0..length) with
/// `value`. Returns the same array pointer.
#[no_mangle]
pub extern "C" fn js_array_fill(arr: *mut ArrayHeader, value: f64) -> *mut ArrayHeader {
    // #5552: `fill` writes the same source into every slot — a uniquely-owned
    // (refcount==1) string would alias the source AND every filled slot to each
    // other, so a later `s += x` corrupts them all. Demote once to shared (no-op
    // for SSO / non-string; mirrors `js_array_push_f64`, #5548).
    crate::string::js_string_addref_if_heap_string(value);
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        return arr;
    }
    // #3148: TypedArray receiver — fill the whole array, element-typed.
    if crate::typedarray::lookup_typed_array_kind(arr as usize).is_some() {
        return crate::typedarray::js_typed_array_fill(
            arr as *mut crate::typedarray::TypedArrayHeader,
            value,
            0,
            0.0,
            0,
            0.0,
        ) as *mut ArrayHeader;
    }
    unsafe {
        let len = (*arr).length as usize;
        if len == 0 {
            return arr;
        }
        let elements = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
        for i in 0..len {
            // GC_STORE_AUDIT(BARRIERED): fill slot writes are followed by layout/barrier rebuild.
            *elements.add(i) = value;
        }
        rebuild_array_layout(arr);
        arr
    }
}

/// `Array.prototype.fill(value, start, end)` — fills the index range
/// `[start, end)` with `value`. Per ECMA-262: negative indices count from
/// the end (`len + idx`), then are clamped to `[0, len]`. `end > len`
/// clamps to `len`, `start > end` yields no-op. Returns the same array.
#[no_mangle]
pub extern "C" fn js_array_fill_range(
    arr: *mut ArrayHeader,
    value: f64,
    start: f64,
    end: f64,
) -> *mut ArrayHeader {
    // #5552: demote a uniquely-owned source string once before it fills the
    // range (no-op for SSO / non-string). See `js_array_fill`.
    crate::string::js_string_addref_if_heap_string(value);
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        return arr;
    }
    // #3148: TypedArray receiver — fill [start, end) over element-typed storage.
    if crate::typedarray::lookup_typed_array_kind(arr as usize).is_some() {
        return crate::typedarray::js_typed_array_fill(
            arr as *mut crate::typedarray::TypedArrayHeader,
            value,
            1,
            start,
            1,
            end,
        ) as *mut ArrayHeader;
    }
    // ECMA-262 §23.1.3.6: ToIntegerOrInfinity(start) then (end) run BEFORE the
    // length==0 early-out, and each fires `valueOf` / `Symbol.toPrimitive`
    // (propagating abrupt completions — test262 fill/return-abrupt-from-start/
    // end). The previous `idx.is_nan()` clamp silently mapped a NaN-boxed
    // object argument to 0 and never threw. The default-end sentinel
    // (+Infinity from codegen) is a real f64 and survives coercion unchanged.
    // An explicit `undefined` end means "to length" (step 7) — coercing it
    // gave NaN → 0 → no-op (`[0, 0].fill(1, 0, undefined)` stayed [0, 0],
    // test262 fill/coerced-indexes).
    let start = crate::builtins::js_to_integer_or_infinity(start);
    let end = if end.to_bits() == crate::value::TAG_UNDEFINED {
        f64::INFINITY
    } else {
        crate::builtins::js_to_integer_or_infinity(end)
    };
    unsafe {
        let len = (*arr).length as i64;
        if len == 0 {
            return arr;
        }
        let clamp = |idx: f64, _default_to_len: bool| -> i64 {
            if idx.is_nan() {
                return 0;
            }
            let mut i = idx as i64;
            if idx.is_infinite() {
                // ToIntegerOrInfinity: +∞ → len, -∞ → 0 (for BOTH start and
                // end — `fill(1, 0, -Infinity)` fills nothing; the absent-end
                // codegen sentinel is +∞ and clamps to len above).
                if idx > 0.0 {
                    return len;
                }
                return 0;
            }
            if i < 0 {
                i += len;
                if i < 0 {
                    i = 0;
                }
            }
            if i > len {
                i = len;
            }
            i
        };
        let s = clamp(start, false);
        let e = clamp(end, true);
        if s >= e {
            return arr;
        }
        let elements = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
        for i in s..e {
            // GC_STORE_AUDIT(BARRIERED): fill range writes are followed by layout/barrier rebuild.
            *elements.add(i as usize) = value;
        }
        rebuild_array_layout(arr);
        arr
    }
}

#[no_mangle]
pub extern "C" fn js_array_fill_generic(
    receiver: f64,
    value: f64,
    has_start: i32,
    start: f64,
    has_end: i32,
    end: f64,
) -> f64 {
    // #5552: demote a uniquely-owned source string once before any slot write.
    // The array receiver delegates to `js_array_fill`/`_range` (which also
    // demote — idempotent), but the generic object-receiver loop below writes
    // `value` into every index directly, so the demote must happen here too.
    crate::string::js_string_addref_if_heap_string(value);
    let receiver_value = JSValue::from_bits(receiver.to_bits());
    if receiver_value.is_null() || receiver_value.is_undefined() {
        throw_fill_nullish_receiver();
    }

    if receiver_value.is_pointer() {
        let raw = (receiver.to_bits() & crate::value::POINTER_MASK) as usize;
        if raw >= crate::gc::GC_HEADER_SIZE + 0x1000 {
            let obj_type = unsafe {
                let hdr =
                    (raw as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
                (*hdr).obj_type
            };
            if obj_type == crate::gc::GC_TYPE_ARRAY || obj_type == crate::gc::GC_TYPE_LAZY_ARRAY {
                let arr = raw as *mut ArrayHeader;
                let result = if has_start != 0 || has_end != 0 {
                    let start_value = if has_start != 0 { start } else { 0.0 };
                    let end_value =
                        if has_end != 0 && !JSValue::from_bits(end.to_bits()).is_undefined() {
                            end
                        } else {
                            f64::INFINITY
                        };
                    js_array_fill_range(arr, value, start_value, end_value)
                } else {
                    js_array_fill(arr, value)
                };
                return f64::from_bits(JSValue::pointer(result as *mut u8).bits());
            }
            if obj_type == crate::gc::GC_TYPE_OBJECT || obj_type == crate::gc::GC_TYPE_CLOSURE {
                // `Get(O, "length")` — must fire an own `length` accessor
                // (propagating its throw, test262
                // fill/return-abrupt-from-this-length) and walk the prototype
                // chain; `ToLength` then fires a `valueOf` on an object-valued
                // length (return-abrupt-from-this-length-as-symbol throws in
                // the coercion).
                let length_key = crate::string::js_string_from_bytes(b"length".as_ptr(), 6);
                let key_v = f64::from_bits(JSValue::string_ptr(length_key).bits());
                let len_value =
                    unsafe { crate::object::js_object_get_property_key(receiver, key_v) };
                let len = fill_to_length(len_value);
                if len == 0 {
                    return receiver;
                }
                let start_index = if has_start != 0 {
                    fill_relative_index(start, len, 0)
                } else {
                    0
                };
                let end_index = if has_end != 0 {
                    fill_relative_index(end, len, len)
                } else {
                    len
                };
                if start_index >= end_index {
                    return receiver;
                }
                for index in start_index..end_index {
                    crate::object::js_object_set_index_polymorphic(raw as i64, index as f64, value);
                }
                return receiver;
            }
        }
    }

    let object = crate::object::js_object_coerce(receiver);
    js_array_fill_generic(object, value, has_start, start, has_end, end)
}
