//! Reflective constructor/prototype surface for `node:async_hooks`.
//!
//! Direct calls on AsyncLocalStorage/AsyncResource are native-dispatched
//! elsewhere. This module supplies the ordinary JS prototype objects so
//! reflection and method-as-value reads see the same functions Node exposes.

use super::callable_exports::set_builtin_closure_length;
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
                && crate::value::addr_class::is_plausible_heap_addr(
                    receiver_value.as_pointer::<u8>() as usize,
                )
            {
                return f64::from_bits(crate::value::TAG_UNDEFINED);
            }
        }

        let args_array = crate::value::js_nanbox_get_pointer(rest);
        crate::object::js_native_call_method_apply(receiver, name_ptr, name_len, args_array)
    }
}

fn attach_prototype(constructor_value: f64, methods: &[(&str, u32)]) -> f64 {
    let constructor_js = JSValue::from_bits(constructor_value.to_bits());
    if !constructor_js.is_pointer() {
        return constructor_value;
    }
    let constructor = constructor_js.as_pointer::<crate::closure::ClosureHeader>() as usize;
    if constructor == 0 {
        return constructor_value;
    }

    // Every allocation below can evacuate the constructor, prototype, method
    // closures, and strings. Keep raw pointers only in updateable roots and
    // reload them immediately before each use.
    let scope = crate::gc::RuntimeHandleScope::new();
    let constructor_handle =
        scope.root_raw_mut_ptr(constructor as *mut crate::closure::ClosureHeader);
    let prototype = js_object_alloc(0, 0);
    if prototype.is_null() {
        return crate::value::js_nanbox_pointer(
            constructor_handle.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64,
        );
    }
    let prototype_handle = scope.root_raw_mut_ptr(prototype);

    let constructor_name = "constructor";
    let constructor_key = crate::string::js_string_from_bytes(
        constructor_name.as_ptr(),
        constructor_name.len() as u32,
    );
    let constructor_key_handle = scope.root_string_ptr(constructor_key);
    js_object_set_field_by_name(
        prototype_handle.get_raw_mut_ptr(),
        constructor_key_handle.get_raw_mut_ptr(),
        crate::value::js_nanbox_pointer(
            constructor_handle.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64,
        ),
    );
    super::super::set_builtin_property_attrs(
        prototype_handle.get_raw_mut_ptr::<ObjectHeader>() as usize,
        constructor_name.to_string(),
        super::super::PropertyAttrs::new(true, false, true),
    );

    let thunk = async_hooks_prototype_method_thunk as *const u8;
    crate::closure::js_register_closure_rest(thunk, 0);
    for &(name, length) in methods {
        let method = crate::closure::js_closure_alloc(thunk, 2);
        if method.is_null() {
            continue;
        }
        let method_handle = scope.root_raw_mut_ptr(method);
        crate::closure::js_closure_set_capture_ptr(
            method_handle.get_raw_mut_ptr(),
            0,
            name.as_ptr() as i64,
        );
        crate::closure::js_closure_set_capture_ptr(
            method_handle.get_raw_mut_ptr(),
            1,
            name.len() as i64,
        );

        let name_string = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let name_handle = scope.root_string_ptr(name_string);
        crate::closure::closure_set_dynamic_prop(
            method_handle.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as usize,
            "name",
            f64::from_bits(JSValue::string_ptr(name_handle.get_raw_mut_ptr()).bits()),
        );
        super::super::set_builtin_property_attrs(
            method_handle.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as usize,
            "name".to_string(),
            super::super::PropertyAttrs::new(false, false, true),
        );
        set_builtin_closure_length(
            method_handle.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as usize,
            length,
        );

        js_object_set_field_by_name(
            prototype_handle.get_raw_mut_ptr(),
            name_handle.get_raw_mut_ptr(),
            crate::value::js_nanbox_pointer(
                method_handle.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64,
            ),
        );
        super::super::set_builtin_property_attrs(
            prototype_handle.get_raw_mut_ptr::<ObjectHeader>() as usize,
            name.to_string(),
            super::super::PropertyAttrs::new(true, false, true),
        );
    }

    crate::closure::closure_set_dynamic_prop(
        constructor_handle.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as usize,
        "prototype",
        crate::value::js_nanbox_pointer(prototype_handle.get_raw_mut_ptr::<ObjectHeader>() as i64),
    );
    super::super::set_builtin_property_attrs(
        constructor_handle.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as usize,
        "prototype".to_string(),
        super::super::PropertyAttrs::new(false, false, false),
    );
    crate::value::js_nanbox_pointer(
        constructor_handle.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64,
    )
}

pub(super) fn attach_async_local_storage_prototype(constructor_value: f64) -> f64 {
    attach_prototype(constructor_value, ASYNC_LOCAL_STORAGE_METHODS)
}

pub(super) fn attach_async_resource_prototype(constructor_value: f64) -> f64 {
    attach_prototype(constructor_value, ASYNC_RESOURCE_METHODS)
}
