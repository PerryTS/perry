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
        // HOLE-initialize the whole capacity so the unused [length, capacity)
        // slack never holds stale arena bits that the whole-heap from-space
        // scan misreads as live from-space pointers.
        let elements_ptr = (ptr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut u64;
        for i in 0..actual_capacity as usize {
            // GC_STORE_AUDIT(INIT): initialization of a just-allocated array
            // that is not yet reachable from any root, and TAG_HOLE is a
            // non-pointer sentinel — there is no old value to remember and no
            // new edge to record, so no write barrier.
            std::ptr::write(elements_ptr.add(i), crate::value::TAG_HOLE);
        }
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

/// Allocate an exact-sized holey array for runtime-owned side storage.
///
/// Unlike [`js_array_alloc_with_length`], this does not add
/// [`MIN_ARRAY_CAPACITY`] growth headroom. Callers must know their final width;
/// the spill buffer used by JSON tape materialization does, and padding every
/// parsed object to 16 side slots would otherwise dominate the object itself.
pub(crate) fn js_array_alloc_with_length_exact(capacity: u32) -> *mut ArrayHeader {
    let ptr = arena_alloc_gc(
        array_byte_size(capacity as usize),
        8,
        crate::gc::GC_TYPE_ARRAY,
    ) as *mut ArrayHeader;

    unsafe {
        (*ptr).length = capacity;
        (*ptr).capacity = capacity;
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
        // ArrayCreate records the requested uint32 length; it does not require
        // a dense backing store for every hole.  Allocating `new Array(2^32-1)`
        // as one contiguous element buffer tried to reserve roughly 32 GiB and
        // either hung or exhausted the process.  Keep ordinary-sized arrays
        // dense, but represent a large fresh holey array exactly like a later
        // sparse length extension: a small backing store plus the full logical
        // length. Indexed reads already treat `index >= capacity` as a hole.
        const MAX_FRESH_DENSE_ARRAY_LENGTH: u32 = 1_000_000;
        let arr = if length > MAX_FRESH_DENSE_ARRAY_LENGTH {
            let arr = js_array_alloc(0);
            unsafe {
                (*arr).length = length;
            }
            arr
        } else {
            js_array_alloc_with_length(length)
        };
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

/// Store a freshly built part string into slot `index` of an all-pointer
/// result array and publish the slot.
///
/// Mirrors `string/split.rs`'s `store_split_string`: the array is allocated
/// with `length == 0` and each element is published only after its write and
/// barrier, so a collection triggered by the NEXT part's allocation never scans
/// an uninitialized slot.
///
/// # Safety
///
/// `arr` must be a live all-pointer `GC_TYPE_ARRAY` with capacity > `index`,
/// and `string` a live `StringHeader`.
unsafe fn store_codepoint_string(
    arr: *mut ArrayHeader,
    index: usize,
    string: *mut crate::string::StringHeader,
) {
    let elements = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
    let value = crate::value::js_nanbox_string(string as i64);
    // GC_STORE_AUDIT(BARRIERED): codepoint array slot is followed by a runtime write barrier.
    ptr::write(elements.add(index), value);
    crate::gc::runtime_write_barrier_slot(
        arr as usize,
        elements.add(index) as usize,
        value.to_bits(),
    );
    (*arr).length = (index + 1) as u32;
}

/// `Array.from(string)` — split the source string into Unicode code points and
/// emit each as a 1-code-point string element (matches `[..."hello"]` /
/// `for (const c of "hello")` semantics). Surrogate pairs in UTF-16 source
/// space materialize as a single code point per ECMA-262 §22.1.5 String
/// Iterator Records, so `[..."🎉"]` yields a 1-element array (not 2).
///
/// #9431: this used to run `std::str::from_utf8(bytes)` and return an EMPTY
/// array on `Err`. Perry string payloads are WTF-8, not UTF-8 — a lone
/// surrogate (`"a\ud83db"`, the half of a pair a slicing bug or a chunked
/// decoder leaves behind) is a legal payload that `from_utf8` rejects — so
/// `Array.from` silently answered `[]` for the whole string while `[...s]` and
/// `for…of` over the same string were correct. Step the raw bytes with the
/// bounded `wtf8_step` decoder instead, which yields one code point per step
/// and reports a lone surrogate as its own single-unit step, exactly as the
/// string iterator does.
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

    // GC safety: `js_string_from_bytes` allocates, and a collection can both
    // reclaim and (under evacuation) MOVE the source string and the result
    // array. The pre-#9431 loop held a raw `elements` pointer and a borrow of
    // the source payload across every per-element allocation — pre-existing,
    // and exactly the `string/split.rs` hazard. Root both, re-read the source
    // after each allocation, and store each part into the rooted array
    // immediately.
    let scope = crate::gc::RuntimeHandleScope::new();
    let s_handle = scope.root_string_ptr(s);
    let src_has_lone_surrogates = (*s).flags & crate::string::STRING_FLAG_HAS_LONE_SURROGATES != 0;

    // Pass 1: count the code points. No allocation here, so the payload cannot
    // move under us.
    let mut count = 0usize;
    {
        let bytes = std::slice::from_raw_parts(crate::string::string_data(s), byte_len);
        let mut i = 0usize;
        while i < bytes.len() {
            let (advance, _, _) = crate::string::wtf8_step(bytes, i);
            i = (i + advance).min(bytes.len());
            count += 1;
        }
    }

    // Pass 2: allocate the result, then fill it slot by slot.
    let (arr, _) = s_handle.across_const::<crate::string::StringHeader, _>(|| {
        js_array_alloc_pointer_elements(count as u32)
    });
    let arr_handle = scope.root_raw_mut_ptr(arr);

    let mut offset = 0usize;
    for index in 0..count {
        // Copy the sequence into a stack buffer BEFORE allocating:
        // `js_string_from_bytes` allocates first and copies second, so handing
        // it a pointer into the GC heap is the #5062 dangling-source class. A
        // WTF-8 sequence is at most 4 bytes.
        let mut buf = [0u8; 4];
        let seq_len;
        {
            let s_now = s_handle.get_raw_const_ptr::<crate::string::StringHeader>();
            let bytes = std::slice::from_raw_parts(
                crate::string::string_data(s_now),
                (*s_now).byte_len as usize,
            );
            if offset >= bytes.len() {
                break;
            }
            let (advance, _, _) = crate::string::wtf8_step(bytes, offset);
            let end = (offset + advance).min(bytes.len());
            seq_len = end - offset;
            buf[..seq_len].copy_from_slice(&bytes[offset..end]);
            offset = end;
        }
        // `js_string_from_bytes` hardcodes flags = 0, so a lone surrogate
        // carved out of a WTF-8 source would lose its marker and
        // `isWellFormed()` on the element would wrongly report true.
        let seq = &buf[..seq_len];
        let (sh, arr_now) = arr_handle.across_mut::<ArrayHeader, _>(|| {
            if src_has_lone_surrogates && crate::string::bytes_have_lone_surrogate(seq) {
                crate::string::js_string_from_wtf8_bytes(seq.as_ptr(), seq_len as u32)
            } else {
                crate::string::js_string_from_bytes(seq.as_ptr(), seq_len as u32)
            }
        });
        store_codepoint_string(arr_now, index, sh);
    }
    arr_handle.get_raw_mut_ptr::<ArrayHeader>()
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

/// Descriptor tag bytes for [`js_value_from_const_descriptor`]. MUST match the
/// serializer in `perry-codegen/src/expr/array_literal.rs`.
const DESC_NUMBER: u8 = 0; // + 8 bytes little-endian f64
const DESC_ARRAY: u8 = 1; // + 4 bytes little-endian u32 count, then `count` values
const DESC_TRUE: u8 = 2;
const DESC_FALSE: u8 = 3;
const DESC_NULL: u8 = 4;
const DESC_UNDEFINED: u8 = 5;

/// #8583 follow-up: materialize a large, fully-CONSTANT array literal from a
/// static rodata descriptor in ONE call, instead of the N per-subarray
/// `js_array_from_values` allocations codegen otherwise emits. A minified bundle
/// data table — a giant nested constant numeric array (the Claude Code bundle's
/// `__33499`) — lowered to 11,104 allocations and a 245k-instruction body that
/// made `rewrite-statepoints-for-gc` fan out; this collapses it to one call over
/// a compact rodata blob.
///
/// Returns a FRESH, mutable value each call: JS array literals are mutable, so
/// the descriptor is a template, never a shared constant. GC is suppressed for
/// the whole build so the partially-built parent arrays held across nested child
/// allocations cannot be collected or moved (mirrors `js_json_parse` and the
/// lazy-array materializer). The blob is compiler-generated and trusted, but
/// every read is bounds-checked so a malformed descriptor declines to
/// `undefined` rather than reading out of bounds.
#[no_mangle]
pub extern "C" fn js_value_from_const_descriptor(ptr: *const u8, len: u32) -> f64 {
    if ptr.is_null() || len == 0 {
        return f64::from_bits(crate::value::JSValue::undefined().bits());
    }
    let _suppress = crate::gc::GcSuppressScope::new();
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    let mut pos = 0usize;
    let bits = build_const_value(bytes, &mut pos);
    f64::from_bits(bits)
}

/// Recursively build the JS value at `bytes[*pos]`, advancing `*pos`. Callers
/// hold GC suppressed, so heap pointers materialized here stay live and pinned
/// for the duration of the whole build.
fn build_const_value(bytes: &[u8], pos: &mut usize) -> u64 {
    use crate::value::JSValue;
    let undefined = || JSValue::undefined().bits();
    let Some(&tag) = bytes.get(*pos) else {
        return undefined();
    };
    *pos += 1;
    match tag {
        DESC_NUMBER => {
            if *pos + 8 > bytes.len() {
                return undefined();
            }
            let mut b = [0u8; 8];
            b.copy_from_slice(&bytes[*pos..*pos + 8]);
            *pos += 8;
            JSValue::number(f64::from_le_bytes(b)).bits()
        }
        DESC_ARRAY => {
            if *pos + 4 > bytes.len() {
                return undefined();
            }
            let mut c = [0u8; 4];
            c.copy_from_slice(&bytes[*pos..*pos + 4]);
            *pos += 4;
            let count = u32::from_le_bytes(c);
            let arr = js_array_alloc_literal(count);
            // All-number rows keep the raw-f64 layout fast path; any pointer
            // element (a nested array) is downgraded per-slot by
            // `store_array_slot`, so gate the numeric mark on a pure-number row.
            let mut all_number = count > 0;
            for i in 0..count as usize {
                if bytes.get(*pos).copied() != Some(DESC_NUMBER) {
                    all_number = false;
                }
                let elem = build_const_value(bytes, pos);
                unsafe { crate::array::store_array_slot(arr, i, elem) };
            }
            if all_number {
                crate::array::js_array_mark_numeric_f64_layout(arr);
            }
            JSValue::pointer(arr as *const u8).bits()
        }
        DESC_TRUE => JSValue::bool(true).bits(),
        DESC_FALSE => JSValue::bool(false).bits(),
        DESC_NULL => JSValue::null().bits(),
        DESC_UNDEFINED => undefined(),
        _ => undefined(),
    }
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
