//! ShapeId owners that may stamp an id after its last receiver dies (#9726).
//!
//! The descriptor table is weak with respect to receivers, but these runtime
//! caches are active metadata owners. Their bits are set on insertion and
//! rebuilt from exact table occupancy after every full trace. Generated module
//! globals are process-lifetime owners and use the record's separate external
//! carrier bit instead.

use super::*;

use crate::codegen_abi::LayoutWord;
#[inline]
pub(crate) fn note_shape_id(shape_id: LayoutWord) {
    // The record alone: no descriptor is materialized to set its carrier bit.
    if let Some(record) = shapes::shape_record_for_word(shape_id) {
        unsafe { shapes::note_cache_carrier_record(record) };
    }
}

#[inline]
fn target_descriptor_resolves(entry: TransitionEntry, expected_len: u32) -> bool {
    shapes::shape_descriptor_for_word(entry.target_shape_id).is_some_and(|descriptor| {
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

/// A transition entry owns its target exactly when generated code or the
/// spread path may publish it without revalidation: a stabilized entry
/// (`target_len != 0`) or a spread entry. An unstable entry's target is
/// revalidated by [`unstable_target_resolves`] before use, so it stays weak.
#[inline]
fn entry_owns_target(entry: &TransitionEntry) -> bool {
    entry.target_len != 0 || spread::source_shape(entry).is_some()
}

/// Rebuild transient cache ownership after a full trace. This runs before a
/// synchronous trace's uncarried-descriptor prune, so every surviving table
/// entry that can publish a ShapeId has already claimed it.
pub(crate) fn recompute_after_full_trace() {
    // Clears every transient bit, then re-notes both directions of the
    // Array-subclass cache. Permanent external owners use a different bit.
    array_tail_transition::recompute_cache_carriers_after_full_trace();
    crate::json_hooks::note_carriers();
    super::arguments::note_arguments_shape_carriers();
    super::alloc_basic::note_keyless_birth_carrier();
    crate::stream_hooks::note_birth_carriers();

    with_used_transition_cache(|table| unsafe {
        for entry in (*table).iter() {
            if entry.next_keys != 0 && entry_owns_target(entry) {
                // The same set as `transition_cache_insert` / `spread::learn`:
                // the target only; predecessor and spread source are weak keys.
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
    for &(keys, runtime_shape_id, _) in state.object_hot.shape_cache_overflow.borrow().values() {
        if !keys.is_null() {
            note_shape_id(LayoutWord(runtime_shape_id));
        }
    }
    shapes::recompute_record_handle_carriers();
}
