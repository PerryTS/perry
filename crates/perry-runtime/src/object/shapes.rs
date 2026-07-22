//! #6759 Phase C1: first-class Shape records, keyed on keys_array identity.
//!
//! A shared `keys_array` already IS a shape (same pointer ⟹ same ordered
//! key list, because mutation always forks a private clone). This module
//! promotes that identity into an explicit per-shape key→slot table,
//! replacing two per-consumer tables that re-derived the same map:
//!
//! * `KEYS_INDEX` — keyed per OBJECT, so 10k same-shape siblings built 10k
//!   private indexes;
//! * `WIDE_KEY_INDEX` — keys-keyed but capped at a 4-entry LRU, so any
//!   working set past 4 wide shapes thrashed.
//!
//! Trust model (inherited from both): entries are accelerators, never
//! authoritative. Every hit re-validates the stored key bytes at the
//! returned slot; a recycled keys_array address or an in-place mutation
//! fails validation, drops the entry, and the caller falls back to the
//! linear scan. Staleness is therefore harmless; the dead-owner prune
//! exists for memory, not correctness. See docs/shape-tree-plan.md.

use crate::array::ArrayHeader;
use std::cell::RefCell;
use std::collections::HashMap;

pub(crate) struct Shape {
    /// Key count covered by `slots`. Longer live array ⟹ catch up
    /// incrementally (append-only while shared); shorter ⟹ a delete
    /// compacted it — drop and rebuild on next lookup.
    indexed_len: u32,
    /// FNV-1a content hash of key bytes → candidate slots (collisions
    /// resolved by the per-hit content validation).
    slots: HashMap<u64, Vec<u32>>,
}

pub(crate) struct ShapeTable {
    entries: RefCell<crate::fast_hash::PtrHashMap<usize, Shape>>,
}

impl ShapeTable {
    pub(crate) fn new() -> Self {
        ShapeTable {
            entries: RefCell::new(crate::fast_hash::new_ptr_hash_map()),
        }
    }
}

/// Build (or extend) the slot map for `keys` covering `key_count` keys.
unsafe fn index_range(shape: &mut Shape, keys: *const ArrayHeader, key_count: u32) {
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let (slots, slot_len) = super::keys_array_dense_slots(keys);
    for i in shape.indexed_len..key_count.min(slot_len as u32) {
        let v = crate::JSValue::from_bits((*slots.add(i as usize)).to_bits());
        if let Some(b) = crate::string::js_string_key_bytes(v, &mut sso) {
            let h = super::key_bytes_hash(b.as_ptr(), b.len());
            shape.slots.entry(h).or_default().push(i);
        }
    }
    shape.indexed_len = key_count;
}

/// Look up `key_bytes` in the shape of `keys`. Returns a slot whose stored
/// key has been re-validated against `key_bytes`; `None` means "not found
/// via the shape" (caller falls back to its linear scan / append path).
///
/// `build` gates first-time index construction (callers keep their
/// historical thresholds: write path ≥ `KEYS_INDEX_THRESHOLD`, read path
/// ≥ `WIDE_KEY_INDEX_MIN_KEYS`) — but an entry that already exists is
/// consulted regardless, so a read may reuse the index a write built.
pub(crate) unsafe fn shape_slot_lookup(
    keys: *const ArrayHeader,
    key_bytes: &[u8],
    key_hash: u64,
    key_count: u32,
    build: bool,
) -> Option<u32> {
    let keys_id = keys as usize;
    let mut entries = crate::state::state().shapes.entries.borrow_mut();
    let shape = match entries.get_mut(&keys_id) {
        Some(s) => {
            if s.indexed_len > key_count {
                // Shrink (delete/compaction): slots are untrustworthy.
                entries.remove(&keys_id);
                return None;
            }
            s
        }
        None => {
            if !build {
                return None;
            }
            entries.entry(keys_id).or_insert(Shape {
                indexed_len: 0,
                slots: HashMap::with_capacity(key_count as usize),
            })
        }
    };
    if shape.indexed_len < key_count {
        index_range(shape, keys, key_count);
    }
    let candidates = shape.slots.get(&key_hash)?;
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let (slots, slot_len) = super::keys_array_dense_slots(keys);
    for &i in candidates {
        if (i as usize) >= slot_len || i >= key_count {
            continue;
        }
        let v = crate::JSValue::from_bits((*slots.add(i as usize)).to_bits());
        if let Some(stored) = crate::string::js_string_key_bytes(v, &mut sso) {
            if stored == key_bytes {
                return Some(i);
            }
        }
    }
    None
}

/// Record a freshly appended key: `keys` (the POST-append array — a clone
/// or grow-realloc lands under its new identity, or nowhere if no entry
/// exists yet) grew to `new_count` with `key_hash` at `slot`.
pub(crate) fn shape_note_append(
    keys: *const ArrayHeader,
    new_count: u32,
    key_hash: u64,
    slot: u32,
) {
    let mut entries = crate::state::state().shapes.entries.borrow_mut();
    if let Some(shape) = entries.get_mut(&(keys as usize)) {
        if shape.indexed_len + 1 == new_count {
            shape.indexed_len = new_count;
            shape.slots.entry(key_hash).or_default().push(slot);
        }
    }
}

/// Back-fill a linear-scan hit (no-op when the shape has no entry — the
/// next lookup builds it wholesale at the caller's threshold).
pub(crate) fn shape_note_hit(keys: *const ArrayHeader, key_hash: u64, slot: u32) {
    let mut entries = crate::state::state().shapes.entries.borrow_mut();
    if let Some(shape) = entries.get_mut(&(keys as usize)) {
        shape.slots.entry(key_hash).or_default().push(slot);
    }
}

/// Drop the shape for a keys_array that was compacted/retired in place
/// (delete path). Address-recycled arrays need no eager drop — validation
/// rejects them — but the delete path knows the map is stale NOW.
pub(crate) fn shape_drop(keys: *const ArrayHeader) {
    crate::state::state()
        .shapes
        .entries
        .borrow_mut()
        .remove(&(keys as usize));
}

/// Memory prune for the dead-owner fan-out: drop shapes whose keys_array
/// is dead. Correctness never depends on this (validation-on-hit).
pub(crate) fn prune_dead_shape_keys(is_dead_owner: &dyn Fn(usize) -> bool) {
    let mut entries = crate::state::state().shapes.entries.borrow_mut();
    if !entries.is_empty() {
        entries.retain(|keys_id, _| !is_dead_owner(*keys_id));
    }
}

#[cfg(test)]
pub(crate) fn test_shape_entry_exists(keys_id: usize) -> bool {
    crate::state::state()
        .shapes
        .entries
        .borrow()
        .get(&keys_id)
        .is_some()
}

#[cfg(test)]
pub(crate) fn test_seed_shape_entry(keys_id: usize) {
    crate::state::state().shapes.entries.borrow_mut().insert(
        keys_id,
        Shape {
            indexed_len: 0,
            slots: HashMap::new(),
        },
    );
}
