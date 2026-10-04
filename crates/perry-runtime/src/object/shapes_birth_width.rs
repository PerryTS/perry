//! #10905: in-object slack tracking, owned by the BIRTH shape.
//!
//! An object born with no keys (`Object.create(P)`) used to get the
//! allocator's two-slot floor whatever its program went on to store, so its
//! third own key and every later one lived in overflow storage for the
//! object's whole life (overflow is permanent: nothing moves a spilled value
//! back inline). A spilled store measured ~220 instructions over an inline
//! one and a spilled read ~100.
//!
//! The fix is V8's in-object slack tracking, expressed as a fact of the
//! shape. `Object.create(P)` is born on the keyless shape `(P, [])` — the
//! BIRTH shape of every object created from P, one per prototype because the
//! prototype is part of shape identity (#11342). That record, and nothing
//! else, carries two numbers in bits its word already reserved:
//!
//! * the WIDTH its descendants grow to: the largest key count of any shape
//!   minted with prototype P (every shape of the transition tree below the
//!   birth shape is minted exactly once, so this is the tree's maximum, which
//!   is what V8 computes when tracking completes), raised further by any
//!   descendant that spills past its inline slots;
//! * a count of the births served while tracking.
//!
//! The first [`TRACKING_BIRTHS`] births are allocated [`TRACKING_WIDTH`] slots
//! wide (or wider, if the width learned so far is larger), so the objects a
//! program creates first — often the only ones — keep their fields inline.
//! Every later birth is allocated at exactly the learned width, and at the
//! allocator's floor when nothing grew. The width is capacity only: the keys
//! stay authoritative, and a birth at width `w` is stamped with the shape
//! `(P, [], live w)`, the same "(keys, width) birth ShapeId" a class born
//! wide gets (`js_object_shape_id_for_class_keys_live`, #11360).
//!
//! Objects already allocated keep working: a key past their inline slots
//! spills exactly as before, and that spill is what teaches the record.
//! Nothing here is consulted by a read or a write; only the allocation of a
//! keyless birth asks, and only the mint and spill paths teach.
//!
//! Polymorphic growth takes the MAXIMUM: descendants of one birth shape that
//! grow to different widths are all born at the widest, capped at
//! [`LEARNED_WIDTH_MAX`] slots. That is the memory cost, and it is bounded:
//! an object smaller than the maximum carries at most the difference in
//! unused inline slots, against the 16-slot overflow array (plus header) the
//! narrow birth allocates on its first spill.
//!
//! Lifetime: the facts live exactly as long as the record. The record is
//! kept through a full collection when a birth asked it during the epoch
//! before ([`RECORD_FLAG_BIRTH_OWNER`], cleared by the epoch rotation), and
//! pruned like any uncarried shape otherwise — a prototype nobody creates
//! from any more forgets its width and relearns it on its next births.

use super::shapes_store::{self, ShapeRecord, RECORD_FLAG_BIRTH_OWNER, RECORD_FLAG_FACTS_INDEXED};
use super::{ShapeObjectKind, ShapeTableInner, PROTO_ID_CLASS, PROTO_ID_DEFAULT};

/// How many births of a keyless birth shape are served while tracking.
pub(crate) const TRACKING_BIRTHS: u32 = 8;
/// The width a birth is served while tracking (unless more was learned).
pub(crate) const TRACKING_WIDTH: u32 = 8;
/// The largest learned width a birth is ever served. Also bounds the byte
/// the width is stored in.
pub(crate) const LEARNED_WIDTH_MAX: u32 = 64;

/// Is `proto_id` a recorded prototype OBJECT's serial — the identity
/// `Object.create(P)` gives its result, and the only band whose keyless
/// birth shape is ever consulted? Excludes the default `Object.prototype`
/// (0: literals), a null prototype, and the class/mixed/unique bands.
#[inline]
pub(super) fn is_prototype_serial(proto_id: u64) -> bool {
    proto_id != PROTO_ID_DEFAULT && proto_id < PROTO_ID_CLASS
}

/// The keyless birth record of `proto_id`, if one is present. Probe only:
/// never mints.
fn find_birth_record(
    inner: &ShapeTableInner,
    slab: &shapes_store::ShapeSlab,
    proto_id: u64,
) -> Option<*mut ShapeRecord> {
    let facts =
        shapes_store::facts_key_proto(0, 0, 0, 0, ShapeObjectKind::Ordinary, 0, proto_id, 0, 0);
    let ids = inner.by_facts.get(&facts)?;
    for &id in ids.as_slice() {
        let Some(record) = slab.record_ptr(id) else {
            continue;
        };
        // SAFETY: a live slab record, read immediately on this agent.
        let r = unsafe { &*record };
        if r.has(RECORD_FLAG_FACTS_INDEXED)
            && r.facts_match_proto(0, 0, 0, 0, ShapeObjectKind::Ordinary, 0, proto_id, 0, 0)
        {
            return Some(record);
        }
    }
    None
}

/// Teach the keyless birth record of `proto_id` that a descendant reached
/// `width` inline slots. A no-op when no such record is present.
pub(super) fn note_descendant_width(
    inner: &ShapeTableInner,
    slab: &shapes_store::ShapeSlab,
    proto_id: u64,
    width: u32,
) {
    if width <= crate::object::INLINE_SLOT_FLOOR as u32 || !is_prototype_serial(proto_id) {
        return;
    }
    if let Some(record) = find_birth_record(inner, slab, proto_id) {
        // SAFETY: a live slab record; single-threaded agent.
        unsafe { (*record).note_descendant_width(width.min(LEARNED_WIDTH_MAX)) };
    }
}

/// The inline width to allocate an object born on the keyless birth shape of
/// `proto_id` with, or 0 for the allocator's floor. Mints the birth shape if
/// it is absent (its first birth, or its first after a prune), counts this
/// birth against the tracking window, and keeps the record through the next
/// full collection.
pub(crate) fn keyless_birth_width(proto_id: u64) -> u32 {
    if !is_prototype_serial(proto_id) {
        return 0;
    }
    let id = super::publish_shape_result(super::shape_descriptor_ensure_with_generation(
        std::ptr::null(),
        0,
        0,
        0,
        ShapeObjectKind::Ordinary,
        proto_id,
        super::ReceiverFacts::NONE,
    ));
    let table = &crate::state::state().shapes;
    let Some(record) = table.slab().record_ptr(id) else {
        return 0;
    };
    // SAFETY: a live slab record; single-threaded agent. No table borrow is
    // held (`shape_descriptor_ensure_with_generation` released it).
    let r = unsafe { &mut *record };
    r.set(RECORD_FLAG_BIRTH_OWNER, true);
    let learned = r.descendant_width();
    let births = r.tracked_births();
    let width = if births < TRACKING_BIRTHS {
        r.set_tracked_births(births + 1);
        learned.max(TRACKING_WIDTH)
    } else {
        learned
    };
    if width <= crate::object::INLINE_SLOT_FLOOR as u32 {
        0
    } else {
        width.min(LEARNED_WIDTH_MAX)
    }
}

/// A spill at `width` slots on `obj` teaches its keyless birth record, so a
/// lineage whose shapes were all minted before the record existed (a prune
/// in between) still learns from the objects that outgrow it.
///
/// Out of line and cold: its caller is the spill store, whose in-capacity
/// fast path must not pay for it.
///
/// # Safety
/// `obj` is a live shaped `ObjectHeader`.
#[cold]
#[inline(never)]
pub(crate) unsafe fn note_spill_width(obj: *const crate::object::ObjectHeader, width: u32) {
    if width <= crate::object::INLINE_SLOT_FLOOR as u32 {
        return;
    }
    let table = &crate::state::state().shapes;
    let slab = table.slab();
    let Some(record) = slab.record_ptr(super::object_shape_stamp(obj)) else {
        return;
    };
    let r = &*record;
    if !r.object_kind().is_ordinary_layout()
        || r.semantic_generation != 0
        || !is_prototype_serial(r.proto_id)
    {
        return;
    }
    let proto_id = r.proto_id;
    let Ok(inner) = table.inner.try_borrow() else {
        return;
    };
    note_descendant_width(&inner, slab, proto_id, width);
}

#[cfg(test)]
#[path = "shapes_birth_width_tests.rs"]
mod tests;
