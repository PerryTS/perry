//! Symbol data properties use the ordinary object's shape and property slots.
//! A private entry is runtime-only and is excluded from reflection.
use super::*;

pub(crate) const PRIVATE_ENTRY: u8 = 0x40;

pub(crate) unsafe fn owner(addr: usize) -> Option<*mut ObjectHeader> {
    let bag = if crate::value::addr_class::is_proxy_cell(addr) {
        crate::proxy_hooks::property_bag(addr)
    } else if crate::value::addr_class::try_read_tracked_gc_header(addr).is_some() {
        descriptor_state::descriptor_holder(addr)
    } else {
        super::handle_expando::handle_property_bag(addr as i64)
    };
    (!bag.is_null()).then_some(bag)
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
    let obj = owner(addr)?;
    let (keys, live) = object_keys_and_live_slot_count(obj);
    let index = position_in(keys, symbol)?;
    let value = object_key_field_at_with_keys(obj, keys.arr(), index, live).bits();
    (value != crate::value::TAG_HOLE).then_some(value)
}

/// Root all operands while publishing the shared layout and growing storage.
/// Existing symbol keys share exactly the same indexed store as string keys.
pub(crate) unsafe fn define(addr: usize, symbol: usize, bits: u64, entry: u8) -> bool {
    let _holder_edit = descriptor_state::HolderEdit::new(addr);
    if _holder_edit.is_some() {
        crate::closure::shape::note_function_own_state_changed(addr);
    }
    let Some(obj) = owner(addr) else {
        return false;
    };
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
                let parent = canonical_keys::canonicalize(&proof, keys.arr(), keys.count());
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

pub(crate) unsafe fn entries(addr: usize, include_private: bool) -> Vec<(usize, u64)> {
    let Some(obj) = owner(addr) else {
        return Vec::new();
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
    let obj = owner(addr)?;
    let (keys, live) = object_keys_and_live_slot_count(obj);
    let pos = position_in(keys, symbol)?;
    if object_key_field_at_with_keys(obj, keys.arr(), pos, live).bits() == crate::value::TAG_HOLE {
        return None;
    }
    Some(key_attrs::keys_entry(keys.arr(), pos))
}

pub(crate) unsafe fn set_entry(addr: usize, symbol: usize, entry: u8) {
    let _holder_edit = descriptor_state::HolderEdit::new(addr);
    let Some(obj) = owner(addr) else {
        return;
    };
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
    let Some(owner) = owner(addr) else {
        return false;
    };
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
    let _holder_edit = descriptor_state::HolderEdit::new(addr);
    let Some(obj) = owner(addr) else {
        return false;
    };
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
                .is_some_and(|header| header.as_ref().obj_type == crate::gc::GC_TYPE_ARRAY)
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
