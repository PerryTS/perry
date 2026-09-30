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
    deprecate_lane(record, slot);
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

/// Deprecate lane `slot` of `record` (a learned fact). The first time, move
/// the prototype-validity word: every emitted key-add memo records it and
/// refuses on a mismatch, so no memo keeps serving the deprecated shape as a
/// key-add target; the runtime key-add then resolves onward to the
/// normalized shape and new objects are born into it (DESIGN §1.5 step 4).
/// Bounded like generalization itself: once per lane of a lineage.
fn deprecate_lane(record: super::shapes::ShapeRecordRef, slot: u32) {
    if record.deprecate_rep_slot(slot) {
        crate::object::proto_validity::bump_proto_validity();
        crate::proxy::store_census(crate::proxy::C_REP_VALIDITY_BUMP);
    }
}

/// Key-add convergence (DESIGN §1.5 step 4): a lineage slot that has seen
/// both a Number and a non-Number is `Any`. `obj` was just stamped `id` by a
/// key-add at `slot` that stored `value_bits`. The sibling of `id` that
/// differs only in lane `slot` (`F64` for a non-Number store, `Any` for a
/// Number store) is looked up in the identity table (one probe, never a
/// mint); when it exists, the `F64` one of the two is deprecated at `slot`,
/// so Number key-adds resolve onward to the `Any` shape, objects still
/// carrying the `F64` one migrate on their next miss, and `obj` itself is
/// restamped to the normalized shape (it holds a Number or its lane is
/// already `Any`). Returns the id `obj` carries.
unsafe fn converge_key_add(obj: *mut ObjectHeader, id: u32, slot: u32, value_bits: u64) -> u32 {
    if slot >= REP_SLOTS {
        return id;
    }
    let Some(d) = shape_descriptor_by_id(id) else {
        return id;
    };
    if field_rep::has_deprecated(d.rep) {
        return id;
    }
    let number = field_rep::f64_slot_bits(value_bits).is_some();
    let lane = slot_rep(d.rep, slot);
    let other = match (number, lane) {
        (false, REP_ANY) => field_rep::REP_F64,
        (true, field_rep::REP_F64) => REP_ANY,
        _ => return id,
    };
    let Some(sibling) = super::shapes::shape_descriptor_find_with_rep(
        d.keys as usize as *const crate::array::ArrayHeader,
        d.logical_key_count,
        d.live_inline_slot_count,
        d.semantic_generation,
        d.object_kind,
        d.hole_count,
        d.proto_id,
        d.summary,
        field_rep::with_slot_rep(d.rep, slot, other),
    ) else {
        return id;
    };
    let f64_shape = if number { id } else { sibling };
    crate::proxy::store_census(crate::proxy::C_REP_CONVERGE);
    if let Some(record) = shape_record_by_id(f64_shape) {
        deprecate_lane(record, slot);
    }
    if !number {
        return id;
    }
    let target = normalized_shape(id);
    if target != id {
        stamp_object_shape_id_with_carrier_note(obj, target);
    }
    target
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
    crate::proxy::store_census(crate::proxy::C_REP_MIGRATE);
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

/// [`migrate_on_miss`] for a miss entry that receives its receiver as a
/// NaN-boxed value: only a pointer-tagged value is considered.
#[inline]
pub(crate) fn migrate_on_miss_value(bits: u64) {
    if bits & crate::value::TAG_MASK == crate::value::POINTER_TAG {
        // SAFETY: `migrate_on_miss` accepts any address and considers only a
        // live shaped object.
        unsafe { migrate_on_miss((bits & crate::value::POINTER_MASK) as usize) };
    }
}

/// The rep of shape `id` (`Any` for an unknown id).
#[inline]
pub(crate) fn shape_rep(id: u32) -> u64 {
    shape_record_by_id(id).map_or(REP_ANY, |record| record.rep())
}

/// T2: the rep of the shape a key-add at `slot` produces from a predecessor
/// carrying `pred_rep`. The predecessor's lanes below `slot` carry (its
/// deprecated lanes normalized to `Any`); the new lane is `F64` iff the value
/// is a JS Number stored INLINE (an overflow slot is outside the store
/// check, so it is always `Any`). A key-only add (`value_bits` = `None`)
/// stores no value yet: its lane is `Any`.
#[inline]
pub(crate) fn key_add_rep(pred_rep: u64, slot: u32, value_bits: Option<u64>, inline: bool) -> u64 {
    let carried = field_rep::normalized(pred_rep) & field_rep::lanes_below(slot);
    if slot >= REP_SLOTS {
        return carried;
    }
    let lane = match value_bits {
        Some(bits) if inline && field_rep::f64_slot_bits(bits).is_some() => field_rep::REP_F64,
        _ => REP_ANY,
    };
    field_rep::with_slot_rep(carried, slot, lane)
}

/// T2 for a cached key-add edge: may the edge's `target` serve a value of
/// this class at `slot`? The target is the class guard (no bit in the cache
/// key): an `F64` lane admits only a Number, a target with a deprecated lane
/// never serves (the slow path resolves onward to its normalized form), and
/// an `Any` lane admits every value (always a valid claim; a lineage whose
/// edge was learned from a non-Number converges on it). `value_bits` = `None`
/// is a key-only add, which an `F64` lane refuses.
#[inline]
pub(crate) fn cached_key_add_admits(target: u32, slot: u32, value_bits: Option<u64>) -> bool {
    let rep = shape_rep(target);
    if rep == REP_ANY {
        return true;
    }
    if field_rep::has_deprecated(rep) {
        return false;
    }
    slot_rep(rep, slot) == REP_ANY
        || value_bits.is_some_and(|bits| field_rep::f64_slot_bits(bits).is_some())
}

/// T2 at a slow-path key-add: publish the keys edge `new_keys`, which appends
/// `slot`, with the successor's rep in the LAST publish before the caller's
/// value store, so the all-`Any` twin of the successor is never minted.
/// Returns the id `obj` carries (the id to teach the transition cache). May
/// mint, so it is a collection point: callers re-read their roots after it.
///
/// * The bound grows (`slot` is outside the live bound): the keys edge is
///   published at the OLD bound (the new slot is outside it, so that
///   intermediate is the same shape as without step 5), then the bound
///   publish carries the rep. The slot holds its allocation-time `undefined`
///   from that stamp to the caller's store; nothing in between collects, and
///   no mint happens after the stamp (mint-then-stamp).
/// * The slot is already inside the live bound: a Number is written into it
///   FIRST (a non-pointer, and the slot is past the key list, so no reader
///   sees it), then the keys edge carries the rep. The `F64` claim holds at
///   every instant, collections inside the mint included.
/// * Overflow / key-only adds carry the predecessor's lanes; the new slot is
///   `Any` (and outside the rep's reach).
pub(crate) unsafe fn publish_key_add_edge(
    obj: *mut ObjectHeader,
    new_keys: super::ObjectKeys,
    pred_rep: u64,
    slot: u32,
    value_bits: Option<u64>,
    inline: bool,
) -> u32 {
    let rep = key_add_rep(pred_rep, slot, value_bits, inline);
    if inline && slot >= super::object_live_slot_count(obj) {
        super::set_object_keys(obj, new_keys);
        super::mark_object_dynamic_shape_unknown(obj);
        super::shapes::publish_object_live_slot_count_rep(obj, slot + 1, Some(rep));
    } else {
        if slot_rep(rep, slot) == field_rep::REP_F64 {
            if let Some(bits) = value_bits.and_then(field_rep::f64_slot_bits) {
                super::slot_store::store_object_field_slot(obj, slot as usize, bits);
            }
        }
        let live = super::object_live_slot_count(obj);
        super::set_object_keys_with_live_rep(obj, new_keys, live, rep);
        super::mark_object_dynamic_shape_unknown(obj);
    }
    let id = publish_key_add_rep(obj, pred_rep, slot, value_bits, inline);
    match value_bits {
        Some(bits) if inline && id != 0 && !crate::object::dictionary::is_dictionary(obj) => {
            converge_key_add(obj, id, slot, bits)
        }
        _ => id,
    }
}

/// The fix-up after [`publish_key_add_edge`]: restamp `obj` to the successor
/// that carries the predecessor's lanes plus the new lane when the publish
/// could not carry it (a stable-tombstone receiver keeps its id across an
/// append; the identity table answered with a record that has since learned
/// a deprecated lane). A no-op when the stamped rep already is the rep.
/// Runs before the value is written (shape word first, §3.3).
unsafe fn publish_key_add_rep(
    obj: *mut ObjectHeader,
    pred_rep: u64,
    slot: u32,
    value_bits: Option<u64>,
    inline: bool,
) -> u32 {
    let id = object_shape_stamp(obj);
    if id == 0 || crate::object::dictionary::is_dictionary(obj) {
        return id;
    }
    let Some(d) = shape_descriptor_by_id(id) else {
        return id;
    };
    let rep = key_add_rep(pred_rep, slot, value_bits, inline);
    if rep == d.rep {
        return id;
    }
    let target = normalized_shape(publish_shape_result(shape_descriptor_intern_with_rep(
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
        // A re-intern of a live record's facts under another rep names no
        // static id.
        None,
    )));
    if target != id {
        stamp_object_shape_id_with_carrier_note(obj, target);
    }
    target
}

/// Is `slot` of shape `id` an `Any` lane (the store IC words' flag: a
/// non-`Any` lane is published with the flag that makes the emitted hit
/// check the value, DESIGN §3.2)?
#[inline]
pub(crate) fn shape_slot_is_any(id: u32, slot: u32) -> bool {
    object_slot_rep_of(id, slot) == REP_ANY
}

/// Does every trace of an object check the field-representation invariant
/// ([`assert_f64_lanes_hold_numbers`])? Always in a debug build or with the
/// `field-rep-assert` feature; with `gc-instruments`, when
/// `PERRY_FIELD_REPR_VERIFY=1` (DESIGN §3.1 verify mode). A binary built
/// without either feature compiles no check, and the knob is one of
/// `gc::instruments::INSTRUMENT_KNOBS`, so setting it there aborts at startup
/// instead of passing having checked nothing.
#[cfg(any(
    debug_assertions,
    feature = "field-rep-assert",
    feature = "gc-instruments"
))]
#[inline]
pub(crate) fn field_rep_verify_enabled() -> bool {
    #[cfg(any(debug_assertions, feature = "field-rep-assert"))]
    {
        true
    }
    #[cfg(not(any(debug_assertions, feature = "field-rep-assert")))]
    {
        use std::sync::OnceLock;
        static CACHED: OnceLock<bool> = OnceLock::new();
        *crate::once_init::get_or_init(&CACHED, || {
            matches!(
                std::env::var("PERRY_FIELD_REPR_VERIFY").ok().as_deref(),
                Some("1") | Some("on") | Some("true")
            )
        })
    }
}

/// T1 birth fill: every `F64` lane of `obj`'s (birth) shape starts as `+0.0`
/// instead of the allocator's `undefined`, so the invariant holds from the
/// moment the object exists, before its constructor stores (a collection may
/// run in between). Codegen gives a class `F64` lanes only for fields its
/// constructor proof writes before anything can read them, so the `+0.0` is
/// never observed. Codegen's inline allocation emits the same fill itself.
///
/// # Safety
/// `obj` is a freshly allocated, stamped, unpublished ordinary object.
pub(crate) unsafe fn birth_fill_f64_lanes(obj: *mut ObjectHeader) {
    let Some(record) = super::shapes::object_shape_record(obj) else {
        return;
    };
    // Deprecated lanes too: a birth into a lineage that has generalized a
    // lane still carries it (the id is fixed), and the invariant covers it.
    let mut lanes = field_rep::f64_lane_slots(field_rep::identity(record.rep()));
    if lanes == 0 {
        return;
    }
    let live = record.live_inline_slot_count();
    let fields = (obj as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *mut u64;
    while lanes != 0 {
        let slot = lanes.trailing_zeros();
        lanes &= lanes - 1;
        if slot < live {
            // GC_STORE_AUDIT(INIT): a fresh unpublished object's F64 lane;
            // +0.0 is a canonical double and never a pointer.
            *fields.add(slot as usize) = 0.0f64.to_bits();
        }
    }
}

/// The field-representation invariant (charter step 5): inside the live
/// bound, every slot under an `F64` (or deprecated) lane of the receiver's
/// shape holds a canonical double. Run at every trace of an object when
/// [`field_rep_verify_enabled`], so a writer that skips the store check trips
/// it at the next collection.
#[cfg(any(
    debug_assertions,
    feature = "field-rep-assert",
    feature = "gc-instruments"
))]
pub(crate) unsafe fn assert_f64_lanes_hold_numbers(
    obj: *const ObjectHeader,
    record: Option<super::shapes::ShapeRecordRef>,
    live: usize,
) {
    let Some(record) = record else {
        return;
    };
    let rep = record.rep();
    if rep == REP_ANY {
        return;
    }
    let fields = (obj as *const u8).add(std::mem::size_of::<ObjectHeader>()) as *const u64;
    for slot in 0..live.min(REP_SLOTS as usize) {
        if slot_rep(rep, slot as u32) == REP_ANY {
            continue;
        }
        let bits = *fields.add(slot);
        if field_rep::f64_slot_bits(bits) != Some(bits) {
            panic!(
                "field-rep invariant: slot {slot} of {obj:p} (shape {:#x}, rep {rep:#x}) holds {bits:#018x}, not a canonical double",
                object_shape_stamp(obj)
            );
        }
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
            // A re-intern of a live record's facts under another rep names
            // no static id.
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
