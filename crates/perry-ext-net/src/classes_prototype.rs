//! Prototype bodies for the Rust-only net payload families.
use super::*;
use perry_ffi::{JsThis, RawClosureHeader, TransientRootedAddr};
use std::ffi::c_void;

extern "C" {
    fn js_value_to_str_ptr_for_ffi(value: f64) -> i64;
}

fn receiver(scope: &TransientRootScope, this: JsThis) -> TransientRootedAddr {
    scope.root_addr(handle_from_value(this.as_f64()).unwrap_or(0))
}
fn string_arg(scope: &TransientRootScope, value: f64) -> TransientRootedAddr {
    if JsValue::from_bits(value.to_bits()).is_undefined() {
        return scope.root_addr(0);
    }
    scope.root_addr(unsafe { js_value_to_str_ptr_for_ffi(value) })
}

unsafe extern "C" fn add_address(
    _: *const RawClosureHeader,
    this: JsThis,
    address: f64,
    family: f64,
) -> f64 {
    let scope = TransientRootScope::enter();
    let this = scope.root_nanbox(this.as_f64());
    let address = scope.root_nanbox(address);
    let family = scope.root_nanbox(family);
    let owner = receiver(&scope, JsThis::from_f64(this.get()));
    block_list(owner.get());
    if let Some(ip) = socket_address_ip(address.get()) {
        return block_list_insert(owner.get(), BlockRule::Address(ip));
    }
    let address = string_arg(&scope, address.get());
    let family = string_arg(&scope, family.get());
    js_net_block_list_add_address(owner.get(), address.get(), family.get())
}
unsafe extern "C" fn add_range(
    _: *const RawClosureHeader,
    this: JsThis,
    start: f64,
    end: f64,
    family: f64,
) -> f64 {
    let scope = TransientRootScope::enter();
    let this = scope.root_nanbox(this.as_f64());
    let start = scope.root_nanbox(start);
    let end = scope.root_nanbox(end);
    let family = scope.root_nanbox(family);
    let owner = receiver(&scope, JsThis::from_f64(this.get()));
    block_list(owner.get());
    let start_ip = socket_address_ip(start.get());
    let end_ip = socket_address_ip(end.get());
    if start_ip.is_some() || end_ip.is_some() {
        let start_ip = match start_ip {
            Some(ip) => ip,
            None => {
                let start = string_arg(&scope, start.get());
                let family = string_arg(&scope, family.get());
                let ip = parse_ip(start.get(), family.get());
                if family.get() == 0 && ip.is_ipv6() {
                    js_net_throw_invalid_address();
                }
                ip
            }
        };
        let end_ip = match end_ip {
            Some(ip) => ip,
            None => {
                let end = string_arg(&scope, end.get());
                let family = string_arg(&scope, family.get());
                let ip = parse_ip(end.get(), family.get());
                if family.get() == 0 && ip.is_ipv6() {
                    js_net_throw_invalid_address();
                }
                ip
            }
        };
        return block_list_add_range_ip(owner.get(), start_ip, end_ip);
    }
    let start = string_arg(&scope, start.get());
    let end = string_arg(&scope, end.get());
    let family = string_arg(&scope, family.get());
    js_net_block_list_add_range(owner.get(), start.get(), end.get(), family.get())
}
unsafe extern "C" fn add_subnet(
    _: *const RawClosureHeader,
    this: JsThis,
    address: f64,
    prefix: f64,
    family: f64,
) -> f64 {
    let scope = TransientRootScope::enter();
    let this = scope.root_nanbox(this.as_f64());
    let address = scope.root_nanbox(address);
    let prefix = scope.root_nanbox(prefix);
    let family = scope.root_nanbox(family);
    let owner = receiver(&scope, JsThis::from_f64(this.get()));
    block_list(owner.get());
    if let Some(ip) = socket_address_ip(address.get()) {
        return block_list_add_subnet_ip(owner.get(), ip, prefix.get());
    }
    let address = string_arg(&scope, address.get());
    let family = string_arg(&scope, family.get());
    js_net_block_list_add_subnet(owner.get(), address.get(), prefix.get(), family.get())
}
unsafe extern "C" fn check(
    _: *const RawClosureHeader,
    this: JsThis,
    address: f64,
    family: f64,
) -> f64 {
    block_list_check_boxed(this.as_f64(), address, family)
}
unsafe extern "C" fn rules(_: *const RawClosureHeader, this: JsThis) -> f64 {
    js_net_block_list_to_json(handle_from_value(this.as_f64()).unwrap_or(0))
}
unsafe extern "C" fn from_json(_: *const RawClosureHeader, this: JsThis, value: f64) -> f64 {
    let handle = handle_from_value(this.as_f64()).unwrap_or(0);
    js_net_block_list_from_json(handle, value)
}
unsafe extern "C" fn address(_: *const RawClosureHeader, this: JsThis) -> f64 {
    let raw = js_net_socket_address_get_address(handle_from_value(this.as_f64()).unwrap_or(0));
    f64::from_bits(JsValue::from_string_ptr(raw).bits())
}
unsafe extern "C" fn family(_: *const RawClosureHeader, this: JsThis) -> f64 {
    let raw = js_net_socket_address_get_family(handle_from_value(this.as_f64()).unwrap_or(0));
    f64::from_bits(JsValue::from_string_ptr(raw).bits())
}
unsafe extern "C" fn port(_: *const RawClosureHeader, this: JsThis) -> f64 {
    js_net_socket_address_get_port(handle_from_value(this.as_f64()).unwrap_or(0))
}
unsafe extern "C" fn flowlabel(_: *const RawClosureHeader, this: JsThis) -> f64 {
    js_net_socket_address_get_flowlabel(handle_from_value(this.as_f64()).unwrap_or(0))
}

unsafe extern "C" fn address_json(_: *const RawClosureHeader, this: JsThis) -> f64 {
    js_net_socket_address_to_json(handle_from_value(this.as_f64()).unwrap_or(0))
}

#[rustfmt::skip] pub(super) unsafe extern "C" fn install_block_list(raw: *mut c_void) {
    let mut proto = np::PayloadPrototype::from_raw(raw);
    {
        // Public length and body metadata share this declaration.
        perry_ffi::js_function_info!(#[realm("node:net.BlockList.prototype.addAddress", spec = { length = 1 }, emit = metadata)] add_address, 2; with_flags(perry_ffi::FN_BUILTIN); bind realm_info, realm_add_address_metadata);
        proto.method("addAddress", realm_info, realm_add_address_metadata!(length));
    }
    {
        // Public length and body metadata share this declaration.
        perry_ffi::js_function_info!(#[realm("node:net.BlockList.prototype.addRange", spec = { length = 2 }, emit = metadata)] add_range, 3; with_flags(perry_ffi::FN_BUILTIN); bind realm_info, realm_add_range_metadata);
        proto.method("addRange", realm_info, realm_add_range_metadata!(length));
    }
    {
        // Public length and body metadata share this declaration.
        perry_ffi::js_function_info!(#[realm("node:net.BlockList.prototype.addSubnet", spec = { length = 2 }, emit = metadata)] add_subnet, 3; with_flags(perry_ffi::FN_BUILTIN); bind realm_info, realm_add_subnet_metadata);
        proto.method("addSubnet", realm_info, realm_add_subnet_metadata!(length));
    }
    {
        // Public length and body metadata share this declaration.
        perry_ffi::js_function_info!(#[realm("node:net.BlockList.prototype.check", spec = { length = 1 }, emit = metadata)] check, 2; with_flags(perry_ffi::FN_BUILTIN); bind realm_info, realm_check_metadata);
        proto.method("check", realm_info, realm_check_metadata!(length));
    }
    proto.method(
        "toJSON",
        perry_ffi::js_function_info!(#[realm("node:net.BlockList.prototype.toJSON")] rules, 0; with_flags(perry_ffi::FN_BUILTIN)),
        0,
    );
    proto.method(
        "fromJSON",
        perry_ffi::js_function_info!(#[realm("node:net.BlockList.prototype.fromJSON")] from_json, 1; with_flags(perry_ffi::FN_BUILTIN)),
        1,
    );
    proto.getter(
        "rules",
        perry_ffi::js_function_info!(#[realm("node:net.BlockList.prototype.rules.<get>")] rules, 0; with_flags(perry_ffi::FN_BUILTIN)),
    );
}

pub(super) unsafe extern "C" fn install_socket_address(raw: *mut c_void) {
    let mut proto = np::PayloadPrototype::from_raw(raw);
    proto.getter(
        "address",
        perry_ffi::js_function_info!(address, 0; with_flags(perry_ffi::FN_BUILTIN)),
    );
    proto.getter(
        "port",
        perry_ffi::js_function_info!(port, 0; with_flags(perry_ffi::FN_BUILTIN)),
    );
    proto.getter(
        "family",
        perry_ffi::js_function_info!(family, 0; with_flags(perry_ffi::FN_BUILTIN)),
    );
    proto.getter(
        "flowlabel",
        perry_ffi::js_function_info!(flowlabel, 0; with_flags(perry_ffi::FN_BUILTIN)),
    );
    proto.method(
        "toJSON",
        perry_ffi::js_function_info!(address_json, 0; with_flags(perry_ffi::FN_BUILTIN)),
        0,
    );
}
