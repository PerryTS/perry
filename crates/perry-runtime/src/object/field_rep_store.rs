//! Charter step 5 (P2): the runtime store check and the representation
//! transitions of a shape's fields (DESIGN §1.4 T3/T4, §1.5).
//!
//! A store into an inline slot whose shape says `F64` keeps the slot `F64`
//! when the value is a JS Number (stored as its canonical double: an INT32
//! box becomes its double, any NaN the canonical NaN). Any other value
//! GENERALIZES the slot before it is written:
//!
//! 1. the carried shape S marks the lane deprecated (`10`, a learned fact of
//!    the record, not identity), so the whole lineage learns it;
//! 2. T := the same facts with every deprecated lane (this one included)
//!    `Any`, found or minted through the one identity table (`by_facts`);
//! 3. the receiver's header gets T **first**, then the caller writes the
//!    value. No safepoint separates the two (§3.3): a collection between them
//!    would see the old Number under an `Any` lane, which the tag test
//!    ignores; the reverse order would show a pointer in an `F64` slot.
//!
//! Objects still carrying S keep a valid shape (every one of them holds a
//! Number at the lane), and converge on T on their next miss
//! ([`migrate_deprecated_receiver`]): one header store, no data movement,
//! because a raw-double Number IS a boxed Number.
//!
//! Nothing is added besides the record word: no per-object bit, no side
//! table. The only table is `by_facts`.

use super::field_rep::{self, slot_rep, REP_ANY, REP_SLOTS};
use super::shapes::{
    object_shape_stamp, publish_shape_result, shape_descriptor_by_id,
    shape_descriptor_intern_with_rep, shape_record_by_id, stamp_object_shape_id_with_carrier_note,
};
use super::ObjectHeader;

/// The representation of `obj`'s inline slot `field_index`: its shape's
/// lane, or `Any` for an unshaped receiver or a slot past the word.
#[inline]
pub(crate) unsafe fn object_slot_rep(obj: *const ObjectHeader, field_index: usize) -> u64 {
    if field_index >= REP_SLOTS as usize {
        return REP_ANY;
    }
    let id = object_shape_stamp(obj);
    if id == 0 {
        return REP_ANY;
    }
    match shape_record_by_id(id) {
        Some(record) => slot_rep(record.rep(), field_index as u32),
        None => REP_ANY,
    }
}

/// The store check of the runtime slot funnel (`slot_store`): the bits to
/// write into `obj`'s inline slot `field_index` for the JS value
/// `value_bits`. For an `Any` lane that is the value unchanged. For an `F64`
/// (or deprecated) lane it is the Number's canonical double, or, for any
/// other value, the value unchanged AFTER the slot has been generalized and
/// the receiver restamped (shape word first).
#[inline]
pub(crate) unsafe fn checked_slot_bits(
    obj: *mut ObjectHeader,
    field_index: usize,
    value_bits: u64,
) -> u64 {
    if object_slot_rep(obj, field_index) == REP_ANY {
        return value_bits;
    }
    match field_rep::f64_slot_bits(value_bits) {
        Some(bits) => bits,
        None => {
            object_store_generalize(obj, field_index as u32);
            value_bits
        }
    }
}

/// T4: `obj` is about to store a non-Number into its `F64` (or deprecated)
/// lane `slot`. Deprecate the lane on the carried shape and restamp `obj` to
/// the normalized successor. The caller writes the value afterwards.
#[cold]
#[inline(never)]
pub(crate) unsafe fn object_store_generalize(obj: *mut ObjectHeader, slot: u32) {
    let id = object_shape_stamp(obj);
    let Some(record) = shape_record_by_id(id) else {
        return;
    };
    record.deprecate_rep_slot(slot);
    let target = normalized_shape(id);
    debug_assert_eq!(
        object_slot_rep_of(target, slot),
        REP_ANY,
        "generalized lane {slot} of shape {id:#x} is not Any in {target:#x}"
    );
    if target != id {
        stamp_object_shape_id_with_carrier_note(obj, target);
    }
}

/// Migrate-on-miss (DESIGN §1.5 step 4): a receiver whose shape has a
/// deprecated lane is restamped to the normalized shape, so a site converges
/// on it instead of going polymorphic. Returns whether it restamped.
#[inline]
pub(crate) unsafe fn migrate_deprecated_receiver(obj: *mut ObjectHeader) -> bool {
    let id = object_shape_stamp(obj);
    if id == 0 {
        return false;
    }
    match shape_record_by_id(id) {
        Some(record) if field_rep::has_deprecated(record.rep()) => {}
        _ => return false,
    }
    let target = normalized_shape(id);
    if target == id {
        return false;
    }
    stamp_object_shape_id_with_carrier_note(obj, target);
    true
}

/// [`migrate_deprecated_receiver`] for a miss entry's receiver, which may be
/// any address: only a live shaped object is considered.
#[inline]
pub(crate) unsafe fn migrate_on_miss(addr: usize) {
    let obj = addr as *mut ObjectHeader;
    if super::object_is_shaped(obj) {
        migrate_deprecated_receiver(obj);
    }
}

/// The shape a generalized lineage converges to from `id`: the same facts
/// with every deprecated lane `Any`. The record the identity table answers
/// with may itself have learned a deprecated lane since, so this follows the
/// normalization until it is a fixed point; each hop drops at least one
/// `F64` lane from identity, so there are at most [`REP_SLOTS`] hops.
pub(crate) fn normalized_shape(mut id: u32) -> u32 {
    loop {
        let Some(d) = shape_descriptor_by_id(id) else {
            return id;
        };
        let rep = field_rep::normalized(d.rep);
        if rep == d.rep {
            return id;
        }
        let next = publish_shape_result(shape_descriptor_intern_with_rep(
            d.keys as usize as *const crate::array::ArrayHeader,
            d.logical_key_count,
            d.live_inline_slot_count,
            d.semantic_generation,
            d.object_kind,
            d.hole_count,
            d.proto_id,
            // The record's complete summary: the same facts, another rep.
            d.summary,
            rep,
            // A re-intern under another rep names no static id (only
            // REP_ANY facts can).
            None,
        ));
        debug_assert_ne!(next, id, "normalizing {id:#x} found itself");
        if next == id {
            return id;
        }
        id = next;
    }
}

#[inline]
fn object_slot_rep_of(id: u32, slot: u32) -> u64 {
    shape_record_by_id(id).map_or(REP_ANY, |record| slot_rep(record.rep(), slot))
}
