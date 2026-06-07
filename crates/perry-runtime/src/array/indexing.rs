//! Indexing — length / element get / element set / hybrid string-or-index dispatch.
use super::*;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

const MAX_DENSE_ARRAY_GROW_LENGTH: u32 = 1_000_000;

/// Lazily-memoized address of the `Array.prototype` array, and a sticky flag
/// recording whether anyone has installed an indexed property on it. An
/// out-of-bounds element read on an ordinary array must fall through to
/// `Array.prototype[index]` (ECMA-262 OrdinaryGet → prototype chain), but in
/// real code nobody adds numeric indices to `Array.prototype`, so the hot OOB
/// path stays a single relaxed atomic load until the (rare) write flips the
/// flag. `usize::MAX` marks the address as not-yet-computed.
static ARRAY_PROTO_ADDR: AtomicUsize = AtomicUsize::new(usize::MAX);
static ARRAY_PROTO_HAS_INDEX: AtomicBool = AtomicBool::new(false);

fn array_prototype_addr() -> usize {
    let cached = ARRAY_PROTO_ADDR.load(Ordering::Relaxed);
    if cached != usize::MAX {
        return cached;
    }
    let ctor = crate::object::js_get_global_this_builtin_value(b"Array".as_ptr(), 5);
    let ctor_value = crate::value::JSValue::from_bits(ctor.to_bits());
    let addr = if ctor_value.is_pointer() {
        let ctor_ptr = ctor_value.as_pointer::<u8>() as usize;
        let proto = crate::closure::closure_get_dynamic_prop(ctor_ptr, "prototype");
        let proto_value = crate::value::JSValue::from_bits(proto.to_bits());
        if proto_value.is_pointer() {
            proto_value.as_pointer::<u8>() as usize
        } else {
            0
        }
    } else {
        0
    };
    ARRAY_PROTO_ADDR.store(addr, Ordering::Relaxed);
    addr
}

/// Record (if `arr` is `Array.prototype`) that the prototype now carries an
/// indexed property, so subsequent out-of-bounds reads consult it. Called from
/// the array element-write paths; cheap (two relaxed atomic loads + compare).
#[inline]
pub(crate) fn note_array_index_write(arr: usize) {
    if !ARRAY_PROTO_HAS_INDEX.load(Ordering::Relaxed) && arr != 0 && arr == array_prototype_addr() {
        ARRAY_PROTO_HAS_INDEX.store(true, Ordering::Relaxed);
    }
}

/// Out-of-bounds element read fallback: `Array.prototype[index]` when the
/// prototype has indexed properties (see `ARRAY_PROTO_HAS_INDEX`). Returns the
/// inherited value, or `undefined` if absent. Skipped entirely when the
/// receiver IS `Array.prototype` (avoids self-recursion) or the flag is unset.
#[inline]
unsafe fn array_oob_prototype_get(receiver: usize, index: u32) -> f64 {
    const TAG_UNDEFINED_F64: f64 = f64::from_bits(0x7FFC_0000_0000_0001u64);
    if !ARRAY_PROTO_HAS_INDEX.load(Ordering::Relaxed) {
        return TAG_UNDEFINED_F64;
    }
    let proto = array_prototype_addr();
    if proto == 0 || proto == receiver {
        return TAG_UNDEFINED_F64;
    }
    let proto_arr = proto as *const ArrayHeader;
    if index >= (*proto_arr).length {
        return TAG_UNDEFINED_F64;
    }
    js_array_get_f64(proto_arr, index)
}

unsafe fn array_sparse_index_property_get(arr: *const ArrayHeader, index: u32) -> Option<f64> {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() || index < (*arr).capacity {
        return None;
    }
    let key = index.to_string();
    array_named_property_get_by_name(arr, &key)
}

unsafe fn array_sparse_index_property_set(arr: *mut ArrayHeader, index: u32, value: f64) {
    let key = index.to_string();
    let key_ptr = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
    array_named_property_set(arr, key_ptr, value);
    let new_length = index + 1;
    if (*arr).length < new_length {
        (*arr).length = new_length;
    }
}

fn array_get_property_by_key(arr: *const ArrayHeader, key: *const crate::StringHeader) -> f64 {
    let value =
        crate::object::js_object_get_field_by_name(arr as *const crate::object::ObjectHeader, key);
    f64::from_bits(value.bits())
}

#[no_mangle]
pub extern "C" fn js_array_length(arr: *const ArrayHeader) -> u32 {
    let arr = {
        let bits = arr as u64;
        let top16 = bits >> 48;
        if top16 >= 0x7FF8 {
            if top16 != (crate::value::POINTER_TAG >> 48) {
                return 0;
            }
            (bits & crate::value::POINTER_MASK) as *const ArrayHeader
        } else {
            arr
        }
    };
    if !arr.is_null() {
        let addr = arr as usize;
        if crate::set::is_registered_set(addr) {
            return crate::set::js_set_size(arr as *const crate::set::SetHeader);
        }
        if crate::map::is_registered_map(addr) {
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
            return array_oob_prototype_get(arr as usize, index);
        }
        if let Some(value) = array_sparse_index_property_get(arr, index) {
            return value;
        }
        if index >= (*arr).capacity {
            return array_oob_prototype_get(arr as usize, index);
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

#[no_mangle]
pub extern "C" fn js_array_numeric_get_f64_unboxed(arr: *mut ArrayHeader, index: u32) -> f64 {
    unsafe {
        if let Some(value) = array_numeric_raw_f64_get(arr, index) {
            return value;
        }
    }
    js_array_get_f64(arr, index)
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
            // Out of bounds: fall through to `Array.prototype[index]` (gated;
            // see `array_oob_prototype_get`). Common case is one atomic load.
            return array_oob_prototype_get(arr as usize, index);
        }
        if let Some(value) = array_sparse_index_property_get(arr, index) {
            return value;
        }
        if index >= (*arr).capacity {
            return array_oob_prototype_get(arr as usize, index);
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
    if array_is_frozen(arr) {
        return;
    }
    unsafe {
        let length = (*arr).length;
        if index >= length {
            return;
        }
        if index >= (*arr).capacity {
            array_sparse_index_property_set(arr, index, value);
            return;
        }
        let value = canonicalize_array_numeric_store_value(arr, value);
        let value_bits = value.to_bits();
        let elements_ptr = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
        // GC_STORE_AUDIT(BARRIERED): unchecked array set is immediately recorded via note_array_slot.
        ptr::write(elements_ptr.add(index as usize), value);
        note_array_slot(arr, index as usize, value_bits);
    }
}

#[no_mangle]
pub extern "C" fn js_array_numeric_set_f64_unboxed(
    arr: *mut ArrayHeader,
    index: u32,
    value: f64,
) -> i32 {
    if array_is_frozen(arr) {
        return 0;
    }
    unsafe {
        if array_numeric_raw_f64_set_inbounds(arr, index, value) {
            return 1;
        }
    }
    0
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
    if array_is_frozen(arr) {
        return;
    }
    unsafe {
        let length = (*arr).length;
        if index >= length {
            return;
        }
        if index >= (*arr).capacity {
            array_sparse_index_property_set(arr, index, value);
            return;
        }
        let value = canonicalize_array_numeric_store_value(arr, value);
        let value_bits = value.to_bits();
        let elements_ptr = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
        // GC_STORE_AUDIT(BARRIERED): array set is immediately recorded via note_array_slot.
        ptr::write(elements_ptr.add(index as usize), value);
        note_array_slot(arr, index as usize, value_bits);
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
    // If this write targets `Array.prototype`, mark the prototype as carrying an
    // indexed property so out-of-bounds element reads on ordinary arrays consult
    // it (ECMA-262 OrdinaryGet → prototype chain). Cheap no-op otherwise.
    note_array_index_write(arr as usize);
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
    let flags = array_object_flags(arr);
    let is_frozen = flags & crate::gc::OBJ_FLAG_FROZEN != 0;
    let blocks_extension =
        flags & (crate::gc::OBJ_FLAG_SEALED | crate::gc::OBJ_FLAG_NO_EXTEND) != 0;
    let scope = crate::gc::RuntimeHandleScope::new();
    let _arr_handle = scope.root_raw_mut_ptr(arr);
    let value_handle = scope.root_nanbox_f64(value);
    unsafe {
        let length = (*arr).length;

        if index == u32::MAX {
            return arr;
        }

        // If index is within bounds, just set it
        if index < length {
            if is_frozen {
                return arr;
            }
            if index >= (*arr).capacity {
                let value = value_handle.get_nanbox_f64();
                array_sparse_index_property_set(arr, index, value);
                return arr;
            }
            let value = canonicalize_array_numeric_store_value(arr, value);
            let value_bits = value.to_bits();
            let elements_ptr = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
            // GC_STORE_AUDIT(BARRIERED): in-bounds extending set is immediately recorded via note_array_slot.
            ptr::write(elements_ptr.add(index as usize), value);
            note_array_slot(arr, index as usize, value_bits);
            return arr;
        }

        if is_frozen || blocks_extension {
            return arr;
        }

        // Need to extend the array
        let new_length = index + 1;
        if new_length > (*arr).capacity && new_length > MAX_DENSE_ARRAY_GROW_LENGTH {
            let value = value_handle.get_nanbox_f64();
            array_sparse_index_property_set(arr, index, value);
            return arr;
        }
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
            // GC_STORE_AUDIT(BARRIERED): sparse gap sentinel is immediately recorded via note_array_slot.
            ptr::write(elements_ptr.add(i as usize), hole);
            note_array_slot(arr, i as usize, crate::value::TAG_HOLE);
        }

        // Set the value
        let value = canonicalize_array_numeric_store_value(arr, value);
        let value_bits = value.to_bits();
        // GC_STORE_AUDIT(BARRIERED): extending set value is immediately recorded via note_array_slot.
        ptr::write(elements_ptr.add(index as usize), value);
        note_array_slot(arr, index as usize, value_bits);
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
    if array_is_frozen(arr) {
        return arr;
    }
    let existing = unsafe { array_named_property_get(arr, key).is_some() };
    if !existing && array_is_sealed_or_no_extend(arr) {
        return arr;
    }
    if let Some(attrs) = crate::object::get_property_attrs(arr as usize, key_str) {
        if !attrs.writable() {
            return arr;
        }
    }
    // Non-numeric string key — fall through to object-property set on the
    // array's expando map. Arrays with named properties are rare but spec-
    // legal.
    unsafe {
        array_named_property_set(arr, key, value);
    }
    arr
}

/// `arr[idx]` where `idx` may be a number or property-key value. This mirrors
/// `js_array_set_index_or_string` for read paths that cannot safely narrow the
/// key through i32 codegen.
#[no_mangle]
pub extern "C" fn js_array_get_index_or_string(arr: *const ArrayHeader, idx: f64) -> f64 {
    if arr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let bits = idx.to_bits();
    let top16 = bits >> 48;
    if top16 == 0x7FFF {
        let key = (bits & 0x0000_FFFF_FFFF_FFFF) as *const crate::StringHeader;
        return array_get_property_by_key(arr, key);
    }
    if top16 == 0x7FF9 {
        let key = crate::value::js_get_string_pointer_unified(idx) as *const crate::StringHeader;
        return array_get_property_by_key(arr, key);
    }

    let numeric = if (bits & crate::value::TAG_MASK) == crate::value::INT32_TAG {
        Some(crate::value::JSValue::from_bits(bits).as_int32() as f64)
    } else if top16 < 0x7FF8 || top16 > 0x7FFF {
        Some(idx)
    } else {
        None
    };
    if let Some(n) = numeric {
        if n.is_finite() && n.trunc() == n && n >= 0.0 && n < u32::MAX as f64 {
            return js_array_get_f64(arr, n as u32);
        }
        if n.is_finite() && n.trunc() == n {
            let key = if n == 0.0 {
                "0".to_string()
            } else {
                format!("{:.0}", n)
            };
            let key_ptr = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
            return array_get_property_by_key(arr, key_ptr);
        }
    }

    if unsafe { crate::symbol::js_is_symbol(idx) } != 0 {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let key = crate::value::js_jsvalue_to_string(idx);
    if key.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    array_get_property_by_key(arr, key as *const crate::StringHeader)
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
    // Treat numeric keys according to the array-index boundary. Only
    // integers in 0..2^32-2 extend element storage; 2^32-1 and larger are
    // ordinary string properties.
    let numeric = if (bits & crate::value::TAG_MASK) == crate::value::INT32_TAG {
        Some(crate::value::JSValue::from_bits(bits).as_int32() as f64)
    } else if top16 < 0x7FF8 || top16 > 0x7FFF {
        Some(idx)
    } else {
        None
    };
    if let Some(n) = numeric {
        if n.is_finite() && n.trunc() == n && n >= 0.0 && n < u32::MAX as f64 {
            return js_array_set_f64_extend(arr, n as u32, value);
        }
        // Any other finite/non-finite number that is NOT a canonical array
        // index (2^32-1 and above, negatives, and non-integer floats such as
        // `a[1.5]`) becomes an ordinary string property. Route through
        // `js_jsvalue_to_string` so the key is the spec ToString of the
        // number ("4294967295", "-1", "1.5", "NaN") rather than a truncated
        // integer — `js_array_set_string_key` then stores it on the expando
        // map without touching `length` or any element slot. (Issue #4543.)
        let key = crate::value::js_jsvalue_to_string(idx);
        if !key.is_null() {
            return js_array_set_string_key(arr, key as *const crate::StringHeader, value);
        }
    }
    // Fallback for a NON-numeric key: a primitive (`a[null]`, `a[undefined]`,
    // `a[true]`, `a[10n]`) or a boxed object (`a[new Number(1)]`). Per
    // ToPropertyKey these become string property keys (or, for `10n`, the
    // canonical index "10"); `js_array_set_string_key` routes accordingly.
    // Arrays previously DROPPED these writes (plain objects handled them).
    // Restricted to `numeric.is_none()`: numeric keys (including non-integer
    // finite floats) are handled above. Symbols stay symbol-keyed.
    if numeric.is_none() && unsafe { crate::symbol::js_is_symbol(idx) } == 0 {
        let key = crate::value::js_jsvalue_to_string(idx);
        if !key.is_null() {
            return js_array_set_string_key(arr, key as *const crate::StringHeader, value);
        }
    }
    arr
}
