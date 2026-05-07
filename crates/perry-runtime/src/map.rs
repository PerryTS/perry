//! Map representation for Perry
//!
//! Maps are heap-allocated with a stable header pointer.
//! The entries array is separately allocated and can be reallocated
//! without changing the MapHeader address.

use crate::fast_hash::{new_ptr_hash_set, PtrHashSet};
use crate::string::StringHeader;
use std::alloc::{alloc, realloc, Layout};
use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::ptr;

/// Must match value.rs TAG_UNDEFINED
const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;

thread_local! {
    static MAP_REGISTRY: RefCell<PtrHashSet<usize>> = RefCell::new(new_ptr_hash_set());
}

fn register_map(ptr: *mut MapHeader) {
    MAP_REGISTRY.with(|r| r.borrow_mut().insert(ptr as usize));
}

pub fn is_registered_map(addr: usize) -> bool {
    // Fast pre-filter: gc_malloc'd Maps carry `GcHeader.obj_type ==
    // GC_TYPE_MAP` at `addr - GC_HEADER_SIZE`. A single i8 load + cmp
    // short-circuits the non-Map path (the common case across the
    // typed-dispatch chain `if is_registered_map { ... } else if
    // is_registered_set { ... } ...`) without paying the
    // `HashSet<usize>::contains` SipHash. The HashSet check still runs
    // on byte-matches to defend against:
    //   1. False-positive aliasing — a non-gc_malloc allocation (Set is
    //      raw-alloc'd, BufferHeader for small buffers via a slab) whose
    //      preceding byte happens to read as 8.
    //   2. Stale post-sweep ptrs — drop_map_index removes from
    //      MAP_REGISTRY; the GcHeader byte may persist until the slot
    //      is reused.
    // Profile (samply, perf-comprehensive): ~5.7% inclusive samples
    // were attributed to is_registered_map's HashSet lookup before
    // this fast path landed.
    if addr < 0x1000 + crate::gc::GC_HEADER_SIZE {
        return false;
    }
    unsafe {
        let header = (addr - crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*header).obj_type != crate::gc::GC_TYPE_MAP {
            return false;
        }
    }
    MAP_REGISTRY.with(|r| r.borrow().contains(&addr))
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

/// Side-table mapping `map_ptr -> (NumericKey-bits -> entries-array-index)`.
/// O(1) `find_key_index` for numeric keys; pointer keys still take the
/// linear-scan path so they remain correct under gen-GC string forwarding.
///
/// Both nesting levels use `PtrHasher` (Fibonacci-multiplicative + xorshift
/// avalanche, see `crate::fast_hash`). The xorshift step is essential here
/// because `NumericKey(u64)` holds f64 bit-patterns — small whole-number
/// EntityIds have mantissa-zero, so pure multiplicative hashing would
/// collapse hundreds of keys into bucket 0 (caught by a 2x regression
/// the first time around). With the avalanche step, even the worst-case
/// integer-f64 inputs distribute across buckets normally.
thread_local! {
    static MAP_INDEX: RefCell<
        crate::fast_hash::PtrHashMap<usize, crate::fast_hash::PtrHashMap<NumericKey, u32>>,
    > = RefCell::new(crate::fast_hash::new_ptr_hash_map());
}

/// Drop the side-table entry AND deregister from `MAP_REGISTRY` for a
/// map address that's about to be reused or freed (called from
/// `gc::sweep`). Safe to call on unregistered addresses.
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
    MAP_REGISTRY.with(|r| {
        r.borrow_mut().remove(&addr);
    });
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

/// Map header - stable address, entries allocated separately
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
    } else {
        key
    }
}

/// Check if a value looks like a heap pointer (raw pointer stored in f64)
/// On most systems, heap pointers have small upper bits (0x0000 or close to it)
fn looks_like_pointer(val: f64) -> bool {
    let bits = val.to_bits();
    // Heap pointers on modern systems typically have upper 16 bits as 0x0000
    // and lower 48 bits as the actual address. Addresses above 0x100000000 are typical.
    let upper_16 = bits >> 48;
    let lower_48 = bits & 0x0000_FFFF_FFFF_FFFF;
    // Check if upper bits are 0 (user-space pointer) and lower bits look like a valid address
    upper_16 == 0 && lower_48 > 0x10000
}

/// Extract pointer from raw f64 (for non-NaN-boxed pointers)
fn as_raw_pointer(val: f64) -> *const u8 {
    val.to_bits() as *const u8
}

/// Compare two strings by content
unsafe fn strings_equal(a: *const StringHeader, b: *const StringHeader) -> bool {
    if a.is_null() || b.is_null() || (a as usize) < 0x1000 || (b as usize) < 0x1000 {
        return a == b;
    }
    let len_a = (*a).byte_len;
    let len_b = (*b).byte_len;
    if len_a != len_b {
        return false;
    }
    // Compare content byte by byte
    let data_a = (a as *const u8).add(std::mem::size_of::<StringHeader>());
    let data_b = (b as *const u8).add(std::mem::size_of::<StringHeader>());
    for i in 0..len_a as usize {
        if *data_a.add(i) != *data_b.add(i) {
            return false;
        }
    }
    true
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
fn string_view_from_bits<'a>(
    bits: u64,
    scratch: &'a mut [u8; crate::value::SHORT_STRING_MAX_LEN],
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

    // Allocate header via GC so the GC can trace Map entries (keys/values)
    // and keep gc-allocated strings/arrays/objects alive
    let ptr = crate::gc::gc_malloc(std::mem::size_of::<MapHeader>(), crate::gc::GC_TYPE_MAP)
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
            panic!("Failed to allocate map entries");
        }
        ptr::write_bytes(entries as *mut u8, 0u8, ent_layout.size());

        // Initialize header
        (*ptr).size = 0;
        (*ptr).capacity = cap;
        (*ptr).entries = entries;

        // Register in map registry for runtime type detection
        register_map(ptr);

        // Initialize / reset the O(1) lookup side-table for this address.
        // gc_malloc may recycle a freed Map's GC slot, so a stale index
        // entry from the prior occupant must be cleared here.
        MAP_INDEX.with(|idx| {
            idx.borrow_mut()
                .insert(ptr as usize, crate::fast_hash::new_ptr_hash_map());
        });

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

unsafe fn find_key_index(map: *const MapHeader, key: f64) -> i32 {
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

    // Side-table fast path is restricted to non-pointer keys. String /
    // object / bigint keys still take the linear scan because the
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

    // Linear scan for pointer keys, or maps with no side-table entry.
    let entries = entries_ptr(map);
    for i in 0..size {
        let entry_key = ptr::read(entries.add((i as usize) * 2));
        if jsvalue_eq(entry_key, key) {
            return i as i32;
        }
    }

    -1
}

/// Grow the entries array if needed (header stays at same address)
unsafe fn ensure_capacity(map: *mut MapHeader) {
    let size = (*map).size;
    let capacity = (*map).capacity;

    if size < capacity {
        return;
    }

    // Double the capacity
    let new_capacity = capacity * 2;
    let old_layout = entries_layout(capacity as usize);
    let new_layout = entries_layout(new_capacity as usize);

    let new_entries = realloc((*map).entries as *mut u8, old_layout, new_layout.size()) as *mut f64;
    if new_entries.is_null() {
        panic!("Failed to grow map entries");
    }

    (*map).entries = new_entries;
    (*map).capacity = new_capacity;
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
            ptr::write(entries.add((idx as usize) * 2 + 1), value);
            return map;
        }

        // Key doesn't exist, append a new entry
        ensure_capacity(map);
        let size = (*map).size;
        let entries = entries_ptr_mut(map);

        ptr::write(entries.add((size as usize) * 2), key);
        ptr::write(entries.add((size as usize) * 2 + 1), value);

        (*map).size = size + 1;

        // Update O(1) side-table for numeric keys only. Pointer keys
        // (strings/objects/bigints) stay out so a gen-GC forward of the
        // backing object can't leave stale bits in the index.
        let key_bits = key.to_bits();
        if is_safe_numeric_key(key_bits) {
            MAP_INDEX.with(|idx| {
                let mut idx = idx.borrow_mut();
                let slot = idx
                    .entry(map as usize)
                    .or_insert_with(crate::fast_hash::new_ptr_hash_map);
                slot.insert(NumericKey(key_bits), size);
            });
        }

        map
    }
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

        if idx < 0 {
            return 0;
        }

        let size = (*map).size;
        let entries = entries_ptr_mut(map);

        // Capture the swapped-in key (if any) before writing, so we can
        // patch its position in the side-table after the swap-and-pop.
        let mut swapped_key: Option<f64> = None;
        if (idx as u32) < size - 1 {
            let last_key = ptr::read(entries.add(((size - 1) as usize) * 2));
            let last_value = ptr::read(entries.add(((size - 1) as usize) * 2 + 1));
            ptr::write(entries.add((idx as usize) * 2), last_key);
            ptr::write(entries.add((idx as usize) * 2 + 1), last_value);
            swapped_key = Some(last_key);
        }

        (*map).size = size - 1;

        // Update side-table: drop the deleted key (if it was indexed),
        // and if we swap-popped, patch the swapped key's stored index.
        let key_bits = key.to_bits();
        let swapped_bits = swapped_key.map(|f| f.to_bits());
        if is_safe_numeric_key(key_bits) || swapped_bits.map(is_safe_numeric_key).unwrap_or(false) {
            MAP_INDEX.with(|midx| {
                let mut midx = midx.borrow_mut();
                if let Some(slot) = midx.get_mut(&(map as usize)) {
                    if is_safe_numeric_key(key_bits) {
                        slot.remove(&NumericKey(key_bits));
                    }
                    if let Some(sb) = swapped_bits {
                        if is_safe_numeric_key(sb) {
                            if let Some(entry) = slot.get_mut(&NumericKey(sb)) {
                                *entry = idx as u32;
                            }
                        }
                    }
                }
            });
        }
        1
    }
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
    unsafe {
        let size = (*map).size as usize;
        let entries = entries_ptr(map);

        // Outer Array sized exactly to hold N pair pointers — set length
        // up front so we can write directly into the elements buffer
        // instead of going through `js_array_push_f64` per pair.
        let result = crate::array::js_array_alloc_with_length(size as u32);
        let result_elems =
            (result as *mut u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *mut u64;

        for i in 0..size {
            let key = ptr::read(entries.add(i * 2));
            let value = ptr::read(entries.add(i * 2 + 1));

            // Inner pair Array: allocate via js_array_alloc (which floors
            // to MIN_ARRAY_CAPACITY), then write key/value/length directly.
            // Skips the two `js_array_push_f64` calls per pair (each does
            // its own bounds + capacity check).
            let pair = crate::array::js_array_alloc(2);
            let pair_elems =
                (pair as *mut u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *mut u64;
            std::ptr::write(pair_elems, key.to_bits());
            std::ptr::write(pair_elems.add(1), value.to_bits());
            (*pair).length = 2;

            // Write the NaN-boxed pair pointer directly into the outer
            // array's element slot — no push.
            let pair_boxed = crate::value::js_nanbox_pointer(pair as i64);
            std::ptr::write(result_elems.add(i), pair_boxed.to_bits());
        }

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
    unsafe {
        let size = (*map).size as usize;
        let entries = entries_ptr(map);
        let result = crate::array::js_array_alloc(size as u32);

        for i in 0..size {
            let key = ptr::read(entries.add(i * 2));
            crate::array::js_array_push_f64(result, key);
        }

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
    unsafe {
        let size = (*map).size as usize;
        let entries = entries_ptr(map);
        let result = crate::array::js_array_alloc(size as u32);

        for i in 0..size {
            let value = ptr::read(entries.add(i * 2 + 1));
            crate::array::js_array_push_f64(result, value);
        }

        result
    }
}

/// Create a new Map from an array of [key, value] pair arrays.
/// Used for `new Map([["a", 1], ["b", 2]])` construction.
#[no_mangle]
pub extern "C" fn js_map_from_array(arr: *const crate::array::ArrayHeader) -> *mut MapHeader {
    let map = js_map_alloc(4);
    if arr.is_null() {
        return map;
    }
    unsafe {
        let len = crate::array::js_array_length(arr);
        for i in 0..len {
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
            js_map_set(map, key, value);
        }
    }
    map
}

/// Iterate over map entries, calling a callback with (value, key, map) for each
#[no_mangle]
pub extern "C" fn js_map_foreach(map: *const MapHeader, callback: f64) {
    let map = clean_map_ptr(map);
    if map.is_null() {
        return;
    }
    unsafe {
        let size = (*map).size as usize;
        let entries = entries_ptr(map);

        // Extract the closure pointer from the NaN-boxed callback.
        // The callback may be NaN-boxed with POINTER_TAG (0x7FFD) or
        // passed as a raw pointer (i64 bitcast to f64). Mask off the
        // upper 16 bits to get the real 48-bit pointer.
        let closure_ptr =
            (callback.to_bits() & 0x0000_FFFF_FFFF_FFFF) as *const crate::closure::ClosureHeader;

        for i in 0..size {
            let key = ptr::read(entries.add(i * 2));
            let value = ptr::read(entries.add(i * 2 + 1));
            // Call closure with (value, key) - Map.forEach callback signature
            crate::closure::js_closure_call2(closure_ptr, value, key);
        }
    }
}
