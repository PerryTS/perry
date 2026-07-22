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
    /// #6759 Phase C3a/C3c: stable shape identity, allocated once at
    /// record birth and preserved by [`shape_keys_grown`] when an owned
    /// keys array reallocates. Stamped into a plain object's
    /// `parent_class_id` header word (dead weight for `class_id == 0`)
    /// and used as the FIELD_CACHE key, so lookups stop churning on
    /// capacity doublings and GC moves. 0 is never allocated ("no id").
    shape_id: u32,
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

/// #6759 C3c: ShapeIds live in their own u32 range, disjoint from every
/// real class id (user counter tops out far below; builtin reserved
/// ranges sit at `0x7FFF_FF00..=0x7FFF_FFFF` and `0xFFFF_0000..`), so a
/// stamp in a plain object's `parent_class_id` can never be mistaken for
/// inheritance data — and vice versa.
pub(crate) const SHAPE_ID_BASE: u32 = 0x8000_0000;
/// Exclusive end of the ShapeId range (2^30 ids ≈ one per shape BIRTH,
/// unreachable in practice).
pub(crate) const SHAPE_ID_END: u32 = 0xC000_0000;

/// #6759 C3c: PROCESS-GLOBAL allocator (supersedes the per-thread counter
/// C3a landed with). Global uniqueness matters because the worker
/// serializer replays `parent_class_id` verbatim: a deep-copied object's
/// stamp arriving on another thread must never alias an id that thread
/// allocated for a different shape. Monotonic — ids are NEVER reused, so
/// a stale stamp or cache entry can only miss, not falsely hit.
static SHAPE_ID_NEXT: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(SHAPE_ID_BASE);

#[inline]
pub(crate) fn is_shape_id(v: u32) -> bool {
    (SHAPE_ID_BASE..SHAPE_ID_END).contains(&v)
}

fn alloc_shape_id() -> u32 {
    use std::sync::atomic::Ordering;
    let id = SHAPE_ID_NEXT.fetch_add(1, Ordering::Relaxed);
    if id >= SHAPE_ID_END {
        // Range exhausted: park the counter (every subsequent call lands
        // here again, so it can never wrap back into the valid range) and
        // stop handing out ids — 0 disables the acceleration, never
        // correctness.
        SHAPE_ID_NEXT.store(SHAPE_ID_END, Ordering::Relaxed);
        return 0;
    }
    id
}

/// #6759 C3c: get-or-create the shape record for `keys` and return its
/// stable id (0 only if the id range is exhausted). Used by the resolve
/// paths to stamp a plain object's header after a successful lookup.
pub(crate) fn shape_id_for_keys_ensure(keys: *const ArrayHeader, key_count: u32) -> u32 {
    let keys_id = keys as usize;
    if keys_id == 0 {
        return 0;
    }
    let mut entries = crate::state::state().shapes.entries.borrow_mut();
    entries
        .entry(keys_id)
        .or_insert_with(|| Shape {
            indexed_len: 0,
            shape_id: alloc_shape_id(),
            slots: HashMap::with_capacity(key_count as usize),
        })
        .shape_id
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
                shape_id: alloc_shape_id(),
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
    crate::state::state().shapes.entries.borrow_mut().insert(
        keys_id,
        Shape {
            indexed_len: 0,
            shape_id: alloc_shape_id(),
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

#[cfg(test)]
mod c3c_tests {
    use super::*;

    fn key(name: &str) -> *mut crate::StringHeader {
        crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
    }

    /// #6759 C3c: ids come from the dedicated range (disjoint from real and
    /// builtin class ids), are stable per keys identity, and distinct
    /// across identities.
    #[test]
    fn shape_ids_are_range_disjoint_and_stable() {
        let _lock = crate::gc::global_side_table_test_lock();
        let a: usize = 0xC3C0_0000_0000_1000;
        let b: usize = 0xC3C0_0000_0000_2000;
        let ida = shape_id_for_keys_ensure(a as *const ArrayHeader, 4);
        let idb = shape_id_for_keys_ensure(b as *const ArrayHeader, 4);
        assert!(is_shape_id(ida) && is_shape_id(idb));
        assert_ne!(ida, idb);
        assert_eq!(shape_id_for_keys_ensure(a as *const ArrayHeader, 4), ida);
        // Real class-id space must never classify as a shape id.
        assert!(!is_shape_id(0));
        assert!(!is_shape_id(1));
        assert!(!is_shape_id(0x7FFF_FF30));
        assert!(!is_shape_id(0xFFFF_0005));
        shape_drop(a as *const ArrayHeader);
        shape_drop(b as *const ArrayHeader);
    }

    /// #6759 C3c stamp invariant on a REAL object through the real
    /// write/read paths: a read resolution stamps a shape id into the
    /// plain object's `parent_class_id`; after further appends the stamp
    /// is either cleared (keys pointer changed) or still equal to the
    /// current keys' id (in-place append / migrated grow).
    #[test]
    fn plain_object_stamp_lifecycle() {
        let _lock = crate::gc::global_side_table_test_lock();
        unsafe {
            let obj = crate::object::js_object_alloc(0, 8);
            for name in ["c3c_a", "c3c_b", "c3c_c"] {
                crate::object::js_object_set_field_by_name(obj, key(name), 1.0);
            }
            assert_eq!((*obj).class_id, 0, "test premise: plain object");
            let _ = crate::object::js_object_get_field_by_name(obj, key("c3c_b"));
            let stamp = (*obj).parent_class_id;
            assert!(
                is_shape_id(stamp),
                "read resolution must stamp a shape id, got {stamp:#x}"
            );

            crate::object::js_object_set_field_by_name(obj, key("c3c_d"), 2.0);
            crate::object::js_object_set_field_by_name(obj, key("c3c_e"), 3.0);
            let stamp2 = (*obj).parent_class_id;
            if stamp2 != 0 {
                assert!(is_shape_id(stamp2));
                let cur = shape_id_for_keys_ensure(
                    (*obj).keys_array,
                    crate::array::js_array_length((*obj).keys_array),
                );
                assert_eq!(
                    stamp2, cur,
                    "a surviving stamp must equal the CURRENT keys' id"
                );
            }

            // Reads still resolve correctly through the id-keyed cache.
            let v = crate::object::js_object_get_field_by_name(obj, key("c3c_d"));
            assert_eq!(f64::from_bits(v.bits()), 2.0);
        }
    }
}
