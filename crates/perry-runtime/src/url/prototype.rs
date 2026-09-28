//! WebIDL URL accessors. The URL's numbered fields are private storage;
//! property lookup and reflection use these descriptors on URL.prototype.

// Only reached through the `global-url` member-table install (see
// `global_this::proto_methods`); a build without that feature links none of it.
#![cfg_attr(not(feature = "global-url"), allow(dead_code))]

use super::parse::*;
use super::url_class::*;
use super::*;

fn require_url_receiver(name: &str, this: crate::closure::JsThis) -> *mut ObjectHeader {
    let this = this.as_f64();
    if let Some(obj) = object_from_f64(this) {
        if is_url_object_shape(obj) {
            return obj;
        }
    }
    let message = format!("Value of URL.prototype.{name} called on an incompatible receiver");
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64));
}

macro_rules! url_getter {
    ($fn_name:ident, $name:literal, $slot:expr) => {
        extern "C" fn $fn_name(
            _closure: *const crate::closure::ClosureHeader,
            this: crate::closure::JsThis,
        ) -> f64 {
            let obj = require_url_receiver($name, this);
            crate::object::js_object_get_field_f64(obj, $slot)
        }
    };
}

macro_rules! url_setter {
    ($fn_name:ident, $name:literal, $setter:path) => {
        extern "C" fn $fn_name(
            _closure: *const crate::closure::ClosureHeader,
            this: crate::closure::JsThis,
            value: f64,
        ) -> f64 {
            let obj = require_url_receiver($name, this);
            $setter(obj, value);
            f64::from_bits(crate::value::TAG_UNDEFINED)
        }
    };
}

url_getter!(get_href, "href", URL_HREF);
url_getter!(get_protocol, "protocol", URL_PROTOCOL);
url_getter!(get_host, "host", URL_HOST);
url_getter!(get_hostname, "hostname", URL_HOSTNAME);
url_getter!(get_port, "port", URL_PORT);
url_getter!(get_pathname, "pathname", URL_PATHNAME);
url_getter!(get_search, "search", URL_SEARCH);
url_getter!(get_hash, "hash", URL_HASH);
url_getter!(get_origin, "origin", URL_ORIGIN);
url_getter!(get_search_params, "searchParams", URL_SEARCH_PARAMS);
url_getter!(get_username, "username", URL_USERNAME);
url_getter!(get_password, "password", URL_PASSWORD);
url_setter!(set_href, "href", js_url_set_href);
url_setter!(set_protocol, "protocol", js_url_set_protocol);
url_setter!(set_host, "host", js_url_set_host);
url_setter!(set_hostname, "hostname", js_url_set_hostname);
url_setter!(set_port, "port", js_url_set_port);
url_setter!(set_pathname, "pathname", js_url_set_pathname);
url_setter!(set_search, "search", js_url_set_search);
url_setter!(set_hash, "hash", js_url_set_hash);
url_setter!(set_username, "username", js_url_set_username);
url_setter!(set_password, "password", js_url_set_password);

pub(crate) fn install_url_prototype_accessors(proto: *mut ObjectHeader) {
    type Info = *const crate::closure::JsFunctionInfo;
    let entries: &[(&str, Info, Option<Info>)] = &[
        (
            "href",
            crate::fn_info!(get_href, 0; with_declared(0)),
            Some(crate::fn_info!(set_href, 1; with_declared(1))),
        ),
        (
            "origin",
            crate::fn_info!(get_origin, 0; with_declared(0)),
            None,
        ),
        (
            "protocol",
            crate::fn_info!(get_protocol, 0; with_declared(0)),
            Some(crate::fn_info!(set_protocol, 1; with_declared(1))),
        ),
        (
            "username",
            crate::fn_info!(get_username, 0; with_declared(0)),
            Some(crate::fn_info!(set_username, 1; with_declared(1))),
        ),
        (
            "password",
            crate::fn_info!(get_password, 0; with_declared(0)),
            Some(crate::fn_info!(set_password, 1; with_declared(1))),
        ),
        (
            "host",
            crate::fn_info!(get_host, 0; with_declared(0)),
            Some(crate::fn_info!(set_host, 1; with_declared(1))),
        ),
        (
            "hostname",
            crate::fn_info!(get_hostname, 0; with_declared(0)),
            Some(crate::fn_info!(set_hostname, 1; with_declared(1))),
        ),
        (
            "port",
            crate::fn_info!(get_port, 0; with_declared(0)),
            Some(crate::fn_info!(set_port, 1; with_declared(1))),
        ),
        (
            "pathname",
            crate::fn_info!(get_pathname, 0; with_declared(0)),
            Some(crate::fn_info!(set_pathname, 1; with_declared(1))),
        ),
        (
            "search",
            crate::fn_info!(get_search, 0; with_declared(0)),
            Some(crate::fn_info!(set_search, 1; with_declared(1))),
        ),
        (
            "searchParams",
            crate::fn_info!(get_search_params, 0; with_declared(0)),
            None,
        ),
        (
            "hash",
            crate::fn_info!(get_hash, 0; with_declared(0)),
            Some(crate::fn_info!(set_hash, 1; with_declared(1))),
        ),
    ];
    let scope = crate::gc::RuntimeHandleScope::new();
    let proto_h = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(proto as i64));
    for &(name, getter, setter) in entries {
        unsafe {
            let get = crate::closure::js_closure_alloc(getter, 0);
            let get_h = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(get as i64));
            crate::object::native_module::set_bound_native_closure_name(
                get,
                &format!("get {name}"),
            );
            let get = crate::value::js_nanbox_get_pointer(get_h.get_nanbox_f64()) as usize;
            crate::object::native_module::set_builtin_closure_length(get, 0);
            crate::object::native_module::set_builtin_closure_non_constructable(get);
            let set_h = setter.map(|func| {
                let set = crate::closure::js_closure_alloc(func, 0);
                let handle = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(set as i64));
                crate::object::native_module::set_bound_native_closure_name(
                    set,
                    &format!("set {name}"),
                );
                let set = crate::value::js_nanbox_get_pointer(handle.get_nanbox_f64()) as usize;
                crate::object::native_module::set_builtin_closure_length(set, 1);
                crate::object::native_module::set_builtin_closure_non_constructable(set);
                handle
            });
            let proto =
                crate::value::js_nanbox_get_pointer(proto_h.get_nanbox_f64()) as *mut ObjectHeader;
            let get_bits = get_h.get_nanbox_f64().to_bits();
            crate::object::install_builtin_getter(proto, name, get_bits);
            let proto =
                crate::value::js_nanbox_get_pointer(proto_h.get_nanbox_f64()) as *mut ObjectHeader;
            crate::object::set_builtin_accessor_descriptor(
                proto as usize,
                name.to_string(),
                crate::object::AccessorDescriptor {
                    get: get_h.get_nanbox_f64().to_bits(),
                    set: set_h.as_ref().map_or(0, |h| h.get_nanbox_f64().to_bits()),
                },
                crate::object::PropertyAttrs::new(true, true, true),
            );
        }
    }
}
