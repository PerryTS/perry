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
    let owner = receiver(&scope, this);
    block_list(owner.get());
    let family = scope.root_nanbox(family);
    let address = string_arg(&scope, address);
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
    let owner = receiver(&scope, this);
    block_list(owner.get());
    let end = scope.root_nanbox(end);
    let family = scope.root_nanbox(family);
    let start = string_arg(&scope, start);
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
    let owner = receiver(&scope, this);
    block_list(owner.get());
    let prefix = scope.root_nanbox(prefix);
    let family = scope.root_nanbox(family);
    let address = string_arg(&scope, address);
    let family = string_arg(&scope, family.get());
    js_net_block_list_add_subnet(owner.get(), address.get(), prefix.get(), family.get())
}
unsafe extern "C" fn check(
    _: *const RawClosureHeader,
    this: JsThis,
    address: f64,
    family: f64,
) -> f64 {
    let scope = TransientRootScope::enter();
    let owner = receiver(&scope, this);
    block_list(owner.get());
    let family = scope.root_nanbox(family);
    let address = string_arg(&scope, address);
    let family = string_arg(&scope, family.get());
    js_net_block_list_check(owner.get(), address.get(), family.get())
}
unsafe extern "C" fn rules(_: *const RawClosureHeader, this: JsThis) -> f64 {
    js_net_block_list_to_json(handle_from_value(this.as_f64()).unwrap_or(0))
}
unsafe extern "C" fn from_json(_: *const RawClosureHeader, this: JsThis, value: f64) -> f64 {
    let handle = handle_from_value(this.as_f64()).unwrap_or(0);
    block_list(handle);
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

pub(super) unsafe extern "C" fn install_block_list(raw: *mut c_void) {
    let mut proto = np::PayloadPrototype::from_raw(raw);
    proto.method(
        "addAddress",
        perry_ffi::js_function_info!(add_address, 2; with_flags(perry_ffi::FN_BUILTIN)),
        1,
    );
    proto.method(
        "addRange",
        perry_ffi::js_function_info!(add_range, 3; with_flags(perry_ffi::FN_BUILTIN)),
        2,
    );
    proto.method(
        "addSubnet",
        perry_ffi::js_function_info!(add_subnet, 3; with_flags(perry_ffi::FN_BUILTIN)),
        2,
    );
    proto.method(
        "check",
        perry_ffi::js_function_info!(check, 2; with_flags(perry_ffi::FN_BUILTIN)),
        1,
    );
    proto.method(
        "toJSON",
        perry_ffi::js_function_info!(rules, 0; with_flags(perry_ffi::FN_BUILTIN)),
        0,
    );
    proto.method(
        "fromJSON",
        perry_ffi::js_function_info!(from_json, 1; with_flags(perry_ffi::FN_BUILTIN)),
        1,
    );
    proto.getter(
        "rules",
        perry_ffi::js_function_info!(rules, 0; with_flags(perry_ffi::FN_BUILTIN)),
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
