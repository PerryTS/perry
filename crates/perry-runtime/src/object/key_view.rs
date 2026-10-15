//! The hole-aware view of an object's own layout keys (object model §3.2:
//! presence is the hole marker in the slot).
//!
//! A layout key whose descriptor entry carries
//! [`ENTRY_OPTIONAL`](super::key_attrs::ENTRY_OPTIONAL) (a family's late or
//! optional slot, a runtime layout's optional member) is ABSENT while its
//! storage slot holds `TAG_HOLE`: not present for `in`, `hasOwn` or
//! `getOwnPropertyDescriptor`, not listed by any enumeration, not readable as
//! a value (a read continues to the prototype). Every reader of presence or
//! order asks this module. A value read that has already loaded the key's
//! slot applies the same rule to the word it holds: `TAG_HOLE` is no value,
//! and the read continues as for an absent key.
//!
//! **Cost.** Whether a layout can have holes is a fact of its immutable key
//! list: the cumulative attribute summary bit
//! [`SUMMARY_HOLES`](super::key_attrs::SUMMARY_HOLES), which the shape record
//! already carries in the byte every attribute-aware reader loads. A layout
//! without it answers "present" for every key with no further load, so it
//! keeps its cost. No cache and no side table: the slot is the authority.
//!
//! **Order.** Present keys enumerate in the layout's key sequence order,
//! holes skipped. A hole filled after birth therefore enumerates at its key
//! position, not in fill order (a later step may append out-of-order fills
//! to the ordered overflow; not decided here).
//!
//! **Overflow.** Once an object has an ordered overflow table (§3.4), the
//! table is the order and presence authority: `overflow::ensure` records a
//! hole slot as a tombstoned entry, so this view answers "present" for such
//! an object and the overflow readers do the rest.
//!
//! **Caches.** A word-keyed presence or absence answer (an IC way, a memo)
//! is not a function of the layout word for a layout that can have holes, so
//! every cache declines such a receiver ([`object_may_have_holes`]).

use super::key_attrs::SUMMARY_HOLES;
use super::{ObjectHeader, ObjectKeys};
use crate::value::TAG_HOLE;
use crate::JSValue;

/// Can a list or shape with attribute summary `summary` have a hole?
#[inline(always)]
pub(crate) const fn summary_may_have_holes(summary: u8) -> bool {
    summary & SUMMARY_HOLES != 0
}

/// Can some own layout key of `obj` be absent because its slot holds the
/// hole? One shape-record load (the summary every attribute-aware reader
/// already reads); false for every layout without optional keys and for an
/// object whose overflow is the authority.
///
/// # Safety
/// `obj` is null or a live object header.
#[inline]
pub(crate) unsafe fn object_may_have_holes(obj: *const ObjectHeader) -> bool {
    // Only an ordinary object holds a ShapeId at +4 (shape rule 3), so the
    // record lookup is the classification.
    if obj.is_null() || !crate::value::addr_class::is_plausible_heap_addr(obj as usize) {
        return false;
    }
    let Some(record) = super::shapes::object_shape_record(obj) else {
        return false;
    };
    summary_may_have_holes(record.summary()) && !super::overflow::has_overflow(obj)
}

/// Can a position of the key list `keys` be a hole? The list's own summary
/// answers (one header load for a list without attributes, which is every
/// list without optional keys). An ordered overflow table is never a hole
/// view: it lists a hole slot as a tombstoned entry.
///
/// # Safety
/// `keys` is null or a live key list (an object's own list or its table).
#[inline]
pub(crate) unsafe fn keys_may_have_holes(keys: ObjectKeys) -> bool {
    !keys.is_null()
        && !super::overflow::is_table(keys.arr())
        && summary_may_have_holes(super::key_attrs::keys_summary(keys.arr(), keys.count()))
}

/// The word in storage slot `slot` of `obj`, or `None` when the object has no
/// storage for it. A layout's slots are physical inline slots (a declared
/// layout's birth allocates them all, holding the hole), whatever the shape's
/// live bound says; past the inline storage a slot lives in the spill.
#[inline]
unsafe fn slot_word(obj: *const ObjectHeader, slot: u32) -> Option<u64> {
    if slot < super::object_inline_capacity(obj) {
        let fields = (obj as *const u8).add(std::mem::size_of::<ObjectHeader>()) as *const u64;
        return Some(*fields.add(slot as usize));
    }
    super::spill::spill_get_present(obj as usize, slot as usize)
}

/// One object's own layout keys, read through the hole rule.
#[derive(Clone, Copy)]
pub(crate) struct KeyView {
    obj: *const ObjectHeader,
    keys: ObjectKeys,
    holes: bool,
}

impl KeyView {
    /// The view of `obj`'s own keys. For a layout without holes (or an object
    /// whose overflow is the authority) every position is present.
    ///
    /// # Safety
    /// `obj` is a live object header; nothing collects while the view is used.
    #[inline]
    pub(crate) unsafe fn of(obj: *const ObjectHeader) -> KeyView {
        Self::with_keys(obj, super::object_keys(obj))
    }

    /// The view of `keys`, the object's own list as the caller already read it.
    ///
    /// # Safety
    /// As [`Self::of`]; `keys` is `obj`'s current list.
    #[inline]
    pub(crate) unsafe fn with_keys(obj: *const ObjectHeader, keys: ObjectKeys) -> KeyView {
        KeyView {
            obj,
            keys,
            holes: keys_may_have_holes(keys),
        }
    }

    /// Is key position `position` present (its slot does not hold the hole)?
    ///
    /// # Safety
    /// The object is live.
    #[inline(always)]
    pub(crate) unsafe fn present(&self, position: u32) -> bool {
        !self.holes || !self.is_hole(position)
    }

    #[inline(never)]
    unsafe fn is_hole(&self, position: u32) -> bool {
        if position >= self.keys.count() {
            return false;
        }
        slot_word(self.obj, self.keys.storage_slot(position)) == Some(TAG_HOLE)
    }
}

/// Is position `position` of `obj`'s own key list `keys` a hole? False for a
/// layout without holes, from the list's summary alone.
///
/// # Safety
/// `obj` is live and `keys` is its current own list.
#[inline]
pub(crate) unsafe fn position_is_hole(
    obj: *const ObjectHeader,
    keys: ObjectKeys,
    position: u32,
) -> bool {
    !KeyView::with_keys(obj, keys).present(position)
}

/// Is `obj`'s own property spelled `bytes` a layout key whose slot holds the
/// hole, so absent?
///
/// # Safety
/// `obj` is null or a live object header.
#[inline]
pub(crate) unsafe fn own_key_bytes_is_hole(obj: *const ObjectHeader, bytes: &[u8]) -> bool {
    if !object_may_have_holes(obj) {
        return false;
    }
    own_key_bytes_is_hole_slow(obj, bytes)
}

#[inline(never)]
unsafe fn own_key_bytes_is_hole_slow(obj: *const ObjectHeader, bytes: &[u8]) -> bool {
    let view = KeyView::of(obj);
    if view.keys.is_null() {
        return false;
    }
    match super::keys_find_property_slot_by_bytes(view.keys.arr(), view.keys.count(), bytes) {
        Some(position) => !view.present(position),
        None => false,
    }
}

/// [`own_key_bytes_is_hole`] for a key value (a string or a symbol).
///
/// # Safety
/// As [`own_key_bytes_is_hole`].
#[inline]
pub(crate) unsafe fn own_key_is_hole(obj: *const ObjectHeader, key: JSValue) -> bool {
    if !object_may_have_holes(obj) {
        return false;
    }
    own_key_is_hole_slow(obj, key)
}

#[inline(never)]
unsafe fn own_key_is_hole_slow(obj: *const ObjectHeader, key: JSValue) -> bool {
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    if let Some(bytes) = crate::string::js_string_key_bytes(key, &mut sso) {
        return own_key_bytes_is_hole_slow(obj, bytes);
    }
    // A symbol key: compare the key word.
    let view = KeyView::of(obj);
    (0..view.keys.count())
        .find(|&position| view.keys.get(position).bits() == key.bits())
        .is_some_and(|position| !view.present(position))
}
