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
        let len = crate::typedarray::element_length(ta) as usize;
        if len == 0 {
            return js_string_from_bytes(ptr::null(), 0);
        }
        let kind = crate::typedarray::element_kind(ta);
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
        let kind = crate::typedarray::element_kind(ta);
        let len = crate::typedarray::element_length(ta) as i32;
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
        let len = crate::typedarray::element_length(ta) as usize;
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

/// `ValidateTypedArray` for `fill`: a detached or out-of-bounds view throws.
unsafe fn validate_fill_receiver(ta: *const TypedArrayHeader) {
    let addr = ta as usize;
    if crate::buffer::is_detached_buffer(crate::buffer::store::owner(addr))
        || crate::buffer::store::out_of_bounds(addr)
    {
        throw_type_error(b"Cannot perform %TypedArray%.prototype.fill on a detached or out-of-bounds ArrayBuffer");
    }
}

/// `ta.fill(value, start?, end?)` — in-place fill; returns the same typed
/// array. Follows %TypedArray%.prototype.fill: `value` is converted once
/// (ToNumber / ToBigInt) before `start` and `end`, the range is clamped
/// against the length seen at entry, then the receiver is validated again
/// and the end is clamped to its current length. The elements are written
/// in the array's own element width with the one `store_raw` conversion.
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
        validate_fill_receiver(ta);
        let len = crate::typedarray::element_length(ta) as isize;
        let kind = crate::typedarray::element_kind(ta);
        // The conversions run user code that can collect and move objects:
        // everything used after the first one is re-read from a root.
        let scope = crate::gc::RuntimeHandleScope::new();
        let ta_root = scope.root_raw_mut_ptr(ta);
        let start_root = scope.root_nanbox_f64(start);
        let end_root = scope.root_nanbox_f64(end);
        let v_root = scope.root_nanbox_f64(bigint::coerce_for_kind(kind, value));
        // `ToIntegerOrInfinity` + RelativeIndex clamp (NaN/undefined -> 0, a
        // null is the number 0).
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
        let s = if has_start != 0 {
            rel(start_root.get_nanbox_f64())
        } else {
            0
        };
        // An explicit `undefined` end defaults to `len` (spec step 8a), unlike a
        // `null` end which is `ToIntegerOrInfinity(null)` = 0.
        let e = if has_end != 0 && !is_undef(end_root.get_nanbox_f64()) {
            rel(end_root.get_nanbox_f64())
        } else {
            len
        };
        // The conversions can run user code that detaches or resizes the buffer.
        let ta = ta_root.get_raw_mut_ptr::<TypedArrayHeader>();
        let v = v_root.get_nanbox_f64();
        validate_fill_receiver(ta);
        let e = (e as usize).min(crate::typedarray::element_length(ta) as usize);
        let s = s as usize;
        if s >= e {
            return ta;
        }
        if typed_array_has_shared_backing(ta) {
            // Shared memory keeps the per-element (atomic-width) store.
            for i in s..e {
                store_at(ta, i, v);
            }
            return ta;
        }
        let size = crate::typedarray::elem_size_for_kind(kind) as usize;
        let base = data_ptr_mut(ta).add(s * size);
        // Replicate the encoded element across one 64-bit word (the element
        // width divides 8, so every word write keeps the element phase), then
        // fill the byte range with word stores; the tail is one overlapping word.
        let mut word = element_bits(kind, v);
        let mut w = size * 8;
        while w < 64 {
            word |= word << w;
            w *= 2;
        }
        let total = (e - s) * size;
        if size == 1 {
            std::ptr::write_bytes(base, word as u8, total);
        } else if total < 8 {
            let bytes = word.to_le_bytes();
            for i in 0..total {
                *base.add(i) = bytes[i];
            }
        } else {
            let words = total / 8;
            for i in 0..words {
                (base.add(i * 8) as *mut u64).write_unaligned(word);
            }
            (base.add(total - 8) as *mut u64).write_unaligned(word);
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
        let kind = crate::typedarray::element_kind(ta);
        let len = crate::typedarray::element_length(ta) as i32;
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
        // #10873 (ES2024): a subarray of a length-tracking view taken WITHOUT an
        // `end` is itself length-tracking — it is constructed with no length.
        let end_absent =
            has_end == 0 || crate::value::JSValue::from_bits(end.to_bits()).is_undefined();
        let len_val = if end_absent && crate::typedarray_view::is_view_length_tracking(ta as usize)
        {
            f64::from_bits(crate::value::TAG_UNDEFINED)
        } else {
            count as f64
        };
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
