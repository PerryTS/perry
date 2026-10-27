use super::*;

/// The word in storage slot `slot` of `obj` (a declared layout's births
/// allocate its slots inline; a slot past the inline storage is the spill's),
/// or `0` (not the hole) when the object has no storage for it.
#[inline(always)]
pub(super) unsafe fn family_slot_word(obj: *const ObjectHeader, slot: u32) -> u64 {
    if slot < super::super::object_inline_capacity(obj) {
        return *((obj as *const u8).add(std::mem::size_of::<ObjectHeader>()) as *const u64)
            .add(slot as usize);
    }
    super::super::spill::spill_get_present(obj as usize, slot as usize).unwrap_or(0)
}

/// Is a key after position `pos` of `obj`'s list present (its slot
/// written)? Then a fill of `pos` in place would enumerate out of order.
pub(crate) unsafe fn later_late_key_present(
    obj: *const ObjectHeader,
    keys: *const crate::array::ArrayHeader,
    count: u32,
    pos: u32,
) -> bool {
    // A family object: its descriptor names every key's slot, all inline (a
    // birth allocates them), so the scan is one load per key.
    if let Some(d) = family_desc_of(obj) {
        if d.key_count == count
            && !d.keys.is_null()
            && d.slot_count <= super::super::object_inline_capacity(obj)
        {
            let fields = (obj as *const u8).add(std::mem::size_of::<ObjectHeader>()) as *const u64;
            return std::slice::from_raw_parts(d.keys, d.key_count as usize)[pos as usize + 1..]
                .iter()
                .any(|k| *fields.add(k.slot as usize) != crate::value::TAG_HOLE);
        }
    }
    // Any present key after `pos` (a required one always is, an optional or
    // an added one once written): node appends a new key after it.
    (pos + 1..count).any(|p| {
        family_slot_word(obj, super::super::key_attrs::key_storage_slot(keys, p))
            != crate::value::TAG_HOLE
    })
}

/// A plain `[[Set]]` of string key `key_bits` (its bytes `bytes`) with
/// `value` on `obj`, which is its own receiver, when the store adds a key to
/// a layout's own terms ([`family_add`]): an optional key whose slot holds
/// the hole (a family's late key, a runtime layout's optional member), or a
/// key a family layout does not name. `false` leaves the store to the
/// caller's ordinary path: not a family object, a present key, a key the
/// overflow holds, a constructor key, and an add that is not plain (a
/// non-extensible object, a prototype chain that may intercept it). That path
/// sees an unwritten late key as absent (`key_view`, `proxy::own_set_descriptor`)
/// and, when it creates the property, comes back through
/// [`family_create`].
///
/// # Safety
/// `obj` is a live object or null; `key_bits` a live string value whose text
/// is `bytes`; the caller holds no other raw heap pointer across the call
/// (the overflow may allocate).
pub(crate) unsafe fn family_store(
    obj: *mut ObjectHeader,
    key_bits: u64,
    bytes: &[u8],
    value: u64,
) -> bool {
    use std::sync::atomic::Ordering::Relaxed;
    let family = family_desc_of(obj).is_some();
    if !family && !super::super::key_view::object_may_have_holes(obj) {
        return false;
    }
    // A private field's by-name write is the private path's: it fills or
    // overwrites the element, never a property of that spelling.
    if super::super::field_get_set::private_namespace_bytes(bytes) {
        return false;
    }
    FAMILY_DIAG.seen.fetch_add(1, Relaxed);
    if family && family_overwrite(obj, bytes, value) {
        return true;
    }
    let add = match family_add_kind(obj, bytes) {
        // A key outside a layout that is not a family's: the ordinary add.
        Some(FamilyAdd::Other) if !family => return false,
        Some(add) => add,
        None => return false,
    };
    let gc = crate::gc::header_from_trusted_user_ptr(obj.cast());
    let receiver =
        f64::from_bits(crate::value::POINTER_TAG | obj as u64 & crate::value::POINTER_MASK);
    if (*gc)._reserved
        & (crate::gc::OBJ_FLAG_FROZEN | crate::gc::OBJ_FLAG_SEALED | crate::gc::OBJ_FLAG_NO_EXTEND)
        != 0
        || super::super::shape_store::chain_blocks(receiver, bytes)
    {
        FAMILY_DIAG.declined.fetch_add(1, Relaxed);
        return false;
    }
    family_add(obj, key_bits, add, value);
    true
}

/// The `CreateDataProperty` step of an ordinary `[[Set]]` whose receiver
/// `receiver` does not hold key `key` (a string value): the caller's walk has
/// decided the add (the chain does not intercept it, the receiver is
/// extensible). For a family object, and for a layout's optional key whose
/// slot holds the hole on any object with such a layout, this makes the add
/// ([`family_add`]); `false` otherwise, and the caller defines the property.
///
/// # Safety
/// As [`family_store`].
pub(crate) unsafe fn family_create(receiver: f64, key: f64, value: f64) -> bool {
    let bits = receiver.to_bits();
    let key_value = crate::JSValue::from_bits(key.to_bits());
    if bits & !crate::value::POINTER_MASK != crate::value::POINTER_TAG || !key_value.is_any_string()
    {
        return false;
    }
    let obj = (bits & crate::value::POINTER_MASK) as *mut ObjectHeader;
    let family = family_desc_of(obj).is_some();
    if !family && !super::super::key_view::object_may_have_holes(obj) {
        return false;
    }
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let Some(bytes) = crate::string::js_string_key_bytes(key_value, &mut sso) else {
        return false;
    };
    let add = match family_add_kind(obj, bytes) {
        Some(FamilyAdd::Other) if !family => return false,
        Some(add) => add,
        None => return false,
    };
    family_add(obj, key.to_bits(), add, value.to_bits());
    true
}

/// The position of the key spelled `bytes` in descriptor `d`'s key sequence:
/// one probe of the descriptor's immutable hash index (§3.3), no key-list
/// scan.
#[inline]
pub(super) unsafe fn desc_position_by_bytes(d: &PerryLayoutDesc, bytes: &[u8]) -> Option<u32> {
    if d.index.is_null() || d.keys.is_null() {
        return None;
    }
    desc_position_hashed(
        d,
        bytes,
        super::super::key_bytes_hash(bytes.as_ptr(), bytes.len()),
    )
}

/// [`desc_position_by_bytes`] for `bytes` whose hash is `hash`.
#[inline]
pub(crate) unsafe fn desc_position_hashed(
    d: &PerryLayoutDesc,
    bytes: &[u8],
    hash: u64,
) -> Option<u32> {
    if d.index.is_null() || d.keys.is_null() {
        return None;
    }
    let mask = d.index_mask as u64;
    let mut at = hash & mask;
    for _ in 0..=mask {
        let pos = *d.index.add(at as usize);
        if pos == u32::MAX {
            return None;
        }
        if pos < d.key_count && {
            let atom = (*d.keys.add(pos as usize)).atom;
            (*atom).hash == hash && atom_bytes(atom) == Some(bytes)
        } {
            return Some(pos);
        }
        at = (at + 1) & mask;
    }
    None
}

/// `obj`'s own key spelled `bytes`, answered by its family layout alone
/// when `obj` is a family object with no overflow (every own key is then a
/// layout key, present while its slot holds a value): `Some(Some((pos,
/// slot)))` when the layout names the key at key position `pos` and storage
/// slot `slot` (the slot may hold the hole: the key is absent), `Some(None)`
/// when it names no such key (no own key of that name), `None` when `obj` is
/// not such an object. One probe of the descriptor's immutable hash index:
/// no key-list scan. Allocation-free.
///
/// # Safety
/// `obj` is a live object.
#[inline]
pub(crate) unsafe fn family_own_key(
    obj: *const ObjectHeader,
    bytes: &[u8],
) -> Option<Option<(u32, u32)>> {
    family_own_key_hashed(obj, bytes, None)
}

/// [`family_own_key`] with the key's hash (`key_bytes_hash(bytes)`) when the
/// caller has it already (a fixed key's compile-time hash).
///
/// # Safety
/// As [`family_own_key`]; `hash`, when given, is `bytes`' hash.
#[inline]
pub(crate) unsafe fn family_own_key_hashed(
    obj: *const ObjectHeader,
    bytes: &[u8],
    hash: Option<u64>,
) -> Option<Option<(u32, u32)>> {
    // The header word names a family descriptor through its shape record
    // (static family words and class-evaluation copies alike); most objects
    // are not family objects and answer from the word's tag bits first.
    let word = (*obj).parent_class_id;
    // Only a static family word (the image's family rows) or a class
    // evaluation's copy (a mixed-kind word) can name a descriptor; both tests
    // read the word alone, before any shape record.
    if word & LAYOUT_SHADOW_BIT != 0
        || !(word_in_layout_index(word)
            || super::super::shapes::shape_word_kind(word)
                == super::super::shapes::SHAPE_ID_KIND_MIXED)
    {
        return None;
    }
    let d = &*family_for_word(word)?;
    desc_own_key(d, bytes, hash)
}

/// [`family_own_key_hashed`] of an object whose family descriptor the caller
/// already holds (`d`, from its layout word or its shape record).
///
/// # Safety
/// `d` is a registered class family descriptor; `hash`, when given, is
/// `bytes`' hash.
#[inline(always)]
pub(crate) unsafe fn desc_own_key(
    d: &PerryLayoutDesc,
    bytes: &[u8],
    hash: Option<u64>,
) -> Option<Option<(u32, u32)>> {
    if d.index.is_null() || d.keys.is_null() {
        return None;
    }
    let keys = std::slice::from_raw_parts(d.keys, d.key_count as usize);
    // A private element is no own property (the index omits it too). A
    // layout lists each spelling once, so only a match is checked.
    let own = |pos: usize, k: &PerryLayoutKey| {
        (k.attrs & super::super::key_attrs::ENTRY_PRIVATE == 0).then_some((pos as u32, k.slot))
    };
    if let Some(pos) = leading_position(bytes).filter(|&pos| (pos as usize) < keys.len()) {
        let k = &keys[pos as usize];
        if atom_bytes(k.atom) == Some(bytes) {
            return Some(own(pos as usize, k));
        }
    }
    // A short list: its keys' lengths, then their bytes, with no hashing.
    if keys.len() <= SHORT_FAMILY_KEYS {
        for (pos, k) in keys.iter().enumerate() {
            if atom_bytes(k.atom) == Some(bytes) {
                return Some(own(pos, k));
            }
        }
        return Some(None);
    }
    let hash = hash.unwrap_or_else(|| super::super::key_bytes_hash(bytes.as_ptr(), bytes.len()));
    Some(desc_position_hashed(d, bytes, hash).map(|pos| (pos, keys[pos as usize].slot)))
}

/// A family layout with at most this many keys is searched by comparing
/// them, cheaper than hashing the name for the index.
pub(super) const SHORT_FAMILY_KEYS: usize = 8;

/// The key position a family layout over node's EventEmitter gives the
/// emitter's state keys: they lead it, in `EventEmitter.init`'s order
/// (`perry-codegen` `layout_families`, the native parent's init keys). Only
/// a first guess for [`family_own_key`], which compares the key there.
#[inline(always)]
pub(super) fn leading_position(bytes: &[u8]) -> Option<u32> {
    match bytes {
        b"_events" => Some(0),
        b"_eventsCount" => Some(1),
        b"_maxListeners" => Some(2),
        _ => None,
    }
}

/// A store of a present key of family object `obj`: a plain writable data
/// entry of its overflow, or (no overflow) a layout key whose entry has no
/// attribute but optional (every family key is, so no store cache serves it,
/// `key_attrs::entry_is_plain_overwrite`) and whose slot holds a value. The
/// store overwrites in place; `false` for anything else.
pub(super) unsafe fn family_overwrite(obj: *mut ObjectHeader, bytes: &[u8], value: u64) -> bool {
    let gc = crate::gc::header_from_trusted_user_ptr(obj.cast());
    if (*gc)._reserved & crate::gc::OBJ_FLAG_FROZEN != 0 {
        return false;
    }
    let table = super::super::overflow::table(obj);
    if !table.is_null() {
        // The overflow holds every present key (a layout key as a reference
        // to its slot): a live plain data entry is overwritten in place.
        let Some(entry) = super::super::overflow::find_bytes(table, bytes) else {
            return false;
        };
        let attrs =
            super::super::overflow::entry_attrs(table as *const crate::array::ArrayHeader, entry);
        return super::super::key_attrs::entry_is_plain_writable_data(attrs)
            && super::super::overflow::store_index(obj, entry, value);
    }
    // A family layout without an overflow: the descriptor's index names the
    // key's slot.
    if let Some(d) = family_desc_of(obj) {
        if d.slot_count <= super::super::object_inline_capacity(obj) {
            let Some(pos) = desc_position_by_bytes(d, bytes) else {
                return false;
            };
            let k = &*d.keys.add(pos as usize);
            if k.attrs != LAYOUT_KEY_LATE || family_slot_word(obj, k.slot) == crate::value::TAG_HOLE
            {
                return false;
            }
            super::super::store_object_field_slot(obj, k.slot as usize, value);
            return true;
        }
    }
    let Some(record) = super::super::shapes::object_shape_record(obj) else {
        return false;
    };
    let keys = record.keys() as usize as *const crate::array::ArrayHeader;
    if keys.is_null() {
        return false;
    }
    let Some(pos) =
        super::super::keys_find_property_slot_by_bytes(keys, record.logical_key_count(), bytes)
    else {
        return false;
    };
    if super::super::key_attrs::keys_entry(keys, pos) != super::super::key_attrs::ENTRY_OPTIONAL {
        return false;
    }
    let slot = super::super::key_attrs::key_storage_slot(keys, pos);
    if slot >= super::super::object_inline_capacity(obj)
        || family_slot_word(obj, slot) == crate::value::TAG_HOLE
    {
        return false;
    }
    super::super::store_object_field_slot(obj, slot as usize, value);
    true
}

/// How a store of the key spelled `bytes` adds it to family object `obj`, or
/// `None` when it is not an add this module makes (a present key, a key the
/// overflow holds, a constructor key).
#[derive(Clone, Copy)]
pub(super) enum FamilyAdd {
    /// A late key at list position `pos`, its slot `slot` holding the hole.
    Late { pos: u32, slot: u32 },
    /// A key the layout does not name.
    Other,
}

pub(super) unsafe fn family_add_kind(obj: *mut ObjectHeader, bytes: &[u8]) -> Option<FamilyAdd> {
    // A family layout without an overflow: the descriptor's index answers.
    if !super::super::overflow::has_overflow(obj) {
        if let Some(d) = family_desc_of(obj) {
            if d.slot_count <= super::super::object_inline_capacity(obj) {
                return match desc_position_by_bytes(d, bytes) {
                    Some(pos) => {
                        let k = &*d.keys.add(pos as usize);
                        (k.attrs & LAYOUT_KEY_LATE != 0
                            && family_slot_word(obj, k.slot) == crate::value::TAG_HOLE)
                            .then_some(FamilyAdd::Late { pos, slot: k.slot })
                    }
                    None => Some(FamilyAdd::Other),
                };
            }
        }
    }
    let record = super::super::shapes::object_shape_record(obj)?;
    let keys = record.keys() as usize as *const crate::array::ArrayHeader;
    let count = record.logical_key_count();
    let position = if keys.is_null() {
        None
    } else {
        super::super::keys_find_property_slot_by_bytes(keys, count, bytes)
    };
    let held_by_overflow = || {
        super::super::overflow::has_overflow(obj)
            && super::super::overflow::find_bytes(super::super::overflow::table(obj), bytes)
                .is_some()
    };
    match position {
        Some(pos) => {
            if super::super::key_attrs::keys_entry(keys, pos)
                & super::super::key_attrs::ENTRY_OPTIONAL
                == 0
            {
                return None;
            }
            let slot = super::super::key_attrs::key_storage_slot(keys, pos);
            if family_slot_word(obj, slot) != crate::value::TAG_HOLE || held_by_overflow() {
                return None;
            }
            Some(FamilyAdd::Late { pos, slot })
        }
        None if held_by_overflow() => None,
        None => Some(FamilyAdd::Other),
    }
}

/// Add the key `key_bits` to family object `obj` with `value` (see the
/// section comment): a late key in order fills its slot in place; out of
/// order, or with the overflow installed, it is appended to the overflow
/// with a reference to its slot; a key the layout does not name is an
/// overflow value.
pub(super) unsafe fn family_add(obj: *mut ObjectHeader, key_bits: u64, add: FamilyAdd, value: u64) {
    use std::sync::atomic::Ordering::Relaxed;
    let shadowed = super::super::overflow::has_overflow(obj);
    let mut out_of_order = false;
    if let FamilyAdd::Late { pos, slot } = add {
        if !shadowed {
            let record = super::super::shapes::object_shape_record(obj).unwrap();
            let keys = record.keys() as usize as *const crate::array::ArrayHeader;
            out_of_order = later_late_key_present(obj, keys, record.logical_key_count(), pos);
            if out_of_order && family_diag_pairs_on() {
                note_out_of_order_pair(obj, pos);
            }
            // In order: the key's position is its enumeration order. An
            // object something inherits from takes the overflow, whose
            // install sets the shadow bit every inherited-read site tests
            // (step 3).
            if !out_of_order
                && slot < super::super::object_inline_capacity(obj)
                && !(super::super::proto_validity::any_prototype_marked()
                    && super::super::proto_validity::object_is_marked_prototype(obj as usize))
            {
                FAMILY_DIAG.fills.fetch_add(1, Relaxed);
                super::super::store_object_field_slot(obj, slot as usize, value);
                return;
            }
        }
    }
    if family_diag_on() {
        note_install_cause(match add {
            FamilyAdd::Late { .. } if out_of_order => install_cause::LATE_OUT_OF_ORDER,
            FamilyAdd::Late { .. } => install_cause::LATE_PROTO_OR_SPILL,
            FamilyAdd::Other => install_cause::OTHER_KEY,
        });
        if matches!(add, FamilyAdd::Other) {
            note_other_key(obj, key_bits, !shadowed);
        }
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let owner = scope.root_raw_mut_ptr(obj);
    let val = scope.root_nanbox_u64(value);
    let atom = crate::string::property_atom_from_value(key_bits);
    let atom = scope.root_nanbox_u64(atom);
    if !shadowed {
        FAMILY_DIAG.installs.fetch_add(1, Relaxed);
    }
    let atom_ptr =
        || (atom.get_nanbox_u64() & crate::value::POINTER_MASK) as *const crate::StringHeader;
    match add {
        FamilyAdd::Late { .. } => {
            if out_of_order {
                FAMILY_DIAG.out_of_order.fetch_add(1, Relaxed);
            }
            // The overflow first (its tombstone for this hole), then the
            // dynamic writer appends the key with a reference to its slot.
            super::super::overflow::ensure(owner.get_raw_mut_ptr::<ObjectHeader>());
            super::super::overflow::store_routed(
                owner.get_raw_mut_ptr::<ObjectHeader>(),
                atom_ptr(),
                f64::from_bits(val.get_nanbox_u64()),
            );
        }
        FamilyAdd::Other => {
            FAMILY_DIAG.other.fetch_add(1, Relaxed);
            super::super::overflow::store_unfixed_routed(
                owner.get_raw_mut_ptr::<ObjectHeader>(),
                atom_ptr(),
                f64::from_bits(val.get_nanbox_u64()),
            );
        }
    }
}

/// [`family_store`] for NaN-boxed operands: `target` is its own receiver and
/// `key` any string value (a heap or a short string).
///
/// # Safety
/// As [`family_store`].
pub(crate) unsafe fn family_store_value(target: f64, key: f64, value: f64) -> bool {
    let bits = target.to_bits();
    if bits & !crate::value::POINTER_MASK != crate::value::POINTER_TAG {
        return false;
    }
    let obj = (bits & crate::value::POINTER_MASK) as *mut ObjectHeader;
    if !family_store_admits(obj) {
        return false;
    }
    let key_value = crate::JSValue::from_bits(key.to_bits());
    if !key_value.is_any_string() {
        return false;
    }
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let Some(bytes) = crate::string::js_string_key_bytes(key_value, &mut sso) else {
        return false;
    };
    // `family_store` reads the text before its first allocation only; a copy
    // keeps the borrow independent of the key string.
    let mut buf = [0u8; 96];
    let owned;
    let bytes: &[u8] = if bytes.len() <= buf.len() {
        buf[..bytes.len()].copy_from_slice(bytes);
        &buf[..bytes.len()]
    } else {
        owned = bytes.to_vec();
        &owned
    };
    family_store(obj, key.to_bits(), bytes, value.to_bits())
}

/// `family_desc_of(obj).is_some() || object_may_have_holes(obj)`, answered
/// from ONE read of the receiver's record: the record names its family
/// descriptor and carries the key list summary. Every dynamic-key store asks
/// this first, so the second record read of the two-call form was paid by
/// every ordinary store.
///
/// # Safety
/// `obj` is a live object or null.
#[inline(always)]
pub(super) unsafe fn family_store_admits(obj: *const ObjectHeader) -> bool {
    if obj.is_null() || !crate::value::addr_class::is_plausible_heap_addr(obj as usize) {
        return false;
    }
    let word = (*obj).parent_class_id;
    if !super::super::shapes::is_layout_word(layout_id_of_word(word)) {
        return false;
    }
    // `obj` is live and carries `word`: the receiver read needs no check.
    let Some(record) = super::super::shapes::shape_record_of_receiver(word) else {
        return false;
    };
    if let Some(index) = record.family_desc_index_plus_one().checked_sub(1) {
        if literal_birth::family_at(index).is_some() {
            return true;
        }
    }
    super::super::key_view::summary_may_have_holes(record.summary())
        && !super::super::overflow::has_overflow(obj)
}

/// [`family_store`] for a key passed as a string header.
///
/// # Safety
/// As [`family_store`].
pub(crate) unsafe fn family_store_str(
    obj: *mut ObjectHeader,
    key: *const crate::StringHeader,
    value: f64,
) -> bool {
    if key.is_null()
        || family_desc_of(obj).is_none() && !super::super::key_view::object_may_have_holes(obj)
    {
        return false;
    }
    let key_bits = crate::JSValue::string_ptr(key as *mut _).bits();
    family_store_value(
        f64::from_bits(crate::value::POINTER_TAG | obj as u64 & crate::value::POINTER_MASK),
        f64::from_bits(key_bits),
        value,
    )
}
