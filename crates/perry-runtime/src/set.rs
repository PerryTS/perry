//! Set representation for Perry
//!
//! Sets are arena-allocated GC objects.
//! A traced store owns inline elements and native index/cursor history.
//! No address registry, collection scanner, or header finalizer is needed.

use crate::string::StringHeader;
use std::alloc::Layout;
use std::cell::RefCell;
use std::ptr;

mod index;
#[cfg(test)]
pub(crate) use index::{
    test_erase_key_identity, test_key_identity, test_snapshot as test_index_snapshot,
};

crate::perry_thread_local! {
    /// Backing Sets whose `forEach` raw-index walk is currently in flight.
    /// Deletes may leave holes but must not compact them while a cursor still
    /// names the old raw layout (#9082). Nested walks are kept as a stack so
    /// exception savepoints can discard precisely the frames a throw skips.
    static SET_FOREACH_STACK: RefCell<crate::exception::CatchStack<usize>> =
        const { RefCell::new(crate::exception::CatchStack::new()) };
}

#[inline]
fn set_foreach_is_active(set: *const SetHeader) -> bool {
    let addr = set as usize;
    SET_FOREACH_STACK.with(|stack| stack.borrow().contains(&addr))
}

/// Open a walk owned by the calling frame (`#[inline(always)]`: the frame
/// mark must be read in the walk's owner).
#[inline(always)]
fn set_foreach_enter(set: *const SetHeader) {
    set_foreach_enter_at(crate::exception::frame_mark(), set);
}

fn set_foreach_enter_at(mark: usize, set: *const SetHeader) {
    SET_FOREACH_STACK.with(|stack| stack.borrow_mut().push(mark, set as usize));
}

/// Pop one completed walk. Returns true when this was the outermost walk of
/// this Set, at which point deferred compaction is safe again.
fn set_foreach_leave(set: *const SetHeader) -> bool {
    let addr = set as usize;
    SET_FOREACH_STACK.with(|stack| {
        let mut stack = stack.borrow_mut();
        let popped = stack.pop();
        debug_assert_eq!(popped, Some(addr));
        !stack.contains(&addr)
    })
}

/// Landing pad at stack pointer `sp`: abandoned walks no longer inhibit
/// compaction (#9082).
pub(crate) fn set_foreach_unwind_to(sp: usize) {
    SET_FOREACH_STACK.with(|stack| {
        stack.borrow_mut().unwind_to(sp);
    });
}

#[cfg(any(test, target_os = "wasi"))]
pub(crate) fn set_foreach_restore_depth(depth: usize) {
    SET_FOREACH_STACK.with(|stack| {
        stack.borrow_mut().restore_depth(depth);
    });
}

pub(crate) fn set_foreach_check_marks(sp: usize) {
    SET_FOREACH_STACK.with(|stack| {
        crate::exception::marks::check_marks(stack.borrow().marks(), sp, "set_foreach")
    });
}

#[cfg(any(test, target_os = "wasi"))]
pub(crate) fn set_foreach_depth() -> usize {
    SET_FOREACH_STACK.with(|stack| stack.borrow().len())
}

pub fn is_registered_set_iterator(addr: usize) -> bool {
    crate::array::has_materialized_iterator_brand(addr, 2)
}

#[cfg(test)]
crate::perry_thread_local! {
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

#[cfg(test)]
static TEST_SET_SIDE_DEALLOCATIONS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);
#[cfg(test)]
static TEST_SET_SIDE_DEALLOCATED_BYTES: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

// The same counts for the current thread only. Set heaps are per-thread, so a
// test that collects on its own thread can assert exact deltas here while other
// test threads free Sets in parallel (see the Map counterpart in `map.rs`).
#[cfg(test)]
std::thread_local! {
    static TEST_THREAD_SET_SIDE_DEALLOCATIONS: std::cell::Cell<(u64, u64)> =
        const { std::cell::Cell::new((0, 0)) };
}

#[cfg(test)]
fn note_test_set_side_deallocation(bytes: usize) {
    use std::sync::atomic::Ordering;

    TEST_SET_SIDE_DEALLOCATIONS.fetch_add(1, Ordering::Relaxed);
    TEST_SET_SIDE_DEALLOCATED_BYTES.fetch_add(bytes as u64, Ordering::Relaxed);
    // `try_with`: a store can drop during thread-exit TLS teardown.
    let _ = TEST_THREAD_SET_SIDE_DEALLOCATIONS.try_with(|count| {
        let (frees, freed) = count.get();
        count.set((frees + 1, freed + bytes as u64));
    });
}

/// (frees, bytes) of Set side storage released on the current thread.
#[cfg(test)]
pub(crate) fn test_thread_set_side_deallocation_snapshot() -> (u64, u64) {
    TEST_THREAD_SET_SIDE_DEALLOCATIONS.with(std::cell::Cell::get)
}

#[cfg(not(test))]
#[inline]
fn note_test_set_side_deallocation(_bytes: usize) {}

#[cfg(test)]
pub(crate) fn test_set_side_deallocation_snapshot() -> (u64, u64) {
    use std::sync::atomic::Ordering;

    (
        TEST_SET_SIDE_DEALLOCATIONS.load(Ordering::Relaxed),
        TEST_SET_SIDE_DEALLOCATED_BYTES.load(Ordering::Relaxed),
    )
}

mod store;
use store::{alloc_store, SetStore};
pub(crate) use store::{
    finalize_owned_store, owned_element_slot_range, owned_store_moved_for_gc,
    refresh_elements_alias,
};

#[cfg(test)]
thread_local! {
/// Every entry into [`is_registered_set`]. Twin of
/// `map::TEST_MAP_REGISTRY_PROBES` — see that counter for what it pins down.
    static TEST_SET_REGISTRY_PROBES: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn test_set_registry_probe_count() -> u64 {
    TEST_SET_REGISTRY_PROBES.with(|c| c.get())
}

pub fn is_registered_set(addr: usize) -> bool {
    #[cfg(test)]
    TEST_SET_REGISTRY_PROBES.with(|c| c.set(c.get().wrapping_add(1)));
    unsafe {
        let Some(header) = crate::value::addr_class::try_read_tracked_arena_header_of_type(
            addr,
            crate::gc::GC_TYPE_SET,
        ) else {
            return false;
        };
        header.as_ref().size as usize
            == (crate::gc::GC_HEADER_SIZE + std::mem::size_of::<SetHeader>()).next_multiple_of(8)
            && !(*(addr as *const SetHeader)).store.is_null()
    }
}

/// Resolve a NaN-boxed (or raw-i64) `this` receiver to a registered `Set`
/// pointer, or `None` if the receiver is not a `Set`. Backs the reflective
/// `Set.prototype.*` thunks so they can perform the spec brand check
/// (`TypeError` on a non-`Set` receiver) before dispatching. Mirrors the
/// receiver extraction used by the `Array.prototype.slice` thunk: a
/// NaN-boxed pointer, or a bare raw-i64 pointer some module-init call sites
/// pass as the receiver. Primitives (undefined/null/number/string/bool)
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

fn rebuild_set_index(set: *mut SetHeader) {
    unsafe {
        index::rebuild_index(set);
    }
}

pub(crate) fn set_header_moved_for_gc(old_addr: usize, new_addr: usize) {
    if old_addr == 0 || new_addr == 0 || old_addr == new_addr {
        return;
    }
    unsafe {
        // GC_STORE_AUDIT(POINTER_FREE): ownership transferred to the live copy.
        let old = old_addr as *mut SetHeader;
        (*old).store = ptr::null_mut();
        (*old).elements = ptr::null_mut();
    }
    SET_FOREACH_STACK.with(|stack| {
        for addr in stack.borrow_mut().iter_mut() {
            if *addr == old_addr {
                *addr = new_addr;
            }
        }
    });
}

#[cfg(test)]
pub(crate) unsafe fn finalize_set_side_allocation_for_gc(set: *mut SetHeader) {
    let store = std::mem::replace(&mut (*set).store, ptr::null_mut());
    if !store.is_null() {
        finalize_owned_store(store.cast(), true);
    }
    if !(*set).meta.is_null() {
        (*(*set).meta).native_state = 0;
    }
    // GC_STORE_AUDIT(POINTER_FREE): Set reset clears elements alias to null; no new edge.
    (*set).elements = ptr::null_mut();
    (*set).size = 0;
    (*set).capacity = 0;
    (*set).used = 0;
}
#[cfg(test)]
pub(crate) fn test_set_index_contains(set: *const SetHeader, value: f64) -> bool {
    unsafe { index::lookup_value(set, normalize_zero(value)).is_some_and(|i| i >= 0) }
}
#[cfg(test)]
pub(crate) fn test_set_side_allocation(addr: usize) -> Option<(usize, usize)> {
    if !is_registered_set(addr) {
        return None;
    }
    unsafe {
        let store = (*(addr as *const SetHeader)).store.as_ref()?;
        (!store.elements.is_null()).then_some((store.elements as usize, store.capacity))
    }
}
#[cfg(test)]
pub(crate) unsafe fn test_set_store_word(set: *const SetHeader) -> usize {
    (*set).store as usize
}
#[cfg(test)]
pub(crate) unsafe fn test_init_owned_set_store(set: *mut SetHeader, capacity: usize) -> *mut u64 {
    let store = store::alloc_store_in::<true>(capacity);
    (*store).used = capacity; // all zeroed fixture slots are safe to trace
    (*set).store = store;
    // GC_STORE_AUDIT(INIT): cfg(test) constructor allocates tenured store with zeroed payload, sets used=capacity, then initializes test header.store/elements before fixture publication. elements is derived inline-payload alias, not independent ownership.
    (*set).elements = (*store).elements;
    (*store).elements.cast()
}

/// Movable Set header with an owned, movable element store; `elements` is
/// re-derived by the Set's GC rewrite hook whenever the store moves.
#[repr(C)]
pub struct SetHeader {
    /// Number of elements in the set
    pub size: u32,
    /// Capacity (allocated space for elements)
    pub capacity: u32,
    /// Pointer to elements array (separately allocated)
    pub elements: *mut f64,
    /// #6759 phase 1 (header unification): per-object metadata record, or
    /// null. Appended LAST so every preceding field keeps its offset. Traced
    /// and rewritten by the `GcRewriteDescriptorKind::Set` arm, which
    /// `trace_heap_rewrite_slots` drives — so the edge is marked, not merely
    /// rewritten (#6812).
    pub meta: *mut crate::object::ObjectMeta,
    /// Extent of the elements array actually written: raw element indices run
    /// `0..used`. `size` stays the LIVE count; `used - size` counts the
    /// tombstoned slots awaiting compaction (offset pinned).
    pub used: u32,
    /// Bumped whenever an element moves to a LOWER raw index
    /// (`compact_set_elements`) or the extent is discarded (`clear`). Raw-index
    /// cursors record the epoch they last saw and rebase through
    /// its owner-local compaction history when it has moved (see `set_cursor_next_raw`).
    /// Appended last; fits in the padding after `used`.
    pub compaction_epoch: u32,
    /// Strong edge to the young-born, movable element store (same former native word).
    pub(crate) store: *mut SetStore,
}

#[cfg(target_pointer_width = "64")]
const _: () = {
    assert!(std::mem::offset_of!(SetHeader, size) == 0);
    assert!(std::mem::offset_of!(SetHeader, capacity) == 4);
    assert!(std::mem::offset_of!(SetHeader, elements) == 8);
    assert!(std::mem::offset_of!(SetHeader, used) == 24);
    assert!(std::mem::offset_of!(SetHeader, compaction_epoch) == 28);
    assert!(std::mem::size_of::<SetHeader>() == 40);
};

// ILP32 (wasm32, arm64_32): the two pointer fields are 4 bytes.
#[cfg(target_pointer_width = "32")]
const _: () = {
    assert!(std::mem::offset_of!(SetHeader, size) == 0);
    assert!(std::mem::offset_of!(SetHeader, capacity) == 4);
    assert!(std::mem::offset_of!(SetHeader, elements) == 8);
    assert!(std::mem::offset_of!(SetHeader, used) == 16);
    assert!(std::mem::offset_of!(SetHeader, compaction_epoch) == 20);
    assert!(std::mem::size_of::<SetHeader>() == 28);
};

/// The tombstone a deleted element's slot takes — same reserved marker as the
/// Map tombstones; `normalize_zero` keeps it out of stored values.
pub(crate) const SET_HOLE_VALUE_BITS: u64 = crate::value::TAG_HOLE;

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

#[cfg(test)]
pub(crate) unsafe fn gc_element_slot_range(
    set: *mut SetHeader,
) -> Option<crate::gc::HeapSlotRange> {
    if set.is_null() {
        return None;
    }
    let size = (*set).size as usize;
    let used = (*set).used as usize;
    let capacity = (*set).capacity as usize;
    if size > used || used > capacity || used > 16_000_000 || (*set).elements.is_null() {
        return None;
    }
    // Defensive tripwire (cf. Map's entries check in layout_slot_visit):
    // a NaN-boxed JSValue read as `elements` carries 0x7FFD in the top
    // bits, and other non-pointer payload words can land below the heap
    // range or in the handle band. Reject any `elements` that is not a
    // plausible heap address rather than dereference a derived slot range
    // from a garbage base. Uses the shared platform-correct classifier
    // (a fixed `>> 47` cutoff would false-reject genuine elements on
    // aarch64 Linux, where user space reaches bit 48).
    let elements_addr = (*set).elements as usize;
    if !crate::value::addr_class::is_plausible_heap_addr(elements_addr) {
        if crate::gc::gc_diag_enabled() {
            eprintln!(
                "[gc-tripwire] Set elements is not a plausible heap address: {:#x} (fabricated Set?)",
                elements_addr
            );
        }
        return None;
    }
    Some(crate::gc::HeapSlotRange::new(
        (*set).elements as *mut u64,
        used,
    ))
}

/// SameValueZero key normalization: -0 → +0.
/// ECMAScript Sets treat -0 and +0 as the same value (23.2.3.1). Without
/// this, `0` (bits 0x0) and `-0` (bits 0x8000_0000_0000_0000) hash/compare
/// as distinct. Non-number JSValues have NaN-box tags in the upper bits,
/// so `v == 0.0` stays false for them.
#[inline(always)]
fn normalize_zero(value: f64) -> f64 {
    if value.to_bits() == SET_HOLE_VALUE_BITS {
        // A leaked array hole reads as `undefined` at every other boundary;
        // canonicalize here so the tombstone can never collide with a value.
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
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
    // Native handles (including WebSocket clients) are identities, never
    // string allocations, even when their ids have grown beyond one page.
    if !crate::value::addr_class::is_above_handle_band(ptr as usize) {
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

    // Installing owner-local state can reserve an array property slot and
    // grow its allocation. Such a forwarding alias still names the same JS
    // array, including when one operand has already been refreshed.
    if a_bits & !crate::value::POINTER_MASK == crate::value::POINTER_TAG
        && b_bits & !crate::value::POINTER_MASK == crate::value::POINTER_TAG
    {
        unsafe {
            let a = (a_bits & crate::value::POINTER_MASK) as usize;
            let b = (b_bits & crate::value::POINTER_MASK) as usize;
            if crate::value::addr_class::try_read_gc_header(a)
                .is_some_and(|h| h.obj_type == crate::gc::GC_TYPE_ARRAY)
                && crate::value::addr_class::try_read_gc_header(b)
                    .is_some_and(|h| h.obj_type == crate::gc::GC_TYPE_ARRAY)
            {
                return crate::array::clean_arr_ptr(a as *const crate::array::ArrayHeader)
                    == crate::array::clean_arr_ptr(b as *const crate::array::ArrayHeader);
            }
        }
    }
    false
}

/// Find the index of a value in the set, or -1 if not found.
/// Uses the lazily allocated, owner-local hash index above eight elements.
/// C-ABI: current elements-array index of `value` (SameValueZero), or `-1.0` if
/// absent. Companion to `js_map_find_key_index` for the delete-safe Set `for-of`
/// fast path (#6075). Only invoked from generated IR, so `#[used]` keeps it.
#[no_mangle]
pub extern "C" fn js_set_find_value_index(set_boxed: f64, value: f64) -> f64 {
    let set = clean_set_ptr(crate::value::js_nanbox_get_pointer(set_boxed) as *const SetHeader);
    if set.is_null() {
        return -1.0;
    }
    unsafe { find_value_index(set, normalize_zero(value)) as f64 }
}
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_SET_FIND_VALUE_INDEX: extern "C" fn(f64, f64) -> f64 = js_set_find_value_index;

/// Members of a set this small are found faster by reading them than by
/// hashing into the side-table twice (set address, then value).
const SMALL_SET_SCAN_MAX: u32 = 8;

/// Live-extent accessor for iteration (`0..used` are the raw indices).
#[inline(always)]
pub(crate) fn set_used_entries(set: *const SetHeader) -> u32 {
    unsafe { (*set).used }
}

/// Raw-indexed element read for the iterator objects: bound by `used`, no
/// compaction — the advance loop skips holes itself.
#[inline(always)]
pub(crate) unsafe fn set_value_raw(set: *const SetHeader, idx: u32) -> f64 {
    if idx >= (*set).used {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    ptr::read(elements_ptr(set).add(idx as usize))
}

/// Squeeze the tombstones out (insertion order preserved), then rebuild the
/// lookup index from the dense buffer.
/// One squeeze of the elements buffer, as a raw-index cursor needs to see it
/// (the Map twin is `map::RemovedSlots`): which raw indices below the cursor
/// disappeared, so it can move down by exactly that many.
enum SetRemovedSlots {
    /// `clear()` discarded the whole extent `0..n`.
    Prefix(u32),
    /// Compaction squeezed out these tombstoned raw indices (ascending).
    Indices(Vec<u32>),
}

impl SetRemovedSlots {
    fn count_below(&self, cursor: u32) -> u32 {
        match self {
            SetRemovedSlots::Prefix(n) => cursor.min(*n),
            SetRemovedSlots::Indices(v) => v.partition_point(|&i| i < cursor) as u32,
        }
    }
}

struct SetCompactionRecord {
    /// The header's `compaction_epoch` AFTER this squeeze.
    epoch: u32,
    removed: SetRemovedSlots,
}

/// Retention budget for a set's squeeze history, in removed raw indices.
///
/// A cursor rebases exactly only through records it has not been trimmed out
/// of, so the budget is what bounds the exactness window: exceeding it needs
/// ONE loop body to delete more than `max(SET_COMPACTION_LOG_MIN_BUDGET,
/// capacity)` entries (deletes plus re-adds count separately) between two of
/// its own reads. Record COUNT is not the bound — a delete+re-add pair on a
/// collection at full capacity squeezes one hole per pair, so counting
/// records would make the window a few dozen pairs.
///
/// Bounded in memory by construction: at most `budget` indices (4 bytes each)
/// plus a record header per squeeze; a `clear()` record supersedes everything
/// before it and truncates them.
const SET_COMPACTION_LOG_MIN_BUDGET: usize = 4096;

struct SetCompactionLog {
    records: std::collections::VecDeque<SetCompactionRecord>,
    /// Sum of `records[..].removed.retained_len()`.
    retained: usize,
}

impl SetRemovedSlots {
    /// Budget cost of this record.
    fn retained_len(&self) -> usize {
        match self {
            SetRemovedSlots::Prefix(_) => 1,
            SetRemovedSlots::Indices(v) => v.len(),
        }
    }
}

#[cfg(test)]
unsafe fn release_set_compaction(set: *mut SetHeader) {
    if let Some(store) = (*set).store.as_mut() {
        drop(store.compaction.take());
    }
}

/// Append a squeeze record and advance the header epoch (every operation
/// that moves elements to lower raw indices or discards the extent).
unsafe fn note_set_compaction(set: *mut SetHeader, removed: SetRemovedSlots) {
    let epoch = (*set).compaction_epoch.wrapping_add(1);
    (*set).compaction_epoch = epoch;
    let budget = std::cmp::max(SET_COMPACTION_LOG_MIN_BUDGET, (*set).capacity as usize);
    if (*(*set).store).compaction.is_none() {
        // Native records contain only indices/epochs, never managed pointers.
        (*(*set).store).compaction = Some(SetCompactionLog {
            records: std::collections::VecDeque::new(),
            retained: 0,
        });
    }
    let entry = (*(*set).store).compaction.as_mut().unwrap();
    if matches!(removed, SetRemovedSlots::Prefix(_)) {
        // The extent was discarded: every cursor rebases to 0 through
        // this record whatever came before, so older history is dead.
        entry.records.clear();
        entry.retained = 0;
    }
    entry.retained += removed.retained_len();
    entry
        .records
        .push_back(SetCompactionRecord { epoch, removed });
    while entry.retained > budget && entry.records.len() > 1 {
        if let Some(oldest) = entry.records.pop_front() {
            entry.retained -= oldest.removed.retained_len();
        }
    }
}

/// Rebase a raw-index cursor that last synchronised at `loop_epoch` onto the
/// current raw layout — exact for the same reason as `map::rebase_map_cursor`:
/// the elements a walk has yielded are precisely the live ones below its
/// cursor, so each squeeze moves the cursor down by the removed count below it.
unsafe fn rebase_set_cursor(set: *const SetHeader, cursor: u32, loop_epoch: u32) -> u32 {
    if (*set).compaction_epoch == loop_epoch {
        return cursor;
    }
    {
        let Some(entry) = (*(*set).store).compaction.as_ref() else {
            return cursor;
        };
        let mut c = cursor;
        for rec in entry
            .records
            .iter()
            .filter(|r| r.epoch.wrapping_sub(loop_epoch) as i32 > 0)
        {
            c -= rec.removed.count_below(c);
        }
        c
    }
}

/// The next live raw index at or after `cursor` (rebased through any
/// squeezes since `loop_epoch`), or `None` when the extent is exhausted.
/// O(holes stepped over); never compacts. See `map::map_cursor_next_raw`.
pub(crate) unsafe fn set_cursor_next_raw(
    set: *const SetHeader,
    cursor: u32,
    loop_epoch: u32,
) -> Option<u32> {
    let mut idx = rebase_set_cursor(set, cursor, loop_epoch);
    let used = (*set).used;
    let elements = elements_ptr(set);
    while idx < used && ptr::read(elements.add(idx as usize)).to_bits() == SET_HOLE_VALUE_BITS {
        idx += 1;
    }
    (idx < used).then_some(idx)
}

#[inline]
pub(crate) fn set_compaction_epoch(set: *const SetHeader) -> u32 {
    unsafe { (*set).compaction_epoch }
}

/// C-ABI, for the `for…of` fast path: next live raw index at or after
/// `cursor` after rebasing through squeezes since `epoch`, or `-1.0` when
/// exhausted. NaN-boxed set; cursor/epoch are the loop's Number temps.
#[no_mangle]
pub extern "C" fn js_set_cursor_next(set_boxed: f64, cursor: f64, epoch: f64) -> f64 {
    let set = clean_set_ptr(crate::value::js_nanbox_get_pointer(set_boxed) as *const SetHeader);
    if set.is_null() {
        return -1.0;
    }
    let cursor = if cursor > 0.0 { cursor as u32 } else { 0 };
    let epoch = if epoch > 0.0 { epoch as u32 } else { 0 };
    match unsafe { set_cursor_next_raw(set, cursor, epoch) } {
        Some(idx) => idx as f64,
        None => -1.0,
    }
}
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_SET_CURSOR_NEXT: extern "C" fn(f64, f64, f64) -> f64 = js_set_cursor_next;

/// C-ABI companion: the header's compaction epoch the loop stores after
/// each step.
#[no_mangle]
pub extern "C" fn js_set_compaction_epoch(set_boxed: f64) -> f64 {
    let set = clean_set_ptr(crate::value::js_nanbox_get_pointer(set_boxed) as *const SetHeader);
    if set.is_null() {
        return 0.0;
    }
    set_compaction_epoch(set) as f64
}
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_SET_COMPACTION_EPOCH: extern "C" fn(f64) -> f64 = js_set_compaction_epoch;

unsafe fn compact_set_elements(set: *mut SetHeader) {
    let used = (*set).used as usize;
    let elements = elements_ptr_mut(set);
    let mut out = 0usize;
    // The raw indices squeezed out, for the cursors that were walking them.
    let mut removed: Vec<u32> = Vec::with_capacity(used.saturating_sub((*set).size as usize));
    for i in 0..used {
        let v = ptr::read(elements.add(i));
        if v.to_bits() == SET_HOLE_VALUE_BITS {
            removed.push(i as u32);
            continue;
        }
        if out != i {
            // GC_STORE_AUDIT(EXTERNAL_BARRIERED): the dirty-span barrier below
            // covers every surviving slot this pass writes. Overlap-safe by
            // construction -- `out <= i` always, so a live element only ever
            // moves DOWN within the one buffer, never onto an unread source.
            ptr::write(elements.add(out), v);
        }
        out += 1;
    }
    debug_assert_eq!(out as u32, (*set).size);
    (*set).used = out as u32;
    if !(*set).store.is_null() {
        (*(*set).store).used = (*set).used as usize;
    }
    if out > 0 {
        // GC_STORE_AUDIT(EXTERNAL_BARRIERED): compaction is followed by a dirty-span barrier for every surviving slot.
        crate::gc::replay_old_parent_slot_range_barriers(
            (*set).store as usize,
            elements as *mut u64,
            out,
        );
    }
    rebuild_set_index(set);
    if !removed.is_empty() {
        note_set_compaction(set, SetRemovedSlots::Indices(removed));
    }
}

pub(crate) unsafe fn compact_if_holey_set(set: *mut SetHeader) {
    if (*set).used != (*set).size {
        compact_set_elements(set);
    }
}

/// The lookup a hot `Set.has` / `Set.add` does on a small set of numbers,
/// with nothing else in the frame: a plain (untagged, non-NaN, non-zero)
/// number against the elements by bit identity. `elements[0..used)` is exactly
/// the membership (`delete` compacts, `add` normalises `-0`), and no tagged
/// value equals a number, so a bit match is a hit and a full scan is a miss.
/// Larger sets probe the existing index directly for an admitted Number.
/// Tagged / zero / NaN values and every string keep the general cold path.
#[inline(always)]
unsafe fn find_value_index_hot(set: *const SetHeader, value: f64) -> Option<i32> {
    let bits = value.to_bits();
    if !crate::map::is_plain_nonzero_number_bits(bits) {
        return None;
    }
    let used = (*set).used;
    if used > SMALL_SET_SCAN_MAX {
        // An untagged legacy pointer needs the ordinary identity hash.
        if bits >> 48 == 0 {
            return None;
        }
        return Some(index::lookup_plain_number(set, bits));
    }
    let elements = elements_ptr(set);
    for i in 0..used {
        if ptr::read(elements.add(i as usize)).to_bits() == bits {
            return Some(i as i32);
        }
    }
    Some(-1)
}

#[inline(always)]
pub(crate) unsafe fn find_value_index(set: *const SetHeader, value: f64) -> i32 {
    if let Some(index) = find_value_index_hot(set, value) {
        return index;
    }
    find_value_index_cold(set, value)
}

#[inline(never)]
unsafe fn find_value_index_cold(set: *const SetHeader, value: f64) -> i32 {
    if let Some(index) = index::lookup_value(set, value) {
        return index;
    }
    let elements = elements_ptr(set);
    for i in 0..(*set).used {
        let candidate = ptr::read(elements.add(i as usize));
        if candidate.to_bits() != SET_HOLE_VALUE_BITS && jsvalue_eq(candidate, value) {
            return i as i32;
        }
    }
    -1
}

/// Grow the elements array if needed (header stays at same address)
unsafe fn ensure_capacity(set: *mut SetHeader) -> bool {
    let size = (*set).size;
    let used = (*set).used;
    let capacity = (*set).capacity;

    if used < capacity {
        return false;
    }

    // A full raw extent can still contain holes. Reclaim them before growing,
    // except while `forEach` has a cursor into this layout: moving survivors
    // then would skip entries. The active walk instead grows the buffer and
    // compacts once its outermost frame finishes.
    if size < used && !set_foreach_is_active(set) {
        compact_set_elements(set);
        if (*set).used < capacity {
            return false;
        }
    }

    // Double the capacity. RULE 3: bounded before the realloc is sized, and
    // with `capacity <= 2^31-1` the doubling can no longer wrap a `u32`.
    let new_capacity = crate::object::shape_rule3::checked_plus_four_word(
        capacity * 2,
        b"Set maximum size exceeded",
    );
    let _no_move = crate::gc::GcSuppressScope::new();
    let previous = (*set).store;
    let next = alloc_store(new_capacity as usize);
    ptr::copy_nonoverlapping((*previous).elements, (*next).elements, (*set).used as usize);
    (*next).used = (*set).used as usize;
    (*next).index = (*previous).index.take();
    (*next).compaction = (*previous).compaction.take();
    // GC_STORE_AUDIT(BARRIERED): publish the owned store and copied young slots.
    (*set).elements = (*next).elements;
    (*set).capacity = new_capacity;
    crate::gc::runtime_store_gc_heap_word_slot(
        set as usize,
        std::ptr::addr_of_mut!((*set).store) as usize,
        next as u64,
    );
    crate::gc::replay_old_parent_slot_range_barriers(
        next as usize,
        (*next).elements as *mut u64,
        (*next).used,
    );
    // GC_STORE_AUDIT(POINTER_FREE): Retired previous Set header elements alias cleared to null; no new edge.
    (*previous).elements = ptr::null_mut();
    (*previous).capacity = 0;
    (*previous).used = 0;
    true
}

/// Allocate a new empty set with the given initial capacity
#[no_mangle]
pub extern "C" fn js_set_alloc(capacity: u32) -> *mut SetHeader {
    // RULE 3 (`object/shape_rule3.rs`): `capacity` occupies payload `+4`, the
    // word the emitted property-read path compares against a cached ShapeId.
    // The bound is 2^31-1 elements — a 16 GiB elements block.
    let capacity =
        crate::object::shape_rule3::checked_plus_four_word(capacity, b"Set maximum size exceeded");
    let cap = if capacity == 0 { 4 } else { capacity };
    // The store is the larger young birth, so it is allocated first, while
    // nothing unrooted is live, and keeps the allocation's normal GC pacing.
    // Only the header birth and the publication need the no-move window.
    let store = unsafe { alloc_store(cap as usize) };
    let _no_move = crate::gc::GcSuppressScope::new();
    let ptr =
        crate::arena::arena_alloc_gc(std::mem::size_of::<SetHeader>(), 8, crate::gc::GC_TYPE_SET)
            .cast::<SetHeader>();
    unsafe {
        // GC_STORE_AUDIT(INIT): bounded birth, followed by an exact owner edge barrier.
        (*ptr).size = 0;
        (*ptr).capacity = cap;
        (*ptr).elements = (*store).elements;
        (*ptr).meta = ptr::null_mut();
        (*ptr).used = 0;
        (*ptr).compaction_epoch = 0;
        (*ptr).store = store;
        crate::gc::runtime_write_barrier_gc_slot(
            ptr as usize,
            std::ptr::addr_of!((*ptr).store) as usize,
            store as u64,
        );
        ptr
    }
}

/// Clean a set pointer that might have NaN-box tag bits.
///
/// *Identity* only — "what value did the caller pass?", not "which `SetHeader`
/// does the operation run on". Use [`clean_set_ptr`] for the latter; the two
/// differ for a `class X extends Set` instance (#7570).
#[inline(always)]
fn set_receiver_identity(set: *const SetHeader) -> *const SetHeader {
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

/// Resolve a `Set` receiver to the `SetHeader` the operation must run on.
///
/// The `Set` twin of `map::clean_map_ptr` — see its doc comment for why a
/// *declared* TypeScript type cannot be trusted as a layout fact, and what each
/// arm means. Briefly: genuine `SetHeader` (`GC_TYPE_SET`) passes through, a
/// `class X extends Set` instance is redirected onto its hidden backing, a
/// plain object merely annotated `Set<T>` resolves to null, and anything with
/// no readable `GcHeader` is left exactly as it was pre-#7570.
#[inline(always)]
fn clean_set_ptr(set: *const SetHeader) -> *const SetHeader {
    let set = set_receiver_identity(set);
    let addr = set as usize;
    match unsafe { crate::value::addr_class::try_read_gc_header(addr) } {
        Some(header) if header.obj_type == crate::gc::GC_TYPE_SET => set,
        Some(header) if header.obj_type == crate::gc::GC_TYPE_OBJECT => {
            crate::object::map_set_subclass::redirect_collection_receiver(
                addr,
                crate::object::map_set_subclass::CollectionKind::Set,
            ) as *const SetHeader
        }
        _ => set,
    }
}

/// [`clean_set_ptr`] for entry points OUTSIDE this module — see
/// `map::resolve_map_receiver` (#7570).
#[inline(always)]
pub(crate) fn resolve_set_receiver(set: *const SetHeader) -> *const SetHeader {
    clean_set_ptr(set)
}

/// Run `op` on the RESOLVED collection and return the RECEIVER.
///
/// `Set.prototype.add` returns its receiver; for a `class X extends Set`
/// instance that is the INSTANCE, not the hidden backing (#7570). The
/// `map::map_op_returning_receiver` twin — same zero-extra-cost common case
/// (one pointer compare), same rooting of the movable instance in the subclass
/// case.
#[inline]
fn set_op_returning_receiver(
    set: *mut SetHeader,
    op: impl FnOnce(*mut SetHeader),
) -> *mut SetHeader {
    let receiver = set_receiver_identity(set as *const SetHeader) as *mut SetHeader;
    let resolved = clean_set_ptr(set as *const SetHeader) as *mut SetHeader;
    if resolved.is_null() {
        return receiver;
    }
    if std::ptr::eq(resolved, receiver) {
        op(resolved);
        return receiver;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let handle = scope.root_raw_mut_ptr(receiver);
    let ((), receiver) = handle.across_mut::<SetHeader, _>(|| op(resolved));
    receiver
}

/// Get the number of elements in the set
#[no_mangle]
pub extern "C" fn js_set_size(set: *const SetHeader) -> u32 {
    let set = clean_set_ptr(set);
    if set.is_null() {
        return 0;
    }
    set_size_body(set)
}

#[inline(always)]
fn set_size_body(set: *const SetHeader) -> u32 {
    unsafe { (*set).size }
}

crate::entry_contracts::kind_entry! {
    js_set_size__Set, Set, fn(set: *const SetHeader) -> u32 {
        set_size_body(set)
    }
}

/// Add a value to the set
/// Returns the set pointer (always the same, stable address)
#[no_mangle]
pub extern "C" fn js_set_add(set: *mut SetHeader, value: f64) -> *mut SetHeader {
    set_op_returning_receiver(set, |set| set_add_resolved(set, value))
}

/// `js_set_add`'s body, on a receiver already resolved to a genuine `SetHeader`.
fn set_add_resolved(set: *mut SetHeader, value: f64) {
    let value = normalize_zero(value);
    unsafe {
        // Check if value already exists
        let idx = find_value_index(set, value);

        if idx >= 0 {
            // Value already exists, nothing to do
            return;
        }

        // Value doesn't exist, need to add it
        let grew = ensure_capacity(set);
        let size = (*set).size;
        let used = (*set).used;
        let elements = elements_ptr_mut(set);
        if grew && used > 0 {
            crate::gc::replay_old_parent_slot_range_barriers(
                (*set).store as usize,
                elements as *mut u64,
                used as usize,
            );
        }

        // Write the value
        // GC_STORE_AUDIT(BARRIERED): Set append stores through its owned heap store.
        crate::gc::runtime_store_gc_jsvalue_slot(
            (*set).store as usize,
            elements.add(used as usize) as usize,
            value.to_bits(),
        );

        (*set).size = size + 1;
        (*set).used = used + 1;
        if !(*set).store.is_null() {
            (*(*set).store).used = (*set).used as usize;
        }
        index::insert_value(set, value, used);
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
    set_op_returning_receiver(set, |set| set_add_string_resolved(set, value))
}

/// `js_set_add_string`'s body, on a resolved `SetHeader`.
fn set_add_string_resolved(set: *mut SetHeader, value: *const StringHeader) {
    let value = boxed_heap_string_value(value);
    unsafe {
        let idx = find_value_index(set, value);

        if idx >= 0 {
            return;
        }

        let grew = ensure_capacity(set);
        let size = (*set).size;
        let used = (*set).used;
        let elements = elements_ptr_mut(set);
        if grew && used > 0 {
            crate::gc::replay_old_parent_slot_range_barriers(
                (*set).store as usize,
                elements as *mut u64,
                used as usize,
            );
        }

        // GC_STORE_AUDIT(BARRIERED): Set append stores through its owned heap store.
        crate::gc::runtime_store_gc_jsvalue_slot(
            (*set).store as usize,
            elements.add(used as usize) as usize,
            value.to_bits(),
        );

        (*set).size = size + 1;
        (*set).used = used + 1;
        if !(*set).store.is_null() {
            (*(*set).store).used = (*set).used as usize;
        }
        index::insert_value(set, value, used);
    }
}

#[inline(always)]
fn boxed_i32_value(value: i32) -> f64 {
    f64::from_bits(crate::value::JSValue::int32(value).bits())
}

// The `_i32`/`_u32`/`_f32`/`_bool` specializations delegate to `js_set_add`,
// which now performs the receiver resolution AND returns the receiver identity
// (#7570); pre-resolving here would hand `js_set_add` the backing and lose the
// instance a `class X extends Set` receiver must return.

#[no_mangle]
pub extern "C" fn js_set_add_i32(set: *mut SetHeader, value: i32) -> *mut SetHeader {
    js_set_add(set, boxed_i32_value(value))
}

#[no_mangle]
pub extern "C" fn js_set_add_u32(set: *mut SetHeader, value: u32) -> *mut SetHeader {
    js_set_add(set, f64::from(value))
}

#[no_mangle]
pub extern "C" fn js_set_add_f32(set: *mut SetHeader, value: f32) -> *mut SetHeader {
    js_set_add(set, f64::from(value))
}

#[no_mangle]
pub extern "C" fn js_set_add_bool(set: *mut SetHeader, value: i32) -> *mut SetHeader {
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
    // #7570: a `Set<T>`-annotated binding can be holding a `class X extends
    // Set` instance (a plain ObjectHeader), or a plain object.
    let set = clean_set_ptr(set);
    if set.is_null() {
        return 0;
    }
    set_has_resolved(set, value)
}

#[inline(always)]
fn set_has_resolved(set: *const SetHeader, value: f64) -> i32 {
    let value = normalize_zero(value);
    unsafe { i32::from(find_value_index(set, value) >= 0) }
}

crate::entry_contracts::kind_entry! {
    js_set_has__Set, Set, fn(set: *const SetHeader, value: f64) -> i32 {
        set_has_resolved(set, value)
    }
}

/// Fast `ReadonlySet.has` that preserves TypeScript's structural semantics.
///
/// A `ReadonlySet<T>` annotation is not a native-layout guarantee: ordinary
/// objects, proxies, and Set subclasses can all inhabit it. A genuine
/// `GC_TYPE_SET` receiver takes the direct lookup without the generic method
/// tower. Every other receiver retains normal JavaScript `receiver.has(value)`
/// dispatch, including user overrides and the usual TypeError behavior.
#[no_mangle]
pub unsafe extern "C-unwind" fn js_readonly_set_has(receiver: f64, value: f64) -> f64 {
    let receiver_value = crate::value::JSValue::from_bits(receiver.to_bits());
    if receiver_value.is_pointer() {
        let raw = receiver_value.as_pointer::<SetHeader>();
        if matches!(
            crate::value::addr_class::try_read_gc_header(raw as usize),
            Some(header) if header.obj_type == crate::gc::GC_TYPE_SET
        ) {
            return f64::from_bits(
                crate::value::JSValue::bool(set_has_resolved(raw, value) != 0).bits(),
            );
        }
    }

    readonly_set_has_structural(receiver, value)
}

/// The structural fallback of [`js_readonly_set_has`]: it can allocate and
/// re-enter generated code, so it roots both operands before crossing that
/// boundary and passes refreshed values into the existing dispatcher (which
/// establishes its own roots before the first collecting probe). Out of line
/// so the genuine-`Set` arm above is a leaf: with the handle scope inlined,
/// every `componentTypeSet.has(type)` paid the fallback's full frame.
#[cold]
#[inline(never)]
unsafe fn readonly_set_has_structural(receiver: f64, value: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver_handle = scope.root_nanbox_f64(receiver);
    let value_handle = scope.root_nanbox_f64(value);
    let refreshed_value = value_handle.get_nanbox_f64();
    crate::object::js_native_call_method(
        receiver_handle.get_nanbox_f64(),
        b"has".as_ptr() as *const i8,
        3,
        &refreshed_value,
        1,
    )
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
    // #7570: resolve a `class X extends Set` receiver onto its backing.
    let set = clean_set_ptr(set as *const SetHeader) as *mut SetHeader;
    if set.is_null() {
        return 0;
    }
    let value = normalize_zero(value);
    unsafe {
        let idx = find_value_index(set, value);

        if idx < 0 {
            return 0;
        }

        let size = (*set).size;
        let elements = elements_ptr_mut(set);
        let deleted_value = ptr::read(elements.add(idx as usize));

        // O(1) ordered delete: survivors keep their RAW indices (no move, no
        // span barrier over the tail, no index-offset repair — the costs that
        // made emptying a Set quadratic). The slot takes the reserved hole
        // marker through the barriered store, so SATB marking still shades
        // the overwritten value; iteration skips holes, and delete-then-
        // re-add still appends at the end (#2831). Holes are squeezed out
        // when they outnumber the live elements, or before growing.
        crate::gc::runtime_store_gc_jsvalue_slot(
            (*set).store as usize,
            elements.add(idx as usize) as usize,
            SET_HOLE_VALUE_BITS,
        );

        (*set).size = size - 1;
        index::remove_value(set, deleted_value, idx as u32);

        let used = (*set).used;
        if used >= 16 && (*set).size < used / 2 && !set_foreach_is_active(set) {
            compact_set_elements(set);
        }
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
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_SET_ADD_STRING: extern "C" fn(
    *mut SetHeader,
    *const StringHeader,
) -> *mut SetHeader = js_set_add_string;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_SET_ADD_NUMBER: extern "C" fn(*mut SetHeader, f64) -> *mut SetHeader =
    js_set_add_number;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_SET_HAS_STRING: extern "C" fn(*const SetHeader, *const StringHeader) -> i32 =
    js_set_has_string;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_SET_HAS_NUMBER: extern "C" fn(*const SetHeader, f64) -> i32 = js_set_has_number;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_SET_DELETE_STRING: extern "C" fn(*mut SetHeader, *const StringHeader) -> i32 =
    js_set_delete_string;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_SET_DELETE_NUMBER: extern "C" fn(*mut SetHeader, f64) -> i32 = js_set_delete_number;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_SET_ADD_I32: extern "C" fn(*mut SetHeader, i32) -> *mut SetHeader = js_set_add_i32;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_SET_HAS_I32: extern "C" fn(*const SetHeader, i32) -> i32 = js_set_has_i32;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_SET_DELETE_I32: extern "C" fn(*mut SetHeader, i32) -> i32 = js_set_delete_i32;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_SET_ADD_U32: extern "C" fn(*mut SetHeader, u32) -> *mut SetHeader = js_set_add_u32;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_SET_HAS_U32: extern "C" fn(*const SetHeader, u32) -> i32 = js_set_has_u32;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_SET_DELETE_U32: extern "C" fn(*mut SetHeader, u32) -> i32 = js_set_delete_u32;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_SET_ADD_F32: extern "C" fn(*mut SetHeader, f32) -> *mut SetHeader = js_set_add_f32;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_SET_HAS_F32: extern "C" fn(*const SetHeader, f32) -> i32 = js_set_has_f32;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_SET_DELETE_F32: extern "C" fn(*mut SetHeader, f32) -> i32 = js_set_delete_f32;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_SET_ADD_BOOL: extern "C" fn(*mut SetHeader, i32) -> *mut SetHeader = js_set_add_bool;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_SET_HAS_BOOL: extern "C" fn(*const SetHeader, i32) -> i32 = js_set_has_bool;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_SET_DELETE_BOOL: extern "C" fn(*mut SetHeader, i32) -> i32 = js_set_delete_bool;

/// Clear all elements from the set
#[no_mangle]
pub extern "C" fn js_set_clear(set: *mut SetHeader) {
    // #7570: resolve a `class X extends Set` receiver onto its backing.
    let set = clean_set_ptr(set as *const SetHeader) as *mut SetHeader;
    if set.is_null() {
        return;
    }
    unsafe {
        let active_foreach = set_foreach_is_active(set);
        let extent = (*set).used;
        // The side-table mirrors the elements exactly, so an already-empty
        // set has nothing to reset — half of a change set's per-entity
        // `adds.clear(); removes.clear()` — and skips the table probe.
        if (*set).size == 0 {
            if !active_foreach && extent > 0 {
                (*set).used = 0;
                if !(*set).store.is_null() {
                    (*(*set).store).used = (*set).used as usize;
                }
                note_set_compaction(set, SetRemovedSlots::Prefix(extent));
            }
            return;
        }
        (*set).size = 0;
        if active_foreach {
            // ECMA-262 keeps the current [[SetData]] list in place while a
            // forEach is walking it. Mark every live slot empty so the cursor
            // skips them; values appended after clear remain after this raw
            // extent and are therefore still visited.
            let used = (*set).used as usize;
            let elements = elements_ptr_mut(set);
            for i in 0..used {
                if ptr::read(elements.add(i)).to_bits() != SET_HOLE_VALUE_BITS {
                    crate::gc::runtime_store_gc_jsvalue_slot(
                        (*set).store as usize,
                        elements.add(i) as usize,
                        SET_HOLE_VALUE_BITS,
                    );
                }
            }
        } else {
            // Discarding the extent moves nothing, but a live raw-index
            // cursor must restart at 0 to see what is appended next (spec:
            // [[SetData]] is emptied in place, later adds are visited), so
            // record it as a squeeze of the whole extent.
            (*set).used = 0;
            if !(*set).store.is_null() {
                (*(*set).store).used = (*set).used as usize;
            }
            note_set_compaction(set, SetRemovedSlots::Prefix(extent));
        }
    }
    unsafe {
        index::clear_index(set);
    }
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
        // LIVE-index accessor (#9462 / #9504): `i` counts live elements, the
        // contract `js_array_length` (== `size`) pairs with for the array-like
        // `set[i]` read, `console.table` and collection equality. Tombstones
        // are squeezed first so raw index == live index — a one-off O(n) for
        // those paths only, RECORDED in the compaction log so an open
        // `for…of` cursor rebases exactly. The walkers read through
        // `js_set_value_raw_at`, which never compacts.
        //
        // While a `forEach` walk is active the squeeze is DEFERRED (its raw
        // counter is not epoch-rebased; compacting under it skipped entries)
        // and the live index is translated by stepping over holes; the
        // outermost walk's completion performs the deferred squeeze.
        let Some(raw) = live_index_to_raw_set(set, i) else {
            return f64::from_bits(UNDEF);
        };
        let elements = (*set).elements as *const f64;
        ptr::read(elements.add(raw as usize))
    }
}

/// Resolve a LIVE element index onto the raw elements buffer — see
/// `map::live_index_to_raw`.
unsafe fn live_index_to_raw_set(set: *const SetHeader, idx: u32) -> Option<u32> {
    if idx >= (*set).size {
        return None;
    }
    if !set_foreach_is_active(set) {
        compact_if_holey_set(set as *mut SetHeader);
        return Some(idx);
    }
    let used = (*set).used;
    let elements = elements_ptr(set);
    let mut live = 0u32;
    let mut raw = 0u32;
    while raw < used {
        if ptr::read(elements.add(raw as usize)).to_bits() != SET_HOLE_VALUE_BITS {
            if live == idx {
                return Some(raw);
            }
            live += 1;
        }
        raw += 1;
    }
    None
}

/// RAW twin of `js_set_value_at` for the raw-index walkers (the `for…of`
/// fast path's `SetValueAt` lowering): bounded by the raw extent `used`,
/// never compacts. The index comes from `js_set_cursor_next`, which only
/// yields LIVE raw indices, so no hole reaches user code.
#[no_mangle]
pub extern "C" fn js_set_value_raw_at(set: *const SetHeader, i: u32) -> f64 {
    const UNDEF: u64 = 0x7FFC_0000_0000_0001;
    let set = clean_set_ptr(set);
    if set.is_null() {
        return f64::from_bits(UNDEF);
    }
    unsafe {
        if i >= (*set).used {
            return f64::from_bits(UNDEF);
        }
        let elements = (*set).elements as *const f64;
        ptr::read(elements.add(i as usize))
    }
}
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_SET_VALUE_RAW_AT: extern "C" fn(*const SetHeader, u32) -> f64 = js_set_value_raw_at;

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
    // Raw element walk below: squeeze holes out first.
    unsafe {
        let resolved = clean_set_ptr(set);
        if !resolved.is_null() {
            compact_if_holey_set(resolved as *mut SetHeader);
        }
    }
    // #7570: resolve a `class X extends Set` receiver onto its backing.
    let set = clean_set_ptr(set);
    if set.is_null() {
        return crate::array::js_array_alloc(0);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let set_handle = scope.root_raw_const_ptr(set);
    unsafe {
        let set = set_handle.get_raw_const_ptr::<SetHeader>();
        let size = (*set).size as usize;
        let array = crate::array::js_array_alloc(size as u32);
        let result_handle = scope.root_raw_mut_ptr(array);
        maybe_force_helper_gc_for_test();
        if size > 0 {
            let set = set_handle.get_raw_const_ptr::<SetHeader>();
            let array = result_handle.get_raw_mut_ptr::<crate::array::ArrayHeader>();
            let src = (*set).elements as *const f64;
            let dst = crate::array::array_elements_ptr(array as *const crate::array::ArrayHeader)
                as *mut f64;
            // GC_STORE_AUDIT(BARRIERED): Set-to-array bulk copy is followed by exact layout/barrier rebuild.
            ptr::copy_nonoverlapping(src, dst, size);
            (*array).length = size as u32;
            crate::array::rebuild_array_layout_exact(array);
        }
        let array = result_handle.get_raw_mut_ptr::<crate::array::ArrayHeader>();
        crate::array::mark_materialized_iterator(array, 2)
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
    set_init_iterable(js_set_alloc(4), value)
}

/// The sole iterable initializer, on the receiver with its final prototype.
pub(crate) fn set_init_iterable(set: *mut SetHeader, value: f64) -> *mut SetHeader {
    use crate::collection_iter::{constructor_iter, ConstructorIter};

    if crate::collection_iter::is_null_or_undefined(value) {
        return set;
    }

    let scope = crate::gc::RuntimeHandleScope::new();
    let value_handle = scope.root_nanbox_f64(value);
    let set_handle = scope.root_raw_mut_ptr(set);
    let adder = crate::collection_iter::require_callable(
        set_handle.with_mut_ptr::<SetHeader, _>(|set| {
            crate::collection_iter::constructor_adder(
                "add",
                crate::value::js_nanbox_pointer(set as i64),
            )
        }),
        "Set.prototype.add",
    );
    let adder = crate::collection_iter::normalize_callable_value(adder);
    let adder_handle = scope.root_nanbox_f64(adder);

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
    js_set_foreach_impl(set, callback, this_arg, collection_override(set));
}

/// The `Set` twin of `map::collection_override` — see its doc (#7570).
#[inline(always)]
fn collection_override(set: *const SetHeader) -> f64 {
    let receiver = set_receiver_identity(set);
    let resolved = clean_set_ptr(set);
    if resolved.is_null() || std::ptr::eq(resolved, receiver) {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    crate::value::js_nanbox_pointer(receiver as i64)
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
    // Raw element walk below: squeeze holes out first. `Set.prototype.forEach`
    // walks raw slots to `size`, so a tombstone was yielded to the callback as
    // the raw marker AND the walk ended early, dropping live elements past it.
    unsafe {
        let resolved = clean_set_ptr(set);
        if !resolved.is_null() && !set_foreach_is_active(resolved) {
            compact_if_holey_set(resolved as *mut SetHeader);
        }
    }
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
    set_foreach_enter(set);
    unsafe {
        // ECMA-262 24.2.3.6: Set.prototype.forEach iterates [[SetData]] in
        // insertion order. `used` is the raw [[SetData]] extent: unlike `size`,
        // it still spans tombstones left by callback-side deletes. Walk that
        // extent, skipping holes, and re-read it each step so values appended
        // during the callback are visited too. Bounding this by the live count
        // surfaced holes and stopped before later live values (#9072).
        let mut i = 0usize;
        loop {
            let set = set_handle.get_raw_const_ptr::<SetHeader>();
            if i >= (*set).used as usize {
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
            i += 1;
            if value.to_bits() == SET_HOLE_VALUE_BITS {
                continue;
            }
            let args = [value, value, set_value];
            let cb = callback_handle.get_nanbox_f64();
            let this_v = crate::closure::JsThis::from_f64(this_handle.get_nanbox_f64());
            let _ = crate::closure::native_call_value_this(cb, this_v, args.as_ptr(), args.len());
        }
    }
    let set = set_handle.get_raw_const_ptr::<SetHeader>();
    if set_foreach_leave(set) {
        unsafe { compact_if_holey_set(set as *mut SetHeader) };
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
    // Raw element walk below: squeeze holes out first, exactly as
    // `js_set_is_subset_of` does. Without this a tombstone is read as an
    // element -- it leaks into the result as the raw marker, and the walk
    // stops at `size` so live elements past the last hole are dropped.
    unsafe {
        if !set.is_null() {
            compact_if_holey_set(set as *mut SetHeader);
        }
    }
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
    // Raw element walk below: squeeze holes out first, exactly as
    // `js_set_is_subset_of` does. Without this a tombstone is read as an
    // element -- it leaks into the result as the raw marker, and the walk
    // stops at `size` so live elements past the last hole are dropped.
    unsafe {
        if !set.is_null() {
            compact_if_holey_set(set as *mut SetHeader);
        }
    }
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
    // Raw element walk below: squeeze holes out first, exactly as
    // `js_set_is_subset_of` does. Without this a tombstone is read as an
    // element -- it leaks into the result as the raw marker, and the walk
    // stops at `size` so live elements past the last hole are dropped.
    unsafe {
        if !set.is_null() {
            compact_if_holey_set(set as *mut SetHeader);
        }
    }
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
    // Raw element walk below: squeeze holes out first, exactly as
    // `js_set_is_subset_of` does. Without this a tombstone is read as an
    // element -- it leaks into the result as the raw marker, and the walk
    // stops at `size` so live elements past the last hole are dropped.
    unsafe {
        if !set.is_null() {
            compact_if_holey_set(set as *mut SetHeader);
        }
    }
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
    // Raw element walk below: squeeze holes out first.
    unsafe {
        let resolved = clean_set_ptr(set);
        if !resolved.is_null() {
            compact_if_holey_set(resolved as *mut SetHeader);
        }
    }
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
    // Raw element walk below: squeeze holes out first, exactly as
    // `js_set_is_subset_of` does. Without this a tombstone is read as an
    // element -- it leaks into the result as the raw marker, and the walk
    // stops at `size` so live elements past the last hole are dropped.
    unsafe {
        if !set.is_null() {
            compact_if_holey_set(set as *mut SetHeader);
        }
    }
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
    // Raw element walk below: squeeze holes out first.
    unsafe {
        let resolved = clean_set_ptr(set);
        if !resolved.is_null() {
            compact_if_holey_set(resolved as *mut SetHeader);
        }
    }
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
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_SET_UNION: extern "C" fn(*const SetHeader, f64) -> *mut SetHeader = js_set_union;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_SET_INTERSECTION: extern "C" fn(*const SetHeader, f64) -> *mut SetHeader =
    js_set_intersection;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_SET_DIFFERENCE: extern "C" fn(*const SetHeader, f64) -> *mut SetHeader =
    js_set_difference;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_SET_SYMDIFF: extern "C" fn(*const SetHeader, f64) -> *mut SetHeader =
    js_set_symmetric_difference;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_SET_IS_SUBSET: extern "C" fn(*const SetHeader, f64) -> i32 = js_set_is_subset_of;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_SET_IS_SUPERSET: extern "C" fn(*const SetHeader, f64) -> i32 = js_set_is_superset_of;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
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

    #[test]
    fn native_handles_are_set_identities_across_scan_and_hash_paths() {
        use crate::value::{addr_class, POINTER_TAG};
        let handles = [
            1,
            4095,
            4096,
            4097,
            8192,
            65535,
            addr_class::COMMON_HANDLE_BAND_END - 1,
            addr_class::FETCH_HANDLE_BAND_START,
            addr_class::FETCH_HANDLE_BAND_END,
            addr_class::HANDLE_BAND_MAX - 1,
        ];
        let set = js_set_alloc(4);
        for &handle in &handles {
            let value = f64::from_bits(POINTER_TAG | handle as u64);
            assert_eq!(js_set_has(set, value), 0);
            js_set_add(set, value);
            assert_eq!(js_set_has(set, value), 1);
        }
        assert_eq!(js_set_size(set), handles.len() as u32);
        for &handle in &handles {
            let value = f64::from_bits(POINTER_TAG | handle as u64);
            assert_eq!(js_set_has(set, value), 1);
            assert_eq!(js_set_has(set, handle as f64), 0);
            assert_eq!(js_set_delete(set, value), 1);
            assert_eq!(js_set_has(set, value), 0);
        }
        assert_eq!(js_set_size(set), 0);
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
            let used = (*set).used as usize;
            (0..used)
                .map(|i| *(elements_ptr(set).add(i)))
                .filter(|v| v.to_bits() != SET_HOLE_VALUE_BITS)
                .collect()
        }
    }

    #[test]
    fn small_set_scan_lane_agrees_with_the_side_table_on_every_value_shape() {
        let set = js_set_alloc(4);
        for value in 1..=4 {
            js_set_add(set, value as f64);
        }
        for value in 1..=4 {
            assert_eq!(js_set_has(set, value as f64), 1);
        }
        assert_eq!(js_set_has(set, 5.0), 0);
        assert_eq!(js_set_has(set, 2.5), 0);
        assert_eq!(js_set_has(set, -1.0), 0);
        // Zero, -0 and NaN are the side-table's (SameValueZero).
        js_set_add(set, -0.0);
        assert_eq!(js_set_has(set, 0.0), 1);
        js_set_add(set, f64::NAN);
        assert_eq!(js_set_has(set, f64::from_bits(0x7FF8_0000_0000_0001)), 1);
        // A tagged value never takes the scan.
        let boxed_true = f64::from_bits(crate::value::TAG_TRUE);
        js_set_add(set, boxed_true);
        assert_eq!(js_set_has(set, boxed_true), 1);
        assert_eq!(js_set_has(set, 1.0), 1);
        // Delete compacts, so the scan keeps seeing exactly the members.
        js_set_delete(set, 2.0);
        assert_eq!(js_set_has(set, 2.0), 0);
        assert_eq!(js_set_has(set, 3.0), 1);
        assert_eq!(js_set_has(set, 4.0), 1);
        // Re-adding a present number is a no-op for the size.
        let size = js_set_size(set);
        js_set_add(set, 3.0);
        assert_eq!(js_set_size(set), size);
        // Clearing an empty set is a no-op that leaves it usable; clearing a
        // populated one resets the side-table (the re-added value is found,
        // the removed ones are not).
        let empty = js_set_alloc(2);
        js_set_clear(empty);
        js_set_add(empty, 3.0);
        assert_eq!(js_set_has(empty, 3.0), 1);
        js_set_clear(set);
        assert_eq!(js_set_size(set), 0);
        assert_eq!(js_set_has(set, 1.0), 0);
        js_set_add(set, 1.0);
        assert_eq!(js_set_has(set, 1.0), 1);
        js_set_clear(set);
        // Restore the members the tail below expects (2 was deleted above).
        for value in [1.0, 3.0, 4.0] {
            js_set_add(set, value);
        }
        // Growing past the scan bound hands every lookup to the side-table.
        for value in 100..120 {
            js_set_add(set, value as f64);
        }
        for value in 100..120 {
            assert_eq!(js_set_has(set, value as f64), 1);
        }
        assert_eq!(js_set_has(set, 1.0), 1);
        assert_eq!(js_set_has(set, 2.0), 0);
        assert_eq!(js_set_has(set, 120.0), 0);
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
            !test_set_index_contains(set, 0.0),
            "small numeric Sets should not allocate an index"
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

    /// `gc_element_slot_range`'s defensive tripwire must reject an
    /// `elements` word that is not a plausible heap address — a NaN-boxed
    /// JSValue (top bits 0x7FFD), a low garbage address below the handle
    /// band, and a handle-band id — while still accepting a genuine
    /// system-allocator `elements` pointer. The previous `>> 47` cutoff
    /// only caught the NaN-box signature and accepted low / handle-band
    /// garbage; the unified `is_plausible_heap_addr` classifier closes
    /// both gaps and is platform-correct.
    #[test]
    fn test_gc_element_slot_range_rejects_implausible_elements() {
        // Genuine Set: elements is a real system-allocator pointer.
        let set = js_set_alloc(4);
        js_set_add(set, 1.0);
        assert!(
            unsafe { gc_element_slot_range(set) }.is_some(),
            "genuine Set elements must yield a slot range"
        );

        // Build a stack SetHeader so we can control `elements` directly.
        let mut header = SetHeader {
            size: 1,
            capacity: 4,
            elements: std::ptr::null_mut(),
            meta: std::ptr::null_mut(),
            used: 1,
            compaction_epoch: 0,
            store: std::ptr::null_mut(),
        };

        let cases: &[(&str, *mut f64)] = &[
            // NaN-boxed JSValue (POINTER_TAG 0x7FFD) — the fabricated-Map
            // signature that crashes on dereference.
            ("nan-box", 0x7FFD_0000_0000_1000u64 as *mut f64),
            // Low address below the handle band — the old `>> 47` check
            // accepted this (0 >> 47 == 0), letting the collector derive a
            // slot range from an unmapped / unrelated low address.
            ("low-addr", 0x4000u64 as *mut f64),
            // Handle-band id (within the band below HANDLE_BAND_MAX).
            ("handle-band", 0x80000u64 as *mut f64),
        ];

        for &(label, fake_elements) in cases {
            header.elements = fake_elements;
            let got = unsafe { gc_element_slot_range(&mut header as *mut SetHeader) };
            assert!(
                got.is_none(),
                "elements={label} must be rejected by the tripwire, got {got:?}"
            );
        }
    }
}

#[cfg(test)]
mod ordered_delete_repair_tests {
    use super::*;

    /// Insertion order must survive a delete from the middle, and every
    /// surviving element must still be findable afterwards — i.e. the repaired
    /// index must agree with the compacted buffer exactly as a full rebuild
    /// would. Deletes from the front, middle and back are all exercised
    /// because each moves a different number of slots.
    #[test]
    fn ordered_delete_preserves_order_and_repairs_the_index() {
        let set = js_set_alloc(8);
        for v in [10.0f64, 20.0, 30.0, 40.0, 50.0] {
            js_set_add(set, v);
        }

        // middle
        assert_eq!(js_set_delete(set, 30.0), 1);
        // front
        assert_eq!(js_set_delete(set, 10.0), 1);
        // back
        assert_eq!(js_set_delete(set, 50.0), 1);

        unsafe {
            assert_eq!((*set).size, 2, "three of five removed");
            compact_if_holey_set(set);
            let elements = elements_ptr(set);
            assert_eq!(
                ptr::read(elements),
                20.0,
                "survivors keep insertion order after compaction"
            );
            assert_eq!(ptr::read(elements.add(1)), 40.0);
        }

        // The repaired index must still resolve every survivor, and must not
        // resolve anything removed.
        assert_eq!(js_set_has(set, 20.0), 1);
        assert_eq!(js_set_has(set, 40.0), 1);
        for gone in [10.0f64, 30.0, 50.0] {
            assert_eq!(js_set_has(set, gone), 0, "{gone} was deleted");
        }

        // A re-add appends at the end (delete-then-re-add ordering, #2831).
        js_set_add(set, 30.0);
        unsafe {
            compact_if_holey_set(set);
            let elements = elements_ptr(set);
            assert_eq!(ptr::read(elements.add(2)), 30.0);
        }
        assert_eq!(js_set_has(set, 30.0), 1);
    }

    /// Emptying a Set one element at a time must leave a consistent, empty
    /// structure — the case whose per-delete rebuild was quadratic.
    #[test]
    fn emptying_a_set_leaves_it_consistent() {
        let set = js_set_alloc(16);
        for i in 0..64 {
            js_set_add(set, i as f64);
        }
        for i in 0..64 {
            assert_eq!(js_set_delete(set, i as f64), 1, "element {i} deletes once");
            assert_eq!(js_set_delete(set, i as f64), 0, "and only once");
        }
        unsafe {
            assert_eq!((*set).size, 0);
        }
        for i in 0..64 {
            assert_eq!(js_set_has(set, i as f64), 0);
        }
        js_set_add(set, 7.0);
        assert_eq!(
            js_set_has(set, 7.0),
            1,
            "the emptied Set still accepts adds"
        );
    }
}

#[cfg(test)]
#[path = "set_tombstone_tests.rs"]
mod set_tombstone_tests;

#[cfg(test)]
pub(crate) fn test_enter_catch_foreach(mark: usize) {
    set_foreach_enter_at(mark, js_set_alloc(4));
}

#[cfg(test)]
pub(crate) unsafe fn test_remove_compaction_history(set: *mut SetHeader) {
    release_set_compaction(set);
}
