//! `node:dgram` UDP support.
//!
//! By default this drives real host UDP sockets (`std::net::UdpSocket`): `bind`
//! opens an OS socket and starts a recv thread via [`crate::dgram_reactor`] that
//! delivers `'message'` events through the event pump; `send` does a real
//! `send_to`; `addMembership`/`setBroadcast`/`setMulticastTTL`/… apply the
//! matching socket option (#4911). Setting `PERRY_DETERMINISTIC_NET=1` reverts
//! to the pre-#4911 in-process loopback registry so unicast delivery between two
//! sockets in the same process stays reproducible in parity fixtures without
//! touching the network.

use std::collections::HashMap;
use std::net::ToSocketAddrs;
use std::sync::{LazyLock, Mutex};

use crate::array::ArrayHeader;
use crate::closure::{
    js_closure_alloc, js_closure_set_capture_ptr, js_register_closure_rest, ClosureHeader,
};
use crate::object::{
    js_object_alloc, js_object_get_field_by_name_f64, js_object_set_field_by_name, ObjectHeader,
};
use crate::value::{
    js_nanbox_pointer, JSValue, POINTER_MASK, TAG_FALSE, TAG_NULL, TAG_TRUE, TAG_UNDEFINED,
};

mod ffi;
mod listeners;
mod net;
mod ops;
mod thunks;

// `#[no_mangle]` FFI entry points. Re-exported so the `crate::dgram::js_dgram_*`
// path (used by `object::native_module_dispatch`) keeps resolving.
pub use ffi::{
    js_dgram_create_socket, js_dgram_socket_add_membership, js_dgram_socket_add_source_membership,
    js_dgram_socket_address, js_dgram_socket_bind, js_dgram_socket_chain, js_dgram_socket_close,
    js_dgram_socket_connect, js_dgram_socket_disconnect, js_dgram_socket_drop_membership,
    js_dgram_socket_drop_source_membership, js_dgram_socket_emit, js_dgram_socket_event_names,
    js_dgram_socket_get_recv_buffer_size, js_dgram_socket_get_send_buffer_size,
    js_dgram_socket_listener_count, js_dgram_socket_noop, js_dgram_socket_on, js_dgram_socket_once,
    js_dgram_socket_ref, js_dgram_socket_remote_address, js_dgram_socket_remove_listener,
    js_dgram_socket_send, js_dgram_socket_set_broadcast, js_dgram_socket_set_multicast_interface,
    js_dgram_socket_set_multicast_loopback, js_dgram_socket_set_multicast_ttl,
    js_dgram_socket_set_recv_buffer_size, js_dgram_socket_set_send_buffer_size,
    js_dgram_socket_set_ttl, js_dgram_socket_unref, js_dgram_socket_zero,
};

// Listener storage / emit (used by trunk SOCKET_METHODS thunks + FFI siblings).
pub(crate) use listeners::{
    add_listener, emit_event, emit_event_value, event_names_impl, listener_snapshot,
    remove_listener,
};

// Networking helpers + `dgram_emit_message` (the latter is called from
// `crate::dgram_reactor`).
pub(crate) use net::{
    bind_socket, build_address_info, build_rinfo, deterministic, dgram_emit_message, ensure_bound,
    live_udp, lookup_bound_socket, message_value, parse_multicast_v4, parse_multicast_v6,
    reactor_id, real_bind, real_send, ref_impl, remove_bound_socket, with_udp,
};

// Socket operation implementations (used by thunks + FFI siblings).
pub(crate) use ops::{
    address_impl, bind_impl, close_impl, connect_impl, create_socket_impl, disconnect_impl,
    get_buffer_size_impl, membership_impl, remote_address_impl, send_destination, send_impl,
    set_broadcast_impl, set_buffer_size_impl, set_multicast_interface_impl,
    set_multicast_loopback_impl, set_multicast_ttl_impl, set_ttl_impl, source_membership_impl,
};

// Closure thunks referenced by SOCKET_METHODS in this trunk.
pub(crate) use thunks::{
    dgram_add_membership_thunk, dgram_add_source_membership_thunk, dgram_address_thunk,
    dgram_bind_thunk, dgram_close_thunk, dgram_connect_thunk, dgram_disconnect_thunk,
    dgram_drop_membership_thunk, dgram_drop_source_membership_thunk, dgram_emit_thunk,
    dgram_event_names_thunk, dgram_get_recv_buffer_size_thunk, dgram_get_send_buffer_size_thunk,
    dgram_listener_count_thunk, dgram_on_thunk, dgram_once_thunk, dgram_ref_thunk,
    dgram_remote_address_thunk, dgram_remove_listener_thunk, dgram_send_thunk,
    dgram_set_broadcast_thunk, dgram_set_multicast_interface_thunk,
    dgram_set_multicast_loopback_thunk, dgram_set_multicast_ttl_thunk,
    dgram_set_recv_buffer_size_thunk, dgram_set_send_buffer_size_thunk, dgram_set_ttl_thunk,
    dgram_unref_thunk, dgram_zero_thunk,
};

pub(crate) const EVENT_LISTENERS_PREFIX: &[u8] = b"__perryDgramListeners:";
pub(crate) const EVENT_ONCE_PREFIX: &[u8] = b"__perryDgramOnce:";

pub(crate) const KEY_TYPE: &[u8] = b"__perryDgramType";
pub(crate) const KEY_BOUND: &[u8] = b"__perryDgramBound";
pub(crate) const KEY_CLOSED: &[u8] = b"__perryDgramClosed";
pub(crate) const KEY_ADDRESS: &[u8] = b"__perryDgramAddress";
pub(crate) const KEY_FAMILY: &[u8] = b"__perryDgramFamily";
pub(crate) const KEY_PORT: &[u8] = b"__perryDgramPort";
pub(crate) const KEY_CONNECTED: &[u8] = b"__perryDgramConnected";
pub(crate) const KEY_REMOTE_ADDRESS: &[u8] = b"__perryDgramRemoteAddress";
pub(crate) const KEY_REMOTE_FAMILY: &[u8] = b"__perryDgramRemoteFamily";
pub(crate) const KEY_REMOTE_PORT: &[u8] = b"__perryDgramRemotePort";
pub(crate) const KEY_RECV_BUFFER_SIZE: &[u8] = b"__perryDgramRecvBufferSize";
pub(crate) const KEY_SEND_BUFFER_SIZE: &[u8] = b"__perryDgramSendBufferSize";
/// Reactor id for the live OS socket (real mode only); links a JS socket back
/// to its `UdpSocket` + recv thread in [`crate::dgram_reactor`].
pub(crate) const KEY_REACTOR_ID: &[u8] = b"__perryDgramReactorId";

type MethodThunk = extern "C" fn(*const ClosureHeader, f64) -> f64;

struct MethodSpec {
    name: &'static str,
    thunk: MethodThunk,
}

const SOCKET_METHODS: &[MethodSpec] = &[
    MethodSpec {
        name: "send",
        thunk: dgram_send_thunk,
    },
    MethodSpec {
        name: "bind",
        thunk: dgram_bind_thunk,
    },
    MethodSpec {
        name: "close",
        thunk: dgram_close_thunk,
    },
    MethodSpec {
        name: "address",
        thunk: dgram_address_thunk,
    },
    MethodSpec {
        name: "remoteAddress",
        thunk: dgram_remote_address_thunk,
    },
    MethodSpec {
        name: "connect",
        thunk: dgram_connect_thunk,
    },
    MethodSpec {
        name: "disconnect",
        thunk: dgram_disconnect_thunk,
    },
    MethodSpec {
        name: "on",
        thunk: dgram_on_thunk,
    },
    MethodSpec {
        name: "addListener",
        thunk: dgram_on_thunk,
    },
    MethodSpec {
        name: "once",
        thunk: dgram_once_thunk,
    },
    MethodSpec {
        name: "off",
        thunk: dgram_remove_listener_thunk,
    },
    MethodSpec {
        name: "removeListener",
        thunk: dgram_remove_listener_thunk,
    },
    MethodSpec {
        name: "emit",
        thunk: dgram_emit_thunk,
    },
    MethodSpec {
        name: "listenerCount",
        thunk: dgram_listener_count_thunk,
    },
    MethodSpec {
        name: "eventNames",
        thunk: dgram_event_names_thunk,
    },
    MethodSpec {
        name: "addMembership",
        thunk: dgram_add_membership_thunk,
    },
    MethodSpec {
        name: "dropMembership",
        thunk: dgram_drop_membership_thunk,
    },
    MethodSpec {
        name: "addSourceSpecificMembership",
        thunk: dgram_add_source_membership_thunk,
    },
    MethodSpec {
        name: "dropSourceSpecificMembership",
        thunk: dgram_drop_source_membership_thunk,
    },
    MethodSpec {
        name: "setBroadcast",
        thunk: dgram_set_broadcast_thunk,
    },
    MethodSpec {
        name: "setMulticastTTL",
        thunk: dgram_set_multicast_ttl_thunk,
    },
    MethodSpec {
        name: "setMulticastLoopback",
        thunk: dgram_set_multicast_loopback_thunk,
    },
    MethodSpec {
        name: "setMulticastInterface",
        thunk: dgram_set_multicast_interface_thunk,
    },
    MethodSpec {
        name: "setTTL",
        thunk: dgram_set_ttl_thunk,
    },
    MethodSpec {
        name: "setRecvBufferSize",
        thunk: dgram_set_recv_buffer_size_thunk,
    },
    MethodSpec {
        name: "setSendBufferSize",
        thunk: dgram_set_send_buffer_size_thunk,
    },
    MethodSpec {
        name: "getRecvBufferSize",
        thunk: dgram_get_recv_buffer_size_thunk,
    },
    MethodSpec {
        name: "getSendBufferSize",
        thunk: dgram_get_send_buffer_size_thunk,
    },
    MethodSpec {
        name: "getSendQueueSize",
        thunk: dgram_zero_thunk,
    },
    MethodSpec {
        name: "getSendQueueCount",
        thunk: dgram_zero_thunk,
    },
    MethodSpec {
        name: "ref",
        thunk: dgram_ref_thunk,
    },
    MethodSpec {
        name: "unref",
        thunk: dgram_unref_thunk,
    },
];

#[derive(Hash, Eq, PartialEq, Clone)]
pub(crate) struct SocketKey {
    pub(crate) address: String,
    pub(crate) port: u16,
}

#[derive(Default)]
pub(crate) struct DgramRegistry {
    pub(crate) next_port: u16,
    pub(crate) bound: HashMap<SocketKey, f64>,
}

pub(crate) static DGRAM_REGISTRY: LazyLock<Mutex<DgramRegistry>> = LazyLock::new(|| {
    Mutex::new(DgramRegistry {
        next_port: 49152,
        bound: HashMap::new(),
    })
});

pub(crate) fn key(name: &str) -> *mut crate::StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

pub(crate) fn hidden_key(bytes: &[u8]) -> *mut crate::StringHeader {
    crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

pub(crate) fn boxed_pointer(ptr: *const u8) -> f64 {
    f64::from_bits(JSValue::pointer(ptr).bits())
}

pub(crate) fn bool_value(value: bool) -> f64 {
    f64::from_bits(if value { TAG_TRUE } else { TAG_FALSE })
}

pub(crate) fn undefined_value() -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

pub(crate) fn null_value() -> f64 {
    f64::from_bits(TAG_NULL)
}

pub(crate) fn str_value(value: &str) -> f64 {
    let ptr = crate::string::js_string_from_bytes(value.as_ptr(), value.len() as u32);
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

pub(crate) fn raw_ptr_from_value(value: f64) -> usize {
    let bits = value.to_bits();
    let jsval = JSValue::from_bits(bits);
    if jsval.is_pointer() || jsval.is_string() || jsval.is_bigint() {
        return (bits & POINTER_MASK) as usize;
    }
    if bits != 0 && bits < 0x0001_0000_0000_0000 {
        return bits as usize;
    }
    0
}

pub(crate) unsafe fn gc_type_for_ptr(raw: usize) -> Option<u8> {
    if raw < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    let header = (raw as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    let gc_type = (*header).obj_type;
    if gc_type <= crate::gc::GC_TYPE_MAX {
        Some(gc_type)
    } else {
        None
    }
}

pub(crate) fn object_ptr_from_value(value: f64) -> Option<*mut ObjectHeader> {
    let raw = raw_ptr_from_value(value);
    if raw < 0x10000 || crate::buffer::is_registered_buffer(raw) {
        return None;
    }
    unsafe {
        if gc_type_for_ptr(raw) != Some(crate::gc::GC_TYPE_OBJECT) {
            return None;
        }
    }
    Some(raw as *mut ObjectHeader)
}

pub(crate) fn get_hidden_value(value: f64, key: &[u8]) -> Option<f64> {
    let obj = object_ptr_from_value(value)?;
    let value = js_object_get_field_by_name_f64(obj as *const ObjectHeader, hidden_key(key));
    if value.to_bits() == TAG_UNDEFINED {
        None
    } else {
        Some(value)
    }
}

pub(crate) fn set_hidden_value(value: f64, key: &[u8], field_value: f64) {
    if let Some(obj) = object_ptr_from_value(value) {
        js_object_set_field_by_name(obj, hidden_key(key), field_value);
    }
}

pub(crate) fn get_prop(value: f64, name: &str) -> Option<f64> {
    let obj = object_ptr_from_value(value)?;
    let value = js_object_get_field_by_name_f64(obj as *const ObjectHeader, key(name));
    if value.to_bits() == TAG_UNDEFINED {
        None
    } else {
        Some(value)
    }
}

pub(crate) fn string_to_rust(value: f64) -> Option<String> {
    let jsval = JSValue::from_bits(value.to_bits());
    if !jsval.is_any_string() {
        return None;
    }
    let ptr = crate::value::js_get_string_pointer_unified(value) as *const crate::StringHeader;
    if ptr.is_null() || (ptr as usize) < 0x10000 {
        return None;
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        Some(String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).to_string())
    }
}

pub(crate) fn string_eq(value: f64, expected: &[u8]) -> bool {
    let Some(actual) = string_to_rust(value) else {
        return false;
    };
    actual.as_bytes() == expected
}

pub(crate) fn is_callable_value(value: f64) -> bool {
    let raw = raw_ptr_from_value(value);
    raw >= 0x10000 && !crate::closure::get_valid_func_ptr(raw as *const ClosureHeader).is_null()
}

pub(crate) fn collect_args(args: *const ArrayHeader) -> Vec<f64> {
    if args.is_null() {
        return Vec::new();
    }
    let len = crate::array::js_array_length(args);
    let mut out = Vec::with_capacity(len as usize);
    for i in 0..len {
        out.push(crate::array::js_array_get_f64(args, i));
    }
    out
}

pub(crate) fn collect_rest_args(rest: f64) -> Vec<f64> {
    let raw = raw_ptr_from_value(rest);
    if raw < 0x10000 {
        return Vec::new();
    }
    collect_args(raw as *const ArrayHeader)
}

pub(crate) fn this_value(closure: *const ClosureHeader) -> f64 {
    if !closure.is_null() {
        let bits = crate::closure::js_closure_get_capture_ptr(closure, 0) as u64;
        if bits != 0 {
            return f64::from_bits(bits);
        }
    }
    crate::object::js_implicit_this_get()
}

pub(crate) fn socket_value_from_handle(handle: i64) -> f64 {
    if handle == 0 {
        return undefined_value();
    }
    let bits = handle as u64;
    if (bits >> 48) >= 0x7FF8 {
        f64::from_bits(bits)
    } else {
        boxed_pointer(handle as *const u8)
    }
}

pub(crate) fn method_value(socket: f64, name: &str, thunk: MethodThunk) -> f64 {
    let func_ptr = thunk as *const u8;
    let closure = js_closure_alloc(func_ptr, 1);
    js_closure_set_capture_ptr(closure, 0, socket.to_bits() as i64);
    js_register_closure_rest(func_ptr, 0);
    crate::object::set_bound_native_closure_name(closure, name);
    js_nanbox_pointer(closure as i64)
}

pub(crate) fn socket_object(socket_type: &str) -> f64 {
    let obj = js_object_alloc(0, SOCKET_METHODS.len() as u32 + 12);
    let socket = boxed_pointer(obj as *const u8);
    set_hidden_value(socket, KEY_TYPE, str_value(socket_type));
    set_hidden_value(socket, KEY_BOUND, bool_value(false));
    set_hidden_value(socket, KEY_CLOSED, bool_value(false));
    set_hidden_value(socket, KEY_CONNECTED, bool_value(false));
    set_hidden_value(socket, KEY_FAMILY, str_value(family_for_type(socket_type)));
    set_hidden_value(socket, KEY_PORT, 0.0);
    set_hidden_value(socket, KEY_REMOTE_PORT, 0.0);
    set_hidden_value(socket, KEY_RECV_BUFFER_SIZE, 65536.0);
    set_hidden_value(socket, KEY_SEND_BUFFER_SIZE, 65536.0);
    for method in SOCKET_METHODS {
        js_object_set_field_by_name(
            obj,
            key(method.name),
            method_value(socket, method.name, method.thunk),
        );
    }
    socket
}

pub(crate) fn family_for_type(socket_type: &str) -> &'static str {
    if socket_type == "udp6" {
        "IPv6"
    } else {
        "IPv4"
    }
}

pub(crate) fn default_bind_address(socket: f64) -> String {
    if string_eq(
        get_hidden_value(socket, KEY_TYPE).unwrap_or_else(|| str_value("udp4")),
        b"udp6",
    ) {
        "::".to_string()
    } else {
        "0.0.0.0".to_string()
    }
}

pub(crate) fn default_loopback_address(socket: f64) -> String {
    if string_eq(
        get_hidden_value(socket, KEY_TYPE).unwrap_or_else(|| str_value("udp4")),
        b"udp6",
    ) {
        "::1".to_string()
    } else {
        "127.0.0.1".to_string()
    }
}

pub(crate) fn family_for_address(address: &str, socket: f64) -> &'static str {
    if address.contains(':')
        || string_eq(get_hidden_value(socket, KEY_TYPE).unwrap_or(0.0), b"udp6")
    {
        "IPv6"
    } else {
        "IPv4"
    }
}

pub(crate) fn normalize_address(address: &str, socket: f64) -> String {
    match address {
        "localhost" => default_loopback_address(socket),
        "" => default_bind_address(socket),
        other => other.to_string(),
    }
}

pub(crate) fn hidden_string(socket: f64, key: &[u8]) -> Option<String> {
    string_to_rust(get_hidden_value(socket, key)?)
}

pub(crate) fn hidden_port(socket: f64, key: &[u8]) -> u16 {
    get_hidden_value(socket, key).unwrap_or(0.0) as u16
}

pub(crate) fn is_truthy_hidden(socket: f64, key: &[u8]) -> bool {
    get_hidden_value(socket, key).is_some_and(|v| crate::value::js_is_truthy(v) != 0)
}

pub(crate) fn is_number_like(value: f64) -> bool {
    let jsval = JSValue::from_bits(value.to_bits());
    jsval.is_int32() || jsval.is_number()
}

pub(crate) fn number_value(value: f64) -> Option<f64> {
    let jsval = JSValue::from_bits(value.to_bits());
    if jsval.is_int32() {
        Some(jsval.as_int32() as f64)
    } else if jsval.is_number() {
        Some(value)
    } else {
        None
    }
}

pub(crate) fn format_received_number(n: f64) -> String {
    if n.is_nan() {
        return "NaN".to_string();
    }
    if n.is_infinite() {
        return if n.is_sign_negative() {
            "-Infinity"
        } else {
            "Infinity"
        }
        .to_string();
    }
    if n.fract() == 0.0 && n.abs() < 1e21 {
        format!("{}", n as i64)
    } else {
        format!("{}", n)
    }
}

pub(crate) fn port_from_value(value: f64, allow_zero: bool) -> u16 {
    let Some(n) = number_value(value) else {
        throw_bad_port(value, allow_zero);
    };
    let lower_ok = if allow_zero { n >= 0.0 } else { n > 0.0 };
    if n.is_finite() && n.fract() == 0.0 && lower_ok && n < 65536.0 {
        return n as u16;
    }
    throw_bad_port(value, allow_zero)
}

pub(crate) fn throw_bad_port(value: f64, allow_zero: bool) -> ! {
    let received = if let Some(n) = number_value(value) {
        format!("type number ({})", format_received_number(n))
    } else {
        crate::fs::validate::describe_received(value)
    };
    let op = if allow_zero { ">=" } else { ">" };
    let message = format!("Port should be {op} 0 and < 65536. Received {received}.");
    crate::fs::validate::throw_range_error_named(&message, "ERR_SOCKET_BAD_PORT")
}

pub(crate) fn throw_bad_socket_type(value: f64) -> ! {
    let received = crate::fs::validate::describe_received(value);
    let message =
        format!("Bad socket type specified. Valid types are: udp4, udp6. Received {received}");
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_SOCKET_BAD_TYPE")
}

pub(crate) fn throw_invalid_message(value: f64) -> ! {
    let message = format!(
        "The \"msg\" argument must be an instance of Buffer, TypedArray, DataView, or a string. Received {}",
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

pub(crate) fn throw_invalid_listener(value: f64) -> ! {
    let message = format!(
        "The \"listener\" argument must be of type function. Received {}",
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

pub(crate) fn throw_not_bound() -> ! {
    crate::fs::validate::throw_error_with_code("getsockname EBADF", "EBADF")
}

pub(crate) fn throw_not_connected() -> ! {
    crate::fs::validate::throw_error_with_code("Not connected", "ERR_SOCKET_DGRAM_NOT_CONNECTED")
}

pub(crate) fn throw_socket_errno(syscall: &'static str, code: &'static str) -> ! {
    let message = format!("{syscall} {code}");
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    crate::node_submodules::register_error_code_pub(msg, code);
    crate::node_submodules::register_error_syscall(msg, syscall);
    let err = crate::error::js_error_new_with_message(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

pub(crate) fn throw_socket_buffer_size(syscall: &'static str) -> ! {
    let message =
        format!("Could not get or set buffer size: {syscall} returned EBADF (bad file descriptor)");
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    crate::node_submodules::register_error_code_pub(msg, "ERR_SOCKET_BUFFER_SIZE");
    crate::node_submodules::register_error_syscall(msg, syscall);
    let err = crate::error::js_error_new_with_message(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

pub(crate) fn throw_invalid_arg_type(arg_name: &str, expected: &str, value: f64) -> ! {
    let message = format!(
        "The \"{}\" argument must be of type {}. Received {}",
        arg_name,
        expected,
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

pub(crate) fn throw_missing_arg(arg_name: &str) -> ! {
    let message = format!("The \"{arg_name}\" argument must be specified");
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_MISSING_ARGS")
}

pub(crate) fn throw_bad_buffer_size() -> ! {
    crate::fs::validate::throw_type_error_with_code(
        "Buffer size must be a positive integer",
        "ERR_SOCKET_BAD_BUFFER_SIZE",
    )
}

pub(crate) fn ensure_running(socket: f64, syscall: &'static str) {
    if !is_truthy_hidden(socket, KEY_BOUND) {
        throw_socket_errno(syscall, "EBADF");
    }
}

pub(crate) fn ensure_buffer_running(socket: f64, syscall: &'static str) {
    if !is_truthy_hidden(socket, KEY_BOUND) {
        throw_socket_buffer_size(syscall);
    }
}

pub(crate) fn validate_number_arg(value: f64, arg_name: &str) -> f64 {
    number_value(value).unwrap_or_else(|| throw_invalid_arg_type(arg_name, "number", value))
}

pub(crate) fn validate_string_arg(value: f64, arg_name: &str) -> String {
    string_to_rust(value).unwrap_or_else(|| throw_invalid_arg_type(arg_name, "string", value))
}

pub(crate) fn is_missing_membership_arg(value: f64) -> bool {
    let jsval = JSValue::from_bits(value.to_bits());
    jsval.is_undefined() || jsval.is_null() || (jsval.is_bool() && !jsval.as_bool())
}

pub(crate) fn callback_from_args(args: &[f64]) -> Option<f64> {
    args.iter()
        .rev()
        .copied()
        .find(|value| is_callable_value(*value))
}

pub(crate) fn call_function(callback: f64, this: f64, args: &[f64]) -> f64 {
    if !is_callable_value(callback) {
        return undefined_value();
    }
    let prev = crate::object::js_implicit_this_set(this);
    let result =
        unsafe { crate::closure::js_native_call_value(callback, args.as_ptr(), args.len()) };
    crate::object::js_implicit_this_set(prev);
    result
}
