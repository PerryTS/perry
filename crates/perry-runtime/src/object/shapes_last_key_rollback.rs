//! The ROLLBACK edge of the shape transition graph: deleting the key a
//! receiver added LAST returns it to the shape of the list without that key.
//!
//! This is V8's "delete the last-added property" (it moves the object back to
//! the map it had before the property was added). A receiver whose shape
//! lists `count` keys on a SHARED canonical backing (`GC_FLAG_SHAPE_SHARED`,
//! a growth chain whose lists are prefixes of one array, none of which ever
//! changes) and that deletes key `count - 1` gets the shape of the prefix of
//! `count - 1` keys of that same backing: the same prototype, generation,
//! kind, live inline bound and brands, with the vacated lane's
//! representation and ConstFn fact dropped. Nothing is copied or tombstoned,
//! the backing stays shared, and the next add of that key is the ordinary
//! key-add edge back. A delete/re-add churn (`delete events[type]` then
//! `events[type] = listener`) so alternates between two shapes the
//! transition caches already hold, instead of forking a private list,
//! tombstoning it and squeezing it.
//!
//! The successor is found by exact-facts interning, so a receiver reaches the
//! same ShapeId as any other path to the same facts, and it is never the
//! predecessor (its key count differs): a `(shape, key)` cache primed for the
//! deleted key cannot hit afterwards.

use super::*;

/// Move `obj` to the shape of its list without its last key, when the delete
/// of key `count - 1` admits the rollback (see the module docs). Returns the
/// successor ShapeId, or 0, having changed nothing, when it does not: the
/// caller then takes its ordinary delete.
///
/// The caller has already checked that the key may be deleted (it is
/// configurable) and clears the vacated slot itself after a success.
///
/// # Safety
/// `obj` is a live `ObjectHeader`; nothing allocates between the caller's key
/// lookup and this call.
pub(crate) unsafe fn publish_object_shape_last_key_rollback(
    obj: *mut crate::object::ObjectHeader,
    count: u32,
) -> u32 {
    if obj.is_null() || count == 0 || !shape_word_is_writable(obj) {
        return 0;
    }
    let Some(header) = crate::value::addr_class::try_read_gc_header(obj as usize) else {
        return 0;
    };
    // A tombstoning receiver owns its list; a descriptor-carrying, frozen or
    // sealed one is the full path's to judge.
    if header.obj_type != crate::gc::GC_TYPE_OBJECT
        || header._reserved
            & (crate::gc::OBJ_FLAG_STABLE_TOMBSTONES
                | crate::gc::OBJ_FLAG_HAS_DESCRIPTORS
                | crate::gc::OBJ_FLAG_FROZEN
                | crate::gc::OBJ_FLAG_SEALED
                | crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO)
            != 0
        || crate::object::dictionary::is_dictionary(obj)
    {
        return 0;
    }
    let from = object_shape_stamp(obj);
    if let Some(to) = cached_rollback_edge(from, count) {
        crate::array::clear_array_subclass_named_prefix_token(obj);
        stamp_object_shape_id_with_carrier_note(obj, to);
        return to;
    }
    let Some(current) = object_shape_descriptor(obj) else {
        return 0;
    };
    if !current.object_kind.is_ordinary_layout()
        || current.hole_count != 0
        || current.logical_key_count != count
        || current.summary != 0
    {
        return 0;
    }
    let keys = current.keys as usize as *mut ArrayHeader;
    if keys.is_null() || !crate::object::key_attrs::keys_attrs(keys).is_null() {
        return 0;
    }
    match crate::value::addr_class::try_read_gc_header(keys as usize) {
        Some(gc)
            if gc.obj_type == crate::gc::GC_TYPE_ARRAY
                && gc.gc_flags & crate::gc::GC_FLAG_SHAPE_SHARED != 0
                && gc.gc_flags & crate::gc::GC_FLAG_FORWARDED == 0 => {}
        _ => return 0,
    }
    let slot = count - 1;
    let rep = current.rep & crate::object::field_rep::lanes_below(slot);
    // At most one ConstFn fact per inline representation lane.
    let mut infos = [shapes_store::ConstFnSlotInfo { slot: 0, info: 0 };
        crate::object::field_rep::REP_SLOTS as usize];
    let mut info_count = 0;
    for info in current.constfn_infos() {
        if u32::from(info.slot) < slot && info_count < infos.len() {
            infos[info_count] = *info;
            info_count += 1;
        }
    }
    let brands = BrandList::copy_of(current.brands());
    let summary =
        receiver_extra_summary(obj) | crate::object::key_attrs::keys_summary_checked(keys, slot);
    // The rollback is structural: the Array-subclass named-prefix proof goes,
    // as with every other transition publisher.
    crate::array::clear_array_subclass_named_prefix_token(obj);
    let Ok(id) = shape_descriptor_intern_with_special(
        keys,
        slot,
        current.live_inline_slot_count,
        current.semantic_generation,
        store_kind::mint_kind(current.object_kind, obj),
        0,
        current.proto_id,
        summary,
        rep,
        &infos[..info_count],
        brands.as_slice(),
        None,
    ) else {
        return 0;
    };
    if id == from {
        return 0;
    }
    note_rollback_edge(from, id);
    stamp_object_shape_id_with_carrier_note(obj, id);
    debug_assert_object_shape_parity(obj);
    id
}

/// Learned rollback edges, direct-mapped by the predecessor ShapeId: the
/// shape a receiver on `from` reaches by deleting its last key. Both ids are
/// integers (no GC root). An edge is a fact of `from`'s immutable record (a
/// shared, tombstone-free key list whose admission it passed), and ShapeIds
/// are never reused, so an entry is valid while its successor's record is
/// present; the receiver's own flags are re-checked by the caller first.
const ROLLBACK_EDGES: usize = 64;

thread_local! {
    static ROLLBACK_EDGE_CACHE: [std::cell::Cell<(u32, u32)>; ROLLBACK_EDGES] = const {
        const EMPTY: std::cell::Cell<(u32, u32)> = std::cell::Cell::new((0, 0));
        [EMPTY; ROLLBACK_EDGES]
    };
}

fn rollback_edge_slot(from: u32) -> usize {
    (u64::from(from).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 58) as usize % ROLLBACK_EDGES
}

/// The learned successor of `from` for a delete of its last key (the
/// `count`-th), while that successor is present.
fn cached_rollback_edge(from: u32, count: u32) -> Option<u32> {
    let (key, to) = ROLLBACK_EDGE_CACHE.with(|cache| cache[rollback_edge_slot(from)].get());
    if key != from || to == 0 {
        return None;
    }
    let from_record = ShapeSlab::agent_record_present(from)?;
    let to_record = ShapeSlab::agent_record_present(to)?;
    // SAFETY: present records of this agent, read immediately.
    unsafe {
        ((*from_record).logical_key_count == count
            && (*to_record).logical_key_count + 1 == count
            && (*to_record).keys == (*from_record).keys)
            .then_some(to)
    }
}

fn note_rollback_edge(from: u32, to: u32) {
    ROLLBACK_EDGE_CACHE.with(|cache| cache[rollback_edge_slot(from)].set((from, to)));
}
