//! Object allocation: `js_object_alloc*`, class-keys array builders,
//! shape-cache-backed fast paths, and the clone/copy/assign helpers.
//!
//! Split out of `object.rs` (issue #1103). Pure relocation — no logic
//! changes. Shared state and helpers remain in the parent `object`
//! module and are reached via `use super::*;`.

use super::*;

/// Allocate a new object with the given class ID and field count
/// Returns a pointer to the object header
#[no_mangle]
pub extern "C" fn js_object_alloc(class_id: u32, field_count: u32) -> *mut ObjectHeader {
    js_object_alloc_with_parent(class_id, 0, field_count)
}

/// #1175: allocate an object whose `[[Prototype]]` is null. Same layout as
/// `js_object_alloc`, but the `OBJ_FLAG_NULL_PROTO` bit is set on the GC
/// header so `Object.getPrototypeOf` returns null instead of the heap
/// pointer / synthesized proto. Used by `querystring.parse` to mirror Node's
/// `Object.create(null)`-backed result and dodge prototype-pollution
/// surprises.
#[no_mangle]
pub extern "C" fn js_object_alloc_null_proto(class_id: u32, field_count: u32) -> *mut ObjectHeader {
    let ptr = js_object_alloc_with_parent(class_id, 0, field_count);
    unsafe {
        let gc = (ptr as *mut u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
        (*gc)._reserved |= crate::gc::OBJ_FLAG_NULL_PROTO;
    }
    ptr
}

/// Allocate a new object with class ID, parent class ID, and field count
/// The parent_class_id is used for instanceof inheritance checks
/// Returns a pointer to the object header
#[no_mangle]
pub extern "C" fn js_object_alloc_with_parent(
    class_id: u32,
    parent_class_id: u32,
    field_count: u32,
) -> *mut ObjectHeader {
    // Register this class's parent for inheritance lookups
    if parent_class_id != 0 {
        register_class(class_id, parent_class_id);
    }

    let header_size = std::mem::size_of::<ObjectHeader>();
    // Allocate at least 8 field slots to match js_object_set_field_by_name's alloc_limit
    // assumption (max(field_count, 8)). Without this, empty objects ({}) with field_count=0
    // would have 0 field slots but js_object_set_field_by_name writes up to 8 fields inline,
    // causing heap buffer overflow into adjacent arena objects.
    let alloc_field_count = std::cmp::max(field_count as usize, 8);
    let fields_size = alloc_field_count * std::mem::size_of::<JSValue>();
    let total_size = header_size + fields_size;

    let ptr = arena_alloc_gc(total_size, 8, crate::gc::GC_TYPE_OBJECT) as *mut ObjectHeader;

    unsafe {
        // Initialize header
        (*ptr).object_type = crate::error::OBJECT_TYPE_REGULAR;
        (*ptr).class_id = class_id;
        (*ptr).parent_class_id = parent_class_id;
        (*ptr).field_count = field_count;
        // GC_STORE_AUDIT(INIT): freshly allocated object starts with no keys-array edge.
        (*ptr).keys_array = ptr::null_mut();

        // Initialize ALL allocated field slots to undefined (not just field_count)
        // We allocate max(field_count, 8) slots but must zero all of them to prevent
        // stale data from previously freed GC objects from bleeding through.
        let fields_ptr = (ptr as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *mut JSValue;
        for i in 0..alloc_field_count {
            // GC_STORE_AUDIT(INIT): freshly allocated object field slot is initialized pointer-free.
            ptr::write(fields_ptr.add(i), JSValue::undefined());
        }
        crate::gc::layout_init_pointer_free(ptr as *mut u8);

        ptr
    }
}

/// Fast object allocation using bump allocator - NO field initialization
/// This is significantly faster for hot paths where constructor immediately sets all fields
/// Returns a pointer to the object header with UNINITIALIZED fields
#[no_mangle]
pub extern "C" fn js_object_alloc_fast(class_id: u32, field_count: u32) -> *mut ObjectHeader {
    let header_size = std::mem::size_of::<ObjectHeader>();
    let alloc_field_count = std::cmp::max(field_count as usize, 8);
    let fields_size = alloc_field_count * std::mem::size_of::<JSValue>();
    let total_size = header_size + fields_size;

    let ptr = arena_alloc_gc(total_size, 8, crate::gc::GC_TYPE_OBJECT) as *mut ObjectHeader;

    unsafe {
        // Initialize header only - fields left uninitialized for constructor to fill
        (*ptr).object_type = crate::error::OBJECT_TYPE_REGULAR;
        (*ptr).class_id = class_id;
        (*ptr).parent_class_id = 0;
        (*ptr).field_count = field_count;
        // GC_STORE_AUDIT(INIT): freshly allocated object starts with no keys-array edge.
        (*ptr).keys_array = ptr::null_mut();
        crate::gc::layout_init_pointer_free(ptr as *mut u8);
    }

    ptr
}

/// Fast object allocation with parent class ID - NO field initialization
#[no_mangle]
pub extern "C" fn js_object_alloc_fast_with_parent(
    class_id: u32,
    parent_class_id: u32,
    field_count: u32,
) -> *mut ObjectHeader {
    // Only register class if it has a parent (one-time operation per class)
    if parent_class_id != 0 {
        register_class(class_id, parent_class_id);
    }

    let header_size = std::mem::size_of::<ObjectHeader>();
    let alloc_field_count = std::cmp::max(field_count as usize, 8);
    let fields_size = alloc_field_count * std::mem::size_of::<JSValue>();
    let total_size = header_size + fields_size;

    let ptr = arena_alloc_gc(total_size, 8, crate::gc::GC_TYPE_OBJECT) as *mut ObjectHeader;

    unsafe {
        (*ptr).object_type = crate::error::OBJECT_TYPE_REGULAR;
        (*ptr).class_id = class_id;
        (*ptr).parent_class_id = parent_class_id;
        (*ptr).field_count = field_count;
        // GC_STORE_AUDIT(INIT): freshly allocated object starts with no keys-array edge.
        (*ptr).keys_array = ptr::null_mut();
        crate::gc::layout_init_pointer_free(ptr as *mut u8);
    }

    ptr
}

/// Fast class instance allocator that takes a pre-built keys_array
/// pointer directly, skipping the per-call SHAPE_CACHE lookup. The
/// codegen pre-builds the keys_array ONCE at module init time
/// (via `js_build_class_keys_array`) and stores the result in a
/// per-class global, then passes that global to this allocator on
/// every `new ClassName()` call. This eliminates the thread-local
/// + RefCell::borrow_mut + HashMap::get cost from the hot
/// allocation path — for benchmarks like `object_create` (1M
/// `new Point(...)` calls) the SHAPE_CACHE lookup was ~30ns/alloc.
///
/// `#[inline]` lets the bitcode-link path
/// (`PERRY_LLVM_BITCODE_LINK=1`) inline the entire body — including
/// the `arena_alloc_gc` call — into the user's `new ClassName()`
/// site, eliminating function-call overhead from the hot loop.
#[no_mangle]
pub extern "C" fn js_object_alloc_class_inline_keys(
    class_id: u32,
    parent_class_id: u32,
    field_count: u32,
    keys_array: *mut ArrayHeader,
) -> *mut ObjectHeader {
    if parent_class_id != 0 {
        register_class(class_id, parent_class_id);
    }
    let header_size = std::mem::size_of::<ObjectHeader>();
    let alloc_field_count = std::cmp::max(field_count as usize, 8);
    let fields_size = alloc_field_count * std::mem::size_of::<JSValue>();
    let total_size = header_size + fields_size;

    let ptr = arena_alloc_gc(total_size, 8, crate::gc::GC_TYPE_OBJECT) as *mut ObjectHeader;

    unsafe {
        (*ptr).object_type = crate::error::OBJECT_TYPE_REGULAR;
        (*ptr).class_id = class_id;
        (*ptr).parent_class_id = parent_class_id;
        (*ptr).field_count = field_count;
        set_object_keys_array(ptr, keys_array);
        crate::gc::layout_init_pointer_free(ptr as *mut u8);
    }
    ptr
}

/// Build (or fetch from SHAPE_CACHE) the keys_array for a class.
/// Called ONCE per class at module init time; the resulting pointer
/// is cached in a per-class global by the codegen and then passed
/// to `js_object_alloc_class_inline_keys` on each `new` call.
///
/// Same packed-keys format as `js_object_alloc_class_with_keys`:
/// null-separated UTF-8 field names.
#[no_mangle]
pub extern "C" fn js_build_class_keys_array(
    class_id: u32,
    field_count: u32,
    packed_keys: *const u8,
    packed_keys_len: u32,
) -> *mut ArrayHeader {
    let shape_id = class_id
        .wrapping_mul(10007)
        .wrapping_add(field_count.wrapping_mul(100003))
        .wrapping_add(1000000);
    let cached = shape_cache_get(shape_id);
    if !cached.is_null() {
        return cached;
    }
    let keys_bytes = unsafe { std::slice::from_raw_parts(packed_keys, packed_keys_len as usize) };
    let keys: Vec<&[u8]> = keys_bytes
        .split(|&b| b == 0)
        .filter(|s| !s.is_empty())
        .collect();
    let num_keys = keys.len();
    // Issue #179: the keys_array and its string elements are shape-cache
    // resident for the program's lifetime (anchored by
    // `scan_shape_cache_roots`). Route them through the longlived arena
    // so general-arena block 0 doesn't get pinned by the first `new C()`
    // in a loop, which cascaded via block-persistence into every
    // subsequent iteration's allocations.
    let arr = crate::array::js_array_alloc_with_length_longlived(num_keys as u32);
    let elements_ptr = unsafe { (arr as *mut u8).add(8) as *mut f64 };
    for (i, key_bytes) in keys.iter().enumerate() {
        let str_ptr = crate::string::js_string_from_bytes_longlived(
            key_bytes.as_ptr(),
            key_bytes.len() as u32,
        );
        let nanboxed = f64::from_bits(
            crate::value::STRING_TAG | (str_ptr as u64 & crate::value::POINTER_MASK),
        );
        unsafe {
            // GC_STORE_AUDIT(BARRIERED): cached method-name array records layout immediately after.
            *elements_ptr.add(i) = nanboxed;
            crate::array::note_array_slot_layout_only(arr, i, nanboxed.to_bits());
        }
    }
    shape_cache_insert(shape_id, arr);
    arr
}

/// Allocate a class instance with a shape-cached keys array for field names.
/// This allows dynamic property access (obj.field1) to work on class instances,
/// not just object literals. Uses class_id as the shape_id for caching.
///
/// Marked `#[inline]` so the LLVM bitcode-link path
/// (`PERRY_LLVM_BITCODE_LINK=1`) can inline the body into hot
/// allocation loops, eliminating the function-call overhead and
/// letting LLVM constant-fold the SHAPE_INLINE_CACHE slot index when
/// `class_id` is a compile-time constant (which it always is at the
/// `new ClassName()` call site).
#[no_mangle]
pub extern "C" fn js_object_alloc_class_with_keys(
    class_id: u32,
    parent_class_id: u32,
    field_count: u32,
    packed_keys: *const u8,
    packed_keys_len: u32,
) -> *mut ObjectHeader {
    // Register parent class if needed
    if parent_class_id != 0 {
        register_class(class_id, parent_class_id);
    }

    let header_size = std::mem::size_of::<ObjectHeader>();
    let alloc_field_count = std::cmp::max(field_count as usize, 8);
    let fields_size = alloc_field_count * std::mem::size_of::<JSValue>();
    let total_size = header_size + fields_size;

    let ptr = arena_alloc_gc(total_size, 8, crate::gc::GC_TYPE_OBJECT) as *mut ObjectHeader;

    unsafe {
        (*ptr).object_type = crate::error::OBJECT_TYPE_REGULAR;
        (*ptr).class_id = class_id;
        (*ptr).parent_class_id = parent_class_id;
        (*ptr).field_count = field_count;
        crate::gc::layout_init_pointer_free(ptr as *mut u8);
    }

    // Use class_id as shape_id for caching the keys array.
    // Hot path: direct-mapped inline cache lookup (no RefCell, no
    // HashMap). Miss path: lazy-build from packed_keys.
    let shape_id = class_id
        .wrapping_mul(10007)
        .wrapping_add(field_count.wrapping_mul(100003))
        .wrapping_add(1000000);
    let cached = shape_cache_get(shape_id);
    let keys_arr = if !cached.is_null() {
        cached
    } else {
        let keys_bytes =
            unsafe { std::slice::from_raw_parts(packed_keys, packed_keys_len as usize) };
        let keys: Vec<&[u8]> = keys_bytes
            .split(|&b| b == 0)
            .filter(|s| !s.is_empty())
            .collect();
        let num_keys = keys.len();
        // Issue #179: shape-cache keys_array lives in the longlived arena
        // (see `js_build_class_keys_array` for the rationale).
        let arr = crate::array::js_array_alloc_with_length_longlived(num_keys as u32);
        let elements_ptr = unsafe { (arr as *mut u8).add(8) as *mut f64 };
        for (i, key_bytes) in keys.iter().enumerate() {
            let str_ptr = crate::string::js_string_from_bytes_longlived(
                key_bytes.as_ptr(),
                key_bytes.len() as u32,
            );
            let nanboxed = f64::from_bits(
                crate::value::STRING_TAG | (str_ptr as u64 & crate::value::POINTER_MASK),
            );
            unsafe {
                // GC_STORE_AUDIT(BARRIERED): cached keys array slot is reflected into layout metadata.
                *elements_ptr.add(i) = nanboxed;
                crate::array::note_array_slot_layout_only(arr, i, nanboxed.to_bits());
            }
        }
        shape_cache_insert(shape_id, arr);
        arr
    };

    unsafe {
        set_object_keys_array(ptr, keys_arr);
    }
    ptr
}

/// Allocate an object with a shape-cached keys array.
/// First call per shape_id creates the keys array from packed_keys (null-separated key names);
/// subsequent calls reuse the cached pointer. This eliminates per-object key string allocation
/// and array construction for repeated object literals with the same shape.
#[no_mangle]
pub extern "C" fn js_object_alloc_with_shape(
    shape_id: u32,
    field_count: u32,
    packed_keys: *const u8,
    packed_keys_len: u32,
) -> *mut ObjectHeader {
    let header_size = std::mem::size_of::<ObjectHeader>();
    // Allocate extra field slots for dynamic property growth (plain objects may get new fields)
    let alloc_field_count = std::cmp::max(field_count as usize, 8);
    let fields_size = alloc_field_count * 8;
    let total_size = header_size + fields_size;
    let obj_ptr = arena_alloc_gc(total_size, 8, crate::gc::GC_TYPE_OBJECT) as *mut ObjectHeader;

    unsafe {
        (*obj_ptr).object_type = crate::error::OBJECT_TYPE_REGULAR;
        (*obj_ptr).class_id = 0;
        (*obj_ptr).parent_class_id = 0;
        // field_count tracks the logical number of fields; extra allocated slots
        // are available for dynamic property growth via js_object_set_field_by_name
        (*obj_ptr).field_count = field_count;

        // Initialize all allocated field slots to undefined (including extra padding)
        let fields_ptr = (obj_ptr as *mut u8).add(header_size) as *mut JSValue;
        for i in 0..alloc_field_count {
            // GC_STORE_AUDIT(INIT): freshly allocated object field slot is initialized pointer-free.
            ptr::write(fields_ptr.add(i), JSValue::undefined());
        }
        crate::gc::layout_init_pointer_free(obj_ptr as *mut u8);
    }

    let cached = shape_cache_get(shape_id);
    let keys_arr = if !cached.is_null() {
        cached
    } else {
        let keys_bytes =
            unsafe { std::slice::from_raw_parts(packed_keys, packed_keys_len as usize) };
        let keys: Vec<&[u8]> = keys_bytes
            .split(|&b| b == 0)
            .filter(|s| !s.is_empty())
            .collect();
        let num_keys = keys.len();
        // Issue #179: shape-cache keys_array lives in the longlived arena.
        let arr = crate::array::js_array_alloc_with_length_longlived(num_keys as u32);
        let elements_ptr = unsafe { (arr as *mut u8).add(8) as *mut f64 };
        for (i, key_bytes) in keys.iter().enumerate() {
            let str_ptr = crate::string::js_string_from_bytes_longlived(
                key_bytes.as_ptr(),
                key_bytes.len() as u32,
            );
            let nanboxed = f64::from_bits(
                crate::value::STRING_TAG | (str_ptr as u64 & crate::value::POINTER_MASK),
            );
            unsafe {
                // GC_STORE_AUDIT(BARRIERED): cached keys array slot is reflected into layout metadata.
                *elements_ptr.add(i) = nanboxed;
                crate::array::note_array_slot_layout_only(arr, i, nanboxed.to_bits());
            }
        }
        shape_cache_insert(shape_id, arr);
        arr
    };

    unsafe {
        set_object_keys_array(obj_ptr, keys_arr);
    }

    obj_ptr
}

/// Clone a spread source object and reserve extra physical slot capacity for additional
/// static properties. Used to implement object spread: `{ ...src, key1: val1, key2: val2 }`.
///
/// - `src_f64`: the spread source object as a NaN-boxed f64 (POINTER_TAG or raw pointer)
/// - `extra_count`: number of additional static properties — reserves physical slot capacity
///   for them, but does NOT add their keys to the keys_array upfront. Codegen is expected to
///   call `js_object_set_field_by_name` for each static prop, which correctly overwrites keys
///   that already exist in the spread source (preserving JS "last key wins" semantics) and
///   appends new keys (using the reserved capacity).
/// - `_static_keys_ptr`/`_static_keys_len`: unused (kept for ABI compat). Previously these
///   were used to pre-populate static keys in keys_array, but that created duplicate entries
///   when a static key matched an existing spread key, and the linear-scan lookup returned
///   the first (stale) match instead of the intended last-key value.
///
/// Returns the new *mut ObjectHeader as an i64 raw pointer (NOT NaN-boxed).
/// The returned object's `field_count` equals the source's field_count (NOT src + extra),
/// but the physical allocation reserves enough slots so subsequent
/// `js_object_set_field_by_name` calls have somewhere to append.
#[no_mangle]
pub unsafe extern "C" fn js_object_clone_with_extra(
    src_f64: f64,
    extra_count: u32,
    _static_keys_ptr: *const u8,
    _static_keys_len: u32,
) -> *mut ObjectHeader {
    // Extract raw pointer from NaN-boxed f64
    let src_bits = src_f64.to_bits();
    let top16 = src_bits >> 48;
    let src_raw = if top16 >= 0x7FF8 {
        (src_bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else {
        src_bits as usize
    };

    let header_size = std::mem::size_of::<ObjectHeader>();

    // If source is invalid, create an empty object with enough capacity for the static props.
    // Physical slot count = max(extra_count, 8) to match js_object_set_field_by_name's
    // alloc_limit = max(field_count, 8) expectation.
    if src_raw < 0x10000 {
        let phys_slots = std::cmp::max(extra_count, 8);
        let total_size = header_size + phys_slots as usize * 8;
        let new_ptr = arena_alloc_gc(total_size, 8, crate::gc::GC_TYPE_OBJECT) as *mut ObjectHeader;
        (*new_ptr).object_type = crate::error::OBJECT_TYPE_REGULAR;
        (*new_ptr).class_id = 0;
        (*new_ptr).parent_class_id = 0;
        (*new_ptr).field_count = 0;
        let fields_ptr = (new_ptr as *mut u8).add(header_size) as *mut u64;
        for i in 0..phys_slots as usize {
            // GC_STORE_AUDIT(INIT): freshly allocated clone field slot is initialized pointer-free.
            ptr::write(fields_ptr.add(i), crate::value::TAG_UNDEFINED);
        }
        crate::gc::layout_init_pointer_free(new_ptr as *mut u8);
        // Empty keys array with capacity reserved for the static props to come.
        let new_keys_arr = crate::array::js_array_alloc(extra_count);
        set_object_keys_array(new_ptr, new_keys_arr);
        return new_ptr;
    }

    let src_ptr = src_raw as *const ObjectHeader;
    let src_field_count = (*src_ptr).field_count;

    // Physical slot capacity: src_field_count + extra_count, but at least max(fc, 8) to match
    // js_object_set_field's alloc_limit check. Extra slots are scratch space for subsequent
    // js_object_set_field_by_name calls.
    let phys_slots = std::cmp::max(src_field_count + extra_count, 8);
    let total_size = header_size + phys_slots as usize * 8;
    let new_ptr = arena_alloc_gc(total_size, 8, crate::gc::GC_TYPE_OBJECT) as *mut ObjectHeader;
    (*new_ptr).object_type = crate::error::OBJECT_TYPE_REGULAR;
    (*new_ptr).class_id = 0;
    (*new_ptr).parent_class_id = 0;
    // Logical field count starts at src's count. js_object_set_field_by_name bumps it when
    // appending new keys.
    (*new_ptr).field_count = src_field_count;

    // Copy source fields (as raw f64/u64 words — preserves NaN-boxing)
    let src_fields = (src_ptr as *const u8).add(header_size) as *const u64;
    let dst_fields = (new_ptr as *mut u8).add(header_size) as *mut u64;
    for i in 0..src_field_count as usize {
        let field_val = *src_fields.add(i);
        // Guard: null POINTER_TAG (0x7FFD_0000_0000_0000) is never legitimate — replace with undefined
        let cleaned = if field_val == 0x7FFD_0000_0000_0000 {
            eprintln!(
                "[CLONE_NULL_PTR] field {} from src={:p} — replacing with undefined",
                i, src_ptr
            );
            crate::value::TAG_UNDEFINED
        } else {
            field_val
        };
        // GC_STORE_AUDIT(INIT): cloned object is unpublished; layout is rebuilt after field copy.
        ptr::write(dst_fields.add(i), cleaned);
    }
    // Initialize scratch slots to undefined
    for i in src_field_count as usize..phys_slots as usize {
        // GC_STORE_AUDIT(INIT): cloned object scratch field slot is initialized pointer-free.
        ptr::write(dst_fields.add(i), crate::value::TAG_UNDEFINED);
    }
    rebuild_object_field_layout(new_ptr, src_field_count as usize);

    // Build keys array: copy ONLY src keys. Static keys are NOT added here — codegen uses
    // js_object_set_field_by_name for each static prop, which appends new keys via
    // js_array_push. Pre-size the keys capacity to avoid immediate reallocation on append.
    let src_keys_arr = (*src_ptr).keys_array;
    let new_keys_arr = crate::array::js_array_alloc(src_field_count + extra_count);
    let new_keys_elements = (new_keys_arr as *mut u8).add(8) as *mut f64;

    if !src_keys_arr.is_null() && (src_keys_arr as usize) >= 0x10000 {
        let src_key_len = (*src_keys_arr).length as usize;
        let src_key_elements = (src_keys_arr as *const u8).add(8) as *const f64;
        let copy_count = src_key_len.min(src_field_count as usize);
        for i in 0..copy_count {
            // GC_STORE_AUDIT(INIT): cloned keys array is unpublished; layout is rebuilt before publication.
            *new_keys_elements.add(i) = *src_key_elements.add(i);
        }
        (*new_keys_arr).length = copy_count as u32;
        rebuild_array_layout_from_slots(new_keys_arr);
    } else {
        (*new_keys_arr).length = 0;
    }

    set_object_keys_array(new_ptr, new_keys_arr);

    new_ptr
}

/// Copy all own enumerable fields from `src` into `dst`, using `js_object_set_field_by_name`
/// semantics (overwrite existing, append new). Used for multi-spread object literals like
/// `{...a, ...b}` to apply each additional spread after the first has been cloned via
/// `js_object_clone_with_extra`.
#[no_mangle]
pub unsafe extern "C" fn js_object_copy_own_fields(dst_i64: i64, src_f64: f64) {
    // Extract dst pointer (may be NaN-boxed or raw)
    let dst_bits = dst_i64 as u64;
    let dst_top16 = dst_bits >> 48;
    let dst_raw = if dst_top16 >= 0x7FF8 {
        (dst_bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else {
        dst_bits as usize
    };
    if dst_raw < 0x10000 {
        return;
    }
    let dst = dst_raw as *mut ObjectHeader;

    // Extract src pointer (NaN-boxed f64)
    let src_bits = src_f64.to_bits();
    let src_top16 = src_bits >> 48;
    let src_raw = if src_top16 >= 0x7FF8 {
        (src_bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else {
        src_bits as usize
    };
    if src_raw < 0x10000 {
        return;
    }
    let src = src_raw as *const ObjectHeader;

    // Iterate src's keys and copy each value via set_field_by_name.
    let src_keys = (*src).keys_array;
    if src_keys.is_null() || (src_keys as usize) < 0x10000 {
        return;
    }
    let key_count = crate::array::js_array_length(src_keys) as usize;
    let src_field_count = (*src).field_count as usize;
    let alloc_limit = std::cmp::max(src_field_count, 8);
    let header_size = std::mem::size_of::<ObjectHeader>();
    let src_fields = (src as *const u8).add(header_size) as *const u64;

    // Iterate up to `key_count`, not `min(key_count, src_field_count)`.
    // For objects with overflow fields (≥9 keys) `src_field_count` caps
    // at the inline alloc_limit (8) and the values for slots ≥ 8 live
    // in OVERFLOW_FIELDS — without iterating to `key_count` and routing
    // slots ≥ alloc_limit through `js_object_get_field`, the copy
    // silently dropped 9th..Nth properties.
    for i in 0..key_count {
        let key_val = crate::array::js_array_get(src_keys, i as u32);
        // #1781: SSO-aware copy — pre-fix the `is_string()` here
        // silently dropped any ≤5-byte key stored as a SHORT_STRING_TAG
        // value, so `Object.assign(target, src)` lost `src.id`,
        // `src.tag`, `src.name`, etc. when those slots used inline SSO.
        // Route SSO through `js_get_string_pointer_unified` so the
        // destination set-by-name path sees a stable heap pointer.
        if !key_val.is_any_string() {
            continue;
        }
        let key_f64 = f64::from_bits(key_val.bits());
        let key_ptr =
            crate::value::js_get_string_pointer_unified(key_f64) as *const crate::StringHeader;
        if key_ptr.is_null() {
            continue;
        }
        let field_f64 = if i < alloc_limit {
            let field_bits = *src_fields.add(i);
            f64::from_bits(field_bits)
        } else {
            let v = js_object_get_field(src, i as u32);
            f64::from_bits(v.bits())
        };
        js_object_set_field_by_name(dst, key_ptr, field_f64);
    }
}

/// `Object.assign(target, source)` for a single source: mutate `target` by
/// copying every own enumerable string-keyed AND symbol-keyed property from
/// `source`, returning `target`. Both args are NaN-boxed JSValues; the return
/// is `target` unchanged so the caller can chain successive sources and the
/// final returned value is the same pointer the user passed in (preserving
/// object identity, class_id, and the existing entries in the SYMBOL_PROPERTIES
/// side table — the bug from #590 was that the previous lowering allocated a
/// fresh object, breaking `result === target` and orphaning target's
/// symbol-keyed properties since the side table is keyed by raw pointer).
///
/// Per spec, undefined/null target throws TypeError; here we silently no-op
/// to match the rest of perry-runtime's permissive style. Non-object sources
/// are skipped (matching `Object.assign(t, null)` / `Object.assign(t, 5)`
/// which are spec-allowed).
#[no_mangle]
pub unsafe extern "C" fn js_object_assign_one(target_f64: f64, source_f64: f64) -> f64 {
    const POINTER_TAG_LOCAL: u64 = 0x7FFD_0000_0000_0000;
    const POINTER_MASK_LOCAL: u64 = 0x0000_FFFF_FFFF_FFFF;

    // Decode target pointer. Accept either NaN-boxed POINTER_TAG or a raw
    // pointer value (defensive: callers occasionally pass i64-typed handles).
    let tgt_bits = target_f64.to_bits();
    let tgt_top16 = tgt_bits >> 48;
    let tgt_raw = if tgt_top16 >= 0x7FF8 {
        if tgt_top16 == 0x7FFC {
            // undefined/null/bool — spec says throw TypeError; silently return.
            return target_f64;
        }
        (tgt_bits & POINTER_MASK_LOCAL) as usize
    } else {
        tgt_bits as usize
    };
    // A real `ObjectHeader` is heap-allocated and #[repr(C)] with u64 /
    // pointer fields, so a valid object pointer is always 8-byte aligned.
    // The tag / `< 0x10000` checks let an untagged, unaligned bit pattern
    // through (observed on Windows: a non-pointer value reaches here as
    // `source`, e.g. 0x1d81ff6950a). Dereferencing it as `*ObjectHeader`
    // is a misaligned read — a hard abort under debug pointer-alignment
    // checks, silent memory corruption (→ later GC out-of-bounds) in
    // release. Treat a misaligned value as a non-object and skip, exactly
    // the documented behaviour for `Object.assign(t, <non-object>)`.
    if tgt_raw < 0x10000 || tgt_raw % 8 != 0 {
        return target_f64;
    }

    // Decode source pointer. Skip null/undefined/non-pointer sources.
    let src_bits = source_f64.to_bits();
    let src_top16 = src_bits >> 48;
    let src_raw = if src_top16 >= 0x7FF8 {
        if src_top16 == 0x7FFC {
            return target_f64;
        }
        (src_bits & POINTER_MASK_LOCAL) as usize
    } else {
        src_bits as usize
    };
    // Same alignment guard as the target above — `src` is dereferenced at
    // `(*src).keys_array` just below; an unaligned non-object source must
    // be skipped, not dereferenced.
    if src_raw < 0x10000 || src_raw % 8 != 0 || src_raw == tgt_raw {
        return target_f64;
    }

    let target = tgt_raw as *mut ObjectHeader;
    let src = src_raw as *const ObjectHeader;

    // #2439: When the target is an array, an integer-keyed source property
    // (e.g. `Object.assign([1,2], {2:3})`) must grow the array's length, not
    // land as an inert object expando. `js_array_set_string_key` parses the
    // key as a canonical array index and routes through `js_array_set_f64_extend`
    // (which extends length + fills holes); non-numeric keys fall back to the
    // object-property path on the array's expando map. Detect array-ness once.
    let target_is_array = {
        let gc_header =
            (target as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        (*gc_header).obj_type == crate::gc::GC_TYPE_ARRAY
    };

    // 1) Copy own string-keyed enumerable properties from source to target,
    //    in source insertion order. Mirrors `js_object_copy_own_fields`.
    let src_keys = (*src).keys_array;
    if !src_keys.is_null() && (src_keys as usize) >= 0x10000 {
        let key_count = crate::array::js_array_length(src_keys) as usize;
        let src_field_count = (*src).field_count as usize;
        let alloc_limit = std::cmp::max(src_field_count, 8);
        let header_size = std::mem::size_of::<ObjectHeader>();
        let src_fields = (src as *const u8).add(header_size) as *const u64;
        // Same overflow-aware iteration as `js_object_copy_own_fields`.
        // #1781: SSO-aware — see the sibling helper above.
        for i in 0..key_count {
            let key_val = crate::array::js_array_get(src_keys, i as u32);
            if !key_val.is_any_string() {
                continue;
            }
            let key_f64 = f64::from_bits(key_val.bits());
            let key_ptr =
                crate::value::js_get_string_pointer_unified(key_f64) as *const crate::StringHeader;
            if key_ptr.is_null() {
                continue;
            }
            let field_f64 = if i < alloc_limit {
                let field_bits = *src_fields.add(i);
                f64::from_bits(field_bits)
            } else {
                let v = js_object_get_field(src, i as u32);
                f64::from_bits(v.bits())
            };
            if target_is_array {
                // Routes integer-index keys to array element-set (extending
                // length); non-numeric keys fall back to the object setter.
                // The array may reallocate on growth, but #233 forwarding
                // means the original `target_f64` still resolves to the new
                // head, so we keep returning it for object identity.
                crate::array::js_array_set_string_key(
                    target as *mut crate::array::ArrayHeader,
                    key_ptr,
                    field_f64,
                );
            } else {
                js_object_set_field_by_name(target, key_ptr, field_f64);
            }
        }
    }

    // 2) Copy own symbol-keyed enumerable properties from source to target.
    //    The clone-then-iterate dance is non-negotiable — the inner
    //    `js_object_set_symbol_property` re-acquires SYMBOL_PROPERTIES'
    //    Mutex; holding the lock across the iteration would deadlock.
    let entries = crate::symbol::clone_symbol_entries_for_obj_ptr(src_raw);
    for (sym_ptr, value_bits) in entries {
        let sym_f64 = f64::from_bits(POINTER_TAG_LOCAL | (sym_ptr as u64 & POINTER_MASK_LOCAL));
        let value_f64 = f64::from_bits(value_bits);
        crate::symbol::js_object_set_symbol_property(target_f64, sym_f64, value_f64);
    }

    target_f64
}
