//! Test-only shape-table helpers, in a sibling file.
//!
//! Extracted from `shapes.rs` to keep it under the repo's 2000-line cap. A
//! child module, so these keep reaching the parent's private items through
//! `super::`.

use super::*;

#[cfg(test)]
#[inline]
pub(crate) fn test_keys_edge_suppressed() -> bool {
    KEYS_EDGE_SUPPRESSED.with(std::cell::Cell::get)
}

/// RAII guard so a panicking fixture cannot leave a suppression on for the
/// next test on this thread.
#[cfg(test)]
pub(crate) struct TestKeysEdgeSuppression {
    edge: bool,
}

#[cfg(test)]
impl TestKeysEdgeSuppression {
    /// Drop the only edge. Nothing roots or rewrites the keys array.
    pub(crate) fn without_descriptor_edge() -> Self {
        Self {
            edge: KEYS_EDGE_SUPPRESSED.with(|c| c.replace(true)),
        }
    }
}

#[cfg(test)]
impl Drop for TestKeysEdgeSuppression {
    fn drop(&mut self) {
        KEYS_EDGE_SUPPRESSED.with(|c| c.set(self.edge));
    }
}

/// Test-only sabotage of the recycled-address type check. Keeping this scoped
/// and unshipped lets the regression fixture prove its detector would fail if
/// both prune and metadata rewrite trusted the replacement tenant.
#[cfg(test)]
pub(crate) struct TestRecycledKeysCheckSuppression {
    previous: bool,
}

#[cfg(test)]
impl TestRecycledKeysCheckSuppression {
    pub(crate) fn new() -> Self {
        Self {
            previous: RECYCLED_KEYS_CHECK_SUPPRESSED.with(|cell| cell.replace(true)),
        }
    }
}

#[cfg(test)]
impl Drop for TestRecycledKeysCheckSuppression {
    fn drop(&mut self) {
        RECYCLED_KEYS_CHECK_SUPPRESSED.with(|cell| cell.set(self.previous));
    }
}

#[cfg(test)]
pub(crate) fn test_shape_entry_exists(keys_id: usize) -> bool {
    crate::state::state()
        .shapes
        .inner
        .borrow()
        .indices
        .get(&keys_id)
        .is_some()
}

#[cfg(test)]
pub(crate) fn test_shape_descriptor_count() -> usize {
    crate::state::state().shapes.slab().len()
}

#[cfg(test)]
pub(crate) fn test_clear_shape_table() {
    let table = &crate::state::state().shapes;
    let mut inner = table.inner.borrow_mut();
    inner.indices.clear();
    inner.by_facts.clear();
    inner.families.clear();
    inner.young_keys.clear();
    // SAFETY: test-only reset with no slab reference held.
    unsafe { table.slab_mut().clear() };
    drop(inner);
    clear_shape_object_kind_cache();
}

#[cfg(test)]
pub(crate) fn test_drop_shape_descriptors(keys_id: usize) {
    let mut inner = crate::state::state().shapes.inner.borrow_mut();
    let stale: Vec<u32> = inner
        .families
        .get(&(keys_id as u64))
        .map(|ids| ids.as_slice().to_vec())
        .unwrap_or_default();
    for id in stale {
        remove_descriptor_and_reverse_indices(&mut inner, id);
    }
}

/// Move the family indexed under `old` to `new`, exactly as the metadata
/// scan does after the collector forwarded that keys array.
#[cfg(test)]
pub(crate) fn test_rekey_shape_family(old: usize, new: usize) {
    let table = &crate::state::state().shapes;
    let mut inner = table.inner.borrow_mut();
    if let Some(ids) = inner.families.remove(&(old as u64)) {
        for &id in ids.as_slice() {
            let Some(record) = table.slab().get(id) else {
                continue;
            };
            if record.has(shapes_store::RECORD_FLAG_FACTS_INDEXED) {
                inner.facts_remove(record.facts_key_with_keys(old as u64), id);
                inner.facts_push_back(record.facts_key_with_keys(new as u64), id);
            }
            inner.family_push_back(new as u64, id);
        }
    }
}

/// The ids currently indexed under `keys_id`, in family order.
#[cfg(test)]
pub(crate) fn test_shape_ids_for_keys(keys_id: usize) -> Vec<u32> {
    crate::state::state()
        .shapes
        .inner
        .borrow()
        .families
        .get(&(keys_id as u64))
        .map(|ids| ids.as_slice().to_vec())
        .unwrap_or_default()
}

#[cfg(test)]
pub(crate) fn test_seed_shape_entry(keys_id: usize) {
    let mut inner = crate::state::state().shapes.inner.borrow_mut();
    inner.note_young_keys(keys_id as u64);
    inner.indices.insert(
        keys_id,
        ShapeIndex {
            indexed_len: 0,
            slots: SlotIndex::new(),
        },
    );
    drop(inner);
    let _ = shape_descriptor_ensure(keys_id as *const ArrayHeader, 0, 0)
        .expect("test shape id range unexpectedly exhausted");
}

#[cfg(test)]
pub(crate) fn test_shape_id_for_keys(keys_id: usize) -> Option<u32> {
    test_shape_ids_for_keys(keys_id).first().copied()
}

/// Number of indexed slots recorded for `keys_id`, or 0 when the address has
/// no `indices` entry. Used by the `shapes.indices` arming tests.
#[cfg(test)]
pub(crate) fn test_shape_index_len(keys_id: usize) -> u32 {
    let inner = crate::state::state().shapes.inner.borrow();
    inner
        .indices
        .get(&keys_id)
        .map(|ix| ix.indexed_len)
        .unwrap_or(0)
}

/// A process-global shape id no descriptor in this agent has claimed, for the
/// `install_external_shape_id` path.
#[cfg(test)]
pub(crate) fn test_unused_external_shape_id() -> u32 {
    let table = &crate::state::state().shapes;
    let mut id = super::SHAPE_ID_END - 1;
    while table.slab().record_ptr(id).is_some() {
        id -= 1;
    }
    id
}

/// Build the slot index for `keys` through the PRODUCTION path
/// (`shape_slot_lookup` with `build = true`), which is the `indices` arm site.
/// Nothing but a call, so it cannot drift from the writer it stands in for.
///
/// # Safety
/// `keys` must be a live keys array of `key_count` dense string slots.
#[cfg(test)]
pub(crate) unsafe fn test_build_slot_index(
    keys: *const super::ArrayHeader,
    probe: &[u8],
    key_count: u32,
) {
    let h = crate::object::keys_lookup::key_bytes_hash(probe.as_ptr(), probe.len());
    let _ = super::shape_slot_lookup(keys, probe, h, key_count, true);
}

/// `shapes_slot_list::shape_index_migrate_after_delete`, reachable from the
/// `gc::tests` suites (the module is private to `shapes`).
#[cfg(test)]
pub(crate) fn test_shape_index_migrate_after_delete(
    old_keys_id: usize,
    new_keys_id: usize,
    removed_slot: u32,
    old_key_count: u32,
    old_keys_shared: bool,
) -> bool {
    super::shapes_slot_list::shape_index_migrate_after_delete(
        old_keys_id,
        new_keys_id,
        removed_slot,
        old_key_count,
        old_keys_shared,
    )
}

/// `shapes_slot_list::install_external_shape_id`, same reason.
#[cfg(test)]
pub(crate) fn test_install_external_shape_id(
    id: u32,
    keys: *const super::ArrayHeader,
    logical_key_count: u32,
    live_inline_slot_count: u32,
) -> bool {
    super::shapes_slot_list::install_external_shape_id(
        id,
        keys,
        logical_key_count,
        live_inline_slot_count,
    )
}
