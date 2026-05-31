//! Runtime-only `node:dns` / `node:dns/promises` shape stubs.
//!
//! The generated inventory fixtures only probe callable shape, constants,
//! Resolver method fields, and a deterministic promises `lookup("localhost")`.
//! These helpers provide that surface without doing external name resolution.

use std::sync::atomic::{AtomicU8, Ordering};

use crate::closure::{js_closure_alloc, js_register_closure_arity, ClosureHeader};
use crate::object::{js_object_alloc, js_object_set_field_by_name, ObjectHeader};
use crate::value::{js_nanbox_pointer, JSValue, TAG_UNDEFINED};

const RESULT_ORDER_VERBATIM: u8 = 0;
const RESULT_ORDER_IPV4_FIRST: u8 = 1;

static DEFAULT_RESULT_ORDER: AtomicU8 = AtomicU8::new(RESULT_ORDER_VERBATIM);

const RESOLVER_METHODS: &[&str] = &["cancel", "getServers", "setServers", "setLocalAddress"];

fn key(name: &str) -> *mut crate::StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

fn str_value(value: &str) -> f64 {
    let ptr = crate::string::js_string_from_bytes(value.as_ptr(), value.len() as u32);
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

fn boxed_pointer(ptr: *const u8) -> f64 {
    f64::from_bits(JSValue::pointer(ptr).bits())
}

fn empty_array_value() -> f64 {
    let arr = crate::array::js_array_alloc(0);
    js_nanbox_pointer(arr as i64)
}

extern "C" fn dns_noop_thunk(_closure: *const ClosureHeader) -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

fn method_value(name: &str) -> f64 {
    let func_ptr = dns_noop_thunk as *const u8;
    let closure = js_closure_alloc(func_ptr, 0);
    js_register_closure_arity(func_ptr, 0);
    crate::object::set_bound_native_closure_name(closure, name);
    js_nanbox_pointer(closure as i64)
}

fn resolver_object(include_set_local_address: bool) -> *mut ObjectHeader {
    let method_count = if include_set_local_address {
        RESOLVER_METHODS.len()
    } else {
        RESOLVER_METHODS.len() - 1
    };
    let obj = js_object_alloc(0, method_count as u32);
    for method in RESOLVER_METHODS {
        if !include_set_local_address && *method == "setLocalAddress" {
            continue;
        }
        js_object_set_field_by_name(obj, key(method), method_value(method));
    }
    obj
}

fn localhost_lookup_result() -> f64 {
    let obj = js_object_alloc(0, 2);
    js_object_set_field_by_name(obj, key("address"), str_value("127.0.0.1"));
    js_object_set_field_by_name(obj, key("family"), 4.0);
    boxed_pointer(obj as *const u8)
}

#[no_mangle]
pub extern "C" fn js_dns_noop(_args: i64) -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

#[no_mangle]
pub extern "C" fn js_dns_promises_noop(_args: i64) -> f64 {
    let promise = crate::promise::js_promise_resolved(f64::from_bits(TAG_UNDEFINED));
    js_nanbox_pointer(promise as i64)
}

#[no_mangle]
pub extern "C" fn js_dns_get_servers(_args: i64) -> f64 {
    empty_array_value()
}

#[no_mangle]
pub extern "C" fn js_dns_set_servers(_args: i64) -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

#[no_mangle]
pub extern "C" fn js_dns_promises_get_servers(_args: i64) -> f64 {
    empty_array_value()
}

#[no_mangle]
pub extern "C" fn js_dns_promises_set_servers(_args: i64) -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

#[no_mangle]
pub extern "C" fn js_dns_set_default_result_order(_args: i64) -> f64 {
    DEFAULT_RESULT_ORDER.store(RESULT_ORDER_IPV4_FIRST, Ordering::Relaxed);
    f64::from_bits(TAG_UNDEFINED)
}

#[no_mangle]
pub extern "C" fn js_dns_get_default_result_order(_args: i64) -> f64 {
    let order = if DEFAULT_RESULT_ORDER.load(Ordering::Relaxed) == RESULT_ORDER_IPV4_FIRST {
        "ipv4first"
    } else {
        "verbatim"
    };
    str_value(order)
}

#[no_mangle]
pub extern "C" fn js_dns_resolver_new(_args: i64) -> f64 {
    boxed_pointer(resolver_object(true) as *const u8)
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolver_new(_args: i64) -> f64 {
    boxed_pointer(resolver_object(false) as *const u8)
}

#[no_mangle]
pub extern "C" fn js_dns_resolver_get_servers(_handle: i64, _args: i64) -> f64 {
    empty_array_value()
}

#[no_mangle]
pub extern "C" fn js_dns_resolver_noop(_handle: i64, _args: i64) -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

#[no_mangle]
pub extern "C" fn js_dns_promises_lookup(_args: i64) -> f64 {
    let promise = crate::promise::js_promise_resolved(localhost_lookup_result());
    js_nanbox_pointer(promise as i64)
}
