//! Test-only shape-table helpers, in a sibling file.
//!
//! Extracted from `shapes.rs` to keep it under the repo's 2000-line cap. A
//! child module, so these keep reaching the parent's private items through
//! `super::`.

use super::*;
use crate::codegen_abi::{LayoutWord, ShapeId};

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

/// Suppress both shape-table young-log writer funnels. A test using this guard
/// must be rejected by the scanner's authoritative re-derivation.
#[cfg(test)]
pub(crate) struct TestShapeYoungLogSuppression(bool);

#[cfg(test)]
impl TestShapeYoungLogSuppression {
    pub(crate) fn new() -> Self {
        Self(SHAPE_YOUNG_LOG_SUPPRESSED.with(|cell| cell.replace(true)))
    }
}

#[cfg(test)]
impl Drop for TestShapeYoungLogSuppression {
    fn drop(&mut self) {
        SHAPE_YOUNG_LOG_SUPPRESSED.with(|cell| cell.set(self.0));
    }
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

/// Every descriptor id filed under the keys array at `keys_id`: its owned
/// family, or the families of the canonical lists on that backing (CKI S2).
#[cfg(test)]
fn test_family_ids_at(inner: &ShapeTableInner, keys_id: usize) -> Vec<LayoutWord> {
    let mut ids: Vec<LayoutWord> = inner
        .owned_families
        .get(&(keys_id as u64))
        .map(|ids| ids.as_slice().to_vec())
        .unwrap_or_default();
    if keys_id == 0 {
        ids.extend_from_slice(inner.keyless_family.as_slice());
    }
    let slab = crate::state::state().shapes.slab();
    crate::object::canonical_keys::for_each_list_shapes(|_, _, list| {
        ids.extend(list.family.as_slice().iter().copied().filter(|&id| {
            slab.get_handle(id)
                .is_some_and(|record| record.keys as usize == keys_id)
        }));
    });
    ids
}

/// The indexed length of the slot index of the keys array at `keys_id`,
/// owned or on its canonical backing's owner node.
#[cfg(test)]
fn test_index_len_at(inner: &ShapeTableInner, keys_id: usize) -> Option<u32> {
    if let Some(index) = inner.owned_indices.get(&keys_id) {
        return Some(index.indexed_len);
    }
    let mut found = None;
    crate::object::canonical_keys::for_each_list_shapes(|_, addr, list| {
        if addr == keys_id {
            if let Some(index) = &list.index {
                found = Some(index.indexed_len);
            }
        }
    });
    found
}

#[cfg(test)]
pub(crate) fn test_shape_entry_exists(keys_id: usize) -> bool {
    test_index_len_at(&crate::state::state().shapes.inner.borrow(), keys_id).is_some()
}

#[cfg(test)]
pub(crate) fn test_shape_descriptor_count() -> usize {
    // Count dynamic records; intrinsic function layouts are permanent seeds.
    crate::state::state().shapes.slab().len()
        - crate::codegen_abi::FUNCTION_SHAPE_COUNT as usize
        - 2
}

#[cfg(test)]
pub(crate) fn test_clear_shape_table() {
    let table = &crate::state::state().shapes;
    let mut inner = table.inner.borrow_mut();
    inner.owned_indices.clear();
    inner.by_facts.clear();
    inner.owned_families.clear();
    inner.keyless_family = IdList::default();
    crate::object::canonical_keys::for_each_list_shapes(|_, _, list| {
        *list = ListShapes::default();
    });
    inner.young_keys.clear();
    SHAPE_CARRIER_YOUNG_KEYS.with(|log| log.borrow_mut().clear());
    // SAFETY: test-only reset with no slab reference held.
    unsafe { table.slab_mut().clear() };
    drop(inner);
    table.seed_function_shapes();
    table.seed_native_birth_layouts();
}

#[cfg(test)]
pub(crate) fn test_drop_shape_descriptors(keys_id: usize) {
    let mut inner = crate::state::state().shapes.inner.borrow_mut();
    let stale: Vec<LayoutWord> = test_family_ids_at(&inner, keys_id);
    // Forced test retirement first releases cache owners, as production GC
    // does before deciding that a handle is uncarried.
    crate::object::with_transition_cache(|table| unsafe {
        for entry in (*table).iter_mut() {
            if stale.contains(&entry.prev_shape_id)
                || stale.contains(&entry.target_shape_id)
                || crate::object::spread::source_shape(entry).is_some_and(|w| stale.contains(&w))
            {
                *entry = std::mem::zeroed();
            }
        }
    });
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
    if let Some(ids) = inner.owned_families.remove(&(old as u64)) {
        for &id in ids.as_slice() {
            let Some(record) = table.slab().get_handle(id) else {
                continue;
            };
            if record.has(shapes_store::RECORD_FLAG_FACTS_INDEXED) {
                inner.facts_remove(record.facts_key_with_keys(old as u64), id);
                inner.facts_push_back(record.facts_key_with_keys(new as u64), id);
            }
            inner.family_push_back(record.keys_node(), new as u64, id);
        }
    }
}

/// The ids currently indexed under `keys_id`, in family order.
#[cfg(test)]
pub(crate) fn test_shape_ids_for_keys(keys_id: usize) -> Vec<LayoutWord> {
    test_family_ids_at(&crate::state::state().shapes.inner.borrow(), keys_id)
}

#[cfg(test)]
pub(crate) fn test_seed_shape_entry(keys_id: usize) {
    let mut inner = crate::state::state().shapes.inner.borrow_mut();
    inner.note_young_keys(keys_id as u64);
    inner.owned_indices.insert(
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
pub(crate) fn test_shape_id_for_keys(keys_id: usize) -> Option<LayoutWord> {
    test_shape_ids_for_keys(keys_id).first().copied()
}

/// Number of indexed slots recorded for `keys_id`, or 0 when the address has
/// no `indices` entry. Used by the `shapes.indices` arming tests.
#[cfg(test)]
pub(crate) fn test_shape_index_len(keys_id: usize) -> u32 {
    test_index_len_at(&crate::state::state().shapes.inner.borrow(), keys_id).unwrap_or(0)
}

/// A process-global shape id no descriptor in this agent has claimed, for the
/// `install_external_shape_id` path.
#[cfg(test)]
pub(crate) fn test_unused_external_shape_id() -> ShapeId {
    let table = &crate::state::state().shapes;
    // The top of the ORDINARY band: an external (typed) id is an ordinary
    // shape, which sites may hold; the dictionary band above it is not. Its
    // plain-kind granule, as a default-identity id is minted.
    let mut id = super::SITE_SHAPE_ID_END - 3 * (1 << super::SHAPE_ID_KIND_SHIFT) - 1;
    while table.slab().record_ptr(ShapeId(id)).is_some() {
        id -= 1;
    }
    ShapeId(id)
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
    id: ShapeId,
    keys: *const super::ArrayHeader,
    logical_key_count: u32,
    live_inline_slot_count: u32,
) -> bool {
    super::shapes_slot_list::install_external_shape_id(
        id,
        keys,
        logical_key_count,
        live_inline_slot_count,
        crate::object::shapes::PROTO_ID_DEFAULT,
        crate::object::shapes::ShapeObjectKind::Ordinary,
        crate::object::field_rep::REP_ANY,
    )
}

/// Does the record of `id` in this agent carry a carrier flag (never pruned
/// while no object carries it)? For the static-id seed tests.
pub(crate) fn test_shape_record_is_carrier(id: LayoutWord) -> bool {
    crate::state::state()
        .shapes
        .slab()
        .get_handle(id)
        .is_some_and(|record| record.cache_carrier())
}

/// Negative control for the census: remove only the memo accelerator while
/// retaining the live authoritative record and its keys family.
pub(crate) fn test_remove_facts_index(id: LayoutWord) {
    let table = &crate::state::state().shapes;
    let record = table.slab().get_handle(id).unwrap();
    let facts = record.facts_key_with_keys(record.keys);
    table.inner.borrow_mut().facts_remove(facts, id);
}
