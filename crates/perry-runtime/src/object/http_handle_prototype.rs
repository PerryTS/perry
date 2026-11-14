//! Effective prototypes for registry-backed HTTP request and response objects.
//! HTTP/1 responses are heap objects; HTTP/2 compatibility responses still use
//! registry handles. Native state remains on the handle, while inherited
//! properties follow the effective prototype, including explicit null links.
use crate::value::JSValue;

use super::class_registry::{http_server_handle_kind_probe, HttpServerHandleKind};
use crate::feature_hooks::Hook;

/// `node_stream::is_builtin_stream_method`. Filled where perry-ext-http
/// registers its handle classifier (`js_register_http_server_handle_kind_probe`),
/// the only way [`kind_of`] answers, so [`field`] never meets it empty; a
/// program without an http server handle links no stream family through the
/// generic property read.
static IS_BUILTIN_STREAM_METHOD: Hook<fn(f64, &str) -> bool> = Hook::empty();

pub(crate) fn install_stream_method_probe() {
    IS_BUILTIN_STREAM_METHOD.set(crate::node_stream::is_builtin_stream_method);
}

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

/// Whether the registry handle is a server-side HTTP value.
pub(crate) fn is_http_handle(handle_id: usize) -> bool {
    kind_of(handle_id).is_some()
}

/// A prototype-only read, preserving a present `undefined` member.
pub(crate) fn inherited_field(
    handle_id: usize,
    key: *const crate::StringHeader,
) -> Option<JSValue> {
    if key.is_null() {
        return None;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let key_root = scope.root_nanbox_f64(crate::value::js_nanbox_string(key as i64));
    let name = unsafe {
        std::slice::from_raw_parts(
            (key as *const u8).add(std::mem::size_of::<crate::StringHeader>()),
            (*key).byte_len as usize,
        )
    }
    .to_vec();
    let mut proto = scope.root_nanbox_f64(of_handle(handle_id)?);
    let receiver = f64::from_bits(crate::value::HANDLE_TAG | handle_id as u64);
    loop {
        let value = proto.get_nanbox_f64();
        if !JSValue::from_bits(value.to_bits()).is_pointer() {
            return None;
        }
        // [[Get]] on a Proxy must not run its separate `has` trap.
        if crate::proxy_hooks::proxy_is_proxy(value) != 0 {
            return Some(JSValue::from_bits(
                crate::proxy::js_reflect_get(value, key_root.get_nanbox_f64(), receiver).to_bits(),
            ));
        }
        if let Some(value) = unsafe {
            super::own_property_get_by_bytes(
                JSValue::from_bits(value.to_bits()).as_pointer(),
                &name,
                receiver,
            )
        } {
            return Some(value);
        }
        proto = scope.root_nanbox_f64(super::object_ops::get_prototype_of_resolved(
            proto.get_nanbox_f64(),
        ));
    }
}

/// Resolve a live HTTP handle before generic registry dispatch. Native own
/// state survives prototype replacement; synthetic inherited members do not.
pub(crate) fn field(handle_id: usize, key: *const crate::StringHeader) -> Option<JSValue> {
    let kind = kind_of(handle_id)?;
    if key.is_null() {
        return Some(JSValue::undefined());
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let key_root = scope.root_nanbox_f64(crate::value::js_nanbox_string(key as i64));
    let name = unsafe {
        std::slice::from_raw_parts(
            (key as *const u8).add(std::mem::size_of::<crate::StringHeader>()),
            (*key).byte_len as usize,
        )
    };
    let name_owned = std::str::from_utf8(name).ok()?.to_owned();
    let name = name_owned.as_str();
    let native = || {
        let key =
            (key_root.get_nanbox_u64() & crate::value::POINTER_MASK) as *const crate::StringHeader;
        let ptr = unsafe { (key as *const u8).add(std::mem::size_of::<crate::StringHeader>()) };
        let value = super::class_handles::handle_property_dispatch()
            .map(|dispatch| unsafe { dispatch(handle_id as i64, ptr, name.len()) })
            .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED));
        JSValue::from_bits(value.to_bits())
    };
    if let Some(value) = unsafe {
        super::handle_expando::handle_own_property_get(handle_id as i64, name.as_ptr(), name.len())
    } {
        return Some(JSValue::from_bits(value.to_bits()));
    }
    let own = match kind {
        HttpServerHandleKind::IncomingMessage => matches!(
            name,
            "_events"
                | "_readableState"
                | "_maxListeners"
                | "socket"
                | "client"
                | "httpVersionMajor"
                | "httpVersionMinor"
                | "httpVersion"
                | "complete"
                | "rawHeaders"
                | "rawTrailers"
                | "joinDuplicateHeaders"
                | "aborted"
                | "upgrade"
                | "url"
                | "method"
                | "statusCode"
                | "statusMessage"
                | "_consuming"
                | "_dumped"
        ),
        HttpServerHandleKind::Http2ServerRequest => matches!(
            name,
            "_events" | "_readableState" | "_maxListeners" | "_eventsCount"
        ),
        HttpServerHandleKind::Http2ServerResponse => matches!(
            name,
            "_events" | "_eventsCount" | "_maxListeners" | "writable" | "req"
        ),
        _ => false,
    };
    if own {
        return Some(native());
    }
    let key =
        (key_root.get_nanbox_u64() & crate::value::POINTER_MASK) as *const crate::StringHeader;
    if let Some(value) = inherited_field(handle_id, key) {
        // Stream/emitter intrinsics use heap listener state. Registry HTTP
        // objects must keep using their native listener store instead.
        let is_builtin_stream_method = IS_BUILTIN_STREAM_METHOD.get().expect(
            "an http server handle in a program compiled without the stream runtime family",
        );
        if is_builtin_stream_method(f64::from_bits(value.bits()), name) {
            return Some(native());
        }
        return Some(value);
    }
    let canonical = scope.root_nanbox_f64(native_prototype(handle_id)?);
    let effective = scope.root_nanbox_f64(of_handle(handle_id)?);
    let reaches_native = canonical.get_nanbox_u64() == effective.get_nanbox_u64()
        || unsafe {
            super::native_call_method::js_object_is_prototype_of_value(
                canonical.get_nanbox_f64(),
                effective.get_nanbox_f64(),
            )
        };
    Some(if reaches_native {
        native()
    } else {
        JSValue::undefined()
    })
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
