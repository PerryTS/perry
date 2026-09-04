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
    crate::state::state()
        .shapes
        .inner
        .borrow_mut()
        .indices
        .insert(
            keys_id,
            ShapeIndex {
                indexed_len: 0,
                slots: crate::fast_hash::new_ptr_hash_map(),
            },
        );
    let _ = shape_descriptor_ensure(keys_id as *const ArrayHeader, 0, 0)
        .expect("test shape id range unexpectedly exhausted");
}

#[cfg(test)]
pub(crate) fn test_shape_id_for_keys(keys_id: usize) -> Option<u32> {
    test_shape_ids_for_keys(keys_id).first().copied()
}
