//! Reflective constructor/prototype surface for `node:async_hooks`.
//!
//! Direct calls on AsyncLocalStorage/AsyncResource are native-dispatched
//! elsewhere. This module supplies the ordinary JS prototype objects so
//! reflection and method-as-value reads see the same functions Node exposes.

use super::callable_exports::{set_bound_native_closure_name, set_builtin_closure_length};
use super::*;

const ASYNC_LOCAL_STORAGE_METHODS: &[(&str, u32)] = &[
    ("run", 2),
    ("getStore", 0),
    ("enterWith", 1),
    ("exit", 1),
    ("disable", 0),
];

const ASYNC_RESOURCE_METHODS: &[(&str, u32)] = &[
    ("asyncId", 0),
    ("triggerAsyncId", 0),
    ("emitDestroy", 0),
    ("runInAsyncScope", 2),
    ("bind", 2),
];

/// Forward a prototype method call through the existing dynamic receiver
/// dispatcher. The rest array preserves every variadic argument for
/// `run`, `exit`, and `runInAsyncScope`.
extern "C" fn async_hooks_prototype_method_thunk(
    closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    unsafe {
        let name_ptr = crate::closure::js_closure_get_capture_ptr(closure, 0) as *const i8;
        let name_len = crate::closure::js_closure_get_capture_ptr(closure, 1) as usize;
        let receiver = crate::object::js_implicit_this_get();
        let name = std::slice::from_raw_parts(name_ptr as *const u8, name_len);

        // Node's enterWith/disable implementations do not brand-check an
        // arbitrary object receiver; they simply have no observable storage
        // state to mutate there. Preserve that no-op behavior instead of
        // asking the generic object dispatcher to call a missing method.
        if matches!(name, b"enterWith" | b"disable") {
            let receiver_value = JSValue::from_bits(receiver.to_bits());
            if receiver_value.is_pointer()
                && !crate::value::addr_class::is_handle_band(
                    receiver_value.as_pointer::<u8>() as usize
                )
            {
                return f64::from_bits(crate::value::TAG_UNDEFINED);
            }
        }

        let args_array = crate::value::js_nanbox_get_pointer(rest);
        crate::object::js_native_call_method_apply(receiver, name_ptr, name_len, args_array)
    }
}

fn attach_prototype(constructor_value: f64, methods: &[(&str, u32)]) {
    let constructor_js = JSValue::from_bits(constructor_value.to_bits());
    if !constructor_js.is_pointer() {
        return;
    }
    let constructor = constructor_js.as_pointer::<crate::closure::ClosureHeader>() as usize;
    if constructor == 0 {
        return;
    }

    let prototype = js_object_alloc(0, 0);
    if prototype.is_null() {
        return;
    }

    let constructor_name = "constructor";
    let constructor_key = crate::string::js_string_from_bytes(
        constructor_name.as_ptr(),
        constructor_name.len() as u32,
    );
    js_object_set_field_by_name(prototype, constructor_key, constructor_value);
    super::super::set_builtin_property_attrs(
        prototype as usize,
        constructor_name.to_string(),
        super::super::PropertyAttrs::new(true, false, true),
    );

    let thunk = async_hooks_prototype_method_thunk as *const u8;
    crate::closure::js_register_closure_rest(thunk, 0);
    for &(name, length) in methods {
        let leaked: &'static [u8] = name.as_bytes().to_vec().leak();
        let method = crate::closure::js_closure_alloc(thunk, 2);
        if method.is_null() {
            continue;
        }
        crate::closure::js_closure_set_capture_ptr(method, 0, leaked.as_ptr() as i64);
        crate::closure::js_closure_set_capture_ptr(method, 1, leaked.len() as i64);
        set_bound_native_closure_name(method, name);
        set_builtin_closure_length(method as usize, length);

        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        js_object_set_field_by_name(
            prototype,
            key,
            crate::value::js_nanbox_pointer(method as i64),
        );
        super::super::set_builtin_property_attrs(
            prototype as usize,
            name.to_string(),
            super::super::PropertyAttrs::new(true, false, true),
        );
    }

    crate::closure::closure_set_dynamic_prop(
        constructor,
        "prototype",
        crate::value::js_nanbox_pointer(prototype as i64),
    );
    super::super::set_builtin_property_attrs(
        constructor,
        "prototype".to_string(),
        super::super::PropertyAttrs::new(false, false, false),
    );
}

pub(super) fn attach_async_local_storage_prototype(constructor_value: f64) {
    attach_prototype(constructor_value, ASYNC_LOCAL_STORAGE_METHODS);
}

pub(super) fn attach_async_resource_prototype(constructor_value: f64) {
    attach_prototype(constructor_value, ASYNC_RESOURCE_METHODS);
}
