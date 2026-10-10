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

pub(super) unsafe fn property_at(owner: *const ObjectHeader, slot: u32) -> IteratorMethod {
    let keys = crate::object::object_keys(owner);
    let entry = crate::object::key_attrs::keys_entry(keys.arr(), slot);
    let value = crate::object::js_object_get_field(owner, slot).bits();
    if entry & crate::object::key_attrs::ENTRY_ACCESSOR != 0 {
        let pair = js_nanbox_get_pointer(f64::from_bits(value)) as *const super::ArrayHeader;
        return IteratorMethod::Getter(f64::from_bits(super::js_array_get(pair, 0).bits()));
    }
    IteratorMethod::Data(f64::from_bits(value))
}

pub(super) unsafe fn iterator_method_at(owner: *const ObjectHeader, slot: u32) -> IteratorMethod {
    // ConstFn is a data lane: accessor edits revoke it. Check the shape body
    // before reading a value or descriptor that the intrinsic does not need.
    if crate::object::shapes::object_shape_record(owner).is_some_and(|shape| {
        shape.constfn_info(slot).is_some_and(|info| {
            (*(info as *const crate::closure::JsFunctionInfo)).code
                == crate::object::array_prototype_values_thunk as *const u8
        })
    }) {
        return IteratorMethod::Intrinsic;
    }
    property_at(owner, slot)
}

/// Own absence was established by entry; read the prototype's actual slot.
/// An absent member continues the ordinary prototype walk.
pub(super) unsafe fn prototype_iterator_method(proto: usize) -> IteratorMethod {
    // A standalone runtime caller can precede globalThis materialization.
    // The ordinary resolver owns that intrinsic bootstrap; 0 is no owner.
    if proto == 0 {
        return IteratorMethod::Generic;
    }
    let bag = super::array_property_bag(proto as *const super::ArrayHeader);
    let symbol = crate::symbol::well_known_symbol("iterator") as usize;
    if !bag.is_null() {
        if let Some(slot) = crate::object::shaped_symbols::position(bag, symbol) {
            return iterator_method_at(bag, slot);
        }
    }
    IteratorMethod::Generic
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
    let flags = super::array_object_flags_resolved(array);
    let method = if flags & crate::gc::GC_ARRAY_CUSTOM_PROTO != 0 {
        IteratorMethod::Generic
    } else {
        let own = super::array_property_bag(array);
        let symbol = crate::symbol::well_known_symbol("iterator") as usize;
        if let Some(slot) = (!own.is_null())
            .then(|| crate::object::shaped_symbols::position(own, symbol))
            .flatten()
        {
            iterator_method_at(own, slot)
        } else {
            prototype_iterator_method(proto)
        }
    };
    iterator_from_method(value, method)
}

#[cfg(test)]
#[path = "iterator_method_tests.rs"]
mod tests;
