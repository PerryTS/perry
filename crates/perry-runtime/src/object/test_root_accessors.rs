//! Test-only accessors for the object module's side-table roots, split out
//! of `mod.rs` to keep it under the 2000-line cap — the same reason as
//! `own_key_probe_tests`. `use super::*` reaches the shared items.
#![cfg(test)]

use super::*;

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
        .map(|(ptr, _)| *ptr as usize)
        .unwrap_or(0);
    (inline, overflow)
}

#[cfg(test)]
pub(crate) fn test_seed_transition_cache_root(next_keys: usize) {
    with_transition_cache(|t| unsafe {
        // GC_STORE_AUDIT(ROOT): test seed mirrors TRANSITION_CACHE_GLOBAL roots scanned by scan_transition_cache_roots_mut.
        let entry = &mut (*t)[0];
        entry.key_ptr = 0;
        crate::gc::runtime_store_root_usize_slot(&mut entry.next_keys, next_keys);
        entry.prev_shape_id = 0;
        entry.target_shape_id = 0;
        entry.slot_idx = 0;
        entry.target_len = 0;
    });
}

#[cfg(test)]
pub(crate) fn test_transition_cache_root() -> usize {
    with_transition_cache(|t| unsafe { (*t)[0].next_keys })
}

#[cfg(test)]
pub(crate) fn test_clear_transition_cache_root() {
    with_transition_cache(|t| unsafe {
        for i in 0..TRANSITION_CACHE_SIZE {
            // GC_STORE_AUDIT(ROOT): test clear writes non-pointer sentinels into scanned TRANSITION_CACHE_GLOBAL roots.
            (*t)[i] = TransitionEntry {
                key_ptr: 0,
                next_keys: 0,
                prev_shape_id: 0,
                target_shape_id: 0,
                slot_idx: 0,
                target_len: 0,
            };
        }
    });
}

#[cfg(test)]
pub(crate) fn test_seed_overflow_fields_root(owner: usize, value_bits: u64) {
    let st = crate::state::state();
    {
        let mut m = st.object_hot.overflow_fields.borrow_mut();
        m.clear();
        m.insert(owner, vec![value_bits]);
    }
    crate::gc::layout_note_slot(owner, 0, value_bits);
    st.object_hot.overflow_last.set((0, std::ptr::null_mut()));
}

#[cfg(test)]
pub(crate) fn debug_overflow_entry_len(owner: usize) -> Option<usize> {
    crate::state::state()
        .object_hot
        .overflow_fields
        .borrow()
        .get(&owner)
        .map(|v| v.len())
}

#[cfg(test)]
pub(crate) fn test_seed_overflow_fields_vec(owner: usize, values: Vec<u64>) {
    let st = crate::state::state();
    st.object_hot
        .overflow_fields
        .borrow_mut()
        .insert(owner, values);
    st.object_hot.overflow_last.set((0, std::ptr::null_mut()));
}

#[cfg(test)]
pub(crate) fn test_clear_overflow_fields_root() {
    let st = crate::state::state();
    st.object_hot.overflow_fields.borrow_mut().clear();
    st.object_hot.overflow_last.set((0, std::ptr::null_mut()));
}

#[cfg(test)]
pub(crate) fn test_overflow_fields_root() -> (usize, u64) {
    let m = crate::state::state().object_hot.overflow_fields.borrow();
    let Some((&owner, fields)) = m.iter().next() else {
        return (0, 0);
    };
    (owner, fields.first().copied().unwrap_or(0))
}

#[cfg(test)]
pub(crate) fn test_overflow_field_bits(owner: usize, index: usize) -> u64 {
    // Mode-aware probe: overflow values live in the spill buffer by default
    // and in the legacy side table under PERRY_OBJECT_SPILL=0.
    if object_spill_enabled()
        && index < SPILL_MAX_FIELD_INDEX
        && unsafe { spill_capable_owner(owner) }
    {
        return spill_get(owner, index).unwrap_or(0);
    }
    crate::state::state()
        .object_hot
        .overflow_fields
        .borrow()
        .get(&owner)
        .and_then(|fields| fields.get(index).copied())
        .unwrap_or(0)
}

#[cfg(test)]
pub(crate) fn test_object_spill_enabled() -> bool {
    object_spill_enabled()
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
