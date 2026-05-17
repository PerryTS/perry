//! Array representation for Perry
//!
//! Arrays are heap-allocated with a header containing:
//! - Length
//! - Capacity
//! - Elements array (inline)

use crate::arena::arena_alloc_gc;
use std::cell::RefCell;
use std::collections::HashMap;
use std::ptr;

thread_local! {
    /// Tagged-template `.raw` side-table — maps a cooked-strings array
    /// pointer to its corresponding raw-strings array pointer. Populated
    /// by `js_tagged_template_register_raw` at the tagged-call site; read
    /// by `js_template_raw` (HIR-folded from `<arg>.raw` on array
    /// receivers). Untagged arrays naturally miss the map and surface
    /// `undefined`, matching the JS semantics `[].raw === undefined`.
    /// Both pointers are GC-rooted via `scan_template_raw_roots`.
    static TEMPLATE_RAW_MAP: RefCell<HashMap<usize, *mut ArrayHeader>> =
        RefCell::new(HashMap::new());
}

/// Register the (cooked, raw) pair for a tagged-template call. Returns
/// `cooked` (so the codegen can chain it inline into the call args).
#[no_mangle]
pub extern "C" fn js_tagged_template_register_raw(
    cooked: *mut ArrayHeader,
    raw: *mut ArrayHeader,
) -> *mut ArrayHeader {
    if !cooked.is_null() && !raw.is_null() {
        TEMPLATE_RAW_MAP.with(|m| {
            m.borrow_mut().insert(cooked as usize, raw);
        });
    }
    cooked
}

/// Read the raw-strings array for a cooked array, or 0 if not a
/// tagged-template strings array.
#[no_mangle]
pub extern "C" fn js_template_raw(cooked: *const ArrayHeader) -> i64 {
    let cleaned = clean_arr_ptr(cooked);
    if cleaned.is_null() {
        return 0;
    }
    TEMPLATE_RAW_MAP.with(|m| {
        m.borrow()
            .get(&(cleaned as usize))
            .map(|&p| p as i64)
            .unwrap_or(0)
    })
}

/// GC root scanner — keeps both cooked and raw arrays in template
/// pairs reachable. Pruning of dead-cooked entries happens lazily on
/// next read miss; for now the map grows unbounded but it's tiny in
/// practice (one entry per distinct tagged-template call site).
pub fn scan_template_raw_roots(mark: &mut dyn FnMut(f64)) {
    const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
    const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
    TEMPLATE_RAW_MAP.with(|m| {
        for (cooked_addr, raw_ptr) in m.borrow().iter() {
            mark(f64::from_bits(
                POINTER_TAG | (*cooked_addr as u64 & POINTER_MASK),
            ));
            if !raw_ptr.is_null() {
                mark(f64::from_bits(
                    POINTER_TAG | (*raw_ptr as u64 & POINTER_MASK),
                ));
            }
        }
    });
}

/// Strip NaN-boxing tags from an array pointer and guard against invalid values.
///
/// Issue #73 follow-up: the `> 0x1000` (4 KB) floor is too permissive
/// for the macOS ARM64 heap layout. A corrupted NaN-box whose 48-bit
/// handle lands in the 1 TB — 2 TB window (e.g. `0x00FF_0000_0000` —
/// a `BufferHeader { length: 0, capacity: 255 }` read as u64) clears
/// the old floor and segfaults `(*arr).length` / SIMD memcpy inside
/// `js_array_slice` / `js_array_length` / etc. Real mimalloc + arena
/// allocations on Darwin consistently land in the 3-5 TB range;
/// constraining to `>= 2 TB && < 128 TB` rejects the observed
/// corruption patterns without cutting off any real heap pointer.
///
/// v0.5.85 follow-up: also validate the GC header byte + length/capacity
/// sanity. A pointer that passes the range check but points into the
/// middle of another allocation (post-GC memory reuse overlaid with
/// e.g. decoded PostgreSQL text column data) reads garbage length
/// values — witnessed `len=775370038 cap=926234674` (both the ASCII
/// bytes of `"6+2.2017"`) flowing through `js_array_slice` and
/// triggering 22GB-wide memcpy segfaults. Post-check: obj_type at
/// `handle-8` must equal GC_TYPE_ARRAY (1), and length must be
/// <= capacity <= 16M (same bound as the GC tracer's sanity guard).
#[inline(always)]
fn clean_arr_ptr(arr: *const ArrayHeader) -> *const ArrayHeader {
    // Heap window varies by OS: Darwin mimalloc lands in the 3-5 TB range;
    // Android scudo + Linux glibc allocate MUCH lower (often < 1 TB); Windows
    // mimalloc lands well under 1 TB (often in the GB-to-tens-of-GB range).
    // Using the Darwin-tight 2 TB floor on Android / Windows silently null-s
    // every real array pointer, turning js_array_set_f64 into a no-op and —
    // at the read side via js_array_map etc. — returning empty arrays for
    // legitimate inputs (issues #385/#386/#387).
    #[cfg(any(target_os = "android", target_os = "linux", target_os = "windows"))]
    const HEAP_MIN: u64 = 0x1000; // 4 KB (classic user-space floor)
    #[cfg(not(any(target_os = "android", target_os = "linux", target_os = "windows")))]
    const HEAP_MIN: u64 = 0x200_0000_0000; // 2 TB — above observed corrupt handles on Darwin
    const HEAP_MAX: u64 = 0x8000_0000_0000; // 47-bit userspace cap
    let bits = arr as u64;
    let top16 = bits >> 48;
    let cleaned = if top16 >= 0x7FF8 {
        if top16 == 0x7FFC || (bits & 0x0000_FFFF_FFFF_FFFF) == 0 {
            return std::ptr::null();
        }
        let cleaned_bits = bits & 0x0000_FFFF_FFFF_FFFF;
        if !(HEAP_MIN..HEAP_MAX).contains(&cleaned_bits) {
            return std::ptr::null();
        }
        cleaned_bits as *const ArrayHeader
    } else {
        if !(HEAP_MIN..HEAP_MAX).contains(&bits) {
            return std::ptr::null();
        }
        arr
    };
    // Issue #233: follow GC_FLAG_FORWARDED forwarding chains. When
    // an array grows (js_array_grow) we install a forwarding pointer
    // at the OLD location so any stale reference — e.g. an async
    // function's caller still holding the pre-grow pointer in its
    // parameter slot — resolves to the current head instead of
    // observing a defunct array whose first 8 bytes (length+capacity)
    // now hold the forwarding pointer. Without this, push beyond
    // initial capacity (16) silently became a no-op for the caller
    // because the new array lived at a different address that the
    // caller's slot was never updated to. The chain is short in
    // practice (1-2 grows) but cap depth at 64 to defend against
    // cycles from corrupted GC state.
    let mut cleaned = cleaned;
    unsafe {
        let mut steps = 0u32;
        while (cleaned as usize) >= crate::gc::GC_HEADER_SIZE + 0x1000 {
            let gc_header =
                (cleaned as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            if (*gc_header).gc_flags & crate::gc::GC_FLAG_FORWARDED == 0 {
                break;
            }
            let new_user = crate::gc::forwarding_address(gc_header) as u64;
            if !(HEAP_MIN..HEAP_MAX).contains(&new_user) {
                return std::ptr::null();
            }
            cleaned = new_user as *const ArrayHeader;
            steps += 1;
            if steps > 64 {
                return std::ptr::null();
            }
        }
    }
    // Issue #179 Phase 2: lazy arrays have a GcHeader with
    // obj_type == GC_TYPE_LAZY_ARRAY. Their layout's first two u32s
    // are (magic, cached_length) rather than (length, capacity) —
    // the sanity check below would reject them. Force-materialize
    // into a real ArrayHeader and substitute the materialized
    // pointer for every downstream accessor. O(1) on subsequent
    // calls (idempotent via the `materialized` cache).
    unsafe {
        if (cleaned as usize) >= crate::gc::GC_HEADER_SIZE + 0x1000 {
            let gc_header =
                (cleaned as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            if (*gc_header).obj_type == crate::gc::GC_TYPE_LAZY_ARRAY {
                let lazy = cleaned as *mut crate::json_tape::LazyArrayHeader;
                if (*lazy).magic == crate::json_tape::LAZY_ARRAY_MAGIC {
                    let materialized = crate::json_tape::force_materialize_lazy(lazy);
                    return materialized as *const ArrayHeader;
                }
            }
        }
    }
    // Length/capacity sanity: a real ArrayHeader has length <= capacity,
    // and length below 100M (800 MB of element payload — well above
    // legitimate large result sets, far below the 775M / 926M patterns
    // we observed when a reused arena slot landed ASCII text at offsets
    // 0/4). Buffers can be much larger than arrays, so only gate the
    // polymorphic entry on the tighter array-sized bound and let
    // buffer-specific runtime paths dispatch themselves when they
    // recognize a registered buffer pointer.
    unsafe {
        let hdr = &*cleaned;
        if hdr.length > hdr.capacity || hdr.length > 100_000_000 {
            // Allow very large BUFFERS to pass — a postgres frame can
            // be 64MB+ of bytes (capacity in the buffer case) with
            // length up to capacity. Detect registered buffers and
            // wave them through; everything else at this size is
            // almost certainly corrupted.
            let addr = cleaned as usize;
            if !crate::buffer::is_registered_buffer(addr)
                && crate::typedarray::lookup_typed_array_kind(addr).is_none()
            {
                return std::ptr::null();
            }
        }
    }
    cleaned
}

#[inline(always)]
fn clean_arr_ptr_mut(arr: *mut ArrayHeader) -> *mut ArrayHeader {
    clean_arr_ptr(arr as *const ArrayHeader) as *mut ArrayHeader
}

/// Array header - precedes the elements in memory
#[repr(C)]
pub struct ArrayHeader {
    /// Number of elements in the array
    pub length: u32,
    /// Capacity (allocated space for elements)
    pub capacity: u32,
}

/// Calculate the byte size for an array with N elements capacity
#[inline]
fn array_byte_size(capacity: usize) -> usize {
    std::mem::size_of::<ArrayHeader>() + capacity * std::mem::size_of::<f64>()
}

/// Minimum initial capacity for arrays to reduce reallocations
const MIN_ARRAY_CAPACITY: u32 = 16;

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
            std::ptr::write(elements_ptr.add(i), crate::value::TAG_HOLE);
        }
    }

    ptr
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
        ptr::copy_nonoverlapping(elements, arr_elements, count as usize);
    }
    arr
}

/// `Array.from({length: N, 0: a, 1: b, ...})` — read the `length` property
/// and emit `obj[0]..obj[N-1]` in order (missing slots fill with `undefined`
/// per spec). Receivers without a numeric `length` property produce an
/// empty array (ToLength coerces non-numbers to 0).
unsafe fn js_array_from_arraylike(obj: *const crate::object::ObjectHeader) -> *mut ArrayHeader {
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
    let elements = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    let undefined = f64::from_bits(TAG_UNDEFINED);
    for i in 0..len {
        // Pre-init to undefined in case the key lookup returns the
        // wrong type / produces a sentinel we want to coerce.
        *elements.add(i as usize) = undefined;
        let key_str = i.to_string();
        let key = crate::string::js_string_from_bytes(key_str.as_ptr(), key_str.len() as u32);
        let v = crate::object::js_object_get_field_by_name_f64(obj, key);
        *elements.add(i as usize) = v;
    }
    arr
}

/// `Array.from(string)` — split the source string into Unicode codepoints
/// and emit each as a 1-codepoint string element (matches `[..."hello"]` /
/// `for (const c of "hello")` semantics). Surrogate pairs in UTF-16 source
/// space materialize as a single codepoint per ECMA-262 §22.1.5 String
/// Iterator Records, so `[..."🎉"]` yields a 1-element array (not 2).
unsafe fn js_array_from_string_codepoints(
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
    let elements = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
    for (i, ch) in src.chars().enumerate() {
        let mut buf = [0u8; 4];
        let s_ref = ch.encode_utf8(&mut buf);
        let s_ptr = crate::string::js_string_from_bytes(s_ref.as_ptr(), s_ref.len() as u32);
        *elements.add(i) = crate::value::js_nanbox_string(s_ptr as i64);
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
    }
    ptr
}

/// Issue #179 Phase 2: if `arr` points at a `LazyArrayHeader`
/// (`GcHeader::obj_type == GC_TYPE_LAZY_ARRAY`), force the lazy
/// value to materialize and return the real `ArrayHeader` pointer.
/// Otherwise returns `arr` unchanged. Every array accessor that
/// doesn't have a lazy-specific fast path (only `.length` does)
/// should funnel through this so correctness is preserved under
/// arbitrary JS code.
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

/// Get the length of an array
/// Also handles Sets and Maps via registry check (for-of iteration treats them as arrays)
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
    unsafe {
        let length = (*arr).length;

        // If index is within bounds, just set it
        if index < length {
            let elements_ptr = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
            ptr::write(elements_ptr.add(index as usize), value);
            return arr;
        }

        // Need to extend the array
        let new_length = index + 1;
        let arr = if new_length > (*arr).capacity {
            js_array_grow(arr, new_length)
        } else {
            arr
        };

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
        }

        // Set the value
        ptr::write(elements_ptr.add(index as usize), value);
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

/// Grow the array to at least the given capacity
/// Returns a new pointer (the old one may be invalid after this)
#[no_mangle]
pub extern "C" fn js_array_grow(arr: *mut ArrayHeader, min_capacity: u32) -> *mut ArrayHeader {
    if arr.is_null() || (arr as usize) < 0x1000 {
        return js_array_alloc(min_capacity);
    }
    // Issue #233: resolve any existing forwarding chain before deciding
    // whether to grow — caller may pass a stale pre-grow pointer.
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        return js_array_alloc(min_capacity);
    }
    unsafe {
        let old_capacity = (*arr).capacity;
        if min_capacity <= old_capacity {
            return arr;
        }

        // Double the capacity, or use min_capacity if larger
        let new_capacity = std::cmp::max(old_capacity * 2, min_capacity);
        let old_size = array_byte_size(old_capacity as usize);
        let new_size = array_byte_size(new_capacity as usize);

        // Allocate new from arena and copy old data.
        let new_ptr = arena_alloc_gc(new_size, 8, crate::gc::GC_TYPE_ARRAY) as *mut ArrayHeader;
        ptr::copy_nonoverlapping(arr as *const u8, new_ptr as *mut u8, old_size);

        (*new_ptr).capacity = new_capacity;

        // Issue #233: install a forwarding pointer at the OLD location
        // so any stale reference (e.g. an async function's caller still
        // holding the pre-grow pointer in its parameter slot) resolves
        // to the new head via clean_arr_ptr's GC_FLAG_FORWARDED follow.
        // Reuses the GC's existing evac-forwarding mechanism — first 8
        // bytes of payload (length+capacity) become the new user ptr.
        // Only valid for arena-allocated arrays (which have a GcHeader
        // 8 bytes before the user pointer); guard with a heap-bounds
        // check that mirrors clean_arr_ptr's HEAP_MIN to skip pointers
        // that don't have a real GcHeader behind them (e.g. test-mode
        // synthetic pointers, longlived-arena edge cases).
        #[cfg(any(target_os = "android", target_os = "linux", target_os = "windows"))]
        const HEAP_MIN: usize = 0x1000;
        #[cfg(not(any(target_os = "android", target_os = "linux", target_os = "windows")))]
        const HEAP_MIN: usize = 0x200_0000_0000;
        if (arr as usize) >= HEAP_MIN + crate::gc::GC_HEADER_SIZE {
            let old_header =
                (arr as *mut u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
            // Only forward arrays that came from the GC arena. A
            // non-array obj_type would mean something has gone wrong
            // upstream; bail out without forwarding rather than corrupt
            // an unrelated allocation's header.
            if (*old_header).obj_type == crate::gc::GC_TYPE_ARRAY {
                crate::gc::set_forwarding_address(old_header, new_ptr as *mut u8);
            }
        }

        new_ptr
    }
}

/// Push an element to the end of an array, growing if needed
/// Returns a pointer to the (possibly reallocated) array
#[no_mangle]
pub extern "C" fn js_array_push_f64(arr: *mut ArrayHeader, value: f64) -> *mut ArrayHeader {
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        return js_array_alloc(0);
    }
    unsafe {
        let length = (*arr).length;
        let capacity = (*arr).capacity;

        let arr = if length >= capacity {
            js_array_grow(arr, length + 1)
        } else {
            arr
        };

        let elements_ptr = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
        ptr::write(elements_ptr.add(length as usize), value);
        (*arr).length = length + 1;
        arr
    }
}

/// Push every element of `source` to the end of `target`, growing as needed.
/// Returns a pointer to the (possibly reallocated) target. Refs #488
/// drizzle-sqlite: drizzle's `mergeQueries` does
/// `result.params.push(...query.params)` which the HIR lowers to
/// `NativeMethodCall { module: "array", method: "push_spread" }` —
/// pre-fix, codegen had no arm for `push_spread`, falling through to the
/// "Unknown native method" catch-all that lowered receiver+args for side
/// effects and returned the `0.0` sentinel. The push never happened and
/// SQL queries went out with 0 params, so INSERT silently inserted
/// nothing and SELECT returned `count=0`. This helper plus the
/// matching codegen arm in `lower_native_method_call` does the actual
/// push loop.
#[no_mangle]
pub extern "C" fn js_array_push_spread_f64(
    target: *mut ArrayHeader,
    source: *const ArrayHeader,
) -> *mut ArrayHeader {
    let source = clean_arr_ptr(source);
    if source.is_null() {
        return target;
    }
    unsafe {
        let src_len = (*source).length;
        if src_len == 0 {
            return target;
        }
        let src_elements_ptr =
            (source as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
        let mut current = target;
        for i in 0..src_len {
            let value = *src_elements_ptr.add(i as usize);
            current = js_array_push_f64(current, value);
        }
        current
    }
}

/// Pop an element from the end of an array.
/// Returns the removed element, or `undefined` if the array is empty (per
/// ECMAScript §23.1.3.21 — `Array.prototype.pop` on an empty array returns
/// undefined, NOT NaN). Pre-fix this returned `f64::NAN` (bare NaN bits,
/// which compare `!== undefined`); callers like `@perryts/mysql`'s pool
/// `acquire()` did `const entry = this.idle.shift(); if (entry !== undefined)`
/// and took the wrong branch on an empty pool. Issue #536.
#[no_mangle]
pub extern "C" fn js_array_pop_f64(arr: *mut ArrayHeader) -> f64 {
    const TAG_UNDEFINED_F64: f64 = f64::from_bits(0x7FFC_0000_0000_0001u64);
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        return TAG_UNDEFINED_F64;
    }
    unsafe {
        let length = (*arr).length;
        if length == 0 {
            return TAG_UNDEFINED_F64;
        }

        let new_length = length - 1;
        let elements_ptr = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
        let value = *elements_ptr.add(new_length as usize);
        (*arr).length = new_length;
        value
    }
}

/// Set the length of an array, JS-spec style.
///
/// Closes #304: `arr.length = N` must truncate when N < length and pad with
/// `undefined` when N > length. Pre-fix Perry routed this through the generic
/// `js_object_set_field_by_name(obj, "length", N)` path which silently set a
/// new "length" property on the array's hidden object dispatch but never
/// touched the `ArrayHeader.length` field — so `arr.length` still read back
/// the original value, and the elements were never cleared.
///
/// `new_length` arrives as f64 from the codegen (assignment value is a
/// JSValue). Truncates to u32 with NaN/negative/non-integer clamped to 0
/// (the spec throws RangeError; we clamp for now since Perry's exception
/// surface is incomplete in places — issue worth a follow-up).
#[no_mangle]
pub extern "C" fn js_array_set_length(arr: *mut ArrayHeader, new_length: f64) {
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        return;
    }
    let n: u32 = if new_length.is_nan() || new_length < 0.0 || new_length > u32::MAX as f64 {
        0
    } else {
        new_length as u32
    };
    unsafe {
        let cur = (*arr).length;
        if n < cur {
            // Truncate: clear elements at indices [n..cur) to TAG_UNDEFINED so
            // any code that resurrects the slot via `arr[i]` reads `undefined`,
            // not stale data. The capacity stays unchanged — JS doesn't
            // require Perry to release the underlying buffer here, and growing
            // back via `push` would just re-overwrite these slots anyway.
            const TAG_UNDEFINED_F64: f64 = f64::from_bits(0x7FFC_0000_0000_0001u64);
            let elements_ptr = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
            for i in n..cur {
                std::ptr::write(elements_ptr.add(i as usize), TAG_UNDEFINED_F64);
            }
            (*arr).length = n;
        } else if n > cur {
            // Extend: pad with TAG_UNDEFINED. Past-capacity extensions go
            // through `js_array_grow` which installs a forwarding pointer at
            // the OLD location (issue #233 mechanism), so the caller's stale
            // pointer transparently follows the chain to the resized buffer
            // on the next access — no callsite-side writeback needed.
            const TAG_UNDEFINED_F64: f64 = f64::from_bits(0x7FFC_0000_0000_0001u64);
            let target = if n > (*arr).capacity {
                js_array_grow(arr, n)
            } else {
                arr
            };
            if !target.is_null() {
                let elements_ptr =
                    (target as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
                for i in cur..n {
                    std::ptr::write(elements_ptr.add(i as usize), TAG_UNDEFINED_F64);
                }
                (*target).length = n;
            }
        }
        // n == cur is a no-op.
    }
}

/// Delete an element from an array by index, creating a "hole".
/// Sets the element to undefined without changing the array length.
/// Matches JavaScript `delete arr[index]` semantics.
/// Returns 1 (true) on success, 0 (false) on failure.
#[no_mangle]
pub extern "C" fn js_array_delete(arr: *mut ArrayHeader, index: u32) -> i32 {
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        return 1;
    }
    unsafe {
        let length = (*arr).length;
        if index >= length {
            return 1; // delete on out-of-bounds always returns true in JS
        }
        const TAG_UNDEFINED_F64: f64 = f64::from_bits(0x7FFC_0000_0000_0001u64);
        let elements_ptr = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
        std::ptr::write(elements_ptr.add(index as usize), TAG_UNDEFINED_F64);
        1
    }
}

/// Shift an element from the beginning of an array.
/// Returns the removed element, or `undefined` if the array is empty (per
/// ECMAScript §23.1.3.27). See the matching note on `js_array_pop_f64` —
/// returning bare `f64::NAN` here was a perry bug that broke the
/// `entry !== undefined` check in connection-pool drivers like
/// `@perryts/mysql`. Issue #536.
#[no_mangle]
pub extern "C" fn js_array_shift_f64(arr: *mut ArrayHeader) -> f64 {
    const TAG_UNDEFINED_F64: f64 = f64::from_bits(0x7FFC_0000_0000_0001u64);
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        return TAG_UNDEFINED_F64;
    }
    unsafe {
        let length = (*arr).length;
        if length == 0 {
            return TAG_UNDEFINED_F64;
        }

        let elements_ptr = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
        let value = *elements_ptr;

        // Shift all elements down
        ptr::copy(elements_ptr.add(1), elements_ptr, (length - 1) as usize);
        (*arr).length = length - 1;
        value
    }
}

/// Unshift an element to the beginning of an array, growing if needed
/// Returns a pointer to the (possibly reallocated) array
#[no_mangle]
pub extern "C" fn js_array_unshift_f64(arr: *mut ArrayHeader, value: f64) -> *mut ArrayHeader {
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        return js_array_alloc(0);
    }
    unsafe {
        let length = (*arr).length;
        let capacity = (*arr).capacity;

        let arr = if length >= capacity {
            js_array_grow(arr, length + 1)
        } else {
            arr
        };

        let elements_ptr = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;

        // Shift all elements up
        ptr::copy(elements_ptr, elements_ptr.add(1), length as usize);
        // Write new element at beginning
        ptr::write(elements_ptr, value);
        (*arr).length = length + 1;
        arr
    }
}

/// Unshift an element as raw JSValue bits (u64), for object/pointer values
/// Returns a pointer to the (possibly reallocated) array
#[no_mangle]
pub extern "C" fn js_array_unshift_jsvalue(arr: *mut ArrayHeader, value: u64) -> *mut ArrayHeader {
    let bits_as_f64 = f64::from_bits(value);
    js_array_unshift_f64(arr, bits_as_f64)
}

/// Find the index of an element in an array
/// Returns -1 if not found
#[no_mangle]
pub extern "C" fn js_array_indexOf_f64(arr: *const ArrayHeader, value: f64) -> i32 {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return -1;
    }
    unsafe {
        let length = (*arr).length;
        let elements_ptr = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;

        for i in 0..length as usize {
            if *elements_ptr.add(i) == value {
                return i as i32;
            }
        }
        -1
    }
}

/// indexOf for arrays, using jsvalue comparison (handles NaN-boxed strings correctly)
#[no_mangle]
pub extern "C" fn js_array_indexOf_jsvalue(arr: *const ArrayHeader, value: f64) -> i32 {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return -1;
    }
    unsafe {
        let length = (*arr).length;
        let elements_ptr = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
        for i in 0..length as usize {
            let element = *elements_ptr.add(i);
            if crate::value::js_jsvalue_equals(element, value) == 1 {
                return i as i32;
            }
        }
        -1
    }
}

/// Check if an array includes a value
/// Returns 1 if found, 0 if not
#[no_mangle]
pub extern "C" fn js_array_includes_f64(arr: *const ArrayHeader, value: f64) -> i32 {
    if js_array_indexOf_f64(arr, value) >= 0 {
        1
    } else {
        0
    }
}

/// Check if an array includes a value using deep equality comparison.
/// This handles NaN-boxed strings by comparing string contents.
/// Returns 1 if found, 0 if not.
#[no_mangle]
pub extern "C" fn js_array_includes_jsvalue(arr: *const ArrayHeader, value: f64) -> i32 {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return 0;
    }
    unsafe {
        let length = (*arr).length;
        let elements_ptr = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;

        // `Array.prototype.includes` uses SameValueZero (ECMA-262 §23.1.3.16),
        // which differs from === in one place: NaN equals NaN. Routing
        // through `js_jsvalue_same_value_zero` preserves the `indexOf(NaN) ===
        // -1` / `includes(NaN) === true` split.
        for i in 0..length as usize {
            let element = *elements_ptr.add(i);
            if crate::value::js_jsvalue_same_value_zero(element, value) == 1 {
                return 1;
            }
        }
        0
    }
}

/// Splice an array - removes elements and optionally inserts new ones
/// start: starting index (can be negative for from-end)
/// delete_count: number of elements to delete
/// items: pointer to elements to insert (can be null if no items)
/// items_count: number of elements to insert
/// Returns a new array containing the deleted elements
/// ALSO modifies arr in place, returns the modified array pointer (may have reallocated)
/// The return is packed: lower 48 bits = deleted array ptr, we return via out param
#[no_mangle]
pub extern "C" fn js_array_splice(
    arr: *mut ArrayHeader,
    start: i32,
    delete_count: i32,
    items: *const f64,
    items_count: u32,
    out_arr: *mut *mut ArrayHeader,
) -> *mut ArrayHeader {
    unsafe {
        let arr = clean_arr_ptr_mut(arr);
        if arr.is_null() {
            if !out_arr.is_null() {
                *out_arr = js_array_alloc(0);
            }
            return js_array_alloc(0);
        }
        let len = (*arr).length as i32;

        // Normalize start index
        let start_idx = if start < 0 {
            (len + start).max(0) as u32
        } else {
            (start as u32).min(len as u32)
        };

        // Normalize delete count
        let actual_delete = if delete_count < 0 {
            0
        } else {
            (delete_count as u32).min(len as u32 - start_idx)
        };

        // Create array of deleted elements
        let deleted = js_array_alloc(actual_delete);
        (*deleted).length = actual_delete;

        let elements_ptr = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
        let deleted_elements =
            (deleted as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;

        // Copy deleted elements to return array
        for i in 0..actual_delete as usize {
            ptr::write(
                deleted_elements.add(i),
                *elements_ptr.add(start_idx as usize + i),
            );
        }

        // Calculate new length
        let new_len = (len as u32 - actual_delete + items_count) as u32;

        // Grow array if needed
        let arr = if new_len > (*arr).capacity {
            js_array_grow(arr, new_len)
        } else {
            arr
        };
        let elements_ptr = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;

        // Shift elements after the splice point
        let tail_start = start_idx + actual_delete;
        let tail_len = len as u32 - tail_start;

        if items_count != actual_delete && tail_len > 0 {
            // Need to shift the tail
            let src = elements_ptr.add(tail_start as usize);
            let dst = elements_ptr.add((start_idx + items_count) as usize);
            ptr::copy(src, dst, tail_len as usize);
        }

        // Insert new items
        if items_count > 0 && !items.is_null() {
            for i in 0..items_count as usize {
                ptr::write(elements_ptr.add(start_idx as usize + i), *items.add(i));
            }
        }

        (*arr).length = new_len;

        // Return modified array via out param
        *out_arr = arr;

        deleted
    }
}

/// Slice an array, returning a new array with elements from start to end (exclusive)
/// Handles negative indices (from end of array)
/// If end is i32::MAX, slices to end of array
#[no_mangle]
pub extern "C" fn js_array_slice(
    arr: *const ArrayHeader,
    start: i32,
    end: i32,
) -> *mut ArrayHeader {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return js_array_alloc(0);
    }
    unsafe {
        let len = (*arr).length as i32;

        // Normalize start index
        let start_idx = if start < 0 {
            (len + start).max(0) as u32
        } else {
            (start as u32).min(len as u32)
        };

        // Normalize end index
        let end_idx = if end == i32::MAX {
            len as u32
        } else if end < 0 {
            (len + end).max(0) as u32
        } else {
            (end as u32).min(len as u32)
        };

        // Calculate slice length
        let slice_len = end_idx.saturating_sub(start_idx);

        // Allocate new array
        let result = js_array_alloc(slice_len);
        (*result).length = slice_len;

        // Copy elements
        let src_elements = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
        let dst_elements = (result as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;

        for i in 0..slice_len as usize {
            ptr::write(
                dst_elements.add(i),
                ptr::read(src_elements.add(start_idx as usize + i)),
            );
        }

        result
    }
}

// ============================================================================
// JSValue-based array functions (for stdlib convenience)
// These store JSValue bits as f64 for uniform storage
// ============================================================================

use crate::value::JSValue;

/// Set an element using JSValue
#[no_mangle]
pub extern "C" fn js_array_set(arr: *mut ArrayHeader, index: u32, value: JSValue) {
    // Convert JSValue bits to f64 for storage
    let bits_as_f64 = f64::from_bits(value.bits());
    js_array_set_f64(arr, index, bits_as_f64);
}

/// Get an element as JSValue
#[no_mangle]
pub extern "C" fn js_array_get(arr: *const ArrayHeader, index: u32) -> JSValue {
    let bits_as_f64 = js_array_get_f64(arr, index);
    JSValue::from_bits(bits_as_f64.to_bits())
}

/// Push a JSValue to the array
#[no_mangle]
pub extern "C" fn js_array_push(arr: *mut ArrayHeader, value: JSValue) -> *mut ArrayHeader {
    let bits_as_f64 = f64::from_bits(value.bits());
    js_array_push_f64(arr, bits_as_f64)
}

/// Allocate and initialize an array from a list of JSValue (stored as u64 bits)
/// This is used for mixed-type arrays where elements can be numbers, strings, objects, etc.
#[no_mangle]
pub extern "C" fn js_array_from_jsvalue(elements: *const u64, count: u32) -> *mut ArrayHeader {
    let arr = js_array_alloc(count);
    unsafe {
        (*arr).length = count;
        let arr_elements = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
        // Each u64 contains NaN-boxed JSValue bits, store as f64 bits
        for i in 0..count as usize {
            let bits = *elements.add(i);
            ptr::write(arr_elements.add(i), f64::from_bits(bits));
        }
    }
    arr
}

/// Get an element from a mixed-type array (returns raw u64 bits for JSValue)
#[no_mangle]
pub extern "C" fn js_array_get_jsvalue(arr: *const ArrayHeader, index: u32) -> u64 {
    let bits_as_f64 = js_array_get_f64(arr, index);
    bits_as_f64.to_bits()
}

/// Set an element in a mixed-type array (value is raw u64 bits for JSValue)
#[no_mangle]
pub extern "C" fn js_array_set_jsvalue(arr: *mut ArrayHeader, index: u32, value: u64) {
    let bits_as_f64 = f64::from_bits(value);
    js_array_set_f64(arr, index, bits_as_f64);
}

/// Set an element in a mixed-type array, extending the array if needed.
/// Returns the (possibly reallocated) array pointer.
#[no_mangle]
pub extern "C" fn js_array_set_jsvalue_extend(
    arr: *mut ArrayHeader,
    index: u32,
    value: u64,
) -> *mut ArrayHeader {
    let bits_as_f64 = f64::from_bits(value);
    js_array_set_f64_extend(arr, index, bits_as_f64)
}

/// Push a JSValue (as u64 bits) to a mixed-type array
#[no_mangle]
pub extern "C" fn js_array_push_jsvalue(arr: *mut ArrayHeader, value: u64) -> *mut ArrayHeader {
    let bits_as_f64 = f64::from_bits(value);
    js_array_push_f64(arr, bits_as_f64)
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
        if !dest_resolved.is_null() && dest_resolved as *const _ != src {
            let dest_len = (*dest_resolved).length;
            let new_len = dest_len + src_len;
            let result = if new_len > (*dest_resolved).capacity {
                js_array_grow(dest_resolved, new_len)
            } else {
                dest_resolved
            };
            let dst_elements =
                (result as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
            ptr::copy_nonoverlapping(
                src_elements,
                dst_elements.add(dest_len as usize),
                src_len as usize,
            );
            (*result).length = new_len;
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
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        return arr;
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
            *elements.add(i) = *elements.add(j);
            *elements.add(j) = tmp;
            i += 1;
            j -= 1;
        }
        arr
    }
}

/// `Array.prototype.fill(value)` — fills every element (0..length) with
/// `value`. Returns the same array pointer.
#[no_mangle]
pub extern "C" fn js_array_fill(arr: *mut ArrayHeader, value: f64) -> *mut ArrayHeader {
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        return arr;
    }
    unsafe {
        let len = (*arr).length as usize;
        if len == 0 {
            return arr;
        }
        let elements = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
        for i in 0..len {
            *elements.add(i) = value;
        }
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
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        return arr;
    }
    unsafe {
        let len = (*arr).length as i64;
        if len == 0 {
            return arr;
        }
        let clamp = |idx: f64, default_to_len: bool| -> i64 {
            if idx.is_nan() {
                return 0;
            }
            let mut i = idx as i64;
            if idx.is_infinite() {
                if idx > 0.0 {
                    return len;
                }
                if default_to_len {
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
            *elements.add(i as usize) = value;
        }
        arr
    }
}

/// `Array.prototype.sort()` — default sort with no comparator. Per JS
/// semantics, elements are converted to strings and compared
/// lexicographically. Sorts in place and returns the same array pointer.
#[no_mangle]
pub extern "C" fn js_array_sort_default(arr: *mut ArrayHeader) -> *mut ArrayHeader {
    use crate::string::StringHeader;
    use crate::value::js_jsvalue_to_string;
    unsafe {
        let arr = clean_arr_ptr(arr as *const ArrayHeader) as *mut ArrayHeader;
        if arr.is_null() {
            return arr;
        }
        // Issue #654: route typed-array receivers (compiler statically
        // typed `arr` as `Float64Array | Int32Array | …` and emitted the
        // ArraySort lowering) through the typed-array sorter so element
        // bytes are read by the right per-kind accessor instead of as
        // raw f64. Without this, `Int8Array.sort()` produced 4 i8 cells
        // re-interpreted as 8-byte f64s — garbage values + occasional
        // OOB reads.
        if crate::typedarray::lookup_typed_array_kind(arr as usize).is_some() {
            return crate::typedarray::js_typed_array_sort_default(
                arr as *mut crate::typedarray::TypedArrayHeader,
            ) as *mut ArrayHeader;
        }
        let length = (*arr).length as usize;
        if length <= 1 {
            return arr;
        }
        let elements_ptr = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;

        // Materialize each element as an owned Rust `String` while keeping the
        // original f64 bits. Using strings (not pointer equality) guarantees
        // correct ordering for numbers, NaN-boxed strings, booleans, null and
        // undefined — matching JS default sort semantics.
        let mut pairs: Vec<(String, f64)> = Vec::with_capacity(length);
        for i in 0..length {
            let val = *elements_ptr.add(i);
            let str_ptr = js_jsvalue_to_string(val);
            let s = if str_ptr.is_null() {
                String::new()
            } else {
                let header = &*(str_ptr as *const StringHeader);
                let bytes_ptr = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                let slice = std::slice::from_raw_parts(bytes_ptr, header.byte_len as usize);
                std::str::from_utf8(slice).unwrap_or("").to_string()
            };
            pairs.push((s, val));
        }

        // Stable lexicographic sort on the string keys.
        pairs.sort_by(|a, b| a.0.cmp(&b.0));

        for (i, (_, val)) in pairs.into_iter().enumerate() {
            *elements_ptr.add(i) = val;
        }

        arr
    }
}

/// `Array.prototype.flat(depth)` — flatten up to `depth` levels deep
/// (ECMA-262 §23.1.3.10). `depth == 0` returns a shallow copy; `Infinity`
/// flattens fully; NaN / negative → 0. Skips Set / Map / non-array
/// pointer-like values during descent (matches the depth=1 helper's
/// dispatch). Used by codegen for `arr.flat(d)` with any non-zero arg
/// count; `flat()` keeps the legacy 1-arg `js_array_flat` fast path.
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
            // NaN-box the inner array pointer so the outer storage slot keeps tag info.
            *dst_elements.add(i) = crate::value::js_nanbox_pointer(pair as i64);
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
        }
        result
    }
}

// ============================================================================
// Array higher-order function methods
// These use closure pointers to call the callback function
// ============================================================================

use crate::closure::{js_closure_call2, js_closure_call3, ClosureHeader};

/// forEach - call callback(element, index) for each element
/// Returns nothing (void)
#[no_mangle]
pub extern "C" fn js_array_forEach(arr: *const ArrayHeader, callback: *const ClosureHeader) {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return;
    }
    unsafe {
        let length = (*arr).length;
        let elements_ptr = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;

        for i in 0..length as usize {
            let element = *elements_ptr.add(i);
            // Pass both element and index to match JS forEach(element, index, array) semantics.
            // Using call2 prevents x86_64 SIGSEGV from garbage in the uninitialized index register.
            js_closure_call2(callback, element, i as f64);
        }
    }
}

/// map - create new array by calling callback(element) on each element
/// Returns pointer to new array
#[no_mangle]
pub extern "C" fn js_array_map(
    arr: *const ArrayHeader,
    callback: *const ClosureHeader,
) -> *mut ArrayHeader {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return js_array_alloc(0);
    }
    unsafe {
        let length = (*arr).length;
        let elements_ptr = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;

        // Allocate result array with same capacity
        let result = js_array_alloc(length);
        let result_elements =
            (result as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;

        for i in 0..length as usize {
            let element = *elements_ptr.add(i);
            // Pass both element and index — JS .map() callback receives (element, index, array).
            // Using call2 ensures the index parameter is defined instead of garbage from registers,
            // which caused SIGSEGV on x86_64 when callbacks used the index (e.g., (_, i) => obj[i]).
            let mapped = js_closure_call2(callback, element, i as f64);
            ptr::write(result_elements.add(i), mapped);
        }
        (*result).length = length;

        result
    }
}

/// sort - sort array in-place using a comparator closure
/// The comparator takes (a, b) and returns negative if a < b, positive if a > b, 0 if equal
/// Returns the same array pointer (sorts in-place)
#[no_mangle]
pub extern "C" fn js_array_sort_with_comparator(
    arr: *mut ArrayHeader,
    comparator: *const ClosureHeader,
) -> *mut ArrayHeader {
    unsafe {
        let arr = clean_arr_ptr(arr as *const ArrayHeader) as *mut ArrayHeader;
        if arr.is_null() {
            return arr;
        }
        // Issue #654: same routing as `js_array_sort_default` — when
        // codegen statically typed the receiver as a typed array but
        // chose the generic ArraySort HIR lowering, dispatch through
        // the typed-array helper instead of treating the buffer as f64s.
        if crate::typedarray::lookup_typed_array_kind(arr as usize).is_some() {
            return crate::typedarray::js_typed_array_sort_with_comparator(
                arr as *mut crate::typedarray::TypedArrayHeader,
                comparator,
            ) as *mut ArrayHeader;
        }
        let length = (*arr).length as usize;
        if length <= 1 {
            return arr;
        }
        let elements_ptr = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;

        // Hoist the closure-dispatch resolution out of the hot loops.
        // For a 1.25M-element sort we'd otherwise hit ~50M HashMap lookups
        // (per-call rest + arity registry queries inside `js_closure_call2`).
        // When the comparator is a plain (a,b) => ... arrow with no captures
        // and no rest, `direct_call` is `Some(typed_fn)` and we call it
        // unconditionally inside the loop. Falls back to `js_closure_call2`
        // for the rare bound-method / rest / over-arity comparators.
        let direct_call = crate::closure::resolve_call2_direct(comparator);

        #[inline(always)]
        unsafe fn cmp_with(
            comparator: *const ClosureHeader,
            direct: Option<extern "C" fn(*const ClosureHeader, f64, f64) -> f64>,
            a: f64,
            b: f64,
        ) -> f64 {
            match direct {
                Some(f) => f(comparator, a, b),
                None => js_closure_call2(comparator, a, b),
            }
        }

        // TimSort-style hybrid: insertion sort for small runs, merge sort for large arrays.
        // Stable, O(n log n) worst case. Insertion sort is used for runs <= 32 elements
        // because it has lower overhead for small inputs.
        const INSERTION_THRESHOLD: usize = 32;

        if length <= INSERTION_THRESHOLD {
            // Insertion sort for small arrays
            for i in 1..length {
                let key = *elements_ptr.add(i);
                let mut j = i as isize - 1;
                while j >= 0 {
                    let cmp = cmp_with(comparator, direct_call, *elements_ptr.add(j as usize), key);
                    if cmp > 0.0 {
                        ptr::write(
                            elements_ptr.add((j + 1) as usize),
                            *elements_ptr.add(j as usize),
                        );
                        j -= 1;
                    } else {
                        break;
                    }
                }
                ptr::write(elements_ptr.add((j + 1) as usize), key);
            }
        } else {
            // Bottom-up merge sort for large arrays — O(n log n) stable sort
            let mut buf: Vec<f64> = Vec::with_capacity(length);
            buf.set_len(length);

            // Phase 1: Sort small runs with insertion sort
            let mut run_start = 0;
            while run_start < length {
                let run_end = (run_start + INSERTION_THRESHOLD).min(length);
                for i in (run_start + 1)..run_end {
                    let key = *elements_ptr.add(i);
                    let mut j = i as isize - 1;
                    while j >= run_start as isize {
                        let cmp =
                            cmp_with(comparator, direct_call, *elements_ptr.add(j as usize), key);
                        if cmp > 0.0 {
                            ptr::write(
                                elements_ptr.add((j + 1) as usize),
                                *elements_ptr.add(j as usize),
                            );
                            j -= 1;
                        } else {
                            break;
                        }
                    }
                    ptr::write(elements_ptr.add((j + 1) as usize), key);
                }
                run_start = run_end;
            }

            // Phase 2: Merge runs, doubling width each pass
            let buf_ptr = buf.as_mut_ptr();
            let mut width = INSERTION_THRESHOLD;
            let mut src = elements_ptr;
            let mut dst = buf_ptr;

            while width < length {
                let mut i = 0;
                while i < length {
                    let left = i;
                    let mid = (i + width).min(length);
                    let right = (i + 2 * width).min(length);

                    // Merge [left..mid) and [mid..right) into dst
                    let mut l = left;
                    let mut r = mid;
                    let mut k = left;
                    while l < mid && r < right {
                        let cmp = cmp_with(comparator, direct_call, *src.add(l), *src.add(r));
                        if cmp <= 0.0 {
                            *dst.add(k) = *src.add(l);
                            l += 1;
                        } else {
                            *dst.add(k) = *src.add(r);
                            r += 1;
                        }
                        k += 1;
                    }
                    while l < mid {
                        *dst.add(k) = *src.add(l);
                        l += 1;
                        k += 1;
                    }
                    while r < right {
                        *dst.add(k) = *src.add(r);
                        r += 1;
                        k += 1;
                    }

                    i += 2 * width;
                }
                // Swap src and dst for next pass
                std::mem::swap(&mut src, &mut dst);
                width *= 2;
            }

            // If final result is in buf, copy back to elements
            if src != elements_ptr {
                ptr::copy_nonoverlapping(src, elements_ptr, length);
            }
        }

        arr
    }
}

/// filter - create new array with elements where callback(element) returns truthy
/// Returns pointer to new array
#[no_mangle]
pub extern "C" fn js_array_filter(
    arr: *const ArrayHeader,
    callback: *const ClosureHeader,
) -> *mut ArrayHeader {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return js_array_alloc(0);
    }
    unsafe {
        let length = (*arr).length;
        let elements_ptr = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;

        // Allocate result array with same capacity (might be smaller)
        let mut result = js_array_alloc(length);
        let mut result_len = 0u32;

        for i in 0..length as usize {
            let element = *elements_ptr.add(i);
            let keep = js_closure_call2(callback, element, i as f64);
            // Proper truthy check: handles NaN-boxed booleans (TAG_FALSE != 0.0 but is falsy)
            if crate::value::js_is_truthy(keep) != 0 {
                result = js_array_push_f64(result, element);
                result_len += 1;
            }
        }

        result
    }
}

/// find - find first element that matches callback(element) => true
/// Returns the element as f64, or f64::NAN (undefined) if not found
#[no_mangle]
pub extern "C" fn js_array_find(arr: *const ArrayHeader, callback: *const ClosureHeader) -> f64 {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    unsafe {
        let length = (*arr).length;
        let elements_ptr = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;

        for i in 0..length as usize {
            let element = *elements_ptr.add(i);
            let result = js_closure_call2(callback, element, i as f64);
            // Proper truthy check: handles NaN-boxed booleans
            if crate::value::js_is_truthy(result) != 0 {
                return element;
            }
        }

        // Not found - return undefined (NaN)
        f64::NAN
    }
}

/// findIndex - find index of first element that matches callback(element) => true
/// Returns the index as i32, or -1 if not found
#[no_mangle]
pub extern "C" fn js_array_findIndex(
    arr: *const ArrayHeader,
    callback: *const ClosureHeader,
) -> i32 {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return -1;
    }
    unsafe {
        let length = (*arr).length;
        let elements_ptr = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;

        for i in 0..length as usize {
            let element = *elements_ptr.add(i);
            let result = js_closure_call2(callback, element, i as f64);
            // Proper truthy check: handles NaN-boxed booleans
            if crate::value::js_is_truthy(result) != 0 {
                return i as i32;
            }
        }

        // Not found
        -1
    }
}

/// findLast - like find but iterates from the end
#[no_mangle]
pub extern "C" fn js_array_find_last(
    arr: *const ArrayHeader,
    callback: *const ClosureHeader,
) -> f64 {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    if crate::typedarray::lookup_typed_array_kind(arr as usize).is_some() {
        return crate::typedarray::js_typed_array_find_last(
            arr as *const crate::typedarray::TypedArrayHeader,
            callback,
        );
    }
    unsafe {
        let length = (*arr).length as usize;
        let elements_ptr = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
        for i in (0..length).rev() {
            let element = *elements_ptr.add(i);
            let result = js_closure_call2(callback, element, i as f64);
            if crate::value::js_is_truthy(result) != 0 {
                return element;
            }
        }
        f64::from_bits(crate::value::TAG_UNDEFINED)
    }
}

/// findLastIndex - like findIndex but iterates from the end
#[no_mangle]
pub extern "C" fn js_array_find_last_index(
    arr: *const ArrayHeader,
    callback: *const ClosureHeader,
) -> i32 {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return -1;
    }
    if crate::typedarray::lookup_typed_array_kind(arr as usize).is_some() {
        let r = crate::typedarray::js_typed_array_find_last_index(
            arr as *const crate::typedarray::TypedArrayHeader,
            callback,
        );
        return r as i32;
    }
    unsafe {
        let length = (*arr).length as usize;
        let elements_ptr = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
        for i in (0..length).rev() {
            let element = *elements_ptr.add(i);
            let result = js_closure_call2(callback, element, i as f64);
            if crate::value::js_is_truthy(result) != 0 {
                return i as i32;
            }
        }
        -1
    }
}

/// at - element access supporting negative indices (arr.at(-1) = last)
#[no_mangle]
pub extern "C" fn js_array_at(arr: *const ArrayHeader, index: f64) -> f64 {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    // If this pointer is actually a typed-array, dispatch there. Typed arrays
    // and Uint8Array/Buffer have different layouts than ArrayHeader, and the
    // codegen happily routes their `.at(i)` through this generic helper.
    let addr = arr as usize;
    if crate::typedarray::lookup_typed_array_kind(addr).is_some() {
        return crate::typedarray::js_typed_array_at(
            addr as *const crate::typedarray::TypedArrayHeader,
            index,
        );
    }
    if crate::buffer::is_registered_buffer(addr) {
        let buf = addr as *const crate::buffer::BufferHeader;
        unsafe {
            let length = (*buf).length as i64;
            let mut idx = index as i64;
            if idx < 0 {
                idx += length;
            }
            if idx < 0 || idx >= length {
                return f64::from_bits(crate::value::TAG_UNDEFINED);
            }
            let data = (buf as *const u8).add(std::mem::size_of::<crate::buffer::BufferHeader>());
            return *data.add(idx as usize) as f64;
        }
    }
    unsafe {
        let length = (*arr).length as i64;
        let mut idx = index as i64;
        if idx < 0 {
            idx += length;
        }
        if idx < 0 || idx >= length {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }
        let elements_ptr = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
        *elements_ptr.add(idx as usize)
    }
}

/// some - returns true if any element matches callback(element) => true
/// Returns TAG_TRUE or TAG_FALSE as f64
#[no_mangle]
pub extern "C" fn js_array_some(arr: *const ArrayHeader, callback: *const ClosureHeader) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return f64::from_bits(TAG_FALSE);
    }
    unsafe {
        let length = (*arr).length;
        let elements_ptr = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;

        for i in 0..length as usize {
            let element = *elements_ptr.add(i);
            let result = js_closure_call2(callback, element, i as f64);
            if crate::value::js_is_truthy(result) != 0 {
                return f64::from_bits(TAG_TRUE);
            }
        }

        f64::from_bits(TAG_FALSE)
    }
}

/// every - returns true if all elements match callback(element) => true
/// Returns TAG_TRUE or TAG_FALSE as f64
#[no_mangle]
pub extern "C" fn js_array_every(arr: *const ArrayHeader, callback: *const ClosureHeader) -> f64 {
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return f64::from_bits(TAG_TRUE);
    }
    unsafe {
        let length = (*arr).length;
        let elements_ptr = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;

        for i in 0..length as usize {
            let element = *elements_ptr.add(i);
            let result = js_closure_call2(callback, element, i as f64);
            if crate::value::js_is_truthy(result) == 0 {
                return f64::from_bits(TAG_FALSE);
            }
        }

        f64::from_bits(TAG_TRUE)
    }
}

/// flatMap - map each element to an array, then flatten one level
/// Returns pointer to new array
#[no_mangle]
pub extern "C" fn js_array_flatMap(
    arr: *const ArrayHeader,
    callback: *const ClosureHeader,
) -> *mut ArrayHeader {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return js_array_alloc(0);
    }
    unsafe {
        let length = (*arr).length;
        let elements_ptr = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;

        let mut result = js_array_alloc(length);

        for i in 0..length as usize {
            let element = *elements_ptr.add(i);
            let mapped = js_closure_call2(callback, element, i as f64);
            // Check if the mapped value is an array (pointer-tagged)
            let bits = mapped.to_bits();
            let top16 = bits >> 48;
            if top16 == 0x7FFD {
                // NaN-boxed pointer — likely an array
                let sub_arr = (bits & 0x0000_FFFF_FFFF_FFFF) as *const ArrayHeader;
                if !sub_arr.is_null() {
                    let sub_len = (*sub_arr).length;
                    let sub_elements = (sub_arr as *const u8)
                        .add(std::mem::size_of::<ArrayHeader>())
                        as *const f64;
                    for j in 0..sub_len as usize {
                        let sub_element = *sub_elements.add(j);
                        result = js_array_push_f64(result, sub_element);
                    }
                }
            } else {
                // Not an array — push as single element
                result = js_array_push_f64(result, mapped);
            }
        }

        result
    }
}

/// reduce - accumulate values using callback(accumulator, element)
/// initial_ptr is pointer to f64 initial value (null if not provided)
/// Returns the final accumulated value
#[no_mangle]
pub extern "C" fn js_array_reduce(
    arr: *const ArrayHeader,
    callback: *const ClosureHeader,
    has_initial: i32,
    initial: f64,
) -> f64 {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return if has_initial != 0 { initial } else { f64::NAN };
    }
    unsafe {
        let length = (*arr).length;
        let elements_ptr = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;

        if length == 0 {
            if has_initial != 0 {
                return initial;
            } else {
                // TypeError in JS, but we return NaN for simplicity
                return f64::NAN;
            }
        }

        let (mut accumulator, start_idx) = if has_initial != 0 {
            (initial, 0)
        } else {
            // Use first element as initial
            (*elements_ptr, 1)
        };

        for i in start_idx..length as usize {
            let element = *elements_ptr.add(i);
            // Refs #488 drizzle-sqlite: spec says callback is
            // `(accumulator, currentValue, currentIndex, array)`. Pre-fix
            // we only passed 2 args, so callbacks like drizzle's
            // `mapResultRow`'s `(result, {path, field}, columnIndex)` got
            // `columnIndex === undefined` and ended up reading `row[undefined]`
            // (which perry returns as `row[0]`) — every column projection
            // collapsed onto the first column's value (alice.age = 1
            // instead of 30). We now pass the index as the 3rd arg.
            // (The 4th `array` arg is intentionally omitted — drizzle and
            // most real callbacks ignore it; adding it would require a
            // call4 path and another NaN-box of the array handle.)
            accumulator = js_closure_call3(callback, accumulator, element, i as f64);
        }

        accumulator
    }
}

/// join - Join array elements into a string with a separator
/// Returns pointer to new StringHeader
#[no_mangle]
pub extern "C" fn js_array_join(
    arr: *const ArrayHeader,
    separator: *const crate::string::StringHeader,
) -> *mut crate::string::StringHeader {
    use crate::string::{js_string_from_bytes, StringHeader};
    use crate::value::JSValue;

    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return crate::string::js_string_from_bytes(b"".as_ptr(), 0);
    }
    unsafe {
        let length = (*arr).length;

        // Empty array returns empty string
        if length == 0 {
            return js_string_from_bytes(ptr::null(), 0);
        }

        let elements_ptr = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;

        // Get separator string
        let sep_str = if separator.is_null() {
            ","
        } else {
            let sep_len = (*separator).byte_len as usize;
            let sep_data = (separator as *const u8).add(std::mem::size_of::<StringHeader>());
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(sep_data, sep_len))
        };

        // Build result string
        let mut result = String::new();
        for i in 0..length as usize {
            if i > 0 {
                result.push_str(sep_str);
            }
            let element_bits = (*elements_ptr.add(i)).to_bits();
            let jsvalue = JSValue::from_bits(element_bits);

            // Issue #907: `Array(n)` initializes slots to TAG_HOLE
            // (see `js_array_alloc_with_length`). Per ES2015 §22.1.3.13
            // (Array.prototype.join), holes go through Get which returns
            // undefined → the spec's ToString step turns them into the
            // empty string. Without this check the catch-all below
            // emitted "[object Object]", so `Array(3).join("0")` returned
            // `"[object Object]0[object Object]0[object Object]"` instead
            // of `"00"`. dayjs's `m(t,e,n)` pad utility builds the UTC
            // offset string via `Array(e+1-r.length).join(n)` and the
            // result silently corrupted `b.z(this)` (the format `i`
            // capture), which downstream triggered
            // `TypeError: (number).replace is not a function` once the
            // catch-all fallthrough reached `i.replace(":","")`.
            if element_bits == crate::value::TAG_HOLE {
                // hole → empty string per spec
                continue;
            }

            // Convert element to string based on its type
            if jsvalue.is_string() {
                let str_ptr = jsvalue.as_pointer() as *const StringHeader;
                let str_len = (*str_ptr).byte_len as usize;
                let str_data = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                let s =
                    std::str::from_utf8_unchecked(std::slice::from_raw_parts(str_data, str_len));
                result.push_str(s);
            } else if jsvalue.is_short_string() {
                // v0.5.214 SSO — decode inline into a stack buffer
                // and push bytes. No heap roundtrip via
                // materialize_to_heap.
                let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
                let n = jsvalue.short_string_to_buf(&mut scratch);
                let s = std::str::from_utf8_unchecked(&scratch[..n]);
                result.push_str(s);
            } else if jsvalue.is_pointer() {
                // POINTER_TAG — may be a string stored with the wrong tag (cross-module)
                let ptr = (element_bits & 0x0000_FFFF_FFFF_FFFF) as *const StringHeader;
                if !ptr.is_null() && (ptr as usize) >= 0x1000 {
                    let str_len = (*ptr).byte_len as usize;
                    let str_data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                    let s = std::str::from_utf8_unchecked(std::slice::from_raw_parts(
                        str_data, str_len,
                    ));
                    result.push_str(s);
                } else {
                    result.push_str("[object Object]");
                }
            } else if jsvalue.is_number() {
                let n = jsvalue.as_number();
                if n.is_nan() {
                    result.push_str("NaN");
                } else if n.is_infinite() {
                    result.push_str(if n > 0.0 { "Infinity" } else { "-Infinity" });
                } else if n == 0.0 {
                    result.push('0');
                } else if n.fract() == 0.0 && n.abs() < 1e15 {
                    result.push_str(&format!("{}", n as i64));
                } else {
                    result.push_str(&format!("{}", n));
                }
            } else if jsvalue.is_null() {
                // null stringifies to empty string in join
            } else if jsvalue.is_undefined() {
                // undefined stringifies to empty string in join
            } else if jsvalue.is_bool() {
                result.push_str(if jsvalue.as_bool() { "true" } else { "false" });
            } else if element_bits > 0x1000
                && element_bits < 0x0001_0000_0000_0000
                && (element_bits & 0x3) == 0
            {
                // Raw pointer fallback — string stored without NaN-box tag
                let str_ptr = element_bits as *const StringHeader;
                let str_len = (*str_ptr).byte_len as usize;
                if str_len < 10_000_000 {
                    let str_data = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                    let s = std::str::from_utf8_unchecked(std::slice::from_raw_parts(
                        str_data, str_len,
                    ));
                    result.push_str(s);
                } else {
                    result.push_str("[object Object]");
                }
            } else {
                // For objects/arrays, just use placeholder
                result.push_str("[object Object]");
            }
        }

        // Create result string - extract ptr/len before passing to avoid
        // potential LLVM reordering of String drop vs copy_nonoverlapping
        let result_ptr = result.as_ptr();
        let result_len = result.len() as u32;
        let ret = js_string_from_bytes(result_ptr, result_len);
        // Ensure result String stays alive until after the copy completes
        std::hint::black_box(&result);
        drop(result);
        ret
    }
}

/// Check if a value is an array (Array.isArray)
/// Returns a NaN-boxed TAG_TRUE/TAG_FALSE JS boolean per JS semantics.
#[no_mangle]
pub extern "C" fn js_array_is_array(value: f64) -> f64 {
    use crate::gc::{GcHeader, GC_HEADER_SIZE, GC_TYPE_ARRAY};
    use crate::value::JSValue;

    const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
    const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
    let false_val = f64::from_bits(TAG_FALSE);
    let true_val = f64::from_bits(TAG_TRUE);

    let bits = value.to_bits();
    let jsvalue = JSValue::from_bits(bits);

    // Get the raw pointer, handling both NaN-boxed and raw bitcast pointers
    let raw_ptr: *const u8 = if jsvalue.is_pointer() {
        jsvalue.as_pointer::<u8>()
    } else {
        // Check for raw bitcast pointer (no NaN-box tag, stored as f64 bits)
        let raw = bits;
        let upper = raw >> 48;
        if upper == 0 && (raw & 0x0000_FFFF_FFFF_FFFF) > 0x10000 {
            raw as *const u8
        } else {
            return false_val;
        }
    };

    if raw_ptr.is_null() {
        return false_val;
    }

    // Check the GC header's obj_type. Both regular arrays and lazy
    // arrays (Phase 5 JSON.parse result) are arrays from the user's
    // perspective — `Array.isArray(JSON.parse("[...]"))` must return
    // true without forcing the lazy header to materialize.
    unsafe {
        let gc_header = raw_ptr.sub(GC_HEADER_SIZE) as *const GcHeader;
        let obj_type = (*gc_header).obj_type;
        if obj_type == GC_TYPE_ARRAY || obj_type == crate::gc::GC_TYPE_LAZY_ARRAY {
            true_val
        } else {
            false_val
        }
    }
}

/// `arr.reduceRight(callback, initial?)` — reduce from right to left
#[no_mangle]
pub extern "C" fn js_array_reduce_right(
    arr: *const ArrayHeader,
    callback: *const ClosureHeader,
    has_initial: i32,
    initial: f64,
) -> f64 {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return if has_initial != 0 { initial } else { f64::NAN };
    }
    unsafe {
        let length = (*arr).length as usize;
        let elements_ptr = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;

        if length == 0 {
            return if has_initial != 0 { initial } else { f64::NAN };
        }

        let (mut accumulator, start_idx) = if has_initial != 0 {
            (initial, length)
        } else {
            (*elements_ptr.add(length - 1), length - 1)
        };

        if start_idx > 0 {
            for i in (0..start_idx).rev() {
                let element = *elements_ptr.add(i);
                // Refs #488: pass index as 3rd arg to match spec
                // `(accumulator, currentValue, currentIndex, array)`.
                accumulator = js_closure_call3(callback, accumulator, element, i as f64);
            }
        }

        accumulator
    }
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
            *dst.add(i) = *src.add(len - 1 - i);
        }
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
        std::ptr::copy_nonoverlapping(src, dst, len);
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
        std::ptr::copy_nonoverlapping(src, dst, len);
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

        // Normalize start index
        let mut s = start as isize;
        if s < 0 {
            s += len;
        }
        if s < 0 {
            s = 0;
        }
        if s > len {
            s = len;
        }

        // Normalize delete count
        let mut dc = delete_count as isize;
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
        for i in 0..s as usize {
            *dst.add(i) = *src.add(i);
        }
        // Copy inserted items
        for i in 0..items_count as usize {
            *dst.add(s as usize + i) = *items.add(i);
        }
        // Copy elements after deleted range
        let after_start = (s + dc) as usize;
        for i in after_start..len as usize {
            *dst.add(s as usize + items_count as usize + i - after_start) = *src.add(i);
        }

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
        let mut idx = index as isize;
        if idx < 0 {
            idx += len;
        }
        if idx < 0 || idx >= len {
            // RangeError in JS — return a copy unchanged
            let new_arr = js_array_alloc(len as u32);
            (*new_arr).length = len as u32;
            let src = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
            let dst = (new_arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
            std::ptr::copy_nonoverlapping(src, dst, len as usize);
            return new_arr;
        }
        let src = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const f64;
        let new_arr = js_array_alloc(len as u32);
        (*new_arr).length = len as u32;
        let dst = (new_arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
        std::ptr::copy_nonoverlapping(src, dst, len as usize);
        *dst.add(idx as usize) = value;
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
        std::ptr::copy(
            elements.add(s as usize),
            elements.add(t as usize),
            count as usize,
        );
        arr
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_array_alloc_and_access() {
        let arr = js_array_alloc(5);

        // Initially empty
        assert_eq!(js_array_length(arr), 0);

        // Push some values
        js_array_push_f64(arr, 1.0);
        js_array_push_f64(arr, 2.0);
        js_array_push_f64(arr, 3.0);

        assert_eq!(js_array_length(arr), 3);
        assert_eq!(js_array_get_f64(arr, 0), 1.0);
        assert_eq!(js_array_get_f64(arr, 1), 2.0);
        assert_eq!(js_array_get_f64(arr, 2), 3.0);

        // Out of bounds returns TAG_UNDEFINED (JS spec: arr[OOB] === undefined)
        assert_eq!(js_array_get_f64(arr, 5).to_bits(), 0x7FFC_0000_0000_0001u64);
    }

    #[test]
    fn test_array_from_f64() {
        let values = [10.0, 20.0, 30.0, 40.0, 50.0];
        let arr = js_array_from_f64(values.as_ptr(), 5);

        assert_eq!(js_array_length(arr), 5);
        assert_eq!(js_array_get_f64(arr, 0), 10.0);
        assert_eq!(js_array_get_f64(arr, 2), 30.0);
        assert_eq!(js_array_get_f64(arr, 4), 50.0);
    }

    #[test]
    fn test_array_set() {
        let arr = js_array_alloc(3);
        js_array_push_f64(arr, 1.0);
        js_array_push_f64(arr, 2.0);
        js_array_push_f64(arr, 3.0);

        js_array_set_f64(arr, 1, 99.0);
        assert_eq!(js_array_get_f64(arr, 1), 99.0);
    }

    #[test]
    fn test_array_get_unchecked_basic() {
        let arr = js_array_alloc(4);
        js_array_push_f64(arr, 10.0);
        js_array_push_f64(arr, 20.0);
        js_array_push_f64(arr, 30.0);

        assert_eq!(js_array_get_f64_unchecked(arr, 0), 10.0);
        assert_eq!(js_array_get_f64_unchecked(arr, 1), 20.0);
        assert_eq!(js_array_get_f64_unchecked(arr, 2), 30.0);
    }

    #[test]
    fn test_array_get_unchecked_out_of_bounds() {
        let arr = js_array_alloc(4);
        js_array_push_f64(arr, 1.0);

        // Out of bounds should return TAG_UNDEFINED (JS spec)
        assert_eq!(
            js_array_get_f64_unchecked(arr, 1).to_bits(),
            0x7FFC_0000_0000_0001u64
        );
        assert_eq!(
            js_array_get_f64_unchecked(arr, 100).to_bits(),
            0x7FFC_0000_0000_0001u64
        );
    }

    #[test]
    fn test_array_get_f64_vs_unchecked_parity() {
        let arr = js_array_alloc(8);
        let values = [1.0, 2.5, -3.0, 0.0, 100.0, f64::INFINITY, f64::NEG_INFINITY];
        for &v in &values {
            js_array_push_f64(arr, v);
        }

        // Both functions should return identical results for plain arrays
        for i in 0..values.len() as u32 {
            let checked = js_array_get_f64(arr, i);
            let unchecked = js_array_get_f64_unchecked(arr, i);
            assert_eq!(
                checked.to_bits(),
                unchecked.to_bits(),
                "parity mismatch at index {}: checked={}, unchecked={}",
                i,
                checked,
                unchecked
            );
        }

        // Out of bounds parity — both return TAG_UNDEFINED
        let oob_checked = js_array_get_f64(arr, 100);
        let oob_unchecked = js_array_get_f64_unchecked(arr, 100);
        assert_eq!(oob_checked.to_bits(), 0x7FFC_0000_0000_0001u64);
        assert_eq!(oob_unchecked.to_bits(), 0x7FFC_0000_0000_0001u64);
    }

    #[test]
    fn test_array_grow_capacity() {
        let mut arr = js_array_alloc(2);

        // Push well beyond initial capacity (push returns new ptr on grow)
        for i in 0..50 {
            arr = js_array_push_f64(arr, i as f64);
        }

        assert_eq!(js_array_length(arr), 50);

        // Verify all values preserved after growth
        for i in 0..50 {
            assert_eq!(
                js_array_get_f64(arr, i),
                i as f64,
                "value at index {} should be {}",
                i,
                i
            );
        }
    }

    #[test]
    fn test_array_set_unchecked_basic() {
        let arr = js_array_alloc(4);
        js_array_push_f64(arr, 1.0);
        js_array_push_f64(arr, 2.0);
        js_array_push_f64(arr, 3.0);

        js_array_set_f64_unchecked(arr, 1, 99.0);
        assert_eq!(js_array_get_f64_unchecked(arr, 1), 99.0);
        // Other elements unchanged
        assert_eq!(js_array_get_f64_unchecked(arr, 0), 1.0);
        assert_eq!(js_array_get_f64_unchecked(arr, 2), 3.0);
    }

    #[test]
    fn test_array_pop_and_push() {
        let arr = js_array_alloc(4);
        let arr = js_array_push_f64(arr, 1.0);
        let arr = js_array_push_f64(arr, 2.0);
        let arr = js_array_push_f64(arr, 3.0);

        let popped = js_array_pop_f64(arr);
        assert_eq!(popped, 3.0);
        assert_eq!(js_array_length(arr), 2);

        let arr = js_array_push_f64(arr, 4.0);
        assert_eq!(js_array_length(arr), 3);
        assert_eq!(js_array_get_f64(arr, 2), 4.0);
    }

    #[test]
    fn test_array_indexOf() {
        let arr = js_array_alloc(4);
        js_array_push_f64(arr, 10.0);
        js_array_push_f64(arr, 20.0);
        js_array_push_f64(arr, 30.0);

        assert_eq!(js_array_indexOf_f64(arr, 10.0), 0);
        assert_eq!(js_array_indexOf_f64(arr, 20.0), 1);
        assert_eq!(js_array_indexOf_f64(arr, 30.0), 2);
        assert_eq!(js_array_indexOf_f64(arr, 99.0), -1);
    }

    #[test]
    fn test_array_includes() {
        let arr = js_array_alloc(4);
        js_array_push_f64(arr, 1.0);
        js_array_push_f64(arr, 2.0);

        assert_eq!(js_array_includes_f64(arr, 1.0), 1);
        assert_eq!(js_array_includes_f64(arr, 2.0), 1);
        assert_eq!(js_array_includes_f64(arr, 3.0), 0);
    }

    #[test]
    fn test_array_from_f64_and_length() {
        let values = [5.0, 10.0, 15.0];
        let arr = js_array_from_f64(values.as_ptr(), 3);

        assert_eq!(js_array_length(arr), 3);
        for i in 0..3 {
            assert_eq!(js_array_get_f64(arr, i), values[i as usize]);
        }
    }

    #[test]
    fn test_array_null_safety() {
        // Null array pointer should not crash
        assert!(js_array_get_f64(std::ptr::null(), 0).is_nan());
        assert!(js_array_get_f64_unchecked(std::ptr::null(), 0).is_nan());
        assert_eq!(js_array_length(std::ptr::null()), 0);
    }

    #[test]
    fn test_array_splice_delete_middle() {
        // [1,2,3,4,5].splice(1, 2) -> deleted=[2,3], arr=[1,4,5]
        let arr = js_array_alloc(8);
        let arr = js_array_push_f64(arr, 1.0);
        let arr = js_array_push_f64(arr, 2.0);
        let arr = js_array_push_f64(arr, 3.0);
        let arr = js_array_push_f64(arr, 4.0);
        let arr = js_array_push_f64(arr, 5.0);
        let mut out_arr: *mut ArrayHeader = std::ptr::null_mut();
        let deleted = js_array_splice(arr, 1, 2, std::ptr::null(), 0, &mut out_arr);

        assert_eq!(js_array_length(out_arr), 3);
        assert_eq!(js_array_get_f64(out_arr, 0), 1.0);
        assert_eq!(js_array_get_f64(out_arr, 1), 4.0);
        assert_eq!(js_array_get_f64(out_arr, 2), 5.0);

        assert_eq!(js_array_length(deleted), 2);
        assert_eq!(js_array_get_f64(deleted, 0), 2.0);
        assert_eq!(js_array_get_f64(deleted, 1), 3.0);
    }

    #[test]
    fn test_array_splice_insert() {
        // [1,2,5].splice(2, 0, 3, 4) -> deleted=[], arr=[1,2,3,4,5]
        let arr = js_array_alloc(8);
        let arr = js_array_push_f64(arr, 1.0);
        let arr = js_array_push_f64(arr, 2.0);
        let arr = js_array_push_f64(arr, 5.0);
        let items = [3.0_f64, 4.0];
        let mut out_arr: *mut ArrayHeader = std::ptr::null_mut();
        let deleted = js_array_splice(arr, 2, 0, items.as_ptr(), 2, &mut out_arr);

        assert_eq!(js_array_length(deleted), 0);
        assert_eq!(js_array_length(out_arr), 5);
        assert_eq!(js_array_get_f64(out_arr, 0), 1.0);
        assert_eq!(js_array_get_f64(out_arr, 1), 2.0);
        assert_eq!(js_array_get_f64(out_arr, 2), 3.0);
        assert_eq!(js_array_get_f64(out_arr, 3), 4.0);
        assert_eq!(js_array_get_f64(out_arr, 4), 5.0);
    }

    #[test]
    fn test_array_splice_replace() {
        // [1,2,3].splice(1, 1, 99) -> deleted=[2], arr=[1,99,3]
        let arr = js_array_alloc(4);
        let arr = js_array_push_f64(arr, 1.0);
        let arr = js_array_push_f64(arr, 2.0);
        let arr = js_array_push_f64(arr, 3.0);
        let items = [99.0_f64];
        let mut out_arr: *mut ArrayHeader = std::ptr::null_mut();
        let deleted = js_array_splice(arr, 1, 1, items.as_ptr(), 1, &mut out_arr);

        assert_eq!(js_array_length(deleted), 1);
        assert_eq!(js_array_get_f64(deleted, 0), 2.0);
        assert_eq!(js_array_length(out_arr), 3);
        assert_eq!(js_array_get_f64(out_arr, 0), 1.0);
        assert_eq!(js_array_get_f64(out_arr, 1), 99.0);
        assert_eq!(js_array_get_f64(out_arr, 2), 3.0);
    }

    #[test]
    fn test_array_splice_delete_to_end() {
        // [1,2,3,4].splice(2) -> deleted=[3,4], arr=[1,2]
        let arr = js_array_alloc(8);
        let arr = js_array_push_f64(arr, 1.0);
        let arr = js_array_push_f64(arr, 2.0);
        let arr = js_array_push_f64(arr, 3.0);
        let arr = js_array_push_f64(arr, 4.0);
        let mut out_arr: *mut ArrayHeader = std::ptr::null_mut();
        let deleted = js_array_splice(arr, 2, i32::MAX, std::ptr::null(), 0, &mut out_arr);

        assert_eq!(js_array_length(out_arr), 2);
        assert_eq!(js_array_get_f64(out_arr, 0), 1.0);
        assert_eq!(js_array_get_f64(out_arr, 1), 2.0);
        assert_eq!(js_array_length(deleted), 2);
        assert_eq!(js_array_get_f64(deleted, 0), 3.0);
        assert_eq!(js_array_get_f64(deleted, 1), 4.0);
    }

    #[test]
    fn test_array_splice_negative_start() {
        // [1,2,3,4].splice(-2, 1) -> deleted=[3], arr=[1,2,4]
        let arr = js_array_alloc(8);
        let arr = js_array_push_f64(arr, 1.0);
        let arr = js_array_push_f64(arr, 2.0);
        let arr = js_array_push_f64(arr, 3.0);
        let arr = js_array_push_f64(arr, 4.0);
        let mut out_arr: *mut ArrayHeader = std::ptr::null_mut();
        let deleted = js_array_splice(arr, -2, 1, std::ptr::null(), 0, &mut out_arr);

        assert_eq!(js_array_length(deleted), 1);
        assert_eq!(js_array_get_f64(deleted, 0), 3.0);
        assert_eq!(js_array_length(out_arr), 3);
        assert_eq!(js_array_get_f64(out_arr, 0), 1.0);
        assert_eq!(js_array_get_f64(out_arr, 1), 2.0);
        assert_eq!(js_array_get_f64(out_arr, 2), 4.0);
    }

    #[test]
    fn test_array_splice_grow_realloc() {
        // Start with capacity 4, splice in 10 items to force reallocation
        let arr = js_array_alloc(4);
        let arr = js_array_push_f64(arr, 1.0);
        let arr = js_array_push_f64(arr, 2.0);
        let items = [
            10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 70.0, 80.0, 90.0, 100.0_f64,
        ];
        let mut out_arr: *mut ArrayHeader = std::ptr::null_mut();
        let deleted = js_array_splice(arr, 1, 0, items.as_ptr(), 10, &mut out_arr);

        assert_eq!(js_array_length(deleted), 0);
        assert_eq!(js_array_length(out_arr), 12);
        assert_eq!(js_array_get_f64(out_arr, 0), 1.0);
        for i in 0..10 {
            assert_eq!(
                js_array_get_f64(out_arr, (i + 1) as u32),
                items[i],
                "mismatch at index {}",
                i + 1
            );
        }
        assert_eq!(js_array_get_f64(out_arr, 11), 2.0);
    }
}

/// Convert any iterator-protocol object (has `.next()` method) to an array.
/// Used by spread on generators, Array.from on generators, etc.
/// Calls `.next()` in a loop until `.done` is true, collecting `.value` entries.
#[no_mangle]
pub extern "C" fn js_iterator_to_array(iter_f64: f64) -> *mut ArrayHeader {
    use crate::closure;
    use crate::object::{js_object_get_field_by_name, ObjectHeader};
    use crate::string::js_string_from_bytes;
    use crate::value::{js_nanbox_get_pointer, TAG_UNDEFINED};

    let arr = js_array_alloc(8); // start with capacity 8

    // Get the iterator object pointer
    let _iter_bits = iter_f64.to_bits();
    let iter_ptr = js_nanbox_get_pointer(iter_f64);
    if iter_ptr == 0 {
        return arr;
    }
    let iter_obj = iter_ptr as *const ObjectHeader;

    // Look up the "next" method on the iterator object
    let next_key = js_string_from_bytes(b"next".as_ptr(), 4);
    let next_val = js_object_get_field_by_name(iter_obj, next_key);
    if next_val.is_undefined() {
        return arr;
    }

    // next_val should be a closure pointer
    let next_f64 = unsafe { f64::from_bits(std::mem::transmute::<_, u64>(next_val)) };
    let next_ptr = js_nanbox_get_pointer(next_f64) as *const closure::ClosureHeader;
    if next_ptr.is_null() {
        return arr;
    }

    // Iterate: call next() until done
    let done_key = js_string_from_bytes(b"done".as_ptr(), 4);
    let value_key = js_string_from_bytes(b"value".as_ptr(), 5);
    let mut result = arr;

    for _ in 0..100_000 {
        // safety limit
        // Call next()
        let result_f64 = closure::js_closure_call1(next_ptr, f64::from_bits(TAG_UNDEFINED));
        let result_ptr = js_nanbox_get_pointer(result_f64);
        if result_ptr == 0 {
            break;
        }
        let result_obj = result_ptr as *const ObjectHeader;

        // Check .done
        let done_val = js_object_get_field_by_name(result_obj, done_key);
        let done_bits = unsafe { std::mem::transmute::<_, u64>(done_val) };
        // done is true when it's TAG_TRUE (0x7FFC_0000_0000_0004) or truthy number
        if done_bits == 0x7FFC_0000_0000_0004 {
            break;
        } // TAG_TRUE

        // Get .value and push to array
        let val = js_object_get_field_by_name(result_obj, value_key);
        let val_f64 = unsafe { f64::from_bits(std::mem::transmute::<_, u64>(val)) };
        result = js_array_push_f64(result, val_f64);
    }

    result
}
