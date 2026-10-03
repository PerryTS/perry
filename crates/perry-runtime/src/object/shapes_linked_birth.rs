//! A fresh object stamped straight into its final, linked shape.
//!
//! The ordinary way an object reaches a shape is one transition per change:
//! a key-add per key, an attribute edit per non-default key, a prototype
//! transition for its link, a lane relearn for the closures it holds. An
//! object whose final keys, attributes, prototype and slot values are all
//! known before it escapes can skip every intermediate shape: this mints the
//! final one from those facts by exact-facts interning (the same record any
//! ordinary path reaching the same facts gets) and stamps it once.

use super::*;

/// Stamp fresh, unobserved `obj` with the shape of `keys[0..count]` (a
/// canonical list carrying its attribute entries), [[Prototype]] identity
/// `proto_id` naming `proto_bits`, and a ConstFn lane on each slot below
/// `count` that `lane` selects and whose value is a closure of one permanent
/// body. Every slot below `count` already holds its value, inline. `obj` has
/// no meta record, so the shape alone records its [[Prototype]]. Returns
/// false, leaving `obj` on its birth shape, when the facts are refused.
///
/// # Safety
/// `obj` is a live, fresh `ObjectHeader` with at least `count` inline slots;
/// `keys` is a live canonical keys array with `count` keys; nothing
/// allocates between the caller's slot stores and this call.
pub(crate) unsafe fn stamp_linked_final_shape(
    obj: *mut crate::object::ObjectHeader,
    keys: *mut ArrayHeader,
    count: u32,
    proto_id: u64,
    proto_bits: u64,
    lane: impl Fn(u32) -> bool,
) -> bool {
    if obj.is_null()
        || keys.is_null()
        || count == 0
        || !(*obj).meta.is_null()
        || !shape_word_is_writable(obj)
    {
        return false;
    }
    let Some(birth) = object_shape_descriptor(obj) else {
        return false;
    };
    let base = (obj as *const u8).add(std::mem::size_of::<crate::object::ObjectHeader>());
    let mut infos: Vec<shapes_store::ConstFnSlotInfo> = Vec::new();
    let mut rep = crate::object::field_rep::REP_ANY;
    for slot in 0..count.min(crate::object::field_rep::REP_SLOTS) {
        if !lane(slot) {
            continue;
        }
        let bits = std::ptr::read(base.add(slot as usize * 8) as *const u64);
        if let Some(info) = crate::object::field_rep_store::constfn_store_info(bits) {
            infos.push(shapes_store::ConstFnSlotInfo {
                slot: slot as u8,
                info,
            });
            rep = crate::object::field_rep::with_slot_rep(
                rep,
                slot,
                crate::object::field_rep::REP_SPECIAL,
            );
        }
    }
    let summary =
        receiver_extra_summary(obj) | crate::object::key_attrs::keys_summary_checked(keys, count);
    // The identity's word names the prototype before any shape names the
    // identity, as in `transition_object_shape_prototype`.
    shapes_prototype::write_identity_word(proto_id, proto_bits);
    let Ok(id) = shape_descriptor_intern_with_special(
        keys,
        count,
        count,
        0,
        store_kind::mint_kind(birth.object_kind, obj),
        0,
        proto_id,
        summary,
        rep,
        &infos,
        None,
    ) else {
        return false;
    };
    stamp_object_shape_id_with_carrier_note(obj, id);
    debug_assert_object_shape_parity(obj);
    true
}
