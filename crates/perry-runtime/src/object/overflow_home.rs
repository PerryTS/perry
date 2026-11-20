//! The home of an overflow `Table` (lane DYNP FR2).
//!
//! A Table hangs off an `ObjectMeta`: its `overflow` word. The operations
//! below take the home's GC owner (an address) and a home kind `META` that says
//! what the owner contributes: where its Table is found, its key-add and
//! own-keys funnels, and the fixed slots that slot-reference entries name.
//! The `ObjectHeader` entry points of `overflow.rs` are thin wrappers over
//! [`ObjHome`] (the owner keeps the shadow bit in its +4 word and its inline
//! slots back the fixed prefix). A [`MetaHome`] is a meta record that no
//! `ObjectHeader` carries: the record is its own GC owner, it is found by
//! `meta.overflow` alone (no +4 word is read or written), and its Table has
//! no fixed prefix, so it holds no slot-reference entry by construction.
//! One Table kind, one implementation per operation.
use super::*;
use crate::object::{ObjectHeader, ObjectMeta};

/// A Table's home: `META` false is an ordinary object ([`ObjHome`]), true a
/// meta record no `ObjectHeader` carries ([`MetaHome`]). The home is a
/// compile-time constant: no owner kind is tested at run time.
pub(crate) enum Home<const META: bool> {}

impl<const META: bool> Home<META> {
    /// The Table, or null.
    #[inline(always)]
    pub(crate) unsafe fn table(owner: usize) -> *mut Table {
        if META {
            (*(owner as *const ObjectMeta)).overflow as usize as *mut Table
        } else {
            super::table(owner as *const ObjectHeader)
        }
    }

    /// Before a first Table: afterwards the owner has its meta. May allocate;
    /// the caller roots `owner`.
    #[inline(always)]
    pub(crate) unsafe fn prepare(owner: usize) {
        if META {
        } else {
            let obj = owner as *mut ObjectHeader;
            crate::object::layout_desc::diag_note_install(obj);
            // A1_UNPROVEN: a user object whose keys overflow, no type test (kind-A/B question for S0b-S0e).
            crate::object::object_meta_ensure(
                crate::meta_obj_runtime_born!(obj),
                crate::object::MetaReason::Overflow,
            );
        }
    }

    /// The keyless slot-reference prefix a first empty Table starts with.
    #[inline(always)]
    pub(crate) fn empty_prefix(live: u32) -> u32 {
        if META {
            0
        } else {
            live.max(crate::object::INLINE_SLOT_FLOOR as u32)
        }
    }

    /// The Table, created (and installed) when the owner has none.
    #[inline(always)]
    pub(crate) unsafe fn ensure(owner: usize) -> *mut Table {
        if META {
            MetaHome::ensure_meta(owner)
        } else {
            super::ensure(owner as *mut ObjectHeader)
        }
    }

    /// Hang `t` off the owner's meta.
    #[inline(always)]
    pub(crate) unsafe fn install(owner: usize, t: *mut Table) {
        if META {
            install_in::<true>(owner, owner as *mut ObjectMeta, t)
        } else {
            super::install(owner as *mut ObjectHeader, t)
        }
    }

    /// Room for one more entry: the call boundary insert crosses (ObjHome: the
    /// object entry point `reserve`, so its inlining is that function's own).
    #[inline(always)]
    pub(crate) unsafe fn reserve(owner: usize) -> *mut Table {
        if META {
            MetaHome::reserve_meta(owner)
        } else {
            super::reserve(owner as *mut ObjectHeader)
        }
    }

    /// Entry attributes, the call boundary insert crosses (ObjHome: `set_entry_attrs`).
    #[inline(always)]
    pub(crate) unsafe fn set_entry_attrs(owner: usize, t: *mut Table, i: u32, attrs: u8) {
        if META {
            MetaHome::set_entry_attrs_meta(owner, t, i, attrs)
        } else {
            super::set_entry_attrs(owner as *mut ObjectHeader, t, i, attrs)
        }
    }

    /// Mark the owner as having a Table.
    #[inline(always)]
    pub(crate) unsafe fn note_installed(owner: usize) {
        if META {
        } else {
            (*(owner as *mut ObjectHeader)).parent_class_id |= SHADOW_BIT;
        }
    }

    /// DEC 250 key-add funnel (an add-locked intrinsic refuses the add).
    #[inline(always)]
    pub(crate) unsafe fn note_add(owner: usize, key: u64) {
        if META { // A bare meta record is no intrinsic and no prototype: nothing reports.
        } else {
            crate::object::builtin_proto::note_write(
                owner as *mut ObjectHeader,
                crate::object::builtin_proto::Mutation::Add(
                    crate::object::builtin_proto::Key::Value(f64::from_bits(key)),
                ),
                "overflow key add",
            );
        }
    }

    /// The owner's own key set changed.
    #[inline(always)]
    pub(crate) unsafe fn note_keys_changed(owner: usize) {
        if META {
        } else {
            crate::object::proto_validity::note_object_own_keys_changed(owner);
        }
    }

    /// Store through slot-reference entry `slot`.
    #[inline(always)]
    pub(crate) unsafe fn store_slot_ref(owner: usize, slot: u32, value: u64) {
        if META {
            MetaHome::store_slot_ref_meta(owner, slot, value)
        } else {
            store_index(owner as *mut ObjectHeader, slot, value);
        }
    }

    /// Fixed slot `fixed` gets the hole.
    /// Store `value` in fixed storage slot `fixed` (the hole retires it).
    #[inline(always)]
    pub(crate) unsafe fn write_fixed(owner: usize, fixed: u32, value: u64) {
        if META {
            MetaHome::write_fixed_meta(owner, fixed, value)
        } else {
            let obj = owner as *mut ObjectHeader;
            if fixed < crate::object::object_live_slot_count(obj) {
                crate::object::store_object_field_slot(obj, fixed as usize, value);
            } else {
                crate::object::spill::spill_set(obj as usize, fixed as usize, value);
            }
        }
    }

    /// The storage slot of layout key `atom` (a property atom), if the
    /// owner's layout names it, with the live inline bound of the same
    /// layout record (the `live` that [`Home::read_fixed`] takes).
    #[inline(always)]
    pub(crate) unsafe fn layout_slot(
        owner: usize,
        atom: *const crate::StringHeader,
    ) -> Option<(u32, u32)> {
        if META {
            // A bare meta record has no layout: every key is a Table entry.

            None
        } else {
            let record =
                crate::object::shapes::object_shape_record(owner as *const ObjectHeader).unwrap();
            let position = crate::object::keys_find_property_slot_by_key_ptr(
                record.keys_view(),
                record.logical_key_count(),
                atom,
            )?;
            Some((
                crate::object::key_attrs::key_storage_slot(
                    record.keys() as usize as *const crate::array::ArrayHeader,
                    position,
                ),
                record.live_inline_slot_count(),
            ))
        }
    }

    /// A write through a dynamic key published: the owner's stamp note.
    #[inline(always)]
    pub(crate) unsafe fn note_stamped(owner: usize) {
        if META {
        } else {
            crate::object::proto_validity::note_object_shape_stamped(
                owner,
                0,
                (*(owner as *const ObjectHeader)).parent_class_id,
            );
        }
    }

    /// Fixed slot `fixed`'s value; the slot gets the hole.
    #[inline(always)]
    pub(crate) unsafe fn take_fixed(owner: usize, fixed: u32) -> u64 {
        if META {
            MetaHome::take_fixed_meta(owner, fixed)
        } else {
            let obj = owner as *mut ObjectHeader;
            if fixed < crate::object::object_live_slot_count(obj) {
                let v = crate::object::field_get_set::object_field_at_with_live(
                    obj,
                    fixed,
                    crate::object::object_live_slot_count(obj),
                )
                .bits();
                crate::object::store_object_field_slot(obj, fixed as usize, TAG_HOLE);
                v
            } else {
                let v = crate::object::spill::spill_get_present(obj as usize, fixed as usize)
                    .unwrap_or(TAG_HOLE);
                crate::object::spill::spill_set(obj as usize, fixed as usize, TAG_HOLE);
                v
            }
        }
    }

    /// The value in fixed storage slot `fixed`, given the owner's live inline
    /// bound `live` (`None` for an absent spill slot).
    #[inline(always)]
    pub(crate) unsafe fn read_fixed(owner: usize, fixed: u32, live: u32) -> Option<u64> {
        if META {
            MetaHome::read_fixed_meta(owner, fixed, live)
        } else {
            let obj = owner as *const ObjectHeader;
            if fixed < live {
                return Some(
                    crate::object::field_get_set::object_field_at_with_live(obj, fixed, live)
                        .bits(),
                );
            }
            crate::object::spill::spill_get_present(obj as usize, fixed as usize)
        }
    }

    /// The owner's live inline slot bound (the accessor read's `live`).
    #[inline(always)]
    pub(crate) unsafe fn live_bound(owner: usize) -> u32 {
        if META {
            0
        } else {
            crate::object::object_live_slot_count(owner as *const ObjectHeader)
        }
    }

    /// Does an own read refuse `value` (the hole, or what only the complete
    /// read answers)?
    #[inline(always)]
    pub(crate) unsafe fn own_read_refused(owner: usize, value: u64) -> bool {
        if META {
            value == TAG_HOLE
        } else {
            let value = crate::JSValue::from_bits(value);
            value.bits() == TAG_HOLE
                || (value.is_undefined()
                    && crate::object::native_this_alias::object_alias(owner as *const ObjectHeader)
                        .is_some())
        }
    }

    /// Op hooks: an `_in` body reaches another op through these, so each
    /// home keeps its own call boundary for that op.
    /// [`get_from_in`] over this home.
    #[inline(always)]
    pub(crate) unsafe fn get_from(t: *const Table, owner: usize, slot: u32) -> Option<u64> {
        if META {
            get_from_in::<true>(t, owner, slot)
        } else {
            super::get_from(t, owner as *const ObjectHeader, slot)
        }
    }

    /// [`own_get_entry_in`] over this home.
    #[inline(always)]
    pub(crate) unsafe fn own_get_entry(
        owner: usize,
        t: *mut Table,
        key: u64,
    ) -> Option<crate::JSValue> {
        if META {
            own_get_entry_in::<true>(owner, t, key)
        } else {
            super::own_get_entry(owner as *const ObjectHeader, t, key)
        }
    }

    /// [`store_index_in`] over this home.
    #[inline(always)]
    pub(crate) unsafe fn store_index(owner: usize, slot: u32, value: u64) -> bool {
        if META {
            store_index_in::<true>(owner, slot, value)
        } else {
            super::store_index(owner as *mut ObjectHeader, slot, value)
        }
    }

    /// [`insert_key_bits_in`] over this home.
    #[inline(always)]
    pub(crate) unsafe fn insert_key(owner: usize, key: u64, value: u64, attrs: u8) -> u32 {
        if META {
            insert_key_bits_in::<true>(owner, key, value, attrs)
        } else {
            super::insert_key_bits(owner as *mut ObjectHeader, key, value, attrs)
        }
    }

    /// What a delete of `key` does while the owner has no Table: `Some`
    /// answers it, `None` continues to the Table.
    #[inline(always)]
    pub(crate) unsafe fn delete_untabled(owner: usize, key: &[u8]) -> Option<bool> {
        if META {
            // No layout key exists, so an owner without a Table has no key to delete.
            Some(true)
        } else {
            super::delete_untabled(owner as *mut ObjectHeader, key)
        }
    }

    /// [`delete_present_key_bits_in`] over this home.
    #[inline(always)]
    pub(crate) unsafe fn delete_present_key(owner: usize, key: u64) -> Option<bool> {
        if META {
            delete_present_key_bits_in::<true>(owner, key)
        } else {
            super::delete_present_key_bits(owner as *mut ObjectHeader, key)
        }
    }

    /// A delete of `key` that names no entry: an ordinary owner's elements
    /// (an Array-subclass index, an arguments element) answer it where they
    /// hold it; a metadata home has no elements.
    #[inline(always)]
    pub(crate) unsafe fn delete_miss(owner: usize, key: &[u8]) -> bool {
        if META {
            true
        } else {
            super::delete_elements_miss(owner as *mut ObjectHeader, key).unwrap_or(true)
        }
    }
}

/// An ordinary object: the Table hangs off `(*obj).meta`.
pub(crate) type ObjHome = Home<false>;

/// A meta record no `ObjectHeader` carries; the record is the GC owner.
pub(crate) type MetaHome = Home<true>;

impl MetaHome {
    unsafe fn ensure_meta(owner: usize) -> *mut Table {
        let existing = MetaHome::table(owner);
        if !existing.is_null() {
            return existing;
        }
        let scope = crate::gc::RuntimeHandleScope::new();
        let home = scope.root_raw_mut_ptr(owner as *mut u8);
        let t = start_in::<true>(owner, 0);
        MetaHome::install(home.get_raw_mut_ptr::<u8>() as usize, t);
        t
    }

    unsafe fn reserve_meta(owner: usize) -> *mut Table {
        reserve_in::<true>(owner)
    }

    unsafe fn set_entry_attrs_meta(owner: usize, t: *mut Table, i: u32, attrs: u8) {
        set_entry_attrs_in::<true>(owner, t, i, attrs)
    }

    unsafe fn store_slot_ref_meta(_owner: usize, _slot: u32, _value: u64) {
        unreachable!("a meta-home Table has no fixed prefix")
    }

    unsafe fn write_fixed_meta(_owner: usize, _fixed: u32, _value: u64) {
        unreachable!("a meta-home Table has no fixed prefix")
    }

    unsafe fn take_fixed_meta(_owner: usize, _fixed: u32) -> u64 {
        unreachable!("a meta-home Table has no fixed prefix")
    }

    unsafe fn read_fixed_meta(_owner: usize, _fixed: u32, _live: u32) -> Option<u64> {
        unreachable!("a meta-home Table has no fixed prefix")
    }
}

/// A meta record born on its own (every word 0, the state `object_meta_ensure`
/// gives a fresh record), for a [`MetaHome`].
#[cfg(test)]
pub(crate) unsafe fn meta_home_new() -> *mut ObjectMeta {
    let meta = crate::arena::arena_alloc_gc(
        std::mem::size_of::<ObjectMeta>(),
        8,
        crate::gc::GC_TYPE_OBJECT_META,
    ) as *mut ObjectMeta;
    std::ptr::write_bytes(meta, 0, 1);
    meta
}

#[inline(always)]
pub(crate) unsafe fn install_in<const META: bool>(
    owner: usize,
    meta: *mut ObjectMeta,
    t: *mut Table,
) {
    (*meta).overflow = t as usize as u64;
    // The slot owner is the live metadata record, whose generation can
    // differ from the receiver's after a collection. Its header decides
    // remembering; an active incremental mark still takes the full barrier.
    crate::gc::runtime_write_barrier_resolved_slot(
        meta as usize,
        &(*meta).overflow as *const _ as usize,
        t as usize as u64,
    );
    Home::<META>::note_installed(owner);
    Home::<META>::note_keys_changed(owner);
    crate::snap_census::attached(owner, &[meta as usize, t as usize]);
}

/// The first Table of an owner whose layout names no key (`live` live inline
/// slots): the keyless prefix `Home::<META>::empty_prefix(live)` and room for the
/// caller's append. The caller roots the owner and installs the Table before
/// its append. An existing Table is returned as is.
#[inline(always)]
pub(crate) unsafe fn start_in<const META: bool>(owner: usize, live: u32) -> *mut Table {
    let existing = Home::<META>::table(owner);
    if !existing.is_null() {
        return existing;
    }
    Home::<META>::prepare(owner);
    let prefix = Home::<META>::empty_prefix(live);
    let t = allocate((prefix + 4).next_power_of_two().max(4));
    for i in 0..prefix {
        // GC_STORE_AUDIT(POINTER_FREE): keyless prefix entry: the hole, an internal slot word, a scalar attrs word.
        *key_at(t, i) = TAG_HOLE;
        *value_at(t, i) = crate::value::INTERNAL_WORD_TAG | i as u64;
        *attrs_at(t, i) = SLOT_REF;
    }
    (*t).length = prefix;
    (*t).fixed_prefix = prefix;
    t
}

/// The Table with room for one append: compacted when tombstones dominate,
/// doubled when full.
#[inline(always)]
pub(crate) unsafe fn reserve_in<const META: bool>(owner: usize) -> *mut Table {
    let t = Home::<META>::ensure(owner);
    if (*t).deleted > 8 && (*t).deleted * 3 > (*t).length {
        let mut out = (*t).fixed_prefix;
        for i in (*t).fixed_prefix..(*t).length {
            if *key_at(t, i) != TAG_HOLE {
                *key_at(t, out) = *key_at(t, i);
                *value_at(t, out) = *value_at(t, i);
                *attrs_at(t, out) = *attrs_at(t, i);
                out += 1;
            }
        }
        (*t).length = out;
        (*t).deleted = 0;
        for i in 0..2 * (*t).capacity as usize {
            // GC_STORE_AUDIT(POINTER_FREE): Compaction clears scalar u32 hash-index column to EMPTY; the separate managed key/value columns receive their own visit_slots barrier.
            index(t).add(i).write(EMPTY);
        }
        for i in 0..out {
            place(t, i);
        }
        visit_slots(t, |slot| {
            crate::gc::runtime_write_barrier_slot(t as usize, slot as usize, *slot)
        });
    }
    if (*t).length < (*t).capacity {
        return t;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let home = scope.root_raw_mut_ptr(owner as *mut u8);
    let old = scope.root_raw_mut_ptr(t);
    let next = allocate((*t).capacity * 2);
    let t = old.get_raw_mut_ptr::<Table>();
    let n = (*t).length as usize;
    std::ptr::copy_nonoverlapping(key_at(t, 0), key_at(next, 0), n);
    std::ptr::copy_nonoverlapping(value_at(t, 0), value_at(next, 0), n);
    std::ptr::copy_nonoverlapping(attrs_at(t, 0), attrs_at(next, 0), n);
    (*next).length = (*t).length;
    (*next).fixed_prefix = (*t).fixed_prefix;
    (*next).deleted = (*t).deleted;
    (*next).summary = (*t).summary;
    for i in 0..(*next).length {
        place(next, i);
    }
    visit_slots(next, |slot| {
        crate::gc::runtime_write_barrier_slot(next as usize, slot as usize, *slot)
    });
    Home::<META>::install(home.get_raw_mut_ptr::<u8>() as usize, next);
    next
}

#[inline(always)]
pub(crate) unsafe fn insert_key_bits_in<const META: bool>(
    owner: usize,
    key: u64,
    value: u64,
    attrs: u8,
) -> u32 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let home = scope.root_raw_mut_ptr(owner as *mut u8);
    let val = scope.root_nanbox_f64(f64::from_bits(value));
    let key = scope.root_nanbox_u64(key);
    let t = Home::<META>::reserve(owner);
    // The general insert marks the table before it touches an entry: any key
    // may arrive here (`key_attrs::TABLE_KEY_HIDDEN`, no per-key decode), and
    // the entry's attributes. Both are "may" bits, never cleared, so a hit or
    // an add that `note_write` refuses only over-approximates. One write
    // here, none on the add path after `note_write`.
    (*t).summary |= crate::object::key_attrs::table_entry_summary_with(
        attrs,
        crate::object::key_attrs::TABLE_KEY_HIDDEN,
    );
    let key = key.get_nanbox_u64();
    let hit = if crate::object::key_attrs::entry_is_private(attrs) {
        (0..(*t).length).find(|&i| {
            *key_at(t, i) == key
                && crate::object::key_attrs::entry_is_private(*attrs_at(t, i) as u8)
        })
    } else {
        find(t, key)
    };
    let slot = hit.unwrap_or((*t).length);
    if slot == (*t).length {
        // DEC 250: an appended entry is a key add (also the re-add of a
        // deleted layout key, whose fixed slot this entry re-attaches),
        // reported while nothing has changed; an add-locked intrinsic refuses it.
        Home::<META>::note_add(home.get_raw_mut_ptr::<u8>() as usize, key);
        let t = Home::<META>::table(home.get_raw_mut_ptr::<u8>() as usize);
        *key_at(t, slot) = key;
        *value_at(t, slot) = val.get_nanbox_f64().to_bits();
        *attrs_at(t, slot) = attrs as u64;
        (*t).length += 1;
        place(t, slot);
        Home::<META>::note_keys_changed(home.get_raw_mut_ptr::<u8>() as usize);
    } else if *attrs_at(t, slot) & SLOT_REF != 0 {
        Home::<META>::store_slot_ref(
            home.get_raw_mut_ptr::<u8>() as usize,
            slot,
            val.get_nanbox_f64().to_bits(),
        );
        Home::<META>::set_entry_attrs(home.get_raw_mut_ptr::<u8>() as usize, t, slot, attrs);
    } else {
        *value_at(t, slot) = val.get_nanbox_f64().to_bits();
        *attrs_at(t, slot) = attrs as u64;
    }
    crate::gc::runtime_write_barrier_resolved_slot(t as usize, key_at(t, slot) as usize, key);
    if *attrs_at(t, slot) & SLOT_REF == 0 {
        crate::gc::runtime_write_barrier_resolved_slot(
            t as usize,
            value_at(t, slot) as usize,
            *value_at(t, slot),
        );
    }
    slot
}

#[inline(always)]
pub(crate) unsafe fn delete_key_bits_in<const META: bool>(owner: usize, key: u64) -> bool {
    delete_present_key_bits_in::<META>(owner, key).unwrap_or(true)
}

/// [`delete_key_bits_in`] that tells a miss (`None`: no entry keyed `key`)
/// from a delete of a present entry.
#[inline(always)]
pub(crate) unsafe fn delete_present_key_bits_in<const META: bool>(
    owner: usize,
    key: u64,
) -> Option<bool> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let home = scope.root_raw_mut_ptr(owner as *mut u8);
    let key = scope.root_nanbox_u64(key);
    let t = Home::<META>::ensure(owner);
    let slot = find(t, key.get_nanbox_u64())?;
    if *attrs_at(t, slot) as u8 & crate::object::key_attrs::ENTRY_NON_CONFIGURABLE != 0 {
        return Some(false);
    }
    if *attrs_at(t, slot) & SLOT_REF != 0 {
        Home::<META>::write_fixed(
            home.get_raw_mut_ptr::<u8>() as usize,
            *value_at(t, slot) as u32,
            TAG_HOLE,
        );
    }
    tombstone(
        Home::<META>::table(home.get_raw_mut_ptr::<u8>() as usize),
        slot,
    );
    Home::<META>::note_keys_changed(home.get_raw_mut_ptr::<u8>() as usize);
    Some(true)
}

/// Delete by key bytes. A spelling that was never interned names no entry;
/// an owner with no Table answers through [`Home::delete_untabled`].
#[inline(always)]
pub(crate) unsafe fn delete_in<const META: bool>(owner: usize, key: &[u8]) -> bool {
    let hash = crate::object::key_bytes_hash(key.as_ptr(), key.len());
    if Home::<META>::table(owner).is_null() {
        if let Some(r) = Home::<META>::delete_untabled(owner, key) {
            return r;
        }
    }
    // A key that was never interned names no entry and no fixed slot.
    let Some(atom) = crate::string::atom_lookup(key, hash) else {
        return Home::<META>::delete_miss(owner, key);
    };
    match Home::<META>::delete_present_key(owner, crate::JSValue::string_ptr(atom as *mut _).bits())
    {
        Some(result) => result,
        None => Home::<META>::delete_miss(owner, key),
    }
}

/// Record entry `i`'s key attributes. An accessor never stays a fixed-slot
/// reference: its pair moves into the entry and the fixed slot gets the hole.
/// Never allocates.
#[inline(always)]
pub(crate) unsafe fn set_entry_attrs_in<const META: bool>(
    owner: usize,
    t: *mut Table,
    i: u32,
    attrs: u8,
) {
    let word = attrs_at(t, i);
    *word = (*word & !0xff) | attrs as u64;
    (*t).summary |= crate::object::key_attrs::table_entry_summary(attrs);
    if *word & SLOT_REF != 0 && attrs & crate::object::key_attrs::ENTRY_ACCESSOR != 0 {
        let value = Home::<META>::take_fixed(owner, *value_at(t, i) as u32);
        *word &= !SLOT_REF;
        *value_at(t, i) = value;
        crate::gc::runtime_write_barrier_slot(t as usize, value_at(t, i) as usize, value);
    }
}

#[cfg(test)]
#[path = "overflow_home_tests.rs"]
mod tests;

/// The value of entry `slot` of `t` (found, not a tombstone): a slot
/// reference reads the owner's fixed slot under the bound `inline_bound`
/// gives, resolved only for a reference.
#[inline(always)]
pub(crate) unsafe fn read_found_in<const META: bool>(
    t: *const Table,
    owner: usize,
    slot: u32,
    inline_bound: impl FnOnce() -> u32,
) -> Option<u64> {
    if *attrs_at(t, slot) & SLOT_REF != 0 {
        let fixed = *value_at(t, slot) as u32;
        return Home::<META>::read_fixed(owner, fixed, inline_bound());
    }
    Some(*value_at(t, slot))
}

/// The value of entry `slot` of the owner's Table `t`; `None` past the end
/// and for a tombstone.
#[inline(always)]
pub(crate) unsafe fn get_from_in<const META: bool>(
    t: *const Table,
    owner: usize,
    slot: u32,
) -> Option<u64> {
    if slot >= (*t).length {
        return None;
    }
    if *key_at(t, slot) == TAG_HOLE {
        return None;
    }
    read_found_in::<META>(t, owner, slot, || Home::<META>::live_bound(owner))
}

/// An own read of `key` (an overflow key) in the owner's Table `t`: a getter
/// runs with the owner as receiver. `None` for no own key and for a value the
/// owner refuses ([`Home::own_read_refused`]).
#[inline(always)]
pub(crate) unsafe fn own_get_entry_in<const META: bool>(
    owner: usize,
    t: *mut Table,
    key: u64,
) -> Option<crate::JSValue> {
    let i = find(t, key)?;
    let live = Home::<META>::live_bound(owner);
    // A getter with an overridden receiver (Reflect.get, super) takes the
    // complete read, which consumes the override.
    if crate::object::key_attrs::key_is_accessor_at(t.cast(), i)
        && crate::object::field_get_set::accessor_receiver_override_armed()
    {
        return None;
    }
    if let Some(value) = crate::object::field_get_set::object_accessor_at_with_live(
        owner as *const ObjectHeader,
        t.cast(),
        i,
        live,
    ) {
        return Some(value);
    }
    let value = Home::<META>::get_from(t, owner, i)?;
    if Home::<META>::own_read_refused(owner, value) {
        return None;
    }
    Some(crate::JSValue::from_bits(value))
}

/// [`own_get_entry_in`] for a key given as a value (a string or a symbol).
#[inline(always)]
pub(crate) unsafe fn own_get_key_in<const META: bool>(
    owner: usize,
    key_bits: u64,
) -> Option<crate::JSValue> {
    let t = Home::<META>::table(owner);
    if t.is_null() {
        return None;
    }
    Home::<META>::own_get_entry(owner, t, overflow_key(key_bits)?)
}

/// A positional store into the owner's Table: a slot reference writes the
/// owner's fixed slot, an owned value the entry. `false` past the end.
#[inline(always)]
pub(crate) unsafe fn store_index_in<const META: bool>(owner: usize, slot: u32, value: u64) -> bool {
    let t = Home::<META>::table(owner);
    if t.is_null() || slot >= (*t).length {
        return false;
    }
    if *attrs_at(t, slot) & SLOT_REF != 0 {
        Home::<META>::write_fixed(owner, *value_at(t, slot) as u32, value);
    } else {
        *value_at(t, slot) = value;
        crate::gc::runtime_write_barrier_slot(t as usize, value_at(t, slot) as usize, value);
    }
    true
}

/// A write of a key the owner's layout does not hold: the existing entry, or
/// one append. `atom` is a property atom.
#[inline(always)]
pub(crate) unsafe fn store_unfixed_in<const META: bool>(
    owner: usize,
    atom: *const crate::StringHeader,
    value: u64,
) {
    let key = crate::JSValue::string_ptr(atom as *mut _).bits();
    let t = Home::<META>::table(owner);
    match if t.is_null() { None } else { find(t, key) } {
        Some(slot) => {
            Home::<META>::store_index(owner, slot, value);
        }
        None => {
            Home::<META>::insert_key(owner, key, value, 0);
        }
    }
}

/// The shared dynamic writer over a home: the layout is probed first
/// ([`Home::layout_slot`]); only an unmatched name becomes a Table value.
#[inline(always)]
pub(crate) unsafe fn store_routed_in<const META: bool>(
    owner: usize,
    key: *const crate::StringHeader,
    value: f64,
) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let home = scope.root_raw_mut_ptr(owner as *mut u8);
    let val = scope.root_nanbox_f64(value);
    let atom = crate::string::property_atom(key);
    let owner = home.get_raw_mut_ptr::<u8>() as usize;
    // A fixed-slot hit changes no key the layout does not already name; only
    // an overflow entry shadows (`install` sets the bit).
    if let Some((fixed, live)) = Home::<META>::layout_slot(owner, atom) {
        let old = Home::<META>::read_fixed(owner, fixed, live).unwrap_or(TAG_HOLE);
        let t = Home::<META>::table(owner);
        let detached = if old == TAG_HOLE && !t.is_null() {
            find(t, crate::JSValue::string_ptr(atom as *mut _).bits())
                .filter(|&i| *attrs_at(t, i) & SLOT_REF == 0)
        } else {
            None
        };
        if let Some(i) = detached {
            // The key's entry owns its value (`set_entry_attrs`).
            Home::<META>::store_index(owner, i, val.get_nanbox_f64().to_bits());
        } else {
            if old == TAG_HOLE {
                let slot = Home::<META>::insert_key(
                    owner,
                    crate::JSValue::string_ptr(atom as *mut _).bits(),
                    crate::value::TAG_UNDEFINED,
                    0,
                );
                let t = Home::<META>::table(home.get_raw_mut_ptr::<u8>() as usize);
                *value_at(t, slot) = crate::value::INTERNAL_WORD_TAG | fixed as u64;
                *attrs_at(t, slot) |= SLOT_REF;
            }
            Home::<META>::write_fixed(
                home.get_raw_mut_ptr::<u8>() as usize,
                fixed,
                val.get_nanbox_f64().to_bits(),
            );
        }
    } else {
        store_unfixed_in::<META>(owner, atom, val.get_nanbox_f64().to_bits());
    }
    Home::<META>::note_stamped(home.get_raw_mut_ptr::<u8>() as usize);
}

/// [`store_routed_in`] for a write whose caller resolved `atom` (a property
/// atom) and proved the layout does not hold it.
#[inline(always)]
pub(crate) unsafe fn store_unfixed_routed_in<const META: bool>(
    owner: usize,
    atom: *const crate::StringHeader,
    value: f64,
) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let home = scope.root_raw_mut_ptr(owner as *mut u8);
    store_unfixed_in::<META>(owner, atom, value.to_bits());
    Home::<META>::note_stamped(home.get_raw_mut_ptr::<u8>() as usize);
}
