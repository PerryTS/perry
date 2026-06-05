//! `Promise.prototype` method thunks with receiver brand checks.
//!
//! Instance value-reads (`const f = promise.then`) intentionally return
//! bound methods elsewhere so common deferred use keeps the original promise.
//! The prototype methods themselves are different: `Promise.prototype.then`
//! is a built-in function that must reject an incompatible `this` value.
//! These thunks cover reflective calls (`.call`/`.apply`) and bare detached
//! prototype calls by reading `IMPLICIT_THIS`, validating that it is a Promise,
//! and then delegating to the existing Promise chaining helpers.

use super::*;

pub(super) fn install_promise_proto_methods(proto_obj: *mut ObjectHeader) {
    use super::global_this::install_proto_method as ipm;

    ipm(
        proto_obj,
        "catch",
        promise_proto_catch_thunk as *const u8,
        1,
    );
    ipm(
        proto_obj,
        "finally",
        promise_proto_finally_thunk as *const u8,
        1,
    );
    ipm(proto_obj, "then", promise_proto_then_thunk as *const u8, 2);
}

fn throw_incompatible_receiver(method: &str) -> ! {
    let msg = format!("Method Promise.prototype.{method} called on incompatible receiver");
    let s = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_typeerror_new(s);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

#[inline]
fn promise_receiver_or_throw(method: &str) -> *mut crate::promise::Promise {
    let receiver = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    if crate::promise::js_value_is_promise(receiver) == 0 {
        throw_incompatible_receiver(method);
    }
    crate::value::js_nanbox_get_pointer(receiver) as *mut crate::promise::Promise
}

#[inline]
fn handler_arg_to_closure(value: f64) -> crate::promise::ClosurePtr {
    let bits = value.to_bits();
    let tag = bits & crate::value::TAG_MASK;
    let candidate = if tag == crate::value::POINTER_TAG {
        bits & crate::value::POINTER_MASK
    } else if tag == 0 {
        bits
    } else {
        0
    } as usize;

    if crate::closure::is_closure_ptr(candidate) {
        candidate as crate::promise::ClosurePtr
    } else {
        std::ptr::null()
    }
}

extern "C" fn promise_proto_then_thunk(
    _closure: *const crate::closure::ClosureHeader,
    on_fulfilled: f64,
    on_rejected: f64,
) -> f64 {
    let promise = promise_receiver_or_throw("then");
    let next = crate::promise::js_promise_then(
        promise,
        handler_arg_to_closure(on_fulfilled),
        handler_arg_to_closure(on_rejected),
    );
    crate::value::js_nanbox_pointer(next as i64)
}

extern "C" fn promise_proto_catch_thunk(
    _closure: *const crate::closure::ClosureHeader,
    on_rejected: f64,
) -> f64 {
    let promise = promise_receiver_or_throw("catch");
    let next = crate::promise::js_promise_catch(promise, handler_arg_to_closure(on_rejected));
    crate::value::js_nanbox_pointer(next as i64)
}

extern "C" fn promise_proto_finally_thunk(
    _closure: *const crate::closure::ClosureHeader,
    on_finally: f64,
) -> f64 {
    let promise = promise_receiver_or_throw("finally");
    let next = crate::promise::js_promise_finally(promise, handler_arg_to_closure(on_finally));
    crate::value::js_nanbox_pointer(next as i64)
}
