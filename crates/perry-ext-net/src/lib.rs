//! Ordinary net.Socket and net.Server owners with link-routed transports.
//! Rust payloads own resources; callbacks and protocol owners are traced JS edges.
use perry_ffi::{alloc_buffer, alloc_string, JsValue, StringHeader, TransientRootScope};
mod bun_tcp;
mod ip;
mod native_dispatch;
mod tls;
pub use tls::{js_ext_tls_connect, js_tls_connect};
mod classes;
pub use classes::*;
mod jsvalue;
pub mod native_transport;
mod payload_closed;
mod payload_events;
mod payload_io;
mod payload_pipe;
mod payload_prototype;
mod payload_provider;
mod payload_server;
mod payload_socket;
#[cfg(test)]
mod payload_tests;
mod payload_tls;
mod payload_transport;
#[cfg(test)]
mod test_async_shims;
use crate::tls::TlsClientConfigData;
pub(crate) use jsvalue::{
    build_error_object, get_object_bool_field, get_object_number_field, get_object_string_field,
    get_object_value_field, is_nanboxed_pointer, jsvalue_to_owned_string, jsvalue_to_socket_bytes,
    string_from_header_i64, unbox_pointer,
};
mod socket_facade;
pub mod turnloop_tls;
pub use socket_facade::*;
mod payload_abi;
pub use payload_abi::*;
pub const TURNLOOP_SUBSYSTEM: u8 = 0;
#[no_mangle]
pub unsafe extern "C" fn js_ext_net_socket_connect(
    arg1_f64: f64,
    arg2_f64: f64,
    arg3_f64: f64,
) -> i64 {
    js_net_socket_connect(arg1_f64, arg2_f64, arg3_f64)
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_connect(arg1_f64: f64, arg2_f64: f64, arg3_f64: f64) -> i64 {
    let scope = TransientRootScope::enter();
    let a = scope.root_nanbox(arg1_f64);
    let b = scope.root_nanbox(arg2_f64);
    let c = scope.root_nanbox(arg3_f64);
    let owner = scope.root_nanbox(payload_socket::new_socket(payload_io::ROUTE, a.get()));
    payload_socket::connect(owner.get(), a.get(), b.get(), c.get());
    payload_transport::raw_owner(owner.get())
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_alloc() -> i64 {
    payload_transport::raw_owner(payload_socket::new_socket(
        payload_io::ROUTE,
        payload_transport::undefined(),
    ))
}

#[no_mangle]
pub unsafe extern "C" fn js_ext_net_socket_alloc(options: f64) -> i64 {
    payload_transport::raw_owner(payload_socket::new_socket(payload_io::ROUTE, options))
}

#[no_mangle]
pub unsafe extern "C" fn js_net_create_server(
    _options_i64: i64,
    connection_listener_i64: i64,
) -> i64 {
    let scope = TransientRootScope::enter();
    let options = scope.root_nanbox(if _options_i64 == 0 {
        payload_transport::undefined()
    } else {
        payload_transport::boxed_addr(_options_i64)
    });
    let callback = scope.root_nanbox(if connection_listener_i64 == 0 {
        payload_transport::undefined()
    } else {
        payload_transport::boxed_addr(connection_listener_i64)
    });
    payload_transport::raw_owner(payload_server::new_server(
        payload_io::ROUTE,
        options.get(),
        callback.get(),
    ))
}

#[no_mangle]
pub unsafe extern "C" fn js_ext_net_create_server(
    options_i64: i64,
    connection_listener_i64: i64,
) -> i64 {
    js_net_create_server(options_i64, connection_listener_i64)
}

#[no_mangle]
pub unsafe extern "C" fn js_net_server_listen(handle: i64, port: f64, arg2: f64, arg3: f64) {
    payload_server::listen(payload_transport::boxed_addr(handle), port, arg2, arg3);
}

#[no_mangle]
pub unsafe extern "C" fn js_net_server_close(handle: i64, callback_i64: i64) {
    payload_server::close(
        payload_transport::boxed_addr(handle),
        if callback_i64 == 0 {
            payload_transport::undefined()
        } else {
            payload_transport::boxed_addr(callback_i64)
        },
    );
}

/// `server.address()` as a JSON string, for the stdlib's untyped method
/// dispatch (`handle` is the server object's address).
#[no_mangle]
pub unsafe extern "C" fn js_net_server_address(handle: i64) -> *mut StringHeader {
    alloc_string(&payload_server::address_json(
        payload_transport::boxed_addr(handle),
    ))
    .as_raw()
}

#[no_mangle]
pub unsafe extern "C" fn js_ext_net_socket_method_connect(
    handle: i64,
    arg1: f64,
    arg2: f64,
    arg3: f64,
) {
    js_net_socket_method_connect(handle, arg1, arg2, arg3);
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_method_connect(
    handle: i64,
    arg1: f64,
    arg2: f64,
    arg3: f64,
) {
    payload_socket::connect(payload_transport::boxed_addr(handle), arg1, arg2, arg3);
}
