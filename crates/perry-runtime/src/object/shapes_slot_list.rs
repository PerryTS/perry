//! `SlotList` — the shape key index's per-hash slot list, in a sibling file.
//!
//! Extracted from `shapes.rs` to keep it under the repo's 2000-line cap.
//! Also carries the two helpers that are mostly `SlotList` manipulation:
//! `record_shape_scan_outcome` (the shape scanner's per-descriptor
//! bookkeeping) and `shape_index_migrate_after_delete`.

/// Slots sharing one content hash.
///
/// Almost always exactly one: the key is an FNV-1a hash of distinct property
/// names, so a bucket with two entries is a genuine hash collision. Storing
/// that common case inline removes a heap allocation PER KEY from every index
/// build — and the index is rebuilt on every populated delete, so a 500-key
/// object was making ~500 `Vec` allocations per `delete`. Allocator and page
/// churn is the dominant cost on that benchmark (`clear_page_erms` 5.6%,
/// `mi_free` 4.2%, `RawVecInner::finish_grow` 2.9%), well above the lookup
/// work itself.
#[derive(Clone, Debug)]
pub(crate) enum SlotList {
    One(u32),
    Many(Vec<u32>),
}

impl SlotList {
    #[inline]
    pub(crate) fn push(&mut self, slot: u32) {
        match self {
            SlotList::One(existing) => {
                *self = SlotList::Many(vec![*existing, slot]);
            }
            SlotList::Many(v) => v.push(slot),
        }
    }

    /// Drop `removed` and shift every slot above it down by one.
    #[inline]
    pub(crate) fn retain_shift(&mut self, removed: u32) {
        let shift = |s: u32| -> Option<u32> {
            match s.cmp(&removed) {
                std::cmp::Ordering::Equal => None,
                std::cmp::Ordering::Less => Some(s),
                std::cmp::Ordering::Greater => Some(s - 1),
            }
        };
        match self {
            SlotList::One(slot) => match shift(*slot) {
                Some(s) => *slot = s,
                None => *self = SlotList::Many(Vec::new()),
            },
            SlotList::Many(v) => {
                v.retain_mut(|s| match shift(*s) {
                    Some(n) => {
                        *s = n;
                        true
                    }
                    None => false,
                });
                if v.len() == 1 {
                    *self = SlotList::One(v[0]);
                }
            }
        }
    }

    #[inline]
    pub(crate) fn is_empty(&self) -> bool {
        match self {
            SlotList::One(_) => false,
            SlotList::Many(v) => v.is_empty(),
        }
    }

    #[inline]
    pub(crate) fn iter(&self) -> impl Iterator<Item = &u32> {
        match self {
            SlotList::One(slot) => std::slice::from_ref(slot).iter(),
            SlotList::Many(v) => v.iter(),
        }
    }
}

use super::{shape_keys_address_is_recycled, ShapeDescriptor};

/// Per-descriptor bookkeeping after its keys address has been probed.
///
/// Lifted out of `scan_shape_table_rekey_mut`'s loop so the memoised path and
/// the probing path cannot drift apart — the probe is what is deduplicated,
/// never the bookkeeping, which still runs once per descriptor.
#[inline]
pub(crate) fn record_shape_scan_outcome(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    id: &u32,
    descriptor: &mut ShapeDescriptor,
    addr: usize,
    moved: bool,
    dead_descriptor_ids: &mut Vec<u32>,
    descriptor_rekeys: &mut Vec<u32>,
) {
    // Validate the POST-visit address. A stale shape key can follow the
    // forwarding record of the non-array tenant that recycled its address;
    // checking only an unmoved old address misses that case.
    if visitor.is_metadata_rewrite_phase() && shape_keys_address_is_recycled(addr) {
        dead_descriptor_ids.push(*id);
    } else if moved {
        descriptor.keys = addr as u64;
    }
    // A live-object edge can rewrite the boxed `keys` slot before this metadata
    // pass. Comparing against the address represented in the reverse maps
    // catches both that ordering and a move observed here.
    if descriptor.keys != descriptor.indexed_keys {
        descriptor_rekeys.push(*id);
    }
}

/// Shift a key index in place after an IN-PLACE delete.
///
/// Twin of [`shape_index_migrate_after_delete`] for an OWNED keys array (no
/// `GC_FLAG_SHAPE_SHARED`), which is compacted in place and therefore keeps
/// its address — and hence its `indices` key. Same shift, same safety net:
/// `shape_slot_lookup` re-validates the stored key against the requested
/// bytes, so a wrong index yields a miss, never a wrong property.
///
/// Returns whether the index is now current, so the caller can skip the
/// `shape_drop` that would otherwise discard it.
#[must_use]
pub(crate) fn shape_index_shift_in_place(
    keys_id: usize,
    removed_slot: u32,
    old_key_count: u32,
) -> bool {
    if keys_id == 0 {
        return false;
    }
    let mut inner = crate::state::state().shapes.inner.borrow_mut();
    let Some(index) = inner.indices.get_mut(&keys_id) else {
        return false;
    };
    if index.indexed_len < old_key_count {
        inner.indices.remove(&keys_id);
        return false;
    }
    index.slots.retain(|_, list| {
        list.retain_shift(removed_slot);
        !list.is_empty()
    });
    index.indexed_len = old_key_count - 1;
    true
}

/// Carry a key index across a delete, instead of re-hashing every key name.
///
/// `delete obj[k]` clones the keys array, so the result has a new address and
/// misses `indices` — which meant a 500-key object rebuilt its whole index on
/// every delete, decoding and FNV-hashing all ~500 property names each time.
/// The surviving keys are the same strings in the same order minus one, so the
/// index can be shifted rather than recomputed: drop the removed slot and
/// decrement every slot above it. No key bytes are touched.
///
/// Safe against a mistake by construction: [`shape_slot_lookup`] re-validates
/// the stored key against the requested bytes before returning a slot, so an
/// index that is wrong produces a MISS and the caller's own fallback, never a
/// wrong property. Only a fully-built index is carried over; a partially built
/// one is dropped and rebuilt as before.
///
/// Returns whether the index was actually carried over: the delete tail uses
/// that to skip the `shape_drop` that would otherwise discard it immediately.
#[must_use]
pub(crate) fn shape_index_migrate_after_delete(
    old_keys_id: usize,
    new_keys_id: usize,
    removed_slot: u32,
    old_key_count: u32,
) -> bool {
    if old_keys_id == 0 || new_keys_id == 0 || old_keys_id == new_keys_id {
        return false;
    }
    let mut inner = crate::state::state().shapes.inner.borrow_mut();
    let Some(mut index) = inner.indices.remove(&old_keys_id) else {
        return false;
    };
    if index.indexed_len < old_key_count {
        // Partially built: shifting it would leave the un-indexed tail
        // misaligned. Dropping it preserves the previous behaviour exactly.
        return false;
    }
    index.slots.retain(|_, list| {
        list.retain_shift(removed_slot);
        !list.is_empty()
    });
    index.indexed_len = old_key_count - 1;
    inner.indices.insert(new_keys_id, index);
    true
}

/// The receiver's current tombstone count, 0 when unshaped.
pub(crate) unsafe fn object_shape_hole_count(obj: *const crate::object::ObjectHeader) -> u32 {
    super::object_shape_descriptor(obj)
        .map(|d| d.hole_count)
        .unwrap_or(0)
}

pub(crate) unsafe fn publish_object_shape_holes(
    obj: *mut crate::object::ObjectHeader,
    hole_count: u32,
) -> u32 {
    if obj.is_null() || !super::shape_word_is_writable(obj) {
        return 0;
    }
    let Some(current) = super::object_shape_descriptor(obj) else {
        return 0;
    };
    let generation = super::SHAPE_SEMANTIC_NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if generation == 0 {
        super::shape_id_exhausted_abort();
    }
    let id = super::publish_shape_result(super::shape_descriptor_ensure_with_holes(
        current.keys as usize as *mut super::ArrayHeader,
        current.logical_key_count,
        current.live_inline_slot_count,
        generation,
        current.object_kind,
        hole_count,
    ));
    (*obj).parent_class_id = id;
    // Retire the predecessor. Its keys array is OWNED (the tombstone path is
    // gated on that), so this object is the only carrier of the old stamp and
    // the id becomes unreachable the moment the header word above is written:
    // stale IC tokens already miss on the stamp compare, and
    // `shape_descriptor_by_id` of a removed id is `None`. Without this, a
    // delete-churn loop minted one descriptor per delete against ONE stable
    // address forever — the reverse-index Vec under that address grew by one
    // per delete and every later publish walked it, which measured as a 26x
    // slowdown (2.06 s → 53.6 s) on `bench_populated_delete` before this
    // line existed.
    {
        let mut inner = crate::state::state().shapes.inner.borrow_mut();
        // Sweep EVERY other id for this keys address, not just the direct
        // predecessor: the delete-then-re-add cycle publishes an id on the
        // APPEND side too, and nothing else retires those — the post-trace
        // dead-key pruning only fires when the keys ARRAY dies, and this
        // array lives at a stable address for the object's whole life.
        // Retiring only the predecessor halved the descriptor pile-up
        // (53.6 s → 25.1 s on the churn benchmark) but ids still accumulated
        // one per iteration from the append publish.
        let stale: Vec<u32> = inner
            .ids_by_keys
            .get(&(current.keys))
            .map(|ids| ids.iter().copied().filter(|&other| other != id).collect())
            .unwrap_or_default();
        for other in stale {
            super::remove_descriptor_and_reverse_indices(&mut inner, other);
        }
    }
    super::debug_assert_object_shape_parity(obj);
    id
}

/// Install a process-global id into this agent's local descriptor table.
/// Module globals are initialized once per process, while workers own distinct
/// runtime state and moving keys pointers. Global id uniqueness makes a local
/// first installation unambiguous; an existing different descriptor fails
/// closed and the caller mints a fresh local id instead.
pub(super) fn install_external_shape_id(
    id: u32,
    keys: *const super::ArrayHeader,
    logical_key_count: u32,
    live_inline_slot_count: u32,
) -> bool {
    if !super::is_shape_id(id) || (keys.is_null() && logical_key_count != 0) {
        return false;
    }
    let descriptor = super::ShapeDescriptor {
        keys: keys as usize as u64,
        indexed_keys: keys as usize as u64,
        record: 0,
        old_carrier: false,
        old_carrier_seen: false,
        cache_carrier: false,
        logical_key_count,
        live_inline_slot_count,
        semantic_generation: 0,
        object_kind: super::ShapeObjectKind::Ordinary,
        hole_count: 0,
    };
    let facts = super::descriptor_facts(descriptor);
    let mut inner = crate::state::state().shapes.inner.borrow_mut();
    if let Some(existing) = inner.descriptors.get(&id) {
        return **existing == descriptor;
    }
    // A worker can have minted an equivalent local descriptor before module
    // initialization installs the process-global codegen id. Keep both id
    // descriptors valid for already-published objects and make the external
    // id canonical for subsequent births in this agent.
    //
    // This is the one insert that can REPLACE a live id with a fresh box, so
    // the lookup_ways cache has to be invalidated here (the fresh-id insert in
    // `intern_shape_descriptor` cannot, and deliberately does not).
    super::invalidate_shape_lookup_cache();
    inner
        .descriptors
        .insert(id, super::box_descriptor(descriptor));
    // An equivalent local descriptor can predate module initialization. Keep
    // both reverse-index entries and prefer the external id for subsequent
    // births in this agent; already-published local ids remain resolvable.
    inner.ids_by_facts.entry(facts).or_default().insert(0, id);
    super::insert_descriptor_id_sorted(inner.ids_by_keys.entry(descriptor.keys).or_default(), id);
    true
}
