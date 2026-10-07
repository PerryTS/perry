//! Descriptor storage uses the existing traced named-property reserve.
//! Ordinary array elements stay in their dense backing. Customized index
//! attributes and accessors are keys of this bag, just like named properties.
use super::*;

pub(crate) unsafe fn array_property_bag(
    arr: *const ArrayHeader,
) -> *mut crate::object::ObjectHeader {
    let header = crate::gc::header_from_trusted_user_ptr(arr.cast());
    if (*header).gc_flags & crate::gc::GC_FLAG_FORWARDED != 0 {
        return array_property_bag(clean_arr_ptr(arr));
    }
    if (*header)._reserved & crate::gc::GC_ARRAY_NAMED_PROPS == 0 {
        return std::ptr::null_mut();
    }
    match reserve_of(arr, (*header)._reserved) {
        Reserve::Bag(bag) => bag,
        _ => std::ptr::null_mut(),
    }
}

pub(crate) unsafe fn names(
    bag: *const crate::object::ObjectHeader,
    enumerable: bool,
) -> Vec<String> {
    let keys = crate::object::object_keys(bag);
    let mut result = Vec::new();
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    for i in 0..keys.count() {
        let entry = crate::object::key_attrs::keys_entry(keys.arr(), i);
        if crate::object::key_attrs::entry_is_private(entry)
            || (enumerable && entry & crate::object::key_attrs::ENTRY_NON_ENUMERABLE != 0)
        {
            continue;
        }
        if let Some(bytes) = crate::string::js_string_key_bytes(keys.get(i), &mut sso) {
            if let Ok(name) = std::str::from_utf8(bytes) {
                result.push(name.to_owned());
            }
        }
    }
    result
}

pub(crate) unsafe fn array_property_bag_ensure(
    arr: *mut ArrayHeader,
) -> *mut crate::object::ObjectHeader {
    // Installs already hold raw owner pointers. Growth leaves a forwarding
    // alias; suppress collection while migrating the existing property slots.
    let _no_move = crate::gc::GcSuppressScope::new();
    let arr = clean_arr_ptr_mut(arr);
    let existing = array_property_bag(arr);
    if !existing.is_null() {
        return existing;
    }
    let props: Vec<(String, f64)> = array_named_property_names(arr, false)
        .into_iter()
        .filter_map(|name| array_named_property_get_by_name(arr, &name).map(|v| (name, v)))
        .collect();
    let arr = if matches!(
        reserve_of(arr, array_object_flags_resolved(arr)),
        Reserve::Inline(..)
    ) {
        materialize_inline(arr)
    } else {
        // Reserving internal storage does not extend the JS property set.
        // Metadata for an existing frozen/sealed property may need a new
        // backing. No user code or collection runs in this no-move window.
        let header = crate::gc::header_from_trusted_user_ptr(arr.cast()).cast_mut();
        let protection = (*header)._reserved
            & (crate::gc::OBJ_FLAG_FROZEN
                | crate::gc::OBJ_FLAG_SEALED
                | crate::gc::OBJ_FLAG_NO_EXTEND);
        (*header)._reserved &= !protection;
        let live = ensure_named_props_slot(arr);
        (*header)._reserved |= protection;
        if !live.is_null() {
            (*crate::gc::header_from_trusted_user_ptr(live.cast()).cast_mut())._reserved |=
                protection;
        }
        live
    };
    assert!(
        !arr.is_null(),
        "descriptor holder must have a property reserve"
    );
    let bag = crate::object::js_object_alloc(0, 0);
    for (name, value) in props {
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        crate::object::object_ops::define_property_force_store_value(bag, key, value);
    }
    FULL_ARRAY_NAMED_PROPS.with(|table| {
        table.borrow_mut().remove(&(arr as usize));
    });
    store_named_props_word(arr, crate::value::js_nanbox_pointer(bag as i64).to_bits());
    mark_array_descriptors(arr);
    bag
}
