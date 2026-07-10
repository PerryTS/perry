//! Array allocation primitives.
use super::*;
use crate::arena::arena_alloc_gc;
use std::ptr;

#[cold]
fn throw_invalid_array_length() -> ! {
    let bytes = b"Invalid array length";
    let msg = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    let err = crate::error::js_rangeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

/// Throw the `RangeError: Invalid array length` raised by `ArraySetLength`
/// when `ToUint32(value) !== ToNumber(value)` (a fractional / out-of-range
/// length). Does not return.
pub(crate) fn array_length_range_error() -> ! {
    throw_invalid_array_length()
}

pub(crate) fn array_length_from_number_or_throw(number: f64) -> u32 {
    if number.is_finite() && number >= 0.0 && number <= u32::MAX as f64 && number.trunc() == number
    {
        number as u32
    } else {
        throw_invalid_array_length()
    }
}

pub(crate) fn array_length_from_property_value_or_throw(value: f64) -> u32 {
    let number = crate::builtins::js_number_coerce(value);
    array_length_from_number_or_throw(number)
}

/// Allocate a new array with the given initial capacity
#[no_mangle]
pub extern "C" fn js_array_alloc(capacity: u32) -> *mut ArrayHeader {
    // Use at least MIN_ARRAY_CAPACITY to reduce reallocations for growing arrays
    let actual_capacity = capacity.max(MIN_ARRAY_CAPACITY);
    let ptr = arena_alloc_gc(
        array_byte_size(actual_capacity as usize),
        8,
        crate::gc::GC_TYPE_ARRAY,
    ) as *mut ArrayHeader;

    unsafe {
        // Initialize header
        (*ptr).length = 0;
        (*ptr).capacity = actual_capacity;
        set_array_numeric_layout(ptr, NumericArrayLayout::RawF64);
        crate::gc::layout_init_pointer_free(ptr as *mut u8);
    }

    ptr
}

/// Allocate a fresh array whose initialized prefix is known to contain only
/// heap pointers. Runtime producers such as `String.prototype.split` use this
/// instead of starting as a raw-f64 array and immediately invalidating that
/// representation on their first string store.
///
/// The caller must grow `length` only after writing each pointer slot. That
/// keeps the all-pointer layout precise if allocation triggers a collection
/// while the result is being materialized.
pub(crate) fn js_array_alloc_pointer_elements(capacity: u32) -> *mut ArrayHeader {
    let actual_capacity = capacity.max(MIN_ARRAY_CAPACITY);
    let ptr = arena_alloc_gc(
        array_byte_size(actual_capacity as usize),
        8,
        crate::gc::GC_TYPE_ARRAY,
    ) as *mut ArrayHeader;

    unsafe {
        (*ptr).length = 0;
        (*ptr).capacity = actual_capacity;
        // Arena slots can be reused after a raw-f64 array. The all-pointer
        // layout owns the same header, so clear numeric representation flags
        // before publishing it as pointer-only.
        clear_array_numeric_layout(ptr);
        crate::gc::layout_init_all_pointer_slots(ptr as *mut u8);
    }

    ptr
}

/// Create a new empty array (convenience alias for `js_array_alloc(0)`).
/// Used by perry-ui audio code.
#[no_mangle]
pub extern "C" fn js_array_create() -> i64 {
    js_array_alloc(0) as i64
}

/// Allocate a new array with the given capacity AND set length = capacity.
/// Used for `new Array(n)` which in JavaScript creates an array with length n.
/// Reachable slots (`0..capacity`) are initialized to TAG_HOLE — a sentinel
/// distinct from TAG_UNDEFINED so the `in` operator and `Object.keys` can
/// distinguish a never-written slot from one explicitly set to `undefined`.
/// Reads via `js_array_get_f64` translate TAG_HOLE → TAG_UNDEFINED so the
/// sentinel never leaks to user code (matches issue #323).
/// Slots beyond `capacity` (up to `actual_capacity`) are unreachable through
/// the bounds-checked accessor, so they're left as-is.
///
/// Caveat: keys-arrays built by `js_object_alloc` (via shape) and one-shot
/// scratch arrays where the caller is about to overwrite every slot pay a
/// tiny init cost here; the alternative — a separate uninitialized variant —
/// would silently re-introduce the issue #323 bug class for any future caller
/// that forgets to overwrite.
#[no_mangle]
pub extern "C" fn js_array_alloc_with_length(capacity: u32) -> *mut ArrayHeader {
    let actual_capacity = capacity.max(MIN_ARRAY_CAPACITY);
    let ptr = arena_alloc_gc(
        array_byte_size(actual_capacity as usize),
        8,
        crate::gc::GC_TYPE_ARRAY,
    ) as *mut ArrayHeader;

    unsafe {
        (*ptr).length = capacity; // Set length = requested capacity
        (*ptr).capacity = actual_capacity;
        let elements_ptr = (ptr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut u64;
        for i in 0..capacity as usize {
            // GC_STORE_AUDIT(POINTER_FREE): TAG_HOLE is a non-pointer sentinel for fresh array slots.
            std::ptr::write(elements_ptr.add(i), crate::value::TAG_HOLE);
        }
        clear_array_numeric_layout(ptr);
        crate::gc::layout_init_pointer_free(ptr as *mut u8);
    }

    ptr
}

/// Runtime path for `Array(value)` / `new Array(value)`.
///
/// A single Number argument is interpreted as an array length and must be a
/// finite uint32. Any other single argument is stored as element 0.
#[no_mangle]
pub extern "C" fn js_array_constructor_single(value: f64) -> *mut ArrayHeader {
    if let Some(number) = value_bits_to_number(value.to_bits()) {
        let length = array_length_from_number_or_throw(number);
        let arr = js_array_alloc_with_length(length);
        if length > 0 {
            // #6011: user-facing `new Array(n)` — every slot is TAG_HOLE, so
            // the raw-f64-or-holes invariant holds by construction and the
            // packed-f64 range-loop guard can skip its verify walk. This is
            // the ONLY `js_array_alloc_with_length` caller that may mark:
            // internal callers (shape keys arrays, sort scratch, …)
            // direct-write slots without the layout-noting store helpers.
            unsafe { mark_array_raw_f64_holes_fresh(arr) };
        }
        return arr;
    }

    let scope = crate::gc::RuntimeHandleScope::new();
    let value_handle = scope.root_nanbox_f64(value);
    let arr = js_array_alloc(1);
    unsafe {
        (*arr).length = 1;
        let value = value_handle.get_nanbox_f64();
        note_array_slot(arr, 0, value.to_bits());
    }
    arr
}

/// Allocate a new array with `length == capacity == capacity` in the
/// **longlived arena** (issue #179). Used to build the shape-cache
/// `keys_array` backing storage, which is cache-resident for the life
/// of the thread and anchored by `scan_shape_cache_roots`.
///
/// Caller fills element slots immediately via direct writes (same
/// contract as `js_array_alloc_with_length`). Uses exact capacity — no
/// `MIN_ARRAY_CAPACITY` padding — because keys arrays never grow
/// (shapes are immutable once built).
#[no_mangle]
pub extern "C" fn js_array_alloc_with_length_longlived(capacity: u32) -> *mut ArrayHeader {
    let ptr = crate::arena::arena_alloc_gc_longlived(
        array_byte_size(capacity as usize),
        8,
        crate::gc::GC_TYPE_ARRAY,
    ) as *mut ArrayHeader;

    unsafe {
        (*ptr).length = capacity;
        (*ptr).capacity = capacity;
        clear_array_numeric_layout(ptr);
        crate::gc::layout_init_pointer_free(ptr as *mut u8);
    }

    ptr
}

/// Allocate and initialize an array from a list of f64 values
#[no_mangle]
pub extern "C" fn js_array_from_f64(elements: *const f64, count: u32) -> *mut ArrayHeader {
    let arr = js_array_alloc(count);
    unsafe {
        (*arr).length = count;
        let arr_elements = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
        // GC_STORE_AUDIT(BARRIERED): bulk array initialization is followed by layout/barrier rebuild.
        ptr::copy_nonoverlapping(elements, arr_elements, count as usize);
        rebuild_array_layout(arr);
    }
    arr
}

/// `Array.from({length: N, 0: a, 1: b, ...})` — read the `length` property
/// and emit `obj[0]..obj[N-1]` in order (missing slots fill with `undefined`
/// per spec). Receivers without a numeric `length` property produce an
/// empty array (ToLength coerces non-numbers to 0).
pub(crate) unsafe fn js_array_from_arraylike(
    obj: *const crate::object::ObjectHeader,
) -> *mut ArrayHeader {
    js_array_from_arraylike_with_missing(obj, f64::from_bits(crate::value::TAG_UNDEFINED))
}

pub(crate) unsafe fn js_array_from_arraylike_holey(
    obj: *const crate::object::ObjectHeader,
) -> *mut ArrayHeader {
    js_array_from_arraylike_with_missing(obj, f64::from_bits(crate::value::TAG_HOLE))
}

unsafe fn js_array_from_arraylike_with_missing(
    obj: *const crate::object::ObjectHeader,
    missing_value: f64,
) -> *mut ArrayHeader {
    if obj.is_null() {
        return js_array_alloc(0);
    }
    let length_key = crate::string::js_string_from_bytes(b"length".as_ptr(), 6);
    let length_val = crate::object::js_object_get_field_by_name_f64(obj, length_key);
    let length_bits = length_val.to_bits();
    // ToLength coercion: NaN / undefined / non-finite / negative → 0.
    let len = if length_val.is_nan()
        || !length_val.is_finite()
        || length_val < 0.0
        || (length_bits >> 48) >= 0x7FF8
    {
        0u32
    } else {
        length_val as u32
    };
    let arr = js_array_alloc(len);
    (*arr).length = len;
    clear_array_numeric_layout(arr);
    let elements = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
    for i in 0..len {
        let key_str = i.to_string();
        let key = crate::string::js_string_from_bytes(key_str.as_ptr(), key_str.len() as u32);
        let key_value = f64::from_bits(crate::value::JSValue::string_ptr(key).bits());
        let obj_value = f64::from_bits(crate::value::JSValue::pointer(obj as *const u8).bits());
        let has_property =
            crate::value::js_is_truthy(crate::object::js_object_has_property(obj_value, key_value))
                != 0;
        let v = if has_property {
            crate::object::js_object_get_field_by_name_f64(obj, key)
        } else {
            missing_value
        };
        // GC_STORE_AUDIT(BARRIERED): arraylike element write is immediately recorded via note_array_slot.
        *elements.add(i as usize) = v;
        note_array_slot(arr, i as usize, v.to_bits());
    }
    refresh_array_numeric_layout(arr);
    arr
}

#[no_mangle]
pub extern "C" fn js_array_from_arraylike_holey_value(boxed: f64) -> *mut ArrayHeader {
    let bits = boxed.to_bits();
    let jv = crate::value::JSValue::from_bits(bits);
    if jv.is_undefined() || jv.is_null() {
        crate::object::has_own_helpers::throw_to_object_nullish_type_error();
    }
    if !jv.is_pointer() {
        return crate::array::js_array_from_value(boxed);
    }
    let raw_addr = (bits & 0x0000_FFFF_FFFF_FFFF) as usize;
    unsafe {
        if let Some(arr) =
            crate::object::arguments_object_to_array(raw_addr as *const crate::object::ObjectHeader)
        {
            return arr;
        }
        if raw_addr >= crate::gc::GC_HEADER_SIZE + 0x1000 {
            let hdr = (raw_addr as *const u8).sub(crate::gc::GC_HEADER_SIZE)
                as *const crate::gc::GcHeader;
            if (*hdr).obj_type == crate::gc::GC_TYPE_OBJECT
                && crate::typedarray::lookup_typed_array_kind(raw_addr).is_none()
                && !crate::buffer::is_registered_buffer(raw_addr)
            {
                return js_array_from_arraylike_holey(
                    raw_addr as *const crate::object::ObjectHeader,
                );
            }
        }
    }
    crate::array::js_array_from_value(boxed)
}

/// `Array.from(string)` — split the source string into Unicode codepoints
/// and emit each as a 1-codepoint string element (matches `[..."hello"]` /
/// `for (const c of "hello")` semantics). Surrogate pairs in UTF-16 source
/// space materialize as a single codepoint per ECMA-262 §22.1.5 String
/// Iterator Records, so `[..."🎉"]` yields a 1-element array (not 2).
pub(crate) unsafe fn js_array_from_string_codepoints(
    s: *const crate::string::StringHeader,
) -> *mut ArrayHeader {
    if s.is_null() {
        return js_array_alloc(0);
    }
    let byte_len = (*s).byte_len as usize;
    if byte_len == 0 {
        return js_array_alloc(0);
    }
    let data_ptr = (s as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, byte_len);
    let src = match std::str::from_utf8(bytes) {
        Ok(s) => s,
        Err(_) => return js_array_alloc(0),
    };
    // Pre-count to size the result exactly.
    let cp_count = src.chars().count() as u32;
    let arr = js_array_alloc(cp_count);
    (*arr).length = cp_count;
    clear_array_numeric_layout(arr);
    let elements = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
    for (i, ch) in src.chars().enumerate() {
        let mut buf = [0u8; 4];
        let s_ref = ch.encode_utf8(&mut buf);
        let s_ptr = crate::string::js_string_from_bytes(s_ref.as_ptr(), s_ref.len() as u32);
        let value = crate::value::js_nanbox_string(s_ptr as i64);
        // GC_STORE_AUDIT(BARRIERED): string codepoint array slot is immediately recorded via note_array_slot.
        *elements.add(i) = value;
        note_array_slot(arr, i, value.to_bits());
    }
    arr
}

/// Exact-sized array allocation for array literals `[a, b, c, ...]`.
///
/// Unlike `js_array_alloc`, this does NOT apply `MIN_ARRAY_CAPACITY=16` padding.
/// Every byte allocated is a byte the literal uses, which keeps tight-loop
/// allocation pressure proportional to the literal size (a 3-element literal
/// costs 32 bytes, not 136). `length` is pre-set to `capacity` so the codegen
/// only needs to emit direct stores for each element; no per-element
/// `js_array_push_f64` call with redundant capacity check.
///
/// Caller contract: the codegen evaluates every element expression *before*
/// calling this function, then emits direct stores to `(arr+8) + i*8` with no
/// intervening GC-triggering operation. Between this call and completion of
/// the stores, the array header reports `length == capacity` but elements are
/// uninitialized; only pure LLVM stores may execute in that window.
#[no_mangle]
pub extern "C" fn js_array_alloc_literal(capacity: u32) -> *mut ArrayHeader {
    let ptr = arena_alloc_gc(
        array_byte_size(capacity as usize),
        8,
        crate::gc::GC_TYPE_ARRAY,
    ) as *mut ArrayHeader;
    unsafe {
        (*ptr).length = capacity;
        (*ptr).capacity = capacity;
        clear_array_numeric_layout(ptr);
        crate::gc::layout_init_pointer_free(ptr as *mut u8);
    }
    ptr
}

/// #5391: build an array literal from a stack buffer of `n` pre-evaluated
/// element values in ONE call, replacing the inline alloc + per-element
/// store/layout-note/barrier sequence codegen otherwise emits at every literal
/// site. For oversized modules that inline expansion makes individual functions
/// enormous (a minified data-table builder reached 18MB of IR), which `clang
/// -O0` compiles in superlinear time; outlining the construction keeps the call
/// site to a buffer fill + one call.
///
/// Mirrors the inline `lower_array_literal` semantics: allocate via
/// `js_array_alloc_literal` (length pre-set to `n`), copy each value, then note
/// the slot layout and emit the write barrier per element. Both GC helpers
/// no-op for non-pointer values, so the per-element calls are unconditional and
/// correct for any mix of numbers and heap references.
#[no_mangle]
pub extern "C" fn js_array_from_values(values: *const f64, n: u32) -> *mut ArrayHeader {
    let arr = js_array_alloc_literal(n);
    if values.is_null() || n == 0 {
        return arr;
    }
    let parent = arr as u64;
    let elems = unsafe { (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64 };
    for i in 0..n as usize {
        let v = unsafe { *values.add(i) };
        let slot = unsafe { elems.add(i) };
        // A uniquely-owned string element now aliases this slot — demote it to
        // shared so a later `s += x` allocates fresh instead of mutating the
        // stored element. No-op for SSO / non-string. (This outline path doesn't
        // funnel through `note_array_slot`.)
        crate::string::js_string_addref_if_heap_string(v);
        // GC_STORE_AUDIT(BARRIERED): element store immediately followed by the
        // slot layout note + write barrier below, identical to the inline
        // array-literal element store via emit_jsvalue_slot_store_on_block.
        unsafe { core::ptr::write(slot, v) };
        let vbits = v.to_bits();
        crate::gc::js_gc_note_slot_layout(parent, i as u32, vbits);
        crate::gc::js_write_barrier_slot(parent, slot as u64, vbits);
    }
    arr
}

/// Issue #179 Phase 2: if `arr` points at a `LazyArrayHeader`
/// (`GcHeader::obj_type == GC_TYPE_LAZY_ARRAY`), force the lazy
/// value to materialize and return the real `ArrayHeader` pointer.
/// Otherwise returns `arr` unchanged. Every array accessor that
/// doesn't have a lazy-specific fast path (only `.length` does)
/// should funnel through this so correctness is preserved under
/// arbitrary JS code.
// #854: lazy-array materialization accessor (issue #179 Phase 2); funnel point
// for non-fast-path array accessors, retained for the lazy-array contract
#[allow(dead_code)]
#[inline]
pub(crate) unsafe fn maybe_force_lazy(arr: *const ArrayHeader) -> *const ArrayHeader {
    if arr.is_null() {
        return arr;
    }
    if (arr as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return arr;
    }
    let gc_header = (arr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    if (*gc_header).obj_type != crate::gc::GC_TYPE_LAZY_ARRAY {
        return arr;
    }
    let lazy = arr as *mut crate::json_tape::LazyArrayHeader;
    if (*lazy).magic != crate::json_tape::LAZY_ARRAY_MAGIC {
        return arr;
    }
    crate::json_tape::force_materialize_lazy(lazy) as *const ArrayHeader
}
