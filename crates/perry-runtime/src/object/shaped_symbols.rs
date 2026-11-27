//! Symbol data properties use the ordinary object's shape and property slots.
//! A private entry is runtime-only and is excluded from reflection.
use super::*;

pub(crate) const PRIVATE_ENTRY: u8 = 0x40;

use descriptor_state::KeyTarget;

/// The own-key target of `addr`'s symbol keys: an ordinary holder, or a
/// function addressed by its closure (its store stays behind the
/// `closure::props` leaves). `None` when it has no own-key storage yet.
pub(crate) unsafe fn owner(addr: usize) -> Option<KeyTarget> {
    let bag = if crate::value::addr_class::is_proxy_cell(addr) {
        crate::proxy_hooks::property_bag(addr)
    } else if crate::value::addr_class::try_read_tracked_gc_header(addr).is_some() {
        match descriptor_state::holder_target(addr) {
            KeyTarget::Object(bag) => bag,
            KeyTarget::Function(p) => {
                return crate::closure::props::fn_has_key_store(p).then_some(KeyTarget::Function(p))
            }
        }
    } else {
        super::handle_expando::handle_property_bag(addr as i64)
    };
    (!bag.is_null()).then_some(KeyTarget::Object(bag))
}

/// The ordinary object holding `addr`'s symbol keys (a function's bag).
#[cfg(test)]
pub(crate) unsafe fn owner_object(addr: usize) -> Option<*mut ObjectHeader> {
    Some(match owner(addr)? {
        KeyTarget::Object(o) => o,
        KeyTarget::Function(p) => crate::closure::props::bag_of(p),
    })
}

pub(crate) unsafe fn position(obj: *const ObjectHeader, symbol: usize) -> Option<u32> {
    position_in(object_keys(obj), symbol)
}

/// [`position`] in the receiver's already resolved `keys`.
pub(crate) unsafe fn position_in(keys: ObjectKeys, symbol: usize) -> Option<u32> {
    let bits = crate::value::POINTER_TAG | symbol as u64;
    let (slots, count) = keys.dense_slots();
    (0..count)
        .find(|&i| (*slots.add(i)).to_bits() == bits)
        .map(|i| i as u32)
}

pub(crate) unsafe fn get(addr: usize, symbol: usize) -> Option<u64> {
    let obj = match owner(addr)? {
        KeyTarget::Object(o) => o,
        KeyTarget::Function(p) => return crate::closure::props::fn_sym_get(p, symbol),
    };
    let (keys, live) = object_keys_and_live_slot_count(obj);
    let index = position_in(keys, symbol)?;
    let value = object_key_field_at_with_keys(obj, keys.arr(), index, live).bits();
    (value != crate::value::TAG_HOLE).then_some(value)
}

/// The value of `obj`'s own symbol slot `symbol` (no holder redirect: `obj`
/// is a live ordinary object). The first listed key is compared first, so a
/// key an object was born with at index 0 costs one compare.
#[inline]
pub(crate) unsafe fn own_private(obj: *const ObjectHeader, symbol: usize) -> Option<u64> {
    let (keys, live) = object_keys_and_live_slot_count(obj);
    let bits = crate::value::POINTER_TAG | symbol as u64;
    let (slots, count) = keys.dense_slots();
    if count == 0 {
        return None;
    }
    let index = if (*slots.add(0)).to_bits() == bits {
        0
    } else {
        position_in(keys, symbol)?
    };
    let value = object_key_field_at_with_keys(obj, keys.arr(), index, live).bits();
    (value != crate::value::TAG_HOLE).then_some(value)
}

/// Fill the hole of `symbol` on an object whose layout lists it: the key is
/// listed optional with `entry` (so the hole is its absence), its slot is
/// inline and still the hole, and no later key is present (filling it keeps
/// the key order the list states). One slot store; the object stays on its
/// layout. `false`, having changed nothing, otherwise.
///
/// # Safety
/// `obj` is a live ordinary object.
pub(crate) unsafe fn fill_listed_hole(
    obj: *mut ObjectHeader,
    symbol: usize,
    bits: u64,
    entry: u8,
) -> bool {
    if super::overflow::has_overflow(obj) {
        return false;
    }
    let (keys, live) = object_keys_and_live_slot_count(obj);
    let Some(index) = position_in(keys, symbol) else {
        return false;
    };
    if key_attrs::keys_entry(keys.arr(), index) != entry | key_attrs::ENTRY_OPTIONAL {
        return false;
    }
    let slot = keys.storage_slot(index);
    if slot >= object_inline_capacity(obj)
        || (index..keys.count()).any(|i| {
            object_key_field_at_with_keys(obj, keys.arr(), i, live).bits() != crate::value::TAG_HOLE
        })
    {
        return false;
    }
    super::store_object_field_slot(obj, slot as usize, bits);
    true
}

/// Root all operands while publishing the shared layout and growing storage.
/// Existing symbol keys share exactly the same indexed store as string keys.
pub(crate) unsafe fn define(addr: usize, symbol: usize, bits: u64, entry: u8) -> bool {
    let _holder_edit = descriptor_state::HolderEdit::new(addr);
    if _holder_edit.is_some() {
        crate::closure::shape::note_function_own_state_changed(addr);
    }
    let obj = match owner(addr) {
        Some(KeyTarget::Object(o)) => o,
        Some(KeyTarget::Function(p)) => {
            return crate::closure::props::fn_sym_define(p, symbol, bits, entry)
        }
        None => return false,
    };
    define_on(obj, symbol, bits, entry)
}

/// [`define`] on the resolved ordinary holder, after the owner's prologue.
pub(crate) unsafe fn define_on(
    obj: *mut ObjectHeader,
    symbol: usize,
    bits: u64,
    entry: u8,
) -> bool {
    let _no_move = crate::gc::GcSuppressScope::new();
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj = scope.root_raw_mut_ptr(obj);
    let symbol = scope.root_nanbox_u64(crate::value::POINTER_TAG | symbol as u64);
    let value = scope.root_nanbox_u64(bits);
    obj.with_mut_ptr::<ObjectHeader, _>(|current| {
        // Once the object has an overflow it is the order authority: a new
        // symbol key is an entry there, after every key created before it.
        // A symbol the layout names keeps meeting its fixed slot (§3.3), also
        // when it is defined again after a delete.
        if super::overflow::has_overflow(current) {
            super::overflow::reattach_fixed(current, symbol.get_nanbox_u64());
            super::overflow::insert_key_bits(
                current,
                symbol.get_nanbox_u64(),
                value.get_nanbox_u64(),
                entry,
            );
            return true;
        }
        // Otherwise a symbol key is a key of the object's layout, as a
        // defined string key is: builtin prototypes keep their symbol
        // methods in fixed slots that method sites and shape proofs read.
        let keys = object_keys(current);
        let existing = position_in(
            keys,
            (symbol.get_nanbox_u64() & crate::value::POINTER_MASK) as usize,
        );
        // An existing key keeps its storage slot; an added key's slot is its
        // position (`key_attrs::SUMMARY_SLOTS`).
        let index = if let Some(index) = existing {
            let slot = keys.storage_slot(index);
            if entry & !key_attrs::ENTRY_OPTIONAL == 0
                && key_attrs::keys_entry(keys.arr(), index) == key_attrs::ENTRY_OPTIONAL
                && slot < object_inline_capacity(current)
            {
                let word = object_key_field_at_with_keys(
                    current,
                    keys.arr(),
                    index,
                    object_live_slot_count(current),
                )
                .bits();
                let later = word == crate::value::TAG_HOLE
                    && (index + 1..keys.count()).any(|i| {
                        object_key_field_at_with_keys(
                            current,
                            keys.arr(),
                            i,
                            object_live_slot_count(current),
                        )
                        .bits()
                            != crate::value::TAG_HOLE
                    });
                if later {
                    super::overflow::ensure(current);
                    super::overflow::reattach_fixed(current, symbol.get_nanbox_u64());
                    super::overflow::insert_key_bits(
                        current,
                        symbol.get_nanbox_u64(),
                        value.get_nanbox_u64(),
                        entry & !key_attrs::ENTRY_OPTIONAL,
                    );
                } else {
                    super::store_object_field_slot(current, slot as usize, value.get_nanbox_u64());
                }
                return true;
            }
            slot
        } else {
            // Reserved native-layout fields precede dynamic keys just as they do
            // on the string-property append path.
            if object_keys(current).is_null() && reserved_slot_floor_for_object(current) != 0 {
                ensure_reserved_floor_keys(current);
            }
            let keys = object_keys(current);
            let index = keys.count();
            let key = JSValue::from_bits(symbol.get_nanbox_u64());
            let next = if let Some(proof) = canonical_keys::SharedLayout::of_receiver(current) {
                let parent = canonical_keys::canonicalize_keys(&proof, keys);
                canonical_keys::extend_slot(
                    &proof,
                    parent,
                    canonical_keys::Appended::Slot(JSValue::from_bits(symbol.get_nanbox_u64())),
                    entry,
                )
                .view()
            } else {
                let array = if keys.is_null() {
                    crate::array::js_array_alloc(0)
                } else {
                    keys.arr()
                };
                let grown = crate::array::js_array_push(array, key);
                let grown = key_attrs::owned_note_append(grown, index, entry);
                ObjectKeys::owned(grown)
            };
            set_object_keys(current, next);
            index
        };
        let live = object_live_slot_count(current);
        if index < object_inline_capacity(current) {
            if index >= live {
                set_object_live_slot_count(current, index + 1);
            }
            js_object_set_field(current, index, JSValue::from_bits(value.get_nanbox_u64()));
        } else {
            // The existing property spill is object-owned, traced storage.
            overflow_set(current as usize, index as usize, value.get_nanbox_u64());
        }
        set_entry(
            current as usize,
            (symbol.get_nanbox_u64() & crate::value::POINTER_MASK) as usize,
            entry,
        );
        true
    })
}

/// The store probe's symbol key add (`layout_probe::symbol_store`, which
/// proved `obj` an ordinary object with no overflow, `symbol` absent from its
/// complete own list, and the add admitted by its chain): the canonical
/// key-add transition with a plain data entry, as a string key's add. The
/// object is its own holder, so there is no HolderEdit and no owner lookup.
/// `false`, having changed nothing, when the receiver's keys are not a
/// shared canonical layout or it still lacks its reserved-slot keys.
pub(crate) unsafe fn append(obj: *mut ObjectHeader, symbol: usize, bits: u64) -> bool {
    let keys = object_keys(obj);
    if keys.is_null() && reserved_slot_floor_for_object(obj) != 0 {
        return false;
    }
    let Some(proof) = canonical_keys::SharedLayout::of_receiver(obj) else {
        return false;
    };
    let _no_move = crate::gc::GcSuppressScope::new();
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj = scope.root_raw_mut_ptr(obj);
    let key = scope.root_nanbox_u64(crate::value::POINTER_TAG | symbol as u64);
    let value = scope.root_nanbox_u64(bits);
    obj.with_mut_ptr::<ObjectHeader, _>(|current| {
        let keys = object_keys(current);
        let index = keys.count();
        let parent = canonical_keys::canonicalize_keys(&proof, keys);
        let next = canonical_keys::extend_slot(
            &proof,
            parent,
            canonical_keys::Appended::Slot(JSValue::from_bits(key.get_nanbox_u64())),
            0,
        )
        .view();
        // One transition: the successor names the appended key and, for an
        // inline slot, the live bound that covers it (a separate live-count
        // publish would mint a second shape per append).
        let inline = index < object_inline_capacity(current);
        let live = object_live_slot_count(current);
        let live = if inline { live.max(index + 1) } else { live };
        super::set_object_keys_with_live(current, next, live);
        if inline {
            js_object_set_field(current, index, JSValue::from_bits(value.get_nanbox_u64()));
        } else {
            overflow_set(current as usize, index as usize, value.get_nanbox_u64());
        }
        true
    })
}

pub(crate) unsafe fn entries(addr: usize, include_private: bool) -> Vec<(usize, u64)> {
    let obj = match owner(addr) {
        Some(KeyTarget::Object(o)) => o,
        Some(KeyTarget::Function(p)) => {
            return crate::closure::props::fn_sym_entries(p, include_private)
        }
        None => return Vec::new(),
    };
    let (keys, live) = object_keys_and_live_slot_count(obj);
    let (slots, count) = keys.dense_slots();
    let mut result = Vec::new();
    for i in 0..count {
        let key = JSValue::from_bits((*slots.add(i)).to_bits());
        if !key.is_pointer() {
            continue;
        }
        if !include_private && key_attrs::keys_entry(keys.arr(), i as u32) & PRIVATE_ENTRY != 0 {
            continue;
        }
        let symbol = (key.bits() & crate::value::POINTER_MASK) as usize;
        // A non-string key in this shape is an actual Symbol, never an encoded name.
        let value = object_key_field_at_with_keys(obj, keys.arr(), i as u32, live).bits();
        if value != crate::value::TAG_HOLE {
            result.push((symbol, value));
        }
    }
    result
}

pub(crate) unsafe fn entry(addr: usize, symbol: usize) -> Option<u8> {
    let obj = match owner(addr)? {
        KeyTarget::Object(o) => o,
        KeyTarget::Function(p) => return crate::closure::props::fn_sym_entry(p, symbol),
    };
    let (keys, live) = object_keys_and_live_slot_count(obj);
    let pos = position_in(keys, symbol)?;
    if object_key_field_at_with_keys(obj, keys.arr(), pos, live).bits() == crate::value::TAG_HOLE {
        return None;
    }
    Some(key_attrs::keys_entry(keys.arr(), pos))
}

pub(crate) unsafe fn set_entry(addr: usize, symbol: usize, entry: u8) {
    let _holder_edit = descriptor_state::HolderEdit::new(addr);
    let obj = match owner(addr) {
        Some(KeyTarget::Object(o)) => o,
        Some(KeyTarget::Function(p)) => {
            return crate::closure::props::fn_sym_set_entry(p, symbol, entry)
        }
        None => return,
    };
    set_entry_on(obj, symbol, entry)
}

/// [`set_entry`] on the resolved ordinary holder, after the owner's prologue.
pub(crate) unsafe fn set_entry_on(obj: *mut ObjectHeader, symbol: usize, entry: u8) {
    let Some(pos) = position(obj, symbol) else {
        return;
    };
    let table = super::overflow::table(obj);
    if !table.is_null() {
        super::overflow::set_entry_attrs(obj as *mut ObjectHeader, table, pos, entry);
        return;
    }
    let _no_move = crate::gc::GcSuppressScope::new();
    let keys = object_keys(obj);
    if key_attrs::keys_entry(keys.arr(), pos) == entry {
        return;
    }
    if let Some(proof) = canonical_keys::SharedLayout::of_receiver(obj) {
        let next = canonical_keys::rebuild_with_entries(&proof, keys, pos, |i, _, old| {
            if i == pos {
                entry
            } else {
                old
            }
        });
        set_object_keys(obj, next.view());
    } else {
        let next = key_attrs::ensure_owned_attrs(keys.arr(), keys.count());
        key_attrs::attrs_set_owned(next, key_attrs::keys_attrs(next), pos, entry);
        set_object_keys(obj, ObjectKeys::owned(next));
        shapes::transition_object_shape_semantics(obj);
    }
}

pub(crate) unsafe fn accessor(addr: usize, symbol: usize) -> Option<(u64, u64)> {
    if entry(addr, symbol)? & key_attrs::ENTRY_ACCESSOR == 0 {
        return None;
    }
    let acc = accessor_pair::pair_of_value(get(addr, symbol)?).unwrap_or_default();
    Some((acc.get, acc.set))
}

pub(crate) unsafe fn define_accessor(addr: usize, symbol: usize, get: u64, set: u64) -> bool {
    let _holder_edit = descriptor_state::HolderEdit::new(addr);
    if _holder_edit.is_some() {
        crate::closure::shape::note_function_own_state_changed(addr);
    }
    let owner = match owner(addr) {
        Some(KeyTarget::Object(o)) => o,
        Some(KeyTarget::Function(p)) => {
            return crate::closure::props::fn_sym_define_accessor(p, symbol, get, set)
        }
        None => return false,
    };
    define_accessor_on(owner, symbol, get, set)
}

/// [`define_accessor`] on the resolved ordinary holder, after the owner's prologue.
pub(crate) unsafe fn define_accessor_on(
    owner: *mut ObjectHeader,
    symbol: usize,
    get: u64,
    set: u64,
) -> bool {
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj = scope.root_raw_mut_ptr(owner);
    let symbol = scope.root_nanbox_u64(crate::value::POINTER_TAG | symbol as u64);
    let get = scope.root_nanbox_u64(if get == 0 {
        crate::value::TAG_UNDEFINED
    } else {
        get
    });
    let set = scope.root_nanbox_u64(if set == 0 {
        crate::value::TAG_UNDEFINED
    } else {
        set
    });
    // String and Symbol accessor entries own the same private pair. Its
    // closure words are initialized once, rather than pushed through the
    // public array API and later read as observable array properties.
    let pair = scope.root_raw_mut_ptr(accessor_pair::pair_new(accessor_pair::Accessor {
        get: get.get_nanbox_u64(),
        set: set.get_nanbox_u64(),
        ..Default::default()
    }));
    obj.with_mut_ptr::<ObjectHeader, _>(|ptr| {
        let addr = ptr as usize;
        let symbol = (symbol.get_nanbox_u64() & crate::value::POINTER_MASK) as usize;
        let attrs = entry(addr, symbol).unwrap_or(0) & key_attrs::ENTRY_ATTR_MASK;
        let flags = attrs
            | key_attrs::ENTRY_ACCESSOR
            | if get.get_nanbox_u64() != crate::value::TAG_UNDEFINED {
                key_attrs::ENTRY_HAS_GET
            } else {
                0
            }
            | if set.get_nanbox_u64() != crate::value::TAG_UNDEFINED {
                key_attrs::ENTRY_HAS_SET
            } else {
                0
            };
        define(
            addr,
            symbol,
            pair.with_const_ptr::<crate::array::ArrayHeader, _>(|ptr| {
                JSValue::pointer(ptr.cast()).bits()
            }),
            flags,
        );
        true
    })
}

pub(crate) unsafe fn delete(addr: usize, symbol: usize) -> bool {
    super::builtin_proto::note_write_addr(
        addr,
        super::builtin_proto::Mutation::Delete(super::builtin_proto::Key::Value(f64::from_bits(
            crate::value::POINTER_TAG | symbol as u64,
        ))),
        "delete symbol",
    );
    let _holder_edit = descriptor_state::HolderEdit::new(addr);
    let obj = match owner(addr) {
        Some(KeyTarget::Object(o)) => o,
        Some(KeyTarget::Function(p)) => return crate::closure::props::fn_sym_delete(p, symbol),
        None => return false,
    };
    delete_on(addr, obj, symbol)
}

/// [`delete`] on the resolved ordinary holder, after the owner's prologue.
pub(crate) unsafe fn delete_on(addr: usize, obj: *mut ObjectHeader, symbol: usize) -> bool {
    // Array.prototype's @@iterator is a fixed slot that array iteration
    // reads (object model step 7): while the object has no overflow, a delete
    // leaves the layout key and holes that slot, so a redefine refills it.
    // Deleting an absent key changes nothing, so it establishes no overflow.
    if !super::overflow::has_overflow(obj) {
        let Some(pos) = position(obj, symbol) else {
            return true;
        };
        // A refused delete changes nothing either.
        if key_attrs::keys_entry(object_keys(obj).arr(), pos) & key_attrs::ENTRY_NON_CONFIGURABLE
            != 0
        {
            return false;
        }
        let pos = object_keys(obj).storage_slot(pos);
        if pos == crate::object::runtime_layouts::ARRAY_PROTOTYPE_ITERATOR_SLOT
            && crate::value::addr_class::try_read_tracked_gc_header(addr)
                .is_some_and(|header| crate::gc_type_is_array!(header.as_ref().obj_type))
        {
            let _no_move = crate::gc::GcSuppressScope::new();
            set_entry(addr, symbol, 0);
            store(obj, pos, crate::value::TAG_HOLE);
            return true;
        }
    }
    if !super::overflow::delete_key_bits(obj, crate::value::POINTER_TAG | symbol as u64) {
        return false;
    }
    true
}

/// The entry of `obj`'s own present symbol key `symbol`, read from `obj`'s
/// own key list (no holder route). `None` when the list does not name it
/// present, or when the object's keys moved to an overflow table (the
/// caller's generic path answers those).
///
/// # Safety
/// `obj` is a live, unforwarded ordinary object whose attributes live in its
/// key list (an object its runtime family births).
#[inline]
unsafe fn own_listed(obj: *const ObjectHeader, symbol: usize) -> Option<(u32, u8, u64)> {
    if super::overflow::has_overflow(obj) {
        return None;
    }
    let (keys, live) = object_keys_and_live_slot_count(obj);
    let index = position_in(keys, symbol)?;
    let value = object_key_field_at_with_keys(obj, keys.arr(), index, live).bits();
    if value == crate::value::TAG_HOLE {
        return None;
    }
    Some((index, key_attrs::keys_entry(keys.arr(), index), value))
}

/// `obj`'s own data value of `symbol`: [[GetOwnProperty]].[[Value]] of an
/// own data property. `None` for an absent key or an accessor.
///
/// # Safety
/// As [`own_listed`].
#[inline]
pub(crate) unsafe fn own_data_value(obj: *const ObjectHeader, symbol: usize) -> Option<u64> {
    let (_, entry, value) = own_listed(obj, symbol)?;
    (entry & key_attrs::ENTRY_ACCESSOR == 0).then_some(value)
}

/// The entry of `obj`'s own present key `symbol` ([`entry`] without the
/// holder route).
///
/// # Safety
/// As [`own_listed`].
#[inline]
pub(crate) unsafe fn own_entry(obj: *const ObjectHeader, symbol: usize) -> Option<u8> {
    own_listed(obj, symbol).map(|(_, entry, _)| entry)
}

/// Store `bits` into `obj`'s own data property `symbol` when OrdinarySet
/// with `obj` as its own receiver writes exactly that slot: the key is own,
/// present, a data property held inline, and writable on an unfrozen object.
/// A runtime-only (`private`) entry is written whatever its attributes, as
/// [`define_on`] writes an existing key. `false`, having changed nothing,
/// otherwise.
///
/// # Safety
/// As [`own_listed`].
#[inline]
pub(crate) unsafe fn store_own_data(
    obj: *mut ObjectHeader,
    symbol: usize,
    bits: u64,
    private: bool,
) -> bool {
    let Some((index, entry, _)) = own_listed(obj, symbol) else {
        return false;
    };
    if entry & key_attrs::ENTRY_ACCESSOR != 0 {
        return false;
    }
    if !private {
        let gc = crate::gc::header_from_trusted_user_ptr(obj as *const u8);
        if entry & key_attrs::ENTRY_NON_WRITABLE != 0
            || (*gc)._reserved & crate::gc::OBJ_FLAG_FROZEN != 0
        {
            return false;
        }
    }
    let slot = object_keys(obj).storage_slot(index);
    if slot >= object_inline_capacity(obj) {
        return false;
    }
    let _no_move = crate::gc::GcSuppressScope::new();
    super::store_object_field_slot(obj, slot as usize, bits);
    true
}

/// A fresh instance of `class_id` born in the final shape its prototype
/// `proto` recorded for it (`ObjectMeta::instance_birth`, written by
/// [`record_birth`]), its slots `0..values.len()` holding `values`. `None`,
/// with nothing reachable made, when no record names `class_id` or the
/// record no longer validates as that birth.
///
/// # Safety
/// `proto` is the live intrinsic prototype (rooted by its realm cell);
/// `values` are pointer-free words (no root or barrier is needed for them).
pub(crate) unsafe fn born_on_record(
    class_id: u32,
    proto: *mut ObjectHeader,
    values: &[u64],
) -> Option<*mut ObjectHeader> {
    let meta = crate::meta_read!(crate::meta_obj_runtime_born!(proto));
    if meta.is_null() {
        return None;
    }
    let word = (*meta).instance_birth;
    if word as u32 != class_id {
        return None;
    }
    let proto_id = (*meta).proto_serial;
    let id = crate::codegen_abi::LayoutWord((word >> 32) as u32);
    let count = values.len() as u32;
    let rep = shapes::shape_rep_for_word(id);
    if !(0..count).all(|slot| field_rep::slot_rep(rep, slot) == field_rep::REP_ANY) {
        return None;
    }
    let obj = super::object_alloc_unpublished(class_id, count);
    if obj.is_null() {
        return None;
    }
    let kind = shapes::store_kind::receiver_ordinary_kind(obj);
    if !shapes::shape_is_attributed_filled_birth(id, proto_id, count, kind) {
        return None;
    }
    if crate::arena::pointer_in_nursery(obj as usize) {
        // GC_STORE_AUDIT(POINTER_FREE): a ShapeId, never a heap reference.
        (*obj).parent_class_id = id.0;
    } else {
        shapes::stamp_object_shape_id_with_carrier_note(obj, id);
    }
    let fields = crate::meta_slots_of!(crate::meta_obj_runtime_born!(obj)) as *mut u64;
    for (i, &bits) in values.iter().enumerate() {
        debug_assert!(!crate::value::JSValue::from_bits(bits).is_pointer());
        crate::gc::runtime_store_jsvalue_slot_layout_deferred(
            obj as usize,
            fields.add(i) as usize,
            i,
            bits,
        );
    }
    Some(obj)
}

/// Record `obj`'s shape on its prototype `proto` as the final birth shape of
/// `class_id` ([`born_on_record`]). `obj` was just built by the family's
/// ordinary sequence, keys in birth order; nothing is recorded when its
/// header carries per-object descriptor state a replay would not reproduce,
/// or when `proto` has no meta record yet (no allocation here).
///
/// # Safety
/// `obj` and `proto` are live; nothing allocates in between.
pub(crate) unsafe fn record_birth(class_id: u32, proto: *mut ObjectHeader, obj: *mut ObjectHeader) {
    let gc = crate::gc::header_from_trusted_user_ptr(obj as *const u8);
    if (*gc)._reserved & crate::gc::OBJ_FLAG_HAS_DESCRIPTORS != 0
        || super::overflow::has_overflow(obj)
    {
        return;
    }
    let meta = crate::meta_read!(crate::meta_obj_runtime_born!(proto));
    if meta.is_null() {
        return;
    }
    let shape = shapes::object_shape_stamp(obj);
    // GC_STORE_AUDIT(POINTER_FREE): a class id and a ShapeId, never a heap
    // reference.
    (*meta).instance_birth = u64::from(class_id) | u64::from(shape.0) << 32;
    shapes::note_old_generation_carrier(shapes::shape_record_for_word(shape));
}

unsafe fn store(obj: *mut ObjectHeader, index: u32, bits: u64) {
    let live = object_live_slot_count(obj);
    if index < object_inline_capacity(obj) {
        if index >= live {
            set_object_live_slot_count(obj, index + 1);
        }
        js_object_set_field(obj, index, JSValue::from_bits(bits));
    } else {
        overflow_set(obj as usize, index as usize, bits);
    }
}
