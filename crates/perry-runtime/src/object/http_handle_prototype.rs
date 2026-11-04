//! The [[Prototype]] of a live node:http / node:http2 server `req`/`res`
//! (#13802).
//!
//! A live server's request and response are perry-ext-http registry handles,
//! not heap objects, so nothing links them to a prototype object. The
//! extension registers a classifier (`js_register_http_server_handle_kind_probe`)
//! naming which constructor's `.prototype` a handle has: `ServerResponse` /
//! `IncomingMessage` for http, `Http2ServerResponse` / `Http2ServerRequest`
//! for the http2 compat API. That prototype is what `Object.getPrototypeOf`
//! answers and what a property read or method call the native dispatch does
//! not know falls back to, so helpers patched onto
//! `Http2ServerResponse.prototype` reach `res`.
//!
//! A prototype the program recorded with `Object.setPrototypeOf(res, proto)`
//! (Express's `app.response`) takes precedence; that is the existing
//! handle-prototype walk, which [`of_handle`] reports and the field fallbacks
//! leave to their callers.
use crate::value::JSValue;

use super::class_registry::{http_server_handle_kind_probe, HttpServerHandleKind};

/// The kind of perry-ext-http server handle `handle_id` is, if any.
fn kind_of(handle_id: usize) -> Option<HttpServerHandleKind> {
    let probe = http_server_handle_kind_probe()?;
    HttpServerHandleKind::from_probe(unsafe { probe(handle_id as i64) })
}

/// The node prototype of the http server handle `handle_id`, ignoring any
/// recorded prototype. `None` for any other handle.
fn native_prototype(handle_id: usize) -> Option<f64> {
    let (module, export) = kind_of(handle_id)?.constructor();
    let proto = super::object_ops::native_export_prototype_value(module, export);
    JSValue::from_bits(proto.to_bits())
        .is_pointer()
        .then_some(proto)
}

/// `Object.getPrototypeOf(handle)` for an http server handle: its recorded
/// prototype, else its node prototype. `None` for any other handle.
pub(crate) fn of_handle(handle_id: usize) -> Option<f64> {
    kind_of(handle_id)?;
    if let Some(bits) = super::prototype_chain::object_static_prototype(handle_id) {
        return Some(f64::from_bits(bits));
    }
    native_prototype(handle_id)
}

/// Does `handle_id` inherit from its node prototype: an http server handle
/// with no recorded prototype.
pub(crate) fn inherits_natively(handle_id: usize) -> bool {
    kind_of(handle_id).is_some()
        && super::prototype_chain::object_static_prototype(handle_id).is_none()
}

/// `handle[key]` read from the node prototype of an http server handle with
/// no recorded prototype: an ordinary [[Get]] with the handle as receiver, so
/// an accessor or a patched method sees `this === res`. `None` for another
/// handle, a handle with a recorded prototype, or a key the chain lacks.
pub(crate) fn inherited_field(
    handle_id: usize,
    key: *const crate::StringHeader,
) -> Option<JSValue> {
    if key.is_null() || !inherits_natively(handle_id) {
        return None;
    }
    // The key is rooted first: building the prototype on first use allocates.
    let scope = crate::gc::RuntimeHandleScope::new();
    let key = scope.root_nanbox_f64(crate::value::js_nanbox_string(key as i64));
    let proto = scope.root_nanbox_f64(native_prototype(handle_id)?);
    let receiver = f64::from_bits(crate::value::HANDLE_TAG | handle_id as u64);
    let value =
        crate::proxy::js_reflect_get(proto.get_nanbox_f64(), key.get_nanbox_f64(), receiver);
    let value = JSValue::from_bits(value.to_bits());
    (!value.is_undefined()).then_some(value)
}

/// `handle.constructor` once the handle dispatch answered `dispatched`: the
/// constructor a dispatcher knows (an http server handle's, #13802), else the
/// empty stub every other registry handle reads, so `data.constructor.name`
/// duck-typing stays safe.
pub(crate) fn constructor_or_stub(dispatched: f64) -> JSValue {
    let value = JSValue::from_bits(dispatched.to_bits());
    if value.is_pointer() {
        value
    } else {
        JSValue::from_bits(super::null_stub_value().to_bits())
    }
}
