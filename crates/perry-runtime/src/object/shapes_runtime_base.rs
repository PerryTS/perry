//! The runtime-layout lineage of a shape (object model, layout lineage).
//!
//! A runtime instance layout with fixed slots that native code reads
//! (`INCOMING_MESSAGE`, `SERVER_RESPONSE`: the payload cell, the request's
//! header view) stays the authority for those slots when the program adds
//! keys to its object (`req[sym] = meta`, `res.write = ...`). Such an add
//! moves the object to a dynamic shape whose key list starts with the
//! layout's keys, each at the same slot: the add appends, it never moves a
//! key. The shape's record names that layout as its *runtime base*, so a
//! native reader answers from the fixed slot instead of a key lookup.
//!
//! The base is a function of the two records' immutable facts: the key list
//! prefix (key, entry byte, storage slot), the slots' field lanes, the
//! inline width, the kind and no integrity level. It is learned on first
//! ask and kept in the record's own extension (`ShapeExtras::runtime_base`);
//! it is never identity and no table holds it. A deleted base key leaves a
//! hole in its slot, which every reader still treats as absence.

use super::*;

use crate::codegen_abi::LayoutWord;

/// The runtime instance layouts whose fixed slots native readers use.
#[inline(always)]
fn bases() -> [u32; 2] {
    use crate::object::runtime_layouts as rl;
    [rl::INCOMING_MESSAGE::WORD, rl::SERVER_RESPONSE::WORD]
}

/// The runtime instance layout (its word) whose fixed slots an object on
/// the unshadowed layout word `word` keeps: `word` itself when it is one,
/// the learned base of a dynamic shape that descends from one by key adds,
/// or 0.
#[inline]
pub(crate) fn runtime_base_of_word(word: u32) -> u32 {
    let [incoming, response] = bases();
    if word == incoming || word == response {
        return word;
    }
    // A registered layout (static id) is its own layout or none; shadowed
    // words are the caller's to strip.
    if word & crate::object::layout_desc::LAYOUT_SHADOW_BIT != 0 || is_static_layout_word(word) {
        return 0;
    }
    let Some(record) = ShapeSlab::agent_record_for_word_present(LayoutWord(word)) else {
        return 0;
    };
    // SAFETY: a present slab record of this agent.
    match unsafe { (*record).runtime_base() } {
        0 => unsafe { learn_runtime_base(record) },
        u32::MAX => 0,
        base => base,
    }
}

/// Derive and record the base of `record` (first ask). Records whose first
/// key is no base's first key are answered without being recorded, so an
/// unrelated shape never acquires an extension.
///
/// # Safety
/// `record` is a present slab record of this agent.
#[cold]
#[inline(never)]
unsafe fn learn_runtime_base(record: *mut ShapeRecord) -> u32 {
    let own = (*record).lift(record);
    if own.keys == 0 || own.logical_key_count == 0 {
        return 0;
    }
    let keys = own.keys_view();
    for base in bases() {
        let Some(base_record) = ShapeSlab::agent_record_for_word_present(LayoutWord(base)) else {
            continue;
        };
        let layout = (*base_record).lift(base_record);
        if layout.keys == 0 || layout.logical_key_count == 0 {
            continue;
        }
        let layout_keys = layout.keys_view();
        if own.logical_key_count < layout.logical_key_count
            || keys.get(0).bits() != layout_keys.get(0).bits()
        {
            continue;
        }
        let found = descends_from(&own, keys, &layout, layout_keys);
        (*record).note_runtime_base(if found { base } else { u32::MAX });
        return if found { base } else { 0 };
    }
    0
}

/// Does `own` keep every key of `layout` at its position, slot, entry and
/// lane, inline, with no integrity level?
///
/// # Safety
/// Both key lists are live; nothing allocates while they are read.
unsafe fn descends_from(
    own: &ShapeDescriptor,
    keys: crate::object::ObjectKeys,
    layout: &ShapeDescriptor,
    layout_keys: crate::object::ObjectKeys,
) -> bool {
    if own.object_kind != layout.object_kind
        || own.integrity_flags != 0
        || layout.integrity_flags != 0
        || own.live_inline_slot_count < layout.live_inline_slot_count
    {
        return false;
    }
    (0..layout.logical_key_count).all(|i| {
        let slot = layout_keys.storage_slot(i);
        keys.get(i).bits() == layout_keys.get(i).bits()
            && keys.storage_slot(i) == slot
            && crate::object::key_attrs::keys_entry(keys.arr(), i)
                == crate::object::key_attrs::keys_entry(layout_keys.arr(), i)
            && slot < layout.live_inline_slot_count
            && own.lane(slot) == layout.lane(slot)
    })
}
