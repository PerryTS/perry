//! `node:dgram` socket operation implementations: createSocket, bind, address,
//! close, connect/disconnect, send routing, membership, multicast and buffer
//! size setters/getters.
//!
//! Split out of `dgram.rs` (pure code move). See the trunk module for the data
//! model and shared helpers.

use super::*;

use std::net::Ipv4Addr;

pub(crate) fn create_socket_impl(args: &[f64]) -> f64 {
    let first = args.first().copied().unwrap_or_else(undefined_value);
    let socket_type = if let Some(kind) = string_to_rust(first) {
        kind
    } else if let Some(kind_value) = get_prop(first, "type") {
        string_to_rust(kind_value).unwrap_or_default()
    } else {
        throw_bad_socket_type(first);
    };
    if socket_type != "udp4" && socket_type != "udp6" {
        throw_bad_socket_type(first);
    }
    let socket = socket_object(&socket_type);
    if let Some(callback) = callback_from_args(args) {
        add_listener(socket, str_value("message"), callback, false);
    }
    socket
}

pub(crate) fn bind_impl(socket: f64, args: &[f64]) -> f64 {
    if is_truthy_hidden(socket, KEY_CLOSED) {
        return socket;
    }
    let mut port = 0u16;
    let mut address = default_bind_address(socket);
    if let Some(first) = args.first().copied() {
        if let Some(option_port) = get_prop(first, "port") {
            port = port_from_value(option_port, true);
            if let Some(option_address) = get_prop(first, "address").and_then(string_to_rust) {
                address = option_address;
            }
        } else if is_number_like(first) {
            port = port_from_value(first, true);
            if let Some(second) = args.get(1).copied().and_then(string_to_rust) {
                address = second;
            }
        }
    }
    let bind_result = if deterministic() {
        bind_socket(socket, port, address);
        Ok(())
    } else {
        real_bind(socket, port, &address)
    };
    match bind_result {
        Ok(()) => {
            emit_event(socket, "listening", &[]);
            if let Some(callback) = callback_from_args(args) {
                call_function(callback, socket, &[]);
            }
        }
        Err(error) => {
            emit_event(socket, "error", &[error]);
        }
    }
    socket
}

pub(crate) fn address_impl(socket: f64) -> f64 {
    if !is_truthy_hidden(socket, KEY_BOUND) {
        throw_not_bound();
    }
    let address =
        hidden_string(socket, KEY_ADDRESS).unwrap_or_else(|| default_bind_address(socket));
    let family = hidden_string(socket, KEY_FAMILY)
        .unwrap_or_else(|| family_for_address(&address, socket).to_string());
    build_address_info(&address, &family, hidden_port(socket, KEY_PORT))
}

pub(crate) fn close_impl(socket: f64, args: &[f64]) -> f64 {
    if is_truthy_hidden(socket, KEY_CLOSED) {
        return undefined_value();
    }
    if deterministic() {
        remove_bound_socket(socket);
    } else if let Some(id) = reactor_id(socket) {
        crate::dgram_reactor::unregister(id);
    }
    set_hidden_value(socket, KEY_BOUND, bool_value(false));
    set_hidden_value(socket, KEY_CONNECTED, bool_value(false));
    set_hidden_value(socket, KEY_CLOSED, bool_value(true));
    if let Some(callback) = callback_from_args(args) {
        call_function(callback, socket, &[]);
    }
    emit_event(socket, "close", &[]);
    undefined_value()
}

pub(crate) fn connect_impl(socket: f64, args: &[f64]) -> f64 {
    let port = args
        .first()
        .copied()
        .map(|value| port_from_value(value, false))
        .unwrap_or_else(|| port_from_value(undefined_value(), false));
    let address = args
        .get(1)
        .copied()
        .and_then(string_to_rust)
        .unwrap_or_else(|| default_loopback_address(socket));
    let address = normalize_address(&address, socket);
    ensure_bound(socket);
    set_hidden_value(socket, KEY_REMOTE_ADDRESS, str_value(&address));
    set_hidden_value(
        socket,
        KEY_REMOTE_FAMILY,
        str_value(family_for_address(&address, socket)),
    );
    set_hidden_value(socket, KEY_REMOTE_PORT, port as f64);
    set_hidden_value(socket, KEY_CONNECTED, bool_value(true));
    emit_event(socket, "connect", &[]);
    if let Some(callback) = callback_from_args(args) {
        call_function(callback, socket, &[]);
    }
    undefined_value()
}

pub(crate) fn disconnect_impl(socket: f64) -> f64 {
    if !is_truthy_hidden(socket, KEY_CONNECTED) {
        throw_not_connected();
    }
    set_hidden_value(socket, KEY_CONNECTED, bool_value(false));
    set_hidden_value(socket, KEY_REMOTE_ADDRESS, undefined_value());
    set_hidden_value(socket, KEY_REMOTE_FAMILY, undefined_value());
    set_hidden_value(socket, KEY_REMOTE_PORT, 0.0);
    undefined_value()
}

pub(crate) fn remote_address_impl(socket: f64) -> f64 {
    if !is_truthy_hidden(socket, KEY_CONNECTED) {
        throw_not_connected();
    }
    let address = hidden_string(socket, KEY_REMOTE_ADDRESS)
        .unwrap_or_else(|| default_loopback_address(socket));
    let family = hidden_string(socket, KEY_REMOTE_FAMILY)
        .unwrap_or_else(|| family_for_address(&address, socket).to_string());
    build_address_info(&address, &family, hidden_port(socket, KEY_REMOTE_PORT))
}

pub(crate) fn send_destination(socket: f64, args: &[f64]) -> (u16, String) {
    if is_truthy_hidden(socket, KEY_CONNECTED)
        && (args.len() <= 1 || args.get(1).copied().is_some_and(is_callable_value))
    {
        let address = hidden_string(socket, KEY_REMOTE_ADDRESS)
            .unwrap_or_else(|| default_loopback_address(socket));
        return (hidden_port(socket, KEY_REMOTE_PORT), address);
    }
    if args.len() >= 4
        && is_number_like(args[1])
        && is_number_like(args[2])
        && is_number_like(args[3])
    {
        let port = port_from_value(args[3], false);
        let address = args
            .get(4)
            .copied()
            .and_then(string_to_rust)
            .unwrap_or_else(|| default_loopback_address(socket));
        return (port, address);
    }
    let port = args
        .get(1)
        .copied()
        .map(|value| port_from_value(value, false))
        .unwrap_or_else(|| port_from_value(undefined_value(), false));
    let address = args
        .get(2)
        .copied()
        .and_then(string_to_rust)
        .unwrap_or_else(|| default_loopback_address(socket));
    (port, address)
}

pub(crate) fn send_impl(socket: f64, args: &[f64]) -> f64 {
    if !deterministic() {
        return real_send(socket, args);
    }
    let msg = args.first().copied().unwrap_or_else(undefined_value);
    let Some((message, size)) = message_value(msg) else {
        throw_invalid_message(msg);
    };
    let (port, address) = send_destination(socket, args);
    ensure_bound(socket);
    let source_address =
        hidden_string(socket, KEY_ADDRESS).unwrap_or_else(|| default_loopback_address(socket));
    let source_family = hidden_string(socket, KEY_FAMILY)
        .unwrap_or_else(|| family_for_address(&source_address, socket).to_string());
    let source_port = hidden_port(socket, KEY_PORT);
    if let Some(target) = lookup_bound_socket(&address, port, socket) {
        if !is_truthy_hidden(target, KEY_CLOSED) {
            let rinfo = build_rinfo(&source_address, &source_family, source_port, size);
            emit_event(target, "message", &[message, rinfo]);
        }
    }
    if let Some(callback) = callback_from_args(args) {
        call_function(callback, socket, &[null_value(), size as f64]);
    }
    undefined_value()
}

pub(crate) fn membership_impl(socket: f64, args: &[f64], syscall: &'static str) -> f64 {
    let multicast_address = args.first().copied().unwrap_or_else(undefined_value);
    if is_missing_membership_arg(multicast_address) {
        throw_missing_arg("multicastAddress");
    }
    let Some(group) = string_to_rust(multicast_address) else {
        throw_socket_errno(syscall, "EINVAL");
    };
    if group.is_empty() {
        throw_socket_errno(syscall, "EINVAL");
    }
    if deterministic() {
        return undefined_value();
    }
    let Some(udp) = live_udp(socket) else {
        throw_socket_errno(syscall, "EBADF");
    };
    let interface = args.get(1).copied().and_then(string_to_rust);
    let dropping = syscall == "dropMembership";
    let result = if let Some(group_v4) = parse_multicast_v4(&group) {
        let iface = interface
            .as_deref()
            .and_then(|s| s.parse::<Ipv4Addr>().ok())
            .unwrap_or(Ipv4Addr::UNSPECIFIED);
        if dropping {
            udp.leave_multicast_v4(&group_v4, &iface)
        } else {
            udp.join_multicast_v4(&group_v4, &iface)
        }
    } else if let Some(group_v6) = parse_multicast_v6(&group) {
        if dropping {
            udp.leave_multicast_v6(&group_v6, 0)
        } else {
            udp.join_multicast_v6(&group_v6, 0)
        }
    } else {
        throw_socket_errno(syscall, "EINVAL");
    };
    if result.is_err() {
        throw_socket_errno(syscall, "EINVAL");
    }
    undefined_value()
}

pub(crate) fn source_membership_impl(socket: f64, args: &[f64], syscall: &'static str) -> f64 {
    let source_address = validate_string_arg(
        args.first().copied().unwrap_or_else(undefined_value),
        "sourceAddress",
    );
    let group_address = validate_string_arg(
        args.get(1).copied().unwrap_or_else(undefined_value),
        "groupAddress",
    );
    if source_address.is_empty() || group_address.is_empty() {
        throw_socket_errno(syscall, "EINVAL");
    }
    if deterministic() {
        return undefined_value();
    }
    let Some(udp) = live_udp(socket) else {
        throw_socket_errno(syscall, "EBADF");
    };
    let (Ok(source_v4), Ok(group_v4)) = (
        source_address.parse::<Ipv4Addr>(),
        group_address.parse::<Ipv4Addr>(),
    ) else {
        // Source-specific multicast over IPv6 is not exposed here.
        throw_socket_errno(syscall, "EINVAL");
    };
    let iface = args
        .get(2)
        .copied()
        .and_then(string_to_rust)
        .and_then(|s| s.parse::<Ipv4Addr>().ok())
        .unwrap_or(Ipv4Addr::UNSPECIFIED);
    let sock_ref = socket2::SockRef::from(&*udp);
    let result = if syscall.starts_with("drop") {
        sock_ref.leave_ssm_v4(&source_v4, &group_v4, &iface)
    } else {
        sock_ref.join_ssm_v4(&source_v4, &group_v4, &iface)
    };
    if result.is_err() {
        throw_socket_errno(syscall, "EINVAL");
    }
    undefined_value()
}

pub(crate) fn set_broadcast_impl(socket: f64, args: &[f64]) -> f64 {
    ensure_running(socket, "setBroadcast");
    if !deterministic() {
        let flag = args
            .first()
            .copied()
            .is_some_and(|v| crate::value::js_is_truthy(v) != 0);
        with_udp(socket, |udp| {
            let _ = udp.set_broadcast(flag);
        });
    }
    undefined_value()
}

pub(crate) fn set_ttl_impl(socket: f64, args: &[f64]) -> f64 {
    let ttl = validate_number_arg(args.first().copied().unwrap_or_else(undefined_value), "ttl");
    if !ttl.is_finite() || !(1.0..=255.0).contains(&ttl) {
        throw_socket_errno("setTTL", "EINVAL");
    }
    ensure_running(socket, "setTTL");
    if !deterministic() {
        with_udp(socket, |udp| {
            let _ = udp.set_ttl(ttl as u32);
        });
    }
    ttl
}

pub(crate) fn set_multicast_ttl_impl(socket: f64, args: &[f64]) -> f64 {
    let ttl = validate_number_arg(args.first().copied().unwrap_or_else(undefined_value), "ttl");
    if !(0.0..=255.0).contains(&ttl) {
        throw_socket_errno("setMulticastTTL", "EINVAL");
    }
    ensure_running(socket, "setMulticastTTL");
    if !deterministic() {
        with_udp(socket, |udp| {
            let _ = udp.set_multicast_ttl_v4(ttl as u32);
        });
    }
    ttl
}

pub(crate) fn set_multicast_loopback_impl(socket: f64, args: &[f64]) -> f64 {
    let arg = args.first().copied().unwrap_or_else(undefined_value);
    ensure_running(socket, "setMulticastLoopback");
    if !deterministic() {
        let flag = crate::value::js_is_truthy(arg) != 0;
        with_udp(socket, |udp| {
            let _ = udp.set_multicast_loop_v4(flag);
        });
    }
    arg
}

pub(crate) fn set_multicast_interface_impl(socket: f64, args: &[f64]) -> f64 {
    let interface_address = validate_string_arg(
        args.first().copied().unwrap_or_else(undefined_value),
        "interfaceAddress",
    );
    if interface_address.is_empty() {
        throw_socket_errno("setMulticastInterface", "EINVAL");
    }
    ensure_running(socket, "setMulticastInterface");
    if !deterministic() {
        if let Ok(iface) = interface_address.parse::<Ipv4Addr>() {
            with_udp(socket, |udp| {
                let _ = socket2::SockRef::from(udp).set_multicast_if_v4(&iface);
            });
        }
    }
    undefined_value()
}

pub(crate) fn validate_buffer_size(value: f64) -> f64 {
    let Some(size) = number_value(value) else {
        throw_bad_buffer_size();
    };
    if !size.is_finite() || size < 0.0 || size.fract() != 0.0 {
        throw_bad_buffer_size();
    }
    size
}

pub(crate) fn set_buffer_size_impl(
    socket: f64,
    args: &[f64],
    key: &[u8],
    syscall: &'static str,
) -> f64 {
    let size = validate_buffer_size(args.first().copied().unwrap_or_else(undefined_value));
    ensure_buffer_running(socket, syscall);
    set_hidden_value(socket, key, size.max(1.0));
    undefined_value()
}

pub(crate) fn get_buffer_size_impl(socket: f64, key: &[u8], syscall: &'static str) -> f64 {
    ensure_buffer_running(socket, syscall);
    get_hidden_value(socket, key).unwrap_or(65536.0)
}
