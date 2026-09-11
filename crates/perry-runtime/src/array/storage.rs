//! Dense queues keep their live elements in a contiguous suffix of the allocation.
//!
//! `capacity` is the capacity remaining AFTER the front offset. The GC header
//! already records the allocation's byte size, so subtracting the remaining
//! capacity from its physical capacity recovers the offset. All length/capacity
//! bounds and sparse-index rules keep their usual meaning. No extra allocation,
//! metadata slot, header word, side table, or survivor copy is needed.
//!
//! GC layouts index the logical live range returned by `array_elements_ptr`.
//! Removing the front invalidates indexed masks, but preserves pointer-free and
//! all-pointer proofs. Surviving slots stay at the same addresses, so their
//! old-to-young barriers remain valid. Growth copies the logical range into a
//! new, unshifted allocation and replays its barriers normally.

use super::ArrayHeader;

#[inline]
pub(crate) unsafe fn array_physical_capacity(arr: *const ArrayHeader) -> usize {
    let header = (arr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    ((*header).size as usize - crate::gc::GC_HEADER_SIZE - std::mem::size_of::<ArrayHeader>()) / 8
}

/// Offset of logical element zero within the original inline backing store.
/// The caller must supply a live, forwarding-resolved real array.
#[inline]
pub(crate) unsafe fn array_front_offset(arr: *const ArrayHeader) -> usize {
    array_physical_capacity(arr) - (*arr).capacity as usize
}

/// Address of the logical element storage, including any lazy queue offset.
///
/// # Safety
/// `arr` must be a live, forwarding-resolved GC_TYPE_ARRAY. As with other GC
/// interior pointers, the result must not be retained across a safepoint.
#[inline]
pub unsafe fn array_elements_ptr(arr: *const ArrayHeader) -> *mut u64 {
    (arr.add(1) as *mut u64).add(array_front_offset(arr))
}

/// Remove one ordinary dense slot without relocating any surviving element.
/// Receiver/property checks happen before entry and nothing here can collect.
pub(super) unsafe fn shift_dense(arr: *mut ArrayHeader) -> f64 {
    let front = array_front_offset(arr);
    let first = array_elements_ptr(arr);
    let bits = *first;
    // GC_STORE_AUDIT(BARRIERED): clearing a removed slot introduces no edge;
    // the live range and logical layout are updated below without a safepoint.
    first.write(crate::value::TAG_HOLE);
    (*arr).length -= 1;
    super::element_shape::clear_element_shape(arr);
    if (*arr).length == 0 {
        (*arr).capacity += front as u32;
        super::rebuild_array_layout(arr);
    } else {
        (*arr).capacity -= 1;
        let flags = super::header::array_object_flags_resolved(arr);
        let layout = flags & (crate::gc::GC_LAYOUT_STATE_MASK | crate::gc::GC_LAYOUT_ALL_POINTERS);
        if layout != crate::gc::GC_LAYOUT_POINTER_FREE
            && layout != (crate::gc::GC_LAYOUT_SIDE_MASK | crate::gc::GC_LAYOUT_ALL_POINTERS)
            && flags & crate::gc::GC_LAYOUT_STATE_MASK != 0
        {
            // Per-index masks described the old logical indices. UNKNOWN
            // traces all live slots; no survivor is reclassified here.
            crate::gc::layout_mark_unknown(arr.cast());
        }
    }
    if bits == crate::value::TAG_HOLE {
        f64::from_bits(crate::value::TAG_UNDEFINED)
    } else {
        f64::from_bits(bits)
    }
}
