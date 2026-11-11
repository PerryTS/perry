//! No-safepoint helpers for replacing array backing storage.
use super::{array_elements_ptr, ArrayHeader};
use crate::{gc, value};

/// Install an array-growth forwarding stub after `tracked_header_for` proves
/// allocator ownership of `old_user_addr`. Keeping the classifier injectable
/// makes the below-2-TiB macOS case deterministic without mapping a fixed low
/// virtual address in the test process.
///
/// # Safety
/// The classifier must return the live header for `old_user_addr`, and
/// `new_user_addr` must be a live array allocation.
#[inline]
pub(in crate::array) unsafe fn install_array_growth_forwarding_with(
    old_user_addr: usize,
    new_user_addr: *mut u8,
    tracked_header_for: impl FnOnce(usize) -> Option<std::ptr::NonNull<gc::GcHeader>>,
) -> bool {
    let Some(header) = tracked_header_for(old_user_addr) else {
        return false;
    };
    let header = header.as_ptr();
    if crate::gc_type_is_not_array!((*header).obj_type)
        || (*header).gc_flags & gc::GC_FLAG_ARENA == 0
    {
        return false;
    }
    gc::set_forwarding_address(header, new_user_addr);
    crate::array::forwarding::note_array_growth_stub();
    true
}

/// No safepoint or user code. The replacement is unpublished and a dense
/// source's full logical length was copied before retiring its old backing.
pub(super) unsafe fn initialize_growth_slack(arr: *mut ArrayHeader, used: usize, capacity: usize) {
    let elements = array_elements_ptr(arr) as *mut u64;
    // Through `black_box` the pattern is not a constant, so the loop stays a
    // few inline vector stores instead of becoming a `memset_pattern16` call,
    // whose setup cost more than a 16- or 32-slot growth slack itself.
    let hole = std::hint::black_box(value::TAG_HOLE);
    for i in used..capacity {
        // GC_STORE_AUDIT(INIT): unpublished slack contains only TAG_HOLE.
        elements.add(i).write(hole);
    }
}

/// Lengthen `arr` where it stands when it is the most recent allocation in
/// the nursery block the inline bump state has open: bump that state by the
/// extra slots and widen the array over them. True when it did.
///
/// Elements sit at the tail of an array's allocation
/// (`storage::array_elements_ptr`: the end of the cell minus `capacity`
/// words), so growing the cell's size and `capacity` by the same number of
/// words leaves the element base, the front offset, any named-property
/// reserve and every copied slot exactly where they were. The new words are
/// filled with `TAG_HOLE`, as a fresh slack would be.
///
/// Equivalent to the copying grow it replaces for everything that can
/// observe the heap, because the extended cell is indistinguishable from a
/// fresh birth of the larger size at the same address:
/// * the bump moves exactly as `arena::try_inline_bump` would for the extra
///   bytes, under the same refusals (an open block, the permitted bound — 0
///   while births are black — no allocation sampling, the inline size cap);
/// * the object-start bitmap records starts only, and the start is unchanged;
/// * the header's `_reserved` layout facts, `length`, `named` and the EK word
///   are untouched, and address-keyed records stay valid because the address
///   does;
/// * the size stays a young, small cell (no large-object storage), and a
///   tenured, forwarded or marked header never qualifies.
///
/// No allocation, safepoint or user code.
///
/// # Safety
/// `arr` is a live, forwarding-resolved array head, `header` its GC header,
/// and `new_capacity > (*arr).capacity`.
#[inline(always)]
pub(super) unsafe fn try_extend_in_place(
    arr: *mut ArrayHeader,
    header: *mut gc::GcHeader,
    new_capacity: u32,
) -> bool {
    #[cfg(test)]
    if !IN_PLACE_IN_TESTS.with(std::cell::Cell::get) {
        return false;
    }
    let old_capacity = (*arr).capacity;
    debug_assert!(new_capacity > old_capacity);
    if (*header).gc_flags & (gc::GC_FLAG_TENURED | gc::GC_FLAG_FORWARDED | gc::GC_FLAG_MARKED) != 0
        || (*header).gc_flags & gc::GC_FLAG_ARENA == 0
    {
        return false;
    }
    let old_total = (*header).size as usize;
    let extra = (new_capacity - old_capacity) as usize * std::mem::size_of::<u64>();
    let new_total = old_total + extra;
    // Same refusals as the inline bump, plus the large-object ceiling a fresh
    // birth of `new_total` bytes would have to respect.
    if new_total >= 16 * 1024
        || gc::is_large_object_total_size_for_type(new_total, gc::GC_TYPE_ARRAY)
        || !crate::arena::alloc_sample::runtime_bump_permitted()
    {
        return false;
    }
    let state = &mut *crate::arena::hot_inline_state();
    if state.data.is_null() {
        return false;
    }
    let cursor = state.data as usize + state.offset;
    if header as usize + old_total != cursor {
        return false;
    }
    let next = state.offset + extra;
    if next > state.size {
        return false;
    }
    state.offset = next;
    // GC_STORE_AUDIT(POINTER_FREE): the cell's scalar size word, widened over
    // bytes this thread's bump region just handed to it.
    (*header).size = new_total as u32;
    (*arr).capacity = new_capacity;
    // The element base is unchanged (see above); the slots past the old
    // capacity are the new words.
    initialize_growth_slack(arr, old_capacity as usize, new_capacity as usize);
    true
}

#[cfg(test)]
thread_local! {
    static IN_PLACE_IN_TESTS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Unit tests opt in to growth in place. Many runtime fixtures grow the most
/// recent birth precisely to obtain a forwarding stub (their premise is
/// "pushing past capacity reallocates"), so under `cfg(test)` growth copies
/// unless this guard is held; the in-place arm's own tests hold it.
#[cfg(test)]
pub(crate) struct GrowInPlaceTestGuard(bool);

#[cfg(test)]
impl GrowInPlaceTestGuard {
    pub(crate) fn on() -> Self {
        Self(IN_PLACE_IN_TESTS.with(|c| c.replace(true)))
    }
}

#[cfg(test)]
impl Drop for GrowInPlaceTestGuard {
    fn drop(&mut self) {
        IN_PLACE_IN_TESTS.with(|c| c.set(self.0));
    }
}
