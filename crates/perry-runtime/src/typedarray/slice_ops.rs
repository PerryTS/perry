//! `%TypedArray%.prototype` join / slice / reverse / fill / subarray (#3148).
//! Split out of `typedarray/mod.rs`.

use super::*;

use std::ptr;

// #3148: %TypedArray%.prototype join / slice / reverse / fill / subarray.
// (reduce/reduceRight/copyWithin/set_from/findIndex live elsewhere — added separately.)
/// `ta.join(sep?)` — Number→String each element (Node formatting), joined by
/// `sep` (default ","). Returns a heap StringHeader.
#[no_mangle]
pub extern "C" fn js_typed_array_join(
    ta: *const TypedArrayHeader,
    separator: *const crate::string::StringHeader,
) -> *mut crate::string::StringHeader {
    use crate::string::{js_string_from_bytes, StringHeader};
    let ta = clean_ta_ptr(ta);
    if ta.is_null() {
        return js_string_from_bytes(b"".as_ptr(), 0);
    }
    unsafe {
        let len = (*ta).length as usize;
        if len == 0 {
            return js_string_from_bytes(ptr::null(), 0);
        }
        let kind = (*ta).kind;
        let sep_str = if separator.is_null() {
            ","
        } else {
            let sep_len = (*separator).byte_len as usize;
            let sep_data = (separator as *const u8).add(std::mem::size_of::<StringHeader>());
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(sep_data, sep_len))
        };
        let mut result = String::new();
        for i in 0..len {
            if i > 0 {
                result.push_str(sep_str);
            }
            result.push_str(&super::format::format_typed_value(
                kind,
                load_at(ta, i),
                false,
            ));
        }
        let ret = js_string_from_bytes(result.as_ptr(), result.len() as u32);
        std::hint::black_box(&result);
        drop(result);
        ret
    }
}

/// `ta.join(sepValue)` — NaN-boxed-separator entry point mirroring
/// `js_array_join_value`.
#[no_mangle]
pub extern "C" fn js_typed_array_join_value(
    ta: *const TypedArrayHeader,
    separator_value: f64,
) -> *mut crate::string::StringHeader {
    let separator = if separator_value.to_bits() == crate::value::TAG_UNDEFINED {
        ptr::null()
    } else {
        // `ToString(separator)`: a Symbol separator is a TypeError (§7.1.17),
        // not a "Symbol(…)" rendering.
        if unsafe { crate::symbol::js_is_symbol(separator_value) } != 0 {
            throw_type_error(b"Cannot convert a Symbol value to a string");
        }
        crate::value::js_jsvalue_to_string(separator_value) as *const crate::string::StringHeader
    };
    js_typed_array_join(ta, separator)
}

/// `ta.slice(start, end?)` — returns a NEW same-kind TypedArray with the
/// selected elements. Mirrors `js_array_slice` index normalization.
#[no_mangle]
pub extern "C" fn js_typed_array_slice(
    ta: *const TypedArrayHeader,
    start: i32,
    end: i32,
) -> *mut TypedArrayHeader {
    let ta = clean_ta_ptr(ta);
    if ta.is_null() {
        return typed_array_alloc(KIND_FLOAT64, 0);
    }
    unsafe {
        let kind = (*ta).kind;
        let len = (*ta).length as i32;
        let start_idx = if start < 0 {
            (len + start).max(0) as u32
        } else {
            (start as u32).min(len as u32)
        };
        let end_idx = if end == i32::MAX {
            len as u32
        } else if end < 0 {
            (len + end).max(0) as u32
        } else {
            (end as u32).min(len as u32)
        };
        let slice_len = end_idx.saturating_sub(start_idx);
        // 23.2.3.27 step 10: A = TypedArraySpeciesCreate(O, « count »).
        let choice = species::species_constructor(ta as usize, kind);
        let result = species::species_create_length(&choice, kind, slice_len as usize);
        if slice_len > 0 {
            if let species::SpeciesChoice::Default = choice {
                // Fast same-kind path: raw byte-copy preserves exact element
                // bits — e.g. Float NaN payloads (`slice/bit-precision`), which
                // a load→f64→store round-trip would canonicalize.
                let out = species::result_as_ptr(result);
                let esz = elem_size_for_kind(kind);
                let src = (data_ptr(ta) as *const u8).add(start_idx as usize * esz);
                let dst = data_ptr_mut(out);
                ptr::copy_nonoverlapping(src, dst, slice_len as usize * esz);
            } else {
                species::copy_range_into(result, ta, start_idx as usize, slice_len as usize);
            }
        }
        species::result_as_ptr(result)
    }
}

/// `ta.reverse()` — in-place reversal; returns the same typed array.
#[no_mangle]
pub extern "C" fn js_typed_array_reverse(ta: *mut TypedArrayHeader) -> *mut TypedArrayHeader {
    let ta = clean_ta_ptr(ta as *const TypedArrayHeader) as *mut TypedArrayHeader;
    if ta.is_null() {
        return ta;
    }
    unsafe {
        let len = (*ta).length as usize;
        if len <= 1 {
            return ta;
        }
        let mut i = 0usize;
        let mut j = len - 1;
        while i < j {
            let a = load_at(ta, i);
            let b = load_at(ta, j);
            store_at(ta, i, b);
            store_at(ta, j, a);
            i += 1;
            j -= 1;
        }
        ta
    }
}

/// `ta.fill(value, start?, end?)` — in-place fill; returns the same typed
/// array. `start`/`end` follow Array.prototype.fill index normalization; pass
/// `has_start == 0` to fill the whole array.
#[no_mangle]
pub extern "C" fn js_typed_array_fill(
    ta: *mut TypedArrayHeader,
    value: f64,
    has_start: i32,
    start: f64,
    has_end: i32,
    end: f64,
) -> *mut TypedArrayHeader {
    let ta = clean_ta_ptr(ta as *const TypedArrayHeader) as *mut TypedArrayHeader;
    if ta.is_null() {
        return ta;
    }
    unsafe {
        let len = (*ta).length as isize;
        // Spec order: convert `value` first (its `valueOf`/`ToBigInt` runs before
        // the index args are coerced), then `ToIntegerOrInfinity` each index.
        let v = bigint::coerce_for_kind((*ta).kind, value);
        // `ToIntegerOrInfinity` + RelativeIndex clamp. `jsvalue_to_f64` performs
        // `ToNumber` (so `null` → 0, `true` → 1, an object → its `valueOf`, a
        // numeric string → its value); `NaN`/`undefined` → 0, ±Infinity saturate
        // to the array bounds. The previous `x.is_nan() ? default : x as isize`
        // mis-handled every NaN-boxed non-number: `null`/`false`/`undefined` all
        // looked like `NaN` and fell back to the *default* (so a `null` end
        // became `len` instead of 0).
        let rel = |x: f64| -> isize {
            let n = jsvalue_to_f64(x);
            let n = if n.is_nan() { 0.0 } else { n };
            let mut idx = if !n.is_finite() {
                if n > 0.0 {
                    len
                } else {
                    0
                }
            } else {
                n.trunc() as isize
            };
            if idx < 0 {
                idx += len;
            }
            idx.clamp(0, len)
        };
        let is_undef = |x: f64| crate::value::JSValue::from_bits(x.to_bits()).is_undefined();
        let s = if has_start != 0 { rel(start) } else { 0 };
        // An explicit `undefined` end defaults to `len` (spec step 8a), unlike a
        // `null`/absent-coerced end which is `ToIntegerOrInfinity(null)` = 0.
        let e = if has_end != 0 && !is_undef(end) {
            rel(end)
        } else {
            len
        };
        let mut i = s;
        while i < e {
            store_at(ta, i as usize, v);
            i += 1;
        }
        ta
    }
}

/// `ta.subarray(begin?, end?)` — returns a NEW same-kind TypedArray that
/// COPIES the selected range. (Perry materializes rather than aliasing the
/// backing store; observationally identical for reads and independent writes
/// of the common cases #3148 targets.)
#[no_mangle]
pub extern "C" fn js_typed_array_subarray(
    ta: *const TypedArrayHeader,
    has_begin: i32,
    begin: f64,
    has_end: i32,
    end: f64,
) -> *mut TypedArrayHeader {
    let ta = clean_ta_ptr(ta);
    if ta.is_null() || lookup_typed_array_kind(ta as usize).is_none() {
        return typed_array_alloc(KIND_FLOAT64, 0);
    }
    unsafe {
        let kind = (*ta).kind;
        let len = (*ta).length as i32;
        // `ToIntegerOrInfinity` + RelativeIndex clamp. `js_number_coerce`
        // performs `ToNumber` (running a `valueOf`/`Symbol.toPrimitive`, which
        // may throw) — done BEFORE the species lookup, per spec order.
        let norm = |has: i32, v: f64, default: i32| -> i32 {
            // Absent OR explicit `undefined` → the default (begin→0, end→len).
            if has == 0 || crate::value::JSValue::from_bits(v.to_bits()).is_undefined() {
                return default;
            }
            let n = crate::builtins::js_number_coerce(v);
            if n.is_nan() {
                return 0;
            }
            let mut x = if !n.is_finite() {
                if n > 0.0 {
                    len
                } else {
                    i32::MIN
                }
            } else {
                n.trunc() as i32
            };
            if x < 0 {
                x = x.saturating_add(len);
            }
            x.clamp(0, len)
        };
        let b = norm(has_begin, begin, 0);
        let e = norm(has_end, end, len);
        let count = (e - b).max(0) as u32;
        // 23.2.3.30: SpeciesCreate(O, « buffer, beginByteOffset, newLength »).
        // A subarray is a VIEW sharing the backing buffer (default and custom).
        let choice = species::species_constructor(ta as usize, kind);
        let elem = elem_size_for_kind(kind) as u32;
        let buffer = crate::typedarray_view::js_typed_array_backing_buffer(ta);
        let byte_offset =
            crate::typedarray_view::js_typed_array_byte_offset(ta) + (b as u32) * elem;
        let buffer_val = crate::value::js_nanbox_pointer(buffer as i64);
        let off_val = byte_offset as f64;
        let len_val = count as f64;
        match choice {
            species::SpeciesChoice::Default => crate::typedarray_view::js_typed_array_view(
                kind as i32,
                buffer_val,
                off_val,
                len_val,
            ),
            species::SpeciesChoice::Custom(c) => {
                let result = species::species_create_args(c, &[buffer_val, off_val, len_val]);
                species::result_as_ptr(result)
            }
        }
    }
}
