//! Array GetMethod from the same receiver/property shapes used by entry.
use crate::object::ObjectHeader;
use crate::value::{js_nanbox_get_pointer, TAG_UNDEFINED};

/// A resolved property value or getter is consumed before the next reentry.
/// No owner pointer or slot index survives a user call.
pub(super) enum IteratorMethod {
    Intrinsic,
    Data(f64),
    Getter(f64),
    Generic,
}

/// The method at key position `pos` of `owner`. A position names its value
/// through the key's storage slot (or its overflow entry), which need not be
/// the position itself once a key was deleted and defined again.
pub(super) unsafe fn property_at(owner: *const ObjectHeader, pos: u32) -> IteratorMethod {
    let keys = crate::object::object_keys(owner);
    let entry = crate::object::key_attrs::keys_entry(keys.arr(), pos);
    let table = crate::object::overflow::table(owner);
    let value = if table.is_null() {
        crate::object::js_object_get_field(owner, keys.storage_slot(pos)).bits()
    } else {
        crate::object::overflow::get_from(table, owner, pos).unwrap_or(TAG_UNDEFINED)
    };
    if entry & crate::object::key_attrs::ENTRY_ACCESSOR != 0 {
        let acc = crate::object::accessor_pair::pair_of_value(value).unwrap_or_default();
        return IteratorMethod::Getter(f64::from_bits(if acc.get == 0 {
            TAG_UNDEFINED
        } else {
            acc.get
        }));
    }
    IteratorMethod::Data(f64::from_bits(value))
}

// Every array named family declares this inline slot, including absent
// properties. Its address is a layout fact; the generic object-field reader
// need not re-prove the bound or search overflow for each iterator entry.
#[inline(always)]
unsafe fn iterator_slot_value(owner: *const ObjectHeader, slot: u32) -> u64 {
    // A1_UNPROVEN: record-first: no proof of the object form at this slot read (kind-A/B question for S0f).
    *crate::meta_slots_of!(crate::meta_obj_runtime_born!(owner as *mut _))
        .cast::<u64>()
        .add(slot as usize)
}

#[cfg(test)]
pub(super) unsafe fn iterator_method_at(owner: *const ObjectHeader, slot: u32) -> IteratorMethod {
    iterator_method_from_slot(owner, slot, iterator_slot_value(owner, slot))
}

unsafe fn iterator_method_from_slot(
    owner: *const ObjectHeader,
    slot: u32,
    value: u64,
) -> IteratorMethod {
    let keys = crate::object::object_keys(owner);
    let entry = crate::object::key_attrs::keys_entry(keys.arr(), slot);
    if entry & crate::object::key_attrs::ENTRY_ACCESSOR != 0 {
        let acc = crate::object::accessor_pair::pair_of_value(value).unwrap_or_default();
        return IteratorMethod::Getter(f64::from_bits(if acc.get == 0 {
            TAG_UNDEFINED
        } else {
            acc.get
        }));
    }
    let value = f64::from_bits(value);
    let raw = js_nanbox_get_pointer(value) as *const crate::closure::ClosureHeader;
    if crate::closure::closure_info(raw)
        .is_some_and(|info| info.code == crate::object::array_prototype_values_thunk as *const u8)
    {
        return IteratorMethod::Intrinsic;
    }
    IteratorMethod::Data(value)
}

/// Every array named holder declares iterator slot 0. A hole means absent;
/// deleting a property cannot relocate this slot.
pub(super) unsafe fn prototype_iterator_method(proto: usize) -> IteratorMethod {
    if proto == 0 {
        return IteratorMethod::Intrinsic;
    }
    let named = super::array_property_bag(proto as *const super::ArrayHeader);
    if named.is_null() {
        return IteratorMethod::Generic;
    }
    let slot = crate::object::runtime_layouts::ARRAY_PROTOTYPE_ITERATOR_SLOT;
    let value = iterator_slot_value(named, slot);
    if value == crate::value::TAG_HOLE {
        // A deleted own member continues on the prototype chain.
        IteratorMethod::Generic
    } else {
        iterator_method_from_slot(named, slot, value)
    }
}

pub(crate) unsafe fn array_iterator_method(
    array: *const super::ArrayHeader,
    proto: usize,
) -> IteratorMethod {
    if super::array_object_flags_resolved(array) & crate::gc::GC_ARRAY_CUSTOM_PROTO != 0 {
        return IteratorMethod::Generic;
    }
    let own = super::array_property_bag_resolved(array);
    let slot = crate::object::runtime_layouts::ARRAY_PROTOTYPE_ITERATOR_SLOT;
    if !own.is_null() {
        let value = iterator_slot_value(own, slot);
        if value != crate::value::TAG_HOLE {
            return iterator_method_from_slot(own, slot, value);
        }
        // A hole is absence only for a holder without an overflow; with one
        // (the shadow bit), the member may be an overflow entry (an accessor
        // moves out of its fixed slot), so the complete GetMethod decides.
        if crate::object::overflow::has_overflow(own) {
            return IteratorMethod::Generic;
        }
    }
    prototype_iterator_method(proto)
}

/// Resolve a descriptor with the receiver already in custody. The caller
/// retains it across the getter and refreshes it for the following Call.
pub(super) unsafe fn read_method(
    receiver: &crate::gc::RuntimeHandle<'_>,
    method: IteratorMethod,
) -> f64 {
    match method {
        IteratorMethod::Data(method) => method,
        IteratorMethod::Getter(getter) => {
            if getter.to_bits() == TAG_UNDEFINED {
                getter
            } else {
                crate::closure::native_call_value_this(
                    getter,
                    crate::closure::JsThis::from_f64(receiver.get_nanbox_f64()),
                    std::ptr::null(),
                    0,
                )
            }
        }
        IteratorMethod::Generic => {
            let symbol = crate::symbol::well_known_symbol("iterator");
            crate::symbol::js_object_get_symbol_property(
                receiver.get_nanbox_f64(),
                crate::value::js_nanbox_pointer(symbol as i64),
            )
        }
        IteratorMethod::Intrinsic => unreachable!(),
    }
}

pub(super) unsafe fn iterator_from_method(value: f64, method: IteratorMethod) -> f64 {
    if matches!(method, IteratorMethod::Intrinsic) {
        return super::array_values_iter(value);
    }
    if let IteratorMethod::Data(method) = method {
        return crate::symbol::js_iterator_result_validate(crate::closure::native_call_value_this(
            method,
            crate::closure::JsThis::from_f64(value),
            std::ptr::null(),
            0,
        ));
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(value);
    let method = read_method(&receiver, method);
    // The single explicit-this dispatcher validates Call and roots only when
    // binding needs allocation. No clone/rebind or callable probe is repeated.
    let iter = crate::closure::native_call_value_this(
        method,
        crate::closure::JsThis::from_f64(receiver.get_nanbox_f64()),
        std::ptr::null(),
        0,
    );
    crate::symbol::js_iterator_result_validate(iter)
}

/// The generic GetIterator caller has already established a dense array.
pub(crate) unsafe fn array_shape_iterator(value: f64) -> f64 {
    let proto = super::array_prototype_addr_if_resolved();
    if proto != 0 {
        return array_shape_iterator_resolved(value, proto);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(value);
    let proto = super::array_prototype_addr();
    array_shape_iterator_resolved(receiver.get_nanbox_f64(), proto)
}

unsafe fn array_shape_iterator_resolved(value: f64, proto: usize) -> f64 {
    let array = super::clean_arr_ptr(js_nanbox_get_pointer(value) as *const super::ArrayHeader);
    let value = crate::value::js_nanbox_pointer(array as i64);
    let method = array_iterator_method(array, proto);
    iterator_from_method(value, method)
}

#[cfg(test)]
#[path = "iterator_method_tests.rs"]
mod tests;
