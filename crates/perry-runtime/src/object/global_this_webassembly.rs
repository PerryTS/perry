//! WebAssembly pieces of the `globalThis` namespace installer.

use super::*;

#[cfg(feature = "wasm-host")]
extern "C" fn webassembly_validate_thunk(
    _closure: *const crate::closure::ClosureHeader,
    bytes: f64,
) -> f64 {
    crate::webassembly::js_webassembly_validate(bytes)
}

#[cfg(not(feature = "wasm-host"))]
extern "C" fn webassembly_validate_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _bytes: f64,
) -> f64 {
    f64::from_bits(crate::value::JSValue::bool(false).bits())
}

#[cfg(feature = "wasm-host")]
extern "C" fn webassembly_instantiate_thunk(
    _closure: *const crate::closure::ClosureHeader,
    bytes: f64,
) -> f64 {
    crate::webassembly::js_webassembly_instantiate(bytes)
}

#[cfg(not(feature = "wasm-host"))]
extern "C" fn webassembly_instantiate_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _bytes: f64,
) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

extern "C" fn webassembly_unsupported_static_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _arg: f64,
) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

pub(super) fn create_webassembly_namespace() -> f64 {
    let ns_obj = js_object_alloc(0, 0);
    if ns_obj.is_null() {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }

    let module_ctor = install_webassembly_constructor(ns_obj, "Module");
    if !module_ctor.is_null() {
        install_webassembly_static_fn(
            module_ctor as *mut ObjectHeader,
            "exports",
            global_this_builtin_noop_thunk as *const u8,
            1,
            true,
        );
        install_webassembly_static_fn(
            module_ctor as *mut ObjectHeader,
            "imports",
            global_this_builtin_noop_thunk as *const u8,
            1,
            true,
        );
        install_webassembly_static_fn(
            module_ctor as *mut ObjectHeader,
            "customSections",
            global_this_builtin_noop_thunk as *const u8,
            2,
            true,
        );
    }

    let instance_ctor = install_webassembly_constructor(ns_obj, "Instance");
    install_webassembly_proto_data(
        instance_ctor,
        "exports",
        f64::from_bits(crate::value::TAG_UNDEFINED),
    );

    let memory_ctor = install_webassembly_constructor(ns_obj, "Memory");
    install_webassembly_proto_data(
        memory_ctor,
        "buffer",
        f64::from_bits(crate::value::TAG_UNDEFINED),
    );
    install_webassembly_proto_method(memory_ctor, "grow", 1);

    let table_ctor = install_webassembly_constructor(ns_obj, "Table");
    install_webassembly_proto_method(table_ctor, "get", 1);
    install_webassembly_proto_method(table_ctor, "grow", 1);
    install_webassembly_proto_data(
        table_ctor,
        "length",
        f64::from_bits(crate::value::TAG_UNDEFINED),
    );
    install_webassembly_proto_method(table_ctor, "set", 2);

    let global_ctor = install_webassembly_constructor(ns_obj, "Global");
    install_webassembly_proto_data(
        global_ctor,
        "value",
        f64::from_bits(crate::value::TAG_UNDEFINED),
    );
    install_webassembly_proto_method(global_ctor, "valueOf", 0);

    for name in [
        "CompileError",
        "LinkError",
        "RuntimeError",
        "Exception",
        "Tag",
    ] {
        let ctor = install_webassembly_constructor(ns_obj, name);
        if matches!(name, "CompileError" | "LinkError" | "RuntimeError") {
            install_webassembly_error_proto_data(ctor, name);
        }
    }

    install_webassembly_object_property(
        ns_obj,
        "JSTag",
        super::super::PropertyAttrs::new(false, false, true),
    );

    install_webassembly_static_fn(
        ns_obj,
        "compile",
        webassembly_unsupported_static_thunk as *const u8,
        1,
        true,
    );
    install_webassembly_static_fn(
        ns_obj,
        "validate",
        webassembly_validate_thunk as *const u8,
        1,
        true,
    );
    install_webassembly_static_fn(
        ns_obj,
        "instantiate",
        webassembly_instantiate_thunk as *const u8,
        1,
        true,
    );
    install_webassembly_static_fn(
        ns_obj,
        "compileStreaming",
        webassembly_unsupported_static_thunk as *const u8,
        1,
        true,
    );
    install_webassembly_static_fn(
        ns_obj,
        "instantiateStreaming",
        webassembly_unsupported_static_thunk as *const u8,
        1,
        true,
    );
    install_webassembly_static_fn(
        ns_obj,
        "promising",
        webassembly_unsupported_static_thunk as *const u8,
        1,
        true,
    );

    crate::value::js_nanbox_pointer(ns_obj as i64)
}

fn install_webassembly_constructor(
    ns_obj: *mut ObjectHeader,
    name: &str,
) -> *mut crate::closure::ClosureHeader {
    if ns_obj.is_null() {
        return std::ptr::null_mut();
    }
    let closure = crate::closure::js_closure_alloc(global_this_builtin_noop_thunk as *const u8, 0);
    if closure.is_null() {
        return std::ptr::null_mut();
    }
    super::super::native_module::set_bound_native_closure_name(closure, name);
    super::super::native_module::set_builtin_closure_length(closure as usize, 1);
    super::super::set_builtin_property_attrs(
        closure as usize,
        "name".to_string(),
        super::super::PropertyAttrs::new(false, false, true),
    );
    super::super::set_builtin_property_attrs(
        closure as usize,
        "length".to_string(),
        super::super::PropertyAttrs::new(false, false, true),
    );

    let proto_obj = js_object_alloc(0, 0);
    if !proto_obj.is_null() {
        let proto_key = crate::string::js_string_from_bytes(b"prototype".as_ptr(), 9);
        let proto_value = crate::value::js_nanbox_pointer(proto_obj as i64);
        js_object_set_field_by_name(closure as *mut ObjectHeader, proto_key, proto_value);
        super::super::set_builtin_property_attrs(
            closure as usize,
            "prototype".to_string(),
            super::super::PropertyAttrs::new(false, false, false),
        );

        let ctor_key = crate::string::js_string_from_bytes(b"constructor".as_ptr(), 11);
        let ctor_value = crate::value::js_nanbox_pointer(closure as i64);
        js_object_set_field_by_name(proto_obj, ctor_key, ctor_value);
        super::super::set_builtin_property_attrs(
            proto_obj as usize,
            "constructor".to_string(),
            super::super::PropertyAttrs::new(true, false, true),
        );
    }

    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let value = crate::value::js_nanbox_pointer(closure as i64);
    js_object_set_field_by_name(ns_obj, key, value);
    super::super::set_builtin_property_attrs(
        ns_obj as usize,
        name.to_string(),
        super::super::PropertyAttrs::new(true, false, true),
    );
    closure
}

fn install_webassembly_static_fn(
    obj: *mut ObjectHeader,
    name: &str,
    func_ptr: *const u8,
    arity: u32,
    enumerable: bool,
) {
    if obj.is_null() {
        return;
    }
    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    if closure.is_null() {
        return;
    }
    crate::closure::js_register_closure_arity(func_ptr, arity);
    super::super::native_module::set_bound_native_closure_name(closure, name);
    super::super::native_module::set_builtin_closure_length(closure as usize, arity);
    super::super::set_builtin_property_attrs(
        closure as usize,
        "name".to_string(),
        super::super::PropertyAttrs::new(false, false, true),
    );
    super::super::set_builtin_property_attrs(
        closure as usize,
        "length".to_string(),
        super::super::PropertyAttrs::new(false, false, true),
    );
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let value = crate::value::js_nanbox_pointer(closure as i64);
    js_object_set_field_by_name(obj, key, value);
    super::super::set_builtin_property_attrs(
        obj as usize,
        name.to_string(),
        super::super::PropertyAttrs::new(true, enumerable, true),
    );
}

fn webassembly_constructor_proto(ctor: *mut crate::closure::ClosureHeader) -> *mut ObjectHeader {
    if ctor.is_null() {
        return std::ptr::null_mut();
    }
    let value = crate::closure::closure_get_dynamic_prop(ctor as usize, "prototype");
    let jsv = crate::value::JSValue::from_bits(value.to_bits());
    if jsv.is_pointer() {
        jsv.as_pointer::<ObjectHeader>() as *mut ObjectHeader
    } else {
        std::ptr::null_mut()
    }
}

fn install_webassembly_proto_method(
    ctor: *mut crate::closure::ClosureHeader,
    name: &str,
    arity: u32,
) {
    let proto = webassembly_constructor_proto(ctor);
    if proto.is_null() {
        return;
    }
    install_proto_method(
        proto,
        name,
        global_this_builtin_noop_thunk as *const u8,
        arity,
    );
}

fn install_webassembly_proto_data(
    ctor: *mut crate::closure::ClosureHeader,
    name: &str,
    value: f64,
) {
    let proto = webassembly_constructor_proto(ctor);
    if proto.is_null() {
        return;
    }
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    js_object_set_field_by_name(proto, key, value);
    super::super::set_builtin_property_attrs(
        proto as usize,
        name.to_string(),
        super::super::PropertyAttrs::new(false, false, true),
    );
}

fn install_webassembly_error_proto_data(ctor: *mut crate::closure::ClosureHeader, name: &str) {
    let proto = webassembly_constructor_proto(ctor);
    if proto.is_null() {
        return;
    }
    let name_key = crate::string::js_string_from_bytes(b"name".as_ptr(), 4);
    let name_string = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    js_object_set_field_by_name(
        proto,
        name_key,
        crate::value::js_nanbox_string(name_string as i64),
    );
    super::super::set_builtin_property_attrs(
        proto as usize,
        "name".to_string(),
        super::super::PropertyAttrs::new(true, false, true),
    );

    let message_key = crate::string::js_string_from_bytes(b"message".as_ptr(), 7);
    let message_string = crate::string::js_string_from_bytes(b"".as_ptr(), 0);
    js_object_set_field_by_name(
        proto,
        message_key,
        crate::value::js_nanbox_string(message_string as i64),
    );
    super::super::set_builtin_property_attrs(
        proto as usize,
        "message".to_string(),
        super::super::PropertyAttrs::new(true, false, true),
    );
}

fn install_webassembly_object_property(
    ns_obj: *mut ObjectHeader,
    name: &str,
    attrs: super::super::PropertyAttrs,
) {
    if ns_obj.is_null() {
        return;
    }
    let obj = js_object_alloc(0, 0);
    if obj.is_null() {
        return;
    }
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let value = crate::value::js_nanbox_pointer(obj as i64);
    js_object_set_field_by_name(ns_obj, key, value);
    super::super::set_builtin_property_attrs(ns_obj as usize, name.to_string(), attrs);
}
