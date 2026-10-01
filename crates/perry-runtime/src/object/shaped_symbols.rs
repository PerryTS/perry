//! Symbol data properties use the ordinary object's shape and property slots.
//! A private entry is runtime-only and is excluded from reflection.
use super::*;

pub(crate) const PRIVATE_ENTRY: u8 = 0x40;

pub(crate) unsafe fn owner(addr: usize) -> Option<*mut ObjectHeader> {
    let header = crate::value::addr_class::try_read_tracked_gc_header(addr)?;
    (header.as_ref().obj_type == crate::gc::GC_TYPE_OBJECT).then_some(addr as *mut ObjectHeader)
}

pub(crate) unsafe fn position(obj: *const ObjectHeader, symbol: usize) -> Option<u32> {
    let keys = object_keys(obj);
    let bits = crate::value::POINTER_TAG | symbol as u64;
    let (slots, count) = keys.dense_slots();
    (0..count)
        .find(|&i| (*slots.add(i)).to_bits() == bits)
        .map(|i| i as u32)
}

pub(crate) unsafe fn get(addr: usize, symbol: usize) -> Option<u64> {
    let obj = owner(addr)?;
    let index = position(obj, symbol)?;
    Some(js_object_get_field(obj, index).bits())
}

/// Root all operands while publishing the shared layout and growing storage.
/// Existing symbol keys share exactly the same indexed store as string keys.
pub(crate) unsafe fn define(addr: usize, symbol: usize, bits: u64, entry: u8) -> bool {
    let Some(obj) = owner(addr) else {
        return false;
    };
    let _no_move = crate::gc::GcSuppressScope::new();
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj = scope.root_raw_mut_ptr(obj);
    let symbol = scope.root_nanbox_u64(crate::value::POINTER_TAG | symbol as u64);
    let value = scope.root_nanbox_u64(bits);
    obj.with_mut_ptr::<ObjectHeader, _>(|current| {
        let existing = position(
            current,
            (symbol.get_nanbox_u64() & crate::value::POINTER_MASK) as usize,
        );
        let index = if let Some(index) = existing {
            index
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
        if index < live.max(INLINE_SLOT_FLOOR as u32) {
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
        crate::symbol::symbol_property_ic_epoch_bump();
        true
    })
}

pub(crate) unsafe fn entries(addr: usize, include_private: bool) -> Vec<(usize, u64)> {
    let Some(obj) = owner(addr) else {
        return Vec::new();
    };
    let keys = object_keys(obj);
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
        result.push((symbol, js_object_get_field(obj, i as u32).bits()));
    }
    result
}

pub(crate) unsafe fn entry(addr: usize, symbol: usize) -> Option<u8> {
    let obj = owner(addr)?;
    let pos = position(obj, symbol)?;
    Some(key_attrs::keys_entry(object_keys(obj).arr(), pos))
}

pub(crate) unsafe fn set_entry(addr: usize, symbol: usize, entry: u8) {
    let Some(obj) = owner(addr) else {
        return;
    };
    let Some(pos) = position(obj, symbol) else {
        return;
    };
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
    crate::symbol::symbol_property_ic_epoch_bump();
}

pub(crate) unsafe fn accessor(addr: usize, symbol: usize) -> Option<(u64, u64)> {
    if entry(addr, symbol)? & key_attrs::ENTRY_ACCESSOR == 0 {
        return None;
    }
    let pair =
        (get(addr, symbol)? & crate::value::POINTER_MASK) as *const crate::array::ArrayHeader;
    let bits = |i| {
        let bits = crate::array::js_array_get(pair, i).bits();
        if bits == crate::value::TAG_UNDEFINED {
            0
        } else {
            bits
        }
    };
    Some((bits(0), bits(1)))
}

pub(crate) unsafe fn define_accessor(addr: usize, symbol: usize, get: u64, set: u64) -> bool {
    if owner(addr).is_none() {
        return false;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj = scope.root_raw_mut_ptr(addr as *mut ObjectHeader);
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
    let pair = scope.root_raw_mut_ptr(crate::array::js_array_alloc(2));
    let next = pair.with_mut_ptr(|ptr| {
        crate::array::js_array_push(ptr, JSValue::from_bits(get.get_nanbox_u64()))
    });
    pair.set_raw_mut_ptr(next);
    let next = pair.with_mut_ptr(|ptr| {
        crate::array::js_array_push(ptr, JSValue::from_bits(set.get_nanbox_u64()))
    });
    pair.set_raw_mut_ptr(next);
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
    let Some(obj) = owner(addr) else {
        return false;
    };
    let Some(pos) = position(obj, symbol) else {
        return true;
    };
    let _no_move = crate::gc::GcSuppressScope::new();
    let keys = object_keys(obj);
    let count = keys.count();
    let mut next = crate::array::js_array_alloc(count.saturating_sub(1));
    let (slots, _) = keys.dense_slots();
    for i in 0..count {
        if i == pos {
            continue;
        }
        let index = if i < pos { i } else { i - 1 };
        next = crate::array::js_array_push(
            next,
            JSValue::from_bits((*slots.add(i as usize)).to_bits()),
        );
        next = key_attrs::owned_note_append(next, index, key_attrs::keys_entry(keys.arr(), i));
        if i > pos {
            let value = js_object_get_field(obj, i);
            store(obj, index, value.bits());
        }
    }
    store(obj, count - 1, crate::value::TAG_UNDEFINED);
    let next = if let Some(proof) = canonical_keys::SharedLayout::of_receiver(obj) {
        canonical_keys::canonicalize(&proof, next, count - 1).view()
    } else {
        ObjectKeys::owned(next)
    };
    set_object_keys(obj, next);
    crate::symbol::symbol_property_ic_epoch_bump();
    true
}

unsafe fn store(obj: *mut ObjectHeader, index: u32, bits: u64) {
    let live = object_live_slot_count(obj);
    if index < live.max(INLINE_SLOT_FLOOR as u32) {
        if index >= live {
            set_object_live_slot_count(obj, index + 1);
        }
        js_object_set_field(obj, index, JSValue::from_bits(bits));
    } else {
        overflow_set(obj as usize, index as usize, bits);
    }
}
