//! Object allocation: `js_object_alloc*`, class-keys array builders,
//! shape-cache-backed fast paths, and the clone/copy/assign helpers.
//!
//! Split out of `object.rs` (issue #1103). Pure relocation — no logic
//! changes. Shared state and helpers remain in the parent `object`
//! module and are reached via `use super::*;`.

use super::*;

static CLASS_KEYS_BY_ID: std::sync::RwLock<Option<std::collections::HashMap<u32, (usize, u32)>>> =
    std::sync::RwLock::new(None);

fn remember_class_keys_array(class_id: u32, field_count: u32, keys_array: *mut ArrayHeader) {
    if class_id == 0 || keys_array.is_null() {
        return;
    }
    let mut guard = CLASS_KEYS_BY_ID.write().unwrap();
    if guard.is_none() {
        *guard = Some(std::collections::HashMap::new());
    }
    guard
        .as_mut()
        .unwrap()
        .insert(class_id, (keys_array as usize, field_count));
}

pub(crate) fn registered_class_keys_array(class_id: u32) -> Option<(*mut ArrayHeader, u32)> {
    let guard = CLASS_KEYS_BY_ID.read().ok()?;
    let (addr, field_count) = guard.as_ref()?.get(&class_id).copied()?;
    if addr == 0 {
        return None;
    }
    Some((addr as *mut ArrayHeader, field_count))
}

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

/// `Object(value)` plain-call coercion (#3149, ECMAScript §20.1.1.1 / ToObject).
///
/// Takes and returns a NaN-boxed JSValue (`f64`):
/// - `undefined` / `null` / no-arg → a fresh ordinary `{}`.
/// - an existing object/array/function (any pointer value) → returned unchanged.
/// - primitive values → boxed primitive wrapper objects so
///   `Object(true).valueOf()`, `Object(0).valueOf()`,
///   `Object("x").valueOf()`, and util.types boxed checks match Node.
///
/// The `new Object(value)` form is handled separately by
/// `js_new_function_construct`'s `"Object"` arm; this is only the bare-call
/// path that previously fell through to the generic dispatcher and returned
/// `undefined`.
#[no_mangle]
pub extern "C" fn js_object_coerce(value: f64) -> f64 {
    let jsval = crate::value::JSValue::from_bits(value.to_bits());
    if jsval.is_undefined() || jsval.is_null() {
        let obj = js_object_alloc(0, 0);
        return crate::value::js_nanbox_pointer(obj as i64);
    }
    if jsval.is_bigint() {
        return crate::builtins::js_boxed_bigint_new(value);
    }
    if unsafe { crate::symbol::js_is_symbol(value) } != 0 {
        return crate::builtins::js_boxed_symbol_new(value);
    }
    if jsval.is_pointer() {
        // Already an object/array/function — pass through unchanged.
        return value;
    }
    if jsval.is_bool() {
        return crate::builtins::js_boxed_boolean_new(value);
    }
    if jsval.is_any_string() {
        return crate::builtins::js_boxed_string_new(value, 1);
    }
    crate::builtins::js_boxed_number_new(value)
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
    let alloc_field_count = std::cmp::max(field_count as usize, crate::object::INLINE_SLOT_FLOOR);
    let fields_size = alloc_field_count * std::mem::size_of::<JSValue>();
    let total_size = header_size + fields_size;

    let ptr = arena_alloc_gc(total_size, 8, crate::gc::GC_TYPE_OBJECT) as *mut ObjectHeader;

    unsafe {
        // Initialize header
        (*ptr).object_type = crate::error::OBJECT_TYPE_REGULAR;
        (*ptr).class_id = class_id;
        (*ptr).parent_class_id = parent_class_id;
        (*ptr).field_count = field_count;
        // GC_STORE_AUDIT(INIT): fresh object starts with no per-object meta record (#6759 B).
        (*ptr).meta = ptr::null_mut();
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
    let alloc_field_count = std::cmp::max(field_count as usize, crate::object::INLINE_SLOT_FLOOR);
    let fields_size = alloc_field_count * std::mem::size_of::<JSValue>();
    let total_size = header_size + fields_size;

    let ptr = arena_alloc_gc(total_size, 8, crate::gc::GC_TYPE_OBJECT) as *mut ObjectHeader;

    unsafe {
        // Initialize header only - fields left uninitialized for constructor to fill
        (*ptr).object_type = crate::error::OBJECT_TYPE_REGULAR;
        (*ptr).class_id = class_id;
        (*ptr).parent_class_id = 0;
        (*ptr).field_count = field_count;
        // GC_STORE_AUDIT(INIT): fresh object starts with no per-object meta record (#6759 B).
        (*ptr).meta = ptr::null_mut();
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
    let alloc_field_count = std::cmp::max(field_count as usize, crate::object::INLINE_SLOT_FLOOR);
    let fields_size = alloc_field_count * std::mem::size_of::<JSValue>();
    let total_size = header_size + fields_size;

    let ptr = arena_alloc_gc(total_size, 8, crate::gc::GC_TYPE_OBJECT) as *mut ObjectHeader;

    unsafe {
        (*ptr).object_type = crate::error::OBJECT_TYPE_REGULAR;
        (*ptr).class_id = class_id;
        (*ptr).parent_class_id = parent_class_id;
        (*ptr).field_count = field_count;
        // GC_STORE_AUDIT(INIT): fresh object starts with no per-object meta record (#6759 B).
        (*ptr).meta = ptr::null_mut();
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
    let alloc_field_count = std::cmp::max(field_count as usize, crate::object::INLINE_SLOT_FLOOR);
    let fields_size = alloc_field_count * std::mem::size_of::<JSValue>();
    let total_size = header_size + fields_size;

    let ptr = arena_alloc_gc(total_size, 8, crate::gc::GC_TYPE_OBJECT) as *mut ObjectHeader;

    unsafe {
        (*ptr).object_type = crate::error::OBJECT_TYPE_REGULAR;
        (*ptr).class_id = class_id;
        (*ptr).parent_class_id = parent_class_id;
        (*ptr).field_count = field_count;
        // GC_STORE_AUDIT(INIT): fresh object starts with no per-object meta record (#6759 B).
        (*ptr).meta = ptr::null_mut();
        set_object_keys_array(ptr, keys_array);

        // PerryTS/perry#4717: initialize ALL `max(field_count, 8)` field slots to
        // `undefined`, mirroring `js_object_alloc_with_parent`. The arena hands back
        // recycled bytes, so without this a field read-before-write — or a GC that
        // scans the still-constructing instance — would observe stale arena bytes
        // from a previously-freed object (e.g. `marked`'s `this.defaults` crashing
        // with "Cannot read properties of undefined"). This used to be the caller's
        // job (the inline bump path and `json/parser.rs` both zero-filled by hand);
        // folding it in here keeps every caller — including the outlined `new C()`
        // codegen path — correct by construction.
        let fields_ptr = (ptr as *mut u8).add(header_size) as *mut JSValue;
        for i in 0..alloc_field_count {
            // GC_STORE_AUDIT(INIT): freshly allocated object field slot initialized to undefined.
            ptr::write(fields_ptr.add(i), JSValue::undefined());
        }
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
        remember_class_keys_array(class_id, field_count, cached);
        return cached;
    }
    if field_count == 0 || packed_keys_len == 0 || packed_keys.is_null() {
        let arr = crate::array::js_array_alloc_with_length_longlived(0);
        shape_cache_insert(shape_id, arr);
        remember_class_keys_array(class_id, field_count, arr);
        return arr;
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
    remember_class_keys_array(class_id, field_count, arr);
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
    let alloc_field_count = std::cmp::max(field_count as usize, crate::object::INLINE_SLOT_FLOOR);
    let fields_size = alloc_field_count * std::mem::size_of::<JSValue>();
    let total_size = header_size + fields_size;

    let ptr = arena_alloc_gc(total_size, 8, crate::gc::GC_TYPE_OBJECT) as *mut ObjectHeader;

    unsafe {
        (*ptr).object_type = crate::error::OBJECT_TYPE_REGULAR;
        (*ptr).class_id = class_id;
        (*ptr).parent_class_id = parent_class_id;
        (*ptr).field_count = field_count;
        // GC_STORE_AUDIT(INIT): fresh object starts with no per-object meta record (#6759 B).
        (*ptr).meta = ptr::null_mut();
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
    remember_class_keys_array(class_id, field_count, keys_arr);
    ptr
}

/// Allocate a subclass instance whose parent was resolved DYNAMICALLY at
/// runtime — the `class X extends _mod.default` interop-ESM shape (wall 38).
///
/// At X's compile time the parent's field layout is unknown (the `extends`
/// target is an unresolvable cross-module value, so X's `extends_name` is the
/// unresolved `"default"` and `class_field_global_index`'s parent walk bails),
/// so codegen can only size the instance for X's OWN fields. That
/// under-allocates and mis-lays-out the instance: the parent's constructor (run
/// on this `this` via `run_class_constructor_on_this_flat`) and the parent's
/// inherited methods both address the inherited `__perry_cap_*` / declared
/// fields at the PARENT's own slot indices (parent fields come first in the
/// layout), which lie past X's own-only slots → out-of-bounds reads/writes into
/// adjacent heap. That is wall 45 (`Derived extends _base.default` reads
/// `_c10`/`_c20` captures as garbage numbers/functions).
///
/// The parent edge (`js_register_class_parent_dynamic`) and the parent's
/// keys-array (`js_build_class_keys_array`) are both registered at module-init
/// time, before any `new X()`. So here — at construction time — resolve them and
/// allocate with the MERGED layout: `field_count = parent_field_count +
/// own_field_count` and `keys_array = [parent keys..] ++ [own keys..]` (parent
/// first, exactly the slot order the parent's compiled methods/ctor expect).
/// The parent's keys-array already encodes its WHOLE chain (it was built
/// parent-first at the parent's own compile time, where its ancestors were
/// known), so the immediate parent's registered keys are sufficient. Falls back
/// to the own-only layout (`js_object_alloc_class_with_keys`) when no dynamic
/// parent / parent keys are registered (e.g. the parent is a builtin or a
/// not-yet-initialized module).
#[no_mangle]
pub extern "C" fn js_object_alloc_class_dynamic_parent(
    class_id: u32,
    own_field_count: u32,
    own_packed_keys: *const u8,
    own_packed_keys_len: u32,
) -> *mut ObjectHeader {
    let parent_cid = crate::object::get_parent_class_id(class_id).unwrap_or(0);
    let parent_keys = if parent_cid != 0 {
        registered_class_keys_array(parent_cid)
    } else {
        None
    };
    let Some((parent_arr, _parent_fc)) = parent_keys else {
        // No dynamic parent layout available — own-only fallback keeps the
        // prior baseline (correct for parentless / builtin-parent classes).
        return js_object_alloc_class_with_keys(
            class_id,
            parent_cid,
            own_field_count,
            own_packed_keys,
            own_packed_keys_len,
        );
    };
    let parent_len = unsafe { (*parent_arr).length };

    // Cache the merged keys-array per class. The shape id is namespaced away
    // from the own-only shape (`+ 2_000_000`) so it can't collide with the
    // `js_build_class_keys_array` / `js_object_alloc_class_with_keys` shapes.
    let shape_id = class_id.wrapping_mul(10007).wrapping_add(2_000_000);
    let cached = shape_cache_get(shape_id);
    let (merged_arr, field_count) = if !cached.is_null() {
        (cached, unsafe { (*cached).length })
    } else {
        let own_keys: Vec<&[u8]> = if own_packed_keys.is_null() || own_packed_keys_len == 0 {
            Vec::new()
        } else {
            let bytes = unsafe {
                std::slice::from_raw_parts(own_packed_keys, own_packed_keys_len as usize)
            };
            bytes.split(|&b| b == 0).filter(|s| !s.is_empty()).collect()
        };
        let merged_len = parent_len as usize + own_keys.len();
        let arr = crate::array::js_array_alloc_with_length_longlived(merged_len as u32);
        let dst = unsafe { (arr as *mut u8).add(8) as *mut f64 };
        let src = unsafe { (parent_arr as *mut u8).add(8) as *const f64 };
        unsafe {
            for i in 0..parent_len as usize {
                let bits = (*src.add(i)).to_bits();
                // GC_STORE_AUDIT(INIT): initializing fresh longlived keys-array slot
                // with a longlived parent key; layout recorded below.
                *dst.add(i) = f64::from_bits(bits);
                crate::array::note_array_slot_layout_only(arr, i, bits);
            }
            for (j, key_bytes) in own_keys.iter().enumerate() {
                let str_ptr = crate::string::js_string_from_bytes_longlived(
                    key_bytes.as_ptr(),
                    key_bytes.len() as u32,
                );
                let nanboxed = f64::from_bits(
                    crate::value::STRING_TAG | (str_ptr as u64 & crate::value::POINTER_MASK),
                );
                let idx = parent_len as usize + j;
                // GC_STORE_AUDIT(INIT): initializing fresh longlived keys-array slot
                // with a freshly interned longlived key string; layout recorded below.
                *dst.add(idx) = nanboxed;
                crate::array::note_array_slot_layout_only(arr, idx, nanboxed.to_bits());
            }
        }
        shape_cache_insert(shape_id, arr);
        (arr, merged_len as u32)
    };

    let header_size = std::mem::size_of::<ObjectHeader>();
    let alloc_field_count = std::cmp::max(field_count as usize, crate::object::INLINE_SLOT_FLOOR);
    let fields_size = alloc_field_count * std::mem::size_of::<JSValue>();
    let total_size = header_size + fields_size;
    let ptr = arena_alloc_gc(total_size, 8, crate::gc::GC_TYPE_OBJECT) as *mut ObjectHeader;
    unsafe {
        (*ptr).object_type = crate::error::OBJECT_TYPE_REGULAR;
        (*ptr).class_id = class_id;
        (*ptr).parent_class_id = parent_cid;
        (*ptr).field_count = field_count;
        // GC_STORE_AUDIT(INIT): fresh object starts with no per-object meta record (#6759 B).
        (*ptr).meta = ptr::null_mut();
        let fields_ptr = (ptr as *mut u8).add(header_size) as *mut JSValue;
        for i in 0..alloc_field_count {
            // GC_STORE_AUDIT(INIT): freshly allocated object field slot is initialized pointer-free.
            ptr::write(fields_ptr.add(i), JSValue::undefined());
        }
        set_object_keys_array(ptr, merged_arr);
        crate::gc::layout_init_pointer_free(ptr as *mut u8);
    }
    remember_class_keys_array(class_id, field_count, merged_arr);
    ptr
}

/// Keepalive anchor — `js_object_alloc_class_dynamic_parent` is a
/// generated-code-only callee, so the auto-optimize whole-program build would
/// otherwise dead-strip it (see the FFI-symbol-link-break class).
#[used]
static KEEP_JS_OBJECT_ALLOC_CLASS_DYNAMIC_PARENT: extern "C" fn(
    u32,
    u32,
    *const u8,
    u32,
) -> *mut ObjectHeader = js_object_alloc_class_dynamic_parent;

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
    let alloc_field_count = std::cmp::max(field_count as usize, crate::object::INLINE_SLOT_FLOOR);
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
        // GC_STORE_AUDIT(INIT): fresh object starts with no per-object meta record (#6759 B).
        (*obj_ptr).meta = ptr::null_mut();

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

    // If source is invalid OR not a genuine heap object, create an empty object
    // with capacity for the static props. Physical slot count = max(extra_count,
    // 8) to match js_object_set_field_by_name's alloc_limit = max(field_count, 8).
    // The `top16 >= 0x7FF8` extraction above admits SSO/BigInt/INT32/negative-
    // double payloads and exotic headers (Map/Set/Promise/…) whose bytes are not
    // an ObjectHeader; deref'ing `field_count`/`keys_array` off them is type
    // confusion (#6070). The sole production caller (`js_structured_clone`)
    // already gates on GC_TYPE_OBJECT, so this only hardens against a future one.
    let src_is_object = src_raw >= 0x10000
        && !crate::value::addr_class::is_handle_band(src_raw)
        && matches!(
            crate::value::addr_class::try_read_gc_header(src_raw),
            Some(h) if h.obj_type == crate::gc::GC_TYPE_OBJECT
        );
    if !src_is_object {
        let phys_slots = std::cmp::max(extra_count, crate::object::INLINE_SLOT_FLOOR as u32);
        let total_size = header_size + phys_slots as usize * 8;
        let new_ptr = arena_alloc_gc(total_size, 8, crate::gc::GC_TYPE_OBJECT) as *mut ObjectHeader;
        (*new_ptr).object_type = crate::error::OBJECT_TYPE_REGULAR;
        (*new_ptr).class_id = 0;
        (*new_ptr).parent_class_id = 0;
        (*new_ptr).field_count = 0;
        // GC_STORE_AUDIT(INIT): fresh object starts with no per-object meta record (#6759 B).
        (*new_ptr).meta = ptr::null_mut();
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
    let phys_slots = std::cmp::max(
        src_field_count + extra_count,
        crate::object::INLINE_SLOT_FLOOR as u32,
    );
    let total_size = header_size + phys_slots as usize * 8;
    let new_ptr = arena_alloc_gc(total_size, 8, crate::gc::GC_TYPE_OBJECT) as *mut ObjectHeader;
    (*new_ptr).object_type = crate::error::OBJECT_TYPE_REGULAR;
    (*new_ptr).class_id = 0;
    (*new_ptr).parent_class_id = 0;
    // Logical field count starts at src's count. js_object_set_field_by_name bumps it when
    // appending new keys.
    (*new_ptr).field_count = src_field_count;
    // GC_STORE_AUDIT(INIT): fresh object starts with no per-object meta record (#6759 B).
    (*new_ptr).meta = ptr::null_mut();

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

    // Extract + VALIDATE the src pointer (2026-07-02 audit P0). The old
    // `top16 >= 0x7FF8` catch-all admitted SSO strings (0x7FF9), registry
    // handles, INT32s, and negative doubles, and the only guard was
    // `< 0x10000` — so `{...response}` (a POINTER-tagged fetch-band id) or
    // `{..."ab"}` deref'd a non-heap address as an ObjectHeader (Linux
    // SIGSEGV), and `{...map}` walked a MapHeader's bytes as object fields.
    // Spec (CopyDataProperties): non-objects with no own enumerable string
    // props contribute nothing — so anything that is not a genuine heap
    // OBJECT is skipped. (Known remaining gap, safe now instead of UB:
    // spreading a STRING should yield its index properties; it currently
    // yields none.)
    let src_bits = src_f64.to_bits();
    let src_top16 = src_bits >> 48;
    // Only a POINTER-tagged value can be a spreadable heap object.
    if src_top16 != 0x7FFD {
        return;
    }
    let src_raw = (src_bits & 0x0000_FFFF_FFFF_FFFF) as usize;
    if crate::value::addr_class::is_handle_band(src_raw) || src_raw < 0x10000 {
        return;
    }
    // Probe the GcHeader without deref-faulting and require a real object
    // (Maps/Sets/Promises/etc. have their own layouts — reading their bytes
    // as ObjectHeader fields is type confusion).
    match crate::value::addr_class::try_read_gc_header(src_raw) {
        Some(h) if h.obj_type == crate::gc::GC_TYPE_OBJECT => {}
        _ => return,
    }
    let src = src_raw as *const ObjectHeader;

    // #6667: a native-module namespace (`{ ...require("crypto") }`) stores no
    // real fields — only the internal `__module__` sentinel — so the raw
    // keys_array walk below would copy nothing usable. Enumerate + resolve its
    // export surface instead (the same list `Object.keys` returns), so wildcard
    // interop and object spread see the exports Node's namespace exposes.
    {
        let scope = crate::gc::RuntimeHandleScope::new();
        let dst_h = scope.root_raw_mut_ptr(dst);
        if super::native_module::copy_native_module_exports(src, |key_ptr, value| {
            js_object_set_field_by_name(dst_h.get_raw_mut_ptr::<ObjectHeader>(), key_ptr, value);
        }) {
            return;
        }
    }

    // Iterate src's keys and copy each value via set_field_by_name.
    let src_keys = (*src).keys_array;
    if src_keys.is_null() || (src_keys as usize) < 0x10000 {
        return;
    }
    let key_count = crate::array::js_array_length(src_keys) as usize;
    let src_field_count = (*src).field_count as usize;
    let alloc_limit = std::cmp::max(src_field_count, crate::object::INLINE_SLOT_FLOOR);
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
        // Private elements (`#x`) live in a class instance's keys_array but are
        // never copied by object spread / Object.assign.
        if crate::object::instance_private_key_hidden(src, key_val) {
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
fn throw_object_assign_nullish_target() -> ! {
    let message = "Cannot convert undefined or null to object";
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

#[no_mangle]
pub unsafe extern "C" fn js_object_assign_validate_target(target_f64: f64) -> f64 {
    let target = JSValue::from_bits(target_f64.to_bits());
    if target.is_undefined() || target.is_null() {
        throw_object_assign_nullish_target();
    }
    js_object_coerce(target_f64)
}

/// Parse a property name as a canonical array index (ECMA-262 CanonicalNumeric
/// IndexString restricted to non-negative integers `< 2^32-1`): no leading
/// zeros, round-trips through `to_string`. Used to recognise the in-range code-
/// unit indices of a boxed-String `Object.assign` target.
fn assign_canonical_index(name: &str) -> Option<u32> {
    if name.is_empty() || (name.len() > 1 && name.as_bytes()[0] == b'0') {
        return None;
    }
    let value = name.parse::<u32>().ok()?;
    if value == u32::MAX || value.to_string() != name {
        return None;
    }
    Some(value)
}

/// Spec `Set(to, key, value, true)` inside `Object.assign` uses the strict
/// receiver, so a write that the ordinary `[[Set]]` would reject throws a
/// `TypeError`. Perry's `js_object_set_field_by_name` silently no-ops those
/// cases, so detect them up front: a non-writable existing own data property,
/// an accessor own property with no setter, or a new property on a
/// non-extensible target. Throws when the write must fail.
unsafe fn object_assign_throw_if_set_rejected(
    target: *mut ObjectHeader,
    key_ptr: *const crate::StringHeader,
    name: &str,
) {
    if target.is_null() || (target as usize) <= 0x10000 {
        return;
    }
    // A boxed String primitive target — `Object.assign('abc', src)` does
    // `ToObject('abc')` — exposes its code units as non-writable, non-
    // configurable own index properties ("0".."len-1"), which aren't stored in
    // `keys_array`. A strict `Set` to an in-range index must throw, so detect it
    // before the keys_array-based checks treat the index as a writable new
    // property (test262 Object/assign/assignment-to-readonly-property-of-target
    // -must-throw-a-typeerror-exception).
    if let Some(idx) = assign_canonical_index(name) {
        let target_f64 = f64::from_bits(JSValue::pointer(target as *mut u8).bits());
        if crate::builtins::boxed_primitive_to_string_tag(target_f64) == Some("String") {
            if let Some((_, payload)) = crate::builtins::boxed_primitive_payload(target_f64) {
                let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
                if let Some((ptr, blen)) =
                    crate::string::str_bytes_from_jsvalue(payload, &mut scratch)
                {
                    let len = if ptr.is_null() {
                        0
                    } else {
                        crate::string::compute_utf16_len(ptr, blen)
                    };
                    if idx < len {
                        throw_object_assign_readonly(name);
                    }
                }
            }
        }
    }
    // Accessor own property: a setter must exist, else the write fails. Check
    // this BEFORE `own_key_present`: an accessor-only property (`{ set foo(){} }`)
    // lives in the accessor side table and may have no `keys_array` entry, so
    // `own_key_present` can report it absent — which on a frozen/non-extensible
    // target would mis-classify the setter call as a forbidden new-property add
    // (test262 assign/target-is-frozen-accessor-property-set-succeeds).
    if let Some(acc) = super::get_accessor_descriptor(target as usize, name) {
        if acc.set == 0 {
            throw_object_assign_readonly(name);
        }
        return;
    }
    let exists = own_key_present(target, key_ptr);
    if exists {
        // Data own property: must be writable.
        if let Some(attrs) = super::get_property_attrs(target as usize, name) {
            if !attrs.writable() {
                throw_object_assign_readonly(name);
            }
        }
        return;
    }
    // New property: target must be extensible.
    let gc = gc_header_for(target);
    if (*gc)._reserved & crate::gc::OBJ_FLAG_NO_EXTEND != 0 {
        throw_object_assign_readonly(name);
    }
}

fn throw_object_assign_readonly(name: &str) -> ! {
    throw_object_type_error_with_suffix(
        "Cannot assign to read only property '",
        &format!("{name}' of object '#<Object>'"),
    )
}

/// Strict `Set(to, sym, value, true)` rejection check for a symbol-keyed
/// `Object.assign` write: a non-writable existing symbol data property, an
/// accessor symbol property with no setter, or a new symbol property on a
/// non-extensible target each make the write fail, which under throwing `Set`
/// semantics is a `TypeError`. The string-keyed counterpart is
/// `object_assign_throw_if_set_rejected`.
unsafe fn object_assign_throw_if_symbol_set_rejected(target: *mut ObjectHeader, sym_ptr: usize) {
    let owner = target as usize;
    let existing = crate::symbol::symbol_property_root_bits(owner, sym_ptr).is_some()
        || crate::symbol::symbol_accessor_descriptor_bits(owner, sym_ptr).is_some();
    if existing {
        if let Some((_get, set)) = crate::symbol::symbol_accessor_descriptor_bits(owner, sym_ptr) {
            if set == 0 {
                throw_object_assign_readonly("Symbol()");
            }
        } else if let Some(attrs) = crate::symbol::get_symbol_property_attrs(owner, sym_ptr) {
            if !attrs.writable() {
                throw_object_assign_readonly("Symbol()");
            }
        }
    } else {
        let gc = gc_header_for(target);
        if (*gc)._reserved & crate::gc::OBJ_FLAG_NO_EXTEND != 0 {
            throw_object_assign_readonly("Symbol()");
        }
    }
}

unsafe fn object_assign_set_string_key(
    target: *mut ObjectHeader,
    target_is_array: bool,
    key_ptr: *const crate::StringHeader,
    value_f64: f64,
) {
    // `Object.assign(process.env, parsed)` — how `@next/env` loads `.env` files.
    // `process.env.X` READS lower to `js_getenv` (the real environment), so a
    // field stored on the cached env object leaves every read `undefined`: a
    // Next.js standalone server saw NONE of its `.env` config (myairank's
    // `DATABASE_URL` vanished, mysql2 then connected with an empty user and the
    // MySQL handshake timed out). Route the write through the env setter so it
    // lands where the reads look.
    //
    // This hook lives at the single write funnel rather than as an early exit in
    // `js_object_assign_one`, so every source shape still flows through the
    // decoding below: a primitive/array/proxy source is enumerated correctly,
    // and a nullish source is skipped per spec instead of throwing.
    if !target_is_array && crate::process::is_process_env_ptr(target as usize) {
        crate::process::js_setenv(key_ptr, value_f64);
        return;
    }
    if target_is_array {
        // Routes integer-index keys to array element-set (extending length);
        // non-numeric keys fall back to the object setter.
        crate::array::js_array_set_string_key(
            target as *mut crate::array::ArrayHeader,
            key_ptr,
            value_f64,
        );
    } else {
        // Strict `Set` semantics: reject (throw) a write the ordinary `[[Set]]`
        // would silently drop.
        let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        if let Some(name_bytes) = crate::string::js_string_key_bytes(
            crate::value::JSValue::string_ptr(key_ptr as *mut _),
            &mut sso,
        ) {
            if let Ok(name) = std::str::from_utf8(name_bytes) {
                object_assign_throw_if_set_rejected(target, key_ptr, name);
            }
        }
        js_object_set_field_by_name(target, key_ptr, value_f64);
    }
}

unsafe fn object_assign_string_source(
    target: *mut ObjectHeader,
    target_is_array: bool,
    source_f64: f64,
) {
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let Some((ptr, blen)) = crate::string::str_bytes_from_jsvalue(source_f64, &mut scratch) else {
        return;
    };
    if ptr.is_null() {
        return;
    }
    let bytes = std::slice::from_raw_parts(ptr, blen as usize);
    let Ok(s) = std::str::from_utf8(bytes) else {
        return;
    };
    for (idx, ch) in s.chars().enumerate() {
        let key = idx.to_string();
        let key_ptr = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
        let mut buf = [0u8; 4];
        let ch_str = ch.encode_utf8(&mut buf);
        let value_ptr = crate::string::js_string_from_bytes(ch_str.as_ptr(), ch_str.len() as u32);
        let value_f64 = f64::from_bits(JSValue::string_ptr(value_ptr).bits());
        object_assign_set_string_key(target, target_is_array, key_ptr, value_f64);
    }
}

/// Copy a Proxy source's own enumerable properties onto `target`, driving the
/// proxy's `ownKeys` / `getOwnPropertyDescriptor` / `get` traps in spec order.
/// Any trap that throws longjmps straight past this frame to the caller's
/// `try`/`catch`, which is exactly the abrupt-completion propagation
/// `Object.assign` requires.
unsafe fn object_assign_proxy_source(
    target: *mut ObjectHeader,
    target_f64: f64,
    target_is_array: bool,
    source_f64: f64,
) {
    // `[[OwnPropertyKeys]]` — fires the ownKeys trap (throw propagates).
    let keys_arr = crate::proxy::js_proxy_own_keys(source_f64);
    let keys_val = JSValue::from_bits(keys_arr.to_bits());
    if !keys_val.is_pointer() {
        return;
    }
    let arr = keys_val.as_pointer::<crate::array::ArrayHeader>();
    if arr.is_null() {
        return;
    }
    let n = crate::array::js_array_length(arr);
    for i in 0..n {
        let key = crate::array::js_array_get(arr, i);
        let key_f64 = f64::from_bits(key.bits());
        // `[[GetOwnProperty]]` — fires the getOwnPropertyDescriptor trap.
        let desc = crate::proxy::js_reflect_get_own_property_descriptor(source_f64, key_f64);
        let desc_ptr = (desc.to_bits() & crate::value::POINTER_MASK) as *const ObjectHeader;
        if desc.to_bits() == JSValue::undefined().bits() || desc_ptr.is_null() {
            continue;
        }
        let ek = crate::string::js_string_from_bytes(b"enumerable".as_ptr(), 10);
        if crate::value::js_is_truthy(crate::object::js_object_get_field_by_name_f64(desc_ptr, ek))
            == 0
        {
            continue;
        }
        // `[[Get]]` — fires the get trap.
        let value_f64 = crate::proxy::js_proxy_get(source_f64, key_f64);
        if key.is_any_string() {
            let key_ptr =
                crate::value::js_get_string_pointer_unified(key_f64) as *const crate::StringHeader;
            if !key_ptr.is_null() {
                object_assign_set_string_key(target, target_is_array, key_ptr, value_f64);
            }
        } else if key.is_pointer() {
            // Strict `Set` semantics for symbol keys, same as the ordinary path.
            let sym_ptr = (key.bits() & crate::value::POINTER_MASK) as usize;
            object_assign_throw_if_symbol_set_rejected(target, sym_ptr);
            crate::symbol::js_object_set_symbol_property(target_f64, key_f64, value_f64);
        }
    }
}

/// Per spec, undefined/null target throws TypeError. Non-object sources
/// are skipped except string primitives, which expose enumerable index
/// properties (`Object.assign({}, "ab") -> {0:"a",1:"b"}`).
#[no_mangle]
pub unsafe extern "C" fn js_object_assign_one(target_f64: f64, source_f64: f64) -> f64 {
    let target_f64 = js_object_assign_validate_target(target_f64);

    // NOTE: a `process.env` target is handled in `object_assign_set_string_key`
    // (the single write funnel) rather than here. An early exit at this point
    // would have to re-implement source decoding, and the version that did got
    // all three edge cases wrong: it cast any source pointer to `ObjectHeader`
    // (type confusion on a string/array source) and it enumerated the source
    // with `js_object_keys_value`, which *throws* on `null`/`undefined` instead
    // of skipping it as the spec requires.
    let target_value = JSValue::from_bits(target_f64.to_bits());
    if !target_value.is_pointer() {
        return target_f64;
    }
    let tgt_raw = target_value.as_pointer::<u8>() as usize;
    // A real `ObjectHeader` is heap-allocated and #[repr(C)] with u64 /
    // pointer fields, so a valid object pointer is always 8-byte aligned.
    // If a non-object target reaches here after nullish validation, skip
    // mutation rather than dereferencing an invalid pointer.
    if tgt_raw < 0x10000 || !tgt_raw.is_multiple_of(8) {
        return target_f64;
    }

    let target = tgt_raw as *mut ObjectHeader;

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

    let source = JSValue::from_bits(source_f64.to_bits());
    if source.is_undefined() || source.is_null() {
        return target_f64;
    }
    if source.is_any_string() {
        object_assign_string_source(target, target_is_array, source_f64);
        return target_f64;
    }

    // A Proxy source isn't an `ObjectHeader` (its NaN-box payload is a small
    // registry id, not a heap pointer), so the raw `keys_array` walk below
    // would skip it silently. Spec requires enumerating it through its traps —
    // `[[OwnPropertyKeys]]` (ownKeys), `[[GetOwnProperty]]`
    // (getOwnPropertyDescriptor) for the enumerable test, then `[[Get]]` for
    // each value — with every trap's abrupt completion propagating out (test262
    // Object/assign/source-own-prop-error + source-own-prop-keys-error).
    if crate::proxy::js_proxy_is_proxy(source_f64) != 0 {
        object_assign_proxy_source(target, target_f64, target_is_array, source_f64);
        return target_f64;
    }

    // Decode source pointer. Skip null/undefined/non-pointer sources.
    if !source.is_pointer() {
        return target_f64;
    }
    let src_raw = source.as_pointer::<u8>() as usize;
    // Same alignment guard as the target above — `src` is dereferenced at
    // `(*src).keys_array` just below; an unaligned non-object source must
    // be skipped, not dereferenced. Reject the WHOLE handle band, not just a
    // `< 0x10000` floor: a common-band registry id (crypto `Hash`, `Blob`, …)
    // can sit above 0x10000 and be 8-aligned, so the old floor let it through
    // and `(*src).keys_array` read unmapped memory (SIGSEGV). A native handle
    // has no own enumerable properties to spread, so skipping it yields `{}`,
    // matching Node (`{...new Blob([])}` === `{}`). test_gap_handle_band_object_ops
    // `{...blob}`/`{...hash}`.
    if !crate::value::addr_class::is_above_handle_band(src_raw)
        || !src_raw.is_multiple_of(8)
        || crate::symbol::is_registered_symbol(src_raw)
    {
        return target_f64;
    }

    // A function/closure source is NOT an `ObjectHeader`: reading `keys_array`
    // off it dereferences a bogus field, yielding a garbage `key_count` and a
    // runaway copy loop. Enumerate the closure's own *enumerable* dynamic props
    // instead — the built-in `length`/`name`/`prototype` slots are
    // non-enumerable and excluded, matching `Object.keys`/`getOwnPropertyNames`.
    // (Stripe's `protoExtend` does `Object.assign(Constructor, Super)` to copy a
    // resource class's enumerable statics like `.extend`/`.method`; without this
    // the call hung at `import 'stripe'`.)
    if crate::closure::is_closure_ptr(src_raw) {
        for (name, value) in crate::closure::closure_dynamic_props_snapshot(src_raw) {
            if matches!(name.as_str(), "length" | "name" | "prototype") {
                continue;
            }
            if crate::closure::closure_is_key_deleted(src_raw, &name) {
                continue;
            }
            if let Some(attrs) = get_property_attrs(src_raw, &name) {
                if !attrs.enumerable() {
                    continue;
                }
            }
            let key_ptr = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            object_assign_set_string_key(target, target_is_array, key_ptr, value);
        }
        return target_f64;
    }

    let src = src_raw as *const ObjectHeader;

    // #6667: native-module namespace source (`Object.assign(t, require("crypto"))`).
    // Its exports resolve lazily through the vtable, so the raw keys_array walk
    // below sees only `__module__`. Enumerate + resolve the export surface, then
    // return — native-module namespaces carry no own symbol-keyed properties, so
    // the symbol-copy tail below would be a no-op.
    {
        let scope = crate::gc::RuntimeHandleScope::new();
        let tgt_h = scope.root_raw_mut_ptr(target);
        if super::native_module::copy_native_module_exports(src, |key_ptr, value| {
            object_assign_set_string_key(
                tgt_h.get_raw_mut_ptr::<ObjectHeader>(),
                target_is_array,
                key_ptr,
                value,
            );
        }) {
            // `copy_native_module_exports` allocates (fresh export closures +
            // key strings); a minor GC there can evacuate `target`. Return the
            // handle-reloaded pointer so the caller threads the post-GC
            // location, not the stale `target_f64`.
            return crate::value::js_nanbox_pointer(tgt_h.get_raw_mut_ptr::<ObjectHeader>() as i64);
        }
    }

    // An array source (`Object.assign(t, [1,2])`, `{ ...[1,2] }`) stores its
    // indexed elements in the `ArrayHeader` element buffer, NOT in an
    // `ObjectHeader.keys_array`. ArrayHeader has no such field, so the
    // keys_array read below would deref a garbage pointer and crash (the prior
    // behavior — a hard SIGSEGV on a common operation). Enumerate the dense
    // index range directly through the array API instead. (#5347 Object/assign)
    // Classify the source's GC type once. A genuine plain/class object keeps
    // its own string-keyed props in `ObjectHeader.keys_array`; an array keeps
    // indexed elements in its `ArrayHeader` buffer (handled below). Anything
    // else (Map/Set/Promise/Date/WeakMap/…) has its OWN header layout — reading
    // its bytes as `ObjectHeader.keys_array` yields a garbage pointer that the
    // key-copy loop then walks as an array (a memory-layout-dependent SIGBUS on
    // `Object.assign({}, new Map())`, #6070). Per CopyDataProperties such
    // exotics expose no own enumerable string keys through this path, so they
    // contribute nothing — skip them (mirrors `js_object_copy_own_fields`).
    // Probe the GcHeader without deref-faulting — a handle-band id that passed
    // the guards above would otherwise deref a non-heap address; mirrors the
    // sibling `js_object_copy_own_fields`.
    let source_obj_type = match crate::value::addr_class::try_read_gc_header(src_raw) {
        Some(h) => h.obj_type,
        None => return target_f64,
    };
    let source_is_array = source_obj_type == crate::gc::GC_TYPE_ARRAY;

    // 1) Copy own string-keyed enumerable properties from source to target,
    //    in source insertion order. Mirrors `js_object_copy_own_fields`.
    if source_is_array {
        let arr = src_raw as *const crate::array::ArrayHeader;
        let n = crate::array::js_array_length(arr);
        // Snapshot string expandos (`arr.foo = …`, kept in the named-property
        // side table) BEFORE the index loop: that loop allocates, which can
        // trigger a GC that rekeys the side table to the moved array's new
        // address — after which a lookup by this (stale) address would miss
        // them. They sort AFTER the integer indices in [[OwnPropertyKeys]] order.
        let expandos: Vec<(String, f64)> = crate::array::array_named_property_names(arr, true)
            .into_iter()
            .filter_map(|name| {
                crate::array::array_named_property_get_by_name(arr, &name).map(|v| (name, v))
            })
            .collect();
        for i in 0..n {
            // Holes (absent indices) in a sparse array are NOT own enumerable
            // properties and must be skipped — Object.assign only copies own
            // enumerable properties (test262 assign/target-Array.js).
            if !crate::array::array_spec_has_index(arr, i) {
                continue;
            }
            let value = crate::array::js_array_get(arr, i);
            let key = i.to_string();
            let key_ptr = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
            object_assign_set_string_key(
                target,
                target_is_array,
                key_ptr,
                f64::from_bits(value.bits()),
            );
        }
        for (name, value) in expandos {
            let key_ptr = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            object_assign_set_string_key(target, target_is_array, key_ptr, value);
        }
    } else if source_obj_type == crate::gc::GC_TYPE_OBJECT {
        let src_keys = (*src).keys_array;
        if !src_keys.is_null() && (src_keys as usize) >= 0x10000 {
            // Cap the key count at the keys array's capacity: a malformed keys
            // array can report a bogus, pointer-sized length, and an unclamped
            // `0..key_count` copy loop turns Object.assign / object spread into a
            // minutes-long spin (each `js_array_get` on the phantom tail walks
            // the slow sparse path). Same guard as the wide-key field-get walk.
            let key_count = crate::array::keys_array_len_capped_to_capacity(src_keys);
            // Use the public [[Get]] path, not raw field slots, so accessors run
            // and abrupt completions propagate the way Object.assign requires.
            for i in 0..key_count {
                let key_val = crate::array::js_array_get(src_keys, i as u32);
                if !key_val.is_any_string() {
                    continue;
                }
                // Private elements (`#x`) live in a class instance's keys_array
                // but are never copied by Object.assign / object spread.
                if crate::object::instance_private_key_hidden(src, key_val) {
                    continue;
                }
                let key_f64 = f64::from_bits(key_val.bits());
                let key_ptr = crate::value::js_get_string_pointer_unified(key_f64)
                    as *const crate::StringHeader;
                if key_ptr.is_null() {
                    continue;
                }
                let mut sso_buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
                if let Some(name_bytes) = crate::string::js_string_key_bytes(key_val, &mut sso_buf)
                {
                    if let Ok(name) = std::str::from_utf8(name_bytes) {
                        if let Some(attrs) = get_property_attrs(src_raw, name) {
                            if !attrs.enumerable() {
                                continue;
                            }
                        }
                    }
                }
                let field_f64 = f64::from_bits(js_object_get_field_by_name(src, key_ptr).bits());
                object_assign_set_string_key(target, target_is_array, key_ptr, field_f64);
            }
        }
    }

    // 2) Copy own symbol-keyed enumerable properties from source to target,
    //    in `[[OwnPropertyKeys]]` symbol order (after the string keys). Use the
    //    full own-symbol-key list — `clone_symbol_entries_for_obj_ptr` only
    //    surfaces symbols with a stored *value*, missing accessor-only symbols
    //    (`Object.defineProperty(o, sym, { get })`), so a symbol getter never
    //    ran during assign (test262 assign/strings-and-symbol-order). Snapshot
    //    the symbol pointers first: the inner `[[Get]]` / set re-acquire
    //    SYMBOL_PROPERTIES, so iterating a held snapshot avoids re-entrancy.
    let sym_keys: Vec<usize> = {
        let arr_raw = crate::symbol::js_object_get_own_property_symbols(source_f64);
        let mut v = Vec::new();
        if arr_raw != 0 {
            let arr = arr_raw as *const crate::array::ArrayHeader;
            if !arr.is_null() {
                let n = crate::array::js_array_length(arr);
                for i in 0..n {
                    let sv = crate::array::js_array_get(arr, i);
                    let p = (sv.bits() & crate::value::POINTER_MASK) as usize;
                    if p != 0 {
                        v.push(p);
                    }
                }
            }
        }
        v
    };
    for sym_ptr in sym_keys {
        if !crate::symbol::symbol_property_is_enumerable(src_raw, sym_ptr) {
            continue;
        }
        let sym_f64 = f64::from_bits(JSValue::pointer(sym_ptr as *const u8).bits());
        // Read the source value through `[[Get]]`, not the raw side-table bits,
        // so a symbol-keyed accessor's getter runs during `Object.assign`
        // (test262 assign/strings-and-symbol-order). The earlier string-key
        // copy already uses `[[Get]]` via `js_object_get_field_by_name`.
        let value_f64 = crate::symbol::js_object_get_symbol_property(source_f64, sym_f64);
        // Strict `Set` semantics for symbol-keyed writes too.
        object_assign_throw_if_symbol_set_rejected(target, sym_ptr);
        crate::symbol::js_object_set_symbol_property(target_f64, sym_f64, value_f64);
    }

    target_f64
}
