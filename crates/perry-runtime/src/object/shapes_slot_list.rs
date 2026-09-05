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

use super::shapes_store::{ShapeRecord, RECORD_FLAG_FACTS_INDEXED};

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
/// wrong property. Only a fully-built index is carried over. A partial owned
/// index is dropped with its dying source; a partial shared index remains in
/// place and the clone rebuilds as before.
///
/// A shared source remains live on sibling objects, so its index is cloned
/// before shifting. An owned source is about to die and its index can be moved.
/// This mirrors the keys-array ownership rule itself: forking a shared array is
/// a genuine shape transition, while replacing an owned array transfers its
/// identity.
///
/// Returns whether the index was actually carried over: the delete tail uses
/// that to skip the `shape_drop` that would otherwise discard it immediately.
#[must_use]
pub(crate) fn shape_index_migrate_after_delete(
    old_keys_id: usize,
    new_keys_id: usize,
    removed_slot: u32,
    old_key_count: u32,
    old_keys_shared: bool,
) -> bool {
    if old_keys_id == 0 || new_keys_id == 0 || old_keys_id == new_keys_id {
        return false;
    }
    let mut inner = crate::state::state().shapes.inner.borrow_mut();
    let source_index = if old_keys_shared {
        inner.indices.get(&old_keys_id).cloned()
    } else {
        inner.indices.remove(&old_keys_id)
    };
    let Some(mut index) = source_index else {
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
    inner.note_young_keys(new_keys_id as u64);
    inner.indices.insert(new_keys_id, index);
    true
}

/// The receiver's current tombstone count, 0 when unshaped.
pub(crate) unsafe fn object_shape_hole_count(obj: *const crate::object::ObjectHeader) -> u32 {
    super::object_shape_descriptor(obj)
        .map(|d| d.hole_count)
        .unwrap_or(0)
}

/// Retire growth-era descriptors before an owned keys array changes CONTENT
/// in place.
///
/// Same-address appends preserve every historical prefix, so their ShapeIds
/// remain valid while the array grows. Compaction is different: shifting a
/// middle key rewrites those prefixes. If the later publication merely drops
/// the logical count, exact-facts interning can otherwise rediscover the old
/// count-N id and let a stale IC read the pre-shift slot. The current id stays
/// live until the successor is minted; every private historical id is removed
/// from both reverse indices and the by-id table.
pub(crate) unsafe fn retire_owned_shape_history(
    obj: *const crate::object::ObjectHeader,
    keys: *const super::ArrayHeader,
) {
    if obj.is_null() || keys.is_null() {
        return;
    }
    let current = super::object_shape_stamp(obj);
    let keys_addr = keys as u64;
    let mut inner = crate::state::state().shapes.inner.borrow_mut();
    let stale: Vec<u32> = inner
        .families
        .get(&keys_addr)
        .map(|ids| {
            ids.as_slice()
                .iter()
                .copied()
                .filter(|&id| id != current)
                .collect()
        })
        .unwrap_or_default();
    for id in stale {
        super::remove_descriptor_and_reverse_indices(&mut inner, id);
    }
}

/// Update the private structural facts of a stable-tombstone receiver without
/// changing its ShapeId.
///
/// This is intentionally narrower than general shape publication: the keys
/// allocation must stay at the same address, so the descriptor's GC edge and
/// every surviving `(token, slot)` remain unchanged. Deletes change only the
/// hole count; a re-add appends at the private array's tail and may also widen
/// the inline live bound. A grow-reallocation declines and uses the ordinary
/// mint-then-stamp path.
///
/// A mutable private epoch must not participate in exact-facts interning.
/// Detach it on entry and leave it in the keys-address family, which keeps GC
/// relocation and squeeze-time retirement exact without re-indexing six
/// changing facts on every delete and re-add. The slab record address is
/// stable, so every later lookup observes the updated counts immediately.
pub(crate) unsafe fn try_update_stable_tombstone_shape(
    obj: *mut crate::object::ObjectHeader,
    keys: *mut super::ArrayHeader,
    logical_key_count: u32,
    live_inline_slot_count: u32,
    hole_count: u32,
) -> Option<u32> {
    if obj.is_null() || keys.is_null() || !super::shape_word_is_writable(obj) {
        return None;
    }
    let gc = crate::value::addr_class::try_read_gc_header(obj as usize)?;
    if gc.obj_type != crate::gc::GC_TYPE_OBJECT
        || gc._reserved & crate::gc::OBJ_FLAG_STABLE_TOMBSTONES == 0
    {
        return None;
    }
    let id = super::object_shape_stamp(obj);
    if !super::is_shape_id(id) {
        return None;
    }

    let table = &crate::state::state().shapes;
    let record = table.slab().record_ptr(id)?;
    // SAFETY: live slab record, single-threaded agent; read then written
    // through the same pointer with nothing else holding a reference.
    let current = unsafe { *record };
    // A stable id may never silently retarget its collector-owned keys edge.
    // Array growth that reallocates falls back to a fresh descriptor.
    if current.keys != keys as u64 || current.object_kind() != super::ShapeObjectKind::Ordinary {
        return None;
    }
    if current.logical_key_count == logical_key_count
        && current.live_inline_slot_count == live_inline_slot_count
        && current.hole_count == hole_count
    {
        return Some(id);
    }

    // Detach from exact-facts interning, so a mutable private epoch is never
    // handed to a second receiver. It stays in the family for GC relocation
    // and squeeze retirement. The accelerator was keyed with the address the
    // record is indexed under, which is `keys` (the caller proved the edge
    // did not move).
    if current.has(RECORD_FLAG_FACTS_INDEXED) {
        let mut inner = table.inner.borrow_mut();
        inner.facts_remove(current.facts_key_with_keys(keys as u64), id);
    }
    unsafe {
        (*record).logical_key_count = logical_key_count;
        (*record).live_inline_slot_count = live_inline_slot_count;
        (*record).hole_count = hole_count;
        (*record).set(RECORD_FLAG_FACTS_INDEXED, false);
    }
    super::debug_assert_object_shape_parity(obj);
    Some(id)
}

/// Update an already-detached stable-tombstone descriptor through the boxed
/// record address returned by `shape_descriptor_by_id`.
///
/// The first stable mutation must use `try_update_stable_tombstone_shape` to
/// detach exact-facts interning. Between those events the record address is
/// stable, its mutable epoch is deliberately invisible to interning, and no
/// table borrow is needed for a counter-only update.
pub(crate) unsafe fn try_update_stable_tombstone_shape_cached(
    obj: *mut crate::object::ObjectHeader,
    current: super::ShapeDescriptor,
    logical_key_count: u32,
    live_inline_slot_count: u32,
    hole_count: u32,
) -> Option<u32> {
    if obj.is_null() || current.record == 0 || !super::shape_word_is_writable(obj) {
        return None;
    }
    let gc = crate::value::addr_class::try_read_gc_header(obj as usize)?;
    if gc.obj_type != crate::gc::GC_TYPE_OBJECT
        || gc._reserved & crate::gc::OBJ_FLAG_STABLE_TOMBSTONES == 0
    {
        return None;
    }
    let id = super::object_shape_stamp(obj);
    if !super::is_shape_id(id) {
        return None;
    }

    // The caller's copy must still name the live record of THIS id: a
    // retired id resolves to nothing, and a record reused under another id
    // (never — ids are not recycled) would resolve to a different address.
    let live = crate::state::state().shapes.slab().record_ptr(id)?;
    if live as usize != current.record {
        return None;
    }
    let record = &mut *live;
    if record.keys != current.keys
        || record.has(RECORD_FLAG_FACTS_INDEXED)
        || record.object_kind() != super::ShapeObjectKind::Ordinary
    {
        return None;
    }
    record.logical_key_count = logical_key_count;
    record.live_inline_slot_count = live_inline_slot_count;
    record.hole_count = hole_count;
    super::debug_assert_object_shape_parity(obj);
    Some(id)
}

/// Retire the token of a detached private epoch while reusing its descriptor
/// record. This is the stable-tombstone squeeze counterpart to a full mint:
/// generated caches must observe a new id after slots are compacted, but no
/// exact-facts interning is needed for a record that cannot be shared by
/// another receiver.
pub(crate) unsafe fn rekey_stable_tombstone_shape_after_squeeze(
    obj: *mut crate::object::ObjectHeader,
    current: super::ShapeDescriptor,
    logical_key_count: u32,
    live_inline_slot_count: u32,
    hole_count: u32,
) -> Option<u32> {
    if obj.is_null() || current.record == 0 || !super::shape_word_is_writable(obj) {
        return None;
    }
    let gc = crate::value::addr_class::try_read_gc_header(obj as usize)?;
    if gc.obj_type != crate::gc::GC_TYPE_OBJECT
        || gc._reserved & crate::gc::OBJ_FLAG_STABLE_TOMBSTONES == 0
    {
        return None;
    }
    let old_id = super::object_shape_stamp(obj);
    if !super::is_shape_id(old_id) {
        return None;
    }
    let new_id = super::alloc_shape_id().ok()?;
    let generation = super::SHAPE_SEMANTIC_NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if generation == 0 {
        super::shape_id_exhausted_abort();
    }

    let table = &crate::state::state().shapes;
    let mut inner = table.inner.borrow_mut();
    let live_ptr = table.slab().record_ptr(old_id)?;
    if live_ptr as usize != current.record {
        return None;
    }
    // SAFETY: live slab record, read immediately.
    let live = unsafe { *live_ptr };
    if live.keys != current.keys
        || live.has(RECORD_FLAG_FACTS_INDEXED)
        || live.object_kind() != super::ShapeObjectKind::Ordinary
    {
        return None;
    }

    // Move the record to its new id in place of the old one. The family entry
    // is replaced where it stands; a family still keyed under a stale address
    // (a rewrite the metadata scan has not yet repaired) simply gains the new
    // id under the current one and sheds the old id on that scan.
    // SAFETY: no slab reference is held across these two calls.
    let mut record = unsafe { table.slab_mut().remove(old_id)? };
    record.logical_key_count = logical_key_count;
    record.live_inline_slot_count = live_inline_slot_count;
    record.semantic_generation = generation;
    record.hole_count = hole_count;
    super::retire_cached_shape_object_kind(old_id);
    unsafe { table.slab_mut().insert(new_id, record) };
    let replaced = inner
        .families
        .get_mut(&record.keys)
        .is_some_and(|ids| ids.replace(old_id, new_id));
    if !replaced {
        // `new_id` came from `alloc_shape_id` a few lines above and is in no
        // list yet (see `IdList::append_unchecked`).
        inner.family_append_fresh(record.keys, new_id);
    }
    inner.indices.remove(&(record.keys as usize));
    drop(inner);

    // #9200: the funnel re-arms the preserved record for a non-nursery
    // receiver. The record kept its flags across the id move, but a receiver
    // promoted since the last trace has no other arming opportunity before
    // the next minor.
    super::stamp_object_shape_id_with_carrier_note(obj, new_id);
    super::debug_assert_object_shape_parity(obj);
    Some(new_id)
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
    if let Some(id) = try_update_stable_tombstone_shape(
        obj,
        current.keys as usize as *mut super::ArrayHeader,
        current.logical_key_count,
        current.live_inline_slot_count,
        hole_count,
    ) {
        return id;
    }
    let generation = super::SHAPE_SEMANTIC_NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if generation == 0 {
        super::shape_id_exhausted_abort();
    }
    // The key count comes from the ARRAY, not the lineage: the O(1) hole
    // delete leaves the length untouched (array == lineage), but the squeeze
    // shrinks it in place before republishing — carrying the lineage count
    // there left a descriptor disagreeing with the authoritative keys edge,
    // which the very next parity assert caught (#9108: reserved_floor
    // at-scale SIGABRT took the whole suite down behind it).
    let keys_ptr = current.keys as usize as *mut super::ArrayHeader;
    let logical_key_count = crate::array::keys_array_len_capped_to_capacity(keys_ptr) as u32;
    let id = super::publish_shape_result(super::shape_descriptor_ensure_with_holes(
        keys_ptr,
        logical_key_count,
        current.live_inline_slot_count,
        generation,
        current.object_kind,
        hole_count,
    ));
    // #9200 THE FIX: stamp through the carrier-note funnel. This publish is
    // the one that minted a fresh (old_carrier=false) descriptor for an
    // already-promoted receiver and then RETIRED the armed predecessor in the
    // keys-address sweep below — leaving the receiver's nursery-young keys
    // array with no root a minor can see. The evacuating minor then swept the
    // keys array while live, `prune_dead_shape_keys` dropped this descriptor,
    // and the receiver came back shapeless (`Object.keys()` empty, fixed-slot
    // reads undefined — the #9200 gap fixture's exact wrong answer).
    super::stamp_object_shape_id_with_carrier_note(obj, id);
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
            .families
            .get(&(current.keys))
            .map(|ids| {
                ids.as_slice()
                    .iter()
                    .copied()
                    .filter(|&other| other != id)
                    .collect()
            })
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
    let keys = keys as usize as u64;
    let mut record = ShapeRecord::new(
        keys,
        logical_key_count,
        live_inline_slot_count,
        0,
        super::ShapeObjectKind::Ordinary,
        0,
    );
    record.set(super::shapes_store::RECORD_FLAG_EXTERNAL_CARRIER, true);
    let table = &crate::state::state().shapes;
    let mut inner = table.inner.borrow_mut();
    if let Some(existing) = table.slab().record_ptr(id) {
        // SAFETY: live slab record, single-threaded agent.
        let matches = unsafe { &*existing }.facts_match(
            keys,
            logical_key_count,
            live_inline_slot_count,
            0,
            super::ShapeObjectKind::Ordinary,
            0,
        );
        if matches {
            // SAFETY: same record and agent discipline as above.
            unsafe { (*existing).set(super::shapes_store::RECORD_FLAG_EXTERNAL_CARRIER, true) };
        }
        return matches;
    }
    // A worker can have minted an equivalent local descriptor before module
    // initialization installs the process-global codegen id. Keep both id
    // descriptors valid for already-published objects and make the external
    // id canonical for subsequent births in this agent: it goes to the FRONT
    // of its accelerator bucket, which is the order exact-facts interning
    // walks.
    // SAFETY: no slab reference is held; `slab().get` above returned a copy.
    unsafe { table.slab_mut().insert(id, record) };
    inner.facts_push_front(record.facts_key_with_keys(keys), id);
    inner.family_push_front(keys, id);
    true
}

/// The address of the ONE `keys` word the collector rewrites for `shape_id`,
/// or `None` when the id names no descriptor in this agent (#8112).
///
/// This is the seam that replaced the post-visit write-back callback. The
/// callback existed because the header word was the strong edge and the
/// descriptor a weak copy that had to be repaired from it, under exact-facts
/// validation, once per traced receiver whose keys array had moved. With the
/// descriptor holding the edge, the slot visitor writes the record directly
/// and there is nothing left to reconcile.
///
/// The returned address belongs to a slab record, so it is stable across
/// descriptor insertion; a record is only cleared by the table's own
/// retirement paths, and its chunk released at the end of a major
/// collection, after every enumeration of the cycle that produced it.
#[cfg(test)]
#[inline]
pub(crate) fn shape_descriptor_keys_slot(shape_id: u32) -> Option<*mut u64> {
    if !super::is_shape_id(shape_id) {
        return None;
    }
    crate::state::state()
        .shapes
        .slab()
        .record_ptr(shape_id)
        .map(|record| record as *mut u64)
}

/// Is `slot` the shared `keys` word of `shape_id`'s descriptor record?
///
/// #8112: that word is a TABLE root, not a slot any receiver owns. Every
/// sibling of the shape enumerates it, so a rewrite performed while tracing
/// one receiver silently changes the edge of every other — including old
/// receivers a minor never visits, for which no per-parent remembered-set page
/// could ever be armed. The remembered-set and old→young verification paths
/// therefore skip it and let the shape table's own root scanner cover it.
#[inline]
pub(crate) fn shape_id_owns_keys_slot(shape_id: u32, slot: *mut u64) -> bool {
    if !super::is_shape_id(shape_id) {
        return false;
    }
    // No table borrow at all: this runs inside collector walks, and a
    // `RefCell` borrow here would make the predicate itself a re-entrancy
    // hazard. The slab is read through a raw pointer.
    crate::state::state()
        .shapes
        .slab()
        .record_ptr(shape_id)
        .is_some_and(|record| record as *mut u64 == slot)
}

#[cfg(test)]
mod tests {
    use super::super::*;

    /// #9006: deleting from one object must copy the shared shape accelerator
    /// to its private keys-array clone, not move it away from untouched siblings.
    #[test]
    fn shared_delete_preserves_the_sibling_shape_index() {
        let _lock = crate::gc::global_side_table_test_lock();
        unsafe {
            const KEY_COUNT: usize = 40;
            let mut packed = Vec::new();
            for i in 0..KEY_COUNT {
                packed.extend_from_slice(format!("shared9006_{i:02}").as_bytes());
                packed.push(0);
            }
            let deleting = crate::object::js_object_alloc_with_shape(
                0x9006_0001,
                KEY_COUNT as u32,
                packed.as_ptr(),
                packed.len() as u32,
            );
            let sibling = crate::object::js_object_alloc_with_shape(
                0x9006_0001,
                KEY_COUNT as u32,
                packed.as_ptr(),
                packed.len() as u32,
            );
            let shared_keys = crate::object::object_keys_array(deleting);
            assert_eq!(shared_keys, crate::object::object_keys_array(sibling));
            let keys_gc = crate::value::addr_class::try_read_gc_header(shared_keys as usize)
                .expect("test premise: shared keys must be a live GC allocation");
            assert_ne!(
                keys_gc.gc_flags & crate::gc::GC_FLAG_SHAPE_SHARED,
                0,
                "test premise: the source keys array must be shared"
            );

            let survivor = b"shared9006_39";
            let survivor_hash = crate::object::key_bytes_hash(survivor.as_ptr(), survivor.len());
            assert_eq!(
                shape_slot_lookup(shared_keys, survivor, survivor_hash, KEY_COUNT as u32, true),
                Some(39),
                "test premise: build the shared source index"
            );

            let victim = b"shared9006_10";
            let victim_key =
                crate::string::js_string_from_bytes(victim.as_ptr(), victim.len() as u32);
            assert_eq!(
                crate::object::js_object_delete_field(deleting, victim_key),
                1
            );
            let private_keys = crate::object::object_keys_array(deleting);
            assert_ne!(private_keys, shared_keys);
            assert_eq!(crate::object::object_keys_array(sibling), shared_keys);

            assert_eq!(
                shape_slot_lookup(
                    shared_keys,
                    survivor,
                    survivor_hash,
                    KEY_COUNT as u32,
                    false,
                ),
                Some(39),
                "deleting a sibling stole the shared source index"
            );
            assert_eq!(
                shape_slot_lookup(
                    private_keys,
                    survivor,
                    survivor_hash,
                    (KEY_COUNT - 1) as u32,
                    false,
                ),
                Some(38),
                "the deleting object did not receive the shifted index"
            );
        }
    }
}
