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
    stamp_linked_final_shape_requested(obj, keys, count, proto_id, proto_bits, lane, None)
}

/// The declaration's private holder identity has already been marked.
/// Requested ids are validated against the complete shape facts.
pub(crate) unsafe fn stamp_linked_final_shape_requested(
    obj: *mut crate::object::ObjectHeader,
    keys: *mut ArrayHeader,
    count: u32,
    proto_id: u64,
    proto_bits: u64,
    lane: impl Fn(u32) -> bool,
    requested: Option<u32>,
) -> bool {
    if obj.is_null()
        || keys.is_null()
        || count == 0
        || (requested.is_none() && !(*obj).meta.is_null())
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
    let Ok(id) = shape_descriptor_intern_with_special_mode(
        keys,
        count,
        count,
        if requested.is_some() {
            (1u64 << 63) | u64::from((*obj).class_id)
        } else {
            0
        },
        store_kind::mint_kind(birth.object_kind, obj),
        0,
        proto_id,
        summary,
        rep,
        &infos,
        // The receiver's private brands carry over (#11791).
        birth.brands(),
        requested,
        true,
    ) else {
        return false;
    };
    if !(*obj).meta.is_null() {
        let meta = (*obj).meta;
        (*meta).prototype = proto_bits;
        // GC_STORE_AUDIT(BARRIERED): the meta may have promoted while the
        // holder was marked. Its prototype slot must be rewritten by minors.
        crate::gc::runtime_write_barrier_slot(
            meta as usize,
            std::ptr::addr_of_mut!((*meta).prototype) as usize,
            proto_bits,
        );
    }
    stamp_object_shape_id_with_carrier_note(obj, id);
    debug_assert_object_shape_parity(obj);
    true
}

/// A declaration holder's static parent identity answers through the existing
/// class-function link (or the realm's Object prototype). Verify that its
/// physical link still names that object before projecting the identity.
/// Mutation mints a different identity when the link changes.
pub(crate) unsafe fn declaration_parent_identity(
    obj: *const crate::object::ObjectHeader,
    recorded: u64,
) -> Option<u64> {
    let record = &*ShapeSlab::agent_record(object_shape_stamp(obj));
    if record.semantic_generation >> 32 != 0x8000_0000 {
        return None;
    }
    let pid = record.proto_id;
    let parent = if pid == PROTO_ID_DEFAULT {
        crate::array::object_prototype_addr_if_resolved() as *const crate::object::ObjectHeader
    } else if (PROTO_ID_CLASS..PROTO_ID_MIXED).contains(&pid) {
        crate::object::class_decl_prototype_object(pid as u32)
    } else {
        return None;
    };
    if !parent.is_null() && crate::value::js_nanbox_pointer(parent as i64).to_bits() == recorded {
        Some(pid)
    } else {
        None
    }
}

/// The reserved declaration identity carries complete keys, rather than an
/// out-of-shape descriptor epoch. It admits the same positional reads as zero.
#[inline]
pub(crate) fn complete_layout_generation(generation: u64) -> bool {
    generation == 0 || generation >> 32 == 0x8000_0000
}

/// Hash-based mutation epochs must never enter the declaration namespace.
#[inline]
pub(crate) fn mutation_generation(hash: u64) -> u64 {
    let generation = hash | (1u64 << 63);
    if generation >> 32 == 0x8000_0000 {
        generation ^ (1u64 << 32)
    } else {
        generation
    }
}

/// A static declaration birth still has every key its declaration supplied.
/// A key deletion leaves that reserved birth id; an Any value write need not.
#[inline]
pub(crate) unsafe fn pristine_declaration_holder(obj: *const crate::object::ObjectHeader) -> bool {
    let id = object_shape_stamp(obj);
    is_static_shape_id(id)
        && (*ShapeSlab::agent_record(id)).semantic_generation
            == ((1u64 << 63) | u64::from((*obj).class_id))
}
