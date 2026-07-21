//! `node:dgram` per-method closure thunks (`extern "C"`), bound onto each socket
//! object via `SOCKET_METHODS` in the trunk module.
//!
//! Split out of `dgram.rs` (pure code move). See the trunk module for the data
//! model and shared helpers.

use super::*;

use crate::closure::ClosureHeader;

pub(crate) extern "C" fn dgram_send_thunk(closure: *const ClosureHeader, rest: f64) -> f64 {
    send_impl(this_value(closure), &collect_rest_args(rest))
}

pub(crate) extern "C" fn dgram_bind_thunk(closure: *const ClosureHeader, rest: f64) -> f64 {
    bind_impl(this_value(closure), &collect_rest_args(rest))
}

pub(crate) extern "C" fn dgram_close_thunk(closure: *const ClosureHeader, rest: f64) -> f64 {
    close_impl(this_value(closure), &collect_rest_args(rest))
}

pub(crate) extern "C" fn dgram_address_thunk(closure: *const ClosureHeader, _rest: f64) -> f64 {
    address_impl(this_value(closure))
}

pub(crate) extern "C" fn dgram_remote_address_thunk(
    closure: *const ClosureHeader,
    _rest: f64,
) -> f64 {
    remote_address_impl(this_value(closure))
}

pub(crate) extern "C" fn dgram_connect_thunk(closure: *const ClosureHeader, rest: f64) -> f64 {
    connect_impl(this_value(closure), &collect_rest_args(rest))
}

pub(crate) extern "C" fn dgram_disconnect_thunk(closure: *const ClosureHeader, _rest: f64) -> f64 {
    disconnect_impl(this_value(closure))
}

pub(crate) extern "C" fn dgram_on_thunk(closure: *const ClosureHeader, rest: f64) -> f64 {
    let socket = this_value(closure);
    let args = collect_rest_args(rest);
    let event = args.first().copied().unwrap_or_else(undefined_value);
    let listener = args.get(1).copied().unwrap_or_else(undefined_value);
    add_listener(socket, event, listener, false);
    socket
}

pub(crate) extern "C" fn dgram_once_thunk(closure: *const ClosureHeader, rest: f64) -> f64 {
    let socket = this_value(closure);
    let args = collect_rest_args(rest);
    let event = args.first().copied().unwrap_or_else(undefined_value);
    let listener = args.get(1).copied().unwrap_or_else(undefined_value);
    add_listener(socket, event, listener, true);
    socket
}

pub(crate) extern "C" fn dgram_remove_listener_thunk(
    closure: *const ClosureHeader,
    rest: f64,
) -> f64 {
    let socket = this_value(closure);
    let args = collect_rest_args(rest);
    if args.len() >= 2 {
        remove_listener(socket, args[0], args[1]);
    }
    socket
}

pub(crate) extern "C" fn dgram_emit_thunk(closure: *const ClosureHeader, rest: f64) -> f64 {
    let socket = this_value(closure);
    let args = collect_rest_args(rest);
    let event = args.first().copied().unwrap_or_else(undefined_value);
    let emitted = emit_event_value(socket, event, args.get(1..).unwrap_or(&[]));
    bool_value(emitted)
}

pub(crate) extern "C" fn dgram_listener_count_thunk(
    closure: *const ClosureHeader,
    rest: f64,
) -> f64 {
    let args = collect_rest_args(rest);
    let event = args.first().copied().unwrap_or_else(undefined_value);
    listener_snapshot(this_value(closure), event).len() as f64
}

pub(crate) extern "C" fn dgram_event_names_thunk(closure: *const ClosureHeader, _rest: f64) -> f64 {
    event_names_impl(this_value(closure))
}

pub(crate) extern "C" fn dgram_add_membership_thunk(
    closure: *const ClosureHeader,
    rest: f64,
) -> f64 {
    membership_impl(
        this_value(closure),
        &collect_rest_args(rest),
        "addMembership",
    )
}

pub(crate) extern "C" fn dgram_drop_membership_thunk(
    closure: *const ClosureHeader,
    rest: f64,
) -> f64 {
    membership_impl(
        this_value(closure),
        &collect_rest_args(rest),
        "dropMembership",
    )
}

pub(crate) extern "C" fn dgram_add_source_membership_thunk(
    closure: *const ClosureHeader,
    rest: f64,
) -> f64 {
    source_membership_impl(
        this_value(closure),
        &collect_rest_args(rest),
        "addSourceSpecificMembership",
    )
}

pub(crate) extern "C" fn dgram_drop_source_membership_thunk(
    closure: *const ClosureHeader,
    rest: f64,
) -> f64 {
    source_membership_impl(
        this_value(closure),
        &collect_rest_args(rest),
        "dropSourceSpecificMembership",
    )
}

pub(crate) extern "C" fn dgram_set_broadcast_thunk(
    closure: *const ClosureHeader,
    rest: f64,
) -> f64 {
    set_broadcast_impl(this_value(closure), &collect_rest_args(rest))
}

pub(crate) extern "C" fn dgram_set_multicast_ttl_thunk(
    closure: *const ClosureHeader,
    rest: f64,
) -> f64 {
    set_multicast_ttl_impl(this_value(closure), &collect_rest_args(rest))
}

pub(crate) extern "C" fn dgram_set_multicast_loopback_thunk(
    closure: *const ClosureHeader,
    rest: f64,
) -> f64 {
    set_multicast_loopback_impl(this_value(closure), &collect_rest_args(rest))
}

pub(crate) extern "C" fn dgram_set_multicast_interface_thunk(
    closure: *const ClosureHeader,
    rest: f64,
) -> f64 {
    set_multicast_interface_impl(this_value(closure), &collect_rest_args(rest))
}

pub(crate) extern "C" fn dgram_set_ttl_thunk(closure: *const ClosureHeader, rest: f64) -> f64 {
    set_ttl_impl(this_value(closure), &collect_rest_args(rest))
}

pub(crate) extern "C" fn dgram_set_recv_buffer_size_thunk(
    closure: *const ClosureHeader,
    rest: f64,
) -> f64 {
    set_buffer_size_impl(
        this_value(closure),
        &collect_rest_args(rest),
        KEY_RECV_BUFFER_SIZE,
        "uv_recv_buffer_size",
    )
}

pub(crate) extern "C" fn dgram_set_send_buffer_size_thunk(
    closure: *const ClosureHeader,
    rest: f64,
) -> f64 {
    set_buffer_size_impl(
        this_value(closure),
        &collect_rest_args(rest),
        KEY_SEND_BUFFER_SIZE,
        "uv_send_buffer_size",
    )
}

pub(crate) extern "C" fn dgram_get_recv_buffer_size_thunk(
    closure: *const ClosureHeader,
    _rest: f64,
) -> f64 {
    get_buffer_size_impl(
        this_value(closure),
        KEY_RECV_BUFFER_SIZE,
        "uv_recv_buffer_size",
    )
}

pub(crate) extern "C" fn dgram_get_send_buffer_size_thunk(
    closure: *const ClosureHeader,
    _rest: f64,
) -> f64 {
    get_buffer_size_impl(
        this_value(closure),
        KEY_SEND_BUFFER_SIZE,
        "uv_send_buffer_size",
    )
}

pub(crate) extern "C" fn dgram_ref_thunk(closure: *const ClosureHeader, _rest: f64) -> f64 {
    ref_impl(this_value(closure), true)
}

pub(crate) extern "C" fn dgram_unref_thunk(closure: *const ClosureHeader, _rest: f64) -> f64 {
    ref_impl(this_value(closure), false)
}

pub(crate) extern "C" fn dgram_zero_thunk(_closure: *const ClosureHeader, _rest: f64) -> f64 {
    0.0
}
