//! Runtime-only `node:dgram` shape stubs.
//!
//! Perry does not implement UDP sockets yet, but the generated Node inventory
//! fixture only probes namespace/socket method shapes and closes the stub. These
//! helpers provide that surface without claiming packet IO support.

use crate::closure::{js_closure_alloc, js_register_closure_arity, ClosureHeader};
use crate::object::{js_object_alloc, js_object_set_field_by_name, ObjectHeader};
use crate::value::{js_nanbox_pointer, JSValue, TAG_UNDEFINED};

const SOCKET_METHODS: &[&str] = &[
    "send",
    "bind",
    "close",
    "address",
    "connect",
    "disconnect",
    "addMembership",
    "dropMembership",
    "setBroadcast",
    "setMulticastTTL",
    "setMulticastLoopback",
    "setMulticastInterface",
    "setTTL",
    "setRecvBufferSize",
    "setSendBufferSize",
    "getRecvBufferSize",
    "getSendBufferSize",
    "ref",
    "unref",
];

fn key(name: &str) -> *mut crate::StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

fn boxed_pointer(ptr: *const u8) -> f64 {
    f64::from_bits(JSValue::pointer(ptr).bits())
}

extern "C" fn dgram_socket_method_thunk(_closure: *const ClosureHeader) -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

fn method_value(name: &str) -> f64 {
    let func_ptr = dgram_socket_method_thunk as *const u8;
    let closure = js_closure_alloc(func_ptr, 0);
    js_register_closure_arity(func_ptr, 0);
    crate::object::set_bound_native_closure_name(closure, name);
    js_nanbox_pointer(closure as i64)
}

fn socket_object() -> *mut ObjectHeader {
    let obj = js_object_alloc(0, SOCKET_METHODS.len() as u32);
    for method in SOCKET_METHODS {
        js_object_set_field_by_name(obj, key(method), method_value(method));
    }
    obj
}

#[no_mangle]
pub extern "C" fn js_dgram_create_socket(_args: i64) -> f64 {
    boxed_pointer(socket_object() as *const u8)
}

#[no_mangle]
pub extern "C" fn js_dgram_socket_noop(_handle: i64, _args: i64) -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}
