//! ArrayHeader struct, pointer-cleaning / GC-layout helpers, and the
//! tagged-template `.raw` side-table. Every other `array::*` sub-module
//! pulls these basics in via `use super::*;`.

use std::cell::RefCell;
use std::collections::HashMap;

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

    /// Tagged-template template-object cache — maps a stable compile-time
    /// call-site id to the frozen cooked/raw array pair for that site.
    static TEMPLATE_OBJECT_CACHE: RefCell<HashMap<u64, (*mut ArrayHeader, *mut ArrayHeader)>> =
        RefCell::new(HashMap::new());

    /// Own non-index properties for Array exotic objects.
    ///
    /// Perry's `ArrayHeader` intentionally stays compact: `length`,
    /// `capacity`, then inline element slots. Treating that header as an
    /// `ObjectHeader` corrupts reads of named keys, so array expandos live in
    /// this side table keyed by the array allocation address. Numeric array
    /// indices remain in element storage; canonical non-indices such as
    /// `"4294967295"` are stored here per ECMA-262.
    static ARRAY_NAMED_PROPS: RefCell<HashMap<usize, Vec<ArrayNamedProperty>>> =
        RefCell::new(HashMap::new());
}

#[derive(Clone)]
struct ArrayNamedProperty {
    name: String,
    value: f64,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NumericArrayLayout {
    RawF64 = 1,
}

#[inline]
pub(crate) fn array_object_flags(arr: *const ArrayHeader) -> u16 {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() || (arr as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return 0;
    }
    unsafe {
        let gc_header =
            (arr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*gc_header).obj_type == crate::gc::GC_TYPE_ARRAY {
            (*gc_header)._reserved
        } else {
            0
        }
    }
}

#[inline]
pub(crate) fn array_is_frozen(arr: *const ArrayHeader) -> bool {
    array_object_flags(arr) & crate::gc::OBJ_FLAG_FROZEN != 0
}

#[inline]
pub(crate) fn array_is_sealed_or_no_extend(arr: *const ArrayHeader) -> bool {
    array_object_flags(arr) & (crate::gc::OBJ_FLAG_SEALED | crate::gc::OBJ_FLAG_NO_EXTEND) != 0
}

unsafe fn mark_template_array_frozen(arr: *mut ArrayHeader) {
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() || (arr as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return;
    }
    let gc_header = (arr as *mut u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
    if (*gc_header).obj_type == crate::gc::GC_TYPE_ARRAY {
        (*gc_header)._reserved |=
            crate::gc::OBJ_FLAG_FROZEN | crate::gc::OBJ_FLAG_SEALED | crate::gc::OBJ_FLAG_NO_EXTEND;
    }
}

unsafe fn register_template_raw_pair(cooked: *mut ArrayHeader, raw: *mut ArrayHeader) {
    if cooked.is_null() || raw.is_null() {
        return;
    }
    TEMPLATE_RAW_MAP.with(|m| {
        m.borrow_mut().insert(cooked as usize, raw);
    });
}

unsafe fn install_template_raw_property(
    cooked_handle: &crate::gc::RuntimeHandle<'_>,
    raw_handle: &crate::gc::RuntimeHandle<'_>,
) {
    let raw_key = crate::string::js_string_from_bytes(b"raw".as_ptr(), 3);
    let cooked = cooked_handle.get_raw_mut_ptr::<ArrayHeader>();
    let raw = raw_handle.get_raw_mut_ptr::<ArrayHeader>();
    if cooked.is_null() || raw.is_null() {
        return;
    }
    array_named_property_set(cooked, raw_key, crate::value::js_nanbox_pointer(raw as i64));
    crate::object::set_property_attrs(
        cooked as usize,
        "raw".to_string(),
        crate::object::PropertyAttrs::new(false, false, false),
    );
}

/// Register the (cooked, raw) pair for a tagged-template call. Returns
/// `cooked` (so the codegen can chain it inline into the call args).
#[no_mangle]
pub extern "C" fn js_tagged_template_register_raw(
    cooked: *mut ArrayHeader,
    raw: *mut ArrayHeader,
) -> *mut ArrayHeader {
    let cooked = clean_arr_ptr_mut(cooked);
    let raw = clean_arr_ptr_mut(raw);
    unsafe {
        register_template_raw_pair(cooked, raw);
    }
    cooked
}

/// Return the frozen template-strings object for a tagged-template call site,
/// initializing the per-site cooked/raw pair on first evaluation.
#[no_mangle]
pub extern "C" fn js_tagged_template_get_or_init(
    site_id: u64,
    cooked: *mut ArrayHeader,
    raw: *mut ArrayHeader,
) -> *mut ArrayHeader {
    if let Some(cached) = TEMPLATE_OBJECT_CACHE.with(|m| {
        m.borrow()
            .get(&site_id)
            .map(|&(cached_cooked, _)| cached_cooked)
    }) {
        return cached;
    }

    let cooked = clean_arr_ptr_mut(cooked);
    let raw = clean_arr_ptr_mut(raw);
    if cooked.is_null() || raw.is_null() {
        return cooked;
    }

    let scope = crate::gc::RuntimeHandleScope::new();
    let cooked_handle = scope.root_raw_mut_ptr(cooked);
    let raw_handle = scope.root_raw_mut_ptr(raw);
    unsafe {
        install_template_raw_property(&cooked_handle, &raw_handle);
        let cooked = cooked_handle.get_raw_mut_ptr::<ArrayHeader>();
        let raw = raw_handle.get_raw_mut_ptr::<ArrayHeader>();
        mark_template_array_frozen(raw);
        mark_template_array_frozen(cooked);
        register_template_raw_pair(cooked, raw);
        TEMPLATE_OBJECT_CACHE.with(|m| {
            m.borrow_mut().insert(site_id, (cooked, raw));
        });
    }
    cooked_handle.get_raw_mut_ptr::<ArrayHeader>()
}

#[used]
static KEEP_TAGGED_TEMPLATE_GET_OR_INIT: extern "C" fn(
    u64,
    *mut ArrayHeader,
    *mut ArrayHeader,
) -> *mut ArrayHeader = js_tagged_template_get_or_init;

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
    let mut visitor = crate::gc::RuntimeRootVisitor::for_copy(mark);
    scan_template_raw_roots_mut(&mut visitor);
}

pub fn scan_template_raw_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    TEMPLATE_OBJECT_CACHE.with(|m| {
        let mut map = m.borrow_mut();
        for (_, (cooked_ptr, raw_ptr)) in map.iter_mut() {
            visitor.visit_raw_mut_ptr_slot(cooked_ptr);
            visitor.visit_raw_mut_ptr_slot(raw_ptr);
        }
    });
    TEMPLATE_RAW_MAP.with(|m| {
        let mut map = m.borrow_mut();
        let mut moved = Vec::new();
        for (&cooked_addr, raw_ptr) in map.iter_mut() {
            let mut new_cooked_addr = cooked_addr;
            if visitor.visit_usize_slot(&mut new_cooked_addr) {
                moved.push((cooked_addr, new_cooked_addr));
            }
            visitor.visit_raw_mut_ptr_slot(raw_ptr);
        }
        for (old_addr, new_addr) in moved {
            if let Some(raw_ptr) = map.remove(&old_addr) {
                map.insert(new_addr, raw_ptr);
            }
        }
    });
    scan_array_named_property_roots_mut(visitor);
}

fn barrier_array_named_props(owner: usize, props: &mut [ArrayNamedProperty]) {
    for prop in props.iter_mut() {
        crate::gc::runtime_write_barrier_external_slot(
            owner,
            &mut prop.value as *mut f64 as usize,
            prop.value.to_bits(),
        );
    }
}

fn merge_array_named_props(
    props: &mut HashMap<usize, Vec<ArrayNamedProperty>>,
    owner: usize,
    owner_props: Vec<ArrayNamedProperty>,
) {
    let entry = props.entry(owner).or_default();
    for prop in owner_props {
        if let Some(existing) = entry.iter_mut().find(|existing| existing.name == prop.name) {
            existing.value = prop.value;
        } else {
            entry.push(prop);
        }
    }
    barrier_array_named_props(owner, entry);
}

pub(crate) fn scan_array_named_property_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    ARRAY_NAMED_PROPS.with(|m| {
        let mut props = m.borrow_mut();
        let mut moved = Vec::new();
        for (&owner, owner_props) in props.iter_mut() {
            let mut new_owner = owner;
            if visitor.visit_metadata_usize_slot(&mut new_owner) {
                moved.push((owner, new_owner));
            }
            for prop in owner_props.iter_mut() {
                visitor.visit_nanbox_f64_slot(&mut prop.value);
            }
        }
        for (old_owner, new_owner) in moved {
            if let Some(old_props) = props.remove(&old_owner) {
                merge_array_named_props(&mut props, new_owner, old_props);
            }
        }
    });
}

unsafe fn string_header_as_str<'a>(key: *const crate::StringHeader) -> Option<&'a str> {
    if key.is_null() {
        return None;
    }
    let len = (*key).byte_len as usize;
    let data = (key as *const u8).add(std::mem::size_of::<crate::StringHeader>());
    let bytes = std::slice::from_raw_parts(data, len);
    std::str::from_utf8(bytes).ok()
}

pub(crate) unsafe fn array_named_property_set(
    arr: *mut ArrayHeader,
    key: *const crate::StringHeader,
    value: f64,
) {
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        return;
    }
    let Some(name) = string_header_as_str(key) else {
        return;
    };
    let owner = arr as usize;
    ARRAY_NAMED_PROPS.with(|m| {
        let mut map = m.borrow_mut();
        let props = map.entry(owner).or_default();
        if let Some(prop) = props.iter_mut().find(|prop| prop.name == name) {
            prop.value = value;
        } else {
            props.push(ArrayNamedProperty {
                name: name.to_string(),
                value,
            });
        }
        barrier_array_named_props(owner, props);
    });
}

pub(crate) unsafe fn array_named_property_get_by_name(
    arr: *const ArrayHeader,
    name: &str,
) -> Option<f64> {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return None;
    }
    ARRAY_NAMED_PROPS.with(|m| {
        m.borrow().get(&(arr as usize)).and_then(|props| {
            props
                .iter()
                .find(|prop| prop.name == name)
                .map(|prop| prop.value)
        })
    })
}

pub(crate) unsafe fn array_named_property_get(
    arr: *const ArrayHeader,
    key: *const crate::StringHeader,
) -> Option<f64> {
    let name = string_header_as_str(key)?;
    array_named_property_get_by_name(arr, name)
}

pub(crate) unsafe fn array_named_property_has(
    arr: *const ArrayHeader,
    key: *const crate::StringHeader,
) -> bool {
    let Some(name) = string_header_as_str(key) else {
        return false;
    };
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return false;
    }
    ARRAY_NAMED_PROPS.with(|m| {
        m.borrow()
            .get(&(arr as usize))
            .map(|props| props.iter().any(|prop| prop.name == name))
            .unwrap_or(false)
    })
}

pub(crate) unsafe fn array_named_property_names(
    arr: *const ArrayHeader,
    enumerable_only: bool,
) -> Vec<String> {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return Vec::new();
    }
    let owner = arr as usize;
    ARRAY_NAMED_PROPS.with(|m| {
        m.borrow()
            .get(&owner)
            .map(|props| {
                props
                    .iter()
                    .filter(|prop| {
                        !enumerable_only
                            || crate::object::get_property_attrs(owner, &prop.name)
                                .map(|attrs| attrs.enumerable())
                                .unwrap_or(true)
                    })
                    .map(|prop| prop.name.clone())
                    .collect()
            })
            .unwrap_or_default()
    })
}

pub(crate) unsafe fn array_named_property_delete(
    arr: *const ArrayHeader,
    key: *const crate::StringHeader,
) -> bool {
    let Some(name) = string_header_as_str(key) else {
        return false;
    };
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return false;
    }
    ARRAY_NAMED_PROPS.with(|m| {
        let mut map = m.borrow_mut();
        let Some(props) = map.get_mut(&(arr as usize)) else {
            return false;
        };
        let Some(index) = props.iter().position(|prop| prop.name == name) else {
            return false;
        };
        props.remove(index);
        true
    })
}

#[cfg(test)]
pub(crate) fn test_seed_template_raw_roots(cooked: *mut ArrayHeader, raw: *mut ArrayHeader) {
    TEMPLATE_RAW_MAP.with(|m| {
        let mut m = m.borrow_mut();
        m.clear();
        m.insert(cooked as usize, raw);
    });
}

#[cfg(test)]
pub(crate) fn test_template_raw_roots() -> (usize, usize) {
    TEMPLATE_RAW_MAP.with(|m| {
        let m = m.borrow();
        let Some((&cooked, raw)) = m.iter().next() else {
            return (0, 0);
        };
        (cooked, *raw as usize)
    })
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
pub(crate) fn clean_arr_ptr(arr: *const ArrayHeader) -> *const ArrayHeader {
    // Heap window varies by OS: macOS mimalloc lands in the 3-5 TB range;
    // Android scudo + Linux glibc allocate MUCH lower (often < 1 TB); Windows
    // mimalloc lands well under 1 TB (often in the GB-to-tens-of-GB range).
    // iOS / tvOS / watchOS / visionOS *device* targets use libsystem_malloc
    // (mimalloc is host-side only) and allocate in the same low range —
    // #1136's `for…of` over `split()` reproed empty because the array
    // pointer landed below 2 TB and `clean_arr_ptr` silently null-ed it.
    // Using the macOS-tight 2 TB floor on Android / Windows / iOS-family
    // silently null-s every real array pointer, turning js_array_set_f64
    // into a no-op and — at the read side via js_array_map etc. —
    // returning empty arrays for legitimate inputs (issues #385/#386/#387
    // for non-macOS hosts; #1136 for iOS device).
    //
    // The iOS *simulator* runs on the macOS host's mimalloc and lands in
    // the 3-5 TB range like macOS itself; lowering the floor to 4 KB does
    // not weaken the guard there because the actual liveness check is the
    // GcHeader / obj_type validation downstream.
    #[cfg(any(
        target_os = "android",
        target_os = "linux",
        target_os = "windows",
        target_os = "ios",
        target_os = "tvos",
        target_os = "watchos",
        target_os = "visionos",
    ))]
    const HEAP_MIN: u64 = 0x1000; // 4 KB (classic user-space floor)
    #[cfg(not(any(
        target_os = "android",
        target_os = "linux",
        target_os = "windows",
        target_os = "ios",
        target_os = "tvos",
        target_os = "watchos",
        target_os = "visionos",
    )))]
    const HEAP_MIN: u64 = 0x200_0000_0000; // 2 TB — above observed corrupt handles on macOS
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
    // #5432: reject small-handle ids (fetch/zlib/proxy/common-registry) that
    // reached an array helper as the receiver. On non-macOS hosts HEAP_MIN is
    // 0x1000 — below the handle band — so a handle like a fetch Headers id
    // (0x40000) passes the window check above, and the forwarding-chain /
    // obj_type derefs below (`cleaned - 8`) would read unmapped low memory and
    // SIGSEGV. Magnitude-classify before any deref and null it (safe
    // empty-result no-op), mirroring the addr_class band-map contract.
    if crate::value::addr_class::is_handle_band(cleaned as usize) {
        return std::ptr::null();
    }
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
    // Length/capacity sanity: dense arrays have length <= capacity and
    // length below 100M (800 MB of element payload — well above legitimate
    // large result sets, far below the 775M / 926M patterns we observed
    // when a reused arena slot landed ASCII text at offsets 0/4). Sparse
    // arrays created by far-index writes are the one legal exception:
    // logical length can be huge while dense capacity stays small and the
    // far slots live in ARRAY_NAMED_PROPS.
    unsafe {
        let hdr = &*cleaned;
        if hdr.length > hdr.capacity || hdr.length > 100_000_000 {
            // Allow very large BUFFERS to pass — a postgres frame can
            // be 64MB+ of bytes (capacity in the buffer case) with
            // length up to capacity. Detect registered buffers and
            // wave them through; everything else at this size is
            // almost certainly corrupted.
            let addr = cleaned as usize;
            let sparse_array_shape = if addr >= crate::gc::GC_HEADER_SIZE + 0x1000 {
                let gc_header = (cleaned as *const u8).sub(crate::gc::GC_HEADER_SIZE)
                    as *const crate::gc::GcHeader;
                (*gc_header).obj_type == crate::gc::GC_TYPE_ARRAY
                    && hdr.length > hdr.capacity
                    && hdr.capacity <= 1_000_000
            } else {
                false
            };
            if sparse_array_shape {
                return cleaned;
            }
            if crate::buffer::is_registered_buffer(addr)
                || crate::typedarray::lookup_typed_array_kind(addr).is_some()
            {
                return cleaned;
            }
            return std::ptr::null();
        }
    }
    cleaned
}

#[inline(always)]
pub(crate) fn clean_arr_ptr_mut(arr: *mut ArrayHeader) -> *mut ArrayHeader {
    clean_arr_ptr(arr as *const ArrayHeader) as *mut ArrayHeader
}

/// #5135: detect a Proxy id arriving where an `ArrayHeader` pointer is
/// expected. immer's array drafts are Proxies typed (statically) as plain
/// arrays, so `draft.push(x)` / `draft.length` reach the native array helpers
/// with the masked proxy id instead of a real heap pointer. Deref-ing one as an
/// `ArrayHeader` reads unmapped memory and SIGSEGVs. Callers use this to detect
/// the case and route the operation through the proxy's traps. Returns the
/// re-boxed (`POINTER_TAG`) proxy value when `arr` is a *registered* proxy.
#[inline]
pub(crate) fn array_ptr_as_proxy(arr: *const ArrayHeader) -> Option<f64> {
    let bits = arr as u64;
    let raw = if (bits >> 48) >= 0x7FF8 {
        bits & 0x0000_FFFF_FFFF_FFFF
    } else {
        bits
    };
    if crate::value::addr_class::is_proxy_id_band(raw as usize) {
        const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
        let boxed = f64::from_bits(POINTER_TAG | raw);
        if crate::proxy::js_proxy_is_proxy(boxed) != 0 {
            return Some(boxed);
        }
    }
    None
}

/// Normalize an Array.prototype method receiver into a real ArrayHeader.
///
/// `Array.prototype.<method>.call(arrayLike, ...)` lets a *generic array-like
/// object* — a plain object with a `length` property and indexed keys, e.g.
/// `{length: 3, 0: "a", 1: "b", 2: "c"}` — stand in for a real Array
/// (ECMA-262 §23.1.3, every method's ToObject(this)/LengthOfArrayLike steps).
///
/// The read-only Array methods (`map`/`filter`/`reduce`/`slice`/`indexOf`/…)
/// all start with `clean_arr_ptr(arr)` and then dereference the result as if it
/// were a real ArrayHeader (reading `(*arr).length` + the inline element
/// buffer). When the receiver is a plain object, `clean_arr_ptr` either nulls
/// it (TypeError downstream) or — if the object's first u32s happen to pass the
/// length<=capacity sanity bound — reads the `ObjectHeader` field_count / inline
/// f64 slots as garbage elements (e.g. `8.48e-314`).
///
/// This helper detects the array-like case via the GC header `obj_type`
/// (`GC_TYPE_OBJECT` == plain object) and materializes it into a real array via
/// `js_array_from_arraylike` (which ToLength-coerces `length` and reads indexed
/// keys `"0".."len-1"`). For genuine arrays (`GC_TYPE_ARRAY`), lazy arrays,
/// typed arrays, buffers, and null/garbage it delegates straight to
/// `clean_arr_ptr` — so the real-array hot path pays nothing beyond one
/// already-warm GC-header byte read and a single integer compare.
///
/// Returns a pointer that is safe to dereference as an `ArrayHeader`, or null
/// (preserving the existing empty-result / TypeError-at-call-site behavior).
#[inline(always)]
pub(crate) fn normalize_array_receiver(arr: *const ArrayHeader) -> *const ArrayHeader {
    // Strip a NaN-box tag (if present) to recover the raw heap address so we
    // can probe the GC header. Mirrors the tag-strip in clean_arr_ptr /
    // flat_clone's array-like detection.
    let bits = arr as u64;
    let raw_addr = if (bits >> 48) >= 0x7FF8 {
        (bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else {
        bits as usize
    };
    // Reject the small-handle band (fetch/zlib/proxy/registry ids, #5432)
    // BEFORE any GcHeader deref. A folded `headersHandle.forEach(cb)` can reach
    // here with a fetch Headers handle (0x40000 band) when the static
    // array-method fold mis-claimed the receiver; `0x40000 - 8` is unmapped low
    // memory, so the stale `0x1008` floor used to SIGSEGV (the addr_class.rs
    // band-map documents this exact #4665/#4800/#5432 shape). Treat any
    // handle-band payload as "not an array" and fall through to clean_arr_ptr,
    // which nulls it — a safe empty-result no-op instead of a crash.
    if crate::value::addr_class::is_above_handle_band(raw_addr) {
        // Hot path first: read the GC-header obj_type byte. A genuine Array is
        // GC_TYPE_ARRAY and falls straight through to `clean_arr_ptr` — the
        // only added cost for `[1,2,3].map(...)` etc. is this one byte read and
        // an integer compare (the registry lookups below are reached ONLY for a
        // plain-object receiver, never for real arrays).
        let obj_type = unsafe {
            let hdr = (raw_addr as *const u8).sub(crate::gc::GC_HEADER_SIZE)
                as *const crate::gc::GcHeader;
            (*hdr).obj_type
        };
        // A genuine `Array` is GC_TYPE_ARRAY and skips this whole block (one
        // compare), keeping the hot `[1,2,3].map(...)` path cheap. Everything
        // else — generic array-like objects, typed arrays, buffers — resolves
        // its registry membership once here.
        if obj_type != crate::gc::GC_TYPE_ARRAY {
            let is_typed_array = crate::typedarray::lookup_typed_array_kind(raw_addr).is_some();
            let is_buffer = crate::buffer::is_registered_buffer(raw_addr);
            if obj_type == crate::gc::GC_TYPE_OBJECT && !is_typed_array && !is_buffer {
                // Generic array-like object receiver
                // (`Array.prototype.<m>.call({length, 0:…}, …)`): materialize
                // `length` + indexed keys into a real array and operate on that.
                return unsafe {
                    crate::array::js_array_from_arraylike(
                        raw_addr as *const crate::object::ObjectHeader,
                    )
                } as *const ArrayHeader;
            }
            // #5484: a registered typed array / buffer is a valid receiver
            // regardless of `clean_arr_ptr`'s macOS 2 TB heap-window heuristic.
            // Typed arrays are old-arena allocations (`arena_alloc_gc_old`) that
            // can land BELOW that floor, where `clean_arr_ptr` would null them —
            // yet `clean_ta_ptr` (4 KB floor) accepts the same address, so
            // `.length`/index access worked while `reduce`/`forEach`/`map`/
            // `join`/`indexOf` (which funnel through here) silently saw an empty
            // array. The registry membership IS the liveness check; return the
            // raw address so the caller's typed-array / buffer dispatch fires.
            if is_typed_array || is_buffer {
                return raw_addr as *const ArrayHeader;
            }
        }
    }
    // Real array / lazy array / typed array / null / garbage: existing path.
    clean_arr_ptr(arr)
}

/// Array header - precedes the elements in memory
#[repr(C)]
pub struct ArrayHeader {
    /// Number of elements in the array
    pub length: u32,
    /// Capacity (allocated space for elements)
    pub capacity: u32,
}

#[inline]
pub(crate) fn value_bits_are_numeric(value_bits: u64) -> bool {
    value_bits_to_number(value_bits).is_some()
}

#[inline]
pub(crate) fn value_bits_to_number(value_bits: u64) -> Option<f64> {
    if (value_bits & crate::value::TAG_MASK) == crate::value::INT32_TAG {
        let lower = (value_bits & crate::value::INT32_MASK) as u32;
        // #321/effect-Schema: a class reference shares the INT32_TAG (0x7FFE)
        // NaN-box shape with genuine small integers — `arrays_finds.rs` lowers
        // a `ClassRef` to its registered class id NaN-boxed with INT32_TAG, and
        // downstream property / method / `instanceof` dispatch keys off the
        // surviving 0x7FFE tag. A class ref is NOT a numeric array element, so
        // treating it as the integer `class_id` here let the raw-f64 numeric
        // layout canonicalize the slot to `class_id.to_bits()`, stripping the
        // tag (`canonicalize_array_numeric_store_bits` /
        // `note_array_numeric_index_write`). That turned a class value passed
        // through a rest parameter — `Union(...members)` in effect's Schema,
        // whose `members.map((m) => m.ast)` then dereferenced the bare number
        // as an object — into a SIGSEGV. Reporting class refs as non-numeric
        // keeps such arrays off the raw-f64 fast path and preserves the tag.
        // A genuine integer whose value coincides with a registered class id
        // only loses the raw-f64 *optimization* (it is still a valid number
        // when read back), so correctness is never at stake.
        if crate::object::is_class_id_registered(lower) {
            return None;
        }
        return Some((lower as i32) as f64);
    }
    let upper = value_bits >> 48;
    if (0x7FF9..=0x7FFF).contains(&upper) {
        return None;
    }
    Some(canonical_raw_f64(f64::from_bits(value_bits)))
}

#[no_mangle]
pub extern "C" fn js_array_numeric_value_to_raw_f64(value: f64) -> f64 {
    value_bits_to_number(value.to_bits()).unwrap_or(f64::NAN)
}

#[inline]
fn canonical_raw_f64(value: f64) -> f64 {
    if value.is_nan() {
        f64::NAN
    } else {
        value
    }
}

#[inline]
pub(crate) unsafe fn canonicalize_array_numeric_store_bits(
    arr: *mut ArrayHeader,
    value_bits: u64,
) -> u64 {
    if array_numeric_layout(arr) == Some(NumericArrayLayout::RawF64) {
        if let Some(number) = value_bits_to_number(value_bits) {
            return number.to_bits();
        }
    }
    value_bits
}

#[inline]
pub(crate) unsafe fn canonicalize_array_numeric_store_value(
    arr: *mut ArrayHeader,
    value: f64,
) -> f64 {
    f64::from_bits(canonicalize_array_numeric_store_bits(arr, value.to_bits()))
}

#[inline]
unsafe fn array_slot_bits(arr: *const ArrayHeader, index: usize) -> u64 {
    let slot = (arr as *const u8).add(std::mem::size_of::<ArrayHeader>()) as *const u64;
    *slot.add(index)
}

#[inline]
unsafe fn array_slots_are_numeric(arr: *const ArrayHeader) -> bool {
    if arr.is_null() {
        return false;
    }
    let length = (*arr).length as usize;
    let capacity = (*arr).capacity as usize;
    if length > capacity || length > 16_000_000 {
        return false;
    }
    for i in 0..length {
        if value_bits_to_number(array_slot_bits(arr, i)).is_none() {
            return false;
        }
    }
    true
}

#[inline]
unsafe fn array_gc_header(arr: *const ArrayHeader) -> Option<*mut crate::gc::GcHeader> {
    if arr.is_null() || (arr as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    let header = (arr as *mut u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
    if (*header).obj_type != crate::gc::GC_TYPE_ARRAY {
        return None;
    }
    Some(header)
}

#[inline]
unsafe fn array_has_raw_f64_layout_flag(arr: *const ArrayHeader) -> bool {
    array_gc_header(arr)
        .is_some_and(|header| (*header)._reserved & crate::gc::GC_ARRAY_RAW_F64_LAYOUT != 0)
}

#[inline]
unsafe fn set_array_raw_f64_layout_flag(arr: *const ArrayHeader) {
    if let Some(header) = array_gc_header(arr) {
        (*header)._reserved |= crate::gc::GC_ARRAY_RAW_F64_LAYOUT;
    }
}

#[inline]
unsafe fn clear_array_raw_f64_layout_flag(arr: *const ArrayHeader) {
    if let Some(header) = array_gc_header(arr) {
        let had_raw_layout = (*header)._reserved & crate::gc::GC_ARRAY_RAW_F64_LAYOUT != 0;
        (*header)._reserved &= !crate::gc::GC_ARRAY_RAW_F64_LAYOUT;
        if had_raw_layout {
            crate::typed_feedback::invalidate_representation_change(arr as usize);
        }
    }
}

pub(crate) unsafe fn mark_array_as_arguments_object(arr: *const ArrayHeader) {
    if let Some(header) = array_gc_header(arr) {
        (*header)._reserved |= crate::gc::GC_ARRAY_ARGUMENTS_OBJECT;
    }
}

#[no_mangle]
pub extern "C" fn js_array_mark_arguments_object(arr: *mut ArrayHeader) -> *mut ArrayHeader {
    unsafe {
        mark_array_as_arguments_object(arr as *const ArrayHeader);
    }
    arr
}

pub(crate) unsafe fn array_has_arguments_object_flag(arr: *const ArrayHeader) -> bool {
    array_gc_header(arr)
        .is_some_and(|header| (*header)._reserved & crate::gc::GC_ARRAY_ARGUMENTS_OBJECT != 0)
}

unsafe fn rebuild_array_numeric_raw_f64(arr: *mut ArrayHeader) -> bool {
    if arr.is_null() {
        return false;
    }
    let length = (*arr).length as usize;
    let capacity = (*arr).capacity as usize;
    if length > capacity || length > 16_000_000 {
        clear_array_numeric_layout(arr);
        return false;
    }

    let elements = array_elements_ptr(arr);
    for i in 0..length {
        let slot_bits = array_slot_bits(arr, i);
        let Some(number) = value_bits_to_number(slot_bits) else {
            clear_array_numeric_layout(arr);
            return false;
        };
        // GC_STORE_AUDIT(POINTER_FREE): raw-f64 layout rewrite stores numeric payloads only.
        std::ptr::write(elements.add(i) as *mut f64, number);
    }

    set_array_raw_f64_layout_flag(arr);
    crate::gc::layout_init_pointer_free(arr as *mut u8);
    true
}

#[inline]
pub(crate) unsafe fn set_array_numeric_layout(arr: *mut ArrayHeader, layout: NumericArrayLayout) {
    if arr.is_null() {
        return;
    }
    match layout {
        NumericArrayLayout::RawF64 => set_array_raw_f64_layout_flag(arr),
    }
    crate::gc::layout_init_pointer_free(arr as *mut u8);
}

#[inline]
pub(crate) unsafe fn clear_array_numeric_layout(arr: *const ArrayHeader) {
    if arr.is_null() {
        return;
    }
    clear_array_raw_f64_layout_flag(arr);
}

#[inline]
pub(crate) fn clear_array_numeric_layout_ptr(user_ptr: usize) {
    if user_ptr == 0 {
        return;
    }
    unsafe {
        clear_array_raw_f64_layout_flag(user_ptr as *const ArrayHeader);
    }
}

#[inline]
pub(crate) fn transfer_array_numeric_layout(old_user: usize, new_user: usize) {
    if old_user == 0 || new_user == 0 || old_user == new_user {
        return;
    }
    unsafe {
        if array_has_raw_f64_layout_flag(old_user as *const ArrayHeader) {
            set_array_raw_f64_layout_flag(new_user as *const ArrayHeader);
        } else {
            clear_array_raw_f64_layout_flag(new_user as *const ArrayHeader);
        }
    }
}

#[inline]
pub(crate) unsafe fn array_numeric_layout(arr: *const ArrayHeader) -> Option<NumericArrayLayout> {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return None;
    }
    array_has_raw_f64_layout_flag(arr).then_some(NumericArrayLayout::RawF64)
}

#[inline]
pub(crate) unsafe fn note_array_numeric_write(arr: *mut ArrayHeader, value_bits: u64) {
    if !value_bits_are_numeric(value_bits) {
        clear_array_numeric_layout(arr);
    }
}

#[inline]
pub(crate) unsafe fn note_array_numeric_index_write(
    arr: *mut ArrayHeader,
    index: usize,
    value_bits: u64,
) -> u64 {
    let Some(number) = value_bits_to_number(value_bits) else {
        clear_array_numeric_layout(arr);
        return value_bits;
    };
    if array_has_raw_f64_layout_flag(arr) && index < (*arr).length as usize {
        let elements = array_elements_ptr(arr) as *mut f64;
        // GC_STORE_AUDIT(POINTER_FREE): raw-f64 numeric slot update cannot contain a heap pointer.
        std::ptr::write(elements.add(index), number);
        return number.to_bits();
    }
    value_bits
}

#[inline]
pub(crate) unsafe fn ensure_array_numeric_raw_f64(arr: *mut ArrayHeader) -> bool {
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        return false;
    }
    let length = (*arr).length as usize;
    let capacity = (*arr).capacity as usize;
    if length > capacity || length > 16_000_000 {
        clear_array_numeric_layout(arr);
        return false;
    }
    if array_has_raw_f64_layout_flag(arr) {
        return true;
    }
    rebuild_array_numeric_raw_f64(arr)
}

#[inline]
pub(crate) unsafe fn array_numeric_raw_f64_get(arr: *mut ArrayHeader, index: u32) -> Option<f64> {
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        return None;
    }
    // An index converted to an accessor (or given custom attrs) via
    // `Object.defineProperty` must dispatch through the slow path.
    if array_object_flags(arr) & crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS != 0 {
        return None;
    }
    if index >= (*arr).length {
        return None;
    }
    if !ensure_array_numeric_raw_f64(arr) {
        return None;
    }
    let elements = array_elements_ptr(arr) as *const f64;
    Some(*elements.add(index as usize))
}

#[inline]
pub(crate) unsafe fn array_numeric_raw_f64_set_inbounds(
    arr: *mut ArrayHeader,
    index: u32,
    value: f64,
) -> bool {
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() || index >= (*arr).length {
        return false;
    }
    // Accessor setters / non-writable attrs on indices need the slow path.
    if array_object_flags(arr) & crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS != 0 {
        return false;
    }
    let original_bits = value.to_bits();
    let value_bits = canonicalize_array_numeric_store_bits(arr, original_bits);
    let value = f64::from_bits(value_bits);
    if !ensure_array_numeric_raw_f64(arr) {
        return false;
    }
    let elements_ptr = array_elements_ptr(arr) as *mut f64;
    // GC_STORE_AUDIT(POINTER_FREE): raw-f64 numeric field store is layout-noted below.
    std::ptr::write(elements_ptr.add(index as usize), value);
    note_array_numeric_index_write(arr, index as usize, value_bits);
    crate::gc::layout_note_slot(arr as usize, index as usize, value_bits);
    value_bits_are_numeric(original_bits)
}

#[inline]
pub(crate) unsafe fn array_numeric_raw_f64_push_inbounds(
    arr: *mut ArrayHeader,
    value: f64,
) -> bool {
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() || !ensure_array_numeric_raw_f64(arr) {
        return false;
    }
    let length = (*arr).length;
    let capacity = (*arr).capacity;
    if length >= capacity || length > 16_000_000 || capacity > 16_000_000 {
        return false;
    }

    let Some(number) = value_bits_to_number(value.to_bits()) else {
        clear_array_numeric_layout(arr);
        return false;
    };
    let elements_ptr = array_elements_ptr(arr) as *mut f64;
    // GC_STORE_AUDIT(POINTER_FREE): raw-f64 push stores numeric payloads only.
    std::ptr::write(elements_ptr.add(length as usize), number);
    crate::gc::layout_note_slot(arr as usize, length as usize, number.to_bits());
    (*arr).length = length + 1;
    true
}

#[inline]
pub(crate) unsafe fn refresh_array_numeric_layout(arr: *mut ArrayHeader) {
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        return;
    }
    if array_slots_are_numeric(arr) {
        rebuild_array_numeric_raw_f64(arr);
    } else {
        clear_array_numeric_layout(arr);
    }
}

#[no_mangle]
pub extern "C" fn js_array_mark_numeric_f64_layout(arr: *mut ArrayHeader) -> i32 {
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        return 0;
    }
    unsafe {
        if !array_slots_are_numeric(arr) {
            clear_array_numeric_layout(arr);
            return 0;
        }
        rebuild_array_numeric_raw_f64(arr);
    }
    1
}

#[no_mangle]
pub extern "C" fn js_array_clear_numeric_layout(arr: *mut ArrayHeader) {
    let arr = clean_arr_ptr_mut(arr);
    unsafe {
        clear_array_numeric_layout(arr);
    }
}

#[no_mangle]
pub extern "C" fn js_array_note_numeric_write(arr: *mut ArrayHeader, value_bits: u64) {
    let arr = clean_arr_ptr_mut(arr);
    unsafe {
        note_array_numeric_write(arr, value_bits);
    }
}

#[no_mangle]
pub extern "C" fn js_array_is_numeric_f64_layout(arr: *const ArrayHeader) -> i32 {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return 0;
    }
    unsafe {
        if array_numeric_layout(arr) == Some(NumericArrayLayout::RawF64) {
            return 1;
        }
        if array_slots_are_numeric(arr) {
            rebuild_array_numeric_raw_f64(arr as *mut ArrayHeader);
            return 1;
        }
        clear_array_numeric_layout(arr);
    }
    0
}

// These raw numeric-array helpers are called from generated code, so release/LTO
// builds may otherwise internalize and strip the `#[no_mangle]` exports.
#[used]
static KEEP_JS_ARRAY_NUMERIC_VALUE_TO_RAW_F64: extern "C" fn(f64) -> f64 =
    js_array_numeric_value_to_raw_f64;
#[used]
static KEEP_JS_ARRAY_MARK_NUMERIC_F64_LAYOUT: extern "C" fn(*mut ArrayHeader) -> i32 =
    js_array_mark_numeric_f64_layout;
#[used]
static KEEP_JS_ARRAY_CLEAR_NUMERIC_LAYOUT: extern "C" fn(*mut ArrayHeader) =
    js_array_clear_numeric_layout;
#[used]
static KEEP_JS_ARRAY_NOTE_NUMERIC_WRITE: extern "C" fn(*mut ArrayHeader, u64) =
    js_array_note_numeric_write;
#[used]
static KEEP_JS_ARRAY_IS_NUMERIC_F64_LAYOUT: extern "C" fn(*const ArrayHeader) -> i32 =
    js_array_is_numeric_f64_layout;

/// Calculate the byte size for an array with N elements capacity
#[inline]
pub(crate) fn array_byte_size(capacity: usize) -> usize {
    std::mem::size_of::<ArrayHeader>() + capacity * std::mem::size_of::<f64>()
}

#[inline]
unsafe fn array_elements_ptr(arr: *mut ArrayHeader) -> *mut u64 {
    (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut u64
}

pub(crate) unsafe fn gc_element_slot_range(
    arr: *mut ArrayHeader,
) -> Option<crate::gc::HeapSlotRange> {
    if arr.is_null() {
        return None;
    }
    let length = (*arr).length as usize;
    let capacity = (*arr).capacity as usize;
    if length > capacity || length > 16_000_000 {
        return None;
    }
    Some(crate::gc::HeapSlotRange::new(
        array_elements_ptr(arr),
        length,
    ))
}

#[inline]
pub(crate) unsafe fn note_array_slot(arr: *mut ArrayHeader, index: usize, value_bits: u64) {
    let value_bits = canonicalize_array_numeric_store_bits(arr, value_bits);
    // GC_STORE_AUDIT(BARRIERED): shared helper notes layout and emits the array slot barrier below.
    std::ptr::write(array_elements_ptr(arr).add(index), value_bits);
    note_array_numeric_index_write(arr, index, value_bits);
    crate::gc::layout_note_slot(arr as usize, index, value_bits);
    let slot = array_elements_ptr(arr).add(index) as usize;
    crate::gc::runtime_write_barrier_slot(arr as usize, slot, value_bits);
}

#[inline]
pub(crate) unsafe fn note_array_slot_layout_only(
    arr: *mut ArrayHeader,
    index: usize,
    value_bits: u64,
) {
    let value_bits = canonicalize_array_numeric_store_bits(arr, value_bits);
    // GC_STORE_AUDIT(INIT): layout-only helper is restricted to fresh/suppressed caller sites.
    std::ptr::write(array_elements_ptr(arr).add(index), value_bits);
    note_array_numeric_index_write(arr, index, value_bits);
    crate::gc::layout_note_slot(arr as usize, index, value_bits);
}

#[inline]
pub(crate) unsafe fn store_array_slot(arr: *mut ArrayHeader, index: usize, value_bits: u64) {
    let value_bits = canonicalize_array_numeric_store_bits(arr, value_bits);
    note_array_numeric_index_write(arr, index, value_bits);
    let slot = array_elements_ptr(arr).add(index) as usize;
    let stored_bits = if array_has_raw_f64_layout_flag(arr) {
        match value_bits_to_number(value_bits) {
            Some(number) => number.to_bits(),
            None => {
                clear_array_numeric_layout(arr);
                value_bits
            }
        }
    } else {
        value_bits
    };
    crate::gc::runtime_store_jsvalue_slot(arr as usize, slot, index, stored_bits);
}

#[inline]
pub(crate) unsafe fn rebuild_array_layout(arr: *mut ArrayHeader) {
    if arr.is_null() {
        return;
    }
    let length = (*arr).length as usize;
    let capacity = (*arr).capacity as usize;
    if length > capacity || length > 16_000_000 {
        clear_array_numeric_layout(arr);
        crate::gc::layout_mark_unknown(arr as *mut u8);
        return;
    }
    crate::gc::layout_rebuild_from_slots(arr as *mut u8, array_elements_ptr(arr), length);
    refresh_array_numeric_layout(arr);
    if crate::arena::pointer_in_old_gen(arr as usize) {
        let slots = array_elements_ptr(arr);
        for i in 0..length {
            let slot = slots.add(i);
            crate::gc::runtime_write_barrier_slot(arr as usize, slot as usize, *slot);
        }
    }
}

#[inline]
pub(crate) unsafe fn rebuild_array_layout_exact(arr: *mut ArrayHeader) {
    if arr.is_null() {
        return;
    }
    let length = (*arr).length as usize;
    let capacity = (*arr).capacity as usize;
    if length > capacity || length > 16_000_000 {
        clear_array_numeric_layout(arr);
        crate::gc::layout_mark_unknown(arr as *mut u8);
        return;
    }
    crate::gc::layout_rebuild_exact_from_slots(arr as *mut u8, array_elements_ptr(arr), length);
    refresh_array_numeric_layout(arr);
    if crate::arena::pointer_in_old_gen(arr as usize) {
        let slots = array_elements_ptr(arr);
        for i in 0..length {
            let slot = slots.add(i);
            crate::gc::runtime_write_barrier_slot(arr as usize, slot as usize, *slot);
        }
    }
}

#[inline]
pub(crate) unsafe fn replay_array_growth_write_barriers(arr: *mut ArrayHeader) {
    if arr.is_null() || !crate::arena::pointer_in_old_gen(arr as usize) {
        return;
    }

    let length = (*arr).length as usize;
    if length == 0 || length > 16_000_000 {
        return;
    }

    let slots = array_elements_ptr(arr);
    if crate::gc::layout_visit_pointer_slots_for_user(arr as usize, length, |index| {
        let slot = slots.add(index);
        crate::gc::runtime_write_barrier_slot(arr as usize, slot as usize, *slot);
    }) {
        return;
    }

    for i in 0..length {
        let slot = slots.add(i);
        crate::gc::runtime_write_barrier_slot(arr as usize, slot as usize, *slot);
    }
}

#[inline]
pub(crate) unsafe fn mark_array_layout_unknown(arr: *mut ArrayHeader) {
    clear_array_numeric_layout(arr);
    crate::gc::layout_mark_unknown(arr as *mut u8);
}

/// Minimum initial capacity for arrays to reduce reallocations
pub(crate) const MIN_ARRAY_CAPACITY: u32 = 16;
