//! `node:dgram` socket operation implementations: createSocket, bind, address,
//! close, connect/disconnect, send routing, membership, multicast and buffer
//! size setters/getters.
//!
//! Split out of `dgram.rs` (pure code move). See the trunk module for the data
//! model and shared helpers.

use super::*;

use std::net::Ipv4Addr;

extern "C" fn dgram_abort_close_task(closure: *const ClosureHeader) -> f64 {
    if closure.is_null() {
        return undefined_value();
    }
    close_impl(js_closure_get_capture_f64(closure, 0), &[]);
    undefined_value()
}

fn schedule_abort_close(socket: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let socket = scope.root_nanbox_f64(socket);
    crate::closure::js_register_closure_arity(dgram_abort_close_task as *const u8, 0);
    let task = js_closure_alloc(dgram_abort_close_task as *const u8, 1);
    js_closure_set_capture_f64(task, 0, socket.get_nanbox_f64());
    crate::builtins::js_queue_microtask(task as i64);
}

extern "C" fn dgram_abort_listener(closure: *const ClosureHeader) -> f64 {
    if !closure.is_null() {
        schedule_abort_close(js_closure_get_capture_f64(closure, 0));
    }
    undefined_value()
}

fn attach_abort_signal(socket: f64, signal: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let socket = scope.root_nanbox_f64(socket);
    let signal = scope.root_nanbox_f64(signal);
    let signal_ptr = crate::url::js_abort_signal_resolve_ptr(signal.get_nanbox_f64());
    if signal_ptr.is_null() {
        throw_invalid_arg_type("options.signal", "AbortSignal", signal.get_nanbox_f64());
    }
    if crate::url::js_abort_signal_is_aborted(signal_ptr) != 0 {
        schedule_abort_close(socket.get_nanbox_f64());
        return;
    }

    crate::closure::js_register_closure_arity(dgram_abort_listener as *const u8, 0);
    let listener = js_closure_alloc(dgram_abort_listener as *const u8, 1);
    js_closure_set_capture_f64(listener, 0, socket.get_nanbox_f64());
    let listener = scope.root_nanbox_f64(boxed_pointer(listener as *const u8));
    let abort_event = scope.root_nanbox_f64(str_value("abort"));
    let signal_ptr = crate::url::js_abort_signal_resolve_ptr(signal.get_nanbox_f64());
    crate::url::js_abort_signal_add_listener(
        signal_ptr,
        abort_event.get_nanbox_f64(),
        listener.get_nanbox_f64(),
    );
    set_hidden_value(
        socket.get_nanbox_f64(),
        KEY_ABORT_SIGNAL,
        signal.get_nanbox_f64(),
    );
    set_hidden_value(
        socket.get_nanbox_f64(),
        KEY_ABORT_LISTENER,
        listener.get_nanbox_f64(),
    );
}

fn detach_abort_signal(socket: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let socket = scope.root_nanbox_f64(socket);
    let Some(signal) = get_hidden_value(socket.get_nanbox_f64(), KEY_ABORT_SIGNAL) else {
        return;
    };
    let signal = scope.root_nanbox_f64(signal);
    let Some(listener) = get_hidden_value(socket.get_nanbox_f64(), KEY_ABORT_LISTENER) else {
        return;
    };
    let listener = scope.root_nanbox_f64(listener);
    let abort_event = scope.root_nanbox_f64(str_value("abort"));
    let signal_ptr = crate::url::js_abort_signal_resolve_ptr(signal.get_nanbox_f64());
    crate::url::js_abort_signal_remove_listener(
        signal_ptr,
        abort_event.get_nanbox_f64(),
        listener.get_nanbox_f64(),
    );
    set_hidden_value(socket.get_nanbox_f64(), KEY_ABORT_SIGNAL, undefined_value());
    set_hidden_value(
        socket.get_nanbox_f64(),
        KEY_ABORT_LISTENER,
        undefined_value(),
    );
}

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
    if let Some(options) = object_ptr_from_value(first) {
        let options = boxed_pointer(options as *const u8);
        if let Some(lookup) = get_prop(options, "lookup") {
            if !is_callable_value(lookup) {
                throw_invalid_arg_type("options.lookup", "function", lookup);
            }
            set_hidden_value(socket, KEY_LOOKUP, lookup);
        }
        if let Some(signal) = get_prop(options, "signal") {
            attach_abort_signal(socket, signal);
        }
        if let Some(size) = get_prop(options, "recvBufferSize") {
            set_hidden_value(
                socket,
                KEY_RECV_BUFFER_SIZE,
                validate_option_buffer_size(size, "options.recvBufferSize").max(1.0),
            );
        }
        if let Some(size) = get_prop(options, "sendBufferSize") {
            set_hidden_value(
                socket,
                KEY_SEND_BUFFER_SIZE,
                validate_option_buffer_size(size, "options.sendBufferSize").max(1.0),
            );
        }
        if let Some(block_list) = get_prop(options, "sendBlockList") {
            set_hidden_value(socket, KEY_SEND_BLOCK_LIST, block_list);
        }
    }
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
    if let Some(lookup) = get_hidden_value(socket, KEY_LOOKUP) {
        invoke_lookup(socket, lookup, &address);
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
            set_hidden_value(socket, KEY_BIND_ATTEMPTED, bool_value(true));
            emit_event(socket, "error", &[error]);
        }
    }
    socket
}

pub(crate) fn address_impl(socket: f64) -> f64 {
    if !is_truthy_hidden(socket, KEY_BOUND) {
        if is_truthy_hidden(socket, KEY_BIND_ATTEMPTED) {
            return build_address_info(
                &default_bind_address(socket),
                family_for_type(
                    &hidden_string(socket, KEY_TYPE).unwrap_or_else(|| "udp4".to_string()),
                ),
                0,
            );
        }
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
    detach_abort_signal(socket);
    set_hidden_value(socket, KEY_BOUND, bool_value(false));
    set_hidden_value(socket, KEY_CONNECTED, bool_value(false));
    set_hidden_value(socket, KEY_CLOSED, bool_value(true));
    emit_event(socket, "close", &[]);
    if let Some(callback) = callback_from_args(args) {
        call_function(callback, socket, &[]);
    }
    socket
}

pub(crate) fn connect_impl(socket: f64, args: &[f64]) -> f64 {
    if is_truthy_hidden(socket, KEY_CONNECTED) {
        crate::fs::validate::throw_error_with_code(
            "Already connected",
            "ERR_SOCKET_DGRAM_IS_CONNECTED",
        );
    }
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
    if send_blocked(socket, &address) {
        let error = socket_error_value("connect ERR_IP_BLOCKED", "ERR_IP_BLOCKED", "connect");
        if let Some(callback) = callback_from_args(args) {
            call_function(callback, socket, &[error]);
            return undefined_value();
        }
        crate::exception::js_throw(error);
    }
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
    if is_truthy_hidden(socket, KEY_CONNECTED) {
        let uses_slice = send_uses_offset_length(socket, args);
        let port = args.get(3).copied();
        let address = args.get(4).copied();
        let has_port = port.is_some_and(|value| {
            crate::value::js_is_truthy(value) != 0 && !(uses_slice && is_callable_value(value))
        });
        let has_address = address.is_some_and(|value| crate::value::js_is_truthy(value) != 0);
        if has_port || has_address {
            crate::fs::validate::throw_error_with_code(
                "Already connected",
                "ERR_SOCKET_DGRAM_IS_CONNECTED",
            );
        }
        let address = hidden_string(socket, KEY_REMOTE_ADDRESS)
            .unwrap_or_else(|| default_loopback_address(socket));
        return (hidden_port(socket, KEY_REMOTE_PORT), address);
    }
    if send_uses_offset_length(socket, args) {
        let port = port_from_value(args[3], false);
        let address = send_address(socket, args.get(4).copied());
        return (port, address);
    }
    let port = args
        .get(1)
        .copied()
        .map(|value| port_from_value(value, false))
        .unwrap_or_else(|| port_from_value(undefined_value(), false));
    let address = send_address(socket, args.get(2).copied());
    (port, address)
}

pub(crate) fn send_impl(socket: f64, args: &[f64]) -> f64 {
    let (message, size, bytes) = send_message(socket, args);
    let (port, address) = send_destination(socket, args);
    if send_blocked(socket, &address) {
        return finish_send(
            socket,
            args,
            Err(socket_error_value(
                "send ERR_IP_BLOCKED",
                "ERR_IP_BLOCKED",
                "send",
            )),
        );
    }
    if !deterministic() {
        return real_send_bytes(socket, args, bytes, port, address);
    }
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
    finish_send(socket, args, Ok(size))
}

pub(crate) fn sendto_impl(socket: f64, args: &[f64]) -> f64 {
    let message = args.first().copied().unwrap_or_else(undefined_value);
    if message_bytes(message).is_none() {
        throw_invalid_arg_type("msg", "Buffer, TypedArray, DataView, or string", message);
    }
    for (index, name) in [(1, "offset"), (2, "length")] {
        let value = args.get(index).copied().unwrap_or_else(undefined_value);
        if number_value(value).is_none() {
            throw_invalid_arg_type(name, "number", value);
        }
    }
    let port = args.get(3).copied().unwrap_or_else(undefined_value);
    if number_value(port).is_none() {
        throw_invalid_arg_type("port", "number", port);
    }
    let address = args.get(4).copied().unwrap_or_else(undefined_value);
    if string_to_rust(address).is_none() {
        throw_invalid_arg_type("address", "string", address);
    }
    send_impl(socket, args)
}

fn send_address(socket: f64, value: Option<f64>) -> String {
    let Some(value) = value else {
        return default_loopback_address(socket);
    };
    if matches!(value.to_bits(), TAG_UNDEFINED | TAG_NULL) || is_callable_value(value) {
        return default_loopback_address(socket);
    }
    let address =
        string_to_rust(value).unwrap_or_else(|| throw_invalid_arg_type("address", "string", value));
    if address.is_empty() {
        default_loopback_address(socket)
    } else {
        address
    }
}

fn send_uses_offset_length(socket: f64, args: &[f64]) -> bool {
    if is_truthy_hidden(socket, KEY_CONNECTED) {
        return args.get(2).copied().is_some_and(is_number_like);
    }
    args.get(4)
        .copied()
        .is_some_and(|value| crate::value::js_is_truthy(value) != 0)
        || args.get(3).copied().is_some_and(|value| {
            crate::value::js_is_truthy(value) != 0 && !is_callable_value(value)
        })
}

fn send_message(socket: f64, args: &[f64]) -> (f64, usize, Vec<u8>) {
    let value = args.first().copied().unwrap_or_else(undefined_value);
    let mut bytes = message_bytes(value).unwrap_or_else(|| throw_invalid_message(value));
    if send_uses_offset_length(socket, args) {
        let offset_value = args.get(1).copied().unwrap_or_else(undefined_value);
        let length_value = args.get(2).copied().unwrap_or_else(undefined_value);
        let offset_number = number_value(offset_value)
            .unwrap_or_else(|| throw_invalid_arg_type("offset", "number", offset_value));
        let length_number = number_value(length_value)
            .unwrap_or_else(|| throw_invalid_arg_type("length", "number", length_value));
        let offset = offset_number as usize;
        let length = length_number as usize;
        if !offset_number.is_finite()
            || !length_number.is_finite()
            || offset_number < 0.0
            || length_number < 0.0
            || offset.saturating_add(length) > bytes.len()
        {
            crate::fs::validate::throw_range_error_named(
                "Attempt to access memory outside buffer bounds",
                "ERR_BUFFER_OUT_OF_BOUNDS",
            );
        }
        bytes = bytes[offset..offset + length].to_vec();
    }
    let message = make_buffer(&bytes);
    (message, bytes.len(), bytes)
}

fn send_blocked(socket: f64, address: &str) -> bool {
    let scope = crate::gc::RuntimeHandleScope::new();
    let socket = scope.root_nanbox_f64(socket);
    let Some(list) = get_hidden_value(socket.get_nanbox_f64(), KEY_SEND_BLOCK_LIST) else {
        return false;
    };
    let list = scope.root_nanbox_f64(list);
    let Some(check) = dynamic_prop(list.get_nanbox_f64(), b"check") else {
        return false;
    };
    let check = scope.root_nanbox_f64(check);
    let family = if address.contains(':') {
        "ipv6"
    } else {
        "ipv4"
    };
    let address = scope.root_nanbox_f64(str_value(address));
    let family = scope.root_nanbox_f64(str_value(family));
    crate::value::js_is_truthy(call_function(
        check.get_nanbox_f64(),
        list.get_nanbox_f64(),
        &[address.get_nanbox_f64(), family.get_nanbox_f64()],
    )) != 0
}

extern "C" fn lookup_callback(_closure: *const ClosureHeader, _rest: f64) -> f64 {
    undefined_value()
}

fn invoke_lookup(socket: f64, lookup: f64, address: &str) {
    crate::closure::js_register_closure_rest(lookup_callback as *const u8, 0);
    let callback = js_closure_alloc(lookup_callback as *const u8, 0);
    let family = if address.contains(':') { 6.0 } else { 4.0 };
    call_function(
        lookup,
        socket,
        &[
            str_value(address),
            family,
            boxed_pointer(callback as *const u8),
        ],
    );
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
    if ttl.is_nan() || (ttl.is_finite() && !(0.0..=255.0).contains(&ttl)) {
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

fn validate_option_buffer_size(value: f64, name: &'static str) -> f64 {
    if number_value(value).is_none() {
        throw_invalid_arg_type(name, "number", value);
    }
    validate_buffer_size(value)
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
