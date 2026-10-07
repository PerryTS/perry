//! Ordinary named properties live in the cell's shaped null-prototype bag.
use super::store;

pub fn buffer_get_own_prop(addr: usize, prop: &str) -> Option<f64> {
    // This public probe also receives addresses released by thread teardown.
    if addr == 0 || !super::header::header_is_owned(addr) {
        return None;
    }
    unsafe { store::bag_get(addr, prop) }
}

pub fn buffer_define_own_data_prop(addr: usize, prop: &str, value: f64) {
    if addr == 0 {
        return;
    }
    crate::typedarray_named::note_named_mutation(addr, prop.as_bytes());
    unsafe {
        store::bag_set(addr, prop, value, false);
    }
}

pub fn buffer_set_own_prop(addr: usize, prop: &str, value: f64) {
    if addr == 0 {
        return;
    }
    if let Some(accessor) = crate::object::get_accessor_descriptor(addr, prop) {
        if accessor.set != 0 {
            unsafe {
                crate::object::invoke_accessor_setter(
                    accessor.set,
                    crate::value::js_nanbox_pointer(addr as i64),
                    value,
                );
            }
        }
        return;
    }
    let exists = buffer_has_own_prop(addr, prop);
    if exists && crate::object::get_property_attrs(addr, prop).is_some_and(|a| !a.writable()) {
        return;
    }
    if !exists && unsafe { (*store::header(addr))._reserved & crate::gc::OBJ_FLAG_NO_EXTEND != 0 } {
        return;
    }
    buffer_define_own_data_prop(addr, prop, value);
}

pub fn buffer_read_own_prop(addr: usize, prop: &str) -> Option<f64> {
    if addr == 0 {
        return None;
    }
    if let Some(accessor) = crate::object::get_accessor_descriptor(addr, prop) {
        if accessor.get == 0 {
            return Some(f64::from_bits(crate::value::TAG_UNDEFINED));
        }
        let scope = crate::gc::RuntimeHandleScope::new();
        let receiver = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(addr as i64));
        let getter = scope.root_nanbox_u64(accessor.get);
        let value = unsafe {
            crate::object::invoke_accessor_getter(
                getter.get_nanbox_u64(),
                receiver.get_nanbox_f64(),
            )
        };
        return Some(f64::from_bits(value.bits()));
    }
    buffer_get_own_prop(addr, prop)
}

pub fn buffer_has_own_prop(addr: usize, prop: &str) -> bool {
    if addr == 0 {
        return false;
    }
    unsafe {
        let bag = store::bag(addr);
        store::own_slot(bag, prop.as_bytes()).is_some_and(|slot| {
            crate::object::object_field_at_with_live(
                bag,
                slot,
                crate::object::object_live_slot_count(bag),
            )
            .bits()
                != crate::value::TAG_HOLE
        })
    }
}

pub fn buffer_own_prop_names(addr: usize) -> Vec<String> {
    if addr == 0 {
        return Vec::new();
    }
    unsafe {
        let bag = store::bag(addr);
        if bag.is_null() {
            return Vec::new();
        }
        let keys = crate::object::object_keys(bag);
        let mut result = Vec::new();
        for i in 0..keys.count() {
            let key = crate::array::js_array_get_f64(keys.arr(), i);
            let mut short = [0; crate::value::SHORT_STRING_MAX_LEN];
            let Some(bytes) = crate::string::js_string_key_bytes(
                crate::value::JSValue::from_bits(key.to_bits()),
                &mut short,
            ) else {
                continue;
            };
            if crate::object::field_get_set::enumeration::is_internal_runtime_key_bytes(bytes) {
                continue;
            }
            let live = crate::object::object_field_at_with_live(
                bag,
                i,
                crate::object::object_live_slot_count(bag),
            );
            if live.bits() == crate::value::TAG_HOLE {
                continue;
            }
            result.push(String::from_utf8_lossy(bytes).into_owned());
        }
        result
    }
}

pub fn buffer_delete_own_prop(addr: usize, prop: &str) -> bool {
    if !buffer_has_own_prop(addr, prop) {
        return false;
    }
    let _suppress = crate::gc::GcSuppressScope::new();
    unsafe {
        let bag = store::bag(addr);
        let key = crate::string::js_string_from_bytes(prop.as_ptr(), prop.len() as u32);
        return crate::object::js_object_delete_field(bag, key) != 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shaped_engine_keys_keep_owner_edges_immutable_to_public_writes() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _suppress = crate::gc::GcSuppressScope::new();
        let owner = super::super::buffer_alloc(16) as usize;
        let view =
            store::new_view(crate::gc::GC_TYPE_BUFFER_UINT8ARRAY, owner, 0, 16, false) as usize;
        buffer_set_own_prop(view, "tag", 1.0);
        let attrs = crate::object::get_property_attrs(view, store::VIEW_OWNER_KEY).unwrap();
        assert!(!attrs.writable() && !attrs.enumerable() && !attrs.configurable());
        buffer_set_own_prop(view, store::VIEW_OWNER_KEY, 0.0);
        assert!(!buffer_delete_own_prop(view, store::VIEW_OWNER_KEY));
        assert_eq!(unsafe { store::owner(view) }, owner);
        unsafe {
            store::bag_set(owner, store::PIN_OVERFLOW_KEY, 1.0, true);
            store::bag_set(owner, store::PIN_OVERFLOW_KEY, 2.0, true);
            assert_eq!(store::bag_get(owner, store::PIN_OVERFLOW_KEY), Some(2.0));
        }
    }

    #[test]
    fn making_engine_keys_publicly_mutable_turns_the_invariant_red() {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "buffer::own_props::tests::shaped_engine_keys_keep_owner_edges_immutable_to_public_writes", "--nocapture"])
            .env("PERRY_B4_SABOTAGE", "private_key_descriptor").output().unwrap();
        assert!(String::from_utf8_lossy(&child.stdout).contains("running 1 test"));
        assert!(!child.status.success());
    }

    #[test]
    fn own_property_names_preserve_creation_order() {
        let _lock = crate::gc::global_side_table_test_lock();
        let owner = super::super::buffer_alloc(0) as usize;
        buffer_define_own_data_prop(owner, "second", 2.0);
        buffer_define_own_data_prop(owner, "first", 1.0);
        buffer_define_own_data_prop(owner, "second", 22.0);
        assert_eq!(
            buffer_own_prop_names(owner),
            ["second", "first"],
            "updating a property must keep its original position"
        );

        assert!(buffer_delete_own_prop(owner, "second"));
        buffer_define_own_data_prop(owner, "second", 222.0);
        assert_eq!(
            buffer_own_prop_names(owner),
            ["first", "second"],
            "deleting and recreating a property must append it"
        );
    }
}
