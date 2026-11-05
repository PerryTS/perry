//! Test-only accessors for the object module's side-table roots, split out
//! of `mod.rs` to keep it under the 2000-line cap — the same reason as
//! `own_key_probe_tests`. `use super::*` reaches the shared items.
#![cfg(test)]

use super::*;
use crate::codegen_abi::LayoutWord;

#[cfg(test)]
pub(crate) fn test_shape_cache_root(shape_id: u32) -> (usize, usize) {
    let st = crate::state::state();
    let slot = (shape_id as usize) & (SHAPE_INLINE_CACHE_SIZE - 1);
    let inline = unsafe { (*st.object_hot.shape_inline_cache.get())[slot].keys_array as usize };
    let overflow = st
        .object_hot
        .shape_cache_overflow
        .borrow()
        .get(&shape_id)
        .map(|(ptr, _, _)| *ptr as usize)
        .unwrap_or(0);
    (inline, overflow)
}

#[cfg(test)]
pub(crate) fn test_seed_transition_cache_root(next_keys: usize) {
    // Slot 0, no key and no length marker: the `len_marker == 0` arm of the
    // shared predicate with `kid == 0`, which classifies as not-relevant.
    // Arming through the shared helper is what keeps every writer of this
    // table on one rule (this seam used to carry a third variant of it).
    super::arm_transition_cache_young(0, next_keys, 0, 0);
    with_transition_cache(|t| unsafe {
        // GC_STORE_AUDIT(ROOT): test seed mirrors TRANSITION_CACHE_GLOBAL roots scanned by scan_transition_cache_roots_mut.
        let entry = &mut (*t)[0];
        entry.key_ptr = 0;
        crate::gc::runtime_store_root_usize_slot(&mut entry.next_keys, next_keys);
        entry.prev_shape_id = LayoutWord(0);
        entry.target_shape_id = LayoutWord(0);
        entry.slot_idx = 0;
        entry.target_len = 0;
    });
}

/// `test_seed_transition_cache_root` under a predecessor ShapeId that resolves,
/// so `prune_dead_transition_cache_entries` (which retires an entry whose
/// predecessor has no descriptor) keeps the entry across a collection.
#[cfg(test)]
pub(crate) fn test_seed_transition_cache_root_for_shape(
    prev_shape_id: LayoutWord,
    next_keys: usize,
) {
    test_seed_transition_cache_root(next_keys);
    with_transition_cache(|t| unsafe {
        (*t)[0].prev_shape_id = prev_shape_id;
    });
}

#[cfg(test)]
pub(crate) fn test_transition_cache_root() -> usize {
    with_transition_cache(|t| unsafe { (*t)[0].next_keys })
}

#[cfg(test)]
pub(crate) fn test_clear_transition_cache_root() {
    super::TRANSITION_CACHE_YOUNG.with(|log| log.borrow_mut().clear());
    with_transition_cache(|t| unsafe {
        for i in 0..TRANSITION_CACHE_SIZE {
            // GC_STORE_AUDIT(ROOT): test clear writes non-pointer sentinels into scanned TRANSITION_CACHE_GLOBAL roots.
            (*t)[i] = TransitionEntry {
                key_ptr: 0,
                next_keys: 0,
                prev_shape_id: LayoutWord(0),
                target_shape_id: LayoutWord(0),
                slot_idx: 0,
                target_len: 0,
            };
        }
    });
}
/// Test probe: address of the owner's spill buffer allocation (0 = none).
#[cfg(test)]
pub(crate) fn test_spill_buffer_addr(owner: usize) -> usize {
    unsafe {
        let obj = owner as *const ObjectHeader;
        if (*obj).meta.is_null() {
            return 0;
        }
        (*(*obj).meta).spill as usize
    }
}

/// Read object-owned spill storage in GC tests.
pub(crate) fn test_overflow_field_bits(owner: usize, index: usize) -> u64 {
    overflow_get(owner, index).unwrap_or(0)
}
