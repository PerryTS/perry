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
    let mut visitor = crate::gc::RuntimeRootVisitor::for_copy(mark);
    scan_template_raw_roots_mut(&mut visitor);
}

pub fn scan_template_raw_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
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
pub(crate) fn clean_arr_ptr_mut(arr: *mut ArrayHeader) -> *mut ArrayHeader {
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
    crate::gc::layout_note_slot(arr as usize, index, value_bits);
    let slot = array_elements_ptr(arr).add(index) as usize;
    crate::gc::runtime_write_barrier_slot(arr as usize, slot, value_bits);
}

#[inline]
pub(crate) unsafe fn rebuild_array_layout(arr: *mut ArrayHeader) {
    if arr.is_null() {
        return;
    }
    let length = (*arr).length as usize;
    let capacity = (*arr).capacity as usize;
    if length > capacity || length > 16_000_000 {
        crate::gc::layout_mark_unknown(arr as *mut u8);
        return;
    }
    crate::gc::layout_rebuild_from_slots(arr as *mut u8, array_elements_ptr(arr), length);
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
        crate::gc::layout_mark_unknown(arr as *mut u8);
        return;
    }
    crate::gc::layout_rebuild_exact_from_slots(arr as *mut u8, array_elements_ptr(arr), length);
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
    crate::gc::layout_mark_unknown(arr as *mut u8);
}

/// Minimum initial capacity for arrays to reduce reallocations
pub(crate) const MIN_ARRAY_CAPACITY: u32 = 16;
