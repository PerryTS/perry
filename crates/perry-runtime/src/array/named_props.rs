//! Own non-index ("named") properties of Array exotic objects, stored WITH
//! the array instead of in an address-keyed side table (#10166, brief 4).
//!
//! ## Storage
//!
//! An array that carries named properties has [`crate::gc::GC_ARRAY_NAMED_PROPS`]
//! set in its `GcHeader._reserved` word and reserves the FIRST physical
//! element slot of its backing store (`arr + 1`, physical index 0) for one
//! NaN-boxed pointer to a *pairs array*: an ordinary, never user-visible
//! `GC_TYPE_ARRAY` holding `[key0, value0, key1, value1, …]`. Keys are heap
//! strings (marked shared, so an in-place append can never rewrite one),
//! values are NaN-boxed JS values, and pair order is insertion order, which is
//! the enumeration order ECMA-262 requires after the integer indices.
//!
//! The reserve is expressed through the dense-queue front offset
//! `storage.rs` already defines: `capacity` excludes the reserved slot, so
//! `array_front_offset(arr) >= 1` for every flagged array and the logical
//! element addresses come out unchanged for every reader — including codegen,
//! which derives the element base from `GcHeader.size` and `capacity`
//! (`perry-codegen/src/array_storage.rs`). No layout table, pointer mask, or
//! inline fast path changes; the reserved word is simply front slack that
//! happens to hold a pointer.
//!
//! ## Why not the side table
//!
//! `ARRAY_NAMED_PROPS` was a `PtrHashMap<usize, Vec<_>>` keyed by the array
//! ADDRESS. Every regex `exec`/`match` result (`index`/`input`/`groups`) paid a
//! hash insert plus a `Vec` allocation, every read a hash probe, every move a
//! rekey (`visit_metadata_usize_slot` + merge), and every collection a
//! `retain` over the WHOLE table with the dead-owner predicate — about
//! 300 instructions per entry, on a table the short-lived exec results kept
//! refilling. With the pointer stored in the array nothing is keyed by an
//! address: a moved array carries its slot along, and a dead array's pairs die
//! with it, so there is no rekey and no prune.
//!
//! ## GC custody
//!
//! The pairs pointer is a child edge of the array. `gc::layout_slot_visit`'s
//! Array arm emits the reserved slot as a fixed slot (next to the #9304
//! prototype slot), so marking retains the pairs array, evacuation and
//! compaction rewrite the edge, and the remembered-set scan sees it through
//! the page the write barrier dirtied. Every store into the slot goes through
//! [`store_pairs_pointer`], which is the one barriered funnel. `js_array_grow`
//! re-reserves the slot in the replacement allocation and copies the pointer
//! (barriered again) before installing the growth forwarding stub, and
//! `storage::shift_dense`'s empty-queue reset keeps the reserve out of
//! `capacity`.
//!
//! ## Reserving the slot on an existing array
//!
//! A fresh runtime-built array (regex results) is born with the reserve
//! (`js_array_alloc_named_props_reserved`). A user array gains it on its first
//! expando: if the array is a shifted queue (`array_front_offset >= 1`) the
//! dead front slot is taken as is; otherwise the elements move up by one slot
//! inside the allocation (the dense-move funnel `finish_array_dense_move_layout`
//! translates dirty pages / replays barriers), growing first through
//! `js_array_grow` when the allocation is full. Only that last case changes the
//! array's address, and it does so exactly like a `push` past capacity does —
//! which is why the setter returns the live head.

use super::header::{
    array_object_flags_from_tag, array_object_flags_resolved, array_receiver_gc_tag, clean_arr_ptr,
    clean_arr_ptr_mut, string_header_as_str, string_header_bytes,
};
use super::{array_elements_ptr, array_front_offset, ArrayHeader};
use crate::gc::RuntimeHandleScope;
use crate::value::{POINTER_MASK, POINTER_TAG, STRING_TAG, TAG_HOLE, TAG_MASK, TAG_UNDEFINED};

/// A runtime-owned literal key with its intern-table hash computed at compile
/// time (`string::fnv1a_bytes`), so installing it is one direct-mapped slot
/// probe and no per-call hashing. Interned strings are rooted and rewritten by
/// the intern table's own scanner; the pointer is never cached here, which is
/// what keeps this correct under any root-scanner registry a test installs.
#[cfg(feature = "regex-engine")]
#[derive(Clone, Copy)]
pub(crate) struct LiteralKey {
    bytes: &'static [u8],
    hash: u64,
}

#[cfg(feature = "regex-engine")]
impl LiteralKey {
    pub(crate) const fn new(name: &'static str) -> Self {
        Self {
            bytes: name.as_bytes(),
            hash: crate::string::fnv1a_bytes(name.as_bytes()),
        }
    }

    /// The canonical interned string. Allocates on the first use per thread.
    #[inline]
    fn intern(self) -> *const crate::StringHeader {
        crate::string::intern_ascii_literal_hashed(self.bytes, self.hash)
    }
}

/// Physical slot the pairs pointer lives in when `GC_ARRAY_NAMED_PROPS` is set.
///
/// # Safety
/// `arr` must be a live, forwarding-resolved `GC_TYPE_ARRAY` head.
#[inline(always)]
pub(crate) unsafe fn array_named_props_slot(arr: *const ArrayHeader) -> *mut u64 {
    arr.add(1) as *mut u64
}

/// Whether the resolved head reserves the named-property slot.
///
/// # Safety
/// `arr` must satisfy [`array_object_flags_resolved`]'s contract.
#[inline(always)]
pub(crate) unsafe fn array_named_props_flagged_resolved(arr: *const ArrayHeader) -> bool {
    array_object_flags_resolved(arr) & crate::gc::GC_ARRAY_NAMED_PROPS != 0
}

/// Number of physical slots reserved in front of the logical elements (0 or 1).
/// `js_array_grow` sizes the replacement allocation with it and
/// `storage::shift_dense` keeps it out of `capacity`.
///
/// # Safety
/// `arr` must satisfy [`array_object_flags_resolved`]'s contract.
#[inline(always)]
pub(crate) unsafe fn array_named_props_reserve(arr: *const ArrayHeader) -> usize {
    usize::from(array_named_props_flagged_resolved(arr))
}

/// The flag word of a receiver `clean_arr_ptr` resolved, read through the
/// validated header path. `clean_arr_ptr` waves registered Buffer / TypedArray
/// receivers through, and those carry no `GcHeader`: the eight bytes below
/// their payload are allocator bookkeeping, so a raw `_reserved` read there
/// could invent the flag and dereference the buffer's first data word as a
/// pairs pointer.
#[inline]
unsafe fn resolved_flags(arr: *const ArrayHeader) -> u16 {
    array_object_flags_from_tag(array_receiver_gc_tag(arr))
}

/// The pairs array of a resolved head, or null when the head has no reserve or
/// the reserve is still empty.
///
/// # Safety
/// `arr` must be a live, forwarding-resolved `GC_TYPE_ARRAY` head.
#[inline]
unsafe fn pairs_of(arr: *const ArrayHeader, flags: u16) -> *mut ArrayHeader {
    if flags & crate::gc::GC_ARRAY_NAMED_PROPS == 0 {
        return std::ptr::null_mut();
    }
    let bits = *array_named_props_slot(arr);
    if bits & TAG_MASK != POINTER_TAG {
        return std::ptr::null_mut();
    }
    // Invariant: the slot always holds the LIVE pairs head. Growth writes the
    // replacement back through `store_pairs_pointer` before anything can read
    // it, and every collector move rewrites the slot (it is an enumerated
    // child edge), so no forwarding resolution is needed here.
    (bits & POINTER_MASK) as *mut ArrayHeader
}

/// Resolve `arr` and return its pairs array, or null.
#[inline]
unsafe fn resolve_pairs(arr: *const ArrayHeader) -> (*const ArrayHeader, *mut ArrayHeader) {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return (arr, std::ptr::null_mut());
    }
    (arr, pairs_of(arr, resolved_flags(arr)))
}

/// Index of the key slot whose string content equals `name`, if any.
///
/// # Safety
/// `pairs` must be a live pairs array.
#[inline]
unsafe fn find_pair(pairs: *const ArrayHeader, name: &str) -> Option<usize> {
    let len = (*pairs).length as usize;
    let elems = array_elements_ptr(pairs);
    let wanted = name.as_bytes();
    let mut i = 0;
    while i + 1 < len {
        let key_bits = *elems.add(i);
        if key_bits & TAG_MASK == STRING_TAG {
            let key = (key_bits & POINTER_MASK) as *const crate::StringHeader;
            if string_header_bytes(key) == Some(wanted) {
                return Some(i);
            }
        }
        i += 2;
    }
    None
}

/// Store `pairs` into the reserved slot of `arr`. The ONE barriered funnel for
/// the edge (also used by `js_array_grow` when it carries the edge to the
/// replacement head).
///
/// # Safety
/// `arr` must be a live, forwarding-resolved head with the reserve, `pairs` a
/// live pairs array. Nothing may allocate between the write and the barrier.
pub(crate) unsafe fn store_pairs_pointer(arr: *mut ArrayHeader, pairs: *mut ArrayHeader) {
    let slot = array_named_props_slot(arr);
    let bits = crate::value::js_nanbox_pointer(pairs as i64).to_bits();
    // GC_STORE_AUDIT(BARRIERED): the reserved-slot edge is recorded by the
    // slot barrier below; the collector enumerates this exact word as a fixed
    // child slot of the array.
    std::ptr::write(slot, bits);
    crate::gc::runtime_write_barrier_slot(arr as usize, slot as usize, bits);
}

/// Allocate a pairs array with room for `pair_capacity` pairs. Its layout is
/// left `GC_LAYOUT_UNKNOWN` (the allocator's zero state): the collector
/// tag-scans its handful of slots, so no store below needs a layout note that
/// would do anything. `length` is `preset_len` slots of `TAG_HOLE`, so a
/// collection in the middle of a fill still visits every slot written so far.
///
/// Allocates: may collect.
unsafe fn pairs_alloc(pair_capacity: usize, preset_len: usize) -> *mut ArrayHeader {
    let capacity = (pair_capacity.max(2) * 2) as u32;
    let pairs = crate::arena::arena_alloc_gc(
        super::header::array_byte_size(capacity as usize),
        8,
        crate::gc::GC_TYPE_ARRAY,
    ) as *mut ArrayHeader;
    (*pairs).length = preset_len as u32;
    (*pairs).capacity = capacity;
    let elems = array_elements_ptr(pairs);
    // The caller writes every slot below `preset_len` before anything else can
    // allocate; only the slack needs the sentinel.
    for i in preset_len..capacity as usize {
        // GC_STORE_AUDIT(INIT): hole-initialising a just-allocated array that
        // nothing references yet; TAG_HOLE is a non-pointer sentinel.
        std::ptr::write(elems.add(i), TAG_HOLE);
    }
    pairs
}

/// Store one word into slot `i` of a pairs array.
///
/// A pairs array is never raw-f64 and keeps the allocator's
/// `GC_LAYOUT_UNKNOWN` (tag-scanned) state for its whole life — growth copies
/// that state, and the only in-place mutation (`finish_array_dense_move_layout`
/// on delete) leaves it alone — so the layout and numeric notes the generic
/// element store performs have nothing to do here. The slot barrier is the
/// one thing a store owes.
///
/// # Safety
/// `pairs` must be a live pairs array and `i < capacity`.
#[inline]
unsafe fn write_slot(pairs: *mut ArrayHeader, i: usize, bits: u64) {
    let slot = array_elements_ptr(pairs).add(i);
    // GC_STORE_AUDIT(BARRIERED): runtime_write_barrier_slot below records the
    // edge; the pairs array's layout is tag-scanned, so no layout note is owed.
    std::ptr::write(slot, bits);
    crate::gc::runtime_write_barrier_slot(pairs as usize, slot as usize, bits);
}

/// Write one pair into slots `i`, `i + 1` of a pairs array (no length change).
///
/// # Safety
/// `pairs` must be live and `i + 1 < capacity`; `key_bits` a `STRING_TAG` box.
#[inline]
unsafe fn write_pair(pairs: *mut ArrayHeader, i: usize, key_bits: u64, value_bits: u64) {
    write_slot(pairs, i, key_bits);
    write_slot(pairs, i + 1, value_bits);
}

/// Give a resolved head the reserved slot, returning the live head — which is
/// a different allocation only when the array was full and had to grow (a
/// forwarding stub then resolves the old address, exactly as after a `push`
/// past capacity). Returns null when the reserve could not be taken (a sealed
/// or frozen full array, which no caller can add a property to anyway).
///
/// # Safety
/// `arr` must be a live, forwarding-resolved `GC_TYPE_ARRAY` head that
/// `resolved_flags` reported as a real array. May allocate (via `js_array_grow`).
unsafe fn ensure_named_props_slot(arr: *mut ArrayHeader) -> *mut ArrayHeader {
    if array_named_props_flagged_resolved(arr) {
        return arr;
    }
    let mut arr = arr;
    if array_front_offset(arr) == 0 {
        let capacity = (*arr).capacity as usize;
        let dense = ((*arr).length as usize).min(capacity);
        if dense >= capacity {
            // Full: no slack anywhere. Grow exactly like an append would.
            let grown = super::js_array_grow(arr, (capacity as u32).saturating_add(1));
            if grown.is_null() || ((*grown).capacity as usize) <= dense {
                return std::ptr::null_mut();
            }
            arr = grown;
        }
        let capacity = (*arr).capacity as usize;
        let dense = ((*arr).length as usize).min(capacity);
        let elems = array_elements_ptr(arr);
        // GC_STORE_AUDIT(BARRIERED): the dense prefix moves up by one slot
        // inside its own allocation; `finish_array_dense_move_layout` below
        // translates the old array's dirty-page coverage or replays the
        // barriers for the moved survivors.
        std::ptr::copy(elems, elems.add(1), dense);
        (*arr).capacity -= 1;
        debug_assert_eq!(array_front_offset(arr), 1);
        super::finish_array_dense_move_layout(arr, elems, elems.add(1), dense, elems, 0);
    }
    // GC_STORE_AUDIT(INIT): the reserved word is dead front slack until the
    // flag below publishes it as a child slot; a non-pointer sentinel goes in
    // first so the collector never reads a stale element there.
    std::ptr::write(array_named_props_slot(arr), TAG_HOLE);
    let header = crate::gc::header_from_trusted_user_ptr(arr.cast()).cast_mut();
    (*header)._reserved |= crate::gc::GC_ARRAY_NAMED_PROPS;
    arr
}

/// The existing special-property bit is deliberately conservative and
/// monotone: sharing it with named properties gives the callback-free array
/// consumers an address-local absence proof without a second flag test.
#[inline]
unsafe fn mark_array_descriptors(arr: *mut ArrayHeader) {
    let header = crate::gc::header_from_trusted_user_ptr(arr.cast()).cast_mut();
    (*header)._reserved |= crate::gc::OBJ_FLAG_ARRAY_DESCRIPTORS;
}

/// `arr[name] = value` for a non-index string key. Returns the live head: the
/// array moves only when it was full and had to grow (see
/// [`ensure_named_props_slot`]); callers that keep using the receiver must use
/// the returned pointer, as they already do after `js_array_grow`.
///
/// # Safety
/// `key` must be a live heap string. May allocate (a pairs array, growth).
pub(crate) unsafe fn array_named_property_set(
    arr: *mut ArrayHeader,
    key: *const crate::StringHeader,
    value: f64,
) -> *mut ArrayHeader {
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        return arr;
    }
    let flags = resolved_flags(arr);
    let Some(name) = string_header_as_str(key) else {
        return arr;
    };
    let pairs = pairs_of(arr, flags);
    if !pairs.is_null() {
        // Overwrite in place: no allocation, no move.
        if let Some(i) = find_pair(pairs, name) {
            write_slot(pairs, i + 1, value.to_bits());
            return arr;
        }
        // Append into existing room: still no allocation.
        let len = (*pairs).length as usize;
        if len + 2 <= (*pairs).capacity as usize {
            let key_bits = crate::value::js_nanbox_string(key as i64).to_bits();
            crate::string::js_string_addref_if_heap_string(f64::from_bits(key_bits));
            write_pair(pairs, len, key_bits, value.to_bits());
            (*pairs).length = (len + 2) as u32;
            return arr;
        }
    }
    // A new key that needs the reserve, a pairs array, or pairs growth: those
    // allocate, so root everything a collection could move.
    let scope = RuntimeHandleScope::new();
    let key_handle = scope.root_string_ptr(key);
    let value_handle = scope.root_nanbox_f64(value);
    let base_handle = scope.root_raw_mut_ptr(arr);
    // Taking the reserve may grow (allocate): the base is reloaded across it.
    let (arr, base) = base_handle.across_mut::<ArrayHeader, _>(|| ensure_named_props_slot(arr));
    if arr.is_null() {
        // No reserve could be taken (sealed/frozen full array): the property
        // is not added, exactly as the guard ladder above this call decided.
        return clean_arr_ptr_mut(base);
    }
    let arr_handle = scope.root_raw_mut_ptr(arr);
    let mut pairs = pairs_of(arr, array_object_flags_resolved(arr));
    if pairs.is_null() {
        let (fresh, arr) = arr_handle.across_mut::<ArrayHeader, _>(|| pairs_alloc(4, 0));
        store_pairs_pointer(clean_arr_ptr_mut(arr), fresh);
        pairs = fresh;
    }
    let len = (*pairs).length as usize;
    if len + 2 > (*pairs).capacity as usize {
        let (grown, arr) = arr_handle
            .across_mut::<ArrayHeader, _>(|| super::js_array_grow(pairs, (len + 2) as u32));
        store_pairs_pointer(clean_arr_ptr_mut(arr), grown);
        pairs = grown;
    }
    // Nothing below allocates: the final reads are scoped to non-allocating
    // operations.
    let arr = arr_handle.with_mut_ptr::<ArrayHeader, _>(clean_arr_ptr_mut);
    // The key is now shared with the pairs array: an in-place append on a
    // still-unique source string must copy instead of rewriting our key.
    let key_bits = key_handle.with_const_ptr::<crate::StringHeader, _>(|key| {
        crate::value::js_nanbox_string(key as i64).to_bits()
    });
    crate::string::js_string_addref_if_heap_string(f64::from_bits(key_bits));
    write_pair(
        pairs,
        len,
        key_bits,
        value_handle.get_nanbox_f64().to_bits(),
    );
    (*pairs).length = (len + 2) as u32;
    mark_array_descriptors(arr);
    arr
}

/// Allocate a pairs array holding `keys` (values `undefined`) for a FRESHLY
/// built array, so that [`array_named_props_install_fresh`] can install the
/// values without allocating. Keys are interned literals, pointer-identical to
/// the same literals elsewhere in the program and rooted by the intern table.
///
/// Allocates: may collect. Callers must re-read every value they intend to
/// install from a handle AFTER this returns.
#[cfg(feature = "regex-engine")]
pub(crate) unsafe fn array_named_props_pairs_alloc(keys: &[LiteralKey]) -> *mut ArrayHeader {
    // Intern every key BEFORE allocating the pairs array (the first use per
    // thread allocates), so that nothing between the allocation and the key
    // stores can collect. After that each probe is a hit that returns the
    // table's canonical pointer, stored before the next probe.
    for key in keys {
        key.intern();
    }
    let pairs = pairs_alloc(keys.len(), keys.len() * 2);
    for (i, key) in keys.iter().enumerate() {
        let key = key.intern();
        debug_assert!(!key.is_null());
        write_pair(
            pairs,
            i * 2,
            crate::value::js_nanbox_string(key as i64).to_bits(),
            TAG_UNDEFINED,
        );
    }
    pairs
}

/// Install `values` (one per key of `pairs`, in key order) and attach the pairs
/// array to a FRESHLY built array born with the reserve
/// (`js_array_alloc_named_props_reserved`). Nothing here allocates, so the
/// caller's raw values stay current. A fresh array has no accessor
/// descriptors, attributes, or freeze/seal state, which is what makes
/// bypassing `js_array_set_string_key`'s guard ladder sound.
///
/// # Safety
/// `arr` must be a live head born with the reserve; `pairs` must come from
/// [`array_named_props_pairs_alloc`] with exactly `values.len()` keys.
#[cfg(feature = "regex-engine")]
pub(crate) unsafe fn array_named_props_install_fresh(
    arr: *mut ArrayHeader,
    pairs: *mut ArrayHeader,
    values: &[f64],
) {
    let arr = clean_arr_ptr_mut(arr);
    if arr.is_null() {
        return;
    }
    debug_assert!(array_named_props_flagged_resolved(arr));
    debug_assert_eq!((*pairs).length as usize, values.len() * 2);
    for (i, value) in values.iter().enumerate() {
        write_slot(pairs, i * 2 + 1, value.to_bits());
    }
    store_pairs_pointer(arr, pairs);
    mark_array_descriptors(arr);
}

/// Does this (already resolved) array head carry at least one named property?
///
/// # Safety
/// `arr` must satisfy [`array_object_flags_resolved`]'s contract.
#[inline]
pub(crate) unsafe fn array_has_named_properties_resolved(arr: *const ArrayHeader) -> bool {
    let pairs = pairs_of(arr, array_object_flags_resolved(arr));
    !pairs.is_null() && (*pairs).length >= 2
}

/// Whether an already-resolved array owns numeric indices among its named
/// properties. Those indices live beyond the dense allocation; growing the
/// allocation across one without migrating it would hide the property because
/// indexed reads consult the named properties only at `index >= capacity`.
///
/// # Safety
/// `arr` must satisfy [`array_object_flags_resolved`]'s contract.
pub(crate) unsafe fn array_has_sparse_index_properties_resolved(arr: *const ArrayHeader) -> bool {
    let pairs = pairs_of(arr, array_object_flags_resolved(arr));
    if pairs.is_null() {
        return false;
    }
    let len = (*pairs).length as usize;
    let elems = array_elements_ptr(pairs);
    let mut i = 0;
    while i + 1 < len {
        let key_bits = *elems.add(i);
        if key_bits & TAG_MASK == STRING_TAG {
            let key = (key_bits & POINTER_MASK) as *const crate::StringHeader;
            if string_header_as_str(key)
                .is_some_and(|name| crate::object::canonical_array_index(name).is_some())
            {
                return true;
            }
        }
        i += 2;
    }
    false
}

pub(crate) unsafe fn array_named_property_get_by_name(
    arr: *const ArrayHeader,
    name: &str,
) -> Option<f64> {
    let (_, pairs) = resolve_pairs(arr);
    if pairs.is_null() {
        return None;
    }
    find_pair(pairs, name).map(|i| f64::from_bits(*array_elements_ptr(pairs).add(i + 1)))
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
    let (_, pairs) = resolve_pairs(arr);
    !pairs.is_null() && find_pair(pairs, name).is_some()
}

/// Own named-property keys in insertion order (after the integer indices in
/// `[[OwnPropertyKeys]]`), optionally only the enumerable ones.
pub(crate) unsafe fn array_named_property_names(
    arr: *const ArrayHeader,
    enumerable_only: bool,
) -> Vec<String> {
    let (arr, pairs) = resolve_pairs(arr);
    if pairs.is_null() {
        return Vec::new();
    }
    let owner = arr as usize;
    let len = (*pairs).length as usize;
    let elems = array_elements_ptr(pairs);
    let mut names = Vec::with_capacity(len / 2);
    let mut i = 0;
    while i + 1 < len {
        let key_bits = *elems.add(i);
        i += 2;
        if key_bits & TAG_MASK != STRING_TAG {
            continue;
        }
        let key = (key_bits & POINTER_MASK) as *const crate::StringHeader;
        let Some(name) = string_header_as_str(key) else {
            continue;
        };
        if enumerable_only
            && !crate::object::get_property_attrs(owner, name)
                .map(|attrs| attrs.enumerable())
                .unwrap_or(true)
        {
            continue;
        }
        names.push(name.to_string());
    }
    names
}

pub(crate) unsafe fn array_named_property_delete(
    arr: *const ArrayHeader,
    key: *const crate::StringHeader,
) -> bool {
    let Some(name) = string_header_as_str(key) else {
        return false;
    };
    array_named_property_delete_by_name(arr, name)
}

/// Remove one pair, closing the gap so insertion order is preserved for the
/// survivors. Nothing here allocates.
pub(crate) unsafe fn array_named_property_delete_by_name(
    arr: *const ArrayHeader,
    name: &str,
) -> bool {
    let (_, pairs) = resolve_pairs(arr);
    if pairs.is_null() {
        return false;
    }
    let Some(i) = find_pair(pairs, name) else {
        return false;
    };
    let len = (*pairs).length as usize;
    let elems = array_elements_ptr(pairs);
    let moved = len - i - 2;
    // GC_STORE_AUDIT(BARRIERED): survivors slide down inside the pairs
    // allocation; `finish_array_dense_move_layout` below translates dirty
    // pages or replays their barriers, and the vacated tail holds holes.
    std::ptr::copy(elems.add(i + 2), elems.add(i), moved);
    std::ptr::write(elems.add(len - 2), TAG_HOLE);
    std::ptr::write(elems.add(len - 1), TAG_HOLE);
    (*pairs).length = (len - 2) as u32;
    super::finish_array_dense_move_layout(pairs, elems.add(i + 2), elems.add(i), moved, elems, 0);
    true
}

/// Test-only view of the reserve: `(flagged, pairs head or 0, pair count)`.
#[cfg(test)]
pub(crate) unsafe fn test_named_props_state(arr: *const ArrayHeader) -> (bool, usize, usize) {
    let arr = clean_arr_ptr(arr);
    if arr.is_null() {
        return (false, 0, 0);
    }
    let flags = resolved_flags(arr);
    let pairs = pairs_of(arr, flags);
    let count = if pairs.is_null() {
        0
    } else {
        (*pairs).length as usize / 2
    };
    (
        flags & crate::gc::GC_ARRAY_NAMED_PROPS != 0,
        pairs as usize,
        count,
    )
}
