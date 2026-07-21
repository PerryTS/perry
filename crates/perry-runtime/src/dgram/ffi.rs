//! `node:dgram` `#[no_mangle]` FFI entry points called from generated code.
//!
//! Split out of `dgram.rs` (pure code move). See the trunk module for the data
//! model and shared helpers.

use super::*;

use crate::array::ArrayHeader;

#[no_mangle]
pub extern "C" fn js_dgram_create_socket(args: *const ArrayHeader) -> f64 {
    create_socket_impl(&collect_args(args))
}

#[no_mangle]
pub extern "C" fn js_dgram_socket_send(handle: i64, args: *const ArrayHeader) -> f64 {
    send_impl(socket_value_from_handle(handle), &collect_args(args))
}

#[no_mangle]
pub extern "C" fn js_dgram_socket_bind(handle: i64, args: *const ArrayHeader) -> f64 {
    bind_impl(socket_value_from_handle(handle), &collect_args(args))
}

#[no_mangle]
pub extern "C" fn js_dgram_socket_close(handle: i64, args: *const ArrayHeader) -> f64 {
    close_impl(socket_value_from_handle(handle), &collect_args(args))
}

#[no_mangle]
pub extern "C" fn js_dgram_socket_address(handle: i64, _args: *const ArrayHeader) -> f64 {
    address_impl(socket_value_from_handle(handle))
}

#[no_mangle]
pub extern "C" fn js_dgram_socket_remote_address(handle: i64, _args: *const ArrayHeader) -> f64 {
    remote_address_impl(socket_value_from_handle(handle))
}

#[no_mangle]
pub extern "C" fn js_dgram_socket_connect(handle: i64, args: *const ArrayHeader) -> f64 {
    connect_impl(socket_value_from_handle(handle), &collect_args(args))
}

#[no_mangle]
pub extern "C" fn js_dgram_socket_disconnect(handle: i64, _args: *const ArrayHeader) -> f64 {
    disconnect_impl(socket_value_from_handle(handle))
}

#[no_mangle]
pub extern "C" fn js_dgram_socket_on(handle: i64, args: *const ArrayHeader) -> f64 {
    let socket = socket_value_from_handle(handle);
    let args = collect_args(args);
    add_listener(
        socket,
        args.first().copied().unwrap_or_else(undefined_value),
        args.get(1).copied().unwrap_or_else(undefined_value),
        false,
    );
    socket
}

#[no_mangle]
pub extern "C" fn js_dgram_socket_once(handle: i64, args: *const ArrayHeader) -> f64 {
    let socket = socket_value_from_handle(handle);
    let args = collect_args(args);
    add_listener(
        socket,
        args.first().copied().unwrap_or_else(undefined_value),
        args.get(1).copied().unwrap_or_else(undefined_value),
        true,
    );
    socket
}

#[no_mangle]
pub extern "C" fn js_dgram_socket_remove_listener(handle: i64, args: *const ArrayHeader) -> f64 {
    let socket = socket_value_from_handle(handle);
    let args = collect_args(args);
    if args.len() >= 2 {
        remove_listener(socket, args[0], args[1]);
    }
    socket
}

#[no_mangle]
pub extern "C" fn js_dgram_socket_emit(handle: i64, args: *const ArrayHeader) -> f64 {
    let socket = socket_value_from_handle(handle);
    let args = collect_args(args);
    bool_value(emit_event_value(
        socket,
        args.first().copied().unwrap_or_else(undefined_value),
        args.get(1..).unwrap_or(&[]),
    ))
}

#[no_mangle]
pub extern "C" fn js_dgram_socket_listener_count(handle: i64, args: *const ArrayHeader) -> f64 {
    let args = collect_args(args);
    listener_snapshot(
        socket_value_from_handle(handle),
        args.first().copied().unwrap_or_else(undefined_value),
    )
    .len() as f64
}

#[no_mangle]
pub extern "C" fn js_dgram_socket_event_names(handle: i64, _args: *const ArrayHeader) -> f64 {
    event_names_impl(socket_value_from_handle(handle))
}

#[no_mangle]
pub extern "C" fn js_dgram_socket_add_membership(handle: i64, args: *const ArrayHeader) -> f64 {
    membership_impl(
        socket_value_from_handle(handle),
        &collect_args(args),
        "addMembership",
    )
}

#[no_mangle]
pub extern "C" fn js_dgram_socket_drop_membership(handle: i64, args: *const ArrayHeader) -> f64 {
    membership_impl(
        socket_value_from_handle(handle),
        &collect_args(args),
        "dropMembership",
    )
}

#[no_mangle]
pub extern "C" fn js_dgram_socket_add_source_membership(
    handle: i64,
    args: *const ArrayHeader,
) -> f64 {
    source_membership_impl(
        socket_value_from_handle(handle),
        &collect_args(args),
        "addSourceSpecificMembership",
    )
}

#[no_mangle]
pub extern "C" fn js_dgram_socket_drop_source_membership(
    handle: i64,
    args: *const ArrayHeader,
) -> f64 {
    source_membership_impl(
        socket_value_from_handle(handle),
        &collect_args(args),
        "dropSourceSpecificMembership",
    )
}

#[no_mangle]
pub extern "C" fn js_dgram_socket_set_broadcast(handle: i64, args: *const ArrayHeader) -> f64 {
    set_broadcast_impl(socket_value_from_handle(handle), &collect_args(args))
}

#[no_mangle]
pub extern "C" fn js_dgram_socket_set_multicast_ttl(handle: i64, args: *const ArrayHeader) -> f64 {
    set_multicast_ttl_impl(socket_value_from_handle(handle), &collect_args(args))
}

#[no_mangle]
pub extern "C" fn js_dgram_socket_set_multicast_loopback(
    handle: i64,
    args: *const ArrayHeader,
) -> f64 {
    set_multicast_loopback_impl(socket_value_from_handle(handle), &collect_args(args))
}

#[no_mangle]
pub extern "C" fn js_dgram_socket_set_multicast_interface(
    handle: i64,
    args: *const ArrayHeader,
) -> f64 {
    set_multicast_interface_impl(socket_value_from_handle(handle), &collect_args(args))
}

#[no_mangle]
pub extern "C" fn js_dgram_socket_set_ttl(handle: i64, args: *const ArrayHeader) -> f64 {
    set_ttl_impl(socket_value_from_handle(handle), &collect_args(args))
}

#[no_mangle]
pub extern "C" fn js_dgram_socket_set_recv_buffer_size(
    handle: i64,
    args: *const ArrayHeader,
) -> f64 {
    set_buffer_size_impl(
        socket_value_from_handle(handle),
        &collect_args(args),
        KEY_RECV_BUFFER_SIZE,
        "uv_recv_buffer_size",
    )
}

#[no_mangle]
pub extern "C" fn js_dgram_socket_set_send_buffer_size(
    handle: i64,
    args: *const ArrayHeader,
) -> f64 {
    set_buffer_size_impl(
        socket_value_from_handle(handle),
        &collect_args(args),
        KEY_SEND_BUFFER_SIZE,
        "uv_send_buffer_size",
    )
}

#[no_mangle]
pub extern "C" fn js_dgram_socket_get_recv_buffer_size(
    handle: i64,
    _args: *const ArrayHeader,
) -> f64 {
    get_buffer_size_impl(
        socket_value_from_handle(handle),
        KEY_RECV_BUFFER_SIZE,
        "uv_recv_buffer_size",
    )
}

#[no_mangle]
pub extern "C" fn js_dgram_socket_get_send_buffer_size(
    handle: i64,
    _args: *const ArrayHeader,
) -> f64 {
    get_buffer_size_impl(
        socket_value_from_handle(handle),
        KEY_SEND_BUFFER_SIZE,
        "uv_send_buffer_size",
    )
}

#[no_mangle]
pub extern "C" fn js_dgram_socket_chain(handle: i64, _args: *const ArrayHeader) -> f64 {
    socket_value_from_handle(handle)
}

#[no_mangle]
pub extern "C" fn js_dgram_socket_ref(handle: i64, _args: *const ArrayHeader) -> f64 {
    ref_impl(socket_value_from_handle(handle), true)
}

#[no_mangle]
pub extern "C" fn js_dgram_socket_unref(handle: i64, _args: *const ArrayHeader) -> f64 {
    ref_impl(socket_value_from_handle(handle), false)
}

#[no_mangle]
pub extern "C" fn js_dgram_socket_zero(_handle: i64, _args: *const ArrayHeader) -> f64 {
    0.0
}

#[no_mangle]
pub extern "C" fn js_dgram_socket_noop(_handle: i64, _args: *const ArrayHeader) -> f64 {
    undefined_value()
}
