//! Charter step 3, accessor stage: an accessor property's getter/setter pair
//! lives in the VALUE SLOT of the key on the object that owns it (V8's
//! `AccessorPair`), not in an address-keyed side table.
//!
//! The key's attribute entry (`key_attrs.rs`) says the key is an accessor;
//! the slot says which functions. So a read that resolves to an accessor on
//! a prototype is: compare the receiver's ShapeId, load the holder's slot,
//! call the getter — the shape proves the entry, the slot names the function.
//!
//! # Representation
//!
//! A pair is a small runtime-internal `GC_TYPE_ARRAY` of [`PAIR_LEN`] words:
//!
//! | word | holds |
//! |---|---|
//! | [`PAIR_GET`] | the getter as a NaN-boxed closure, or `undefined` |
//! | [`PAIR_SET`] | the setter as a NaN-boxed closure, or `undefined` |
//! | [`PAIR_RAW_GET`] | a class getter's compiled entry `fn(this) -> value` (its address bits), or 0 |
//! | [`PAIR_RAW_SET`] | a class setter's compiled entry `fn(this, v) -> value` (its address bits), or 0 |
//!
//! The two closure words are ordinary traced slots. The raw entries are code
//! addresses stored as their plain bits: an address below 2^48 has none of the
//! NaN-box tag bits set, so the collector reads the word as a (subnormal) JS
//! Number and never follows it, and a cache hit uses it without conversion. A class accessor keeps both forms: the closure is what
//! reflection hands out (`getOwnPropertyDescriptor(C.prototype, k).get`), the
//! raw entry is what an inline cache calls with the receiver as `this`.
//!
//! A pair is NEVER a JS value. It is reachable only through a slot whose
//! key's entry carries `ENTRY_ACCESSOR`, and every reader of such a slot goes
//! through [`slot_accessor`]; a raw slot reader that has not asked the key's
//! entry must treat an accessor key's slot as having no data value (see
//! `key_attrs::object_slot_data`).

use crate::array::ArrayHeader;
use crate::value::{POINTER_MASK, POINTER_TAG, TAG_MASK, TAG_UNDEFINED};

/// The getter closure word.
pub(crate) const PAIR_GET: usize = 0;
/// The setter closure word.
pub(crate) const PAIR_SET: usize = 1;
/// The class getter's compiled entry word.
pub(crate) const PAIR_RAW_GET: usize = 2;
/// The class setter's compiled entry word.
pub(crate) const PAIR_RAW_SET: usize = 3;
/// Words in a pair.
pub(crate) const PAIR_LEN: usize = 4;

/// The functions of one accessor property.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub(crate) struct Accessor {
    /// NaN-boxed getter closure bits, 0 when absent.
    pub get: u64,
    /// NaN-boxed setter closure bits, 0 when absent.
    pub set: u64,
    /// A class getter's compiled entry, 0 when absent.
    pub raw_get: usize,
    /// A class setter's compiled entry, 0 when absent.
    pub raw_set: usize,
}

/// Largest code address a raw word can hold: below it no NaN-box tag bit is
/// set, so the word is a Number to the collector.
const RAW_ADDRESS_LIMIT: usize = 1 << 48;

#[inline]
fn raw_word(raw: usize) -> u64 {
    debug_assert!(raw < RAW_ADDRESS_LIMIT);
    if raw < RAW_ADDRESS_LIMIT {
        raw as u64
    } else {
        0
    }
}

#[inline]
fn raw_of(word: u64) -> usize {
    if (word as usize) < RAW_ADDRESS_LIMIT {
        word as usize
    } else {
        0
    }
}

/// The accessor a pair VALUE holds, without re-proving that it is one — for a
/// cache hit whose entry proved it at prime time (the holder's key is an
/// accessor, and a slot of an accessor key is written only by an accessor
/// install, which transitions the holder's ShapeId).
///
/// # Safety
/// `value` is a NaN-boxed pointer to a pair.
#[inline(always)]
pub(crate) unsafe fn pair_of_value_unchecked(value: u64) -> Accessor {
    let w = crate::array::array_elements_ptr((value & POINTER_MASK) as *const ArrayHeader);
    Accessor {
        get: closure_of(*w.add(PAIR_GET)),
        set: closure_of(*w.add(PAIR_SET)),
        raw_get: raw_of(*w.add(PAIR_RAW_GET)),
        raw_set: raw_of(*w.add(PAIR_RAW_SET)),
    }
}

#[inline]
fn closure_word(bits: u64) -> u64 {
    if bits == 0 {
        TAG_UNDEFINED
    } else {
        bits
    }
}

#[inline]
fn closure_of(word: u64) -> u64 {
    if word & TAG_MASK == POINTER_TAG && word & POINTER_MASK != 0 {
        word
    } else {
        0
    }
}

/// Allocate a pair holding `acc`.
///
/// # Safety
/// May collect: the caller roots what it holds. `acc`'s closures are live.
pub(crate) unsafe fn pair_new(acc: Accessor) -> *mut ArrayHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let get = scope.root_nanbox_u64(closure_word(acc.get));
    let set = scope.root_nanbox_u64(closure_word(acc.set));
    let pair = crate::array::js_array_alloc_key_list(PAIR_LEN as u32, false);
    let w = crate::array::array_elements_ptr(pair);
    // GC_STORE_AUDIT(INIT): an unpublished array; its layout is rebuilt from
    // the slots below, after `length` covers them, before anything can read it.
    *w.add(PAIR_GET) = get.get_nanbox_u64();
    *w.add(PAIR_SET) = set.get_nanbox_u64();
    *w.add(PAIR_RAW_GET) = raw_word(acc.raw_get);
    *w.add(PAIR_RAW_SET) = raw_word(acc.raw_set);
    (*pair).length = PAIR_LEN as u32;
    crate::object::gc_slots::rebuild_array_layout_from_slots(pair);
    if crate::arena::pointer_in_old_gen(pair as usize) {
        for i in [PAIR_GET, PAIR_SET] {
            let slot = w.add(i);
            crate::gc::runtime_write_barrier_slot(pair as usize, slot as usize, *slot);
        }
    }
    pair
}

/// The accessor a pair value (a NaN-boxed pointer to a pair) holds, or
/// `None` when `value` is not a pair — an accessor key whose slot was never
/// given one reads as an accessor with neither half.
///
/// # Safety
/// `value` is the slot value of a key whose entry carries `ENTRY_ACCESSOR`.
pub(crate) unsafe fn pair_of_value(value: u64) -> Option<Accessor> {
    if value & TAG_MASK != POINTER_TAG {
        return None;
    }
    let pair = (value & POINTER_MASK) as *const ArrayHeader;
    let header = crate::value::addr_class::try_read_gc_header(pair as usize)?;
    if header.obj_type != crate::gc::GC_TYPE_ARRAY || (*pair).length as usize != PAIR_LEN {
        return None;
    }
    let w = crate::array::array_elements_ptr(pair);
    Some(Accessor {
        get: closure_of(*w.add(PAIR_GET)),
        set: closure_of(*w.add(PAIR_SET)),
        raw_get: raw_of(*w.add(PAIR_RAW_GET)),
        raw_set: raw_of(*w.add(PAIR_RAW_SET)),
    })
}

/// The accessor stored in `obj`'s slot for key position `pos`.
///
/// # Safety
/// `obj` is a live `ObjectHeader` and `pos` a key position whose entry
/// carries `ENTRY_ACCESSOR`.
pub(crate) unsafe fn slot_accessor(obj: *const crate::object::ObjectHeader, pos: u32) -> Accessor {
    let live = crate::object::object_live_slot_count(obj);
    let value = crate::object::field_get_set::object_field_at_with_live(obj, pos, live).bits();
    pair_of_value(value).unwrap_or_default()
}

/// The accessor `obj`'s own key `key` holds, from the key's slot, when its
/// entry says it is one (charter step 3).
///
/// # Safety
/// `obj` is a live object whose attributes live with its keys.
pub(crate) unsafe fn own_accessor(obj: usize, key: &[u8]) -> Option<Accessor> {
    let obj = obj as *const crate::object::ObjectHeader;
    if !crate::object::key_attrs::object_key_is_accessor(obj, key) {
        return None;
    }
    let keys = crate::object::object_keys(obj);
    let pos = crate::object::keys_find_slot_by_bytes(keys.arr(), keys.count(), key)?;
    Some(slot_accessor(obj, pos))
}

/// Store `acc` (or, for `None`, `undefined`) in the value slot of `obj`'s
/// own key `key`, whose entry the funnel has already made say so. Runs in a
/// no-move window: installers hold the receiver as a raw address.
///
/// # Safety
/// `obj` is a live object whose attributes live with its keys, and `key` is
/// one of its own keys.
pub(crate) unsafe fn store_own_accessor(obj: usize, key: &str, acc: Option<Accessor>) {
    let _no_move = crate::gc::GcSuppressScope::new();
    let value = match acc {
        Some(acc) => crate::value::js_nanbox_pointer(pair_new(acc) as i64),
        None => f64::from_bits(crate::value::TAG_UNDEFINED),
    };
    let key_str = crate::string::js_string_from_bytes(key.as_ptr(), key.len() as u32);
    crate::object::object_ops::define_property_force_store_value(
        obj as *mut crate::object::ObjectHeader,
        key_str,
        value,
    );
}

#[inline]
pub(crate) fn pair_from(acc: &crate::object::AccessorDescriptor) -> Accessor {
    Accessor {
        get: acc.get,
        set: acc.set,
        raw_get: 0,
        raw_set: 0,
    }
}

#[inline]
pub(crate) fn descriptor_from(acc: Accessor) -> crate::object::AccessorDescriptor {
    crate::object::AccessorDescriptor {
        get: acc.get,
        set: acc.set,
    }
}

#[cfg(test)]
#[path = "accessor_pair_tests.rs"]
mod accessor_pair_tests;
