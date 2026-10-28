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
    if (*header).obj_type != gc::GC_TYPE_ARRAY || (*header).gc_flags & gc::GC_FLAG_ARENA == 0 {
        return false;
    }
    gc::set_forwarding_address(header, new_user_addr);
    true
}

/// No safepoint or user code. The replacement is unpublished and a dense
/// source's full logical length was copied before retiring its old backing.
pub(super) unsafe fn initialize_growth_slack(arr: *mut ArrayHeader, used: usize, capacity: usize) {
    let elements = array_elements_ptr(arr) as *mut u64;
    for i in used..capacity {
        // GC_STORE_AUDIT(INIT): unpublished slack contains only TAG_HOLE.
        elements.add(i).write(value::TAG_HOLE);
    }
}
