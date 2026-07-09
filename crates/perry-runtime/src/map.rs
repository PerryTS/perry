//! Map representation for Perry
//!
//! Maps are arena-allocated GC objects.
//! The entries array is separately allocated and can be reallocated
//! without changing the MapHeader address between GC moves.

use crate::fast_hash::{new_ptr_hash_set, PtrHashSet};
use crate::string::StringHeader;
use std::alloc::{alloc, dealloc, realloc, Layout};
use std::cell::RefCell;
use std::hash::{Hash, Hasher};
use std::ptr;

/// Must match value.rs TAG_UNDEFINED
const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;

thread_local! {
    static MAP_ITERATOR_ARRAYS: RefCell<PtrHashSet<usize>> = RefCell::new(new_ptr_hash_set());
}

fn mark_map_iterator_array(arr: *mut crate::array::ArrayHeader) {
    if !arr.is_null() {
        MAP_ITERATOR_ARRAYS.with(|r| {
            r.borrow_mut().insert(arr as usize);
        });
    }
}

pub fn is_registered_map_iterator(addr: usize) -> bool {
    MAP_ITERATOR_ARRAYS.with(|r| r.borrow().contains(&addr))
}

#[cfg(test)]
thread_local! {
    static TEST_FORCE_HELPER_GC: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn test_force_next_map_helper_gc() {
    TEST_FORCE_HELPER_GC.with(|force| force.set(force.get().saturating_add(1)));
}

#[cfg(test)]
fn maybe_force_helper_gc_for_test() {
    let should_collect = TEST_FORCE_HELPER_GC.with(|force| {
        let remaining = force.get();
        if remaining > 0 {
            force.set(remaining - 1);
            true
        } else {
            false
        }
    });
    if should_collect {
        let _ = crate::gc::gc_collect_minor();
    }
}

#[cfg(not(test))]
#[inline(always)]
fn maybe_force_helper_gc_for_test() {}

thread_local! {
    static MAP_REGISTRY: RefCell<PtrHashSet<usize>> = RefCell::new(new_ptr_hash_set());
}

fn register_map(ptr: *mut MapHeader) {
    MAP_REGISTRY.with(|r| r.borrow_mut().insert(ptr as usize));
}

pub fn is_registered_map(addr: usize) -> bool {
    // #4004: small-handle registry ids (Web Fetch, perry-ffi/node:http, timers,
    // …) are NaN-boxed POINTER_TAG values living below the small-handle
    // cutoff; they are not heap addresses. Managed Maps are arena-allocated
    // above it. See `value::addr_class` for the band map.
    if crate::value::addr_class::is_handle_band(addr) {
        return false;
    }
    // Registry FIRST: it is authoritative and dereference-free (mirrors
    // set::is_registered_set, #4665). The previous ordering probed
    // `GcHeader.obj_type` at `addr - 8` as a fast pre-filter BEFORE the
    // registry lookup — that dereferenced arbitrary above-band candidate
    // pointers (e.g. garbage read off a mis-typed receiver) and segfaults on
    // Linux where freed/foreign pages get unmapped (mimalloc on macOS retains
    // them, hiding the bug). The pre-filter's perf rationale (a ~5.7%-sample
    // SipHash `HashSet::contains`) predates MAP_REGISTRY moving to the
    // Fibonacci-hash `PtrHashSet`, which is what set.rs ships with today.
    if !MAP_REGISTRY.with(|r| r.borrow().contains(&addr)) {
        return false;
    }
    // A registered address is a live arena Map; the header read is safe and
    // guards against a stale entry whose memory was reused by another type.
    match unsafe { crate::value::addr_class::try_read_gc_header(addr) } {
        Some(header) => header.obj_type == crate::gc::GC_TYPE_MAP,
        None => false,
    }
}

/// Resolve a NaN-boxed (or raw-i64) `this` receiver to a registered `Map`
/// pointer, or `None` if the receiver is not a `Map`. Backs the reflective
/// `Map.prototype.*` thunks so they can perform the spec brand check
/// (`TypeError` on a non-`Map` receiver) before dispatching. See
/// `set::set_ptr_from_receiver_bits` for the receiver-extraction rationale.
pub fn map_ptr_from_receiver_bits(bits: u64) -> Option<*mut MapHeader> {
    let jsv = crate::value::JSValue::from_bits(bits);
    let addr = if jsv.is_pointer() {
        (bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else if bits >> 48 == 0 && bits > 0x10000 {
        bits as usize
    } else {
        return None;
    };
    if is_registered_map(addr) {
        Some(addr as *mut MapHeader)
    } else {
        None
    }
}

/// Numeric-key index entry: hashed/compared by raw f64 bits only.
/// Strings/object-pointer keys are NOT inserted here — those still go
/// through the linear-scan fallback in `find_key_index`. The reason is
/// that gen-GC may forward a string/object behind a Map.entries slot,
/// and the entries-array gets rewritten via `rewrite_map_fields`, but
/// the side-table's stored f64 bits for that key go stale. A subsequent
/// lookup that triggers `jsvalue_eq` on the stale bits would deref
/// freed memory (string content compare). Numeric f64 values have no
/// pointers, so they're safe to index by bits.
#[derive(Clone, Copy)]
struct NumericKey(u64);

impl Hash for NumericKey {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl PartialEq for NumericKey {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}
impl Eq for NumericKey {}

/// `true` if `bits` is a non-pointer JSValue (number, bool, undefined,
/// null, INT32, or any NaN-tagged value that is NOT a string/heap pointer).
/// We index only these in the side-table.
#[inline]
fn is_safe_numeric_key(bits: u64) -> bool {
    let upper = bits >> 48;
    // STRING_TAG (0x7FFF), POINTER_TAG (0x7FFD), BIGINT_TAG (0x7FFE) are pointers.
    if upper == 0x7FFF || upper == 0x7FFD || upper == 0x7FFE {
        return false;
    }
    // SHORT_STRING_TAG (0x7FF9) inline SSO strings need content-based
    // comparison against heap STRING_TAG keys (issue #434). Routing them
    // through the bits-keyed side-table would mask cross-representation
    // matches: a Map populated with heap-string keys has no side-table
    // slot, so an SSO lookup would short-circuit to -1 and skip the
    // linear-scan fallback that calls `jsvalue_eq`. Force SSO keys onto
    // the linear path so content equality kicks in.
    if upper == (crate::value::SHORT_STRING_TAG >> 48) {
        return false;
    }
    // Raw pointer (0x0000) with a plausible heap address is also a pointer.
    if upper == 0x0000 {
        let lower = bits & 0x0000_FFFF_FFFF_FFFF;
        if lower > 0x10000 {
            return false;
        }
    }
    true
}

// Side-table mapping `map_ptr -> (NumericKey-bits -> entries-array-index)`.
// O(1) `find_key_index` for numeric keys; pointer keys still take the
// linear-scan path so they remain correct under gen-GC string forwarding.
//
// Both nesting levels use `PtrHasher` (Fibonacci-multiplicative + xorshift
// avalanche, see `crate::fast_hash`). The xorshift step is essential here
// because `NumericKey(u64)` holds f64 bit-patterns — small whole-number
// EntityIds have mantissa-zero, so pure multiplicative hashing would
// collapse hundreds of keys into bucket 0 (caught by a 2x regression
// the first time around). With the avalanche step, even the worst-case
// integer-f64 inputs distribute across buckets normally.
thread_local! {
    static MAP_INDEX: RefCell<
        crate::fast_hash::PtrHashMap<usize, crate::fast_hash::PtrHashMap<NumericKey, u32>>,
    > = RefCell::new(crate::fast_hash::new_ptr_hash_map());
}

// Side-table mapping `map_ptr -> (FNV-1a 64-bit content hash -> Vec<entries-array-index>)`
// for STRING keys. Bypasses the gen-GC-stale-bits constraint that keeps
// `MAP_INDEX` numeric-only by hashing the string's CONTENT, not its
// pointer bits — so a forwarded heap-string and an SSO inline string
// with the same bytes share the same bucket. Stored values are u32
// indexes into the entries array (not pointers), which survive
// `rewrite_map_fields` evacuation rewrites untouched.
//
// The per-bucket `Vec<u32>` accommodates hash collisions: while FNV-1a
// 64-bit collisions are vanishingly rare for distinct strings, we still
// validate each candidate via `jsvalue_eq` on lookup so a collision
// just costs an extra few-byte memcmp, never a wrong answer.
//
// Pre-fix `Map.set("key_" + i, …)` over 500k inserts was O(N²) because
// each `set` did a linear `find_key_index` to dedup-check; with this
// table the dedup probe is O(1) amortized.
thread_local! {
    static MAP_STRING_INDEX: RefCell<
        crate::fast_hash::PtrHashMap<usize, std::collections::HashMap<u64, Vec<u32>>>,
    > = RefCell::new(crate::fast_hash::new_ptr_hash_map());
}

/// FNV-1a 64-bit content hash for any string-like JSValue.
/// Returns `None` for non-strings, `Some(FNV_OFFSET_BASIS)` for the empty
/// string. SSO and heap STRING_TAG hash into the same space because both
/// representations decode through `string_view_from_bits`.
#[inline]
fn string_content_hash(value_bits: u64) -> Option<u64> {
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let (ptr, len) = string_view_from_bits(value_bits, &mut scratch)?;
    // FNV-1a 64-bit constants per http://www.isthe.com/chongo/tech/comp/fnv/
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    if len == 0 {
        return Some(h);
    }
    unsafe {
        let bytes = std::slice::from_raw_parts(ptr, len as usize);
        for &b in bytes {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    Some(h)
}

#[inline]
fn boxed_heap_string_key(key: *const StringHeader) -> f64 {
    f64::from_bits(crate::value::STRING_TAG | ((key as u64) & crate::value::POINTER_MASK))
}

/// Drop the side-table entry AND deregister from `MAP_REGISTRY` for a
/// map address that's about to be reused or freed. Safe to call on
/// unregistered addresses.
///
/// Without the `MAP_REGISTRY.remove`, a freed Map's address would
/// permanently identify as a Map even after the GC slot is reused for
/// (say) an Array — so `js_array_get_f64` would route through the Map
/// branch, read the new Array's first u32 as `(*map).size`, the next
/// 8 bytes as `(*map).entries`, and dereference whatever bit pattern
/// happened to land at offset 8. With gen-GC churn this manifested as
/// an `EXC_BAD_ACCESS` at address 0x7ffd_02xx_xxxx_xxxx (POINTER_TAG
/// bits read as a raw pointer) inside `js_array_get_f64 + 672` while
/// `processCommands` iterated `commands[i]` over an Array whose memory
/// had been a Map a few collections earlier.
pub fn drop_map_index(addr: usize) {
    MAP_INDEX.with(|idx| {
        idx.borrow_mut().remove(&addr);
    });
    MAP_STRING_INDEX.with(|idx| {
        idx.borrow_mut().remove(&addr);
    });
    MAP_REGISTRY.with(|r| {
        r.borrow_mut().remove(&addr);
    });
}

pub(crate) fn map_header_moved_for_gc(old_addr: usize, new_addr: usize) {
    if old_addr == 0 || new_addr == 0 || old_addr == new_addr {
        return;
    }
    MAP_REGISTRY.with(|r| {
        let mut registry = r.borrow_mut();
        if registry.remove(&old_addr) {
            registry.insert(new_addr);
        }
    });
    MAP_INDEX.with(|idx| {
        let mut idx = idx.borrow_mut();
        idx.remove(&new_addr);
        if let Some(slot) = idx.remove(&old_addr) {
            idx.insert(new_addr, slot);
        }
    });
    MAP_STRING_INDEX.with(|idx| {
        let mut idx = idx.borrow_mut();
        idx.remove(&new_addr);
        if let Some(slot) = idx.remove(&old_addr) {
            idx.insert(new_addr, slot);
        }
    });
}

pub(crate) unsafe fn finalize_map_side_allocation_for_gc(map: *mut MapHeader) {
    if map.is_null() {
        return;
    }
    let addr = map as usize;
    let was_registered = MAP_REGISTRY.with(|r| r.borrow_mut().remove(&addr));
    MAP_INDEX.with(|idx| {
        idx.borrow_mut().remove(&addr);
    });
    MAP_STRING_INDEX.with(|idx| {
        idx.borrow_mut().remove(&addr);
    });
    if !was_registered {
        return;
    }

    let entries = (*map).entries;
    let capacity = (*map).capacity as usize;
    if !entries.is_null() && capacity > 0 {
        dealloc(entries as *mut u8, entries_layout(capacity));
        crate::gc::gc_note_external_side_free(entries_layout(capacity).size());
    }
    // GC_STORE_AUDIT(POINTER_FREE): finalizer clears external entries side-allocation pointer after deregistration/deallocation.
    (*map).entries = std::ptr::null_mut();
    (*map).capacity = 0;
    (*map).size = 0;
}

fn is_dead_copied_minor_from_space_map(addr: usize) -> bool {
    let space = crate::arena::classify_heap_space(addr);
    if !matches!(space, crate::arena::HeapSpace::NurseryEden)
        && space != crate::arena::active_survivor_space()
    {
        return false;
    }
    if addr < crate::gc::GC_HEADER_SIZE {
        return false;
    }
    unsafe {
        let header = (addr - crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*header).obj_type != crate::gc::GC_TYPE_MAP {
            return false;
        }
        let flags = (*header).gc_flags;
        flags & crate::gc::GC_FLAG_ARENA != 0
            && flags & (crate::gc::GC_FLAG_MARKED | crate::gc::GC_FLAG_FORWARDED) == 0
    }
}

/// #6010: registry-driven finalization of DEAD Maps at sweep entry, for the
/// non-copying cycle kinds (fallback minor / full mark-sweep). A dead Map
/// sitting in the ACTIVE nursery allocation block is never processed by any
/// sweeper — the block is still being bump-allocated into, so it is neither
/// reset nor object-walked — and bulk block resets skip per-object finalize
/// hooks anyway. Its multi-megabyte external entries buffer therefore leaked
/// for the life of the process. Walk the registry right after trace (marks
/// fresh, nothing cleared yet) and free the buffers of provably-dead maps;
/// the 16-byte headers stay behind as ordinary dead bytes for whichever
/// block operation eventually reclaims them.
///
/// Deadness: unmarked ∧ not pinned ∧ not forwarded, and — for a MINOR trace,
/// which never traces the old generation — additionally not tenured and
/// physically in the nursery (the same "unmarked nursery object is garbage"
/// invariant the ordinary sweeper relies on, backed by the write-barrier
/// remembered set for old→young edges).
pub(crate) fn collect_dead_registered_maps_post_trace(full_trace: bool) -> Vec<usize> {
    MAP_REGISTRY.with(|r| {
        r.borrow()
            .iter()
            .copied()
            .filter(|&addr| unsafe { registered_map_is_dead_post_trace(addr, full_trace) })
            .collect()
    })
}

/// Finalize one collected-dead Map (budget-chunked by the sweep state).
pub(crate) fn finalize_collected_dead_map(addr: usize) {
    unsafe {
        finalize_map_side_allocation_for_gc(addr as *mut MapHeader);
    }
}

unsafe fn registered_map_is_dead_post_trace(addr: usize, full_trace: bool) -> bool {
    let Some(header) = crate::value::addr_class::try_read_gc_header(addr) else {
        return false;
    };
    if header.obj_type != crate::gc::GC_TYPE_MAP {
        return false;
    }
    let flags = header.gc_flags;
    if flags
        & (crate::gc::GC_FLAG_MARKED | crate::gc::GC_FLAG_PINNED | crate::gc::GC_FLAG_FORWARDED)
        != 0
    {
        return false;
    }
    if full_trace {
        return true;
    }
    if flags & crate::gc::GC_FLAG_TENURED != 0 {
        return false;
    }
    matches!(
        crate::arena::classify_heap_generation(addr),
        crate::arena::HeapGeneration::Nursery
    )
}

pub(crate) fn finalize_dead_copied_minor_from_space_maps() -> usize {
    let maps = MAP_REGISTRY.with(|r| {
        r.borrow()
            .iter()
            .copied()
            .filter(|&addr| is_dead_copied_minor_from_space_map(addr))
            .collect::<Vec<_>>()
    });
    let count = maps.len();
    for addr in maps {
        unsafe {
            finalize_map_side_allocation_for_gc(addr as *mut MapHeader);
        }
    }
    count
}

#[cfg(test)]
pub(crate) fn test_map_numeric_index_contains(map: *const MapHeader, key: f64) -> bool {
    let key = normalize_zero(key);
    let bits = key.to_bits();
    if !is_safe_numeric_key(bits) {
        return false;
    }
    MAP_INDEX.with(|idx| {
        idx.borrow()
            .get(&(map as usize))
            .is_some_and(|slot| slot.contains_key(&NumericKey(bits)))
    })
}

#[cfg(test)]
pub(crate) fn test_map_string_index_contains(map: *const MapHeader, key: f64) -> bool {
    let bits = key.to_bits();
    let Some(hash) = string_content_hash(bits) else {
        return false;
    };
    MAP_STRING_INDEX.with(|idx| {
        idx.borrow()
            .get(&(map as usize))
            .is_some_and(|slot| slot.get(&hash).is_some_and(|bucket| !bucket.is_empty()))
    })
}

/// Strip NaN-boxing tags from a map pointer (defensive guard).
/// If the pointer has NaN-boxing tags in the upper 16 bits, strip them.
/// Returns null for undefined/null NaN-boxing tags.
#[inline(always)]
fn clean_map_ptr(map: *const MapHeader) -> *const MapHeader {
    let bits = map as u64;
    let top16 = bits >> 48;
    if top16 >= 0x7FF8 {
        if top16 == 0x7FFC || (bits & 0x0000_FFFF_FFFF_FFFF) == 0 {
            return std::ptr::null();
        }
        (bits & 0x0000_FFFF_FFFF_FFFF) as *const MapHeader
    } else {
        map
    }
}

#[inline(always)]
fn clean_map_ptr_mut(map: *mut MapHeader) -> *mut MapHeader {
    clean_map_ptr(map as *const MapHeader) as *mut MapHeader
}

/// Map header - GC-movable address, entries allocated separately
#[repr(C)]
pub struct MapHeader {
    /// Number of key-value pairs in the map
    pub size: u32,
    /// Capacity (allocated space for entries)
    pub capacity: u32,
    /// Pointer to entries array (separately allocated)
    pub entries: *mut f64,
}

/// Each map entry is 16 bytes (key + value, both as f64/JSValue)
const ENTRY_SIZE: usize = 16;

/// Calculate the layout for an entries array with N entries capacity
fn entries_layout(capacity: usize) -> Layout {
    let entries_size = capacity * ENTRY_SIZE;
    Layout::from_size_align(entries_size.max(8), 8).unwrap()
}

/// Get pointer to entries array
unsafe fn entries_ptr(map: *const MapHeader) -> *const f64 {
    (*map).entries as *const f64
}

/// Get mutable pointer to entries array
unsafe fn entries_ptr_mut(map: *mut MapHeader) -> *mut f64 {
    (*map).entries
}

/// SameValueZero key normalization: -0 → +0.
/// ECMAScript Maps/Sets treat -0 and +0 as the same key (23.1.3.9). Without
/// this, `0` (bits 0x0) and `-0` (bits 0x8000_0000_0000_0000) hash/compare
/// as distinct keys. Non-number JSValues have NaN-box tags in the upper bits
/// so `v == 0.0` stays false for them (NaN-tagged f64 is never equal to 0.0).
#[inline(always)]
fn normalize_zero(key: f64) -> f64 {
    if key == 0.0 {
        0.0
    } else if key.is_nan() && crate::value::JSValue::from_bits(key.to_bits()).is_number() {
        // SameValueZero treats every NaN as the same key (23.1.3.x). The
        // bits-keyed side-table and the bit-equality fast path in `jsvalue_eq`
        // would otherwise bucket distinct NaN payloads separately. Canonicalize
        // genuine number NaNs only — `is_number()` excludes NaN-boxed tagged
        // values (objects/strings/bigints), whose payloads must be preserved.
        f64::NAN
    } else {
        key
    }
}

#[inline(always)]
fn normalize_number_key_from_boxed(key: f64) -> Option<f64> {
    let js_value = crate::value::JSValue::from_bits(key.to_bits());
    if js_value.is_int32() {
        Some(normalize_zero(js_value.as_int32() as f64))
    } else if js_value.is_number() {
        Some(normalize_zero(key))
    } else {
        None
    }
}

/// Extract a string pointer from a value that might be NaN-boxed with various tags.
/// Returns the raw pointer if the value looks like it contains a string pointer, or null otherwise.
/// Does NOT handle SHORT_STRING_TAG (SSO) — those don't carry a heap pointer;
/// use `string_view_from_bits` for representation-agnostic content access.
fn extract_string_ptr_from_value(bits: u64) -> *const StringHeader {
    let upper = bits >> 48;
    match upper {
        0x7FFF => (bits & 0x0000_FFFF_FFFF_FFFF) as *const StringHeader, // STRING_TAG
        0x7FFD => (bits & 0x0000_FFFF_FFFF_FFFF) as *const StringHeader, // POINTER_TAG (string stored as generic pointer)
        0x0000 => {
            // Raw pointer (no NaN-boxing tag)
            let lower = bits & 0x0000_FFFF_FFFF_FFFF;
            if lower > 0x10000 {
                lower as *const StringHeader
            } else {
                std::ptr::null()
            }
        }
        _ => std::ptr::null(),
    }
}

/// Return a `(ptr, byte_len)` view for any string-like JSValue.
/// Heap pointers point into the `StringHeader`'s inline data; SSO values
/// decode into `scratch`. Returns `None` for non-string values.
///
/// Issue #434: pre-fix, jsvalue_eq only handled heap-pointer string
/// representations, so `Map.get(JSON.parse('"hello"'))` missed the
/// `"hello"` key stored as STRING_TAG.
fn string_view_from_bits(
    bits: u64,
    scratch: &mut [u8; crate::value::SHORT_STRING_MAX_LEN],
) -> Option<(*const u8, u32)> {
    let upper = bits >> 48;
    if upper == (crate::value::SHORT_STRING_TAG >> 48) {
        let len = ((bits & crate::value::SHORT_STRING_LEN_MASK)
            >> crate::value::SHORT_STRING_LEN_SHIFT) as usize;
        let data = bits & crate::value::SHORT_STRING_DATA_MASK;
        for (i, slot) in scratch.iter_mut().enumerate().take(len) {
            *slot = ((data >> (i * 8)) & 0xFF) as u8;
        }
        return Some((scratch.as_ptr(), len as u32));
    }
    let ptr = extract_string_ptr_from_value(bits);
    if ptr.is_null() || (ptr as usize) < 0x1000 {
        return None;
    }
    unsafe {
        let len = (*ptr).byte_len;
        let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        Some((data, len))
    }
}

/// Check if a value looks like it contains a string (heap STRING_TAG / inline
/// SHORT_STRING_TAG SSO, or POINTER_TAG / raw pointer that *actually* points at a
/// `GC_TYPE_STRING` allocation).
///
/// Issue #549: pre-fix, this returned `true` for any POINTER_TAG value because
/// `extract_string_ptr_from_value` accepts the tag without validating the GC
/// type at the pointee. That made `jsvalue_eq` content-compare two distinct
/// objects (or an object and a string, etc.) by reinterpreting the
/// `ObjectHeader` as a `StringHeader` — `class_id` showed up as `byte_len`,
/// the comparison read raw memory past the header as "string bytes", and two
/// empty `{}` literals (same class_id, both empty) ended up colliding inside
/// `Set.add` / `Map.set`. Validate the GC header here so only real string
/// pointees enter the content-comparison path; everything else falls back
/// to the bit-identity check that JS Set/Map `SameValueZero` semantics call
/// for on object keys.
fn is_string_like(bits: u64) -> bool {
    let upper = bits >> 48;
    if upper == (crate::value::SHORT_STRING_TAG >> 48) {
        return true;
    }
    // STRING_TAG always identifies a string pointee — accept without GC check.
    if upper == 0x7FFF {
        return !extract_string_ptr_from_value(bits).is_null();
    }
    let ptr = extract_string_ptr_from_value(bits);
    if ptr.is_null() || (ptr as usize) < 0x1000 {
        return false;
    }
    unsafe {
        let gc_hdr =
            (ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        (*gc_hdr).obj_type == crate::gc::GC_TYPE_STRING
    }
}

/// Check if two JSValues are equal (for map key comparison)
/// Handles STRING_TAG (0x7FFF), POINTER_TAG (0x7FFD), SHORT_STRING_TAG (0x7FF9 SSO),
/// raw pointers (0x0000), and cross-tag combinations (e.g., STRING_TAG vs SHORT_STRING_TAG).
fn jsvalue_eq(a: f64, b: f64) -> bool {
    let a_bits = a.to_bits();
    let b_bits = b.to_bits();

    // Fast path: identical bit patterns
    if a_bits == b_bits {
        return true;
    }

    // Symbols are compared by identity only — two distinct symbols are never
    // equal (and a same-symbol match was already caught by the bit-equality
    // fast path). A description-less `Symbol()` exposes a zero-length string
    // view, so without this guard it would content-compare equal to the ""
    // key and collide inside Map/Set. (#4570)
    if unsafe { crate::symbol::js_is_symbol(a) != 0 || crate::symbol::js_is_symbol(b) != 0 } {
        return false;
    }

    if is_string_like(a_bits) && is_string_like(b_bits) {
        let mut a_scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        let mut b_scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        if let (Some((a_ptr, a_len)), Some((b_ptr, b_len))) = (
            string_view_from_bits(a_bits, &mut a_scratch),
            string_view_from_bits(b_bits, &mut b_scratch),
        ) {
            if a_len != b_len {
                return false;
            }
            if a_len == 0 {
                return true;
            }
            unsafe {
                let a_slice = std::slice::from_raw_parts(a_ptr, a_len as usize);
                let b_slice = std::slice::from_raw_parts(b_ptr, b_len as usize);
                return a_slice == b_slice;
            }
        }
    }

    false
}

/// Allocate a new empty map with the given initial capacity
#[no_mangle]
pub extern "C" fn js_map_alloc(capacity: u32) -> *mut MapHeader {
    let cap = if capacity == 0 { 4 } else { capacity };
    let ent_layout = entries_layout(cap as usize);

    // Allocate the fixed-size header in the managed arena. The entries buffer
    // remains external and is traced through the Map rewrite descriptor.
    let ptr =
        crate::arena::arena_alloc_gc(std::mem::size_of::<MapHeader>(), 8, crate::gc::GC_TYPE_MAP)
            as *mut MapHeader;

    unsafe {
        // Entries array uses standard alloc (not gc-tracked, just data).
        // Zero the buffer at allocation: libc hands out raw memory and a
        // freshly-allocated Map after a sibling was freed often lands on
        // the same address. find_key_index walks entries[0..size]; if a
        // realloc-grow leaves stale bytes in the live range a `has()`
        // check can find a stale key from a prior Map. Witnessed in
        // ecs-perf-test/repro/foreach-many.ts iter 5: 2500 stale entries
        // from iter 4's freed buffer made `Map.has(5121)` return true
        // on a fresh Map that never saw entity 5121.
        let entries = alloc(ent_layout) as *mut f64;
        if entries.is_null() {
            // #5067 — catchable RangeError instead of aborting on OOM.
            crate::error::throw_allocation_failed();
        }
        ptr::write_bytes(entries as *mut u8, 0u8, ent_layout.size());

        // Initialize header
        (*ptr).size = 0;
        (*ptr).capacity = cap;
        // GC_STORE_AUDIT(INIT): map entries buffer is external storage; element stores are barriered separately.
        (*ptr).entries = entries;

        // Register in map registry for runtime type detection
        register_map(ptr);

        // Initialize / reset the O(1) lookup side-table for this address.
        // Arena reuse may recycle a freed Map's GC slot, so a stale index
        // entry from the prior occupant must be cleared here.
        MAP_INDEX.with(|idx| {
            idx.borrow_mut()
                .insert(ptr as usize, crate::fast_hash::new_ptr_hash_map());
        });
        MAP_STRING_INDEX.with(|idx| {
            idx.borrow_mut()
                .insert(ptr as usize, std::collections::HashMap::new());
        });

        // #6010: the entries buffer is invisible to the arena/malloc GC
        // triggers; record its bytes as external churn so Map-heavy
        // workloads still collect (and finalize dead siblings). Safe here:
        // the header is fully initialized + registered, and the triggered
        // cycle is conservative + non-moving, so `ptr` stays valid.
        crate::gc::gc_note_external_side_alloc(ent_layout.size());

        ptr
    }
}

/// Get the number of entries in the map
#[no_mangle]
pub extern "C" fn js_map_size(map: *const MapHeader) -> u32 {
    let map = clean_map_ptr(map);
    if map.is_null() {
        return 0;
    }
    unsafe { (*map).size }
}

/// Find the index of a key in the map, or -1 if not found.
/// Uses the O(1) MAP_INDEX side-table; falls back to a linear scan only
/// when no side-table entry exists (e.g. a Map produced by a path that
/// bypassed `js_map_alloc`).
/// Below this size, linear scan over the entries buffer beats the
/// side-table lookup (RefCell::borrow + HashMap::get is ~100ns per
/// call; a linear scan over <=8 f64 keys is ~10-20ns + better cache
/// locality). Most archetype.componentData / per-entity-relations Maps
/// hold 1-3 entries — paying the side-table cost on them dominates
/// the perf-comprehensive sync-heavy benchmarks.
const SIDE_TABLE_THRESHOLD: u32 = 8;

/// C-ABI: current entries-array index of `key` (SameValueZero), or `-1.0` if
/// absent. Used by the delete-safe `for-of` fast path (#6075) to re-derive the
/// cursor after a mid-iteration delete compacts the entries array. Only invoked
/// from generated IR, so `#[used]` keeps it linked on the default compile path.
#[no_mangle]
pub extern "C" fn js_map_find_key_index(map_boxed: f64, key: f64) -> f64 {
    let map = clean_map_ptr(crate::value::js_nanbox_get_pointer(map_boxed) as *const MapHeader);
    if map.is_null() {
        return -1.0;
    }
    unsafe { find_key_index(map, normalize_zero(key)) as f64 }
}
#[used]
static KEEP_MAP_FIND_KEY_INDEX: extern "C" fn(f64, f64) -> f64 = js_map_find_key_index;

pub(crate) unsafe fn find_key_index(map: *const MapHeader, key: f64) -> i32 {
    let size = (*map).size;
    let key_bits = key.to_bits();

    // Small maps: linear scan beats side-table dispatch.
    if size <= SIDE_TABLE_THRESHOLD {
        let entries = entries_ptr(map);
        for i in 0..size {
            let entry_key = ptr::read(entries.add((i as usize) * 2));
            if jsvalue_eq(entry_key, key) {
                return i as i32;
            }
        }
        return -1;
    }

    // Side-table fast path is restricted to non-pointer keys. Object /
    // bigint pointer keys still take the linear scan because the
    // side-table's stored bits go stale when gen-GC forwards the
    // backing object (see comment on `NumericKey`).
    if is_safe_numeric_key(key_bits) {
        let hit = MAP_INDEX.with(|idx| {
            let idx = idx.borrow();
            if let Some(slot) = idx.get(&(map as usize)) {
                if let Some(&i) = slot.get(&NumericKey(key_bits)) {
                    if i < size {
                        return Some(i as i32);
                    }
                }
                return Some(-1i32);
            }
            None
        });
        if let Some(v) = hit {
            return v;
        }
    }

    // String-key fast path: content-hashed side-table bypasses the
    // gen-GC-stale-bits constraint by hashing the bytes (heap-pointer
    // string and SSO collide into the same bucket). Index values are
    // u32 entry offsets — pointer-stable across `rewrite_map_fields`.
    if is_string_like(key_bits) {
        if let Some(h) = string_content_hash(key_bits) {
            let entries = entries_ptr(map);
            let hit = MAP_STRING_INDEX.with(|idx| {
                let idx = idx.borrow();
                if let Some(slot) = idx.get(&(map as usize)) {
                    if let Some(bucket) = slot.get(&h) {
                        // FNV-1a collisions are rare but possible; validate
                        // each candidate via `jsvalue_eq` (memcmp on bytes).
                        for &cand_idx in bucket {
                            if cand_idx >= size {
                                continue;
                            }
                            let cand_key = ptr::read(entries.add((cand_idx as usize) * 2));
                            if jsvalue_eq(cand_key, key) {
                                return Some(cand_idx as i32);
                            }
                        }
                    }
                    return Some(-1i32);
                }
                None
            });
            if let Some(v) = hit {
                return v;
            }
        }
    }

    // Linear scan for object/bigint pointer keys, or maps with no side-table entry.
    let entries = entries_ptr(map);
    for i in 0..size {
        let entry_key = ptr::read(entries.add((i as usize) * 2));
        if jsvalue_eq(entry_key, key) {
            return i as i32;
        }
    }

    -1
}

unsafe fn find_string_key_index(map: *const MapHeader, key: *const StringHeader) -> i32 {
    let size = (*map).size;
    let key_value = boxed_heap_string_key(key);
    let key_bits = key_value.to_bits();

    if size <= SIDE_TABLE_THRESHOLD {
        let entries = entries_ptr(map);
        for i in 0..size {
            let entry_key = ptr::read(entries.add((i as usize) * 2));
            if jsvalue_eq(entry_key, key_value) {
                return i as i32;
            }
        }
        return -1;
    }

    if let Some(h) = string_content_hash(key_bits) {
        let entries = entries_ptr(map);
        let hit = MAP_STRING_INDEX.with(|idx| {
            let idx = idx.borrow();
            if let Some(slot) = idx.get(&(map as usize)) {
                if let Some(bucket) = slot.get(&h) {
                    for &cand_idx in bucket {
                        if cand_idx >= size {
                            continue;
                        }
                        let cand_key = ptr::read(entries.add((cand_idx as usize) * 2));
                        if jsvalue_eq(cand_key, key_value) {
                            return Some(cand_idx as i32);
                        }
                    }
                }
                return Some(-1i32);
            }
            None
        });
        if let Some(v) = hit {
            return v;
        }
    }

    let entries = entries_ptr(map);
    for i in 0..size {
        let entry_key = ptr::read(entries.add((i as usize) * 2));
        if jsvalue_eq(entry_key, key_value) {
            return i as i32;
        }
    }

    -1
}

/// Grow the entries array if needed (header stays at same address)
unsafe fn ensure_capacity(map: *mut MapHeader) -> bool {
    let size = (*map).size;
    let capacity = (*map).capacity;

    if size < capacity {
        return false;
    }

    // Double the capacity
    let new_capacity = capacity * 2;
    let old_layout = entries_layout(capacity as usize);
    let new_layout = entries_layout(new_capacity as usize);

    let new_entries = realloc((*map).entries as *mut u8, old_layout, new_layout.size()) as *mut f64;
    if new_entries.is_null() {
        // #5067 — a constructor-driven `new Map(hugeIterable)` can hit this
        // growth path; surface a catchable RangeError instead of aborting.
        crate::error::throw_allocation_failed();
    }

    // GC_STORE_AUDIT(INIT): map external buffer pointer moves; live entry slots are dirtied by caller.
    (*map).entries = new_entries;
    (*map).capacity = new_capacity;
    // #6010: growth delta counts as external churn (see js_map_alloc). The
    // header is consistent again, and a triggered cycle is conservative +
    // non-moving, so the caller's raw `map`/entries pointers stay valid.
    crate::gc::gc_note_external_side_alloc(new_layout.size() - old_layout.size());
    true
}

unsafe fn map_set_string_key_value(
    map: *mut MapHeader,
    key: *const StringHeader,
    value: f64,
) -> *mut MapHeader {
    let idx = find_string_key_index(map, key);

    if idx >= 0 {
        let entries = entries_ptr_mut(map);
        let value_slot = entries.add((idx as usize) * 2 + 1);
        // GC_STORE_AUDIT(EXTERNAL_BARRIERED): map value slot uses the shared external-slot helper.
        crate::gc::runtime_store_external_jsvalue_slot(
            map as usize,
            value_slot as usize,
            value.to_bits(),
        );
        return map;
    }

    let grew = ensure_capacity(map);
    let size = (*map).size;
    let entries = entries_ptr_mut(map);
    if grew && size > 0 {
        crate::gc::runtime_dirty_external_slot_span(
            map as usize,
            entries as usize,
            size as usize * 2,
        );
    }

    let key_value = boxed_heap_string_key(key);
    let key_slot = entries.add((size as usize) * 2);
    let value_slot = entries.add((size as usize) * 2 + 1);
    // GC_STORE_AUDIT(EXTERNAL_BARRIERED): map append key/value slots use the shared external-slot helper.
    crate::gc::runtime_store_external_jsvalue_slot(
        map as usize,
        key_slot as usize,
        key_value.to_bits(),
    );
    crate::gc::runtime_store_external_jsvalue_slot(
        map as usize,
        value_slot as usize,
        value.to_bits(),
    );

    (*map).size = size + 1;

    if let Some(h) = string_content_hash(key_value.to_bits()) {
        MAP_STRING_INDEX.with(|idx| {
            let mut idx = idx.borrow_mut();
            let slot = idx
                .entry(map as usize)
                .or_insert_with(std::collections::HashMap::new);
            slot.entry(h).or_insert_with(Vec::new).push(size);
        });
    }

    map
}

/// Set a key-value pair in the map
/// The map pointer is stable (never reallocated)
#[no_mangle]
pub extern "C" fn js_map_set(map: *mut MapHeader, key: f64, value: f64) -> *mut MapHeader {
    let map = clean_map_ptr_mut(map);
    if map.is_null() {
        return map;
    }
    let key = normalize_zero(key);
    unsafe {
        // Check if key already exists (O(1) via MAP_INDEX)
        let idx = find_key_index(map, key);

        if idx >= 0 {
            // Update existing value (key position unchanged → no index update)
            let entries = entries_ptr_mut(map);
            let value_slot = entries.add((idx as usize) * 2 + 1);
            // GC_STORE_AUDIT(EXTERNAL_BARRIERED): map value slot uses the shared external-slot helper.
            crate::gc::runtime_store_external_jsvalue_slot(
                map as usize,
                value_slot as usize,
                value.to_bits(),
            );
            return map;
        }

        // Key doesn't exist, append a new entry
        let grew = ensure_capacity(map);
        let size = (*map).size;
        let entries = entries_ptr_mut(map);
        if grew && size > 0 {
            crate::gc::runtime_dirty_external_slot_span(
                map as usize,
                entries as usize,
                size as usize * 2,
            );
        }

        let key_slot = entries.add((size as usize) * 2);
        let value_slot = entries.add((size as usize) * 2 + 1);
        // GC_STORE_AUDIT(EXTERNAL_BARRIERED): map append key/value slots use the shared external-slot helper.
        crate::gc::runtime_store_external_jsvalue_slot(
            map as usize,
            key_slot as usize,
            key.to_bits(),
        );
        crate::gc::runtime_store_external_jsvalue_slot(
            map as usize,
            value_slot as usize,
            value.to_bits(),
        );

        (*map).size = size + 1;

        // Update O(1) side-table for numeric keys. Object/bigint pointer
        // keys stay out so a gen-GC forward of the backing object can't
        // leave stale bits in the index.
        let key_bits = key.to_bits();
        if is_safe_numeric_key(key_bits) {
            MAP_INDEX.with(|idx| {
                let mut idx = idx.borrow_mut();
                let slot = idx
                    .entry(map as usize)
                    .or_insert_with(crate::fast_hash::new_ptr_hash_map);
                slot.insert(NumericKey(key_bits), size);
            });
        } else if is_string_like(key_bits) {
            // String key: content-hashed index bypasses the gen-GC stale-bits
            // constraint by storing entry indexes (not pointers) keyed by
            // FNV-1a 64-bit hash of the bytes.
            if let Some(h) = string_content_hash(key_bits) {
                MAP_STRING_INDEX.with(|idx| {
                    let mut idx = idx.borrow_mut();
                    let slot = idx
                        .entry(map as usize)
                        .or_insert_with(std::collections::HashMap::new);
                    slot.entry(h).or_insert_with(Vec::new).push(size);
                });
            }
        }

        map
    }
}

#[no_mangle]
pub extern "C" fn js_map_set_number_key(
    map: *mut MapHeader,
    key: f64,
    value: f64,
) -> *mut MapHeader {
    let Some(key) = normalize_number_key_from_boxed(key) else {
        return js_map_set(map, key, value);
    };
    js_map_set(map, key, value)
}

#[no_mangle]
pub extern "C" fn js_map_set_string_number(
    map: *mut MapHeader,
    key: *const StringHeader,
    value: f64,
) -> *mut MapHeader {
    let map = clean_map_ptr_mut(map);
    if map.is_null() {
        return map;
    }
    unsafe { map_set_string_key_value(map, key, value) }
}

#[no_mangle]
pub extern "C" fn js_map_set_string_key(
    map: *mut MapHeader,
    key: *const StringHeader,
    value: f64,
) -> *mut MapHeader {
    let map = clean_map_ptr_mut(map);
    if map.is_null() {
        return map;
    }
    unsafe { map_set_string_key_value(map, key, value) }
}

#[no_mangle]
pub extern "C" fn js_map_set_string_i32(
    map: *mut MapHeader,
    key: *const StringHeader,
    value: i32,
) -> *mut MapHeader {
    let map = clean_map_ptr_mut(map);
    if map.is_null() {
        return map;
    }
    let value_bits = crate::value::INT32_TAG | ((value as u32) as u64);
    unsafe { map_set_string_key_value(map, key, f64::from_bits(value_bits)) }
}

#[no_mangle]
pub extern "C" fn js_map_set_string_u32(
    map: *mut MapHeader,
    key: *const StringHeader,
    value: u32,
) -> *mut MapHeader {
    let map = clean_map_ptr_mut(map);
    if map.is_null() {
        return map;
    }
    unsafe { map_set_string_key_value(map, key, f64::from(value)) }
}

#[no_mangle]
pub extern "C" fn js_map_set_string_f32(
    map: *mut MapHeader,
    key: *const StringHeader,
    value: f32,
) -> *mut MapHeader {
    let map = clean_map_ptr_mut(map);
    if map.is_null() {
        return map;
    }
    unsafe { map_set_string_key_value(map, key, f64::from(value)) }
}

#[no_mangle]
pub extern "C" fn js_map_set_string_bool(
    map: *mut MapHeader,
    key: *const StringHeader,
    value: i32,
) -> *mut MapHeader {
    let map = clean_map_ptr_mut(map);
    if map.is_null() {
        return map;
    }
    let value_bits = if value != 0 {
        crate::value::TAG_TRUE
    } else {
        crate::value::TAG_FALSE
    };
    unsafe { map_set_string_key_value(map, key, f64::from_bits(value_bits)) }
}

#[no_mangle]
pub extern "C" fn js_map_set_string_string(
    map: *mut MapHeader,
    key: *const StringHeader,
    value: *const StringHeader,
) -> *mut MapHeader {
    let map = clean_map_ptr_mut(map);
    if map.is_null() {
        return map;
    }
    unsafe { map_set_string_key_value(map, key, boxed_heap_string_key(value)) }
}

/// Get a value from the map by key
/// Returns the value, or TAG_UNDEFINED if not found
#[no_mangle]
pub extern "C" fn js_map_get(map: *const MapHeader, key: f64) -> f64 {
    let map = clean_map_ptr(map);
    if map.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let key = normalize_zero(key);
    unsafe {
        let idx = find_key_index(map, key);

        if idx >= 0 {
            let entries = entries_ptr(map);
            return ptr::read(entries.add((idx as usize) * 2 + 1));
        }

        f64::from_bits(TAG_UNDEFINED)
    }
}

#[no_mangle]
pub extern "C" fn js_map_get_number_key(map: *const MapHeader, key: f64) -> f64 {
    let Some(key) = normalize_number_key_from_boxed(key) else {
        return js_map_get(map, key);
    };
    js_map_get(map, key)
}

#[no_mangle]
pub extern "C" fn js_map_get_string_key(map: *const MapHeader, key: *const StringHeader) -> f64 {
    let map = clean_map_ptr(map);
    if map.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    unsafe {
        let idx = find_string_key_index(map, key);

        if idx >= 0 {
            let entries = entries_ptr(map);
            return ptr::read(entries.add((idx as usize) * 2 + 1));
        }

        f64::from_bits(TAG_UNDEFINED)
    }
}

/// Check if the map has a key
/// Returns 1 if found, 0 if not found
#[no_mangle]
pub extern "C" fn js_map_has(map: *const MapHeader, key: f64) -> i32 {
    let map = clean_map_ptr(map);
    if map.is_null() {
        return 0;
    }
    let key = normalize_zero(key);
    unsafe {
        if find_key_index(map, key) >= 0 {
            1
        } else {
            0
        }
    }
}

#[no_mangle]
pub extern "C" fn js_map_has_number_key(map: *const MapHeader, key: f64) -> i32 {
    let Some(key) = normalize_number_key_from_boxed(key) else {
        return js_map_has(map, key);
    };
    js_map_has(map, key)
}

#[no_mangle]
pub extern "C" fn js_map_has_string_key(map: *const MapHeader, key: *const StringHeader) -> i32 {
    let map = clean_map_ptr(map);
    if map.is_null() {
        return 0;
    }
    unsafe {
        if find_string_key_index(map, key) >= 0 {
            1
        } else {
            0
        }
    }
}

#[no_mangle]
pub extern "C" fn js_map_delete_string_key(map: *mut MapHeader, key: *const StringHeader) -> i32 {
    let map = clean_map_ptr_mut(map);
    if map.is_null() {
        return 0;
    }
    unsafe {
        let idx = find_string_key_index(map, key);
        delete_entry_at_index(map, idx)
    }
}

#[no_mangle]
pub extern "C" fn js_map_delete_number_key(map: *mut MapHeader, key: f64) -> i32 {
    let Some(key) = normalize_number_key_from_boxed(key) else {
        return js_map_delete(map, key);
    };
    js_map_delete(map, key)
}

// Codegen emits these string-key typed lowering helpers directly from
// generated LLVM IR. Keep roots prevent whole-program LTO/dead-strip from
// removing the exported symbols when the Rust crate graph has no caller.
#[used]
static KEEP_JS_MAP_SET_STRING_NUMBER: extern "C" fn(
    *mut MapHeader,
    *const StringHeader,
    f64,
) -> *mut MapHeader = js_map_set_string_number;
#[used]
static KEEP_JS_MAP_SET_NUMBER_KEY: extern "C" fn(*mut MapHeader, f64, f64) -> *mut MapHeader =
    js_map_set_number_key;
#[used]
static KEEP_JS_MAP_SET_STRING_KEY: extern "C" fn(
    *mut MapHeader,
    *const StringHeader,
    f64,
) -> *mut MapHeader = js_map_set_string_key;
#[used]
static KEEP_JS_MAP_SET_STRING_I32: extern "C" fn(
    *mut MapHeader,
    *const StringHeader,
    i32,
) -> *mut MapHeader = js_map_set_string_i32;
#[used]
static KEEP_JS_MAP_SET_STRING_U32: extern "C" fn(
    *mut MapHeader,
    *const StringHeader,
    u32,
) -> *mut MapHeader = js_map_set_string_u32;
#[used]
static KEEP_JS_MAP_SET_STRING_F32: extern "C" fn(
    *mut MapHeader,
    *const StringHeader,
    f32,
) -> *mut MapHeader = js_map_set_string_f32;
#[used]
static KEEP_JS_MAP_SET_STRING_BOOL: extern "C" fn(
    *mut MapHeader,
    *const StringHeader,
    i32,
) -> *mut MapHeader = js_map_set_string_bool;
#[used]
static KEEP_JS_MAP_SET_STRING_STRING: extern "C" fn(
    *mut MapHeader,
    *const StringHeader,
    *const StringHeader,
) -> *mut MapHeader = js_map_set_string_string;
#[used]
static KEEP_JS_MAP_GET_STRING_KEY: extern "C" fn(*const MapHeader, *const StringHeader) -> f64 =
    js_map_get_string_key;
#[used]
static KEEP_JS_MAP_GET_NUMBER_KEY: extern "C" fn(*const MapHeader, f64) -> f64 =
    js_map_get_number_key;
#[used]
static KEEP_JS_MAP_HAS_STRING_KEY: extern "C" fn(*const MapHeader, *const StringHeader) -> i32 =
    js_map_has_string_key;
#[used]
static KEEP_JS_MAP_HAS_NUMBER_KEY: extern "C" fn(*const MapHeader, f64) -> i32 =
    js_map_has_number_key;
#[used]
static KEEP_JS_MAP_DELETE_STRING_KEY: extern "C" fn(*mut MapHeader, *const StringHeader) -> i32 =
    js_map_delete_string_key;
#[used]
static KEEP_JS_MAP_DELETE_NUMBER_KEY: extern "C" fn(*mut MapHeader, f64) -> i32 =
    js_map_delete_number_key;

/// Delete a key from the map
/// Returns 1 if deleted, 0 if key not found
#[no_mangle]
pub extern "C" fn js_map_delete(map: *mut MapHeader, key: f64) -> i32 {
    let map = clean_map_ptr_mut(map);
    if map.is_null() {
        return 0;
    }
    let key = normalize_zero(key);
    unsafe {
        let idx = find_key_index(map, key);
        delete_entry_at_index(map, idx)
    }
}

unsafe fn delete_entry_at_index(map: *mut MapHeader, idx: i32) -> i32 {
    if idx < 0 {
        return 0;
    }
    let size = (*map).size;
    let idx = idx as usize;
    if idx >= size as usize {
        return 0;
    }
    let entries = entries_ptr_mut(map);

    // #2831: preserve insertion order. JS Map iteration must keep the
    // relative order of surviving entries after a delete (and a
    // delete-then-re-add appends at the end). The previous swap-and-pop
    // moved the last entry into the hole, reordering iteration. Shift
    // every entry after `idx` down by one slot instead.
    for i in idx..(size as usize - 1) {
        let next_key = ptr::read(entries.add((i + 1) * 2));
        let next_value = ptr::read(entries.add((i + 1) * 2 + 1));
        // GC_STORE_AUDIT(EXTERNAL_BARRIERED): map compaction slots use the shared external-slot helper.
        crate::gc::runtime_store_external_jsvalue_slot(
            map as usize,
            entries.add(i * 2) as usize,
            next_key.to_bits(),
        );
        crate::gc::runtime_store_external_jsvalue_slot(
            map as usize,
            entries.add(i * 2 + 1) as usize,
            next_value.to_bits(),
        );
    }

    (*map).size = size - 1;

    // The shift changes the entry index of every surviving key at or
    // after `idx`, so the O(1) lookup side-tables can't be patched in
    // place cheaply. Rebuild them from the compacted buffer.
    rebuild_map_index(map);
    1
}

/// Rebuild the numeric + string lookup side-tables for `map` from its
/// current compacted entries buffer. Used after an order-preserving
/// `delete` shifts entry indexes (#2831).
unsafe fn rebuild_map_index(map: *mut MapHeader) {
    if map.is_null() {
        return;
    }
    let size = (*map).size as usize;
    let capacity = (*map).capacity as usize;
    if size > capacity || size > 16_000_000 || (*map).entries.is_null() {
        return;
    }
    let entries = entries_ptr(map);
    MAP_INDEX.with(|idx| {
        let mut idx = idx.borrow_mut();
        let slot = idx
            .entry(map as usize)
            .or_insert_with(crate::fast_hash::new_ptr_hash_map);
        slot.clear();
        for i in 0..size {
            let key_bits = ptr::read(entries.add(i * 2)).to_bits();
            if is_safe_numeric_key(key_bits) {
                slot.insert(NumericKey(key_bits), i as u32);
            }
        }
    });
    MAP_STRING_INDEX.with(|idx| {
        let mut idx = idx.borrow_mut();
        let slot = idx
            .entry(map as usize)
            .or_insert_with(std::collections::HashMap::new);
        slot.clear();
        for i in 0..size {
            let key_bits = ptr::read(entries.add(i * 2)).to_bits();
            if is_string_like(key_bits) {
                if let Some(h) = string_content_hash(key_bits) {
                    slot.entry(h).or_insert_with(Vec::new).push(i as u32);
                }
            }
        }
    });
}

/// Clear all entries from the map
#[no_mangle]
pub extern "C" fn js_map_clear(map: *mut MapHeader) {
    let map = clean_map_ptr_mut(map);
    if map.is_null() {
        return;
    }
    unsafe {
        (*map).size = 0;
    }
    MAP_INDEX.with(|idx| {
        let mut idx = idx.borrow_mut();
        if let Some(slot) = idx.get_mut(&(map as usize)) {
            slot.clear();
        }
    });
    MAP_STRING_INDEX.with(|idx| {
        let mut idx = idx.borrow_mut();
        if let Some(slot) = idx.get_mut(&(map as usize)) {
            slot.clear();
        }
    });
}

/// Read the key at entry index `idx` of `map`. Used by perry-hir's
/// `for (const [k, v] of mapExpr)` fast path to avoid materializing
/// pair Arrays via `js_map_entries`. Returns `TAG_UNDEFINED` for an
/// out-of-range index or null map; the caller is expected to bound
/// the loop by `js_map_size`.
#[no_mangle]
pub extern "C" fn js_map_entry_key_at(map: *const MapHeader, idx: u32) -> f64 {
    let map = clean_map_ptr(map);
    if map.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    unsafe {
        let size = (*map).size;
        if idx >= size {
            return f64::from_bits(TAG_UNDEFINED);
        }
        let entries = entries_ptr(map);
        ptr::read(entries.add(idx as usize * 2))
    }
}

/// Companion to `js_map_entry_key_at` — read the value at entry index `idx`.
#[no_mangle]
pub extern "C" fn js_map_entry_value_at(map: *const MapHeader, idx: u32) -> f64 {
    let map = clean_map_ptr(map);
    if map.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    unsafe {
        let size = (*map).size;
        if idx >= size {
            return f64::from_bits(TAG_UNDEFINED);
        }
        let entries = entries_ptr(map);
        ptr::read(entries.add(idx as usize * 2 + 1))
    }
}

/// Get the entries of a map as an array of [key, value] pairs
/// Returns an array where each element is a 2-element array [key, value]
#[no_mangle]
pub extern "C" fn js_map_entries(map: *const MapHeader) -> *mut crate::array::ArrayHeader {
    let map = clean_map_ptr(map);
    if map.is_null() {
        return crate::array::js_array_alloc(0);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let map_handle = scope.root_raw_const_ptr(map);
    unsafe {
        let map = map_handle.get_raw_const_ptr::<MapHeader>();
        let size = (*map).size as usize;

        // Outer Array sized exactly to hold N pair pointers — set length
        // up front so we can write directly into the elements buffer
        // instead of going through `js_array_push_f64` per pair.
        let result = crate::array::js_array_alloc_with_length(size as u32);
        let result_handle = scope.root_raw_mut_ptr(result);
        maybe_force_helper_gc_for_test();

        for i in 0..size {
            // Inner pair Array: allocate via js_array_alloc (which floors
            // to MIN_ARRAY_CAPACITY), then write key/value/length directly.
            // Skips the two `js_array_push_f64` calls per pair (each does
            // its own bounds + capacity check).
            let pair = crate::array::js_array_alloc(2);
            let map = map_handle.get_raw_const_ptr::<MapHeader>();
            let entries = entries_ptr(map);
            let key = ptr::read(entries.add(i * 2));
            let value = ptr::read(entries.add(i * 2 + 1));
            // GC_STORE_AUDIT(BARRIERED): pair array key slot uses the shared array slot-store helper.
            crate::array::store_array_slot(pair, 0, key.to_bits());
            // GC_STORE_AUDIT(BARRIERED): pair array value slot uses the shared array slot-store helper.
            crate::array::store_array_slot(pair, 1, value.to_bits());
            (*pair).length = 2;
            crate::array::rebuild_array_layout_exact(pair);

            // Write the NaN-boxed pair pointer directly into the outer
            // array's element slot — no push.
            let pair_boxed = crate::value::js_nanbox_pointer(pair as i64);
            let result = result_handle.get_raw_mut_ptr::<crate::array::ArrayHeader>();
            // GC_STORE_AUDIT(BARRIERED): outer entries array slot uses the shared array slot-store helper.
            crate::array::store_array_slot(result, i, pair_boxed.to_bits());
        }
        let result = result_handle.get_raw_mut_ptr::<crate::array::ArrayHeader>();
        crate::array::rebuild_array_layout_exact(result);

        mark_map_iterator_array(result);
        result
    }
}

/// Get the keys of a map as an array
#[no_mangle]
pub extern "C" fn js_map_keys(map: *const MapHeader) -> *mut crate::array::ArrayHeader {
    let map = clean_map_ptr(map);
    if map.is_null() {
        return crate::array::js_array_alloc(0);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let map_handle = scope.root_raw_const_ptr(map);
    unsafe {
        let map = map_handle.get_raw_const_ptr::<MapHeader>();
        let size = (*map).size as usize;
        let result = crate::array::js_array_alloc(size as u32);
        let result_handle = scope.root_raw_mut_ptr(result);
        maybe_force_helper_gc_for_test();

        for i in 0..size {
            let map = map_handle.get_raw_const_ptr::<MapHeader>();
            let entries = entries_ptr(map);
            let key = ptr::read(entries.add(i * 2));
            let result = result_handle.get_raw_mut_ptr::<crate::array::ArrayHeader>();
            // GC_STORE_AUDIT(BARRIERED): map keys array slot uses the shared array slot-store helper.
            crate::array::store_array_slot(result, i, key.to_bits());
            (*result).length = (i + 1) as u32;
        }

        let result = result_handle.get_raw_mut_ptr::<crate::array::ArrayHeader>();
        mark_map_iterator_array(result);
        result
    }
}

/// Get the values of a map as an array
#[no_mangle]
pub extern "C" fn js_map_values(map: *const MapHeader) -> *mut crate::array::ArrayHeader {
    let map = clean_map_ptr(map);
    if map.is_null() {
        return crate::array::js_array_alloc(0);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let map_handle = scope.root_raw_const_ptr(map);
    unsafe {
        let map = map_handle.get_raw_const_ptr::<MapHeader>();
        let size = (*map).size as usize;
        let result = crate::array::js_array_alloc(size as u32);
        let result_handle = scope.root_raw_mut_ptr(result);
        maybe_force_helper_gc_for_test();

        for i in 0..size {
            let map = map_handle.get_raw_const_ptr::<MapHeader>();
            let entries = entries_ptr(map);
            let value = ptr::read(entries.add(i * 2 + 1));
            let result = result_handle.get_raw_mut_ptr::<crate::array::ArrayHeader>();
            // GC_STORE_AUDIT(BARRIERED): map values array slot uses the shared array slot-store helper.
            crate::array::store_array_slot(result, i, value.to_bits());
            (*result).length = (i + 1) as u32;
        }

        let result = result_handle.get_raw_mut_ptr::<crate::array::ArrayHeader>();
        mark_map_iterator_array(result);
        result
    }
}

/// Copy all entries of a source Map into a freshly-allocated Map.
/// Used by `js_map_from_array` for the `new Map(otherMap)` case: a Map is
/// itself iterable in JS and yields `[key, value]` pairs, so cloning must
/// preserve every entry rather than treat the MapHeader bytes as an
/// ArrayHeader (which read `size`/`capacity` as `length`/`capacity` and
/// produced an empty Map — the root cause of effect's `FiberRefs.updateAs`
/// dropping every fiber-ref except the one being set; see #33/#321).
fn copy_map_into_new(src: *const MapHeader) -> *mut MapHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let src = clean_map_ptr(src);
    if src.is_null() {
        return js_map_alloc(4);
    }
    let src_handle = scope.root_raw_const_ptr(src);
    let size = unsafe {
        let s = src_handle.get_raw_const_ptr::<MapHeader>();
        (*s).size as usize
    };
    let map = js_map_alloc(size.max(4) as u32);
    let map_handle = scope.root_raw_mut_ptr(map);
    for i in 0..size {
        let (key, value) = unsafe {
            let s = src_handle.get_raw_const_ptr::<MapHeader>();
            if i >= (*s).size as usize {
                break;
            }
            let entries = entries_ptr(s);
            (
                ptr::read(entries.add(i * 2)),
                ptr::read(entries.add(i * 2 + 1)),
            )
        };
        let map = map_handle.get_raw_mut_ptr::<MapHeader>();
        js_map_set(map, key, value);
    }
    map_handle.get_raw_mut_ptr::<MapHeader>()
}

/// Create a new Map from an iterable source. Two shapes are supported:
/// - an array of `[key, value]` pair arrays (`new Map([["a", 1]])`), and
/// - another Map (`new Map(otherMap)`), whose entries are copied directly.
///
/// The Map case is detected first because a MapHeader and an ArrayHeader
/// share the same `(u32, u32)` prefix but mean different things, so casting
/// a Map to ArrayHeader silently mis-reads it. Codegen passes the raw
/// (unboxed) pointer here for both `Expr::MapNewFromArray` shapes.
#[no_mangle]
pub extern "C" fn js_map_from_array(arr: *const crate::array::ArrayHeader) -> *mut MapHeader {
    // `new Map(otherMap)`: a Map is iterable and yields [k, v] pairs. The
    // registry check (GcHeader.obj_type fast-path + MAP_REGISTRY) is robust
    // against false positives from the shared header prefix.
    if !arr.is_null() && crate::map::is_registered_map(arr as usize) {
        return copy_map_into_new(arr as *const MapHeader);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let arr_handle = if arr.is_null() {
        None
    } else {
        Some(scope.root_raw_const_ptr(arr))
    };
    let map = js_map_alloc(4);
    let map_handle = scope.root_raw_mut_ptr(map);
    if arr.is_null() {
        return map_handle.get_raw_mut_ptr::<MapHeader>();
    }
    maybe_force_helper_gc_for_test();
    let arr = arr_handle
        .as_ref()
        .expect("non-null array should have a runtime handle")
        .get_raw_const_ptr::<crate::array::ArrayHeader>();
    let len = crate::array::js_array_length(arr);
    for i in 0..len {
        let arr = arr_handle
            .as_ref()
            .expect("non-null array should have a runtime handle")
            .get_raw_const_ptr::<crate::array::ArrayHeader>();
        // Each entry must itself be a 2-element array [key, value].
        // Array elements are stored as f64 NaN-boxed values; nested arrays
        // come through as POINTER_TAG-boxed f64 values.
        let entry_val = crate::array::js_array_get_f64(arr, i);
        let entry_bits = entry_val.to_bits();
        // Extract the inner array pointer (strip NaN-box tag if present).
        let upper = entry_bits >> 48;
        let inner_ptr = if upper == 0x7FFD || upper == 0x7FFF || upper == 0x7FFA {
            // NaN-boxed pointer
            (entry_bits & 0x0000_FFFF_FFFF_FFFF) as *const crate::array::ArrayHeader
        } else if upper == 0x0000 {
            let lower = entry_bits & 0x0000_FFFF_FFFF_FFFF;
            if lower > 0x10000 {
                lower as *const crate::array::ArrayHeader
            } else {
                continue;
            }
        } else {
            continue;
        };
        if inner_ptr.is_null() {
            continue;
        }
        let inner_len = crate::array::js_array_length(inner_ptr);
        if inner_len < 2 {
            continue;
        }
        let key = crate::array::js_array_get_f64(inner_ptr, 0);
        let value = crate::array::js_array_get_f64(inner_ptr, 1);
        let map = map_handle.get_raw_mut_ptr::<MapHeader>();
        js_map_set(map, key, value);
    }
    map_handle.get_raw_mut_ptr::<MapHeader>()
}

/// `new Map(init)` with full `AddEntriesFromIterable` semantics (issue #2770).
///
/// Takes the NaN-boxed init value (not a pre-unboxed array pointer) so it can
/// classify the argument exactly like Node:
/// - `null`/`undefined` → empty Map,
/// - another Map / Set / Array / string / custom iterable → consume its
///   yielded values,
/// - non-iterable (number, boolean, bigint, symbol, function, plain object
///   without `[Symbol.iterator]`) → throw
///   `TypeError: <type> ... is not iterable (...)`.
///
/// Each yielded value must be an *object* (array or plain object). Its `[0]`
/// and `[1]` properties become the entry key/value (missing → `undefined`),
/// so `new Map([['k']])` and `new Map([[]])` keep entries with `undefined`
/// components. A non-object yielded value throws
/// `TypeError: Iterator value <v> is not an entry object`.
///
/// The `new Map(existingMap)` fast path is preserved via `js_for_of_to_array`
/// (Maps materialize to their `[k, v]` pair arrays) inside `classify_init`.
#[no_mangle]
pub extern "C" fn js_map_from_iterable(value: f64) -> *mut MapHeader {
    use crate::collection_iter::{constructor_iter, ConstructorIter};

    let scope = crate::gc::RuntimeHandleScope::new();
    let value_handle = scope.root_nanbox_f64(value);

    let adder = crate::collection_iter::require_callable(
        crate::collection_iter::builtin_prototype_method("Map", "set"),
        "Map.prototype.set",
    );
    let adder = crate::collection_iter::normalize_callable_value(adder);
    let adder_handle = scope.root_nanbox_f64(adder);

    fn add_entry(
        map_handle: crate::gc::RuntimeHandle<'_>,
        adder_handle: crate::gc::RuntimeHandle<'_>,
        entry: f64,
        iter_to_close: Option<f64>,
    ) {
        if !crate::collection_iter::is_entry_object(entry) {
            if let Some(iter) = iter_to_close {
                crate::collection_iter::iterator_close(iter);
            }
            crate::collection_iter::throw_not_entry_object(entry);
        }
        let entry_bits = entry.to_bits() as i64;
        let pair = crate::collection_iter::call_capturing_throw(|| {
            let key = crate::object::js_object_get_index_polymorphic(entry_bits, 0.0);
            let val = crate::object::js_object_get_index_polymorphic(entry_bits, 1.0);
            let args = [key, val];
            let adder = adder_handle.get_nanbox_f64();
            let map = map_handle.get_raw_mut_ptr::<MapHeader>();
            if crate::object::is_builtin_map_set_value(adder) {
                crate::map::js_map_set(map, key, val);
                f64::from_bits(crate::value::TAG_UNDEFINED)
            } else {
                let map_value = crate::value::js_nanbox_pointer(map as i64);
                crate::collection_iter::call_with_this_capturing_throw(adder, map_value, &args)
                    .unwrap_or_else(|exc| crate::exception::js_throw(exc))
            }
        });
        if let Err(exc) = pair {
            if let Some(iter) = iter_to_close {
                crate::collection_iter::iterator_close(iter);
            }
            crate::exception::js_throw(exc);
        }
    }

    match constructor_iter(value_handle.get_nanbox_f64()) {
        ConstructorIter::Empty => js_map_alloc(4),
        ConstructorIter::Array(arr_value) => {
            let arr_handle = scope.root_nanbox_f64(arr_value);
            let map = js_map_alloc(4);
            let map_handle = scope.root_raw_mut_ptr(map);
            let arr_ptr = crate::value::js_nanbox_get_pointer(arr_handle.get_nanbox_f64())
                as *mut crate::array::ArrayHeader;
            if !arr_ptr.is_null() {
                let len = {
                    let arr = crate::value::js_nanbox_get_pointer(arr_handle.get_nanbox_f64())
                        as *const crate::array::ArrayHeader;
                    crate::array::js_array_length(arr)
                };
                for i in 0..len {
                    let entry = {
                        let arr = crate::value::js_nanbox_get_pointer(arr_handle.get_nanbox_f64())
                            as *const crate::array::ArrayHeader;
                        crate::array::js_array_get_f64(arr, i)
                    };
                    add_entry(map_handle, adder_handle, entry, None);
                }
            }
            map_handle.get_raw_mut_ptr::<MapHeader>()
        }
        ConstructorIter::Iterator(iter) => {
            let iter_handle = scope.root_nanbox_f64(iter);
            let map = js_map_alloc(4);
            let map_handle = scope.root_raw_mut_ptr(map);
            loop {
                let iter = iter_handle.get_nanbox_f64();
                let next = crate::collection_iter::iterator_next_value(iter);
                let Some(entry) = next else {
                    break;
                };
                add_entry(map_handle, adder_handle, entry, Some(iter));
            }
            map_handle.get_raw_mut_ptr::<MapHeader>()
        }
    }
}

// #2770: `js_map_from_iterable` is only invoked from generated LLVM IR
// (codegen emits the `new Map(...)` call in
// `perry-codegen/src/expr/misc_methods.rs`), so it has zero internal Rust
// callers. The whole-program auto-optimize bitcode link would otherwise
// internalize + dead-strip the `#[no_mangle]` export and break the default
// compile path. The `#[used]` anchor pins it (see project_auto_optimize_keepalive).
#[used]
static KEEP_JS_MAP_FROM_ITERABLE: extern "C" fn(f64) -> *mut MapHeader = js_map_from_iterable;

/// `Map.prototype.forEach(callback, thisArg)` — calls `callback` with the
/// full `(value, key, map)` argument triple (#2830) and binds `thisArg` as
/// the callback's `this` for non-arrow functions. `this_arg` is `undefined`
/// when omitted at the call site.
#[no_mangle]
pub extern "C" fn js_map_foreach(map: *const MapHeader, callback: f64, this_arg: f64) {
    js_map_foreach_impl(
        map,
        callback,
        this_arg,
        f64::from_bits(crate::value::TAG_UNDEFINED),
    );
}

/// `Map.prototype.forEach` for a `class … extends Map` subclass instance: the
/// 3rd callback argument and the `self === collection` identity must be the
/// SUBCLASS instance (`collection`), not the hidden backing map. The actual
/// iteration runs over `map` (the backing). `collection` is a NaN-boxed value.
pub(crate) fn js_map_foreach_with_collection(
    map: *const MapHeader,
    callback: f64,
    this_arg: f64,
    collection: f64,
) {
    js_map_foreach_impl(map, callback, this_arg, collection);
}

fn js_map_foreach_impl(
    map: *const MapHeader,
    callback: f64,
    this_arg: f64,
    collection_override: f64,
) {
    // ECMA-262 Map.prototype.forEach step 4: a non-callable callback throws a
    // TypeError *before* iterating (and before any null-map early return).
    // Without this, a non-function callback either silently no-ops or — for a
    // numeric value — is dereferenced as a function pointer and segfaults.
    crate::array::js_validate_array_callback(callback);
    let map = clean_map_ptr(map);
    if map.is_null() {
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let map_handle = scope.root_raw_const_ptr(map);
    let callback_handle = scope.root_nanbox_f64(callback);
    let this_handle = scope.root_nanbox_f64(this_arg);
    // When a subclass instance is the observable receiver, root it too so it
    // survives a GC triggered inside the callback.
    let has_override = collection_override.to_bits() != crate::value::TAG_UNDEFINED;
    let collection_handle = scope.root_nanbox_f64(collection_override);
    unsafe {
        let map = map_handle.get_raw_const_ptr::<MapHeader>();
        // The collection itself is the third callback argument and the
        // identity user code compares `self === m` against.
        // ECMA-262 24.1.3.5: forEach iterates [[MapData]] in insertion order,
        // re-reading the live entry count each step. Entries appended during
        // the callback (`map.set` inside the callback) MUST be visited, so the
        // loop bound is re-evaluated against `(*map).size` every iteration
        // rather than snapshotting the initial size — see the
        // `iterates-values-added-after-foreach-begins` / `deleted-values`
        // Test262 cases.
        let mut i = 0usize;
        loop {
            let map = map_handle.get_raw_const_ptr::<MapHeader>();
            if i >= (*map).size as usize {
                break;
            }
            // Re-derive the collection identity each step from a rooted handle
            // so a GC during a prior callback (which may relocate the backing
            // map or the subclass instance) never bakes in a stale pointer.
            let map_value = if has_override {
                collection_handle.get_nanbox_f64()
            } else {
                crate::value::js_nanbox_pointer(map as i64)
            };
            let entries = entries_ptr(map);
            let key = ptr::read(entries.add(i * 2));
            let value = ptr::read(entries.add(i * 2 + 1));
            // Root the visited key so the post-callback slot comparison below
            // stays valid across a GC move during the callback.
            let key_handle = scope.root_nanbox_f64(key);
            let args = [value, key, map_value];
            let cb = callback_handle.get_nanbox_f64();
            let this_v = this_handle.get_nanbox_f64();
            // Bind `thisArg` for the duration of the call (matches the
            // URLSearchParams.forEach pattern); `js_native_call_value`
            // dispatches the NaN-boxed callback with the full arg vector.
            let prev_this = crate::object::js_implicit_this_set(this_v);
            let _ = crate::closure::js_native_call_value(cb, args.as_ptr(), args.len());
            crate::object::js_implicit_this_set(prev_this);
            // Deleting an entry compacts the backing vector (later entries
            // shift left). If the callback deleted the just-visited entry (or
            // an earlier one), slot `i` now holds the NEXT unvisited entry —
            // advancing would skip it (ECMA-262 visits every not-yet-deleted
            // entry; mirrors the `js_set_foreach_impl` fix). Only advance when
            // slot `i` still holds the key just visited.
            let map = map_handle.get_raw_const_ptr::<MapHeader>();
            if i < (*map).size as usize {
                let now_key = ptr::read(entries_ptr(map).add(i * 2));
                if now_key.to_bits() == key_handle.get_nanbox_f64().to_bits() {
                    i += 1;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::string::js_string_from_bytes;

    #[test]
    fn string_number_specialized_helpers_use_string_content_keys() {
        let key_a = js_string_from_bytes(b"score".as_ptr(), 5);
        let key_b = js_string_from_bytes(b"score".as_ptr(), 5);
        assert_ne!(key_a as usize, key_b as usize);

        let map = js_map_alloc(4);
        js_map_set_string_number(map, key_a, 7.5);

        assert_eq!(js_map_size(map), 1);
        assert_eq!(js_map_has_string_key(map, key_b), 1);
        assert_eq!(js_map_get(map, boxed_heap_string_key(key_b)), 7.5);
        assert_eq!(js_map_get_string_key(map, key_b), 7.5);

        js_map_set_string_number(map, key_b, 9.25);
        assert_eq!(
            js_map_size(map),
            1,
            "same-content string keys should update the existing entry"
        );
        assert_eq!(js_map_get(map, boxed_heap_string_key(key_a)), 9.25);
        assert_eq!(js_map_get_string_key(map, key_a), 9.25);

        assert_eq!(js_map_delete_string_key(map, key_b), 1);
        assert_eq!(js_map_size(map), 0);
        assert_eq!(js_map_has_string_key(map, key_a), 0);
        assert_eq!(js_map_get_string_key(map, key_a).to_bits(), TAG_UNDEFINED);
        assert_eq!(js_map_delete_string_key(map, key_a), 0);

        let missing = js_string_from_bytes(b"missing".as_ptr(), 7);
        assert_eq!(js_map_get_string_key(map, missing).to_bits(), TAG_UNDEFINED);

        js_map_set_string_key(map, key_a, f64::from_bits(crate::value::TAG_TRUE));
        assert_eq!(js_map_size(map), 1);
        assert_eq!(
            js_map_get_string_key(map, key_b).to_bits(),
            crate::value::TAG_TRUE
        );

        js_map_set_string_key(map, key_b, f64::from_bits(crate::value::TAG_FALSE));
        assert_eq!(
            js_map_size(map),
            1,
            "same-content string keys should update generic JSValue entries"
        );
        assert_eq!(
            js_map_get_string_key(map, key_a).to_bits(),
            crate::value::TAG_FALSE
        );

        js_map_set_string_bool(map, key_a, 1);
        assert_eq!(js_map_size(map), 1);
        assert_eq!(
            js_map_get_string_key(map, key_b).to_bits(),
            crate::value::TAG_TRUE
        );

        js_map_set_string_bool(map, key_b, 0);
        assert_eq!(
            js_map_size(map),
            1,
            "same-content string keys should update typed boolean entries"
        );
        assert_eq!(
            js_map_get_string_key(map, key_a).to_bits(),
            crate::value::TAG_FALSE
        );

        js_map_set_string_i32(map, key_a, 42);
        assert_eq!(js_map_size(map), 1);
        assert_eq!(
            js_map_get_string_key(map, key_b).to_bits(),
            crate::value::JSValue::int32(42).bits()
        );

        js_map_set_string_i32(map, key_b, -7);
        assert_eq!(
            js_map_size(map),
            1,
            "same-content string keys should update typed int32 entries"
        );
        assert_eq!(
            js_map_get_string_key(map, key_a).to_bits(),
            crate::value::JSValue::int32(-7).bits()
        );

        js_map_set_string_u32(map, key_a, u32::MAX);
        assert_eq!(js_map_size(map), 1);
        assert_eq!(
            js_map_get_string_key(map, key_b).to_bits(),
            (u32::MAX as f64).to_bits()
        );

        js_map_set_string_u32(map, key_b, 4_000_000_000);
        assert_eq!(
            js_map_size(map),
            1,
            "same-content string keys should update typed uint32 entries"
        );
        assert_eq!(
            js_map_get_string_key(map, key_a).to_bits(),
            4_000_000_000_f64.to_bits()
        );

        js_map_set_string_f32(map, key_a, 1.5);
        assert_eq!(js_map_size(map), 1);
        assert_eq!(js_map_get_string_key(map, key_b), 1.5);

        js_map_set_string_f32(map, key_b, -2.25);
        assert_eq!(
            js_map_size(map),
            1,
            "same-content string keys should update typed float32 entries"
        );
        assert_eq!(js_map_get_string_key(map, key_a), -2.25);

        let value_a = js_string_from_bytes(b"ready".as_ptr(), 5);
        let value_b = js_string_from_bytes(b"done".as_ptr(), 4);
        js_map_set_string_string(map, key_a, value_a);
        assert_eq!(js_map_size(map), 1);
        assert_eq!(
            js_map_get_string_key(map, key_b).to_bits(),
            boxed_heap_string_key(value_a).to_bits()
        );

        js_map_set_string_string(map, key_b, value_b);
        assert_eq!(
            js_map_size(map),
            1,
            "same-content string keys should update typed string value entries"
        );
        assert_eq!(
            js_map_get_string_key(map, key_a).to_bits(),
            boxed_heap_string_key(value_b).to_bits()
        );
    }

    #[test]
    fn number_key_specialized_helpers_preserve_numeric_keys_and_fallback() {
        let map = js_map_alloc(4);

        js_map_set_number_key(map, -0.0, 7.5);
        assert_eq!(js_map_size(map), 1);
        assert_eq!(js_map_has_number_key(map, 0.0), 1);
        assert_eq!(js_map_get_number_key(map, 0.0), 7.5);
        assert!(
            test_map_numeric_index_contains(map, 0.0),
            "numeric helper should populate the numeric side-table"
        );

        js_map_set_number_key(map, 0.0, 9.25);
        assert_eq!(
            js_map_size(map),
            1,
            "-0 and +0 should update the same numeric-key entry"
        );
        assert_eq!(js_map_get(map, -0.0), 9.25);
        assert_eq!(js_map_delete_number_key(map, -0.0), 1);
        assert_eq!(js_map_has_number_key(map, 0.0), 0);

        let string_key = js_string_from_bytes(b"fallback".as_ptr(), 8);
        let boxed_string_key = boxed_heap_string_key(string_key);
        js_map_set_number_key(map, boxed_string_key, 13.0);
        assert_eq!(
            js_map_get_number_key(map, boxed_string_key),
            13.0,
            "nonnumeric calls to the numeric helper should preserve generic fallback semantics"
        );
        assert!(
            test_map_string_index_contains(map, boxed_string_key),
            "fallback insertion should still update the string content side-table"
        );
        assert_eq!(js_map_delete_number_key(map, boxed_string_key), 1);
        assert_eq!(js_map_has(map, boxed_string_key), 0);
    }
}
