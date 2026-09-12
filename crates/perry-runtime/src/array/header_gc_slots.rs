//! Array element-slot GC bookkeeping: slot ranges, write-barrier notes, and
//! layout rebuild/replay for `ArrayHeader`.
//!
//! Split out of `header.rs` to keep it under the 2,000-line file gate.

use super::header::*;
use super::ArrayHeader;

#[cfg(test)]
thread_local! {
    static DENSE_MOVE_LAYOUT_CLASSIFIED_SLOTS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn test_reset_dense_move_layout_classified_slots() {
    DENSE_MOVE_LAYOUT_CLASSIFIED_SLOTS.with(|count| count.set(0));
}

#[cfg(test)]
pub(crate) fn test_dense_move_layout_classified_slots() -> usize {
    DENSE_MOVE_LAYOUT_CLASSIFIED_SLOTS.with(std::cell::Cell::get)
}

#[inline]
fn note_layout_classified_slots(count: usize) {
    #[cfg(test)]
    DENSE_MOVE_LAYOUT_CLASSIFIED_SLOTS.with(|total| total.set(total.get() + count));
    #[cfg(not(test))]
    let _ = count;
}

pub(crate) unsafe fn gc_element_slot_range(
    arr: *mut ArrayHeader,
) -> Option<crate::gc::HeapSlotRange> {
    if arr.is_null() {
        return None;
    }
    let length = (*arr).length as usize;
    let capacity = (*arr).capacity as usize;
    if capacity > 16_000_000 || capacity > super::array_physical_capacity(arr) {
        return None;
    }
    if length > capacity {
        // Preserve the old corruption fail-closed behavior while admitting
        // legitimate sparse headers: the remaining capacity must fit the
        // tracked allocation, allowing the consumed prefix of a dense queue.
        let Some(gc_header) = crate::value::addr_class::try_read_tracked_gc_header(arr as usize)
        else {
            return None;
        };
        if checked_array_allocation_size(super::array_physical_capacity(arr))
            != Some((*gc_header.as_ptr()).size as usize)
        {
            return None;
        }
    }
    // A large `new Array(length)` intentionally starts with `length >
    // capacity`. Only the allocated dense prefix can contain inline child
    // edges; indices beyond it live in the separately-rooted named-property
    // table. Returning no range here would lose pointer values written into
    // that prefix while it grows (#9371).
    let dense_prefix = length.min(capacity);
    Some(crate::gc::HeapSlotRange::new(
        array_elements_ptr(arr),
        dense_prefix,
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

/// [`note_array_slot`] for a live plain Array whose flag word was already
/// read by the caller's receiver guard. This preserves the exact store,
/// numeric-layout, element-shape, per-slot-layout, and barrier sequence
/// without redispatching through `array_numeric_layout` merely to recover the
/// same raw-f64 bits.
///
/// # Safety
///
/// `arr` must be a live, forwarding-resolved `GC_TYPE_ARRAY`; `index` must be
/// inside its allocation, and `flags` must be the current preceding
/// `GcHeader::_reserved` word with no intervening safepoint.
#[inline]
pub(crate) unsafe fn note_array_slot_resolved_flags(
    arr: *mut ArrayHeader,
    index: usize,
    value: f64,
    flags: u16,
) {
    let value = canonicalize_array_numeric_store_value_from_flags(flags, value);
    let mut value_bits = value.to_bits();
    let slot_ptr = array_elements_ptr(arr).add(index);
    let old_bits = std::ptr::read(slot_ptr);
    // GC_STORE_AUDIT(BARRIERED): resolved-flags helper notes layout and emits the array slot barrier below.
    std::ptr::write(slot_ptr, value_bits);
    value_bits = note_array_numeric_index_write(arr, index, value_bits);
    crate::gc::layout_note_slot_aware(arr as usize, index, value_bits, old_bits);
    let slot = slot_ptr as usize;
    crate::gc::runtime_write_barrier_slot(arr as usize, slot, value_bits);
}

/// Store and record one element on an array whose live head and header flags
/// the caller has already resolved.
///
/// The ordinary [`note_array_slot`] entry point must rediscover the numeric
/// layout through `clean_arr_ptr`. Hot array writers have already paid that
/// ownership/forwarding proof and have the same header word in hand for their
/// frozen/descriptor checks. Reusing it here avoids reclassifying the same
/// pointer while preserving the numeric-layout note, GC layout note, and write
/// barrier as one indivisible store protocol.
///
/// # Safety
///
/// `arr` must be the non-null result of `clean_arr_ptr_mut`, and `flags` must
/// have been read from that exact live head with no intervening Perry
/// allocation or safepoint.
#[inline]
pub(crate) unsafe fn store_array_slot_resolved(
    arr: *mut ArrayHeader,
    index: usize,
    value: f64,
    flags: u16,
) -> u64 {
    let value = canonicalize_array_numeric_store_value_from_flags(flags, value);
    let value_bits = value.to_bits();
    // GC_STORE_AUDIT(BARRIERED): the layout note and runtime_write_barrier_slot
    // below cover this resolved-head slot write.
    std::ptr::write(array_elements_ptr(arr).add(index), value_bits);
    note_array_numeric_index_write(arr, index, value_bits);
    crate::gc::layout_note_slot(arr as usize, index, value_bits);
    let slot = array_elements_ptr(arr).add(index) as usize;
    crate::gc::runtime_write_barrier_slot(arr as usize, slot, value_bits);
    value_bits
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
    // Generic `shift`, `unshift`, `splice`, `fill`, `copyWithin`, and `reverse`
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
    note_layout_classified_slots(length);
    let was_all_pointer = super::header::array_object_flags_resolved(arr)
        & (crate::gc::GC_LAYOUT_STATE_MASK | crate::gc::GC_LAYOUT_ALL_POINTERS)
        == (crate::gc::GC_LAYOUT_SIDE_MASK | crate::gc::GC_LAYOUT_ALL_POINTERS);
    if length == 0 && was_all_pointer {
        // The branch below re-arms the all-pointer claim, and
        // `layout_init_all_pointer_slots` already does everything the
        // zero-slot rebuild would have done first — clears the typed-intact
        // bit and forgets both per-object record kinds
        // (`layout_forget_object`) before setting the state — so the rebuild
        // was a second pass over the same registries for every
        // `pooled.length = 0`. Skip straight to the re-arm.
        crate::gc::layout_init_all_pointer_slots(arr as *mut u8);
        return;
    }
    if length != 0 {
        // Numeric canonicalization proves the complete payload pointer-free
        // and installs that GC layout itself. Do it before allocating and
        // building a pointer mask, and avoid replaying barriers for numbers.
        // This reads the actual slots; bulk stores may have invalidated the
        // array's previous representation or introduced a class-ref tag.
        super::header::refresh_array_numeric_layout_resolved(arr);
        if array_has_raw_f64_layout_flag(arr) {
            return;
        }
    }
    if length != 0
        && crate::gc::layout_all_pointer_slots_would_hold(array_elements_ptr(arr), length)
    {
        // A homogeneous pointer payload needs only the existing all-pointer
        // header claim, not an allocated bit mask with every bit set. The
        // predicate reads every actual slot, including after arbitrary bulk
        // stores; no previous layout claim is trusted here.
        crate::gc::layout_init_all_pointer_slots(arr as *mut u8);
    } else {
        crate::gc::layout_rebuild_from_slots(arr as *mut u8, array_elements_ptr(arr), length);
    }
    if length == 0 {
        // `layout_rebuild_from_slots` just left the head POINTER_FREE with its
        // per-object records dropped and the typed-intact bit cleared, which
        // is everything `refresh_array_numeric_layout` would redo for zero
        // slots via `rebuild_array_numeric_raw_f64` -> `layout_init_pointer_free`
        // (a second header resolution, a second forget probe). There are no
        // slots for the old-gen barrier replay either.
        //
        // An empty array holds BOTH vacuous claims, so keep the one its
        // history predicts. A pool bucket that held pointers and is emptied
        // for reuse (`pooled.length = 0`) will take pointers again: leaving
        // it `SIDE_MASK | ALL_POINTERS` — the state a declared `[]` literal
        // starts in — lets every push take the inline pointer-layout arm,
        // where resetting it to POINTER_FREE | RAW_F64 made the first push
        // of every reuse pay a layout transition (5k per frame on the ECS
        // command buffer). A non-pointer store into that state still demotes
        // it to UNKNOWN through `layout_note_slot`, exactly as it does for a
        // declared literal. Everything else keeps the raw-f64 claim.
        if was_all_pointer {
            crate::gc::layout_init_all_pointer_slots(arr as *mut u8);
        } else {
            super::header::set_array_raw_f64_layout_flag(arr);
        }
        return;
    }
    if crate::arena::pointer_in_old_gen(arr as usize) {
        // The range barrier keeps incremental shading and records old→young
        // edges page by page, hoisting the invariant parent checks.
        crate::gc::replay_old_parent_slot_range_barriers(
            arr as usize,
            array_elements_ptr(arr),
            length,
        );
    }
}

/// Settle GC metadata after a dense in-place element move without rebuilding
/// it from every live slot.
///
/// `moved_src..moved_src + moved_count` was copied verbatim to `moved_dst` and
/// `inserted_start..inserted_start + inserted_count` contains the newly-written
/// values. A pointer-free layout remains exact when the inserted values are
/// pointer-free, and the header-only all-pointer proof remains exact when they
/// are all pointers. An index-specific mixed mask no longer names the moved
/// slots, so it is dropped to UNKNOWN in O(1). Element-shape evidence is always
/// revoked: inserted values can change KIND even when the array length does
/// not change (#7480).
///
/// Survivor references are not new parent-child edges, but an old array's
/// dirty-page coverage follows their byte move. Translate that coverage
/// instead of replaying a write barrier for every survivor. This remains
/// page-only during incremental marking: the inserted-slot barriers below
/// perform all shading owed by the operation's genuinely new edges.
///
/// # Safety
///
/// `arr` is a live, forwarding-resolved ordinary Array. All ranges belong to
/// its inline allocation and no safepoint may occur between the move and this
/// call. The caller has already published the new logical length.
#[inline]
pub(crate) unsafe fn finish_array_dense_move_layout(
    arr: *mut ArrayHeader,
    moved_src: *const u64,
    moved_dst: *mut u64,
    moved_count: usize,
    inserted_start: *mut u64,
    inserted_count: usize,
) {
    if arr.is_null() {
        return;
    }

    super::element_shape::clear_element_shape(arr);
    let flags = super::header::array_object_flags_resolved(arr);
    let layout = flags & (crate::gc::GC_LAYOUT_STATE_MASK | crate::gc::GC_LAYOUT_ALL_POINTERS);
    let pointer_free = layout == crate::gc::GC_LAYOUT_POINTER_FREE;
    let all_pointer =
        layout == (crate::gc::GC_LAYOUT_SIDE_MASK | crate::gc::GC_LAYOUT_ALL_POINTERS);

    if moved_count != 0 && moved_src != moved_dst.cast_const() && !pointer_free {
        let copied_bytes = moved_count * std::mem::size_of::<u64>();
        if !crate::gc::relocate_moved_old_object_dirty_pages(
            arr as usize,
            moved_src as usize,
            moved_dst as usize,
            copied_bytes,
        ) {
            crate::gc::replay_old_parent_slot_range_barriers(arr as usize, moved_dst, moved_count);
        }
    }

    note_layout_classified_slots(inserted_count);
    let mut inserted_are_pointer_free = true;
    let mut inserted_are_all_pointer = true;
    let mut inserted_are_all_numeric = true;
    for index in 0..inserted_count {
        let slot = inserted_start.add(index);
        let bits = *slot;
        let pointer = crate::gc::layout_pointer_bearing_bits(bits);
        inserted_are_pointer_free &= !pointer;
        inserted_are_all_pointer &= pointer;
        inserted_are_all_numeric &= value_bits_to_number(bits).is_some();
        crate::gc::runtime_write_barrier_slot(arr as usize, slot as usize, bits);
    }

    if !inserted_are_all_numeric {
        clear_array_numeric_layout(arr);
    }
    if (pointer_free && inserted_are_pointer_free)
        || (all_pointer && inserted_are_all_pointer)
        || (moved_count == 0 && inserted_count == 0)
    {
        return;
    }
    if flags & crate::gc::GC_LAYOUT_STATE_MASK != 0 {
        crate::gc::layout_mark_unknown(arr.cast());
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

    // `length` may exceed the allocation for a large fresh holey array. Growth
    // copies and barriers only the allocated dense prefix; scanning logical
    // length here walked beyond intermediate growth allocations and corrupted
    // earlier values once the array became dense (#9371).
    let dense_prefix = ((*arr).length as usize).min((*arr).capacity as usize);
    if dense_prefix == 0 || dense_prefix > 16_000_000 {
        return;
    }

    let slots = array_elements_ptr(arr);
    if crate::gc::layout_visit_pointer_slots_for_user(arr as usize, dense_prefix, |index| {
        let slot = slots.add(index);
        crate::gc::runtime_write_barrier_slot(arr as usize, slot as usize, *slot);
    }) {
        return;
    }

    // One parent, one contiguous slot run — the loop form of the barrier, whose
    // per-store entry point would re-derive the parent classification `length`
    // times and re-assert a page-granular fact ~512 times per page. See
    // `gc::barrier::replay_old_parent_slot_range`.
    crate::gc::replay_old_parent_slot_range_barriers(arr as usize, slots, dense_prefix);
}

#[inline]
pub(crate) unsafe fn mark_array_layout_unknown(arr: *mut ArrayHeader) {
    clear_array_numeric_layout(arr);
    crate::gc::layout_mark_unknown(arr as *mut u8);
}

/// Minimum initial capacity for arrays to reduce reallocations
pub(crate) const MIN_ARRAY_CAPACITY: u32 = 16;
