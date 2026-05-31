//! Minimal Web messaging globals used by Node-compatible `globalThis` and
//! `node:worker_threads` constructor identity.

use crate::closure::{js_closure_alloc, js_register_closure_arity, ClosureHeader};
use crate::object::{self, ObjectHeader};
use crate::string::{js_string_from_bytes, StringHeader};
use crate::value::JSValue;

fn js_undefined() -> f64 {
    f64::from_bits(JSValue::undefined().bits())
}

fn js_null() -> f64 {
    f64::from_bits(JSValue::null().bits())
}

fn js_bool(value: bool) -> f64 {
    f64::from_bits(JSValue::bool(value).bits())
}

fn boxed_object(obj: *mut ObjectHeader) -> f64 {
    crate::value::js_nanbox_pointer(obj as i64)
}

fn key(name: &str) -> *mut StringHeader {
    js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

fn set_field(obj: *mut ObjectHeader, name: &str, value: f64) {
    object::js_object_set_field_by_name(obj, key(name), value);
}

fn get_global_constructor(name: &str) -> f64 {
    let global = object::js_get_global_this();
    let global_obj = crate::value::js_nanbox_get_pointer(global) as *const ObjectHeader;
    if global_obj.is_null() {
        return js_undefined();
    }
    object::js_object_get_field_by_name_f64(global_obj, key(name))
}

fn constructor_prototype(name: &str) -> f64 {
    let ctor = get_global_constructor(name);
    let ctor_ptr = crate::value::js_nanbox_get_pointer(ctor) as usize;
    if ctor_ptr == 0 {
        return js_undefined();
    }
    crate::closure::closure_get_dynamic_prop(ctor_ptr, "prototype")
}

fn set_object_prototype(obj: *mut ObjectHeader, prototype: f64) {
    if obj.is_null() {
        return;
    }
    if crate::value::js_nanbox_get_pointer(prototype) != 0 {
        object::prototype_chain::object_set_static_prototype(obj as usize, prototype.to_bits());
    }
}

fn closure_value(func_ptr: *const u8, name: &str, arity: u32) -> f64 {
    js_register_closure_arity(func_ptr, arity);
    let closure = js_closure_alloc(func_ptr, 0);
    object::set_bound_native_closure_name(closure, name);
    object::set_builtin_closure_length(closure as usize, arity);
    crate::value::js_nanbox_pointer(closure as i64)
}

extern "C" fn noop0(_closure: *const ClosureHeader) -> f64 {
    js_undefined()
}

extern "C" fn noop1(_closure: *const ClosureHeader, _arg0: f64) -> f64 {
    js_undefined()
}

extern "C" fn noop2(_closure: *const ClosureHeader, _arg0: f64, _arg1: f64) -> f64 {
    js_undefined()
}

extern "C" fn has_ref(_closure: *const ClosureHeader) -> f64 {
    js_bool(false)
}

fn install_method(obj: *mut ObjectHeader, name: &str, func_ptr: *const u8, arity: u32) {
    set_field(obj, name, closure_value(func_ptr, name, arity));
}

/// Install the Node-shaped prototype members for the three messaging
/// constructors. The method bodies are intentionally small no-ops here; this
/// slice is about constructor identity and shape, while full message delivery
/// remains worker_threads parity follow-up work.
pub fn populate_messaging_prototype(builtin_name: &str, proto: *mut ObjectHeader, ctor: f64) {
    if proto.is_null() {
        return;
    }
    set_field(proto, "constructor", ctor);
    object::set_builtin_property_attrs(
        proto as usize,
        "constructor".to_string(),
        object::PropertyAttrs::new(true, false, true),
    );

    match builtin_name {
        "MessagePort" => {
            install_method(proto, "postMessage", noop2 as *const u8, 2);
            install_method(proto, "start", noop0 as *const u8, 0);
            install_method(proto, "ref", noop0 as *const u8, 0);
            install_method(proto, "unref", noop0 as *const u8, 0);
            install_method(proto, "hasRef", has_ref as *const u8, 0);
            set_field(proto, "onmessage", js_null());
            set_field(proto, "onmessageerror", js_null());
            install_method(proto, "close", noop0 as *const u8, 0);
        }
        "MessageChannel" => {}
        "BroadcastChannel" => {
            set_field(proto, "name", js_undefined());
            install_method(proto, "close", noop0 as *const u8, 0);
            install_method(proto, "postMessage", noop1 as *const u8, 1);
            install_method(proto, "ref", noop0 as *const u8, 0);
            install_method(proto, "unref", noop0 as *const u8, 0);
            set_field(proto, "onmessage", js_null());
            set_field(proto, "onmessageerror", js_null());
        }
        _ => {}
    }
}

fn message_port_object() -> *mut ObjectHeader {
    let obj = object::js_object_alloc(0, 0);
    let proto = constructor_prototype("MessagePort");
    set_object_prototype(obj, proto);
    set_field(obj, "constructor", get_global_constructor("MessagePort"));
    set_field(obj, "onmessage", js_null());
    set_field(obj, "onmessageerror", js_null());
    obj
}

#[no_mangle]
pub extern "C" fn js_message_channel_new() -> f64 {
    let obj = object::js_object_alloc(0, 0);
    set_object_prototype(obj, constructor_prototype("MessageChannel"));
    set_field(obj, "constructor", get_global_constructor("MessageChannel"));
    set_field(obj, "port1", boxed_object(message_port_object()));
    set_field(obj, "port2", boxed_object(message_port_object()));
    boxed_object(obj)
}

pub(crate) extern "C" fn js_message_channel_constructor_call_error(
    _closure: *const ClosureHeader,
) -> f64 {
    throw_constructor_call_error()
}

#[no_mangle]
pub extern "C" fn js_broadcast_channel_new(name: f64) -> f64 {
    let obj = object::js_object_alloc(0, 0);
    set_object_prototype(obj, constructor_prototype("BroadcastChannel"));
    set_field(
        obj,
        "constructor",
        get_global_constructor("BroadcastChannel"),
    );
    let name_ptr = crate::builtins::js_string_coerce(name);
    let name_value = f64::from_bits(JSValue::string_ptr(name_ptr).bits());
    set_field(obj, "name", name_value);
    set_field(obj, "onmessage", js_null());
    set_field(obj, "onmessageerror", js_null());
    boxed_object(obj)
}

pub(crate) extern "C" fn js_broadcast_channel_constructor_call_error(
    _closure: *const ClosureHeader,
    _arg: f64,
) -> f64 {
    throw_constructor_call_error()
}

pub(crate) extern "C" fn js_message_port_constructor_call_error(
    _closure: *const ClosureHeader,
) -> f64 {
    throw_constructor_call_error()
}

#[no_mangle]
pub extern "C" fn js_message_port_constructor_error() -> f64 {
    throw_constructor_call_error()
}

fn throw_constructor_call_error() -> f64 {
    let msg = js_string_from_bytes(b"Constructor cannot be called".as_ptr(), 28);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}
