//! Response methods installed as their own bodies on the sealed HTTP prototypes.
//! The receiver word proves the response layout; the attached payload proves
//! the native owner. Other receivers keep the ordinary method dispatch.
use super::handle_dispatch::*;
use super::types::TAG_UNDEFINED;
use perry_ffi::TransientRootScope;
use perry_ffi::{JsFunctionInfo, JsThis, RawClosureHeader};

extern "C" {
    fn js_node_http_response_layout_receiver(receiver: f64) -> i32;
    fn js_node_http_method_fallback(
        c: *const RawClosureHeader,
        this: JsThis,
        a: f64,
        b: f64,
        d: f64,
    ) -> f64;
}

macro_rules! method {
    ($body:ident, $op:ident, $min:expr, $server_only:expr) => {
        extern "C" fn $body(
            c: *const RawClosureHeader,
            this: JsThis,
            a: f64,
            b: f64,
            d: f64,
        ) -> f64 {
            unsafe {
                let args = [a, b, d];
                let len = args
                    .iter()
                    .rposition(|v| v.to_bits() != TAG_UNDEFINED)
                    .map_or(0, |i| i + 1);
                let handle = (this.as_f64().to_bits() & super::types::PTR_MASK) as i64;
                if len.checked_sub($min).is_none()
                    || js_node_http_response_layout_receiver(this.as_f64()) == 0
                    || !super::response_payload::is_response(handle)
                    || ($server_only
                        && super::response::response_state(handle)
                            .is_some_and(|s| s.outgoing_message_only))
                {
                    return js_node_http_method_fallback(c, this, a, b, d);
                }
                $op(handle, &args[..len])
            }
        }
    };
}
method!(set_header_body, set_header, 1, false);
#[rustfmt::skip] perry_ffi::realm_decl! { info perry_ffi::fn_info!(export static SET_HEADER = crate::server::response_methods::set_header_body, 3; with_declared(3)); realm { "node:http.OutgoingMessage.prototype.setHeader", capture = property_key, constructible = true, spec = {length = 2}, emit = metadata(realm_http_set_header_public); } }
method!(write_head_body, write_head, 1, true);
#[rustfmt::skip] perry_ffi::realm_decl! { info perry_ffi::fn_info!(export static WRITE_HEAD = crate::server::response_methods::write_head_body, 3; with_declared(3)); realm { "node:http.ServerResponse.prototype.writeHead", capture = property_key, constructible = true; } }
method!(write_body, write, 1, false);
#[rustfmt::skip] perry_ffi::realm_decl! { info perry_ffi::fn_info!(export static WRITE = crate::server::response_methods::write_body, 3; with_declared(3)); realm { "node:http.OutgoingMessage.prototype.write", capture = property_key, constructible = true; } }
method!(end_body, end, 0, false);
#[rustfmt::skip] perry_ffi::realm_decl! { info perry_ffi::fn_info!(export static END = crate::server::response_methods::end_body, 3; with_declared(3)); realm { "node:http.OutgoingMessage.prototype.end", capture = property_key, constructible = true; } }

/// Cold prototype construction protocol of the existing HTTP export provider.
/// The result is immutable image metadata, never a JS value or a heap edge.
pub(super) fn info(name: &str) -> f64 {
    let info = match name {
        "setHeader" => &SET_HEADER,
        "writeHead" => &WRITE_HEAD,
        "write" => &WRITE,
        "end" => &END,
        _ => return f64::from_bits(TAG_UNDEFINED),
    };
    f64::from_bits(info as *const JsFunctionInfo as u64)
}

pub(super) unsafe fn set_header(handle: i64, args: &[f64]) -> f64 {
    let response_scope = TransientRootScope::enter();
    let response_root = response_scope.root_addr(handle);

    let value = response_scope.root_nanbox(
        args.get(1)
            .copied()
            .unwrap_or(f64::from_bits(TAG_UNDEFINED)),
    );
    let name = string_value_arg(args[0]);
    if !name.is_null() {
        // Pass the raw JSValue so array values (Set-Cookie) keep their
        // per-element structure for one-line-per-element wire output.
        super::response::js_node_http_res_set_header(response_root.get(), name, value.get());
    }
    handle_to_pointer_f64(response_root.get())
}

pub(super) unsafe fn write_head(handle: i64, args: &[f64]) -> f64 {
    let response_scope = TransientRootScope::enter();
    let response_root = response_scope.root_addr(handle);

    super::response::js_node_http_res_write_head(
        response_root.get(),
        number_arg(Some(args[0]), 200.0),
        raw_arg(args.get(1).copied()),
        raw_arg(args.get(2).copied()),
    );
    handle_to_pointer_f64(response_root.get())
}

pub(super) unsafe fn write(handle: i64, args: &[f64]) -> f64 {
    let response_scope = TransientRootScope::enter();
    let response_root = response_scope.root_addr(handle);

    // `write(chunk[, encoding][, callback])` — the callback is the
    // last closure-valued arg (#4904).
    let cb = args[1..]
        .iter()
        .rev()
        .map(|a| closure_arg(Some(*a)))
        .find(|c| *c != 0)
        .unwrap_or(0);
    bool_value(
        crate::server::response::js_node_http_res_write_with_cb(response_root.get(), args[0], cb)
            != 0,
    )
}

pub(super) unsafe fn end(handle: i64, args: &[f64]) -> f64 {
    let response_scope = TransientRootScope::enter();
    let response_root = response_scope.root_addr(handle);
    let undef = f64::from_bits(TAG_UNDEFINED);

    // `end([chunk][, encoding][, callback])` — `end(cb)` passes the
    // callback first (#4904).
    let first = args.first().copied().unwrap_or(undef);
    let first_cb = closure_arg(Some(first));
    let (chunk, cb) = if first_cb != 0 {
        (undef, first_cb)
    } else {
        let cb = args
            .get(1..)
            .unwrap_or(&[])
            .iter()
            .rev()
            .map(|a| closure_arg(Some(*a)))
            .find(|c| *c != 0)
            .unwrap_or(0);
        (first, cb)
    };
    crate::server::response_end::js_node_http_res_end_with_cb(response_root.get(), chunk, cb);
    handle_to_pointer_f64(response_root.get())
}

#[cfg(test)]
mod tests {
    use super::*;
    use perry_runtime::{gc, object, value};

    unsafe fn get(receiver: f64, name: &str) -> f64 {
        let key = perry_runtime::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let key = value::js_nanbox_string(key as i64);
        perry_runtime::proxy::js_reflect_get(receiver, key, receiver)
    }

    #[test]
    fn response_method_bodies_are_installed_and_legacy_handles_fall_back() {
        // HTTP prototype identity is per agent. A fresh agent exercises cold
        // construction even when another test already materialized a prototype.
        std::thread::Builder::new()
            .stack_size(16 << 20)
            .spawn(|| unsafe {
                gc::initialize_runtime_fixture();
                super::super::native_dispatch::js_ext_http_nm_install();
                let scope = gc::RuntimeHandleScope::new();
                let response = scope.root_nanbox_f64(value::js_nanbox_pointer(
                    super::super::response::js_node_http_server_response_standalone_new(
                        f64::from_bits(TAG_UNDEFINED),
                    ),
                ));
                let receiver = || response.get_nanbox_f64();
                assert_eq!(
                    js_node_http_response_layout_receiver(receiver()),
                    1,
                    "the direct proof must be live"
                );
                for (name, info) in [
                    ("setHeader", &SET_HEADER),
                    ("writeHead", &WRITE_HEAD),
                    ("write", &WRITE),
                    ("end", &END),
                ] {
                    let method = get(receiver(), name);
                    let c = (method.to_bits() & super::super::types::PTR_MASK)
                        as *const perry_runtime::closure::ClosureHeader;
                    assert_eq!(
                        (*c).info as usize,
                        info as *const JsFunctionInfo as usize,
                        "{name} must enter its own body"
                    );
                }
                let method = scope.root_nanbox_f64(get(receiver(), "setHeader"));
                let key = scope.root_nanbox_f64(value::js_nanbox_string(
                    perry_runtime::string::js_string_from_bytes(b"x-direct".as_ptr(), 8) as i64,
                ));
                let call = || {
                    perry_ffi::call_value(
                        method.get_nanbox_f64(),
                        JsThis::from_f64(receiver()),
                        &[key.get_nanbox_f64(), 42.0],
                    )
                };
                assert_eq!(call().to_bits(), receiver().to_bits());
                let handle = || (receiver().to_bits() & super::super::types::PTR_MASK) as i64;
                assert_eq!(
                    super::super::response::js_node_http_res_get_header(handle(), perry_ffi::string_arg_ptr(key.get_nanbox_f64())),
                    42.0
                );
                let field = perry_runtime::string::js_string_from_bytes(b"extra".as_ptr(), 5);
                object::js_object_set_field_by_name(
                    (receiver().to_bits() & super::super::types::PTR_MASK)
                        as *mut object::ObjectHeader,
                    field,
                    1.0,
                );
                assert_eq!(js_node_http_response_layout_receiver(receiver()), 1,
                "own-key growth preserves the ordinary response carrier; property lookup chooses the method");
                assert_eq!(call().to_bits(), receiver().to_bits());
                let other = scope.root_nanbox_f64(value::js_nanbox_pointer(
                    object::js_object_alloc(0, 0) as i64,
                ));
                let mut state = super::super::response::ResponseState::new();
                state.standalone = true;
                assert!(super::super::response_payload::attach_to(
                    other.get_nanbox_f64(),
                    state,
                    f64::from_bits(TAG_UNDEFINED)
                ));
                assert_eq!(
                    js_node_http_response_layout_receiver(other.get_nanbox_f64()),
                    1,
                    "the current ordinary layout and payload type prove an attached carrier"
                );
                let attached_result = perry_ffi::call_value(
                    method.get_nanbox_f64(),
                    JsThis::from_f64(other.get_nanbox_f64()),
                    &[key.get_nanbox_f64(), 43.0],
                );
                assert_eq!(attached_result.to_bits(), other.get_nanbox_f64().to_bits());
                let other_handle =
                    (other.get_nanbox_f64().to_bits() & super::super::types::PTR_MASK) as i64;
                assert_eq!(
                    super::super::response::js_node_http_res_get_header(other_handle, perry_ffi::string_arg_ptr(key.get_nanbox_f64())),
                    43.0
                );
                let legacy = perry_ffi::register_handle(
                    super::super::response::ServerResponse::new(),
                );
                let legacy_value = value::js_nanbox_pointer(legacy);
                assert_eq!(
                    js_node_http_response_layout_receiver(legacy_value), 0,
                    "a legacy handle has no ordinary layout word",
                );
                let legacy_result = perry_ffi::call_value(
                    method.get_nanbox_f64(), JsThis::from_f64(legacy_value),
                    &[key.get_nanbox_f64(), 44.0],
                );
                assert_eq!(
                    legacy_result.to_bits(), legacy_value.to_bits(),
                    "the complete fallback must remain live",
                );
                assert_eq!(
                    super::super::response::js_node_http_res_get_header(legacy, perry_ffi::string_arg_ptr(key.get_nanbox_f64())),
                    44.0,
                );
                perry_ffi::take_handle::<super::super::response::ServerResponse>(legacy);
            })
            .unwrap()
            .join()
            .unwrap();
    }
}

// Source-only realm facts beside the owning builtin bodies.
macro_rules! realm_decl {
    ($facts:literal) => {};
}
