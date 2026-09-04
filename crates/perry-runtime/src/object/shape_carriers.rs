//! ShapeId owners that may stamp an id after its last receiver dies (#9726).
//!
//! The descriptor table is weak with respect to receivers, but these runtime
//! caches are active metadata owners. Their bits are set on insertion and
//! rebuilt from exact table occupancy after every full trace. Generated module
//! globals are process-lifetime owners and use the record's separate external
//! carrier bit instead.

use super::*;

#[inline]
pub(crate) fn note_shape_id(shape_id: u32) {
    unsafe { shapes::note_cache_carrier(shapes::shape_descriptor_by_id(shape_id)) };
}

#[inline]
fn target_descriptor_resolves(entry: TransitionEntry, expected_len: u32) -> bool {
    shapes::shape_descriptor_by_id(entry.target_shape_id).is_some_and(|descriptor| {
        descriptor.keys == entry.next_keys as u64 && descriptor.logical_key_count == expected_len
    })
}

/// The runtime-only transition path revalidates weak, unstabilized entries
/// before publishing their target id. Generated probes reject these entries.
#[inline]
pub(crate) fn unstable_target_resolves(entry: TransitionEntry) -> bool {
    let expected_len = (entry.slot_idx & TRANSITION_SLOT_IDX_MASK).wrapping_add(1);
    target_descriptor_resolves(entry, expected_len)
}

/// A stabilized entry is a call-free generated-code publisher only while its
/// exact target array and descriptor facts still agree with the cached edge.
#[inline]
fn stable_target_resolves(entry: TransitionEntry) -> bool {
    let expected_len = (entry.slot_idx & TRANSITION_SLOT_IDX_MASK).wrapping_add(1);
    if entry.target_len != expected_len {
        return false;
    }
    let Some(header) =
        (unsafe { crate::value::addr_class::try_read_tracked_gc_header(entry.next_keys) })
    else {
        return false;
    };
    unsafe {
        if (*header.as_ptr()).obj_type != crate::gc::GC_TYPE_ARRAY {
            return false;
        }
        let keys = entry.next_keys as *const ArrayHeader;
        if (*keys).length != expected_len || (*keys).length > (*keys).capacity {
            return false;
        }
    }
    target_descriptor_resolves(entry, expected_len)
}

/// Rebuild transient cache ownership after a full trace. This runs before a
/// synchronous trace's uncarried-descriptor prune, so every surviving table
/// entry that can publish a ShapeId has already claimed it.
pub(crate) fn recompute_after_full_trace() {
    // Clears every transient bit, then re-notes both directions of the
    // Array-subclass cache. Permanent external owners use a different bit.
    array_tail_transition::recompute_cache_carriers_after_full_trace();

    with_transition_cache(|table| unsafe {
        for entry in (*table).iter() {
            if stable_target_resolves(*entry) {
                note_shape_id(entry.target_shape_id);
            }
        }
    });

    let state = crate::state::state();
    unsafe {
        for entry in (&*state.object_hot.shape_inline_cache.get()).iter() {
            note_shape_id(entry.runtime_shape_id);
        }
    }
    for &(keys, runtime_shape_id) in state.object_hot.shape_cache_overflow.borrow().values() {
        if !keys.is_null() {
            note_shape_id(runtime_shape_id);
        }
    }
}
