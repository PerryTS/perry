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
//! No per-object bit or side table is added. ConstFn body facts live in the
//! optional extension owned by that shape record; `by_facts` remains the
//! single identity table.

use super::field_rep::{self, slot_rep, REP_ANY, REP_SLOTS};
use super::shapes::{
    object_shape_stamp, publish_shape_result, stamp_object_shape_id_with_carrier_note,
};
use super::ObjectHeader;
use crate::codegen_abi::LayoutWord;
use crate::object::shapes::shape_descriptor_of_receiver;
use crate::object::shapes::shape_record_of_receiver;
use crate::object::shapes::shape_rep_of_receiver;

/// The representation of `obj`'s inline slot `field_index`: its shape's
/// lane, or `Any` for an unshaped receiver or a slot past the word.
#[inline]
pub(crate) unsafe fn object_slot_rep(obj: *const ObjectHeader, field_index: usize) -> u64 {
    if field_index >= REP_SLOTS as usize {
        return object_slot_rep_past_word(obj, field_index as u32);
    }
    // An unstamped receiver (0) or an id with no record reads the absent
    // record's word: `Any` in every lane.
    slot_rep(
        unsafe { crate::object::shapes::shape_rep_of_object(obj, (object_shape_stamp(obj)).0) },
        field_index as u32,
    )
}

/// The lane of a slot past the `rep` word: SPECIAL for a ConstFn body-list
/// entry of the receiver's shape, `Any` otherwise (also for an unshaped
/// receiver). Out of line: a store to a slot below 32 never reaches it.
#[inline(never)]
#[optimize(size)]
unsafe fn object_slot_rep_past_word(obj: *const ObjectHeader, field_index: u32) -> u64 {
    (unsafe { crate::object::shapes::shape_record_of_object(obj, (object_shape_stamp(obj)).0) })
        .map_or(REP_ANY, |record| record.lane(field_index))
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
    let rep = object_slot_rep(obj, field_index);
    if rep == REP_ANY {
        return value_bits;
    }
    if rep == field_rep::REP_SPECIAL {
        let record =
            crate::object::shapes::shape_record_of_object(obj, (object_shape_stamp(obj)).0);
        if record.is_some_and(|r| r.lane_is_i32(field_index as u32)) {
            // I32 at rest (PR C): an int32 Number is stored as its I32 word.
            // Any other value generalizes the slot first: its I32 word is
            // rewritten to its double before the lane leaves I32.
            return match crate::layout_slot::encode_lane(value_bits) {
                Some(word) => word,
                None => {
                    generalize_i32_lane(obj, field_index as u32);
                    value_bits
                }
            };
        }
        let expected = record.and_then(|r| r.constfn_info(field_index as u32));
        if expected.is_some() && expected == constfn_store_info(value_bits) {
            return value_bits;
        }
        // A NoPointer producer, if P5 accepts it, needs its own admission
        // rule. Until then any unmatched SPECIAL slot fails closed to Any.
        object_store_generalize(obj, field_index as u32);
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

/// An I32 lane of `obj` meets a value that is no int32 Number (unproven
/// code; on proven code the facts would have joined the write into the field
/// and it would not be I32). The slot's I32 word becomes its canonical
/// double, then `obj` alone moves to the same shape with the lane `Any`
/// (re-interned through the one identity table, shape word after the slot,
/// so no instant shows an I32 word under `Any` or a double under I32). Other
/// objects keep the lane: their words are still I32. The caller writes the
/// value afterwards.
#[cold]
#[inline(never)]
pub(crate) unsafe fn generalize_i32_lane(obj: *mut ObjectHeader, slot: u32) {
    let id = object_shape_stamp(obj);
    let Some(d) = crate::object::shapes::shape_descriptor_of_object(obj, id.0) else {
        return;
    };
    let rep = field_rep::without_lane(d.rep, slot);
    let infos: Vec<_> = d.constfn_infos().to_vec();
    let brands = d.brands().to_vec();
    // The mint may collect: root the receiver across it.
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_raw_mut_ptr(obj);
    let target = publish_shape_result(super::shapes::shape_descriptor_intern_with_special(
        d.keys_view(),
        d.logical_key_count,
        d.live_inline_slot_count,
        d.semantic_generation,
        d.object_kind,
        d.hole_count,
        d.proto_id,
        d.summary,
        rep,
        &infos,
        &brands,
        None,
        d.integrity_flags,
    ));
    receiver.with_mut_ptr(|obj| {
        let fields = (obj as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *mut u64;
        let w = *fields.add(slot as usize);
        // GC_STORE_AUDIT(POINTER_FREE): the decoded canonical double of an
        // I32 word, never a pointer.
        *fields.add(slot as usize) = crate::layout_slot::decode_word(w);
        if target != LayoutWord(0) && target != id {
            stamp_object_shape_id_with_carrier_note(obj, target);
        }
    });
}

/// Give `obj` I32 lanes at the slots of `mask` (I32 at rest, PR C): a class
/// born keyless reaches its layout through its field definitions, whose
/// key-adds learn lanes from values, never from facts; codegen calls this once
/// the class's own field definitions have run and before any of its code reads
/// a field (`perry-codegen` `expr::rest_fields`). Each slot's Number becomes
/// its I32 word and the shape gains the lanes, in one re-intern. A slot that
/// does not hold an int32 Number keeps its lane (a false proof, which check
/// mode reports at the next proven read). Returns the receiver.
pub(crate) unsafe fn install_i32_lanes(obj: *mut ObjectHeader, mask: u32) {
    let id = object_shape_stamp(obj);
    if install_i32_lanes_learned(obj, id, mask) {
        return;
    }
    let Some(d) = crate::object::shapes::shape_descriptor_of_object(obj, id.0) else {
        return;
    };
    if super::overflow::has_overflow(obj) {
        return;
    }
    let live = d.live_inline_slot_count.min(REP_SLOTS);
    let fields = (obj as *const u8).add(std::mem::size_of::<ObjectHeader>()) as *const u64;
    let mut rep = d.rep;
    let mut lanes = 0u32;
    let mut rest = mask;
    while rest != 0 {
        let slot = rest.trailing_zeros();
        rest &= rest - 1;
        if slot >= live || d.lane_is_constfn(slot) {
            continue;
        }
        if crate::layout_slot::encode_lane(*fields.add(slot as usize)).is_none() {
            crate::layout_slot::false_proof(*fields.add(slot as usize));
            continue;
        }
        rep = field_rep::with_slot_rep(rep, slot, field_rep::REP_SPECIAL);
        lanes |= 1 << slot;
    }
    if lanes == 0 || rep == d.rep {
        return;
    }
    let infos: Vec<_> = d.constfn_infos().to_vec();
    let brands = d.brands().to_vec();
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_raw_mut_ptr(obj);
    let target = publish_shape_result(super::shapes::shape_descriptor_intern_with_special(
        d.keys_view(),
        d.logical_key_count,
        d.live_inline_slot_count,
        d.semantic_generation,
        d.object_kind,
        d.hole_count,
        d.proto_id,
        d.summary,
        rep,
        &infos,
        &brands,
        None,
        d.integrity_flags,
    ));
    if target == LayoutWord(0) || target == id {
        return;
    }
    // The step is a learned edge of the record (as a field definition's is,
    // `field_define_target`): the next instance of the class follows it
    // without a mint. Its key count equals its source's, which no definition
    // edge has, so the two readings never take each other's edge.
    if let Some(record) = shape_record_of_receiver(id.0) {
        record.note_field_define_target(target);
    }
    receiver.with_mut_ptr(|obj| {
        // No safepoint from here to the stamp: the words change under the
        // old lanes (`Any`/`F64`), which the collector reads as non-pointers.
        if object_shape_stamp(obj) != id {
            return;
        }
        let fields = (obj as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *mut u64;
        let mut rest = lanes;
        while rest != 0 {
            let slot = rest.trailing_zeros() as usize;
            rest &= rest - 1;
            if let Some(word) = crate::layout_slot::encode_lane(*fields.add(slot)) {
                // GC_STORE_AUDIT(POINTER_FREE): an I32 word (0x7FFE band).
                *fields.add(slot) = word;
            }
        }
        stamp_object_shape_id_with_carrier_note(obj, target);
    });
}

/// [`install_i32_lanes`] along the record's learned lanes step: the target
/// is the same layout (keys, counts, prototype, kind, attributes, ConstFn
/// lanes) whose rep is the source's with an I32 lane at every slot of
/// `mask`. Noncollecting; `false` when there is no such edge or a slot does
/// not hold an int32 Number.
unsafe fn install_i32_lanes_learned(obj: *mut ObjectHeader, id: LayoutWord, mask: u32) -> bool {
    let Some(from) = crate::object::shapes::shape_record_of_object(obj, id.0) else {
        return false;
    };
    let target = from.field_define_target();
    let Some(to) = shape_record_of_receiver(target.0) else {
        return false;
    };
    let mut rep = from.rep();
    let mut rest = mask;
    while rest != 0 {
        let slot = rest.trailing_zeros();
        rest &= rest - 1;
        rep = field_rep::with_slot_rep(rep, slot, field_rep::REP_SPECIAL);
    }
    // `to` was learned from this very record (a definition edge adds a key),
    // whose facts are immutable: the same key count names the lanes step.
    // Its key list may be the canonical copy of `from`'s, so the list's
    // address is not compared.
    if target == id
        || to.rep() != rep
        || to.logical_key_count() != from.logical_key_count()
        || to.live_inline_slot_count() != from.live_inline_slot_count()
        || to.proto_id() != from.proto_id()
        || to.object_kind() != from.object_kind()
        || to.summary() != from.summary()
        || to.special_constfn_mask() != from.special_constfn_mask()
        || super::overflow::has_overflow(obj)
    {
        return false;
    }
    let fields = (obj as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *mut u64;
    let mut rest = mask;
    while rest != 0 {
        let slot = rest.trailing_zeros() as usize;
        rest &= rest - 1;
        if crate::layout_slot::encode_lane(*fields.add(slot)).is_none() {
            return false;
        }
    }
    let mut rest = mask;
    while rest != 0 {
        let slot = rest.trailing_zeros() as usize;
        rest &= rest - 1;
        if let Some(word) = crate::layout_slot::encode_lane(*fields.add(slot)) {
            // GC_STORE_AUDIT(POINTER_FREE): an I32 word (0x7FFE band).
            *fields.add(slot) = word;
        }
    }
    // As a field definition's learned edge stamps a fresh object
    // (`class_field_chain::try_define`).
    let meta = crate::meta_read!(crate::meta_obj_runtime_born!(obj as *mut _));
    if meta.is_null() || (*meta).flags & super::OBJECT_META_FLAG_IS_PROTOTYPE == 0 {
        super::shapes::stamp_birth_shape_id_with_carrier_note(obj, target);
    } else {
        stamp_object_shape_id_with_carrier_note(obj, target);
    }
    true
}

/// A closure that can preserve an existing ConstFn body claim. Its address
/// is deliberately discarded: factory instances must keep their own current
/// closure and captured values in the slot. A rebindable `this` clone with
/// the same info is not safe for the method site's direct body call.
#[inline]
pub(crate) unsafe fn constfn_store_info(value_bits: u64) -> Option<u64> {
    if value_bits & !crate::value::POINTER_MASK != crate::value::POINTER_TAG {
        return None;
    }
    let addr = (value_bits & crate::value::POINTER_MASK) as usize;
    if !crate::closure::is_closure_ptr(addr) {
        return None;
    }
    let closure = addr as *const crate::closure::ClosureHeader;
    let raw_count = (*closure).capture_count;
    if raw_count & crate::closure::CAPTURES_THIS_FLAG != 0
        && raw_count & crate::closure::NO_THIS_REBIND_FLAG == 0
        && !crate::closure::closure_is_arrow(closure)
    {
        return None;
    }
    let info = (*closure).info;
    let info = info.as_ref()?;
    if info.flags & crate::codegen_abi::FN_PERMANENT_IMAGE == 0 {
        return None;
    }
    Some(info as *const crate::closure::JsFunctionInfo as usize as u64)
}

/// T4: `obj` is about to store a non-Number into its `F64` (or deprecated)
/// lane `slot`. Deprecate the lane on the carried shape and restamp `obj` to
/// the normalized successor. The caller writes the value afterwards.
#[cold]
#[inline(never)]
pub(crate) unsafe fn object_store_generalize(obj: *mut ObjectHeader, slot: u32) {
    let id = object_shape_stamp(obj);
    let Some(record) = (unsafe { crate::object::shapes::shape_record_of_object(obj, (id).0) })
    else {
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
    let changed = if record.lane_is_constfn(slot) {
        record.deprecate_special_to_any(slot)
    } else {
        record.deprecate_rep_slot(slot)
    };
    if changed {}
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
unsafe fn converge_key_add(
    obj: *mut ObjectHeader,
    id: LayoutWord,
    slot: u32,
    value_bits: u64,
) -> u32 {
    if slot >= REP_SLOTS {
        return id.0;
    }
    let Some(d) = shape_descriptor_of_receiver((id).0) else {
        return id.0;
    };
    if field_rep::has_deprecated(d.rep) {
        return id.0;
    }
    let number = field_rep::f64_slot_bits(value_bits).is_some();
    let lane = slot_rep(d.rep, slot);
    let other = match (number, lane) {
        (false, REP_ANY) => field_rep::REP_F64,
        (true, field_rep::REP_F64) => REP_ANY,
        _ => return id.0,
    };
    let Some(sibling) = super::shapes::shape_descriptor_find_with_rep(
        d.keys_view(),
        d.logical_key_count,
        d.live_inline_slot_count,
        d.semantic_generation,
        d.object_kind,
        d.hole_count,
        d.proto_id,
        d.summary,
        field_rep::with_slot_rep(d.rep, slot, other),
        d.brands(),
        d.integrity_flags,
    ) else {
        return id.0;
    };
    let f64_shape = if number { id } else { sibling };
    if let Some(record) = shape_record_of_receiver((f64_shape).0) {
        deprecate_lane(record, slot);
    }
    if !number {
        return id.0;
    }
    let target = normalized_shape(id);
    if target != id {
        stamp_object_shape_id_with_carrier_note(obj, target);
    }
    target.0
}

/// Migrate-on-miss (DESIGN §1.5 step 4): a receiver whose shape has a
/// deprecated lane is restamped to the normalized shape, so a site converges
/// on it instead of going polymorphic. Returns whether it restamped.
#[inline]
pub(crate) unsafe fn migrate_deprecated_receiver(obj: *mut ObjectHeader) -> bool {
    let id = object_shape_stamp(obj);
    if id == LayoutWord(0) {
        return false;
    }
    match unsafe { crate::object::shapes::shape_record_of_object(obj, (id).0) } {
        Some(record)
            if field_rep::has_deprecated(record.rep()) || record.has_special_deprecation() => {}
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

/// The expected id is the completed class layout, never a keyless allocation's
/// birth id. Exact identity is the hot contract; a completed ConstFn shape can
/// additionally serve that layout's field
/// offsets and numeric lanes. The two records supply every fact; no mapping
/// from allocation id to final id is maintained. Writes still check the live
/// slot's rep before skipping the checked store funnel.
pub(crate) fn completed_shape_matches_layout(actual: LayoutWord, expected: LayoutWord) -> bool {
    if actual == expected {
        return true;
    }
    let (Some(a), Some(b)) = (
        shape_descriptor_of_receiver((actual).0),
        shape_descriptor_of_receiver((expected).0),
    ) else {
        return false;
    };
    if a.proto_id != b.proto_id {
        return linked_shape_matches_layout(&a, &b);
    }
    let base_rep = a
        .constfn_infos()
        .iter()
        .fold(a.rep, |rep, i| field_rep::without_lane(rep, i.slot));
    a.has_constfn_lane()
        && !b.has_constfn_lane()
        && a.object_kind == super::shapes::ShapeObjectKind::Ordinary
        && a.object_kind == b.object_kind
        && a.keys == b.keys
        && a.logical_key_count == b.logical_key_count
        && a.live_inline_slot_count == b.live_inline_slot_count
        && a.proto_id == b.proto_id
        && a.semantic_generation == 0
        && b.semantic_generation == 0
        && a.hole_count == 0
        && b.hole_count == 0
        && a.summary == 0
        && b.summary == 0
        && field_rep::identity_with_special(base_rep) == field_rep::identity_with_special(b.rep)
}

/// An instance of a per-evaluation class (`ClassExprFresh`) carries its
/// class's completed layout `b` re-linked to its evaluation's prototype: the same
/// keys, slots, lanes and attributes at a `MIXED` identity of the same class
/// (`object_link_class_evaluation_prototype`). A class field guard reads and
/// writes an OWN data slot, which no prototype can shadow, so `a` serves the
/// guard exactly as `b` does. Any other fact that differs refuses.
fn linked_shape_matches_layout(
    a: &super::shapes::ShapeDescriptor,
    b: &super::shapes::ShapeDescriptor,
) -> bool {
    super::shapes::proto_id_links_class_instance(a.proto_id, b.proto_id)
        && a.object_kind == b.object_kind
        && a.keys == b.keys
        && a.logical_key_count == b.logical_key_count
        && a.live_inline_slot_count == b.live_inline_slot_count
        && a.semantic_generation == 0
        && b.semantic_generation == 0
        && a.hole_count == 0
        && b.hole_count == 0
        && a.summary == b.summary
        && a.special_constfn_mask == b.special_constfn_mask
        && a.constfn_infos()
            .iter()
            .map(|i| i.slot)
            .eq(b.constfn_infos().iter().map(|i| i.slot))
        && field_rep::identity_with_special(a.rep) == field_rep::identity_with_special(b.rep)
}

/// The rep of shape `id` (`Any` for an unknown id).
#[inline]
pub(crate) fn shape_rep(id: LayoutWord) -> u64 {
    shape_record_of_receiver((id).0).map_or(REP_ANY, |record| record.rep())
}

/// T2: the rep of the shape a key-add at `slot` produces from a predecessor
/// carrying `pred_rep`. The predecessor's lanes below `slot` carry (its
/// deprecated lanes normalized to `Any`); the new lane is `F64` iff the value
/// is a JS Number stored INLINE (an overflow slot is outside the store
/// check, so it is always `Any`). A key-only add (`value_bits` = `None`)
/// stores no value yet: its lane is `Any`.
#[inline]
pub(crate) fn key_add_rep(pred_rep: u64, slot: u32, value_bits: Option<u64>, inline: bool) -> u64 {
    let carried = field_rep::normalized_without_special(pred_rep) & field_rep::lanes_below(slot);
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
/// is a key-only add, which an `F64` lane refuses. A SPECIAL target is
/// refused until the cached publication path can write the current closure
/// under Any before it stamps the body-specific shape.
#[inline]
pub(crate) fn cached_key_add_admits(
    target: LayoutWord,
    slot: u32,
    value_bits: Option<u64>,
) -> bool {
    cached_key_add_slot_bits(
        target,
        slot,
        value_bits.unwrap_or(crate::value::TAG_UNDEFINED),
    )
    .is_some()
}

/// The same T2 admission, retaining the canonical slot bits for an exact
/// cached publication. No callback or collection may separate it from use.
#[inline]
pub(crate) fn cached_key_add_slot_bits(
    target: LayoutWord,
    slot: u32,
    value_bits: u64,
) -> Option<u64> {
    let rep = shape_rep(target);
    if rep == REP_ANY {
        return Some(value_bits);
    }
    if field_rep::has_deprecated(rep) {
        return None;
    }
    match slot_rep(rep, slot) {
        REP_ANY => Some(value_bits),
        field_rep::REP_F64 => field_rep::f64_slot_bits(value_bits),
        // The cached transition stamps its target before widening and
        // writing the new slot. A SPECIAL target must take the ordered slow
        // path until the cache hit prewrites under an Any predecessor.
        field_rep::REP_SPECIAL => None,
        _ => None,
    }
}

#[cfg(test)]
thread_local! {
    /// Test seam: the number of [`publish_key_add_edge`] publications after
    /// which a copying minor collection runs, modelling a mint that collects.
    /// Only a test arms it; it is compiled out of every non-test build.
    pub(crate) static TEST_COLLECT_AFTER_KEY_ADD_PUBLISH: std::cell::Cell<u32> =
        const { std::cell::Cell::new(0) };
}

#[cfg(test)]
fn test_collect_after_publish() {
    let armed = TEST_COLLECT_AFTER_KEY_ADD_PUBLISH.with(|c| {
        let n = c.get();
        c.set(n.saturating_sub(1));
        n > 0
    });
    if armed {
        crate::gc::gc_collect_minor();
    }
}

#[cfg(not(test))]
#[inline(always)]
fn test_collect_after_publish() {}

/// T2 at a slow-path key-add: publish the keys edge `new_keys`, which appends
/// `slot`, with the successor's rep in the last publish. For F64 the final
/// shape precedes the caller's value store. For ConstFn, an Any intermediate
/// is minted first, the rooted closure is stored and traced there, and only
/// then is the body-specific successor stamped.
/// Returns the id `obj` carries (the id to teach the transition cache). May
/// mint, so it is a collection point: callers re-read their roots after it.
///
/// * The bound grows (`slot` is outside the live bound): the keys edge is
///   published at the OLD bound (the new slot is outside it, so that
///   intermediate is the same shape as without step 5), then the bound
///   publish carries the rep. For F64 the slot holds its allocation-time
///   `undefined` until the caller's store. For ConstFn the bound publish is
///   Any and the closure is prewritten before SPECIAL is stamped.
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
) -> LayoutWord {
    let rep = key_add_rep(pred_rep, slot, value_bits, inline);
    // A ConstFn birth may be minted only for an executable-image body.
    let candidate = if inline && slot < REP_SLOTS && !super::overflow::has_overflow(obj) {
        value_bits.and_then(|bits| unsafe { constfn_store_info(bits) })
    } else {
        None
    };
    // Every publication below mints, and the GC call-effects classifier
    // cannot prove a mint non-collecting (the keys-array resolvers and the
    // attribute lookup reach a collector), so each one is a collection point.
    // Root the receiver and re-read it at every use after the first publish;
    // root the closure too, so the ConstFn prewrite stores its current
    // address. The final SPECIAL shape is published only after that closure
    // has been written into the now traced Any slot.
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_raw_mut_ptr(obj);
    let value_root = candidate
        .zip(value_bits)
        .map(|(_, bits)| scope.root_nanbox_f64(f64::from_bits(bits)));
    if inline && slot >= super::object_live_slot_count(obj) {
        receiver.with_mut_ptr(|obj| super::set_object_keys(obj, new_keys));
        receiver.with_mut_ptr(|obj| {
            super::shapes::publish_object_live_slot_count_rep(obj, slot + 1, Some(rep))
        });
    } else {
        if slot_rep(rep, slot) == field_rep::REP_F64 {
            if let Some(bits) = value_bits.and_then(field_rep::f64_slot_bits) {
                super::slot_store::store_object_field_slot(obj, slot as usize, bits);
            }
        }
        let live = super::object_live_slot_count(obj);
        super::set_object_keys_with_live_rep(obj, new_keys, live, rep);
    }
    test_collect_after_publish();
    let fresh_bits = value_root
        .as_ref()
        .map(|root| root.get_nanbox_f64().to_bits())
        .or(value_bits);
    let constfn_info = candidate.filter(|&info| {
        fresh_bits.is_some_and(|bits| unsafe { constfn_store_info(bits) } == Some(info))
    });
    if let (Some(_), Some(bits)) = (constfn_info, fresh_bits) {
        // The current shape describes this slot as Any. The regular store
        // barrier makes the closure visible to a moving collection before
        // the body-specific shape is minted or stamped.
        receiver.with_mut_ptr(|obj| {
            super::slot_store::store_object_field_slot(obj, slot as usize, bits)
        });
    }
    let id = receiver.with_mut_ptr(|obj| {
        publish_key_add_rep(obj, pred_rep, slot, fresh_bits, inline, constfn_info)
    });
    test_collect_after_publish();
    let value_bits = value_root
        .as_ref()
        .map(|root| root.get_nanbox_f64().to_bits())
        .or(value_bits);
    LayoutWord(receiver.with_mut_ptr(|obj| match value_bits {
        Some(bits)
            if inline && id != LayoutWord(0) && !crate::object::overflow::has_overflow(obj) =>
        {
            converge_key_add(obj, id, slot, bits)
        }
        _ => id.0,
    }))
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
    constfn_info: Option<u64>,
) -> LayoutWord {
    let id = object_shape_stamp(obj);
    if id == LayoutWord(0) || crate::object::overflow::has_overflow(obj) {
        return id;
    }
    let Some(d) = (unsafe { crate::object::shapes::shape_descriptor_of_object(obj, (id).0) })
    else {
        return id;
    };
    let rep = key_add_rep(pred_rep, slot, value_bits, inline);
    let rep = if constfn_info.is_some() {
        field_rep::with_special_lane(rep, slot)
    } else {
        rep
    };
    if rep == d.rep {
        return id;
    }
    let infos = constfn_info.map(|info| super::shapes::ConstFnSlotInfo { slot, info });
    let target = normalized_shape(publish_shape_result(
        super::shapes::shape_descriptor_intern_with_special(
            d.keys_view(),
            d.logical_key_count,
            d.live_inline_slot_count,
            d.semantic_generation,
            d.object_kind,
            d.hole_count,
            d.proto_id,
            d.summary,
            rep,
            infos.as_slice(),
            &d.brands().to_vec(),
            None,
            d.integrity_flags,
        ),
    ));
    if target != id {
        stamp_object_shape_id_with_carrier_note(obj, target);
    }
    target
}

/// A private field's lanes (#11791). Claiming the `ENTRY_PRIVATE` entry is a
/// key-only append that publishes an all-`Any` rep; the claim is an append
/// like any key-add, though, so its successor's rep is [`key_add_rep`]'s: the
/// predecessor's lanes below `slot` carry (no slot moved), and the new lane
/// is `F64` when the initializer stored a Number into an INLINE slot. This
/// returns that successor of `obj`'s current shape, with the shape it starts
/// from and the slot's canonical double bits, or `None` when the current
/// shape already has that rep (or is a dictionary).
///
/// Mints, so it is a collection point: it reads `obj` only before the mint,
/// and the caller re-reads its root for [`install_private_field_lanes`].
pub(crate) unsafe fn private_field_lanes_target(
    obj: *mut ObjectHeader,
    slot: u32,
    pred_rep: u64,
) -> Option<(u32, u32, u64)> {
    if super::overflow::has_overflow(obj) {
        return None;
    }
    let id = object_shape_stamp(obj);
    let d = (unsafe { crate::object::shapes::shape_descriptor_of_object(obj, (id).0) })?;
    if field_rep::has_deprecated(d.rep)
        || field_rep::special_lane_slots(d.rep) != 0
        || d.has_tail_constfn()
    {
        return None;
    }
    let inline = slot < d.live_inline_slot_count;
    let fields_ptr = (obj as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *const u64;
    let value_bits = inline.then(|| *fields_ptr.add(slot as usize));
    let rep = key_add_rep(pred_rep, slot, value_bits, inline);
    if rep == d.rep {
        return None;
    }
    let bits = value_bits
        .and_then(field_rep::f64_slot_bits)
        .unwrap_or_default();
    let target = normalized_shape(publish_shape_result(
        super::shapes::shape_descriptor_intern_with_special(
            d.keys_view(),
            d.logical_key_count,
            d.live_inline_slot_count,
            d.semantic_generation,
            d.object_kind,
            d.hole_count,
            d.proto_id,
            d.summary,
            rep,
            &[],
            &d.brands().to_vec(),
            None,
            d.integrity_flags,
        ),
    ));
    (target != LayoutWord(0) && target != id).then_some((id.0, target.0, bits))
}

/// Install [`private_field_lanes_target`]'s answer: an `F64` slot's canonical
/// double first (a non-pointer under the current `Any` lane), then the shape
/// (DESIGN §3.3 order). A receiver that changed shape in between keeps its
/// shape.
pub(crate) unsafe fn install_private_field_lanes(
    obj: *mut ObjectHeader,
    slot: u32,
    (from, target, bits): (u32, u32, u64),
) {
    if object_shape_stamp(obj) != LayoutWord(from) {
        return;
    }
    if slot < REP_SLOTS && slot_rep(shape_rep_of_receiver(target), slot) == field_rep::REP_F64 {
        super::slot_store::store_object_field_slot(obj, slot as usize, bits);
    }
    stamp_object_shape_id_with_carrier_note(obj, LayoutWord(target));
}

/// Is `slot` of shape `id` an `Any` lane (the store IC words' flag: a
/// non-`Any` lane is published with the flag that makes the emitted hit
/// check the value, DESIGN §3.2)? The store tests' oracle.
#[cfg(test)]
#[inline]
pub(crate) fn shape_slot_is_any(id: LayoutWord, slot: u32) -> bool {
    object_slot_rep_of(id, slot) == REP_ANY
}

/// Exact raw-double admission. SPECIAL may also be non-Any, but ConstFn
/// holds a pointer and must never take an emitted F64 store/read path.
#[inline]
pub(crate) fn shape_slot_is_f64(id: LayoutWord, slot: u32) -> bool {
    matches!(
        object_slot_rep_of(id, slot),
        field_rep::REP_F64 | field_rep::REP_F64_DEPRECATED
    )
}

/// Does every trace of an object check the field-representation invariant
/// ([`assert_field_rep_lanes`])? Always in a debug build or with the
/// `field-rep-assert` feature; with `gc-instruments`, when
/// `PERRY_FIELD_REPR_VERIFY=1` (DESIGN §3.1 verify mode). A binary built
/// without either feature compiles no check, and the knob is one of
/// `gc::instruments::INSTRUMENT_KNOBS`, so setting it there aborts at startup
/// instead of passing having checked nothing.
#[cfg(any(debug_assertions, feature = "field-rep-assert", perry_gc_instruments))]
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
    let mut lanes = field_rep::f64_lane_slots(field_rep::identity_with_special(record.rep()));
    // I32 at rest (PR C): an I32 lane starts as the I32 word of 0.
    let mut i32_lanes = record.i32_lanes();
    if lanes == 0 && i32_lanes == 0 {
        return;
    }
    let live = record.live_inline_slot_count();
    let fields = (obj as *mut u8).add(std::mem::size_of::<ObjectHeader>()) as *mut u64;
    while i32_lanes != 0 {
        let slot = i32_lanes.trailing_zeros();
        i32_lanes &= i32_lanes - 1;
        if slot < live {
            // GC_STORE_AUDIT(INIT): the I32 word of 0, never a pointer.
            *fields.add(slot as usize) = crate::value::INT32_TAG;
        }
    }
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

/// Verify all admitted field representations, including deprecated carriers:
/// deprecation changes future admission, never an existing carrier's body fact.
/// Collector traversal may precede closure-slot rewriting, so SPECIAL checks
/// resolve validated forwarding before examining closure payload metadata.
#[cfg(any(
    test,
    debug_assertions,
    feature = "field-rep-assert",
    perry_gc_instruments
))]
pub(crate) unsafe fn assert_field_rep_lanes(
    obj: *const ObjectHeader,
    record: Option<super::shapes::ShapeRecordRef>,
    live: usize,
) {
    let Some(record) = record else {
        return;
    };
    let rep = record.rep();
    if rep == REP_ANY && !record.has_tail_constfn() {
        return;
    }
    let fields = (obj as *const u8).add(std::mem::size_of::<ObjectHeader>()) as *const u64;
    for slot in REP_SLOTS as usize..live {
        if record.lane_is_constfn(slot as u32) {
            assert_constfn_slot_body(obj, record, slot, *fields.add(slot));
        }
    }
    for slot in 0..live.min(REP_SLOTS as usize) {
        let bits = *fields.add(slot);
        match slot_rep(rep, slot as u32) {
            field_rep::REP_F64 | field_rep::REP_F64_DEPRECATED => {
                if field_rep::f64_slot_bits(bits) != Some(bits) {
                    panic!(
                        "field-rep invariant: slot {slot} of {obj:p} (shape {:#x}, rep {rep:#x}) holds {bits:#018x}, not a canonical double",
                        object_shape_stamp(obj)
                    );
                }
            }
            field_rep::REP_SPECIAL if record.special_constfn_mask() & (1 << slot) != 0 => {
                assert_constfn_slot_body(obj, record, slot, bits);
            }
            field_rep::REP_SPECIAL => {
                if !crate::layout_slot::is_i32_word(bits) {
                    panic!(
                        "field-rep invariant: I32 slot {slot} of {obj:p} (shape {:#x}, rep {rep:#x}) holds {bits:#018x}, not an I32 word",
                        object_shape_stamp(obj)
                    );
                }
            }
            _ => {} // The reserved optional NoPointer producer remains disabled.
        }
    }
}

/// Also used at the existing cold method-prime refusal: an unchecked store
/// must be diagnosed even if no collection follows before generic dispatch.
#[cfg(any(
    test,
    debug_assertions,
    feature = "field-rep-assert",
    perry_gc_instruments
))]
#[cold]
#[optimize(size)]
#[inline(never)]
pub(crate) unsafe fn assert_constfn_slot_body(
    obj: *const ObjectHeader,
    record: super::shapes::ShapeRecordRef,
    slot: usize,
    bits: u64,
) {
    let expected = record.constfn_info(slot as u32);
    let actual = if bits & !crate::value::POINTER_MASK == crate::value::POINTER_TAG {
        crate::gc::field_rep_live_address((bits & crate::value::POINTER_MASK) as usize)
            .and_then(|addr| constfn_store_info(crate::value::POINTER_TAG | addr as u64))
    } else {
        None
    };
    if expected.is_none() || actual != expected {
        panic!(
            "field-rep invariant: SPECIAL ConstFn slot {slot} of {obj:p} (shape {:#x}) holds {bits:#018x}, body {actual:?} disagrees with shape body {expected:?}",
            object_shape_stamp(obj)
        );
    }
}

/// The shape a generalized lineage converges to from `id`: the same facts
/// with every deprecated lane `Any`. The record the identity table answers
/// with may itself have learned a deprecated lane since, so this follows the
/// normalization until it is a fixed point; each hop drops at least one
/// `F64` lane from identity, so there are at most [`REP_SLOTS`] hops.
#[optimize(size)]
pub(crate) fn normalized_shape(mut id: LayoutWord) -> LayoutWord {
    loop {
        let Some(d) = shape_descriptor_of_receiver((id).0) else {
            return id;
        };
        let (to_nopointer, to_any) = d.deprecation_targets();
        let rep = field_rep::normalized_with_special(d.rep, to_nopointer, to_any);
        // A body past word 0 learned `to_any` in its tail word: its lane is
        // dropped from the successor's list, exactly as word 0's lanes are
        // dropped from its `rep`.
        let tail_gone = |slot: u32| {
            slot >= field_rep::REP_SLOTS
                && d.tail_deprecation((slot / field_rep::REP_SLOTS - 1) as usize)
                    & (1u32 << (slot % field_rep::REP_SLOTS))
                    != 0
        };
        let tail_changes =
            d.has_tail_constfn() && d.constfn_infos().iter().any(|e| tail_gone(e.slot));
        if rep == d.rep && !tail_changes {
            return id;
        }
        let infos: Vec<_> = d
            .constfn_infos()
            .iter()
            .copied()
            .filter(|entry| {
                if entry.slot < field_rep::REP_SLOTS {
                    field_rep::slot_rep(rep, entry.slot) == field_rep::REP_SPECIAL
                } else {
                    !tail_gone(entry.slot)
                }
            })
            .collect();
        let next = publish_shape_result(super::shapes::shape_descriptor_intern_with_special(
            d.keys_view(),
            d.logical_key_count,
            d.live_inline_slot_count,
            d.semantic_generation,
            d.object_kind,
            d.hole_count,
            d.proto_id,
            // The record's complete summary: the same facts, another rep.
            d.summary,
            rep,
            &infos,
            &d.brands().to_vec(),
            // A re-intern of a live record's facts under another rep names
            // no static id.
            None,
            d.integrity_flags,
        ));
        debug_assert_ne!(next, id, "normalizing {id:#x} found itself");
        if next == id {
            return id;
        }
        id = next;
    }
}

#[inline]
pub(crate) fn object_slot_rep_of(id: LayoutWord, slot: u32) -> u64 {
    shape_record_of_receiver((id).0).map_or(REP_ANY, |record| record.lane(slot))
}
