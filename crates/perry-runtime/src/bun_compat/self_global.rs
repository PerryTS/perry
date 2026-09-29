//! Bun's replaceable `self` accessor (#10306). The platform hook runs before
//! every module, but must never undo a user's assignment or deletion.

use crate::closure::{js_closure_alloc, ClosureHeader};
use crate::object::{AccessorDescriptor, ObjectHeader, PropertyAttrs};
use crate::value::{js_nanbox_pointer, JSValue};

crate::perry_thread_local! {
    static INSTALLED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub(super) fn install_once() {
    if INSTALLED.with(|installed| installed.replace(true)) {
        return;
    }
    // Bootstrap executes no user code. Keep its raw global/key/closure pointers
    // in one no-move window, as the main globalThis bootstrap does.
    let _no_move = crate::gc::GcSuppressScope::new();
    let global = JSValue::from_bits(crate::object::js_get_global_this().to_bits())
        .as_pointer::<ObjectHeader>()
        .cast_mut();
    let getter_fn = crate::fn_info!(get_self, 0; with_declared(0));
    let setter_fn = crate::fn_info!(set_self, 1; with_declared(1));
    let getter = js_closure_alloc(getter_fn, 0);
    let setter = js_closure_alloc(setter_fn, 0);
    for (closure, name, arity) in [(getter, "get", 0), (setter, "set", 1)] {
        crate::object::native_module::set_bound_native_closure_name(closure, name);
        crate::object::native_module::set_builtin_closure_length(closure as usize, arity);
        crate::object::native_module::set_builtin_closure_non_constructable(closure as usize);
    }
    let key = crate::string::js_string_from_bytes(b"self".as_ptr(), 4);
    crate::object::define_builtin_data_property(
        global,
        key,
        js_nanbox_pointer(global as i64),
        "self".into(),
        PropertyAttrs::new(true, true, true),
    );
    crate::object::set_builtin_accessor_descriptor(
        global as usize,
        "self".into(),
        AccessorDescriptor {
            get: js_nanbox_pointer(getter as i64).to_bits(),
            set: js_nanbox_pointer(setter as i64).to_bits(),
        },
        PropertyAttrs::new(true, true, true),
    );
}

extern "C" fn get_self(_closure: *const ClosureHeader, _this: crate::closure::JsThis) -> f64 {
    crate::object::js_get_global_this()
}

extern "C" fn set_self(
    _closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    value: f64,
) -> f64 {
    // Like Bun, even a borrowed setter replaces the current realm's property.
    // Once replaced, subsequent assignments are ordinary writable data stores.
    let _no_move = crate::gc::GcSuppressScope::new();
    let global = JSValue::from_bits(crate::object::js_get_global_this().to_bits())
        .as_pointer::<ObjectHeader>()
        .cast_mut();
    let key = crate::string::js_string_from_bytes(b"self".as_ptr(), 4);
    crate::object::clear_accessor_descriptor(global as usize, "self");
    crate::object::define_builtin_data_property(
        global,
        key,
        value,
        "self".into(),
        PropertyAttrs::new(true, true, true),
    );
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bun_self_global_installs_in_an_existing_realm_once() {
        super::super::platform::reset_bun_platform_for_test();
        let _no_move = crate::gc::GcSuppressScope::new();
        let global_value = crate::object::js_get_global_this();
        let global = JSValue::from_bits(global_value.to_bits())
            .as_pointer::<ObjectHeader>()
            .cast_mut();
        let key = crate::string::js_string_from_bytes(b"self".as_ptr(), 4);
        assert!(crate::object::js_object_get_field_by_name(global, key).is_undefined());

        super::super::js_set_bun_platform();
        let descriptor = crate::object::get_accessor_descriptor(global as usize, "self").unwrap();
        assert_ne!(descriptor.get, 0);
        assert_ne!(descriptor.set, 0);
        assert_eq!(
            crate::object::js_object_get_field_by_name(global, key).bits(),
            global_value.to_bits()
        );

        set_self(std::ptr::null(), crate::closure::JsThis::UNDEFINED, 42.0);
        assert!(crate::object::get_accessor_descriptor(global as usize, "self").is_none());
        super::super::js_set_bun_platform();
        assert_eq!(
            crate::object::js_object_get_field_by_name(global, key).as_number(),
            42.0
        );

        let key_value = crate::value::js_nanbox_string(key as i64);
        assert_ne!(
            crate::object::js_object_delete_dynamic(global, key_value),
            0
        );
        super::super::js_set_bun_platform();
        assert!(crate::object::js_object_get_field_by_name(global, key).is_undefined());

        // The once guard belongs to the realm, not the process-wide platform
        // flag. A second thread must install its own alias after this one did.
        assert!(std::thread::spawn(|| {
            super::super::js_set_bun_platform();
            let _no_move = crate::gc::GcSuppressScope::new();
            let global_value = crate::object::js_get_global_this();
            let global = JSValue::from_bits(global_value.to_bits())
                .as_pointer::<ObjectHeader>()
                .cast_mut();
            let key = crate::string::js_string_from_bytes(b"self".as_ptr(), 4);
            crate::object::js_object_get_field_by_name(global, key).bits() == global_value.to_bits()
        })
        .join()
        .unwrap());
        super::super::platform::reset_bun_platform_for_test();
    }
}
