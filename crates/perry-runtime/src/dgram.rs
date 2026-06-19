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
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::{Arc, LazyLock, Mutex};

use crate::array::ArrayHeader;
use crate::closure::{
    js_closure_alloc, js_closure_set_capture_ptr, js_register_closure_rest, ClosureHeader,
};
use crate::object::{
    js_object_alloc, js_object_get_field_by_name_f64, js_object_keys, js_object_set_field_by_name,
    ObjectHeader,
};
use crate::value::{
    js_nanbox_pointer, JSValue, POINTER_MASK, TAG_FALSE, TAG_NULL, TAG_TRUE, TAG_UNDEFINED,
};

const EVENT_LISTENERS_PREFIX: &[u8] = b"__perryDgramListeners:";
const EVENT_ONCE_PREFIX: &[u8] = b"__perryDgramOnce:";

const KEY_TYPE: &[u8] = b"__perryDgramType";
const KEY_BOUND: &[u8] = b"__perryDgramBound";
const KEY_CLOSED: &[u8] = b"__perryDgramClosed";
const KEY_ADDRESS: &[u8] = b"__perryDgramAddress";
const KEY_FAMILY: &[u8] = b"__perryDgramFamily";
const KEY_PORT: &[u8] = b"__perryDgramPort";
const KEY_CONNECTED: &[u8] = b"__perryDgramConnected";
const KEY_REMOTE_ADDRESS: &[u8] = b"__perryDgramRemoteAddress";
const KEY_REMOTE_FAMILY: &[u8] = b"__perryDgramRemoteFamily";
const KEY_REMOTE_PORT: &[u8] = b"__perryDgramRemotePort";
const KEY_RECV_BUFFER_SIZE: &[u8] = b"__perryDgramRecvBufferSize";
const KEY_SEND_BUFFER_SIZE: &[u8] = b"__perryDgramSendBufferSize";
/// Reactor id for the live OS socket (real mode only); links a JS socket back
/// to its `UdpSocket` + recv thread in [`crate::dgram_reactor`].
const KEY_REACTOR_ID: &[u8] = b"__perryDgramReactorId";

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
struct SocketKey {
    address: String,
    port: u16,
}

#[derive(Default)]
struct DgramRegistry {
    next_port: u16,
    bound: HashMap<SocketKey, f64>,
}

static DGRAM_REGISTRY: LazyLock<Mutex<DgramRegistry>> = LazyLock::new(|| {
    Mutex::new(DgramRegistry {
        next_port: 49152,
        bound: HashMap::new(),
    })
});

fn key(name: &str) -> *mut crate::StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

fn hidden_key(bytes: &[u8]) -> *mut crate::StringHeader {
    crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

fn boxed_pointer(ptr: *const u8) -> f64 {
    f64::from_bits(JSValue::pointer(ptr).bits())
}

fn bool_value(value: bool) -> f64 {
    f64::from_bits(if value { TAG_TRUE } else { TAG_FALSE })
}

fn undefined_value() -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

fn null_value() -> f64 {
    f64::from_bits(TAG_NULL)
}

fn str_value(value: &str) -> f64 {
    let ptr = crate::string::js_string_from_bytes(value.as_ptr(), value.len() as u32);
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

fn raw_ptr_from_value(value: f64) -> usize {
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

unsafe fn gc_type_for_ptr(raw: usize) -> Option<u8> {
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

fn object_ptr_from_value(value: f64) -> Option<*mut ObjectHeader> {
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

fn get_hidden_value(value: f64, key: &[u8]) -> Option<f64> {
    let obj = object_ptr_from_value(value)?;
    let value = js_object_get_field_by_name_f64(obj as *const ObjectHeader, hidden_key(key));
    if value.to_bits() == TAG_UNDEFINED {
        None
    } else {
        Some(value)
    }
}

fn set_hidden_value(value: f64, key: &[u8], field_value: f64) {
    if let Some(obj) = object_ptr_from_value(value) {
        js_object_set_field_by_name(obj, hidden_key(key), field_value);
    }
}

fn get_prop(value: f64, name: &str) -> Option<f64> {
    let obj = object_ptr_from_value(value)?;
    let value = js_object_get_field_by_name_f64(obj as *const ObjectHeader, key(name));
    if value.to_bits() == TAG_UNDEFINED {
        None
    } else {
        Some(value)
    }
}

fn string_to_rust(value: f64) -> Option<String> {
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

fn string_eq(value: f64, expected: &[u8]) -> bool {
    let Some(actual) = string_to_rust(value) else {
        return false;
    };
    actual.as_bytes() == expected
}

fn is_callable_value(value: f64) -> bool {
    let raw = raw_ptr_from_value(value);
    raw >= 0x10000 && !crate::closure::get_valid_func_ptr(raw as *const ClosureHeader).is_null()
}

fn collect_args(args: *const ArrayHeader) -> Vec<f64> {
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

fn collect_rest_args(rest: f64) -> Vec<f64> {
    let raw = raw_ptr_from_value(rest);
    if raw < 0x10000 {
        return Vec::new();
    }
    collect_args(raw as *const ArrayHeader)
}

fn this_value(closure: *const ClosureHeader) -> f64 {
    if !closure.is_null() {
        let bits = crate::closure::js_closure_get_capture_ptr(closure, 0) as u64;
        if bits != 0 {
            return f64::from_bits(bits);
        }
    }
    crate::object::js_implicit_this_get()
}

fn socket_value_from_handle(handle: i64) -> f64 {
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

fn method_value(socket: f64, name: &str, thunk: MethodThunk) -> f64 {
    let func_ptr = thunk as *const u8;
    let closure = js_closure_alloc(func_ptr, 1);
    js_closure_set_capture_ptr(closure, 0, socket.to_bits() as i64);
    js_register_closure_rest(func_ptr, 0);
    crate::object::set_bound_native_closure_name(closure, name);
    js_nanbox_pointer(closure as i64)
}

fn socket_object(socket_type: &str) -> f64 {
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

fn family_for_type(socket_type: &str) -> &'static str {
    if socket_type == "udp6" {
        "IPv6"
    } else {
        "IPv4"
    }
}

fn default_bind_address(socket: f64) -> String {
    if string_eq(
        get_hidden_value(socket, KEY_TYPE).unwrap_or_else(|| str_value("udp4")),
        b"udp6",
    ) {
        "::".to_string()
    } else {
        "0.0.0.0".to_string()
    }
}

fn default_loopback_address(socket: f64) -> String {
    if string_eq(
        get_hidden_value(socket, KEY_TYPE).unwrap_or_else(|| str_value("udp4")),
        b"udp6",
    ) {
        "::1".to_string()
    } else {
        "127.0.0.1".to_string()
    }
}

fn family_for_address(address: &str, socket: f64) -> &'static str {
    if address.contains(':')
        || string_eq(get_hidden_value(socket, KEY_TYPE).unwrap_or(0.0), b"udp6")
    {
        "IPv6"
    } else {
        "IPv4"
    }
}

fn normalize_address(address: &str, socket: f64) -> String {
    match address {
        "localhost" => default_loopback_address(socket),
        "" => default_bind_address(socket),
        other => other.to_string(),
    }
}

fn hidden_string(socket: f64, key: &[u8]) -> Option<String> {
    string_to_rust(get_hidden_value(socket, key)?)
}

fn hidden_port(socket: f64, key: &[u8]) -> u16 {
    get_hidden_value(socket, key).unwrap_or(0.0) as u16
}

fn is_truthy_hidden(socket: f64, key: &[u8]) -> bool {
    get_hidden_value(socket, key).is_some_and(|v| crate::value::js_is_truthy(v) != 0)
}

fn is_number_like(value: f64) -> bool {
    let jsval = JSValue::from_bits(value.to_bits());
    jsval.is_int32() || jsval.is_number()
}

fn number_value(value: f64) -> Option<f64> {
    let jsval = JSValue::from_bits(value.to_bits());
    if jsval.is_int32() {
        Some(jsval.as_int32() as f64)
    } else if jsval.is_number() {
        Some(value)
    } else {
        None
    }
}

fn format_received_number(n: f64) -> String {
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

fn port_from_value(value: f64, allow_zero: bool) -> u16 {
    let Some(n) = number_value(value) else {
        throw_bad_port(value, allow_zero);
    };
    let lower_ok = if allow_zero { n >= 0.0 } else { n > 0.0 };
    if n.is_finite() && n.fract() == 0.0 && lower_ok && n < 65536.0 {
        return n as u16;
    }
    throw_bad_port(value, allow_zero)
}

fn throw_bad_port(value: f64, allow_zero: bool) -> ! {
    let received = if let Some(n) = number_value(value) {
        format!("type number ({})", format_received_number(n))
    } else {
        crate::fs::validate::describe_received(value)
    };
    let op = if allow_zero { ">=" } else { ">" };
    let message = format!("Port should be {op} 0 and < 65536. Received {received}.");
    crate::fs::validate::throw_range_error_named(&message, "ERR_SOCKET_BAD_PORT")
}

fn throw_bad_socket_type(value: f64) -> ! {
    let received = crate::fs::validate::describe_received(value);
    let message =
        format!("Bad socket type specified. Valid types are: udp4, udp6. Received {received}");
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_SOCKET_BAD_TYPE")
}

fn throw_invalid_message(value: f64) -> ! {
    let message = format!(
        "The \"msg\" argument must be an instance of Buffer, TypedArray, DataView, or a string. Received {}",
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

fn throw_invalid_listener(value: f64) -> ! {
    let message = format!(
        "The \"listener\" argument must be of type function. Received {}",
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

fn throw_not_bound() -> ! {
    crate::fs::validate::throw_error_with_code("getsockname EBADF", "EBADF")
}

fn throw_not_connected() -> ! {
    crate::fs::validate::throw_error_with_code("Not connected", "ERR_SOCKET_DGRAM_NOT_CONNECTED")
}

fn throw_socket_errno(syscall: &'static str, code: &'static str) -> ! {
    let message = format!("{syscall} {code}");
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    crate::node_submodules::register_error_code_pub(msg, code);
    crate::node_submodules::register_error_syscall(msg, syscall);
    let err = crate::error::js_error_new_with_message(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

fn throw_socket_buffer_size(syscall: &'static str) -> ! {
    let message =
        format!("Could not get or set buffer size: {syscall} returned EBADF (bad file descriptor)");
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    crate::node_submodules::register_error_code_pub(msg, "ERR_SOCKET_BUFFER_SIZE");
    crate::node_submodules::register_error_syscall(msg, syscall);
    let err = crate::error::js_error_new_with_message(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

fn throw_invalid_arg_type(arg_name: &str, expected: &str, value: f64) -> ! {
    let message = format!(
        "The \"{}\" argument must be of type {}. Received {}",
        arg_name,
        expected,
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

fn throw_missing_arg(arg_name: &str) -> ! {
    let message = format!("The \"{arg_name}\" argument must be specified");
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_MISSING_ARGS")
}

fn throw_bad_buffer_size() -> ! {
    crate::fs::validate::throw_type_error_with_code(
        "Buffer size must be a positive integer",
        "ERR_SOCKET_BAD_BUFFER_SIZE",
    )
}

fn ensure_running(socket: f64, syscall: &'static str) {
    if !is_truthy_hidden(socket, KEY_BOUND) {
        throw_socket_errno(syscall, "EBADF");
    }
}

fn ensure_buffer_running(socket: f64, syscall: &'static str) {
    if !is_truthy_hidden(socket, KEY_BOUND) {
        throw_socket_buffer_size(syscall);
    }
}

fn validate_number_arg(value: f64, arg_name: &str) -> f64 {
    number_value(value).unwrap_or_else(|| throw_invalid_arg_type(arg_name, "number", value))
}

fn validate_string_arg(value: f64, arg_name: &str) -> String {
    string_to_rust(value).unwrap_or_else(|| throw_invalid_arg_type(arg_name, "string", value))
}

fn is_missing_membership_arg(value: f64) -> bool {
    let jsval = JSValue::from_bits(value.to_bits());
    jsval.is_undefined() || jsval.is_null() || (jsval.is_bool() && !jsval.as_bool())
}

fn callback_from_args(args: &[f64]) -> Option<f64> {
    args.iter()
        .rev()
        .copied()
        .find(|value| is_callable_value(*value))
}

fn call_function(callback: f64, this: f64, args: &[f64]) -> f64 {
    if !is_callable_value(callback) {
        return undefined_value();
    }
    let prev = crate::object::js_implicit_this_set(this);
    let result =
        unsafe { crate::closure::js_native_call_value(callback, args.as_ptr(), args.len()) };
    crate::object::js_implicit_this_set(prev);
    result
}

fn listener_event_key(prefix: &[u8], event: f64) -> Option<*mut crate::StringHeader> {
    let event = string_to_rust(event)?;
    let mut bytes = prefix.to_vec();
    bytes.extend_from_slice(event.as_bytes());
    Some(hidden_key(&bytes))
}

fn listener_storage(socket: f64, event: f64) -> Option<(f64, f64)> {
    let listener_key = listener_event_key(EVENT_LISTENERS_PREFIX, event)?;
    let once_key = listener_event_key(EVENT_ONCE_PREFIX, event)?;
    let listeners = {
        let obj = object_ptr_from_value(socket)?;
        let value = js_object_get_field_by_name_f64(obj as *const ObjectHeader, listener_key);
        if value.to_bits() == TAG_UNDEFINED {
            return None;
        }
        value
    };
    let once = {
        let obj = object_ptr_from_value(socket)?;
        let value = js_object_get_field_by_name_f64(obj as *const ObjectHeader, once_key);
        if value.to_bits() == TAG_UNDEFINED {
            return None;
        }
        value
    };
    Some((listeners, once))
}

fn ensure_listener_storage(socket: f64, event: f64) -> Option<(f64, f64)> {
    let listener_key = listener_event_key(EVENT_LISTENERS_PREFIX, event)?;
    let once_key = listener_event_key(EVENT_ONCE_PREFIX, event)?;
    let obj = object_ptr_from_value(socket)?;
    let listeners = {
        let value = js_object_get_field_by_name_f64(obj as *const ObjectHeader, listener_key);
        if value.to_bits() == TAG_UNDEFINED {
            let arr = crate::array::js_array_alloc(0);
            let arr_value = boxed_pointer(arr as *const u8);
            js_object_set_field_by_name(obj, listener_key, arr_value);
            arr_value
        } else {
            value
        }
    };
    let once = {
        let value = js_object_get_field_by_name_f64(obj as *const ObjectHeader, once_key);
        if value.to_bits() == TAG_UNDEFINED {
            let arr = crate::array::js_array_alloc(0);
            let arr_value = boxed_pointer(arr as *const u8);
            js_object_set_field_by_name(obj, once_key, arr_value);
            arr_value
        } else {
            value
        }
    };
    Some((listeners, once))
}

fn set_listener_storage(socket: f64, event: f64, listeners: f64, once: f64) {
    let Some(obj) = object_ptr_from_value(socket) else {
        return;
    };
    if let Some(listener_key) = listener_event_key(EVENT_LISTENERS_PREFIX, event) {
        js_object_set_field_by_name(obj, listener_key, listeners);
    }
    if let Some(once_key) = listener_event_key(EVENT_ONCE_PREFIX, event) {
        js_object_set_field_by_name(obj, once_key, once);
    }
}

fn add_listener(socket: f64, event: f64, listener: f64, once: bool) {
    if string_to_rust(event).is_none() {
        return;
    }
    if !is_callable_value(listener) {
        throw_invalid_listener(listener);
    }
    let Some((listeners, once_flags)) = ensure_listener_storage(socket, event) else {
        return;
    };
    let listeners_raw = raw_ptr_from_value(listeners) as *const ArrayHeader;
    let once_raw = raw_ptr_from_value(once_flags) as *const ArrayHeader;
    let len = crate::array::js_array_length(listeners_raw);
    let mut out_listeners = crate::array::js_array_alloc(len + 1);
    let mut out_once = crate::array::js_array_alloc(len + 1);
    for i in 0..len {
        out_listeners = crate::array::js_array_push_f64(
            out_listeners,
            crate::array::js_array_get_f64(listeners_raw, i),
        );
        out_once =
            crate::array::js_array_push_f64(out_once, crate::array::js_array_get_f64(once_raw, i));
    }
    out_listeners = crate::array::js_array_push_f64(out_listeners, listener);
    out_once = crate::array::js_array_push_f64(out_once, bool_value(once));
    set_listener_storage(
        socket,
        event,
        boxed_pointer(out_listeners as *const u8),
        boxed_pointer(out_once as *const u8),
    );
}

fn listener_snapshot(socket: f64, event: f64) -> Vec<(f64, bool)> {
    let Some((listeners, once_flags)) = listener_storage(socket, event) else {
        return Vec::new();
    };
    let listeners_raw = raw_ptr_from_value(listeners) as *const ArrayHeader;
    let once_raw = raw_ptr_from_value(once_flags) as *const ArrayHeader;
    if listeners_raw.is_null() || once_raw.is_null() {
        return Vec::new();
    }
    let len = crate::array::js_array_length(listeners_raw);
    let mut out = Vec::with_capacity(len as usize);
    for i in 0..len {
        out.push((
            crate::array::js_array_get_f64(listeners_raw, i),
            crate::value::js_is_truthy(crate::array::js_array_get_f64(once_raw, i)) != 0,
        ));
    }
    out
}

fn remove_listener(socket: f64, event: f64, listener: f64) -> bool {
    let Some((listeners, once_flags)) = listener_storage(socket, event) else {
        return false;
    };
    let listeners_raw = raw_ptr_from_value(listeners) as *const ArrayHeader;
    let once_raw = raw_ptr_from_value(once_flags) as *const ArrayHeader;
    if listeners_raw.is_null() || once_raw.is_null() {
        return false;
    }
    let len = crate::array::js_array_length(listeners_raw);
    let mut remove_idx = None;
    for i in (0..len).rev() {
        if crate::array::js_array_get_f64(listeners_raw, i).to_bits() == listener.to_bits() {
            remove_idx = Some(i);
            break;
        }
    }
    let Some(remove_idx) = remove_idx else {
        return false;
    };
    let mut out_listeners = crate::array::js_array_alloc(len.saturating_sub(1));
    let mut out_once = crate::array::js_array_alloc(len.saturating_sub(1));
    for i in 0..len {
        if i == remove_idx {
            continue;
        }
        out_listeners = crate::array::js_array_push_f64(
            out_listeners,
            crate::array::js_array_get_f64(listeners_raw, i),
        );
        out_once =
            crate::array::js_array_push_f64(out_once, crate::array::js_array_get_f64(once_raw, i));
    }
    set_listener_storage(
        socket,
        event,
        boxed_pointer(out_listeners as *const u8),
        boxed_pointer(out_once as *const u8),
    );
    true
}

fn remove_once_listeners(socket: f64, event: f64) {
    let Some((listeners, once_flags)) = listener_storage(socket, event) else {
        return;
    };
    let listeners_raw = raw_ptr_from_value(listeners) as *const ArrayHeader;
    let once_raw = raw_ptr_from_value(once_flags) as *const ArrayHeader;
    if listeners_raw.is_null() || once_raw.is_null() {
        return;
    }
    let len = crate::array::js_array_length(listeners_raw);
    let mut out_listeners = crate::array::js_array_alloc(len);
    let mut out_once = crate::array::js_array_alloc(len);
    for i in 0..len {
        let once = crate::value::js_is_truthy(crate::array::js_array_get_f64(once_raw, i)) != 0;
        if !once {
            out_listeners = crate::array::js_array_push_f64(
                out_listeners,
                crate::array::js_array_get_f64(listeners_raw, i),
            );
            out_once = crate::array::js_array_push_f64(
                out_once,
                crate::array::js_array_get_f64(once_raw, i),
            );
        }
    }
    set_listener_storage(
        socket,
        event,
        boxed_pointer(out_listeners as *const u8),
        boxed_pointer(out_once as *const u8),
    );
}

fn emit_event_value(socket: f64, event: f64, args: &[f64]) -> bool {
    let snapshot = listener_snapshot(socket, event);
    if snapshot.is_empty() {
        return false;
    }
    if snapshot.iter().any(|(_, once)| *once) {
        remove_once_listeners(socket, event);
    }
    for (listener, _) in snapshot {
        call_function(listener, socket, args);
    }
    true
}

fn emit_event(socket: f64, event: &str, args: &[f64]) -> bool {
    emit_event_value(socket, str_value(event), args)
}

/// `socket.eventNames()` — the list of events with at least one registered
/// listener, in registration order. Recomputed from the socket's hidden
/// listener-storage fields (keyed by `EVENT_LISTENERS_PREFIX`) so it self-
/// corrects when `once` listeners fire or listeners are removed, matching
/// Node's EventEmitter.eventNames().
fn event_names_impl(socket: f64) -> f64 {
    let Some(obj) = object_ptr_from_value(socket) else {
        return boxed_pointer(crate::array::js_array_alloc(0) as *const u8);
    };
    let keys = js_object_keys(obj);
    let mut out = crate::array::js_array_alloc(0);
    if !keys.is_null() {
        let len = crate::array::js_array_length(keys);
        for i in 0..len {
            let Some(key_name) = string_to_rust(crate::array::js_array_get_f64(keys, i)) else {
                continue;
            };
            let Some(event) = key_name
                .as_bytes()
                .strip_prefix(EVENT_LISTENERS_PREFIX)
                .map(|rest| String::from_utf8_lossy(rest).into_owned())
            else {
                continue;
            };
            let event_value = str_value(&event);
            if !listener_snapshot(socket, event_value).is_empty() {
                out = crate::array::js_array_push_f64(out, event_value);
            }
        }
    }
    boxed_pointer(out as *const u8)
}

fn allocate_port(registry: &mut DgramRegistry, address: &str) -> u16 {
    for _ in 0..16384 {
        let port = registry.next_port;
        registry.next_port = if registry.next_port >= 65535 {
            49152
        } else {
            registry.next_port + 1
        };
        if !registry.bound.contains_key(&SocketKey {
            address: address.to_string(),
            port,
        }) {
            return port;
        }
    }
    49152
}

fn remove_bound_socket(socket: f64) {
    if !is_truthy_hidden(socket, KEY_BOUND) {
        return;
    }
    let Some(address) = hidden_string(socket, KEY_ADDRESS) else {
        return;
    };
    let port = hidden_port(socket, KEY_PORT);
    let key = SocketKey { address, port };
    if let Ok(mut registry) = DGRAM_REGISTRY.lock() {
        if registry
            .bound
            .get(&key)
            .is_some_and(|value| value.to_bits() == socket.to_bits())
        {
            registry.bound.remove(&key);
        }
    }
}

fn bind_socket(socket: f64, port: u16, address: String) -> u16 {
    let address = normalize_address(&address, socket);
    let family = family_for_address(&address, socket);
    remove_bound_socket(socket);
    let actual_port = if let Ok(mut registry) = DGRAM_REGISTRY.lock() {
        let actual_port = if port == 0 {
            allocate_port(&mut registry, &address)
        } else {
            port
        };
        registry.bound.insert(
            SocketKey {
                address: address.clone(),
                port: actual_port,
            },
            socket,
        );
        actual_port
    } else {
        port
    };
    set_hidden_value(socket, KEY_ADDRESS, str_value(&address));
    set_hidden_value(socket, KEY_FAMILY, str_value(family));
    set_hidden_value(socket, KEY_PORT, actual_port as f64);
    set_hidden_value(socket, KEY_BOUND, bool_value(true));
    actual_port
}

fn ensure_bound(socket: f64) {
    if is_truthy_hidden(socket, KEY_BOUND) {
        return;
    }
    if deterministic() {
        bind_socket(socket, 0, default_loopback_address(socket));
    } else {
        let _ = real_bind(socket, 0, &default_bind_address(socket));
    }
}

fn lookup_bound_socket(address: &str, port: u16, socket: f64) -> Option<f64> {
    let address = normalize_address(address, socket);
    let fallbacks: &[&str] = if address.contains(':') {
        &[address.as_str(), "::"]
    } else {
        &[address.as_str(), "0.0.0.0"]
    };
    let registry = DGRAM_REGISTRY.lock().ok()?;
    for candidate in fallbacks {
        let key = SocketKey {
            address: (*candidate).to_string(),
            port,
        };
        if let Some(value) = registry.bound.get(&key) {
            return Some(*value);
        }
    }
    None
}

fn build_address_info(address: &str, family: &str, port: u16) -> f64 {
    let obj = js_object_alloc(0, 3);
    js_object_set_field_by_name(obj, key("address"), str_value(address));
    js_object_set_field_by_name(obj, key("family"), str_value(family));
    js_object_set_field_by_name(obj, key("port"), port as f64);
    boxed_pointer(obj as *const u8)
}

fn build_rinfo(address: &str, family: &str, port: u16, size: usize) -> f64 {
    let obj = js_object_alloc(0, 4);
    js_object_set_field_by_name(obj, key("address"), str_value(address));
    js_object_set_field_by_name(obj, key("family"), str_value(family));
    js_object_set_field_by_name(obj, key("port"), port as f64);
    js_object_set_field_by_name(obj, key("size"), size as f64);
    boxed_pointer(obj as *const u8)
}

fn message_value(value: f64) -> Option<(f64, usize)> {
    let jsval = JSValue::from_bits(value.to_bits());
    if jsval.is_any_string() {
        let ptr = crate::value::js_get_string_pointer_unified(value) as *const crate::StringHeader;
        if ptr.is_null() {
            return None;
        }
        let buf = crate::buffer::js_buffer_from_string(ptr, 0);
        let len = unsafe { (*buf).length as usize };
        return Some((boxed_pointer(buf as *const u8), len));
    }
    let raw = raw_ptr_from_value(value);
    if raw >= 0x10000 && crate::buffer::is_registered_buffer(raw) {
        let buf = raw as *const crate::buffer::BufferHeader;
        return Some((value, unsafe { (*buf).length as usize }));
    }
    if raw >= 0x10000 && crate::typedarray::lookup_typed_array_kind(raw).is_some() {
        let len = unsafe {
            crate::typedarray::typed_array_bytes(raw as *const crate::typedarray::TypedArrayHeader)
                .map(|bytes| bytes.len())
                .unwrap_or(0)
        };
        return Some((value, len));
    }
    None
}

/// Whether `PERRY_DETERMINISTIC_NET=1` — use the in-process loopback registry
/// instead of real OS sockets (#4911).
fn deterministic() -> bool {
    crate::stub_diag::deterministic_net_enabled()
}

/// The reactor id stashed on a real-mode socket, if it is bound.
fn reactor_id(socket: f64) -> Option<u64> {
    get_hidden_value(socket, KEY_REACTOR_ID)
        .and_then(number_value)
        .map(|n| n as u64)
}

fn live_udp(socket: f64) -> Option<Arc<UdpSocket>> {
    crate::dgram_reactor::udp_for(reactor_id(socket)?)
}

/// Build a `Buffer` JS value from raw datagram bytes.
fn make_buffer(data: &[u8]) -> f64 {
    let buf = crate::buffer::js_buffer_alloc(data.len() as i32, 0);
    unsafe {
        if !buf.is_null() {
            if !data.is_empty() {
                let dst = (buf as *mut u8).add(std::mem::size_of::<crate::buffer::BufferHeader>());
                // GC_STORE_AUDIT(POINTER_FREE): raw datagram bytes copied into a
                // freshly-allocated Buffer payload — u8 data, never heap pointers.
                std::ptr::copy_nonoverlapping(data.as_ptr(), dst, data.len());
            }
            (*buf).length = data.len() as u32;
        }
    }
    boxed_pointer(buf as *const u8)
}

/// Deliver one received datagram to its socket as a `'message'` event. Called
/// on the main thread from [`crate::dgram_reactor::pump`]. The `Buffer` is
/// GC-rooted across the `rinfo` allocation so a collection between the two
/// can't reclaim it.
pub(crate) fn dgram_emit_message(
    socket_bits: u64,
    data: &[u8],
    src_ip: &str,
    src_port: u16,
    src_family: &str,
) {
    let socket = f64::from_bits(socket_bits);
    let scope = crate::gc::RuntimeHandleScope::new();
    let buffer = scope.root_nanbox_f64(make_buffer(data));
    let rinfo = scope.root_nanbox_f64(build_rinfo(src_ip, src_family, src_port, data.len()));
    emit_event_value(
        socket,
        str_value("message"),
        &[buffer.get_nanbox_f64(), rinfo.get_nanbox_f64()],
    );
}

/// Extract the raw bytes to transmit from a `send()` message argument
/// (string → UTF-8, Buffer, or TypedArray/DataView).
fn message_bytes(value: f64) -> Option<Vec<u8>> {
    if let Some(text) = string_to_rust(value) {
        return Some(text.into_bytes());
    }
    let raw = raw_ptr_from_value(value);
    if raw >= 0x10000 && crate::buffer::is_registered_buffer(raw) {
        let buf = raw as *const crate::buffer::BufferHeader;
        unsafe {
            let len = (*buf).length as usize;
            let data = (raw as *const u8).add(std::mem::size_of::<crate::buffer::BufferHeader>());
            return Some(std::slice::from_raw_parts(data, len).to_vec());
        }
    }
    if raw >= 0x10000 && crate::typedarray::lookup_typed_array_kind(raw).is_some() {
        return unsafe {
            crate::typedarray::typed_array_bytes(raw as *const crate::typedarray::TypedArrayHeader)
                .map(<[u8]>::to_vec)
        };
    }
    None
}

/// Map a `std::io::ErrorKind` from a socket syscall onto the Node error code.
fn io_error_code(err: &std::io::Error) -> &'static str {
    match err.kind() {
        std::io::ErrorKind::AddrInUse => "EADDRINUSE",
        std::io::ErrorKind::AddrNotAvailable => "EADDRNOTAVAIL",
        std::io::ErrorKind::PermissionDenied => "EACCES",
        std::io::ErrorKind::ConnectionRefused => "ECONNREFUSED",
        _ => "EINVAL",
    }
}

/// Build (not throw) a Node-style socket error value with `code`/`syscall`.
fn socket_error_value(message: &str, code: &'static str, syscall: &'static str) -> f64 {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    crate::node_submodules::register_error_code_pub(msg, code);
    crate::node_submodules::register_error_syscall(msg, syscall);
    let err = crate::error::js_error_new_with_message(msg);
    boxed_pointer(err as *const u8)
}

fn dns_not_found_value(host: &str) -> f64 {
    socket_error_value(
        &format!("getaddrinfo ENOTFOUND {host}"),
        "ENOTFOUND",
        "getaddrinfo",
    )
}

/// Resolve a `send()` destination to a concrete `SocketAddr`. IP literals are
/// used verbatim; hostnames go through `getaddrinfo`.
fn resolve_send_addr(address: &str, port: u16) -> Result<SocketAddr, f64> {
    if let Ok(ip) = address.parse::<IpAddr>() {
        return Ok(SocketAddr::new(ip, port));
    }
    match (address, port).to_socket_addrs() {
        Ok(mut iter) => iter.next().ok_or_else(|| dns_not_found_value(address)),
        Err(_) => Err(dns_not_found_value(address)),
    }
}

/// Real bind: open + bind an OS `UdpSocket`, register it with the reactor (which
/// starts the recv thread), and record the actual local address. On failure
/// returns the error value for the caller to emit as `'error'`.
fn real_bind(socket: f64, port: u16, address: &str) -> Result<(), f64> {
    let address = normalize_address(address, socket);
    let udp = match UdpSocket::bind((address.as_str(), port)) {
        Ok(udp) => udp,
        Err(err) => {
            return Err(socket_error_value(
                &format!("bind {} {address}:{port}", io_error_code(&err)),
                io_error_code(&err),
                "bind",
            ));
        }
    };
    let (actual_address, actual_port, family) = match udp.local_addr() {
        Ok(sa) => (
            sa.ip().to_string(),
            sa.port(),
            if sa.is_ipv4() { "IPv4" } else { "IPv6" },
        ),
        Err(_) => (address.clone(), port, family_for_address(&address, socket)),
    };
    let id = crate::dgram_reactor::register(socket.to_bits(), Arc::new(udp));
    set_hidden_value(socket, KEY_REACTOR_ID, id as f64);
    set_hidden_value(socket, KEY_ADDRESS, str_value(&actual_address));
    set_hidden_value(socket, KEY_FAMILY, str_value(family));
    set_hidden_value(socket, KEY_PORT, actual_port as f64);
    set_hidden_value(socket, KEY_BOUND, bool_value(true));
    Ok(())
}

/// Real `send()`: transmit over the OS socket. Errors go to the callback when
/// one is supplied, otherwise to an `'error'` event (Node semantics).
fn real_send(socket: f64, args: &[f64]) -> f64 {
    let msg = args.first().copied().unwrap_or_else(undefined_value);
    let Some(bytes) = message_bytes(msg) else {
        throw_invalid_message(msg);
    };
    let (port, address) = send_destination(socket, args);
    if let Some(err) = ensure_bound_real(socket) {
        return finish_send(socket, args, Err(err));
    }
    let outcome = match (live_udp(socket), resolve_send_addr(&address, port)) {
        (Some(udp), Ok(dest)) => match udp.send_to(&bytes, dest) {
            Ok(_) => Ok(bytes.len()),
            Err(err) => Err(socket_error_value(
                &format!("send {}", io_error_code(&err)),
                io_error_code(&err),
                "send",
            )),
        },
        (_, Err(err)) => Err(err),
        (None, _) => Err(socket_error_value("send EBADF", "EBADF", "send")),
    };
    finish_send(socket, args, outcome)
}

fn finish_send(socket: f64, args: &[f64], outcome: Result<usize, f64>) -> f64 {
    match (outcome, callback_from_args(args)) {
        (Ok(size), Some(callback)) => {
            call_function(callback, socket, &[null_value(), size as f64]);
        }
        (Ok(_), None) => {}
        (Err(error), Some(callback)) => {
            call_function(callback, socket, &[error]);
        }
        (Err(error), None) => {
            emit_event(socket, "error", &[error]);
        }
    }
    undefined_value()
}

/// Implicit bind on first `send`/`connect` (real mode). Returns an error value
/// if the bind failed.
fn ensure_bound_real(socket: f64) -> Option<f64> {
    if is_truthy_hidden(socket, KEY_BOUND) {
        return None;
    }
    real_bind(socket, 0, &default_bind_address(socket)).err()
}

/// Borrow the live `UdpSocket` and run `f`; no-op when the socket is not bound
/// to a real OS socket (e.g. closed).
fn with_udp<F: FnOnce(&UdpSocket)>(socket: f64, f: F) {
    if let Some(udp) = live_udp(socket) {
        f(&udp);
    }
}

fn parse_multicast_v4(addr: &str) -> Option<Ipv4Addr> {
    addr.parse::<Ipv4Addr>().ok()
}

fn parse_multicast_v6(addr: &str) -> Option<Ipv6Addr> {
    addr.parse::<Ipv6Addr>().ok()
}

/// `socket.ref()` / `socket.unref()` — toggle whether the bound socket keeps
/// the event loop alive. No-op in deterministic mode (no real socket).
fn ref_impl(socket: f64, refed: bool) -> f64 {
    if !deterministic() {
        if let Some(id) = reactor_id(socket) {
            crate::dgram_reactor::set_refed(id, refed);
        }
    }
    socket
}

fn create_socket_impl(args: &[f64]) -> f64 {
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

fn bind_impl(socket: f64, args: &[f64]) -> f64 {
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

fn address_impl(socket: f64) -> f64 {
    if !is_truthy_hidden(socket, KEY_BOUND) {
        throw_not_bound();
    }
    let address =
        hidden_string(socket, KEY_ADDRESS).unwrap_or_else(|| default_bind_address(socket));
    let family = hidden_string(socket, KEY_FAMILY)
        .unwrap_or_else(|| family_for_address(&address, socket).to_string());
    build_address_info(&address, &family, hidden_port(socket, KEY_PORT))
}

fn close_impl(socket: f64, args: &[f64]) -> f64 {
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

fn connect_impl(socket: f64, args: &[f64]) -> f64 {
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

fn disconnect_impl(socket: f64) -> f64 {
    if !is_truthy_hidden(socket, KEY_CONNECTED) {
        throw_not_connected();
    }
    set_hidden_value(socket, KEY_CONNECTED, bool_value(false));
    set_hidden_value(socket, KEY_REMOTE_ADDRESS, undefined_value());
    set_hidden_value(socket, KEY_REMOTE_FAMILY, undefined_value());
    set_hidden_value(socket, KEY_REMOTE_PORT, 0.0);
    undefined_value()
}

fn remote_address_impl(socket: f64) -> f64 {
    if !is_truthy_hidden(socket, KEY_CONNECTED) {
        throw_not_connected();
    }
    let address = hidden_string(socket, KEY_REMOTE_ADDRESS)
        .unwrap_or_else(|| default_loopback_address(socket));
    let family = hidden_string(socket, KEY_REMOTE_FAMILY)
        .unwrap_or_else(|| family_for_address(&address, socket).to_string());
    build_address_info(&address, &family, hidden_port(socket, KEY_REMOTE_PORT))
}

fn send_destination(socket: f64, args: &[f64]) -> (u16, String) {
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

fn send_impl(socket: f64, args: &[f64]) -> f64 {
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

fn membership_impl(socket: f64, args: &[f64], syscall: &'static str) -> f64 {
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

fn source_membership_impl(socket: f64, args: &[f64], syscall: &'static str) -> f64 {
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

fn set_broadcast_impl(socket: f64, args: &[f64]) -> f64 {
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

fn set_ttl_impl(socket: f64, args: &[f64]) -> f64 {
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

fn set_multicast_ttl_impl(socket: f64, args: &[f64]) -> f64 {
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

fn set_multicast_loopback_impl(socket: f64, args: &[f64]) -> f64 {
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

fn set_multicast_interface_impl(socket: f64, args: &[f64]) -> f64 {
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

fn validate_buffer_size(value: f64) -> f64 {
    let Some(size) = number_value(value) else {
        throw_bad_buffer_size();
    };
    if !size.is_finite() || size < 0.0 || size.fract() != 0.0 {
        throw_bad_buffer_size();
    }
    size
}

fn set_buffer_size_impl(socket: f64, args: &[f64], key: &[u8], syscall: &'static str) -> f64 {
    let size = validate_buffer_size(args.first().copied().unwrap_or_else(undefined_value));
    ensure_buffer_running(socket, syscall);
    set_hidden_value(socket, key, size.max(1.0));
    undefined_value()
}

fn get_buffer_size_impl(socket: f64, key: &[u8], syscall: &'static str) -> f64 {
    ensure_buffer_running(socket, syscall);
    get_hidden_value(socket, key).unwrap_or(65536.0)
}

extern "C" fn dgram_send_thunk(closure: *const ClosureHeader, rest: f64) -> f64 {
    send_impl(this_value(closure), &collect_rest_args(rest))
}

extern "C" fn dgram_bind_thunk(closure: *const ClosureHeader, rest: f64) -> f64 {
    bind_impl(this_value(closure), &collect_rest_args(rest))
}

extern "C" fn dgram_close_thunk(closure: *const ClosureHeader, rest: f64) -> f64 {
    close_impl(this_value(closure), &collect_rest_args(rest))
}

extern "C" fn dgram_address_thunk(closure: *const ClosureHeader, _rest: f64) -> f64 {
    address_impl(this_value(closure))
}

extern "C" fn dgram_remote_address_thunk(closure: *const ClosureHeader, _rest: f64) -> f64 {
    remote_address_impl(this_value(closure))
}

extern "C" fn dgram_connect_thunk(closure: *const ClosureHeader, rest: f64) -> f64 {
    connect_impl(this_value(closure), &collect_rest_args(rest))
}

extern "C" fn dgram_disconnect_thunk(closure: *const ClosureHeader, _rest: f64) -> f64 {
    disconnect_impl(this_value(closure))
}

extern "C" fn dgram_on_thunk(closure: *const ClosureHeader, rest: f64) -> f64 {
    let socket = this_value(closure);
    let args = collect_rest_args(rest);
    let event = args.first().copied().unwrap_or_else(undefined_value);
    let listener = args.get(1).copied().unwrap_or_else(undefined_value);
    add_listener(socket, event, listener, false);
    socket
}

extern "C" fn dgram_once_thunk(closure: *const ClosureHeader, rest: f64) -> f64 {
    let socket = this_value(closure);
    let args = collect_rest_args(rest);
    let event = args.first().copied().unwrap_or_else(undefined_value);
    let listener = args.get(1).copied().unwrap_or_else(undefined_value);
    add_listener(socket, event, listener, true);
    socket
}

extern "C" fn dgram_remove_listener_thunk(closure: *const ClosureHeader, rest: f64) -> f64 {
    let socket = this_value(closure);
    let args = collect_rest_args(rest);
    if args.len() >= 2 {
        remove_listener(socket, args[0], args[1]);
    }
    socket
}

extern "C" fn dgram_emit_thunk(closure: *const ClosureHeader, rest: f64) -> f64 {
    let socket = this_value(closure);
    let args = collect_rest_args(rest);
    let event = args.first().copied().unwrap_or_else(undefined_value);
    let emitted = emit_event_value(socket, event, args.get(1..).unwrap_or(&[]));
    bool_value(emitted)
}

extern "C" fn dgram_listener_count_thunk(closure: *const ClosureHeader, rest: f64) -> f64 {
    let args = collect_rest_args(rest);
    let event = args.first().copied().unwrap_or_else(undefined_value);
    listener_snapshot(this_value(closure), event).len() as f64
}

extern "C" fn dgram_event_names_thunk(closure: *const ClosureHeader, _rest: f64) -> f64 {
    event_names_impl(this_value(closure))
}

extern "C" fn dgram_add_membership_thunk(closure: *const ClosureHeader, rest: f64) -> f64 {
    membership_impl(
        this_value(closure),
        &collect_rest_args(rest),
        "addMembership",
    )
}

extern "C" fn dgram_drop_membership_thunk(closure: *const ClosureHeader, rest: f64) -> f64 {
    membership_impl(
        this_value(closure),
        &collect_rest_args(rest),
        "dropMembership",
    )
}

extern "C" fn dgram_add_source_membership_thunk(closure: *const ClosureHeader, rest: f64) -> f64 {
    source_membership_impl(
        this_value(closure),
        &collect_rest_args(rest),
        "addSourceSpecificMembership",
    )
}

extern "C" fn dgram_drop_source_membership_thunk(closure: *const ClosureHeader, rest: f64) -> f64 {
    source_membership_impl(
        this_value(closure),
        &collect_rest_args(rest),
        "dropSourceSpecificMembership",
    )
}

extern "C" fn dgram_set_broadcast_thunk(closure: *const ClosureHeader, rest: f64) -> f64 {
    set_broadcast_impl(this_value(closure), &collect_rest_args(rest))
}

extern "C" fn dgram_set_multicast_ttl_thunk(closure: *const ClosureHeader, rest: f64) -> f64 {
    set_multicast_ttl_impl(this_value(closure), &collect_rest_args(rest))
}

extern "C" fn dgram_set_multicast_loopback_thunk(closure: *const ClosureHeader, rest: f64) -> f64 {
    set_multicast_loopback_impl(this_value(closure), &collect_rest_args(rest))
}

extern "C" fn dgram_set_multicast_interface_thunk(closure: *const ClosureHeader, rest: f64) -> f64 {
    set_multicast_interface_impl(this_value(closure), &collect_rest_args(rest))
}

extern "C" fn dgram_set_ttl_thunk(closure: *const ClosureHeader, rest: f64) -> f64 {
    set_ttl_impl(this_value(closure), &collect_rest_args(rest))
}

extern "C" fn dgram_set_recv_buffer_size_thunk(closure: *const ClosureHeader, rest: f64) -> f64 {
    set_buffer_size_impl(
        this_value(closure),
        &collect_rest_args(rest),
        KEY_RECV_BUFFER_SIZE,
        "uv_recv_buffer_size",
    )
}

extern "C" fn dgram_set_send_buffer_size_thunk(closure: *const ClosureHeader, rest: f64) -> f64 {
    set_buffer_size_impl(
        this_value(closure),
        &collect_rest_args(rest),
        KEY_SEND_BUFFER_SIZE,
        "uv_send_buffer_size",
    )
}

extern "C" fn dgram_get_recv_buffer_size_thunk(closure: *const ClosureHeader, _rest: f64) -> f64 {
    get_buffer_size_impl(
        this_value(closure),
        KEY_RECV_BUFFER_SIZE,
        "uv_recv_buffer_size",
    )
}

extern "C" fn dgram_get_send_buffer_size_thunk(closure: *const ClosureHeader, _rest: f64) -> f64 {
    get_buffer_size_impl(
        this_value(closure),
        KEY_SEND_BUFFER_SIZE,
        "uv_send_buffer_size",
    )
}

extern "C" fn dgram_ref_thunk(closure: *const ClosureHeader, _rest: f64) -> f64 {
    ref_impl(this_value(closure), true)
}

extern "C" fn dgram_unref_thunk(closure: *const ClosureHeader, _rest: f64) -> f64 {
    ref_impl(this_value(closure), false)
}

extern "C" fn dgram_zero_thunk(_closure: *const ClosureHeader, _rest: f64) -> f64 {
    0.0
}

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
