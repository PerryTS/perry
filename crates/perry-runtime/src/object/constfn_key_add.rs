//! Ordered reuse of a body-specific key-add edge. The existing weak
//! transition cache supplies live target keys; shape records own body facts.
//! No new registry or closure root is introduced; a miss keeps the ordered mint.

use super::{field_rep, field_rep_store, shapes, ObjectHeader};

/// The caller has completed the ordinary Set interception/attribute vet.
/// Reuse only an exact current cached body edge whose Any publication shape
/// is still present. This path never allocates in the GC heap or enters JS:
/// table lookup, carrier notes, the regular barrier and both stamps are
/// non-collecting. The current closure is written under Any before SPECIAL.
unsafe fn try_cached_constfn_key_add(
    obj: *mut ObjectHeader,
    predecessor: u32,
    hit: (super::ObjectKeys, u32, u32),
    bits: u64,
) -> bool {
    let (keys, slot, target) = hit;
    let Some(d) = shapes::shape_descriptor_by_id(target) else {
        return false;
    };
    // Reuse an edge whose only ConstFn lane is the appended slot. Other
    // target forms retain the existing slow publication contract.
    if slot >= field_rep::REP_SLOTS
        || d.special_constfn_mask != 1u32 << slot
        || d.deprecation_targets() != (0, 0)
        || field_rep::has_deprecated(d.rep)
        || d.keys != keys.arr() as usize as u64
        || d.logical_key_count != keys.count()
        || d.live_inline_slot_count <= slot
        || d.live_inline_slot_count
            > super::object_live_slot_count(obj).max(super::INLINE_SLOT_FLOOR as u32)
        || d.constfn_infos().len() != 1
        || u32::from(d.constfn_infos()[0].slot) != slot
        || field_rep_store::constfn_store_info(bits) != Some(d.constfn_infos()[0].info)
    {
        return false;
    }
    let base_rep = field_rep::with_slot_rep(d.rep, slot, field_rep::REP_ANY);
    let Some(any) = shapes::shape_descriptor_find_with_rep(
        keys.arr(),
        d.logical_key_count,
        d.live_inline_slot_count,
        d.semantic_generation,
        d.object_kind,
        d.hole_count,
        d.proto_id,
        d.summary,
        base_rep,
        d.brands(),
    ) else {
        // A collector may have pruned the uncarried Any intermediate. Re-mint
        // it through the rooted slow path rather than cache a second edge.
        return false;
    };
    if !shapes::shape_descriptor_by_id(any).is_some_and(|base| {
        base.rep == base_rep
            && base.special_constfn_mask == 0
            && base.deprecation_targets() == (0, 0)
    }) {
        return false;
    }
    if !shapes::install_cached_object_shape_transition(obj, predecessor, any, keys) {
        return false;
    }
    #[cfg(test)]
    assert_eq!(
        field_rep_store::object_slot_rep(obj, slot as usize),
        field_rep::REP_ANY
    );
    super::slot_store::store_object_field_slot(obj, slot as usize, bits);
    // No safepoint or callback separates the barrier from this exact shape
    // stamp. The slot now contains this receiver's current safe closure.
    #[cfg(test)]
    {
        assert_eq!(
            field_rep_store::object_slot_rep(obj, slot as usize),
            field_rep::REP_ANY
        );
        let fields = (obj as *const u8).add(std::mem::size_of::<ObjectHeader>()) as *const u64;
        assert_eq!(
            *fields.add(slot as usize),
            bits,
            "SPECIAL requires the current closure"
        );
    }
    shapes::stamp_object_shape_id_with_carrier_note(obj, target);
    true
}

/// Ordinary cache hits retain their existing publication. A completed ConstFn
/// hit has already stored this receiver's value under Any and stamped SPECIAL.
pub(super) enum CachedKeyAdd {
    Transition(super::ObjectKeys, u32, u32),
    StoredConstFn,
}

impl CachedKeyAdd {
    #[inline(always)]
    pub(super) fn transition(self) -> Option<(super::ObjectKeys, u32, u32)> {
        match self {
            Self::Transition(keys, slot, target) => Some((keys, slot, target)),
            Self::StoredConstFn => None,
        }
    }
}

/// Resolve a single existing cache probe after the caller's Set semantic vet.
/// Key-only publication retains its SPECIAL refusal in `cached_key_add_admits`.
#[inline(always)]
pub(super) unsafe fn admit_or_store(
    obj: *mut ObjectHeader,
    predecessor: u32,
    hit: (super::ObjectKeys, u32, u32),
    bits: u64,
) -> Option<CachedKeyAdd> {
    if field_rep_store::cached_key_add_admits(hit.2, hit.1, Some(bits)) {
        return Some(CachedKeyAdd::Transition(hit.0, hit.1, hit.2));
    }
    if try_cached_constfn_key_add(obj, predecessor, hit, bits) {
        return Some(CachedKeyAdd::StoredConstFn);
    }
    #[cfg(feature = "shape-mint-diag")]
    super::shape_mint_census::note_transition_rep_refused();
    None
}

#[cfg(test)]
#[path = "constfn_key_add_tests.rs"]
mod tests;
