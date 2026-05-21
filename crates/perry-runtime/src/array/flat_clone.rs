//! flat / clone / entries / keys / values.
use super::*;
use std::ptr;

/// `Array.prototype.flat(depth)` — flatten up to `depth` levels deep
/// (ECMA-262 §23.1.3.10).
#[no_mangle]
pub extern "C" fn js_array_flat_depth(arr: *const ArrayHeader, depth: f64) -> *mut ArrayHeader {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return js_array_alloc(0);
    }
    let levels: u32 = if depth.is_nan() || depth <= 0.0 {
        0
    } else if depth.is_infinite() || depth > u32::MAX as f64 {
        u32::MAX
    } else {
        depth as u32
    };
    unsafe {
        let mut result = js_array_alloc(0);
        result = js_array_flat_into(result, arr, levels);
        result
    }
}

/// Recursive worker for `js_array_flat_depth`. Returns the (possibly
/// re-grown) `result` pointer so `js_array_push_f64`'s reallocation
/// stays in sync across recursive calls.
unsafe fn js_array_flat_into(
    mut result: *mut ArrayHeader,
    src: *const ArrayHeader,
    depth_left: u32,
) -> *mut ArrayHeader {
    let len = (*src).length as usize;
    let elements = (src as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
    for i in 0..len {
        let element = *elements.add(i);
        let bits = element.to_bits();
        let top16 = (bits >> 48) as u16;
        let maybe_arr_ptr = if top16 >= 0x7FF8 {
            if top16 == 0x7FFD {
                let ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as *const ArrayHeader;
                if (ptr as usize) >= 0x1000 {
                    Some(ptr)
                } else {
                    None
                }
            } else {
                None
            }
        } else if top16 == 0 && bits >= 0x10000 && (bits & 0x7) == 0 {
            Some(bits as *const ArrayHeader)
        } else {
            None
        };
        let mut pushed = false;
        if depth_left > 0 {
            if let Some(sub_arr) = maybe_arr_ptr {
                let is_set_or_map = crate::set::is_registered_set(sub_arr as usize)
                    || crate::map::is_registered_map(sub_arr as usize);
                if !is_set_or_map {
                    let obj_type = if (sub_arr as usize) >= crate::gc::GC_HEADER_SIZE + 0x1000 {
                        let hdr = (sub_arr as *const u8).sub(crate::gc::GC_HEADER_SIZE)
                            as *const crate::gc::GcHeader;
                        (*hdr).obj_type
                    } else {
                        0
                    };
                    if obj_type == crate::gc::GC_TYPE_ARRAY {
                        let sub_len = (*sub_arr).length as usize;
                        if sub_len <= 1_000_000 {
                            result = js_array_flat_into(result, sub_arr, depth_left - 1);
                            pushed = true;
                        }
                    }
                }
            }
        }
        if !pushed {
            result = js_array_push_f64(result, element);
        }
    }
    result
}

/// Flatten an array of arrays into a single array (depth=1).
/// For each element: if it's an array pointer (NaN-boxed with POINTER_TAG or raw pointer),
/// append all its elements; otherwise append the element directly.
#[no_mangle]
pub extern "C" fn js_array_flat(arr: *const ArrayHeader) -> *mut ArrayHeader {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return js_array_alloc(0);
    }
    unsafe {
        let len = (*arr).length as usize;
        let elements = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
        let mut result = js_array_alloc(0);

        for i in 0..len {
            let element = *elements.add(i);
            let bits = element.to_bits();
            let top16 = (bits >> 48) as u16;

            // Check if the element is an array pointer (NaN-boxed or raw)
            let maybe_arr_ptr = if top16 >= 0x7FF8 {
                // NaN-boxed value - check if it's a pointer-like tag
                if top16 == 0x7FFD {
                    // POINTER_TAG — extract raw pointer
                    let ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as *const ArrayHeader;
                    if (ptr as usize) >= 0x1000 {
                        Some(ptr)
                    } else {
                        None
                    }
                } else {
                    None // STRING_TAG, BIGINT_TAG, JS_HANDLE_TAG, undefined, NaN
                }
            } else if top16 == 0 && bits >= 0x10000 && (bits & 0x7) == 0 {
                // Raw pointer without NaN-boxing (top 16 bits zero = userspace pointer,
                // >= 64KB to exclude small integers, 8-byte aligned)
                Some(bits as *const ArrayHeader)
            } else {
                None
            };

            if let Some(sub_arr) = maybe_arr_ptr {
                // Check if it's a registered set — if so, it's not an array
                if crate::set::is_registered_set(sub_arr as usize)
                    || crate::map::is_registered_map(sub_arr as usize)
                {
                    // Not an array — push as-is
                    result = js_array_push_f64(result, element);
                } else {
                    // Try to read as array
                    let sub_len = (*sub_arr).length as usize;
                    // Sanity check: if length is unreasonably large, treat as non-array
                    if sub_len <= 1_000_000 {
                        let sub_elements = (sub_arr as *const u8)
                            .add(std::mem::size_of::<ArrayHeader>())
                            as *const f64;
                        for j in 0..sub_len {
                            result = js_array_push_f64(result, *sub_elements.add(j));
                        }
                    } else {
                        result = js_array_push_f64(result, element);
                    }
                }
            } else {
                // Not a pointer - push element directly
                result = js_array_push_f64(result, element);
            }
        }

        result
    }
}

/// Clone an array from a NaN-boxed f64 pointer value.
/// Extracts the array pointer from the NaN-boxed value and creates a shallow copy.
/// If the value is not a valid array pointer, returns an empty array.
/// Also handles Sets (via registry check) — converts Set to Array transparently.
#[no_mangle]
pub extern "C" fn js_array_clone(src: *const ArrayHeader) -> *mut ArrayHeader {
    // Strip a NaN-box tag for the registry/string checks below; the
    // raw_addr path is reused for typed-array / Buffer / string
    // detection. Plain-pointer call sites already pass a clean ptr.
    let raw_addr = if !src.is_null() {
        let bits = src as u64;
        if (bits >> 48) >= 0x7FF8 {
            (bits & 0x0000_FFFF_FFFF_FFFF) as usize
        } else {
            bits as usize
        }
    } else {
        0
    };

    // `Array.from(string)` iterates the source by Unicode codepoint
    // (each codepoint becomes a 1-char string element) per ECMA-262
    // §23.1.2.1. Pre-fix this fell through to the array memcpy path
    // and emitted garbage f64s built from the string's underlying
    // UTF-8 bytes. Detect via the canonical STRING_TAG (top16=0x7FFF)
    // OR via the GC header's obj_type byte when the receiver arrived
    // as a raw pointer (e.g. through a typed-Any local).
    let is_string_src = {
        let top16 = (src as u64) >> 48;
        if top16 == 0x7FFF {
            true
        } else if raw_addr >= crate::gc::GC_HEADER_SIZE + 0x1000 {
            unsafe {
                let hdr = (raw_addr as *const u8).sub(crate::gc::GC_HEADER_SIZE)
                    as *const crate::gc::GcHeader;
                (*hdr).obj_type == crate::gc::GC_TYPE_STRING
            }
        } else {
            false
        }
    };
    if is_string_src {
        let s_ptr = raw_addr as *const crate::string::StringHeader;
        return unsafe { js_array_from_string_codepoints(s_ptr) };
    }

    // Check if this is actually a Set (type unknown at compile time)
    if !src.is_null() && crate::set::is_registered_set(src as usize) {
        return crate::set::js_set_to_array(src as *const crate::set::SetHeader);
    }
    // Check if this is a Map (for Array.from(map) → array of [key, value] pairs)
    if !src.is_null() && crate::map::is_registered_map(src as usize) {
        return crate::map::js_map_entries(src as *const crate::map::MapHeader);
    }

    // `Array.from({length: N, 0: ..., 1: ...})` (array-like object) per
    // ECMA-262 §23.1.2.1 step 8: read `.length`, then for each index
    // 0..length read `obj[i]` (missing slots → undefined). Pre-fix this
    // fell through to the array-memcpy path which read ObjectHeader's
    // `field_count` u32 as `length` and the inline f64 slots as elements
    // — garbage. Detect via `GC_TYPE_OBJECT`.
    if raw_addr >= crate::gc::GC_HEADER_SIZE + 0x1000 {
        let obj_type = unsafe {
            let hdr = (raw_addr as *const u8).sub(crate::gc::GC_HEADER_SIZE)
                as *const crate::gc::GcHeader;
            (*hdr).obj_type
        };
        if obj_type == crate::gc::GC_TYPE_OBJECT {
            let obj = raw_addr as *const crate::ObjectHeader;
            unsafe {
                let keys_arr = (*obj).keys_array;
                if !keys_arr.is_null() && (*keys_arr).length == 1 {
                    let key0 = js_array_get_f64(keys_arr, 0);
                    let key_val = crate::value::JSValue::from_bits(key0.to_bits());
                    let is_entries_key = if key_val.is_string() {
                        let ptr = key_val.as_string_ptr();
                        let len = (*ptr).byte_len as usize;
                        let data =
                            (ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                        std::str::from_utf8(std::slice::from_raw_parts(data, len)).unwrap_or("")
                            == "_entries"
                    } else {
                        false
                    };
                    if is_entries_key {
                        let boxed = crate::url::js_url_search_params_entries_arr(
                            obj as *mut crate::ObjectHeader,
                        );
                        let bits = boxed.to_bits();
                        let ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as *mut ArrayHeader;
                        if !ptr.is_null() {
                            return ptr;
                        }
                    }
                }
            }
            return unsafe { js_array_from_arraylike(raw_addr as *const crate::ObjectHeader) };
        }
    }
    // Issue #578: typed array source — materialize each element through the
    // per-kind accessor instead of memcpy'ing the byte-packed storage as if
    // it were a flat f64 array. Without this, `Array.from(uint8array)` /
    // `[...uint8array]` / `for (const b of uint8array)` (which now wraps
    // the iterable in `Expr::ArrayFrom`) all produced raw bit reinterpretations
    // of the underlying bytes rather than the byte values themselves.
    // Strip NaN-box first so the registry lookup sees the real address.
    if !src.is_null() {
        let bits = src as u64;
        let raw_addr = if (bits >> 48) >= 0x7FF8 {
            (bits & 0x0000_FFFF_FFFF_FFFF) as usize
        } else {
            bits as usize
        };
        if crate::typedarray::lookup_typed_array_kind(raw_addr).is_some() {
            return crate::typedarray::typed_array_to_array(
                raw_addr as *const crate::typedarray::TypedArrayHeader,
            );
        }
        // Uint8Array (legacy Buffer-backed): same materialization story.
        if crate::buffer::is_registered_buffer(raw_addr) {
            return crate::buffer::buffer_to_array(raw_addr as *const crate::buffer::BufferHeader);
        }
    }
    let src = clean_arr_ptr(src);
    if src.is_null() {
        return js_array_alloc(0);
    }
    unsafe {
        let len = (*src).length;
        let result = js_array_alloc(len);
        if len > 0 {
            let src_elements =
                (src as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
            let dst_elements =
                (result as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
            ptr::copy_nonoverlapping(src_elements, dst_elements, len as usize);
            (*result).length = len;
            rebuild_array_layout_exact(result);
        }
        result
    }
}

/// `arr.entries()` — return a new array of [index, value] pairs.
/// Each pair is itself a 2-element array, NaN-boxed with POINTER_TAG so it
/// reads back as an array pointer when iterated. This eagerly materializes
/// the iterator (Perry has no generic iterator protocol yet) so a `for...of`
/// loop over the result walks it as a normal array via `length`/`arr[i]`.
#[no_mangle]
pub extern "C" fn js_array_entries(arr: *const ArrayHeader) -> *mut ArrayHeader {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return js_array_alloc(0);
    }
    unsafe {
        let len = (*arr).length;
        let result = js_array_alloc(len);
        (*result).length = len;
        let src_elements = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
        let dst_elements = (result as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
        for i in 0..len as usize {
            // Build a 2-element [index, value] pair as an inner array.
            let pair = js_array_alloc(2);
            (*pair).length = 2;
            let pair_elems = (pair as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
            *pair_elems.add(0) = i as f64;
            *pair_elems.add(1) = *src_elements.add(i);
            note_array_slot(pair, 0, (i as f64).to_bits());
            note_array_slot(pair, 1, (*src_elements.add(i)).to_bits());
            // NaN-box the inner array pointer so the outer storage slot keeps tag info.
            let pair_value = crate::value::js_nanbox_pointer(pair as i64);
            *dst_elements.add(i) = pair_value;
            note_array_slot(result, i, pair_value.to_bits());
        }
        result
    }
}

/// `arr.keys()` — return a new array of indices [0, 1, ..., length-1].
#[no_mangle]
pub extern "C" fn js_array_keys(arr: *const ArrayHeader) -> *mut ArrayHeader {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return js_array_alloc(0);
    }
    unsafe {
        let len = (*arr).length;
        let result = js_array_alloc(len);
        (*result).length = len;
        let dst_elements = (result as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
        for i in 0..len as usize {
            *dst_elements.add(i) = i as f64;
        }
        result
    }
}

/// `arr.values()` — return a shallow copy of the array.
/// (In JS this returns an iterator; Perry materializes it as a clone so
/// `for...of` over the result iterates the values eagerly.)
#[no_mangle]
pub extern "C" fn js_array_values(arr: *const ArrayHeader) -> *mut ArrayHeader {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return js_array_alloc(0);
    }
    unsafe {
        let len = (*arr).length;
        let result = js_array_alloc(len);
        if len > 0 {
            let src_elements =
                (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
            let dst_elements =
                (result as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
            ptr::copy_nonoverlapping(src_elements, dst_elements, len as usize);
            (*result).length = len;
            rebuild_array_layout(result);
        }
        result
    }
}
