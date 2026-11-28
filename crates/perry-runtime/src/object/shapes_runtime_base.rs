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
//! inline width, the kind and no integrity level. Every record states it
//! from the moment it is published (a two-bit field of its key-prefix facts
//! byte, `ShapeRecord::runtime_base_code`, derived beside the record's other
//! key-prefix facts), so a reader loads one field: no search, no recheck. It
//! is never identity and no table holds it. A deleted base key leaves a hole
//! in its slot, which every reader still treats as absence.

use super::*;

use crate::codegen_abi::LayoutWord;

/// The runtime instance layouts whose fixed slots native readers use, in
/// code order (code = index + 1).
#[inline(always)]
fn bases() -> [u32; 2] {
    use crate::object::runtime_layouts as rl;
    [rl::INCOMING_MESSAGE::WORD, rl::SERVER_RESPONSE::WORD]
}

/// The runtime instance layout (its word) whose fixed slots an object on
/// the unshadowed layout word `word` keeps: `word` itself when it is one,
/// the base its record states, or 0.
#[inline]
pub(crate) fn runtime_base_of_word(word: u32) -> u32 {
    let [incoming, response] = bases();
    if word == incoming || word == response {
        return word;
    }
    // Shadowed words are the caller's to strip.
    if word & crate::object::layout_desc::LAYOUT_SHADOW_BIT != 0 {
        return 0;
    }
    let Some(record) = ShapeSlab::agent_record_for_word_present(LayoutWord(word)) else {
        return 0;
    };
    // SAFETY: a present slab record of this agent.
    match unsafe { (*record).runtime_base_code() } {
        1 => incoming,
        2 => response,
        _ => 0,
    }
}

/// The runtime base code of `record`, published (or republished) under
/// `word`: 0 none, else the index + 1 of the base layout it descends from.
/// One compare per base rejects every record whose first key or key count
/// does not fit, so publication of an unrelated shape reads no key list.
pub(in crate::object) fn runtime_base_code_of(record: &ShapeRecord, word: u32) -> u32 {
    let mut code = 0;
    for base in bases() {
        code += 1;
        if word == base {
            return code;
        }
        let Some(base_record) = ShapeSlab::agent_record_for_word_present(LayoutWord(base)) else {
            continue;
        };
        // SAFETY: a present slab record of this agent; `record` is the
        // caller's copy (never the slab row being written), and both key
        // lists are live while nothing allocates here.
        if unsafe { descends_from(record, &*base_record) } {
            return code;
        }
    }
    0
}

/// Does `own` keep every key of `layout` at its position, slot, entry and
/// lane, inline, with the same kind, both on immutable key prefixes and with
/// no integrity level?
///
/// # Safety
/// Both records' key lists are live; nothing allocates while they are read.
unsafe fn descends_from(own: &ShapeRecord, layout: &ShapeRecord) -> bool {
    let count = layout.logical_key_count;
    if count == 0
        || own.logical_key_count < count
        || own.keys == 0
        || layout.keys == 0
        || own.live_inline_slot_count < layout.live_inline_slot_count
        || !own.same_kind_as(layout)
    {
        return false;
    }
    let keys = crate::object::ObjectKeys::new(
        own.keys as usize as *mut crate::ArrayHeader,
        own.logical_key_count,
    );
    let layout_keys =
        crate::object::ObjectKeys::new(layout.keys as usize as *mut crate::ArrayHeader, count);
    if keys.get(0).bits() != layout_keys.get(0).bits()
        || own.integrity_flags() != 0
        || layout.integrity_flags() != 0
        || !keys_prefix_is_immutable(keys.arr())
        || !keys_prefix_is_immutable(layout_keys.arr())
    {
        return false;
    }
    (0..count).all(|i| {
        let slot = layout_keys.storage_slot(i);
        keys.get(i).bits() == layout_keys.get(i).bits()
            && keys.storage_slot(i) == slot
            && crate::object::key_attrs::keys_entry(keys.arr(), i)
                == crate::object::key_attrs::keys_entry(layout_keys.arr(), i)
            && slot < layout.live_inline_slot_count
            && own.lane(slot) == layout.lane(slot)
    })
}
