//! Indexing — length / element get / element set / hybrid string-or-index dispatch.
use super::*;
use std::ptr;

#[no_mangle]
pub extern "C" fn js_array_length(arr: *const ArrayHeader) -> u32 {
    if !arr.is_null() {
        if crate::set::is_registered_set(arr as usize) {
            return crate::set::js_set_size(arr as *const crate::set::SetHeader);
        }
        if crate::map::is_registered_map(arr as usize) {
            return crate::map::js_map_size(arr as *const crate::map::MapHeader);
        }
    }
    // Issue #179 Phase 2: lazy array fast path. Check BEFORE
    // `clean_arr_ptr` because that helper rejects pointers whose
    // first two u32s look implausible as (length, capacity) — and a
    // `LazyArrayHeader`'s first fields are (magic, cached_length),
    // which trip the guard. Strip the NaN-box tag manually first.
    unsafe {
        let bits = arr as u64;
        let top16 = bits >> 48;
        let raw_ptr = if top16 >= 0x7FF8 {
            if top16 == 0x7FFC {
                return 0;
            }
            (bits & 0x0000_FFFF_FFFF_FFFF) as *const ArrayHeader
        } else {
            arr
        };
        if !raw_ptr.is_null() && (raw_ptr as usize) >= crate::gc::GC_HEADER_SIZE + 0x1000 {
            let gc_header =
                (raw_ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            if (*gc_header).obj_type == crate::gc::GC_TYPE_LAZY_ARRAY {
                let lazy = raw_ptr as *const crate::json_tape::LazyArrayHeader;
                if (*lazy).magic == crate::json_tape::LAZY_ARRAY_MAGIC {
                    // If we've already materialized (e.g. an indexed
                    // access forced it), read the authoritative length
                    // from the materialized tree.
                    if !(*lazy).materialized.is_null() {
                        return (*(*lazy).materialized).length;
                    }
                    return (*lazy).cached_length;
                }
            }
        }
    }
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return 0;
    }
    unsafe { (*arr).length }
}

/// Get the length of an array (i64 bridge for perry-ui-macos)
#[no_mangle]
pub extern "C" fn js_array_get_length(arr: i64) -> i64 {
    js_array_length(arr as *const ArrayHeader) as i64
}

/// Get an element from an array by index (i64 bridge for perry-ui-macos)
#[no_mangle]
pub extern "C" fn js_array_get_element(arr: i64, index: i64) -> f64 {
    js_array_get_f64(arr as *const ArrayHeader, index as u32)
}

/// Alias for js_array_get_element (used by perry-ui-windows dialog)
#[no_mangle]
pub extern "C" fn js_array_get_element_f64(arr: i64, index: i64) -> f64 {
    js_array_get_f64(arr as *const ArrayHeader, index as u32)
}

/// Fast-path array element access: skips all polymorphic registry checks
/// (buffer, set, map). Only does bounds checking and element access.
/// Use when the codegen KNOWS the pointer is a plain Array (not Map/Set/Buffer).
#[no_mangle]
pub extern "C" fn js_array_get_f64_unchecked(arr: *const ArrayHeader, index: u32) -> f64 {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return f64::NAN;
    }
    const TAG_UNDEFINED_F64: f64 = f64::from_bits(0x7FFC_0000_0000_0001u64);
    unsafe {
        let length = (*arr).length;
        if index >= length {
            return TAG_UNDEFINED_F64;
        }
        if length > 100000 {
            return TAG_UNDEFINED_F64;
        }
        let elements_ptr = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
        let raw = *elements_ptr.add(index as usize);
        // Issue #323: translate HOLE sentinel (set by `new Array(n)`) back to
        // `undefined`. The sentinel is internal — user code only ever sees
        // TAG_UNDEFINED for unset slots.
        if raw.to_bits() == crate::value::TAG_HOLE {
            return TAG_UNDEFINED_F64;
        }
        raw
    }
}

/// Get an element from an array by index (returns f64)
#[no_mangle]
pub extern "C" fn js_array_get_f64(arr: *const ArrayHeader, index: u32) -> f64 {
    // Issue #179 Phase 5: lazy fast path — must run BEFORE
    // `clean_arr_ptr` because that helper force-materializes a lazy
    // pointer into a regular ArrayHeader. For the common read-only
    // shape (`parsed[i]` on a lazy result), force-materializing the
    // whole tree on first access dominates the workload; the sparse
    // per-element cache only materializes the touched subtree.
    //
    // Same tag-strip pattern as `js_array_length`: v0.5.206 added a
    // lazy guard in `clean_arr_ptr` that force-materializes, but
    // for the sparse-cache path we want to keep the LazyArrayHeader
    // around so the cache persists across calls. Strip the NaN-box
    // tag manually and check obj_type without going through the
    // clean-and-validate helper.
    unsafe {
        let bits = arr as u64;
        let top16 = bits >> 48;
        let raw_ptr = if top16 >= 0x7FF8 {
            if top16 == 0x7FFC {
                return f64::NAN;
            }
            (bits & 0x0000_FFFF_FFFF_FFFF) as *const ArrayHeader
        } else {
            arr
        };
        if !raw_ptr.is_null() && (raw_ptr as usize) >= crate::gc::GC_HEADER_SIZE + 0x1000 {
            let gc_header =
                (raw_ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            if (*gc_header).obj_type == crate::gc::GC_TYPE_LAZY_ARRAY {
                let lazy = raw_ptr as *mut crate::json_tape::LazyArrayHeader;
                if (*lazy).magic == crate::json_tape::LAZY_ARRAY_MAGIC {
                    let value = crate::json_tape::lazy_get(lazy, index);
                    return f64::from_bits(value.bits());
                }
            }
        }
    }
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return f64::NAN;
    }
    // Check if this is actually a TypedArray — dispatch through typed array helper
    if crate::typedarray::lookup_typed_array_kind(arr as usize).is_some() {
        return crate::typedarray::js_typed_array_get(
            arr as *const crate::typedarray::TypedArrayHeader,
            index as i32,
        );
    }
    // Check if this is actually a buffer (Uint8Array) — read individual bytes
    if crate::buffer::is_registered_buffer(arr as usize) {
        let byte_val =
            crate::buffer::js_buffer_get(arr as *const crate::buffer::BufferHeader, index as i32);
        return byte_val as f64;
    }
    // Check if this is a Set — read from elements pointer (not inline)
    if crate::set::is_registered_set(arr as usize) {
        let set = arr as *const crate::set::SetHeader;
        unsafe {
            let size = (*set).size;
            if index >= size {
                return TAG_UNDEFINED_F64;
            }
            let elements = (*set).elements as *const f64;
            return std::ptr::read(elements.add(index as usize));
        }
    }
    // Check if this is a Map — return entries as [key, value] pairs
    if crate::map::is_registered_map(arr as usize) {
        let map = arr as *const crate::map::MapHeader;
        unsafe {
            let size = (*map).size;
            if index >= size {
                return TAG_UNDEFINED_F64;
            }
            let entries = (*map).entries as *const f64;
            // Map entries: key at index*2, return key for simple iteration
            return std::ptr::read(entries.add(index as usize * 2));
        }
    }
    // JS spec: out-of-bounds array access returns `undefined`, not NaN.
    // This matters for destructuring defaults (`const [a, b, c = 30] = [1, 2]`)
    // where the `?? fallback` must see TAG_UNDEFINED, not NaN.
    const TAG_UNDEFINED_F64: f64 = f64::from_bits(0x7FFC_0000_0000_0001u64);
    unsafe {
        let length = (*arr).length;
        if index >= length {
            return TAG_UNDEFINED_F64;
        }
        // Guard: corrupted arrays with unreasonably large length
        if length > 100000 {
            return TAG_UNDEFINED_F64;
        }
        let elements_ptr = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
        let raw = *elements_ptr.add(index as usize);
        // Issue #323: translate HOLE sentinel back to `undefined` (see
        // `js_array_alloc_with_length` for context).
        if raw.to_bits() == crate::value::TAG_HOLE {
            return TAG_UNDEFINED_F64;
        }
        raw
    }
}

/// Fast-path array element write: skips all polymorphic registry checks
/// (buffer). Only does bounds checking and element write.
/// Use when the codegen KNOWS the pointer is a plain Array (not Buffer).
#[no_mangle]
pub extern "C" fn js_array_set_f64_unchecked(arr: *mut ArrayHeader, index: u32, value: f64) {
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        return;
    }
    unsafe {
        let length = (*arr).length;
        if index >= length {
            return;
        }
        let elements_ptr = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
        ptr::write(elements_ptr.add(index as usize), value);
        note_array_slot(arr, index as usize, value.to_bits());
    }
}

/// Set an element in an array by index
/// Note: This does NOT extend the array if index >= length
#[no_mangle]
pub extern "C" fn js_array_set_f64(arr: *mut ArrayHeader, index: u32, value: f64) {
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        return;
    }
    // Check if this is actually a buffer (Uint8Array) — write individual bytes
    if crate::buffer::is_registered_buffer(arr as usize) {
        crate::buffer::js_buffer_set(
            arr as *mut crate::buffer::BufferHeader,
            index as i32,
            value as i32,
        );
        return;
    }
    // Check if this is a typed array — route through per-kind store.
    if crate::typedarray::lookup_typed_array_kind(arr as usize).is_some() {
        crate::typedarray::js_typed_array_set(
            arr as *mut crate::typedarray::TypedArrayHeader,
            index as i32,
            value,
        );
        return;
    }
    unsafe {
        let length = (*arr).length;
        if index >= length {
            return;
        }
        let elements_ptr = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
        ptr::write(elements_ptr.add(index as usize), value);
        note_array_slot(arr, index as usize, value.to_bits());
    }
}

/// Set an element in an array by index, extending the array if needed
/// Returns the (possibly reallocated) array pointer
/// This mimics JavaScript's arr[i] = value behavior
#[no_mangle]
pub extern "C" fn js_array_set_f64_extend(
    arr: *mut ArrayHeader,
    index: u32,
    value: f64,
) -> *mut ArrayHeader {
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        return js_array_alloc(0);
    }
    // Check if this is actually a buffer (Uint8Array) — write individual bytes
    if crate::buffer::is_registered_buffer(arr as usize) {
        crate::buffer::js_buffer_set(
            arr as *mut crate::buffer::BufferHeader,
            index as i32,
            value as i32,
        );
        return arr;
    }
    // Check if this is a typed array — route through per-kind store (no extension).
    if crate::typedarray::lookup_typed_array_kind(arr as usize).is_some() {
        crate::typedarray::js_typed_array_set(
            arr as *mut crate::typedarray::TypedArrayHeader,
            index as i32,
            value,
        );
        return arr;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let _arr_handle = scope.root_raw_mut_ptr(arr);
    let value_handle = scope.root_nanbox_f64(value);
    unsafe {
        let length = (*arr).length;

        // If index is within bounds, just set it
        if index < length {
            let elements_ptr = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
            ptr::write(elements_ptr.add(index as usize), value);
            note_array_slot(arr, index as usize, value.to_bits());
            return arr;
        }

        // Need to extend the array
        let new_length = index + 1;
        let arr = if new_length > (*arr).capacity {
            js_array_grow(arr, new_length)
        } else {
            arr
        };
        let value = value_handle.get_nanbox_f64();

        // Fill any gap with TAG_HOLE so subsequent reads / iteration /
        // JSON.stringify treat them as holes (per ECMA-262 §22.1.3.30
        // step 5.b: holes serialize to "null"). Pre-fix this wrote 0.0
        // which was indistinguishable from a real numeric 0 — sparse
        // arrays serialized as `[0, 0, ...]` instead of `[null, null,
        // ...]`. Read paths translate TAG_HOLE → TAG_UNDEFINED via
        // `js_array_get_f64`'s post-#323 hole handling.
        let elements_ptr = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
        let hole = f64::from_bits(crate::value::TAG_HOLE);
        for i in length..index {
            ptr::write(elements_ptr.add(i as usize), hole);
            note_array_slot(arr, i as usize, crate::value::TAG_HOLE);
        }

        // Set the value
        ptr::write(elements_ptr.add(index as usize), value);
        note_array_slot(arr, index as usize, value.to_bits());
        (*arr).length = new_length;

        arr
    }
}

/// `arr[stringKey] = value` — handles the JS spec rule that numeric-string
/// keys on arrays are coerced to integer indices. Pre-fix the codegen's
/// IndexSet array fast-path applied `fptosi(double, i32)` directly to the
/// NaN-boxed string value, producing garbage indices that all collapsed
/// onto slot 0 (every iteration overwrote the previous).
///
/// Spec: an "array index" is a string whose canonical numeric form is a
/// non-negative integer < 2^32-1. Such writes update the array's element
/// storage; non-numeric string keys fall through to the object-property
/// path on the array's expando map (rare).
///
/// Issue #637 followup: this helper is also called from the polymorphic
/// IndexSet dispatch when the receiver type isn't statically known —
/// the runtime detects the receiver's gc_type byte and routes to the
/// per-kind setter. For Object/Closure receivers, fall through to
/// `js_object_set_field_by_name`. For Array receivers, parse the key
/// as integer and route to `js_array_set_f64_extend`.
#[no_mangle]
pub extern "C" fn js_array_set_string_key(
    arr: *mut ArrayHeader,
    key: *const crate::StringHeader,
    value: f64,
) -> *mut ArrayHeader {
    if arr.is_null() || key.is_null() {
        return arr;
    }
    // Issue #637: also called from polymorphic IndexSet — detect the
    // receiver's gc_type and route accordingly. For Object/Closure
    // (non-array) receivers, just call the object setter directly so
    // the standard expando-property path runs.
    let is_array = unsafe {
        if (arr as usize) >= crate::gc::GC_HEADER_SIZE + 0x1000 {
            let gc_header =
                (arr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            (*gc_header).obj_type == crate::gc::GC_TYPE_ARRAY
        } else {
            false
        }
    };
    if !is_array {
        crate::object::js_object_set_field_by_name(
            arr as *mut crate::object::ObjectHeader,
            key,
            value,
        );
        return arr;
    }
    // Read the key as a Rust &str via the standard StringHeader layout.
    let key_str = unsafe {
        let len = (*key).byte_len as usize;
        if len == 0 {
            return arr;
        }
        let data = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        let bytes = std::slice::from_raw_parts(data, len);
        match std::str::from_utf8(bytes) {
            Ok(s) => s,
            Err(_) => return arr,
        }
    };
    // Try parse as a non-negative integer in array-index range.
    if let Ok(idx) = key_str.parse::<u32>() {
        // Reject leading zeros / signs that would round-trip differently
        // (e.g. "01" -> 1, but the canonical form is "1"; per spec only
        // "1" is a valid array index, "01" is a generic property).
        let canonical = idx.to_string();
        if canonical == key_str && idx < u32::MAX {
            return js_array_set_f64_extend(arr, idx, value);
        }
    }
    // Non-numeric string key — fall through to object-property set on the
    // array's expando map. Arrays with named properties are rare but spec-
    // legal.
    crate::object::js_object_set_field_by_name(arr as *mut crate::object::ObjectHeader, key, value);
    arr
}

/// `arr[idx] = value` where idx may be a NaN-boxed string (numeric-string
/// key) OR a number. Dispatches at runtime: string tags → parse and route
/// to `js_array_set_string_key`; otherwise treat as numeric and route to
/// `js_array_set_f64_extend`. Issue #637 followup: the array fast-path's
/// `fptosi(idx_double, i32)` collapsed every NaN-boxed string to slot 0
/// (NaN→i32 = 0 on most platforms), so `forEach((k) => arr[k] = ...)`
/// over `["0","1","2"]` overwrote slot 0 three times. Codegen routes
/// the array fast-path here when the index expression isn't statically
/// numeric.
#[no_mangle]
pub extern "C" fn js_array_set_index_or_string(
    arr: *mut ArrayHeader,
    idx: f64,
    value: f64,
) -> *mut ArrayHeader {
    if arr.is_null() {
        return arr;
    }
    let bits = idx.to_bits();
    let top16 = bits >> 48;
    // STRING_TAG (0x7FFF) heap pointer — dispatch through the string-key
    // helper which parses the numeric value and routes appropriately.
    // SHORT_STRING_TAG (0x7FF9) is the SSO variant; same path via
    // `js_get_string_pointer_unified` — handled inside `js_string_*` helpers.
    if top16 == 0x7FFF {
        let ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as *const crate::StringHeader;
        return js_array_set_string_key(arr, ptr, value);
    }
    if top16 == 0x7FF9 {
        // SHORT_STRING_TAG (SSO). Materialize as a real StringHeader
        // via `js_get_string_pointer_unified` so `js_array_set_string_key`
        // can read the bytes through the standard layout.
        let str_ptr =
            crate::value::js_get_string_pointer_unified(idx) as *const crate::StringHeader;
        return js_array_set_string_key(arr, str_ptr, value);
    }
    // Treat as numeric (covers Int32 / plain f64 / other tags).
    let idx_i32 = idx as i32;
    if idx_i32 < 0 {
        // Negative numeric key: per JS spec, becomes a string property on
        // the array's expando map. Stringify and delegate.
        let s = idx_i32.to_string();
        let key = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
        crate::object::js_object_set_field_by_name(
            arr as *mut crate::object::ObjectHeader,
            key,
            value,
        );
        return arr;
    }
    js_array_set_f64_extend(arr, idx_i32 as u32, value)
}
