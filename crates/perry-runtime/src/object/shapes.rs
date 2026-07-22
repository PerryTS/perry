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
    /// #6759 Phase C3a: stable shape identity, allocated once at record
    /// birth and preserved by [`shape_keys_grown`] when an owned keys
    /// array reallocates — the identity consumers re-key on in C3b
    /// (FIELD_CACHE, typed_feedback exactness) so they stop churning on
    /// capacity doublings and GC moves. 0 is never allocated ("no id").
    #[allow(dead_code)]
    shape_id: u32,
    /// FNV-1a content hash of key bytes → candidate slots (collisions
    /// resolved by the per-hit content validation).
    slots: HashMap<u64, Vec<u32>>,
}

pub(crate) struct ShapeTable {
    entries: RefCell<crate::fast_hash::PtrHashMap<usize, Shape>>,
    /// #6759 Phase C3a: monotonic ShapeId allocator (1-based; 0 = none).
    /// u32 wrap is theoretical (one id per shape BIRTH, not per object);
    /// on wrap the allocator skips 0 and collision risk is bounded by the
    /// validation-on-hit trust model like every other accelerator here.
    next_id: std::cell::Cell<u32>,
}

impl ShapeTable {
    pub(crate) fn new() -> Self {
        ShapeTable {
            entries: RefCell::new(crate::fast_hash::new_ptr_hash_map()),
            next_id: std::cell::Cell::new(1),
        }
    }

    fn alloc_shape_id(&self) -> u32 {
        let id = self.next_id.get();
        self.next_id.set(id.wrapping_add(1).max(1));
        id
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
    let table = &crate::state::state().shapes;
    let mut entries = table.entries.borrow_mut();
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
            let shape_id = table.alloc_shape_id();
            entries.entry(keys_id).or_insert(Shape {
                indexed_len: 0,
                shape_id,
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

/// #6759 Phase C3a: an OWNED (non-`GC_FLAG_SHAPE_SHARED`) keys array was
/// reallocated by `js_array_push` — the SAME logical shape now lives at a
/// new address. Migrate the record (slot map, indexed_len, shape_id) so it
/// survives the capacity doubling; pre-C3a the record was orphaned at the
/// old address and the next lookup rebuilt it O(key_count), making every
/// doubling of a wide object's build pay a full re-index.
///
/// Callers must pass the OWNED-grow pair only: a shared array's fork is a
/// genuine transition (the clone starts a NEW identity and the old address
/// still describes the siblings' live shape — migrating it would corrupt
/// them). Safety net: a wrong or stale migration cannot produce wrong
/// results — every hit re-validates key bytes against the live array —
/// it only wastes the rebuild this exists to save.
pub(crate) fn shape_keys_grown(old_keys: usize, new_keys: *const ArrayHeader) {
    let new_id = new_keys as usize;
    if old_keys == 0 || new_id == 0 || old_keys == new_id {
        return;
    }
    let mut entries = crate::state::state().shapes.entries.borrow_mut();
    if let Some(shape) = entries.remove(&old_keys) {
        entries.insert(new_id, shape);
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

/// #6759 Phase C3a: rekey shape records when GC evacuation MOVES their
/// keys array, so a wide object's slot map (and its stable `shape_id`)
/// survives a copied minor instead of being orphaned at the from-space
/// address and rebuilt O(key_count) on the next lookup. Metadata-rewrite
/// only — the records hold no heap references (slot indexes + an address
/// used as identity), so outside that phase this scanner is a no-op and
/// marks nothing. Same pattern as the descriptor-table owner rekey.
pub(crate) fn scan_shape_table_rekey_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    if !visitor.is_metadata_rewrite_phase() {
        return;
    }
    let mut entries = crate::state::state().shapes.entries.borrow_mut();
    if entries.is_empty() {
        return;
    }
    let moved: Vec<(usize, usize)> = entries
        .keys()
        .filter_map(|&keys_id| {
            let mut addr = keys_id;
            visitor.visit_metadata_usize_slot(&mut addr);
            (addr != keys_id).then_some((keys_id, addr))
        })
        .collect();
    for (old, new) in moved {
        if let Some(shape) = entries.remove(&old) {
            entries.insert(new, shape);
        }
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
    let table = &crate::state::state().shapes;
    let shape_id = table.alloc_shape_id();
    table.entries.borrow_mut().insert(
        keys_id,
        Shape {
            indexed_len: 0,
            shape_id,
            slots: HashMap::new(),
        },
    );
}

#[cfg(test)]
pub(crate) fn test_shape_id_for_keys(keys_id: usize) -> Option<u32> {
    crate::state::state()
        .shapes
        .entries
        .borrow()
        .get(&keys_id)
        .map(|s| s.shape_id)
}
