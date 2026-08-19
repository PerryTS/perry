//! ArrayHeader struct, pointer-cleaning / GC-layout helpers, and the
//! tagged-template `.raw` side-table. Every other `array::*` sub-module
//! pulls these basics in via `use super::*;`.

use std::cell::RefCell;
use std::collections::HashMap;

crate::perry_thread_local! {
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
    /// Address-keyed `PtrHashMap` (#6386): probed on every exec-array
    /// decoration (regex match/exec) and every `ArraySpeciesCreate`
    /// own-`constructor` check; SipHash dominated those probes.
    static ARRAY_NAMED_PROPS: RefCell<crate::fast_hash::PtrHashMap<usize, Vec<ArrayNamedProperty>>> =
        RefCell::new(crate::fast_hash::new_ptr_hash_map());
}

#[derive(Clone)]
struct ArrayNamedProperty {
    // `Cow` so the per-match exec-array keys (`index`/`input`/`groups`,
    // #6386) borrow statically instead of allocating three `String`s per
    // regex match; dynamically named expandos still own their key.
    name: std::borrow::Cow<'static, str>,
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

/// Read the flag word of an array that [`clean_arr_ptr`] already resolved.
///
/// Hot operations such as `push` need the frozen, descriptor, numeric-layout,
/// and extensibility bits together. Re-entering [`array_object_flags`] for each
/// question repeats allocator ownership classification even though the first
/// clean already proved the receiver is a live, non-forwarded GC array.
///
/// # Safety
///
/// `arr` must be the non-null result of [`clean_arr_ptr`] or
/// [`clean_arr_ptr_mut`] with no intervening allocation or safepoint.
#[inline(always)]
pub(crate) unsafe fn array_object_flags_resolved(arr: *const ArrayHeader) -> u16 {
    debug_assert!(!arr.is_null());
    let gc_header = (arr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    debug_assert_eq!((*gc_header).obj_type, crate::gc::GC_TYPE_ARRAY);
    (*gc_header)._reserved
}

/// The `obj_type` and flag word of the `GcHeader` that precedes `arr`, read
/// once, for a receiver [`clean_arr_ptr`] has already resolved. `(0, 0)` when
/// `arr` is too low to carry a header — `0` is not a legal `obj_type`, so it
/// reads as "unknown" at every call site.
///
/// A non-zero tag is NOT proof that a real header exists — but the reason is
/// narrower than this comment used to claim, and getting it wrong in either
/// direction has cost this file a recurring bug family (#8137/#8138, swept in
/// `gc-handoff/ARRAY-SWEEP-NOTES.md`).
///
/// `Buffer` and `TypedArray` receivers come in BOTH backings:
///
/// * **arena-backed** — `buffer/header.rs`'s `arena_alloc_gc_old(…,
///   GC_TYPE_BUFFER)` and `typedarray/mod.rs`'s `GC_TYPE_TYPED_ARRAY` site.
///   These carry a genuine `GcHeader` with a correct `obj_type`; they are
///   pinned rather than movable, which is a different property from being
///   untracked. This is the population #8041 started nulling.
/// * **external** — `EXTERNAL_BUFFER_REGISTRY` /
///   `EXTERNAL_UINT8ARRAY_REGISTRY` addresses, plus
///   `shared_sab::alloc_shared_sab`'s `alloc_zeroed`. For these the eight
///   bytes below the payload really are allocator bookkeeping and can read
///   as any value.
///
/// So the tag is authoritative only once the address is known to be
/// arena-backed. `typedarray::arena_payload_has_gc_type` is the predicate
/// that does it properly: it range-checks, rejects `HeapSpace::Unknown` for
/// the HEADER address specifically, and validates via `gc_type_info` before
/// trusting the byte. Do not open-code a floor instead. Use a bare tag read
/// only where a wrong tag is harmless:
///
/// * to *skip* a registry probe whose answer for that receiver would have been
///   `false` anyway — the caller must already have routed real buffers and
///   typed arrays elsewhere; or
/// * as the `GC_TYPE_ARRAY` test [`array_object_flags`] already performs on
///   this very byte, where those bookkeeping bytes are allowed to be wrong
///   today.
///
/// What makes the tag *authoritative* for the collection receivers is that
/// every GC allocation carries a header, `Map` and `Set` included:
/// `js_map_alloc` / `js_set_alloc` stamp `GC_TYPE_MAP` / `GC_TYPE_SET` through
/// `arena_alloc_gc`, and that is the single registration site for each. Several
/// comments in the tree still say Map/Set headers come from a bare `alloc()`
/// with no `GcHeader` and that only the registry can classify them; that
/// stopped being true when they moved into the managed arena.
#[inline]
pub(crate) fn array_receiver_gc_tag(arr: *const ArrayHeader) -> (u8, u16) {
    // `try_read_gc_header` rather than this file's usual
    // `>= GC_HEADER_SIZE + 0x1000` floor: that floor sits BELOW the handle
    // band, and `js_array_length` reaches here before its proxy/handle
    // receivers have been routed. The canonical predicate rejects the bands
    // without touching memory, and rejecting an address the old floor would
    // have read costs nothing — a handle is not a Map either way.
    match unsafe { crate::value::addr_class::try_read_gc_header(arr as usize) } {
        Some(header) => (header.obj_type, header._reserved),
        None => (0, 0),
    }
}

/// [`array_object_flags`] answered from a tag [`array_receiver_gc_tag`]
/// already read, for a receiver `clean_arr_ptr` already resolved.
#[inline]
pub(crate) fn array_object_flags_from_tag(tag: (u8, u16)) -> u16 {
    if tag.0 == crate::gc::GC_TYPE_ARRAY {
        tag.1
    } else {
        0
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

#[cfg(feature = "keepalive-anchors")]
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
    props: &mut crate::fast_hash::PtrHashMap<usize, Vec<ArrayNamedProperty>>,
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

/// Remove named-property entries whose array owners are provably dead under
/// the centralized collection-specific liveness policy.
pub(crate) fn prune_dead_array_named_property_owners(is_dead_owner: &dyn Fn(usize) -> bool) {
    ARRAY_NAMED_PROPS.with(|m| {
        m.borrow_mut().retain(|owner, _| !is_dead_owner(*owner));
    });
}

#[cfg(test)]
pub(crate) fn test_array_named_property_owner_exists(owner: usize) -> bool {
    ARRAY_NAMED_PROPS.with(|m| m.borrow().contains_key(&owner))
}

#[cfg(test)]
pub(crate) fn test_clear_array_named_property_roots() {
    ARRAY_NAMED_PROPS.with(|m| m.borrow_mut().clear());
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
                name: std::borrow::Cow::Owned(name.to_string()),
                value,
            });
        }
        barrier_array_named_props(owner, props);
    });
}

/// Batched named-prop install for a FRESHLY built array (#6386): one
/// side-table probe for all entries and `&str` keys (no key `StringHeader`
/// allocations). Callers must guarantee the array was allocated in the same
/// runtime helper invocation — a fresh array has no accessor descriptors, no
/// property attributes, and no freeze/seal state, which is what makes
/// bypassing `js_array_set_string_key`'s guard ladder sound. Keys must not be
/// numeric index strings or `"length"` (those live in element storage /
/// the header, not this side table).
#[cfg(feature = "regex-engine")]
pub(crate) unsafe fn array_named_props_install_fresh(
    arr: *mut ArrayHeader,
    entries: &[(&'static str, f64)],
) {
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        return;
    }
    let owner = arr as usize;
    ARRAY_NAMED_PROPS.with(|m| {
        let mut map = m.borrow_mut();
        let props = map.entry(owner).or_default();
        for (name, value) in entries {
            if let Some(prop) = props.iter_mut().find(|prop| prop.name == *name) {
                prop.value = *value;
            } else {
                props.push(ArrayNamedProperty {
                    name: std::borrow::Cow::Borrowed(*name),
                    value: *value,
                });
            }
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
                    .map(|prop| prop.name.to_string())
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
/// Issue #73/#8035 follow-up: address magnitude is not proof of ownership.
/// Corrupted NaN-box payloads can land in a plausible heap window, while real
/// macOS mimalloc arena allocations can land below the former 2 TiB floor.
/// Registry-handle bands are rejected first; GC headers are read only after
/// arena page metadata or the malloc registry proves ownership.
///
/// v0.5.85 follow-up: also validate the GC header byte + length/capacity
/// sanity. A pointer that passes the range check but points into the
/// middle of another allocation (post-GC memory reuse overlaid with
/// e.g. decoded PostgreSQL text column data) reads garbage length
/// values — witnessed `len=775370038 cap=926234674` (both the ASCII
/// bytes of `"6+2.2017"`) flowing through `js_array_slice` and
/// triggering 22GB-wide memcpy segfaults. The post-check therefore requires
/// `GC_TYPE_ARRAY` and validates length/capacity before any element access
/// (with the registered Buffer/TypedArray and sparse-array exceptions below).
#[inline(always)]
pub(crate) fn clean_arr_ptr(arr: *const ArrayHeader) -> *const ArrayHeader {
    let bits = arr as u64;
    let top16 = bits >> 48;
    let cleaned = if top16 >= 0x7FF8 {
        if top16 == 0x7FFC || (bits & 0x0000_FFFF_FFFF_FFFF) == 0 {
            return std::ptr::null();
        }
        let cleaned_bits = bits & 0x0000_FFFF_FFFF_FFFF;
        cleaned_bits as *const ArrayHeader
    } else {
        arr
    };
    // Preserve the permissive window needed by registered Buffer/TypedArray
    // receivers, but centralize its platform policy in addr_class. Actual GC
    // header reads below require allocator ownership as well.
    if !crate::value::addr_class::is_plausible_heap_addr(cleaned as usize) {
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
    let mut tracked_header =
        unsafe { crate::value::addr_class::try_read_tracked_gc_header(cleaned as usize) };
    unsafe {
        let mut steps = 0u32;
        while let Some(gc_header) = tracked_header {
            let gc_header = gc_header.as_ptr();
            if (*gc_header).obj_type != crate::gc::GC_TYPE_ARRAY
                || (*gc_header).gc_flags & crate::gc::GC_FLAG_FORWARDED == 0
            {
                break;
            }
            let new_user = crate::gc::forwarding_address(gc_header) as usize;
            let Some(target_header) =
                crate::value::addr_class::try_read_tracked_gc_header(new_user)
            else {
                return std::ptr::null();
            };
            if (*target_header.as_ptr()).obj_type != crate::gc::GC_TYPE_ARRAY {
                return std::ptr::null();
            }
            cleaned = new_user as *const ArrayHeader;
            tracked_header = Some(target_header);
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
    let addr = cleaned as usize;
    let tracked_obj_type =
        tracked_header.map(|gc_header| unsafe { (*gc_header.as_ptr()).obj_type });
    if let Some(obj_type) = tracked_obj_type {
        if obj_type == crate::gc::GC_TYPE_LAZY_ARRAY {
            unsafe {
                let lazy = cleaned as *mut crate::json_tape::LazyArrayHeader;
                if (*lazy).magic == crate::json_tape::LAZY_ARRAY_MAGIC {
                    let materialized = crate::json_tape::force_materialize_lazy(lazy);
                    return materialized as *const ArrayHeader;
                }
            }
        }
        // A declared TypeScript type is a hint, never a layout fact. Reject
        // every tracked non-array at this shared funnel before treating its
        // payload as an ArrayHeader (#7574).
        if obj_type != crate::gc::GC_TYPE_ARRAY {
            return std::ptr::null();
        }
    } else if !crate::buffer::is_registered_buffer(addr)
        && crate::typedarray::lookup_typed_array_kind(addr).is_none()
    {
        // Handles, synthetic pointers, and unrelated allocations must be
        // rejected before any GcHeader or ArrayHeader dereference. Registered
        // Buffer/TypedArray receivers intentionally use the compatible
        // length/capacity prefix and carry no GcHeader.
        return std::ptr::null();
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
            let sparse_array_shape = tracked_obj_type == Some(crate::gc::GC_TYPE_ARRAY)
                && hdr.length > hdr.capacity
                && hdr.capacity <= 1_000_000;
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

/// Resolve a receiver a plain-array helper was handed but that is really a
/// registered %TypedArray%, so the helper can delegate to its element-typed
/// `js_typed_array_*` twin.
///
/// **Ask this BEFORE `clean_arr_ptr`, never after.** Codegen routes
/// statically-typed typed-array receivers through the generic `js_array_*`
/// helpers on purpose (#3148 / #654 — perry-codegen `is_array_expr` answers
/// `true` for `Int32Array` &co.) on the contract that each helper
/// re-dispatches on `lookup_typed_array_kind`. But `clean_arr_ptr` *rejects*
/// those receivers, and must: since #7574 it returns null for every tracked
/// non-`GC_TYPE_ARRAY` object, because a `TypedArrayHeader`'s raw per-kind
/// storage is not boxed-f64 `ArrayHeader` slots. Since the 2026-07-09
/// typed-array audit gave every typed array a real `GC_TYPE_TYPED_ARRAY`
/// header, that rejection fires for all of them — so a delegation written
/// after the clean is unreachable code, and its helper silently returns the
/// receiver unmutated. That is exactly how `fill`/`fill_range`/`reverse`/
/// `copyWithin` became no-ops with no error and no diagnostic (#2879).
///
/// Strips the NaN-box tag itself (callers may hold a `POINTER_TAG`-boxed
/// value) and never dereferences the address: the side-table probe is the
/// whole test, which also keeps the header-less legacy shapes safe.
#[inline]
pub(crate) fn typed_array_receiver(
    arr: *mut ArrayHeader,
) -> Option<*mut crate::typedarray::TypedArrayHeader> {
    let addr = array_receiver_addr(arr);
    if addr == 0 {
        return None;
    }
    crate::typedarray::lookup_typed_array_kind(addr)
        .map(|_| addr as *mut crate::typedarray::TypedArrayHeader)
}

/// #8096: resolve a receiver that is a registered `Buffer` / `Uint8Array`
/// into a fresh `KIND_UINT8` %TypedArray% COPY, so an `Array.prototype`
/// helper can delegate to a `js_typed_array_*` twin that only accepts a
/// `TypedArrayHeader`.
///
/// [`typed_array_receiver`] does not answer for the most common typed array
/// in the language. Perry's `new Uint8Array([…])` returns a `BufferHeader`
/// (`buffer::js_uint8array_new`), registered as a BUFFER and marked
/// `mark_as_uint8array` — it is not in the typed-array registry at all. The
/// receiver is still a tracked non-`GC_TYPE_ARRAY` allocation, so
/// `clean_arr_ptr` rejects it exactly as it rejects a real
/// `GC_TYPE_TYPED_ARRAY`, and the caller answers an EMPTY plain array.
///
/// Most `Array.prototype` entry points never see that: `sort` / `with` /
/// `reverse` / `fill` on a Buffer-backed `Uint8Array` resolve through the
/// dynamic method dispatcher, and `copyWithin` grew its own Buffer arm in
/// #8090. `toReversed` and `toSorted` fold unconditionally in HIR
/// (`lower/expr_call/local_array_methods.rs` — no receiver-type guard), and
/// the dynamic tower's own `toReversed` / `toSorted` arms
/// (`object/native_call_method/handle_methods.rs`) call straight back into
/// these same helpers, so those two were wrong on EVERY dispatch path.
///
/// **Only sound for the IMMUTABLE methods.** The answer is a copy, so an
/// in-place mutator delegating to it would sort/reverse the copy and leave
/// the receiver untouched — a different wrong answer. `toReversed`,
/// `toSorted` and `with` all return a new collection, which is why they can
/// use it; `js_array_sort_*` deliberately does not.
///
/// `None` for an `ArrayBuffer` / `SharedArrayBuffer` / `DataView` receiver:
/// none of those has `%TypedArray%.prototype`, so node throws
/// `TypeError: … is not a function` rather than answering elements.
#[inline]
pub(crate) fn buffer_receiver_as_uint8_typed_array(
    arr: *mut ArrayHeader,
) -> Option<*mut crate::typedarray::TypedArrayHeader> {
    let addr = array_receiver_addr(arr);
    if addr == 0
        || !crate::buffer::is_registered_buffer(addr)
        || crate::buffer::is_any_array_buffer(addr)
        || crate::buffer::is_data_view(addr)
    {
        return None;
    }
    // Copy the bytes out BEFORE allocating: `typed_array_alloc` can collect,
    // and a raw payload pointer read across it is exactly the borrowed-heap-
    // slice shape rooting cannot fix.
    let buf = addr as *const crate::buffer::BufferHeader;
    let bytes: Vec<u8> = unsafe {
        let len = (*buf).length as usize;
        if len == 0 {
            Vec::new()
        } else {
            std::slice::from_raw_parts(crate::buffer::buffer_data(buf), len).to_vec()
        }
    };
    let ta =
        crate::typedarray::typed_array_alloc(crate::typedarray::KIND_UINT8, bytes.len() as u32);
    for (i, byte) in bytes.iter().enumerate() {
        crate::typedarray::js_typed_array_set(ta, i as i32, f64::from(*byte));
    }
    Some(ta)
}

/// The de-NaN-boxed address of an `Array.prototype` receiver, for side-table
/// probes only. Says nothing about what lives there — never dereference it
/// without one of the registry answers (`typed_array_receiver`,
/// `buffer::is_registered_buffer`) or a `clean_arr_ptr` round trip.
#[inline]
pub(crate) fn array_receiver_addr(arr: *mut ArrayHeader) -> usize {
    crate::typedarray::strip_nanbox(arr as u64)
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
/// length<=capacity sanity bound — reads the `ObjectHeader`'s words / inline
/// f64 slots as garbage elements (e.g. `8.48e-314`).
///
/// #8113 made that sanity bound WEAKER, not stronger: with `object_type` gone,
/// `length` aliases `class_id` and `capacity` aliases the ShapeId word, so a
/// plain object literal (`class_id == 0`) trivially satisfies
/// `length <= capacity`. The bound was never the defense — the GC-header
/// `obj_type` test below is — but do not reintroduce a caller that leans on it.
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
    // #6011: the raw-f64-or-holes invariant needs numeric stores canonicalized
    // exactly like the dense layout — an INT32-boxed store left verbatim would
    // read as NaN payload through the guard's raw-f64 loads.
    if array_numeric_layout(arr) == Some(NumericArrayLayout::RawF64)
        || array_has_raw_f64_holes_flag(arr)
    {
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

/// Canonicalize a store using the flag word read from an already-resolved
/// array. This is the no-second-classification twin of
/// [`canonicalize_array_numeric_store_value`].
#[inline(always)]
pub(crate) fn canonicalize_array_numeric_store_value_from_flags(flags: u16, value: f64) -> f64 {
    let raw_layout = crate::gc::GC_ARRAY_RAW_F64_LAYOUT | crate::gc::GC_ARRAY_RAW_F64_HOLES;
    if flags & raw_layout != 0 {
        if let Some(number) = value_bits_to_number(value.to_bits()) {
            return number;
        }
    }
    value
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
pub(super) unsafe fn array_gc_header(arr: *const ArrayHeader) -> Option<*mut crate::gc::GcHeader> {
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
        let raw_bits = crate::gc::GC_ARRAY_RAW_F64_LAYOUT | crate::gc::GC_ARRAY_RAW_F64_HOLES;
        let had_raw_layout = (*header)._reserved & raw_bits != 0;
        (*header)._reserved &= !raw_bits;
        if had_raw_layout {
            crate::typed_feedback::invalidate_representation_change(arr as usize);
        }
    }
}

/// #6011: hole-tolerant sibling of the dense raw-f64 flag — see
/// [`crate::gc::GC_ARRAY_RAW_F64_HOLES`]. Queried by the packed-f64
/// range-loop guard's verify pass; cleared alongside the dense flag by the
/// same `clear_array_numeric_layout` choke point.
#[inline]
unsafe fn array_has_raw_f64_holes_flag(arr: *const ArrayHeader) -> bool {
    array_gc_header(arr)
        .is_some_and(|header| (*header)._reserved & crate::gc::GC_ARRAY_RAW_F64_HOLES != 0)
}

#[inline]
unsafe fn set_array_raw_f64_holes_flag(arr: *const ArrayHeader) {
    if let Some(header) = array_gc_header(arr) {
        (*header)._reserved |= crate::gc::GC_ARRAY_RAW_F64_HOLES;
    }
}

/// #6011: mark a freshly hole-initialized user-facing array (`new Array(n)`):
/// every slot in `[0, length)` is `TAG_HOLE`, so the raw-f64-or-holes
/// invariant holds by construction. Callers must guarantee no slot has been
/// written since the hole fill. Internal scratch arrays that direct-write
/// slots afterwards (shape keys arrays, sort temporaries, …) must NOT be
/// marked — they bypass the layout-noting store helpers by contract.
#[inline]
pub(crate) unsafe fn mark_array_raw_f64_holes_fresh(arr: *const ArrayHeader) {
    set_array_raw_f64_holes_flag(arr);
}

/// Repsel 4a.2 (#6904): either raw-f64 invariant bit — the O(1) proof the
/// hole-tolerant fast tiers key on.
#[inline]
pub(crate) unsafe fn array_has_raw_f64_layout_or_holes(arr: *const ArrayHeader) -> bool {
    array_gc_header(arr).is_some_and(|header| {
        (*header)._reserved
            & (crate::gc::GC_ARRAY_RAW_F64_LAYOUT | crate::gc::GC_ARRAY_RAW_F64_HOLES)
            != 0
    })
}

/// Repsel 4a.2 (#6904): hole-fill store for a sparse-extend gap. `TAG_HOLE`
/// is part of the raw-f64-or-holes invariant, so this deliberately does NOT
/// run the numeric-layout clear that [`note_array_slot`] applies to
/// non-numeric values (which permanently demoted every sparsely-extended
/// numeric array to the O(n) verify walk). Layout note + write barrier still
/// apply (TAG_HOLE is a non-pointer sentinel).
#[inline]
pub(crate) unsafe fn note_array_hole_fill_slot(arr: *mut ArrayHeader, index: usize) {
    // GC_STORE_AUDIT(BARRIERED): TAG_HOLE sentinel store, layout-noted and barriered below.
    std::ptr::write(array_elements_ptr(arr).add(index), crate::value::TAG_HOLE);
    crate::gc::layout_note_slot(arr as usize, index, crate::value::TAG_HOLE);
    let slot = array_elements_ptr(arr).add(index) as usize;
    crate::gc::runtime_write_barrier_slot(arr as usize, slot, crate::value::TAG_HOLE);
}

/// Repsel 4a.2 (#6904): follow the growth/GC forwarding chain of a
/// POINTER-tagged array head and return the re-boxed LIVE head; every other
/// input (non-pointer tags, handle-band ids, non-arrays, already-live heads)
/// is returned unchanged.
///
/// Consumed by the inline guard tiers' COLD arms: a caller-held stale head
/// (canonically: a Phase 2 specialized-ABI callee grew an array the CALLER
/// allocated — the callee's growth write-backs update the callee's own param
/// slot, so the caller's binding keeps the pre-growth stub forever) fails
/// every structural guard by design, which pinned such receivers to the
/// boxed fallback on EVERY access. The cold arm calls this once, stores the
/// repaired head back into the receiver's local slot, and every later
/// iteration re-loads the live head and takes the inline tier. Semantics are
/// unchanged — forwarding is transparent, this only re-points the binding at
/// the same JS object's live storage.
#[no_mangle]
pub extern "C" fn js_array_refresh_local_head(value: f64) -> f64 {
    let bits = value.to_bits();
    if bits & crate::value::TAG_MASK != crate::value::POINTER_TAG {
        return value;
    }
    let raw = (bits & crate::value::POINTER_MASK) as usize;
    // Handle-band ids and implausible addresses pass through untouched.
    if !crate::value::addr_class::is_plausible_heap_addr(raw) {
        return value;
    }
    let cleaned = clean_arr_ptr(raw as *const ArrayHeader);
    if cleaned.is_null() || cleaned as usize == raw {
        return value;
    }
    f64::from_bits(crate::value::POINTER_TAG | (cleaned as u64 & crate::value::POINTER_MASK))
}

/// Repsel 4a.2 (#6904): a raw-f64(-or-holes) array that just gap-filled a
/// sparse extend with a numeric value keeps the verified raw-f64-or-holes
/// invariant — but holes now exist, so the DENSE flag must drop while the
/// HOLES flag records the invariant (previously this transition cleared both
/// flags, sending every later access through the O(n) rebuild walk).
#[inline]
pub(crate) unsafe fn demote_array_raw_f64_dense_to_holes(arr: *mut ArrayHeader) {
    if let Some(header) = array_gc_header(arr) {
        (*header)._reserved &= !crate::gc::GC_ARRAY_RAW_F64_LAYOUT;
        (*header)._reserved |= crate::gc::GC_ARRAY_RAW_F64_HOLES;
        crate::typed_feedback::invalidate_representation_change(arr as usize);
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
        if slot_bits == crate::value::TAG_HOLE {
            // #6011: a hole disproves the DENSE raw-f64 layout, but leaves
            // the raw-f64-or-holes invariant intact — the dense flag cannot
            // be set here (dense-flagged arrays have no holes), so there is
            // nothing to clear, and clearing would wrongly drop the holes
            // flag a fresh `new Array(n)` carries.
            return false;
        }
        let Some(number) = value_bits_to_number(slot_bits) else {
            clear_array_numeric_layout(arr);
            return false;
        };
        // #6011: skip the write-back when the slot already holds the raw-f64
        // bits (the overwhelmingly common case when a packed loop produced
        // the values). Halves the memory traffic of the first layout probe
        // of a large freshly-filled array.
        if number.to_bits() != slot_bits {
            // GC_STORE_AUDIT(POINTER_FREE): raw-f64 layout rewrite stores numeric payloads only.
            std::ptr::write(elements.add(i) as *mut f64, number);
        }
    }

    set_array_raw_f64_layout_flag(arr);
    crate::gc::layout_init_pointer_free(arr as *mut u8);
    true
}

/// #6011: hole-tolerant variant of [`rebuild_array_numeric_raw_f64`] for the
/// packed-f64 *range* loop guard. Every non-hole slot is rewritten to a raw
/// f64 (INT32-boxed integers included); `TAG_HOLE` slots are left in place —
/// they never contain a heap pointer, and the guarded loop's inline loads
/// hole-check each slot before use. A slot that is neither numeric nor a hole
/// (string / object / bool / …) clears the layout flag and fails the guard.
///
/// When no holes were seen the array is uniformly numeric, so the RawF64
/// layout flag is set exactly like the strict rebuild. When holes remain the
/// dense flag stays clear but `GC_ARRAY_RAW_F64_HOLES` records the verified
/// raw-f64-or-holes invariant (also pre-set by `new Array(n)` allocation), so
/// later guard entries — and the guard on a fresh hole-filled array — skip
/// the walk entirely. Either way the array is pointer-free and the GC layout
/// note reflects it.
pub(crate) unsafe fn rebuild_array_numeric_raw_f64_allow_holes(arr: *mut ArrayHeader) -> bool {
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
        // Already proven dense raw-f64 numeric — nothing to rewrite.
        return true;
    }
    if array_has_raw_f64_holes_flag(arr) {
        // #6011: invariant flag — every slot in `[0, length)` is already raw
        // f64 bits or TAG_HOLE (established at `new Array(n)` allocation or by
        // a previous verify walk; every non-numeric store clears the flag via
        // `clear_array_numeric_layout`, and numeric stores canonicalize to raw
        // bits via `canonicalize_array_numeric_store_bits`). Nothing to verify
        // or rewrite — this turns the per-loop-entry guard walk over a fresh
        // 100k-slot EMA output array into an O(1) flag test.
        return true;
    }

    let elements = array_elements_ptr(arr);
    let mut saw_hole = false;
    for i in 0..length {
        let slot_bits = array_slot_bits(arr, i);
        if slot_bits == crate::value::TAG_HOLE {
            saw_hole = true;
            continue;
        }
        let Some(number) = value_bits_to_number(slot_bits) else {
            clear_array_numeric_layout(arr);
            return false;
        };
        if number.to_bits() != slot_bits {
            // GC_STORE_AUDIT(POINTER_FREE): raw-f64 layout rewrite stores numeric payloads only.
            std::ptr::write(elements.add(i) as *mut f64, number);
        }
    }

    if !saw_hole {
        set_array_raw_f64_layout_flag(arr);
    } else {
        // Holes remain, but every non-hole slot is now canonical raw f64 —
        // record the verified invariant so re-entering the loop skips the walk.
        set_array_raw_f64_holes_flag(arr);
    }
    crate::gc::layout_init_pointer_free(arr as *mut u8);
    true
}

/// Dense-window variant of [`rebuild_array_numeric_raw_f64_allow_holes`] for
/// the read-only masked-index range loop: after the hole-tolerant rebuild,
/// additionally require that `[min_idx, max_idx_exclusive)` contains NO holes.
/// That loop's inline loads skip the per-slot hole check entirely (its body
/// may interleave several scalar writes per iteration, so a mid-iteration
/// side exit could double-apply effects on re-execution) — a hole inside the
/// window would leak `TAG_HOLE` bits as a raw double. Holes OUTSIDE the
/// window are fine and keep their raw-f64-or-holes invariant.
pub(crate) unsafe fn rebuild_array_numeric_raw_f64_dense_window(
    arr: *mut ArrayHeader,
    min_idx: i32,
    max_idx_exclusive: i32,
) -> bool {
    if !rebuild_array_numeric_raw_f64_allow_holes(arr) {
        return false;
    }
    if array_has_raw_f64_layout_flag(arr) {
        // Dense everywhere — no holes anywhere, window included.
        return true;
    }
    let len = (*arr).length as i64;
    let min = i64::from(min_idx).max(0);
    let max = i64::from(max_idx_exclusive).min(len);
    for i in min..max {
        if array_slot_bits(arr, i as usize) == crate::value::TAG_HOLE {
            return false;
        }
    }
    true
}

/// i32 tier of [`rebuild_array_numeric_raw_f64_dense_window`]: the window
/// must additionally hold only integers representable in a signed i32, so
/// the guarded loop's inline loads may materialize elements with a bare
/// `fptosi` (exact — no ToInt32 wrap tower) and keep bit-mixing chains like
/// bcrypt's Blowfish F in integer registers.
pub(crate) unsafe fn rebuild_array_numeric_raw_f64_dense_window_i32(
    arr: *mut ArrayHeader,
    min_idx: i32,
    max_idx_exclusive: i32,
) -> bool {
    if !rebuild_array_numeric_raw_f64_dense_window(arr, min_idx, max_idx_exclusive) {
        return false;
    }
    let len = (*arr).length as i64;
    let min = i64::from(min_idx).max(0);
    let max = i64::from(max_idx_exclusive).min(len);
    let elements = array_elements_ptr(arr) as *const f64;
    for i in min..max {
        let value = *elements.add(i as usize);
        if !value.is_finite()
            || value.fract() != 0.0
            || value < i32::MIN as f64
            || value > i32::MAX as f64
        {
            return false;
        }
    }
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
    // #7480: the numeric and element-shape invariants are mutually exclusive
    // — an element-shape array's slots are NaN-boxed pointers, which no
    // raw-f64 layout admits. This is also what covers the bulk numeric
    // producers (`js_array_fill_f64_*_extend`) that write slots directly and
    // then declare the layout.
    super::element_shape::clear_element_shape(arr);
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
        } else if array_has_raw_f64_holes_flag(old_user as *const ArrayHeader) {
            // #6011: relocation copies slot bits verbatim, so the verified
            // raw-f64-or-holes invariant carries over to the new backing.
            clear_array_raw_f64_layout_flag(new_user as *const ArrayHeader);
            set_array_raw_f64_holes_flag(new_user as *const ArrayHeader);
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

/// #7469 — declare ONCE, at allocation, that every element this array will
/// hold is a heap pointer, so the per-store pointer-mask bookkeeping
/// (`layout_note_slot`, and the `LAYOUT_SLOT_MASKS` entry it grows) is not
/// needed for the stores codegen has proven pointer-valued.
///
/// Emitted by codegen at the `[]` literal that binds an array local whose every
/// store it can prove pointer-by-construction; see
/// `perry-codegen/src/collectors/all_pointer_arrays.rs` for the proof and
/// `expr/array_push.rs` for the header test that re-validates the declaration
/// at every elided store.
///
/// Two things happen here, and both are load-bearing:
///
/// 1. **The raw-f64 numeric layout is cleared.** `js_array_alloc` publishes
///    every fresh array as `RawF64` + `POINTER_FREE` — the numeric fast paths
///    read such an array's slots back as raw doubles, which for a pointer
///    payload is a reinterpretation of a heap address as a number. The
///    all-pointer declaration and the raw-f64 flag are mutually exclusive
///    claims about the same bytes, and the codegen-side header test refuses the
///    elided store unless BOTH raw-f64 bits are clear, so this clear is what
///    keeps that test satisfiable.
/// 2. **`GC_LAYOUT_SIDE_MASK | GC_LAYOUT_ALL_POINTERS` replaces
///    `GC_LAYOUT_POINTER_FREE`.** For the collector that swaps "skip the whole
///    payload" for "visit every slot in `0..length`" — the same set of slots
///    `GC_LAYOUT_UNKNOWN` visits, so the declaration is conservative in the
///    only direction that matters: a wrong element type costs a rejected slot
///    read (`mark_field_into_worklist` re-validates every word), never a
///    stranded live child.
///
/// On a **non-empty** array the claim is not vacuous, so it is discharged
/// rather than assumed: every slot in `0..length` must be pointer-bearing by
/// `gc::layout_pointer_bearing_bits`, the same predicate the layout mask
/// builder and `GC_LAYOUT_UNKNOWN`'s per-slot re-validation use. The walk is
/// O(literal size), runs once at the binding, and does not have to trust the
/// caller's static proof.
///
/// #8102 is why that path exists. `emit_all_pointer_array_declaration` is
/// emitted from the `Stmt::Let` tail, i.e. *after* an array literal's element
/// stores have already installed a per-slot side mask. Refusing every
/// non-empty array therefore made the declaration a **silent no-op** for
/// `const a: C[] = [x, y]`, so every later `a.push(…)` failed the codegen
/// header test and paid the per-store layout note #7469 exists to delete —
/// measured at +33.9% instructions on a 4M-push loop, against the
/// byte-for-byte equivalent array built empty and pushed into.
/// `collectors/all_pointer_arrays.rs` already admits such a literal (see
/// `literal_of_object_elements_is_admitted`), so the proof was being issued and
/// then discarded.
///
/// A refusal is still silent and safe — the header keeps whatever layout it
/// had, and the codegen header test then declines the elided store and routes
/// the push through `js_array_push_f64`, which notes every slot as it always
/// did.
#[no_mangle]
pub extern "C" fn js_array_declare_all_pointer_elements(arr: *mut ArrayHeader) {
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        return;
    }
    unsafe {
        let length = (*arr).length as usize;
        let slots = if length == 0 {
            std::ptr::null()
        } else {
            array_elements_ptr(arr) as *const u64
        };
        // Clear the raw-f64 claim FIRST only when the declaration will stick:
        // a refused declaration must leave the header exactly as it found it.
        if !crate::gc::layout_all_pointer_slots_would_hold(slots, length) {
            return;
        }
        clear_array_numeric_layout(arr);
        crate::gc::layout_init_all_pointer_slots(arr as *mut u8);
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
        // #6011 follow-up: a holes-flagged array (`new Array(n)` mid-fill)
        // cannot be DENSE raw-f64, and answering that per store via the
        // verify walk below made a sequential fill loop O(N²) — the walk
        // reads the already-filled prefix on every store before hitting the
        // next hole (the fmr benchmark hang). While the LAST slot is still a
        // hole the array is provably not dense: answer 0 in O(1). Once the
        // last slot is written (a completed sequential fill), fall through so
        // the single walk below can upgrade the array to the dense flag.
        if array_has_raw_f64_holes_flag(arr) {
            let length = (*arr).length as usize;
            if length > 0 && array_slot_bits(arr, length - 1) == crate::value::TAG_HOLE {
                return 0;
            }
        }
        // #6011: single combined verify+rewrite pass. The old shape
        // (`array_slots_are_numeric` scan, THEN `rebuild_array_numeric_raw_
        // f64` rewrite) walked every slot twice on the first numeric-layout
        // probe of a large array — 2×100k slot reads per call for an
        // EMA-style `new Array(100_000)` filled by a loop. The rebuild
        // already fails cleanly on the first non-numeric slot (clearing the
        // layout flag); slots converted before such a failure were genuinely
        // numeric, and INT32-box → raw-f64 canonicalization is value-
        // preserving for every reader, so stopping mid-way is unobservable.
        if rebuild_array_numeric_raw_f64(arr as *mut ArrayHeader) {
            return 1;
        }
    }
    0
}

// These raw numeric-array helpers are called from generated code, so release/LTO
// builds may otherwise internalize and strip the `#[no_mangle]` exports.
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_ARRAY_NUMERIC_VALUE_TO_RAW_F64: extern "C" fn(f64) -> f64 =
    js_array_numeric_value_to_raw_f64;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_ARRAY_MARK_NUMERIC_F64_LAYOUT: extern "C" fn(*mut ArrayHeader) -> i32 =
    js_array_mark_numeric_f64_layout;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_ARRAY_CLEAR_NUMERIC_LAYOUT: extern "C" fn(*mut ArrayHeader) =
    js_array_clear_numeric_layout;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_ARRAY_NOTE_NUMERIC_WRITE: extern "C" fn(*mut ArrayHeader, u64) =
    js_array_note_numeric_write;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_ARRAY_DECLARE_ALL_POINTER_ELEMENTS: extern "C" fn(*mut ArrayHeader) =
    js_array_declare_all_pointer_elements;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_ARRAY_IS_NUMERIC_F64_LAYOUT: extern "C" fn(*const ArrayHeader) -> i32 =
    js_array_is_numeric_f64_layout;
#[cfg(feature = "keepalive-anchors")]
#[used]
static KEEP_JS_ARRAY_REFRESH_LOCAL_HEAD: extern "C" fn(f64) -> f64 = js_array_refresh_local_head;

/// Calculate the byte size for an array with N elements capacity
#[inline]
pub(crate) fn array_byte_size(capacity: usize) -> usize {
    std::mem::size_of::<ArrayHeader>() + capacity * std::mem::size_of::<f64>()
}

#[inline]
pub(super) unsafe fn array_elements_ptr(arr: *mut ArrayHeader) -> *mut u64 {
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
    // "Fresh/suppressed caller" does NOT imply barrier-free: a BORN-OLD array
    // (>16KB, e.g. a >2048-element JSON.parse result) is old-gen from birth, so
    // storing a young child creates an old→young edge that later minors need in
    // the remembered set — GC suppression only protects DURING the caller's
    // fill, not after it returns. This was the missing-edge bug behind the
    // old-young-edge-verifier failures (155 edges, all born-old array→young
    // object; slot_page_ever_dirty=false = the store never hit any barrier):
    // JSON.parse filled born-old arrays through this helper, the children were
    // swept live on a later minor → "value is not a function". The old-gen
    // check hits the page-generation cache (same array → same cached range), so
    // young arrays pay ~one cached compare.
    if crate::arena::pointer_in_old_gen(arr as usize) {
        let slot = array_elements_ptr(arr).add(index) as usize;
        crate::gc::runtime_write_barrier_slot(arr as usize, slot, value_bits);
    }
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
    // #7480: this is the post-hoc funnel most bulk element mutators use —
    // `shift`, `unshift`, `splice`, `fill`, `copyWithin`, and `reverse` all
    // mutate slots with bare `ptr::write` / `ptr::copy` and then land here.
    // NOT `sort`: its default path writes the rank permutation back through
    // `RootedArrayElems::set`, so it revokes through the STORE funnel
    // (`layout_note_slot`) instead — established by sabotage in #7608's
    // matrix (removing the revoke here leaves the sort test green). They are permutations or arbitrary
    // rewrites, so the element-shape proof is dropped conservatively; a
    // still-homogeneous array re-earns it on the next `ensure`.
    super::element_shape::clear_element_shape(arr);
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
    // #7480: same conservative drop as `rebuild_array_layout` — see there.
    super::element_shape::clear_element_shape(arr);
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

    // One parent, one contiguous slot run — the loop form of the barrier, whose
    // per-store entry point would re-derive the parent classification `length`
    // times and re-assert a page-granular fact ~512 times per page. See
    // `gc::barrier::replay_old_parent_slot_range`.
    crate::gc::replay_old_parent_slot_range_barriers(arr as usize, slots, length);
}

#[inline]
pub(crate) unsafe fn mark_array_layout_unknown(arr: *mut ArrayHeader) {
    clear_array_numeric_layout(arr);
    crate::gc::layout_mark_unknown(arr as *mut u8);
}

/// Minimum initial capacity for arrays to reduce reallocations
pub(crate) const MIN_ARRAY_CAPACITY: u32 = 16;
