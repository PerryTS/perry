//! Set representation for Perry
//!
//! Sets are arena-allocated GC objects.
//! The elements array is separately allocated and can be reallocated
//! without changing the SetHeader address between GC moves.

use crate::fast_hash::{new_ptr_hash_set, PtrHashSet};
use crate::string::StringHeader;
use std::alloc::{alloc, dealloc, realloc, Layout};
use std::cell::RefCell;
use std::hash::{Hash, Hasher};
use std::ptr;

thread_local! {
    static SET_ITERATOR_ARRAYS: RefCell<PtrHashSet<usize>> = RefCell::new(new_ptr_hash_set());
}

fn mark_set_iterator_array(arr: *mut crate::array::ArrayHeader) {
    if !arr.is_null() {
        SET_ITERATOR_ARRAYS.with(|r| {
            r.borrow_mut().insert(arr as usize);
        });
    }
}

pub fn is_registered_set_iterator(addr: usize) -> bool {
    SET_ITERATOR_ARRAYS.with(|r| r.borrow().contains(&addr))
}

#[cfg(test)]
thread_local! {
    static TEST_FORCE_HELPER_GC: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
pub(crate) fn test_force_next_set_helper_gc() {
    TEST_FORCE_HELPER_GC.with(|force| force.set(true));
}

#[cfg(test)]
fn maybe_force_helper_gc_for_test() {
    let should_collect = TEST_FORCE_HELPER_GC.with(|force| force.replace(false));
    if should_collect {
        let _ = crate::gc::gc_collect_minor();
    }
}

#[cfg(not(test))]
#[inline(always)]
fn maybe_force_helper_gc_for_test() {}

thread_local! {
    static SET_REGISTRY: RefCell<PtrHashSet<usize>> = RefCell::new(new_ptr_hash_set());
}

/// A wrapper around f64 JSValues that implements Hash and Eq using
/// content-based comparison for strings (matching jsvalue_eq semantics).
#[derive(Clone)]
struct JSValueKey(f64);

impl Hash for JSValueKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let bits = self.0.to_bits();
        let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        if is_string_like(bits) {
            if let Some((data, len)) = string_view_from_bits(bits, &mut scratch) {
                // String value: hash by content so identical strings with
                // different representations (heap STRING_TAG / inline SSO /
                // POINTER_TAG / raw pointer) produce the same hash.
                unsafe {
                    // Distinct domain tag so string hashes don't collide
                    // with non-string bit patterns.
                    0xFFFF_FFFFu32.hash(state);
                    len.hash(state);
                    let slice = std::slice::from_raw_parts(data, len as usize);
                    slice.hash(state);
                }
                return;
            }
        }
        bits.hash(state);
    }
}

impl PartialEq for JSValueKey {
    fn eq(&self, other: &Self) -> bool {
        jsvalue_eq(self.0, other.0)
    }
}
impl Eq for JSValueKey {}

// Side-table mapping set_ptr -> (JSValueKey -> index_in_elements).
// Provides O(1) lookup for `find_value_index` instead of O(n) linear scan.
//
// Both nesting levels use `PtrHasher` (Fibonacci-multiplicative + xorshift
// avalanche). Outer key is set heap-pointer; inner is `JSValueKey` whose
// `Hash` impl writes either string-content bytes or f64 bits — the
// avalanche step handles both cleanly. Same rationale as MAP_INDEX
// (commit 39e253cd) — the perry-runtime registries don't need
// SipHash's DoS-resistance for keys that never come from external input.
thread_local! {
    static SET_INDEX: RefCell<
        crate::fast_hash::PtrHashMap<usize, crate::fast_hash::PtrHashMap<JSValueKey, u32>>,
    > = RefCell::new(crate::fast_hash::new_ptr_hash_map());
}

fn register_set(ptr: *mut SetHeader) {
    SET_REGISTRY.with(|r| r.borrow_mut().insert(ptr as usize));
}

pub fn is_registered_set(addr: usize) -> bool {
    // #4004: reject the small-handle band (Web Fetch / node:http / timer ids
    // are NaN-boxed POINTER_TAG values, not heap addresses) before
    // dereferencing the GC header. Managed Sets are arena-allocated above the
    // cutoff. See `value::addr_class` for the band map.
    if crate::value::addr_class::is_handle_band(addr) {
        return false;
    }
    // Registry FIRST: it is authoritative and dereference-free. Probing the
    // GC header before consulting the registry dereferenced `addr - 8` for
    // arbitrary candidate pointers (e.g. garbage read off a TypedArray
    // header by a mis-typed caller) — segfaults on Linux where freed/foreign
    // pages get unmapped (mimalloc on macOS retains them, hiding the bug).
    if !SET_REGISTRY.with(|r| r.borrow().contains(&addr)) {
        return false;
    }
    // A registered address is a live arena Set; the header read is safe and
    // guards against a stale entry whose memory was reused by another type.
    match unsafe { crate::value::addr_class::try_read_gc_header(addr) } {
        Some(header) => header.obj_type == crate::gc::GC_TYPE_SET,
        None => false,
    }
}

/// Resolve a NaN-boxed (or raw-i64) `this` receiver to a registered `Set`
/// pointer, or `None` if the receiver is not a `Set`. Backs the reflective
/// `Set.prototype.*` thunks so they can perform the spec brand check
/// (`TypeError` on a non-`Set` receiver) before dispatching. Mirrors the
/// receiver extraction used by the `Array.prototype.slice` thunk: a
/// NaN-boxed pointer, or a bare raw-i64 pointer some module-init call sites
/// stash in `IMPLICIT_THIS`. Primitives (undefined/null/number/string/bool)
/// carry a non-zero tag in the top 16 bits and resolve to `None`.
pub fn set_ptr_from_receiver_bits(bits: u64) -> Option<*mut SetHeader> {
    let jsv = crate::value::JSValue::from_bits(bits);
    let addr = if jsv.is_pointer() {
        (bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else if bits >> 48 == 0 && bits > 0x10000 {
        bits as usize
    } else {
        return None;
    };
    if is_registered_set(addr) {
        Some(addr as *mut SetHeader)
    } else {
        None
    }
}

#[cfg(test)]
pub(crate) fn test_clear_set_roots() {
    SET_REGISTRY.with(|r| r.borrow_mut().clear());
    SET_INDEX.with(|idx| idx.borrow_mut().clear());
}

pub fn scan_set_roots(_mark: &mut dyn FnMut(f64)) {
    // Set entries are traced through GC_TYPE_SET. Kept as a no-op
    // compatibility shim for callers that still link the legacy scanner.
}

pub fn scan_set_roots_mut(_visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    // Set entries are object-owned external slots, not runtime roots.
}

fn rebuild_set_index(set: *mut SetHeader) {
    if set.is_null() {
        return;
    }
    unsafe {
        let size = (*set).size as usize;
        let capacity = (*set).capacity as usize;
        if size > capacity || size > 16_000_000 || (*set).elements.is_null() {
            return;
        }
        let elements = elements_ptr(set);
        SET_INDEX.with(|idx| {
            let mut idx = idx.borrow_mut();
            let map = idx
                .entry(set as usize)
                .or_insert_with(crate::fast_hash::new_ptr_hash_map);
            map.clear();
            for i in 0..size {
                map.insert(JSValueKey(ptr::read(elements.add(i))), i as u32);
            }
        });
    }
}

pub(crate) fn rebuild_set_index_for_gc(set: *mut SetHeader) {
    rebuild_set_index(set);
}

pub fn drop_set_index(addr: usize) {
    SET_INDEX.with(|idx| {
        idx.borrow_mut().remove(&addr);
    });
    SET_REGISTRY.with(|r| {
        r.borrow_mut().remove(&addr);
    });
}

pub(crate) fn set_header_moved_for_gc(old_addr: usize, new_addr: usize) {
    if old_addr == 0 || new_addr == 0 || old_addr == new_addr {
        return;
    }
    SET_REGISTRY.with(|r| {
        let mut registry = r.borrow_mut();
        if registry.remove(&old_addr) {
            registry.insert(new_addr);
        }
    });
    SET_INDEX.with(|idx| {
        let mut idx = idx.borrow_mut();
        idx.remove(&new_addr);
        if let Some(slot) = idx.remove(&old_addr) {
            idx.insert(new_addr, slot);
        }
    });
}

pub(crate) unsafe fn finalize_set_side_allocation_for_gc(set: *mut SetHeader) {
    if set.is_null() {
        return;
    }
    let addr = set as usize;
    let was_registered = SET_REGISTRY.with(|r| r.borrow_mut().remove(&addr));
    SET_INDEX.with(|idx| {
        idx.borrow_mut().remove(&addr);
    });
    if !was_registered {
        return;
    }

    let elements = (*set).elements;
    let capacity = (*set).capacity as usize;
    if !elements.is_null() && capacity > 0 {
        dealloc(elements as *mut u8, elements_layout(capacity));
        crate::gc::gc_note_external_side_free(elements_layout(capacity).size());
    }
    // GC_STORE_AUDIT(POINTER_FREE): finalizer clears external elements side-allocation pointer after deregistration/deallocation.
    (*set).elements = std::ptr::null_mut();
    (*set).capacity = 0;
    (*set).size = 0;
}

fn is_dead_copied_minor_from_space_set(addr: usize) -> bool {
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
        if (*header).obj_type != crate::gc::GC_TYPE_SET {
            return false;
        }
        let flags = (*header).gc_flags;
        flags & crate::gc::GC_FLAG_ARENA != 0
            && flags & (crate::gc::GC_FLAG_MARKED | crate::gc::GC_FLAG_FORWARDED) == 0
    }
}

/// #6010: registry-driven finalization of DEAD Sets at sweep entry — the Set
/// analog of `finalize_dead_registered_maps_post_trace` (see map.rs for the
/// full rationale: dead collections in the ACTIVE nursery block are never
/// object-walked by any sweeper, leaking their external elements buffers).
pub(crate) fn collect_dead_registered_sets_post_trace(full_trace: bool) -> Vec<usize> {
    SET_REGISTRY.with(|r| {
        r.borrow()
            .iter()
            .copied()
            .filter(|&addr| unsafe { registered_set_is_dead_post_trace(addr, full_trace) })
            .collect()
    })
}

/// Finalize one collected-dead Set (budget-chunked by the sweep state).
pub(crate) fn finalize_collected_dead_set(addr: usize) {
    unsafe {
        finalize_set_side_allocation_for_gc(addr as *mut SetHeader);
    }
}

unsafe fn registered_set_is_dead_post_trace(addr: usize, full_trace: bool) -> bool {
    let Some(header) = crate::value::addr_class::try_read_gc_header(addr) else {
        return false;
    };
    if header.obj_type != crate::gc::GC_TYPE_SET {
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

pub(crate) fn finalize_dead_copied_minor_from_space_sets() -> usize {
    let sets = SET_REGISTRY.with(|r| {
        r.borrow()
            .iter()
            .copied()
            .filter(|&addr| is_dead_copied_minor_from_space_set(addr))
            .collect::<Vec<_>>()
    });
    let count = sets.len();
    for addr in sets {
        unsafe {
            finalize_set_side_allocation_for_gc(addr as *mut SetHeader);
        }
    }
    count
}

#[cfg(test)]
pub(crate) fn test_set_index_contains(set: *const SetHeader, value: f64) -> bool {
    let value = normalize_zero(value);
    SET_INDEX.with(|idx| {
        idx.borrow()
            .get(&(set as usize))
            .is_some_and(|slot| slot.contains_key(&JSValueKey(value)))
    })
}

/// Set header - GC-movable address, elements allocated separately
#[repr(C)]
pub struct SetHeader {
    /// Number of elements in the set
    pub size: u32,
    /// Capacity (allocated space for elements)
    pub capacity: u32,
    /// Pointer to elements array (separately allocated)
    pub elements: *mut f64,
}

/// Each set element is 8 bytes (f64/JSValue)
const ELEMENT_SIZE: usize = 8;

/// Calculate the layout for an elements array with N elements capacity
fn elements_layout(capacity: usize) -> Layout {
    let elements_size = capacity * ELEMENT_SIZE;
    Layout::from_size_align(elements_size.max(8), 8).unwrap()
}

/// Get pointer to elements array
unsafe fn elements_ptr(set: *const SetHeader) -> *const f64 {
    (*set).elements as *const f64
}

/// Get mutable pointer to elements array
unsafe fn elements_ptr_mut(set: *mut SetHeader) -> *mut f64 {
    (*set).elements
}

pub(crate) unsafe fn gc_element_slot_range(
    set: *mut SetHeader,
) -> Option<crate::gc::HeapSlotRange> {
    if set.is_null() {
        return None;
    }
    let size = (*set).size as usize;
    let capacity = (*set).capacity as usize;
    if size > capacity || size > 16_000_000 || (*set).elements.is_null() {
        return None;
    }
    Some(crate::gc::HeapSlotRange::new(
        (*set).elements as *mut u64,
        size,
    ))
}

/// SameValueZero key normalization: -0 → +0.
/// ECMAScript Sets treat -0 and +0 as the same value (23.2.3.1). Without
/// this, `0` (bits 0x0) and `-0` (bits 0x8000_0000_0000_0000) hash/compare
/// as distinct. Non-number JSValues have NaN-box tags in the upper bits,
/// so `v == 0.0` stays false for them.
#[inline(always)]
fn normalize_zero(value: f64) -> f64 {
    if value == 0.0 {
        0.0
    } else if value.is_nan() && crate::value::JSValue::from_bits(value.to_bits()).is_number() {
        // SameValueZero treats every NaN as the same value (23.2.3.x).
        // Canonicalize genuine number NaNs only — `is_number()` excludes
        // NaN-boxed tagged values (objects/strings/bigints).
        f64::NAN
    } else {
        value
    }
}

#[inline(always)]
fn normalize_number_value_from_boxed(value: f64) -> Option<f64> {
    let js_value = crate::value::JSValue::from_bits(value.to_bits());
    if js_value.is_int32() {
        Some(normalize_zero(js_value.as_int32() as f64))
    } else if js_value.is_number() {
        Some(normalize_zero(value))
    } else {
        None
    }
}

/// Compare two strings by content
#[cfg(test)]
unsafe fn strings_equal(a: *const StringHeader, b: *const StringHeader) -> bool {
    if a.is_null() || b.is_null() || (a as usize) < 0x1000 || (b as usize) < 0x1000 {
        return a == b;
    }
    // Fast path: same pointer means same string
    if std::ptr::eq(a, b) {
        return true;
    }
    let len_a = (*a).byte_len;
    let len_b = (*b).byte_len;
    if len_a != len_b {
        return false;
    }
    let data_a = (a as *const u8).add(std::mem::size_of::<StringHeader>());
    let data_b = (b as *const u8).add(std::mem::size_of::<StringHeader>());
    // Use slice comparison which leverages SIMD-optimized memcmp
    let slice_a = std::slice::from_raw_parts(data_a, len_a as usize);
    let slice_b = std::slice::from_raw_parts(data_b, len_a as usize);
    slice_a == slice_b
}

/// Extract a string pointer from a value that might be NaN-boxed with various tags.
/// Does NOT handle SHORT_STRING_TAG (SSO) — those don't carry a heap pointer;
/// use `string_view_from_bits` for representation-agnostic content access.
fn extract_string_ptr_from_value(bits: u64) -> *const StringHeader {
    let upper = bits >> 48;
    match upper {
        0x7FFF => (bits & 0x0000_FFFF_FFFF_FFFF) as *const StringHeader, // STRING_TAG
        0x7FFD => (bits & 0x0000_FFFF_FFFF_FFFF) as *const StringHeader, // POINTER_TAG
        0x0000 => {
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

/// Return a `(ptr, byte_len)` view for any string-like JSValue:
/// - `STRING_TAG` heap strings, `POINTER_TAG`, and raw pointers point at a
///   `StringHeader` and yield its inline data.
/// - `SHORT_STRING_TAG` (SSO) inline values decode their length+bytes into
///   `scratch` and return a pointer into it.
///
/// Returns `None` for non-string values. Callers must keep `scratch` alive
/// for the lifetime of the returned slice in the SSO case.
///
/// Issue #434: pre-fix, `JSValueKey::hash` and `jsvalue_eq` only recognized
/// heap-pointer string representations, so `Set.has(JSON.parse('"input"'))`
/// missed the `"input"` literal stored as STRING_TAG.
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

fn is_string_like(bits: u64) -> bool {
    let upper = bits >> 48;
    if upper == (crate::value::SHORT_STRING_TAG >> 48) {
        return true;
    }
    // STRING_TAG always identifies a string pointee — accept without GC check.
    if upper == 0x7FFF {
        return !extract_string_ptr_from_value(bits).is_null();
    }
    // Issue #549: POINTER_TAG and raw pointers must be GC-validated as strings
    // before we treat them as string-like. Pre-fix, two distinct `{}` objects
    // both got POINTER_TAG, both passed `is_string_like`, and `jsvalue_eq`
    // content-compared them by reinterpreting `ObjectHeader` as `StringHeader`
    // (class_id became byte_len, etc.) — colliding empty objects in `Set.add`.
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

#[inline]
fn boxed_heap_string_value(value: *const StringHeader) -> f64 {
    f64::from_bits(crate::value::STRING_TAG | ((value as u64) & crate::value::POINTER_MASK))
}

/// Check if two JSValues are equal (for set element comparison).
/// Handles STRING_TAG (0x7FFF), POINTER_TAG (0x7FFD), SHORT_STRING_TAG (0x7FF9 SSO),
/// raw pointers, and cross-tag combinations.
fn jsvalue_eq(a: f64, b: f64) -> bool {
    let a_bits = a.to_bits();
    let b_bits = b.to_bits();

    if a_bits == b_bits {
        return true;
    }

    // Symbols compare by identity only (same-symbol caught by the fast path
    // above). A description-less `Symbol()` exposes a zero-length string view,
    // so without this guard it would content-compare equal to the "" key and
    // collide inside Set/Map. (#4570)
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

/// Find the index of a value in the set, or -1 if not found.
/// Uses the O(1) hash index side-table.
unsafe fn find_value_index(set: *const SetHeader, value: f64) -> i32 {
    SET_INDEX.with(|idx| {
        let idx = idx.borrow();
        if let Some(map) = idx.get(&(set as usize)) {
            if let Some(&index) = map.get(&JSValueKey(value)) {
                if index < (*set).size {
                    return index as i32;
                }
            }
        }
        -1
    })
}

/// Grow the elements array if needed (header stays at same address)
unsafe fn ensure_capacity(set: *mut SetHeader) -> bool {
    let size = (*set).size;
    let capacity = (*set).capacity;

    if size < capacity {
        return false;
    }

    // Double the capacity
    let new_capacity = capacity * 2;
    let old_layout = elements_layout(capacity as usize);
    let new_layout = elements_layout(new_capacity as usize);

    let new_elements =
        realloc((*set).elements as *mut u8, old_layout, new_layout.size()) as *mut f64;
    if new_elements.is_null() {
        // #5067 — a constructor-driven `new Set(hugeIterable)` can hit this
        // growth path; surface a catchable RangeError instead of aborting.
        crate::error::throw_allocation_failed();
    }

    // GC_STORE_AUDIT(INIT): set external buffer pointer moves; live slots are dirtied by caller.
    (*set).elements = new_elements;
    (*set).capacity = new_capacity;
    // #6010: growth delta counts as external churn (see js_set_alloc). The
    // header is consistent again, and a triggered cycle is conservative +
    // non-moving, so the caller's raw `set`/elements pointers stay valid.
    crate::gc::gc_note_external_side_alloc(new_layout.size() - old_layout.size());
    true
}

/// Allocate a new empty set with the given initial capacity
#[no_mangle]
pub extern "C" fn js_set_alloc(capacity: u32) -> *mut SetHeader {
    let cap = if capacity == 0 { 4 } else { capacity };
    let elem_layout = elements_layout(cap as usize);
    unsafe {
        let ptr = crate::arena::arena_alloc_gc(
            std::mem::size_of::<SetHeader>(),
            8,
            crate::gc::GC_TYPE_SET,
        ) as *mut SetHeader;
        let elements = alloc(elem_layout) as *mut f64;
        if elements.is_null() {
            // #5067 — catchable RangeError instead of aborting on OOM.
            crate::error::throw_allocation_failed();
        }

        // Initialize header
        (*ptr).size = 0;
        (*ptr).capacity = cap;
        // GC_STORE_AUDIT(INIT): set elements buffer is external storage; element stores are barriered separately.
        (*ptr).elements = elements;

        // Register in set registry for runtime type detection
        register_set(ptr);

        // Initialize O(1) lookup index
        SET_INDEX.with(|idx| {
            idx.borrow_mut()
                .insert(ptr as usize, crate::fast_hash::new_ptr_hash_map());
        });

        // #6010: the elements buffer is invisible to the arena/malloc GC
        // triggers; record its bytes as external churn so Set-heavy
        // workloads still collect (and finalize dead siblings). Safe here:
        // the header is fully initialized + registered, and the triggered
        // cycle is conservative + non-moving, so `ptr` stays valid.
        crate::gc::gc_note_external_side_alloc(elem_layout.size());

        ptr
    }
}

/// Clean a set pointer that might have NaN-box tag bits
#[inline(always)]
fn clean_set_ptr(set: *const SetHeader) -> *const SetHeader {
    let bits = set as u64;
    let top16 = bits >> 48;
    if top16 >= 0x7FF8 {
        if top16 == 0x7FFC || (bits & 0x0000_FFFF_FFFF_FFFF) == 0 {
            return std::ptr::null();
        }
        (bits & 0x0000_FFFF_FFFF_FFFF) as *const SetHeader
    } else {
        set
    }
}

/// Get the number of elements in the set
#[no_mangle]
pub extern "C" fn js_set_size(set: *const SetHeader) -> u32 {
    let set = clean_set_ptr(set);
    if set.is_null() {
        return 0;
    }
    unsafe { (*set).size }
}

/// Add a value to the set
/// Returns the set pointer (always the same, stable address)
#[no_mangle]
pub extern "C" fn js_set_add(set: *mut SetHeader, value: f64) -> *mut SetHeader {
    let value = normalize_zero(value);
    unsafe {
        // Check if value already exists
        let idx = find_value_index(set, value);

        if idx >= 0 {
            // Value already exists, nothing to do
            return set;
        }

        // Value doesn't exist, need to add it
        let grew = ensure_capacity(set);
        let size = (*set).size;
        let elements = elements_ptr_mut(set);
        if grew && size > 0 {
            crate::gc::runtime_dirty_external_slot_span(
                set as usize,
                elements as usize,
                size as usize,
            );
        }

        // Write the value
        // GC_STORE_AUDIT(EXTERNAL_BARRIERED): Set append stores through the shared external-slot helper.
        crate::gc::runtime_store_external_jsvalue_slot(
            set as usize,
            elements.add(size as usize) as usize,
            value.to_bits(),
        );

        // Update the hash index
        SET_INDEX.with(|idx| {
            let mut idx = idx.borrow_mut();
            if let Some(map) = idx.get_mut(&(set as usize)) {
                map.insert(JSValueKey(value), size);
            }
        });

        (*set).size = size + 1;
        set
    }
}

#[no_mangle]
pub extern "C" fn js_set_add_number(set: *mut SetHeader, value: f64) -> *mut SetHeader {
    let Some(value) = normalize_number_value_from_boxed(value) else {
        return js_set_add(set, value);
    };
    js_set_add(set, value)
}

#[no_mangle]
pub extern "C" fn js_set_add_string(
    set: *mut SetHeader,
    value: *const StringHeader,
) -> *mut SetHeader {
    let set = clean_set_ptr(set as *const SetHeader) as *mut SetHeader;
    if set.is_null() {
        return set;
    }
    let value = boxed_heap_string_value(value);
    unsafe {
        let idx = find_value_index(set, value);

        if idx >= 0 {
            return set;
        }

        let grew = ensure_capacity(set);
        let size = (*set).size;
        let elements = elements_ptr_mut(set);
        if grew && size > 0 {
            crate::gc::runtime_dirty_external_slot_span(
                set as usize,
                elements as usize,
                size as usize,
            );
        }

        // GC_STORE_AUDIT(EXTERNAL_BARRIERED): Set append stores through the shared external-slot helper.
        crate::gc::runtime_store_external_jsvalue_slot(
            set as usize,
            elements.add(size as usize) as usize,
            value.to_bits(),
        );

        SET_INDEX.with(|idx| {
            let mut idx = idx.borrow_mut();
            if let Some(map) = idx.get_mut(&(set as usize)) {
                map.insert(JSValueKey(value), size);
            }
        });

        (*set).size = size + 1;
        set
    }
}

#[inline(always)]
fn boxed_i32_value(value: i32) -> f64 {
    f64::from_bits(crate::value::JSValue::int32(value).bits())
}

#[no_mangle]
pub extern "C" fn js_set_add_i32(set: *mut SetHeader, value: i32) -> *mut SetHeader {
    let set = clean_set_ptr(set as *const SetHeader) as *mut SetHeader;
    if set.is_null() {
        return set;
    }
    js_set_add(set, boxed_i32_value(value))
}

#[no_mangle]
pub extern "C" fn js_set_add_u32(set: *mut SetHeader, value: u32) -> *mut SetHeader {
    let set = clean_set_ptr(set as *const SetHeader) as *mut SetHeader;
    if set.is_null() {
        return set;
    }
    js_set_add(set, f64::from(value))
}

#[no_mangle]
pub extern "C" fn js_set_add_f32(set: *mut SetHeader, value: f32) -> *mut SetHeader {
    let set = clean_set_ptr(set as *const SetHeader) as *mut SetHeader;
    if set.is_null() {
        return set;
    }
    js_set_add(set, f64::from(value))
}

#[no_mangle]
pub extern "C" fn js_set_add_bool(set: *mut SetHeader, value: i32) -> *mut SetHeader {
    let set = clean_set_ptr(set as *const SetHeader) as *mut SetHeader;
    if set.is_null() {
        return set;
    }
    let boxed = if value != 0 {
        f64::from_bits(crate::value::TAG_TRUE)
    } else {
        f64::from_bits(crate::value::TAG_FALSE)
    };
    js_set_add(set, boxed)
}

/// Check if the set has a value
/// Returns 1 if found, 0 if not found
#[no_mangle]
pub extern "C" fn js_set_has(set: *const SetHeader, value: f64) -> i32 {
    let value = normalize_zero(value);
    unsafe {
        if find_value_index(set, value) >= 0 {
            1
        } else {
            0
        }
    }
}

#[no_mangle]
pub extern "C" fn js_set_has_number(set: *const SetHeader, value: f64) -> i32 {
    let Some(value) = normalize_number_value_from_boxed(value) else {
        return js_set_has(set, value);
    };
    js_set_has(set, value)
}

#[no_mangle]
pub extern "C" fn js_set_has_string(set: *const SetHeader, value: *const StringHeader) -> i32 {
    let set = clean_set_ptr(set);
    if set.is_null() {
        return 0;
    }
    let value = boxed_heap_string_value(value);
    unsafe {
        if find_value_index(set, value) >= 0 {
            1
        } else {
            0
        }
    }
}

#[no_mangle]
pub extern "C" fn js_set_has_i32(set: *const SetHeader, value: i32) -> i32 {
    let set = clean_set_ptr(set);
    if set.is_null() {
        return 0;
    }
    js_set_has(set, boxed_i32_value(value))
}

#[no_mangle]
pub extern "C" fn js_set_has_u32(set: *const SetHeader, value: u32) -> i32 {
    let set = clean_set_ptr(set);
    if set.is_null() {
        return 0;
    }
    js_set_has(set, f64::from(value))
}

#[no_mangle]
pub extern "C" fn js_set_has_f32(set: *const SetHeader, value: f32) -> i32 {
    let set = clean_set_ptr(set);
    if set.is_null() {
        return 0;
    }
    js_set_has(set, f64::from(value))
}

#[no_mangle]
pub extern "C" fn js_set_has_bool(set: *const SetHeader, value: i32) -> i32 {
    let set = clean_set_ptr(set);
    if set.is_null() {
        return 0;
    }
    let boxed = if value != 0 {
        f64::from_bits(crate::value::TAG_TRUE)
    } else {
        f64::from_bits(crate::value::TAG_FALSE)
    };
    js_set_has(set, boxed)
}

/// Delete a value from the set
/// Returns 1 if deleted, 0 if value not found
#[no_mangle]
pub extern "C" fn js_set_delete(set: *mut SetHeader, value: f64) -> i32 {
    let value = normalize_zero(value);
    unsafe {
        let idx = find_value_index(set, value);

        if idx < 0 {
            return 0;
        }

        let size = (*set).size;
        let elements = elements_ptr_mut(set);

        // #2831: preserve insertion order. The previous swap-remove moved
        // the last element into the hole, reordering iteration. Shift every
        // element after `idx` down by one slot instead so survivors keep
        // their relative order (and a delete-then-re-add appends at the end).
        for i in (idx as usize)..(size as usize - 1) {
            let next_value = ptr::read(elements.add(i + 1));
            // GC_STORE_AUDIT(EXTERNAL_BARRIERED): Set compaction stores through the shared external-slot helper.
            crate::gc::runtime_store_external_jsvalue_slot(
                set as usize,
                elements.add(i) as usize,
                next_value.to_bits(),
            );
        }

        (*set).size = size - 1;

        // The shift changes the stored index of every surviving element at
        // or after `idx`, so rebuild the O(1) lookup index from the
        // compacted buffer.
        rebuild_set_index(set);
        1
    }
}

#[no_mangle]
pub extern "C" fn js_set_delete_number(set: *mut SetHeader, value: f64) -> i32 {
    let Some(value) = normalize_number_value_from_boxed(value) else {
        return js_set_delete(set, value);
    };
    js_set_delete(set, value)
}

#[no_mangle]
pub extern "C" fn js_set_delete_string(set: *mut SetHeader, value: *const StringHeader) -> i32 {
    let set = clean_set_ptr(set as *const SetHeader) as *mut SetHeader;
    if set.is_null() {
        return 0;
    }
    let value = boxed_heap_string_value(value);
    js_set_delete(set, value)
}

#[no_mangle]
pub extern "C" fn js_set_delete_i32(set: *mut SetHeader, value: i32) -> i32 {
    let set = clean_set_ptr(set as *const SetHeader) as *mut SetHeader;
    if set.is_null() {
        return 0;
    }
    js_set_delete(set, boxed_i32_value(value))
}

#[no_mangle]
pub extern "C" fn js_set_delete_u32(set: *mut SetHeader, value: u32) -> i32 {
    let set = clean_set_ptr(set as *const SetHeader) as *mut SetHeader;
    if set.is_null() {
        return 0;
    }
    js_set_delete(set, f64::from(value))
}

#[no_mangle]
pub extern "C" fn js_set_delete_f32(set: *mut SetHeader, value: f32) -> i32 {
    let set = clean_set_ptr(set as *const SetHeader) as *mut SetHeader;
    if set.is_null() {
        return 0;
    }
    js_set_delete(set, f64::from(value))
}

#[no_mangle]
pub extern "C" fn js_set_delete_bool(set: *mut SetHeader, value: i32) -> i32 {
    let set = clean_set_ptr(set as *const SetHeader) as *mut SetHeader;
    if set.is_null() {
        return 0;
    }
    let boxed = if value != 0 {
        f64::from_bits(crate::value::TAG_TRUE)
    } else {
        f64::from_bits(crate::value::TAG_FALSE)
    };
    js_set_delete(set, boxed)
}

// Codegen emits these string-key typed lowering helpers directly from
// generated LLVM IR. Keep roots prevent whole-program LTO/dead-strip from
// removing the exported symbols when the Rust crate graph has no caller.
#[used]
static KEEP_JS_SET_ADD_STRING: extern "C" fn(
    *mut SetHeader,
    *const StringHeader,
) -> *mut SetHeader = js_set_add_string;
#[used]
static KEEP_JS_SET_ADD_NUMBER: extern "C" fn(*mut SetHeader, f64) -> *mut SetHeader =
    js_set_add_number;
#[used]
static KEEP_JS_SET_HAS_STRING: extern "C" fn(*const SetHeader, *const StringHeader) -> i32 =
    js_set_has_string;
#[used]
static KEEP_JS_SET_HAS_NUMBER: extern "C" fn(*const SetHeader, f64) -> i32 = js_set_has_number;
#[used]
static KEEP_JS_SET_DELETE_STRING: extern "C" fn(*mut SetHeader, *const StringHeader) -> i32 =
    js_set_delete_string;
#[used]
static KEEP_JS_SET_DELETE_NUMBER: extern "C" fn(*mut SetHeader, f64) -> i32 = js_set_delete_number;
#[used]
static KEEP_JS_SET_ADD_I32: extern "C" fn(*mut SetHeader, i32) -> *mut SetHeader = js_set_add_i32;
#[used]
static KEEP_JS_SET_HAS_I32: extern "C" fn(*const SetHeader, i32) -> i32 = js_set_has_i32;
#[used]
static KEEP_JS_SET_DELETE_I32: extern "C" fn(*mut SetHeader, i32) -> i32 = js_set_delete_i32;
#[used]
static KEEP_JS_SET_ADD_U32: extern "C" fn(*mut SetHeader, u32) -> *mut SetHeader = js_set_add_u32;
#[used]
static KEEP_JS_SET_HAS_U32: extern "C" fn(*const SetHeader, u32) -> i32 = js_set_has_u32;
#[used]
static KEEP_JS_SET_DELETE_U32: extern "C" fn(*mut SetHeader, u32) -> i32 = js_set_delete_u32;
#[used]
static KEEP_JS_SET_ADD_F32: extern "C" fn(*mut SetHeader, f32) -> *mut SetHeader = js_set_add_f32;
#[used]
static KEEP_JS_SET_HAS_F32: extern "C" fn(*const SetHeader, f32) -> i32 = js_set_has_f32;
#[used]
static KEEP_JS_SET_DELETE_F32: extern "C" fn(*mut SetHeader, f32) -> i32 = js_set_delete_f32;
#[used]
static KEEP_JS_SET_ADD_BOOL: extern "C" fn(*mut SetHeader, i32) -> *mut SetHeader = js_set_add_bool;
#[used]
static KEEP_JS_SET_HAS_BOOL: extern "C" fn(*const SetHeader, i32) -> i32 = js_set_has_bool;
#[used]
static KEEP_JS_SET_DELETE_BOOL: extern "C" fn(*mut SetHeader, i32) -> i32 = js_set_delete_bool;

/// Clear all elements from the set
#[no_mangle]
pub extern "C" fn js_set_clear(set: *mut SetHeader) {
    unsafe {
        (*set).size = 0;
    }
    SET_INDEX.with(|idx| {
        let mut idx = idx.borrow_mut();
        if let Some(map) = idx.get_mut(&(set as usize)) {
            map.clear();
        }
    });
}

/// Direct read of the i-th element in insertion order — counterpart to
/// `js_map_entry_value_at`. Lets the codegen iterate `for (const x of set)`
/// without materializing the elements buffer into a transient Array via
/// `js_set_to_array`. Returns `undefined` (NaN-box) for out-of-range / null.
#[no_mangle]
pub extern "C" fn js_set_value_at(set: *const SetHeader, i: u32) -> f64 {
    const UNDEF: u64 = 0x7FFC_0000_0000_0001;
    let set = clean_set_ptr(set);
    if set.is_null() {
        return f64::from_bits(UNDEF);
    }
    unsafe {
        if i >= (*set).size {
            return f64::from_bits(UNDEF);
        }
        let elements = (*set).elements as *const f64;
        ptr::read(elements.add(i as usize))
    }
}

/// Convert a Set to an Array (for Array.from(set))
/// Returns a new array containing all elements of the set.
///
/// Bulk path: for non-empty sets we pre-allocate an array with capacity
/// = set size and `memcpy` the f64s directly, then set `length = size`
/// in one shot. The previous loop did N `js_array_push_f64` calls
/// (each chasing a forwarding pointer + checking capacity + bumping
/// length); on a 1.25M-element set the per-push overhead added ~70 ms
/// to `[...set]` (vs Bun's 2 ms). The bulk copy is correctness-safe
/// because we own the freshly-allocated destination — no aliasing,
/// no concurrent modification, capacity is exact.
#[no_mangle]
pub extern "C" fn js_set_to_array(set: *const SetHeader) -> *mut crate::array::ArrayHeader {
    if set.is_null() {
        return crate::array::js_array_alloc(0);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let set_handle = scope.root_raw_const_ptr(set);
    unsafe {
        let set = set_handle.get_raw_const_ptr::<SetHeader>();
        let size = (*set).size as usize;
        let result = crate::array::js_array_alloc(size as u32);
        let result_handle = scope.root_raw_mut_ptr(result);
        maybe_force_helper_gc_for_test();
        if size > 0 {
            let set = set_handle.get_raw_const_ptr::<SetHeader>();
            let result = result_handle.get_raw_mut_ptr::<crate::array::ArrayHeader>();
            let src = (*set).elements as *const f64;
            let dst = (result as *mut u8).add(std::mem::size_of::<crate::array::ArrayHeader>())
                as *mut f64;
            // GC_STORE_AUDIT(BARRIERED): Set-to-array bulk copy is followed by exact layout/barrier rebuild.
            ptr::copy_nonoverlapping(src, dst, size);
            (*result).length = size as u32;
            crate::array::rebuild_array_layout_exact(result);
        }
        let result = result_handle.get_raw_mut_ptr::<crate::array::ArrayHeader>();
        mark_set_iterator_array(result);
        result
    }
}

/// Create a Set from an Array (for `new Set(array)`)
/// Takes an ArrayHeader pointer and adds all elements to a new Set
#[no_mangle]
pub extern "C" fn js_set_from_array(arr: *const crate::array::ArrayHeader) -> *mut SetHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let arr_handle = if arr.is_null() {
        None
    } else {
        Some(scope.root_raw_const_ptr(arr))
    };
    let set = js_set_alloc(4);
    let set_handle = scope.root_raw_mut_ptr(set);
    if arr.is_null() {
        return set_handle.get_raw_mut_ptr::<SetHeader>();
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
        let element = crate::array::js_array_get_f64(arr, i);
        let set = set_handle.get_raw_mut_ptr::<SetHeader>();
        js_set_add(set, element);
    }
    set_handle.get_raw_mut_ptr::<SetHeader>()
}

/// Create a Set from any iterable JS value (for `new Set(iter)`).
///
/// Takes a NaN-boxed value and dispatches by tag:
/// - Array (POINTER_TAG over ArrayHeader) → iterate elements (delegates
///   to `js_set_from_array`)
/// - String (STRING_TAG or SHORT_STRING_TAG) → iterate codepoints,
///   adding each as a single-char string to the set (matches the JS
///   spec: `new Set("abc")` → `{"a", "b", "c"}`)
/// - undefined / null → empty set (matches `new Set()` and `new Set(null)`)
/// - any other iterable (Map/Set/custom `[Symbol.iterator]`) → consume its
///   yielded values
/// - non-iterable number / boolean / bigint / symbol / function / plain
///   object → throw `TypeError: <type> ... is not iterable (...)` (#2771)
///
/// Used by codegen for `Expr::SetNewFromArray` so a single call site
/// handles any iterable input shape — closes #421-related bug where
/// `new Set("abc")` (hono's `regExpMetaChars` pattern) crashed because
/// `js_set_from_array` blindly cast the StringHeader to ArrayHeader, and
/// #2771 (arbitrary iterables + TypeError on non-iterables).
#[no_mangle]
pub extern "C" fn js_set_from_iterable(value: f64) -> *mut SetHeader {
    use crate::collection_iter::{constructor_iter, ConstructorIter};

    let scope = crate::gc::RuntimeHandleScope::new();
    let value_handle = scope.root_nanbox_f64(value);
    let adder = crate::collection_iter::require_callable(
        crate::collection_iter::builtin_prototype_method("Set", "add"),
        "Set.prototype.add",
    );
    let adder = crate::collection_iter::normalize_callable_value(adder);
    let adder_handle = scope.root_nanbox_f64(adder);

    let set = js_set_alloc(4);
    let set_handle = scope.root_raw_mut_ptr(set);

    let add_value = |element: f64, iter_to_close: Option<f64>| {
        let args = [element];
        let adder = adder_handle.get_nanbox_f64();
        let set = set_handle.get_raw_mut_ptr::<SetHeader>();
        let result = if crate::object::is_builtin_set_add_value(adder) {
            crate::set::js_set_add(set, element);
            Ok(f64::from_bits(crate::value::TAG_UNDEFINED))
        } else {
            let set_value = crate::value::js_nanbox_pointer(set as i64);
            crate::collection_iter::call_with_this_capturing_throw(adder, set_value, &args)
        };
        if let Err(exc) = result {
            if let Some(iter) = iter_to_close {
                crate::collection_iter::iterator_close(iter);
            }
            crate::exception::js_throw(exc);
        }
    };

    match constructor_iter(value_handle.get_nanbox_f64()) {
        ConstructorIter::Empty => {}
        ConstructorIter::Array(arr_value) => {
            let arr_handle = scope.root_nanbox_f64(arr_value);
            let arr_ptr = crate::value::js_nanbox_get_pointer(arr_handle.get_nanbox_f64())
                as *mut crate::array::ArrayHeader;
            if !arr_ptr.is_null() {
                maybe_force_helper_gc_for_test();
                let len = {
                    let arr = crate::value::js_nanbox_get_pointer(arr_handle.get_nanbox_f64())
                        as *const crate::array::ArrayHeader;
                    crate::array::js_array_length(arr)
                };
                for i in 0..len {
                    let element = {
                        let arr = crate::value::js_nanbox_get_pointer(arr_handle.get_nanbox_f64())
                            as *const crate::array::ArrayHeader;
                        crate::array::js_array_get_f64(arr, i)
                    };
                    add_value(element, None);
                }
            }
        }
        ConstructorIter::Iterator(iter) => {
            let iter_handle = scope.root_nanbox_f64(iter);
            loop {
                let iter = iter_handle.get_nanbox_f64();
                let next = crate::collection_iter::iterator_next_value(iter);
                let Some(element) = next else {
                    break;
                };
                add_value(element, Some(iter));
            }
        }
    }
    set_handle.get_raw_mut_ptr::<SetHeader>()
}

/// `Set.prototype.forEach(callback, thisArg)` — calls `callback` with
/// `(value, value, set)` (key === value for Sets, #2830) and binds
/// `thisArg` as the callback's `this`. `this_arg` is `undefined` when
/// omitted at the call site.
#[no_mangle]
pub extern "C" fn js_set_foreach(set: *const SetHeader, callback: f64, this_arg: f64) {
    js_set_foreach_impl(
        set,
        callback,
        this_arg,
        f64::from_bits(crate::value::TAG_UNDEFINED),
    );
}

/// `Set.prototype.forEach` for a `class … extends Set` subclass instance: the
/// 3rd callback argument and the `self === collection` identity must be the
/// SUBCLASS instance (`collection`), not the hidden backing set. Iteration runs
/// over `set` (the backing). `collection` is a NaN-boxed value.
pub(crate) fn js_set_foreach_with_collection(
    set: *const SetHeader,
    callback: f64,
    this_arg: f64,
    collection: f64,
) {
    js_set_foreach_impl(set, callback, this_arg, collection);
}

fn js_set_foreach_impl(
    set: *const SetHeader,
    callback: f64,
    this_arg: f64,
    collection_override: f64,
) {
    // ECMA-262 Set.prototype.forEach step 4: a non-callable callback throws a
    // TypeError before iterating (and before any null-set early return).
    crate::array::js_validate_array_callback(callback);
    let set = clean_set_ptr(set);
    if set.is_null() {
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let set_handle = scope.root_raw_const_ptr(set);
    let callback_handle = scope.root_nanbox_f64(callback);
    let this_handle = scope.root_nanbox_f64(this_arg);
    let has_override = collection_override.to_bits() != crate::value::TAG_UNDEFINED;
    let collection_handle = scope.root_nanbox_f64(collection_override);
    unsafe {
        // ECMA-262 24.2.3.6: Set.prototype.forEach iterates [[SetData]] in
        // insertion order, re-reading the live entry count each step. Entries
        // appended during the callback (`set.add` inside the callback) MUST be
        // visited, so the loop bound is re-evaluated against `(*set).size` every
        // iteration rather than snapshotting the initial size — mirrors
        // `js_map_foreach_impl`.
        let mut i = 0usize;
        loop {
            let set = set_handle.get_raw_const_ptr::<SetHeader>();
            if i >= (*set).size as usize {
                break;
            }
            // The Set itself is the third callback argument / `self === s`.
            // Re-derive each step from a rooted handle so a GC during a prior
            // callback never bakes in a stale (relocated) pointer.
            let set_value = if has_override {
                collection_handle.get_nanbox_f64()
            } else {
                crate::value::js_nanbox_pointer(set as i64)
            };
            let elements = elements_ptr(set);
            let value = ptr::read(elements.add(i));
            let args = [value, value, set_value];
            let cb = callback_handle.get_nanbox_f64();
            let this_v = this_handle.get_nanbox_f64();
            let prev_this = crate::object::js_implicit_this_set(this_v);
            let _ = crate::closure::js_native_call_value(cb, args.as_ptr(), args.len());
            crate::object::js_implicit_this_set(prev_this);
            i += 1;
        }
    }
}

// =====================================================================
// ES2024 Set composition methods (TC39 Set-methods proposal, Node 22+).
// #2872: union / intersection / difference / symmetricDifference return a
// NEW Set; isSubsetOf / isSupersetOf / isDisjointFrom return a boolean.
//
// All four set-returning methods preserve insertion order per spec:
// `union` is `this` elements followed by `other`'s new elements;
// `intersection` / `difference` keep `this`'s order; `symmetricDifference`
// is `this`'s leftover elements followed by `other`'s leftover elements.
//
// The `other` argument is received as a NaN-boxed value (f64). For the
// scoped basic behavior (#2872) it is expected to be a real Set; we
// recover its pointer via `clean_set_ptr` (which masks the NaN-box tag).
// =====================================================================

/// Recover a `*const SetHeader` from a NaN-boxed `other` argument. Returns
/// null if the value isn't a registered Set (callers treat null as empty).
unsafe fn other_set_ptr(other: f64) -> *const SetHeader {
    let raw = other.to_bits() as *const SetHeader;
    let cleaned = clean_set_ptr(raw);
    if cleaned.is_null() {
        return ptr::null();
    }
    if is_registered_set(cleaned as usize) {
        cleaned
    } else {
        ptr::null()
    }
}

/// `Set.prototype.union(other)` — new Set with elements of both sets,
/// `this`'s elements first (insertion order) then `other`'s new elements.
#[no_mangle]
pub extern "C" fn js_set_union(set: *const SetHeader, other: f64) -> *mut SetHeader {
    let set = clean_set_ptr(set);
    let scope = crate::gc::RuntimeHandleScope::new();
    let result = js_set_alloc(4);
    let result_handle = scope.root_raw_mut_ptr(result);
    let set_handle = if set.is_null() {
        None
    } else {
        Some(scope.root_raw_const_ptr(set))
    };
    let other_handle = unsafe {
        let o = other_set_ptr(other);
        if o.is_null() {
            None
        } else {
            Some(scope.root_raw_const_ptr(o))
        }
    };
    unsafe {
        if let Some(ref h) = set_handle {
            let s = h.get_raw_const_ptr::<SetHeader>();
            for i in 0..(*s).size as usize {
                let s = h.get_raw_const_ptr::<SetHeader>();
                let v = ptr::read(elements_ptr(s).add(i));
                js_set_add(result_handle.get_raw_mut_ptr::<SetHeader>(), v);
            }
        }
        if let Some(ref h) = other_handle {
            let o = h.get_raw_const_ptr::<SetHeader>();
            for i in 0..(*o).size as usize {
                let o = h.get_raw_const_ptr::<SetHeader>();
                let v = ptr::read(elements_ptr(o).add(i));
                js_set_add(result_handle.get_raw_mut_ptr::<SetHeader>(), v);
            }
        }
    }
    result_handle.get_raw_mut_ptr::<SetHeader>()
}

/// `Set.prototype.intersection(other)` — new Set with elements present in
/// both sets, keeping `this`'s insertion order.
#[no_mangle]
pub extern "C" fn js_set_intersection(set: *const SetHeader, other: f64) -> *mut SetHeader {
    let set = clean_set_ptr(set);
    let scope = crate::gc::RuntimeHandleScope::new();
    let result = js_set_alloc(4);
    let result_handle = scope.root_raw_mut_ptr(result);
    if set.is_null() {
        return result_handle.get_raw_mut_ptr::<SetHeader>();
    }
    let set_handle = scope.root_raw_const_ptr(set);
    let other = unsafe { other_set_ptr(other) };
    let other_handle = if other.is_null() {
        None
    } else {
        Some(scope.root_raw_const_ptr(other))
    };
    unsafe {
        let s = set_handle.get_raw_const_ptr::<SetHeader>();
        for i in 0..(*s).size as usize {
            let s = set_handle.get_raw_const_ptr::<SetHeader>();
            let v = ptr::read(elements_ptr(s).add(i));
            let in_other = match other_handle {
                Some(ref h) => {
                    let o = h.get_raw_const_ptr::<SetHeader>();
                    js_set_has(o, v) != 0
                }
                None => false,
            };
            if in_other {
                js_set_add(result_handle.get_raw_mut_ptr::<SetHeader>(), v);
            }
        }
    }
    result_handle.get_raw_mut_ptr::<SetHeader>()
}

/// `Set.prototype.difference(other)` — new Set with elements of `this` that
/// are NOT in `other`, keeping `this`'s insertion order.
#[no_mangle]
pub extern "C" fn js_set_difference(set: *const SetHeader, other: f64) -> *mut SetHeader {
    let set = clean_set_ptr(set);
    let scope = crate::gc::RuntimeHandleScope::new();
    let result = js_set_alloc(4);
    let result_handle = scope.root_raw_mut_ptr(result);
    if set.is_null() {
        return result_handle.get_raw_mut_ptr::<SetHeader>();
    }
    let set_handle = scope.root_raw_const_ptr(set);
    let other = unsafe { other_set_ptr(other) };
    let other_handle = if other.is_null() {
        None
    } else {
        Some(scope.root_raw_const_ptr(other))
    };
    unsafe {
        let s = set_handle.get_raw_const_ptr::<SetHeader>();
        for i in 0..(*s).size as usize {
            let s = set_handle.get_raw_const_ptr::<SetHeader>();
            let v = ptr::read(elements_ptr(s).add(i));
            let in_other = match other_handle {
                Some(ref h) => {
                    let o = h.get_raw_const_ptr::<SetHeader>();
                    js_set_has(o, v) != 0
                }
                None => false,
            };
            if !in_other {
                js_set_add(result_handle.get_raw_mut_ptr::<SetHeader>(), v);
            }
        }
    }
    result_handle.get_raw_mut_ptr::<SetHeader>()
}

/// `Set.prototype.symmetricDifference(other)` — new Set with elements in
/// exactly one of the sets. `this`'s leftover elements (not in `other`)
/// first, then `other`'s leftover elements (not in `this`).
#[no_mangle]
pub extern "C" fn js_set_symmetric_difference(set: *const SetHeader, other: f64) -> *mut SetHeader {
    let set = clean_set_ptr(set);
    let scope = crate::gc::RuntimeHandleScope::new();
    let result = js_set_alloc(4);
    let result_handle = scope.root_raw_mut_ptr(result);
    let set_handle = if set.is_null() {
        None
    } else {
        Some(scope.root_raw_const_ptr(set))
    };
    let other = unsafe { other_set_ptr(other) };
    let other_handle = if other.is_null() {
        None
    } else {
        Some(scope.root_raw_const_ptr(other))
    };
    unsafe {
        // `this` elements not in `other`.
        if let Some(ref sh) = set_handle {
            let s = sh.get_raw_const_ptr::<SetHeader>();
            for i in 0..(*s).size as usize {
                let s = sh.get_raw_const_ptr::<SetHeader>();
                let v = ptr::read(elements_ptr(s).add(i));
                let in_other = match other_handle {
                    Some(ref h) => {
                        let o = h.get_raw_const_ptr::<SetHeader>();
                        js_set_has(o, v) != 0
                    }
                    None => false,
                };
                if !in_other {
                    js_set_add(result_handle.get_raw_mut_ptr::<SetHeader>(), v);
                }
            }
        }
        // `other` elements not in `this`.
        if let Some(ref oh) = other_handle {
            let o = oh.get_raw_const_ptr::<SetHeader>();
            for i in 0..(*o).size as usize {
                let o = oh.get_raw_const_ptr::<SetHeader>();
                let v = ptr::read(elements_ptr(o).add(i));
                let in_set = match set_handle {
                    Some(ref h) => {
                        let s = h.get_raw_const_ptr::<SetHeader>();
                        js_set_has(s, v) != 0
                    }
                    None => false,
                };
                if !in_set {
                    js_set_add(result_handle.get_raw_mut_ptr::<SetHeader>(), v);
                }
            }
        }
    }
    result_handle.get_raw_mut_ptr::<SetHeader>()
}

/// `Set.prototype.isSubsetOf(other)` — true if every element of `this` is
/// also in `other`. Returns 1/0.
#[no_mangle]
pub extern "C" fn js_set_is_subset_of(set: *const SetHeader, other: f64) -> i32 {
    let set = clean_set_ptr(set);
    if set.is_null() {
        return 1; // empty set is a subset of anything
    }
    unsafe {
        let other = other_set_ptr(other);
        let size = (*set).size as usize;
        if other.is_null() {
            return (size == 0) as i32;
        }
        let elements = elements_ptr(set);
        for i in 0..size {
            let v = ptr::read(elements.add(i));
            if js_set_has(other, v) == 0 {
                return 0;
            }
        }
        1
    }
}

/// `Set.prototype.isSupersetOf(other)` — true if every element of `other`
/// is also in `this`. Returns 1/0.
#[no_mangle]
pub extern "C" fn js_set_is_superset_of(set: *const SetHeader, other: f64) -> i32 {
    let set = clean_set_ptr(set);
    unsafe {
        let other = other_set_ptr(other);
        if other.is_null() {
            return 1; // every set is a superset of an empty other
        }
        let osize = (*other).size as usize;
        if set.is_null() {
            return (osize == 0) as i32;
        }
        let oelements = elements_ptr(other);
        for i in 0..osize {
            let v = ptr::read(oelements.add(i));
            if js_set_has(set, v) == 0 {
                return 0;
            }
        }
        1
    }
}

/// `Set.prototype.isDisjointFrom(other)` — true if the sets share no
/// elements. Returns 1/0.
#[no_mangle]
pub extern "C" fn js_set_is_disjoint_from(set: *const SetHeader, other: f64) -> i32 {
    let set = clean_set_ptr(set);
    if set.is_null() {
        return 1;
    }
    unsafe {
        let other = other_set_ptr(other);
        if other.is_null() {
            return 1;
        }
        let size = (*set).size as usize;
        let elements = elements_ptr(set);
        for i in 0..size {
            let v = ptr::read(elements.add(i));
            if js_set_has(other, v) != 0 {
                return 0;
            }
        }
        1
    }
}

// #2872: keepalive anchors so the auto-optimize whole-program-LLVM-bitcode
// rebuild doesn't dead-strip these codegen-only `#[no_mangle]` entry points
// (see project_auto_optimize_keepalive_3320 / PR #3320).
#[used]
static KEEP_SET_UNION: extern "C" fn(*const SetHeader, f64) -> *mut SetHeader = js_set_union;
#[used]
static KEEP_SET_INTERSECTION: extern "C" fn(*const SetHeader, f64) -> *mut SetHeader =
    js_set_intersection;
#[used]
static KEEP_SET_DIFFERENCE: extern "C" fn(*const SetHeader, f64) -> *mut SetHeader =
    js_set_difference;
#[used]
static KEEP_SET_SYMDIFF: extern "C" fn(*const SetHeader, f64) -> *mut SetHeader =
    js_set_symmetric_difference;
#[used]
static KEEP_SET_IS_SUBSET: extern "C" fn(*const SetHeader, f64) -> i32 = js_set_is_subset_of;
#[used]
static KEEP_SET_IS_SUPERSET: extern "C" fn(*const SetHeader, f64) -> i32 = js_set_is_superset_of;
#[used]
static KEEP_SET_IS_DISJOINT: extern "C" fn(*const SetHeader, f64) -> i32 = js_set_is_disjoint_from;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::string::js_string_from_bytes;

    #[test]
    fn test_set_add_and_has() {
        let set = js_set_alloc(4);
        js_set_add(set, 1.0);
        js_set_add(set, 2.0);
        js_set_add(set, 3.0);

        assert_eq!(js_set_has(set, 1.0), 1);
        assert_eq!(js_set_has(set, 2.0), 1);
        assert_eq!(js_set_has(set, 3.0), 1);
        assert_eq!(js_set_has(set, 4.0), 0);
        assert_eq!(js_set_has(set, 0.0), 0);
    }

    // #2872: helper to pass a Set pointer as the NaN-boxed `other` argument.
    // A raw heap pointer (top16 == 0) is returned unchanged by `clean_set_ptr`,
    // so reinterpreting the pointer bits as f64 round-trips through
    // `other_set_ptr`.
    fn as_other(set: *mut SetHeader) -> f64 {
        f64::from_bits(set as u64)
    }

    fn collect(set: *const SetHeader) -> Vec<f64> {
        unsafe {
            let size = (*set).size as usize;
            (0..size).map(|i| *(elements_ptr(set).add(i))).collect()
        }
    }

    #[test]
    fn test_set_union() {
        let a = js_set_alloc(4);
        js_set_add(a, 1.0);
        js_set_add(a, 2.0);
        let b = js_set_alloc(4);
        js_set_add(b, 2.0);
        js_set_add(b, 3.0);
        let u = js_set_union(a, as_other(b));
        assert_eq!(collect(u), vec![1.0, 2.0, 3.0]);
    }

    #[test]
    fn test_set_intersection() {
        let a = js_set_alloc(4);
        js_set_add(a, 1.0);
        js_set_add(a, 2.0);
        let b = js_set_alloc(4);
        js_set_add(b, 2.0);
        js_set_add(b, 3.0);
        let r = js_set_intersection(a, as_other(b));
        assert_eq!(collect(r), vec![2.0]);
    }

    #[test]
    fn test_set_difference() {
        let a = js_set_alloc(4);
        js_set_add(a, 1.0);
        js_set_add(a, 2.0);
        let b = js_set_alloc(4);
        js_set_add(b, 2.0);
        js_set_add(b, 3.0);
        let r = js_set_difference(a, as_other(b));
        assert_eq!(collect(r), vec![1.0]);
    }

    #[test]
    fn test_set_symmetric_difference() {
        let a = js_set_alloc(4);
        js_set_add(a, 1.0);
        js_set_add(a, 2.0);
        let b = js_set_alloc(4);
        js_set_add(b, 2.0);
        js_set_add(b, 3.0);
        let r = js_set_symmetric_difference(a, as_other(b));
        // `a` leftovers (1) then `b` leftovers (3) — insertion order.
        assert_eq!(collect(r), vec![1.0, 3.0]);
    }

    #[test]
    fn test_set_predicates() {
        let a = js_set_alloc(4);
        js_set_add(a, 1.0);
        js_set_add(a, 2.0);

        let superset = js_set_alloc(4);
        js_set_add(superset, 1.0);
        js_set_add(superset, 2.0);
        js_set_add(superset, 3.0);
        assert_eq!(js_set_is_subset_of(a, as_other(superset)), 1);
        assert_eq!(js_set_is_superset_of(a, as_other(superset)), 0);

        let one = js_set_alloc(4);
        js_set_add(one, 1.0);
        assert_eq!(js_set_is_superset_of(a, as_other(one)), 1);

        let disjoint = js_set_alloc(4);
        js_set_add(disjoint, 4.0);
        assert_eq!(js_set_is_disjoint_from(a, as_other(disjoint)), 1);
        assert_eq!(js_set_is_disjoint_from(a, as_other(superset)), 0);
    }

    #[test]
    fn test_set_add_duplicate() {
        let set = js_set_alloc(4);
        js_set_add(set, 42.0);
        js_set_add(set, 42.0);
        js_set_add(set, 42.0);

        assert_eq!(js_set_size(set), 1);
    }

    #[test]
    fn test_set_delete() {
        let set = js_set_alloc(4);
        js_set_add(set, 1.0);
        js_set_add(set, 2.0);
        js_set_add(set, 3.0);

        // Delete existing value
        assert_eq!(js_set_delete(set, 2.0), 1);
        assert_eq!(js_set_size(set), 2);
        assert_eq!(js_set_has(set, 2.0), 0);

        // Other values still present
        assert_eq!(js_set_has(set, 1.0), 1);
        assert_eq!(js_set_has(set, 3.0), 1);

        // Delete non-existing value
        assert_eq!(js_set_delete(set, 99.0), 0);
        assert_eq!(js_set_size(set), 2);
    }

    #[test]
    fn test_set_clear() {
        let set = js_set_alloc(4);
        js_set_add(set, 1.0);
        js_set_add(set, 2.0);
        js_set_add(set, 3.0);

        js_set_clear(set);
        assert_eq!(js_set_size(set), 0);
        assert_eq!(js_set_has(set, 1.0), 0);
        assert_eq!(js_set_has(set, 2.0), 0);
        assert_eq!(js_set_has(set, 3.0), 0);
    }

    #[test]
    fn test_set_size_tracking() {
        let set = js_set_alloc(4);
        assert_eq!(js_set_size(set), 0);

        js_set_add(set, 1.0);
        assert_eq!(js_set_size(set), 1);

        js_set_add(set, 2.0);
        assert_eq!(js_set_size(set), 2);

        // Duplicate doesn't increase size
        js_set_add(set, 1.0);
        assert_eq!(js_set_size(set), 2);

        js_set_delete(set, 1.0);
        assert_eq!(js_set_size(set), 1);
    }

    #[test]
    fn test_set_grow_beyond_initial_capacity() {
        let set = js_set_alloc(2);
        // Add more elements than initial capacity
        for i in 0..20 {
            js_set_add(set, i as f64);
        }

        assert_eq!(js_set_size(set), 20);
        for i in 0..20 {
            assert_eq!(js_set_has(set, i as f64), 1, "should contain {}", i);
        }
        assert_eq!(js_set_has(set, 20.0), 0);
    }

    #[test]
    fn test_set_string_values() {
        // Create two string headers with identical content at different addresses
        let s1 = js_string_from_bytes(b"hello".as_ptr(), 5);
        let s2 = js_string_from_bytes(b"hello".as_ptr(), 5);

        // Verify they are at different addresses
        assert_ne!(s1 as usize, s2 as usize);

        // NaN-box with STRING_TAG (0x7FFF)
        let val1 = f64::from_bits(0x7FFF_0000_0000_0000 | (s1 as u64 & 0x0000_FFFF_FFFF_FFFF));
        let val2 = f64::from_bits(0x7FFF_0000_0000_0000 | (s2 as u64 & 0x0000_FFFF_FFFF_FFFF));

        let set = js_set_alloc(4);
        js_set_add(set, val1);

        // Adding string with same content (different pointer) should be duplicate
        js_set_add(set, val2);
        assert_eq!(
            js_set_size(set),
            1,
            "strings with same content should be deduplicated"
        );

        // has() should find by content
        assert_eq!(js_set_has(set, val2), 1);
    }

    #[test]
    fn test_set_string_specialized_helpers_use_content_keys() {
        let s1 = js_string_from_bytes(b"hello".as_ptr(), 5);
        let s2 = js_string_from_bytes(b"hello".as_ptr(), 5);
        assert_ne!(s1 as usize, s2 as usize);

        let set = js_set_alloc(4);
        js_set_add_string(set, s1);
        assert_eq!(js_set_size(set), 1);
        assert_eq!(js_set_has_string(set, s2), 1);

        js_set_add_string(set, s2);
        assert_eq!(
            js_set_size(set),
            1,
            "same-content string values should deduplicate"
        );

        assert_eq!(js_set_delete_string(set, s2), 1);
        assert_eq!(js_set_has_string(set, s1), 0);
    }

    #[test]
    fn test_set_number_specialized_helpers_preserve_numeric_values_and_fallback() {
        let set = js_set_alloc(4);

        js_set_add_number(set, -0.0);
        assert_eq!(js_set_size(set), 1);
        assert_eq!(js_set_has_number(set, 0.0), 1);
        assert!(
            test_set_index_contains(set, 0.0),
            "numeric helper should populate the Set side-table"
        );

        js_set_add_number(set, 0.0);
        assert_eq!(
            js_set_size(set),
            1,
            "-0 and +0 should deduplicate through the numeric helper"
        );
        assert_eq!(js_set_delete_number(set, -0.0), 1);
        assert_eq!(js_set_has_number(set, 0.0), 0);

        let string = js_string_from_bytes(b"fallback".as_ptr(), 8);
        let boxed_string =
            f64::from_bits(crate::value::STRING_TAG | (string as u64 & crate::value::POINTER_MASK));
        js_set_add_number(set, boxed_string);
        assert_eq!(
            js_set_has_number(set, boxed_string),
            1,
            "nonnumeric calls to the numeric helper should preserve generic fallback semantics"
        );
        assert_eq!(js_set_delete_number(set, boxed_string), 1);
        assert_eq!(js_set_has(set, boxed_string), 0);
    }

    #[test]
    fn test_set_i32_specialized_helpers_use_int32_keys() {
        let set = js_set_alloc(4);
        js_set_add_i32(set, 42);
        js_set_add_i32(set, 42);
        assert_eq!(js_set_size(set), 1);
        assert_eq!(js_set_has_i32(set, 42), 1);
        assert_eq!(js_set_has_i32(set, -7), 0);

        let boxed = f64::from_bits(crate::value::JSValue::int32(42).bits());
        assert_eq!(js_set_has(set, boxed), 1);
        assert_eq!(js_set_delete_i32(set, 42), 1);
        assert_eq!(js_set_has(set, boxed), 0);
        assert_eq!(js_set_delete_i32(set, 42), 0);
    }

    #[test]
    fn test_set_u32_specialized_helpers_use_number_keys() {
        let set = js_set_alloc(4);
        js_set_add_u32(set, u32::MAX);
        js_set_add_u32(set, u32::MAX);
        assert_eq!(js_set_size(set), 1);
        assert_eq!(js_set_has_u32(set, u32::MAX), 1);
        assert_eq!(js_set_has_u32(set, 7), 0);

        let boxed = u32::MAX as f64;
        assert_eq!(js_set_has(set, boxed), 1);
        assert_eq!(js_set_delete_u32(set, u32::MAX), 1);
        assert_eq!(js_set_has(set, boxed), 0);
        assert_eq!(js_set_delete_u32(set, u32::MAX), 0);
    }

    #[test]
    fn test_set_f32_specialized_helpers_use_number_keys() {
        let set = js_set_alloc(4);
        js_set_add_f32(set, 1.5);
        js_set_add_f32(set, 1.5);
        assert_eq!(js_set_size(set), 1);
        assert_eq!(js_set_has_f32(set, 1.5), 1);
        assert_eq!(js_set_has_f32(set, -2.25), 0);

        let boxed = 1.5_f64;
        assert_eq!(js_set_has(set, boxed), 1);
        assert_eq!(js_set_delete_f32(set, 1.5), 1);
        assert_eq!(js_set_has(set, boxed), 0);
        assert_eq!(js_set_delete_f32(set, 1.5), 0);
    }

    #[test]
    fn test_set_bool_specialized_helpers_use_boolean_keys() {
        let set = js_set_alloc(4);
        js_set_add_bool(set, 1);
        js_set_add_bool(set, 1);
        assert_eq!(js_set_size(set), 1);
        assert_eq!(js_set_has_bool(set, 1), 1);
        assert_eq!(js_set_has_bool(set, 0), 0);

        let boxed = f64::from_bits(crate::value::TAG_TRUE);
        assert_eq!(js_set_has(set, boxed), 1);
        assert_eq!(js_set_delete_bool(set, 1), 1);
        assert_eq!(js_set_has(set, boxed), 0);
        assert_eq!(js_set_delete_bool(set, 1), 0);
    }

    #[test]
    fn test_set_mixed_number_values() {
        let set = js_set_alloc(4);

        // Various number values
        js_set_add(set, 0.0);
        js_set_add(set, -1.0);
        js_set_add(set, 3.14);
        js_set_add(set, f64::INFINITY);
        js_set_add(set, f64::NEG_INFINITY);

        assert_eq!(js_set_size(set), 5);
        assert_eq!(js_set_has(set, 0.0), 1);
        assert_eq!(js_set_has(set, -1.0), 1);
        assert_eq!(js_set_has(set, 3.14), 1);
        assert_eq!(js_set_has(set, f64::INFINITY), 1);
        assert_eq!(js_set_has(set, f64::NEG_INFINITY), 1);
    }

    #[test]
    fn test_set_large() {
        let set = js_set_alloc(4);
        let n = 1000;

        for i in 0..n {
            js_set_add(set, i as f64);
        }
        assert_eq!(js_set_size(set), n);

        // Verify all values present
        for i in 0..n {
            assert_eq!(js_set_has(set, i as f64), 1, "should contain {}", i);
        }

        // Values outside range not present
        assert_eq!(js_set_has(set, n as f64), 0);
        assert_eq!(js_set_has(set, -1.0), 0);
    }

    #[test]
    fn test_set_delete_and_re_add() {
        let set = js_set_alloc(4);
        js_set_add(set, 1.0);
        js_set_add(set, 2.0);
        js_set_add(set, 3.0);

        js_set_delete(set, 2.0);
        assert_eq!(js_set_has(set, 2.0), 0);

        // Re-add the deleted value
        js_set_add(set, 2.0);
        assert_eq!(js_set_has(set, 2.0), 1);
        assert_eq!(js_set_size(set), 3);
    }

    #[test]
    fn test_set_to_array_roundtrip() {
        let set = js_set_alloc(4);
        js_set_add(set, 10.0);
        js_set_add(set, 20.0);
        js_set_add(set, 30.0);

        let arr = js_set_to_array(set);
        assert_eq!(crate::array::js_array_length(arr), 3);

        // Verify all values are in the array
        let mut found = [false; 3];
        for i in 0..3 {
            let val = crate::array::js_array_get_f64(arr, i);
            if val == 10.0 {
                found[0] = true;
            }
            if val == 20.0 {
                found[1] = true;
            }
            if val == 30.0 {
                found[2] = true;
            }
        }
        assert!(found.iter().all(|&f| f), "all values should be in array");
    }

    // --- String comparison tests (Phase 0D) ---

    #[test]
    fn test_strings_equal_same_content() {
        let s1 = js_string_from_bytes(b"test".as_ptr(), 4);
        let s2 = js_string_from_bytes(b"test".as_ptr(), 4);
        assert_ne!(s1 as usize, s2 as usize);
        assert!(unsafe { strings_equal(s1, s2) });
    }

    #[test]
    fn test_strings_equal_different_length() {
        let s1 = js_string_from_bytes(b"hello".as_ptr(), 5);
        let s2 = js_string_from_bytes(b"hi".as_ptr(), 2);
        assert!(!unsafe { strings_equal(s1, s2) });
    }

    #[test]
    fn test_strings_equal_same_pointer() {
        let s1 = js_string_from_bytes(b"hello".as_ptr(), 5);
        // Same pointer should be equal
        assert!(unsafe { strings_equal(s1, s1) });
    }

    #[test]
    fn test_strings_equal_empty() {
        let s1 = js_string_from_bytes(std::ptr::null(), 0);
        let s2 = js_string_from_bytes(std::ptr::null(), 0);
        assert!(unsafe { strings_equal(s1, s2) });
    }

    #[test]
    fn test_strings_equal_different_content() {
        let s1 = js_string_from_bytes(b"abc".as_ptr(), 3);
        let s2 = js_string_from_bytes(b"abd".as_ptr(), 3);
        assert!(!unsafe { strings_equal(s1, s2) });
    }

    #[test]
    fn test_jsvalue_eq_numbers() {
        assert!(jsvalue_eq(1.0, 1.0));
        assert!(jsvalue_eq(0.0, 0.0));
        assert!(!jsvalue_eq(1.0, 2.0));
    }

    #[test]
    fn test_jsvalue_eq_cross_tag_strings() {
        let s1 = js_string_from_bytes(b"hello".as_ptr(), 5);
        let s2 = js_string_from_bytes(b"hello".as_ptr(), 5);

        // STRING_TAG
        let val1 = f64::from_bits(0x7FFF_0000_0000_0000 | (s1 as u64 & 0x0000_FFFF_FFFF_FFFF));
        // POINTER_TAG (different tag, same content)
        let val2 = f64::from_bits(0x7FFD_0000_0000_0000 | (s2 as u64 & 0x0000_FFFF_FFFF_FFFF));

        assert!(
            jsvalue_eq(val1, val2),
            "cross-tag strings with same content should be equal"
        );
    }
}
