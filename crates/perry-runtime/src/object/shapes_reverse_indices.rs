//! Reverse-index maintenance for the shape descriptor table.
//!
//! Descriptors are indexed both by exact semantic facts and by their keys
//! allocation. Stable-tombstone epochs deliberately leave exact-facts
//! interning while retaining the keys index, so all insertion, relocation,
//! and retirement bookkeeping lives together here.

use super::{
    invalidate_shape_lookup_cache, retire_cached_shape_object_kind, ShapeDescriptor, ShapeFacts,
    ShapeTableInner,
};

#[inline]
pub(super) fn descriptor_facts(descriptor: ShapeDescriptor) -> ShapeFacts {
    ShapeFacts {
        keys: descriptor.keys,
        logical_key_count: descriptor.logical_key_count,
        live_inline_slot_count: descriptor.live_inline_slot_count,
        semantic_generation: descriptor.semantic_generation,
        object_kind: descriptor.object_kind,
        hole_count: descriptor.hole_count,
    }
}

fn descriptor_facts_with_keys(descriptor: ShapeDescriptor, keys: u64) -> ShapeFacts {
    ShapeFacts {
        keys,
        logical_key_count: descriptor.logical_key_count,
        live_inline_slot_count: descriptor.live_inline_slot_count,
        semantic_generation: descriptor.semantic_generation,
        object_kind: descriptor.object_kind,
        hole_count: descriptor.hole_count,
    }
}

pub(super) fn remove_descriptor_id_from_facts_index(
    inner: &mut ShapeTableInner,
    facts: ShapeFacts,
    id: u32,
) {
    let remove_entry = if let Some(ids) = inner.ids_by_facts.get_mut(&facts) {
        if let Ok(index) = ids.binary_search(&id) {
            ids.remove(index);
        } else {
            ids.retain(|&candidate| candidate != id);
        }
        ids.is_empty()
    } else {
        false
    };
    if remove_entry {
        inner.ids_by_facts.remove(&facts);
    }
}

fn remove_descriptor_id_from_keys_index(inner: &mut ShapeTableInner, keys: u64, id: u32) {
    let remove_entry = if let Some(ids) = inner.ids_by_keys.get_mut(&keys) {
        if let Ok(index) = ids.binary_search(&id) {
            ids.remove(index);
        } else {
            ids.retain(|&candidate| candidate != id);
        }
        ids.is_empty()
    } else {
        false
    };
    if remove_entry {
        inner.ids_by_keys.remove(&keys);
    }
}

#[inline]
pub(super) fn insert_descriptor_id_sorted(ids: &mut Vec<u32>, id: u32) {
    if let Err(index) = ids.binary_search(&id) {
        ids.insert(index, id);
    }
}

/// Repair one descriptor after its collector-owned `keys` slot moved.
///
/// `indexed_keys` records the address under which the id is still indexed, so
/// this is O(population sharing the old/new facts) rather than O(all shapes).
pub(super) fn sync_descriptor_reverse_indices(inner: &mut ShapeTableInner, id: u32) {
    let Some(descriptor) = inner.descriptors.get(&id).map(|record| **record) else {
        return;
    };
    if descriptor.indexed_keys == descriptor.keys {
        return;
    }

    let old_facts = descriptor_facts_with_keys(descriptor, descriptor.indexed_keys);
    let new_facts = descriptor_facts(descriptor);
    if descriptor.facts_indexed {
        remove_descriptor_id_from_facts_index(inner, old_facts, id);
    }
    remove_descriptor_id_from_keys_index(inner, descriptor.indexed_keys, id);
    if descriptor.facts_indexed {
        insert_descriptor_id_sorted(inner.ids_by_facts.entry(new_facts).or_default(), id);
    }
    insert_descriptor_id_sorted(inner.ids_by_keys.entry(descriptor.keys).or_default(), id);
    if let Some(record) = inner.descriptors.get_mut(&id) {
        record.indexed_keys = descriptor.keys;
    }
}

pub(super) fn remove_descriptor_and_reverse_indices(inner: &mut ShapeTableInner, id: u32) {
    // The record's box is about to be dropped; any cached way naming it must
    // stop matching.
    invalidate_shape_lookup_cache();
    let Some(descriptor) = inner.descriptors.remove(&id) else {
        return;
    };
    retire_cached_shape_object_kind(id);
    let facts = descriptor_facts_with_keys(*descriptor, descriptor.indexed_keys);
    if descriptor.facts_indexed {
        remove_descriptor_id_from_facts_index(inner, facts, id);
    }
    remove_descriptor_id_from_keys_index(inner, descriptor.indexed_keys, id);
}
