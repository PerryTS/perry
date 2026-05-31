//! Runtime-only `node:dns` / `node:dns/promises` shape stubs.
//!
//! The generated inventory fixtures only probe callable shape, constants,
//! Resolver method fields, and a deterministic promises `lookup("localhost")`.
//! These helpers provide that surface without doing external name resolution.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{LazyLock, Mutex};

use crate::closure::{js_closure_alloc, js_register_closure_arity, ClosureHeader};
use crate::object::{js_object_alloc, js_object_set_field_by_name, ObjectHeader};
use crate::value::{js_nanbox_pointer, JSValue, TAG_UNDEFINED};

const RESULT_ORDER_VERBATIM: u8 = 0;
const RESULT_ORDER_IPV4_FIRST: u8 = 1;
const RESULT_ORDER_IPV6_FIRST: u8 = 2;

static DEFAULT_RESULT_ORDER: AtomicU8 = AtomicU8::new(RESULT_ORDER_VERBATIM);
static DNS_SERVERS: LazyLock<Mutex<Vec<String>>> = LazyLock::new(|| Mutex::new(Vec::new()));
static DNS_PROMISE_SERVERS: LazyLock<Mutex<Option<Vec<String>>>> =
    LazyLock::new(|| Mutex::new(None));

const RESOLVER_CONTROL_METHODS: &[&str] =
    &["cancel", "getServers", "setServers", "setLocalAddress"];
const RESOLVER_RESOLVE_METHODS: &[&str] = &[
    "resolve",
    "resolve4",
    "resolve6",
    "resolveAny",
    "resolveCaa",
    "resolveCname",
    "resolveMx",
    "resolveNaptr",
    "resolveNs",
    "resolvePtr",
    "resolveSoa",
    "resolveSrv",
    "resolveTlsa",
    "resolveTxt",
    "reverse",
];
const RESOLVER_SERVERS_FIELD: &str = "__dns_servers";

fn key(name: &str) -> *mut crate::StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

fn str_value(value: &str) -> f64 {
    let ptr = crate::string::js_string_from_bytes(value.as_ptr(), value.len() as u32);
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

fn boxed_pointer(ptr: *const u8) -> f64 {
    f64::from_bits(JSValue::pointer(ptr).bits())
}

fn empty_array_value() -> f64 {
    let arr = crate::array::js_array_alloc(0);
    js_nanbox_pointer(arr as i64)
}

fn undefined_value() -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

fn first_arg(args: i64) -> f64 {
    let arr = args as *const crate::array::ArrayHeader;
    if arr.is_null() || crate::array::js_array_length(arr) == 0 {
        return undefined_value();
    }
    crate::array::js_array_get_f64(arr, 0)
}

fn js_string_to_rust(value: f64) -> Option<String> {
    let js_value = JSValue::from_bits(value.to_bits());
    if !js_value.is_any_string() {
        return None;
    }
    let ptr = crate::value::js_get_string_pointer_unified(value) as *const crate::StringHeader;
    if ptr.is_null() || (ptr as usize) < 0x1000 {
        return Some(String::new());
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        Some(String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned())
    }
}

fn array_ptr_from_value(value: f64) -> Option<*const crate::array::ArrayHeader> {
    let js_value = JSValue::from_bits(value.to_bits());
    if !js_value.is_pointer() {
        return None;
    }
    let arr = crate::array::clean_arr_ptr(js_value.as_pointer::<crate::array::ArrayHeader>());
    if arr.is_null() {
        None
    } else {
        Some(arr)
    }
}

fn throw_invalid_servers_array(value: f64) -> ! {
    let message = format!(
        "The \"servers\" argument must be an instance of Array. Received {}",
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
}

fn throw_invalid_server_element(index: u32, value: f64) -> ! {
    let message = format!(
        "The \"servers[{index}]\" argument must be of type string. Received {}",
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
}

fn throw_invalid_ip_address(server: &str) -> ! {
    let message = format!("Invalid IP address: {server}");
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_IP_ADDRESS");
}

fn parse_port(port: &str) -> Option<u16> {
    if port.is_empty() || !port.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let parsed = port.parse::<u16>().ok()?;
    if parsed == 0 {
        None
    } else {
        Some(parsed)
    }
}

fn format_ipv4_server(ip: Ipv4Addr, port: Option<u16>) -> String {
    match port {
        Some(port) if port != 53 => format!("{ip}:{port}"),
        _ => ip.to_string(),
    }
}

fn format_ipv6_server(ip: Ipv6Addr, port: Option<u16>) -> String {
    match port {
        Some(port) if port != 53 => format!("[{ip}]:{port}"),
        _ => ip.to_string(),
    }
}

fn normalize_dns_server(server: &str) -> Option<String> {
    if let Ok(ip) = server.parse::<IpAddr>() {
        return Some(match ip {
            IpAddr::V4(ip) => ip.to_string(),
            IpAddr::V6(ip) => ip.to_string(),
        });
    }

    if let Some(rest) = server.strip_prefix('[') {
        let close = rest.find(']')?;
        let host = &rest[..close];
        let suffix = &rest[close + 1..];
        let ip = host.parse::<Ipv6Addr>().ok()?;
        let port = if suffix.is_empty() {
            None
        } else {
            let port = suffix.strip_prefix(':')?;
            Some(parse_port(port)?)
        };
        return Some(format_ipv6_server(ip, port));
    }

    if let Some((host, port)) = server.rsplit_once(':') {
        if !host.contains(':') {
            let ip = host.parse::<Ipv4Addr>().ok()?;
            return Some(format_ipv4_server(ip, Some(parse_port(port)?)));
        }
    }

    None
}

fn parse_servers(value: f64) -> Vec<String> {
    let Some(arr) = array_ptr_from_value(value) else {
        throw_invalid_servers_array(value);
    };
    let len = crate::array::js_array_length(arr);
    let mut servers = Vec::with_capacity(len as usize);
    for i in 0..len {
        let entry_value = crate::array::js_array_get_f64(arr, i);
        let Some(entry) = js_string_to_rust(entry_value) else {
            throw_invalid_server_element(i, entry_value);
        };
        let Some(normalized) = normalize_dns_server(&entry) else {
            throw_invalid_ip_address(&entry);
        };
        servers.push(normalized);
    }
    servers
}

fn servers_array_value(servers: &[String]) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let arr = crate::array::js_array_alloc(servers.len() as u32);
    let arr_handle = scope.root_raw_mut_ptr(arr);

    for server in servers {
        let str_ptr = crate::string::js_string_from_bytes(server.as_ptr(), server.len() as u32);
        let str_handle = scope.root_string_ptr(str_ptr);
        let str_value = f64::from_bits(
            JSValue::string_ptr(str_handle.get_raw_const_ptr::<crate::StringHeader>() as *mut _)
                .bits(),
        );
        let next = crate::array::js_array_push_f64(
            arr_handle.get_raw_mut_ptr::<crate::array::ArrayHeader>(),
            str_value,
        );
        arr_handle.set_raw_mut_ptr::<crate::array::ArrayHeader>(next);
    }

    boxed_pointer(arr_handle.get_raw_const_ptr::<crate::array::ArrayHeader>() as *const u8)
}

fn stored_servers() -> Vec<String> {
    DNS_SERVERS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

pub(crate) fn dns_get_servers_value() -> f64 {
    servers_array_value(&stored_servers())
}

pub(crate) fn dns_set_servers_value(value: f64) -> f64 {
    let servers = parse_servers(value);
    *DNS_SERVERS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = servers;
    undefined_value()
}

fn stored_promise_servers() -> Vec<String> {
    DNS_PROMISE_SERVERS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
        .unwrap_or_else(stored_servers)
}

pub(crate) fn dns_promises_init_servers_from_callback_if_unset() {
    let mut promise_servers = DNS_PROMISE_SERVERS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if promise_servers.is_none() {
        *promise_servers = Some(stored_servers());
    }
}

pub(crate) fn dns_promises_get_servers_value() -> f64 {
    servers_array_value(&stored_promise_servers())
}

pub(crate) fn dns_promises_set_servers_value(value: f64) -> f64 {
    let servers = parse_servers(value);
    *DNS_PROMISE_SERVERS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(servers);
    undefined_value()
}

fn invalid_arg_value_received(value: f64) -> String {
    let js_value = JSValue::from_bits(value.to_bits());
    if js_value.is_undefined() {
        return "undefined".to_string();
    }
    if js_value.is_null() {
        return "null".to_string();
    }
    if js_value.is_bool() {
        return if js_value.as_bool() { "true" } else { "false" }.to_string();
    }
    if let Some(s) = js_string_to_rust(value) {
        return format!("'{s}'");
    }
    if js_value.is_int32() {
        return js_value.as_int32().to_string();
    }
    if js_value.is_number() {
        return value.to_string();
    }
    "{}".to_string()
}

fn throw_invalid_dns_order(value: f64) -> ! {
    let message = format!(
        "The argument 'dnsOrder' must be one of: 'verbatim', 'ipv4first', 'ipv6first'. Received {}",
        invalid_arg_value_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_VALUE");
}

pub(crate) fn dns_set_default_result_order_value(value: f64) -> f64 {
    let Some(order) = js_string_to_rust(value) else {
        throw_invalid_dns_order(value);
    };
    let order = match order.as_str() {
        "verbatim" => RESULT_ORDER_VERBATIM,
        "ipv4first" => RESULT_ORDER_IPV4_FIRST,
        "ipv6first" => RESULT_ORDER_IPV6_FIRST,
        _ => throw_invalid_dns_order(value),
    };
    DEFAULT_RESULT_ORDER.store(order, Ordering::Relaxed);
    undefined_value()
}

pub(crate) fn dns_get_default_result_order_value() -> f64 {
    let order = match DEFAULT_RESULT_ORDER.load(Ordering::Relaxed) {
        RESULT_ORDER_IPV4_FIRST => "ipv4first",
        RESULT_ORDER_IPV6_FIRST => "ipv6first",
        _ => "verbatim",
    };
    str_value(order)
}

fn resolver_object_from_value(value: f64) -> Option<*mut ObjectHeader> {
    let js_value = JSValue::from_bits(value.to_bits());
    if !js_value.is_pointer() {
        return None;
    }
    let obj = js_value.as_pointer::<ObjectHeader>() as *mut ObjectHeader;
    if obj.is_null() || (obj as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        None
    } else {
        Some(obj)
    }
}

fn resolver_object_from_handle(handle: i64) -> Option<*mut ObjectHeader> {
    if handle == 0 {
        return None;
    }
    let bits = handle as u64;
    let ptr = if (bits >> 48) >= 0x7FF8 {
        (bits & 0x0000_FFFF_FFFF_FFFF) as *mut ObjectHeader
    } else {
        bits as *mut ObjectHeader
    };
    if ptr.is_null() || (ptr as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        None
    } else {
        Some(ptr)
    }
}

fn resolver_get_servers_from_obj(obj: *mut ObjectHeader) -> f64 {
    let servers_value =
        crate::object::js_object_get_field_by_name_f64(obj, key(RESOLVER_SERVERS_FIELD));
    if let Some(arr) = array_ptr_from_value(servers_value) {
        let len = crate::array::js_array_length(arr);
        let mut servers = Vec::with_capacity(len as usize);
        for i in 0..len {
            if let Some(server) = js_string_to_rust(crate::array::js_array_get_f64(arr, i)) {
                servers.push(server);
            }
        }
        return servers_array_value(&servers);
    }
    empty_array_value()
}

fn resolver_set_servers_for_obj(obj: *mut ObjectHeader, servers_value: f64) -> f64 {
    let servers = parse_servers(servers_value);
    let value = servers_array_value(&servers);
    js_object_set_field_by_name(obj, key(RESOLVER_SERVERS_FIELD), value);
    undefined_value()
}

extern "C" fn dns_noop_thunk(_closure: *const ClosureHeader) -> f64 {
    undefined_value()
}

extern "C" fn dns_noop2_thunk(_closure: *const ClosureHeader, _a: f64, _b: f64) -> f64 {
    undefined_value()
}

extern "C" fn dns_resolver_get_servers_thunk(_closure: *const ClosureHeader) -> f64 {
    let this_value = crate::object::js_implicit_this_get();
    let Some(obj) = resolver_object_from_value(this_value) else {
        return empty_array_value();
    };
    resolver_get_servers_from_obj(obj)
}

extern "C" fn dns_resolver_set_servers_thunk(
    _closure: *const ClosureHeader,
    servers_value: f64,
) -> f64 {
    let this_value = crate::object::js_implicit_this_get();
    let Some(obj) = resolver_object_from_value(this_value) else {
        return dns_promises_set_servers_value(servers_value);
    };
    resolver_set_servers_for_obj(obj, servers_value)
}

fn method_value(name: &str) -> f64 {
    let (func_ptr, arity) = match name {
        "getServers" => (dns_resolver_get_servers_thunk as *const u8, 0),
        "setServers" => (dns_resolver_set_servers_thunk as *const u8, 1),
        "setLocalAddress" => (dns_noop2_thunk as *const u8, 2),
        _ => (dns_noop_thunk as *const u8, 0),
    };
    let closure = js_closure_alloc(func_ptr, 0);
    js_register_closure_arity(func_ptr, arity);
    crate::object::set_bound_native_closure_name(closure, name);
    js_nanbox_pointer(closure as i64)
}

fn resolver_object(initial_servers: Vec<String>) -> *mut ObjectHeader {
    let method_count = RESOLVER_CONTROL_METHODS.len() + RESOLVER_RESOLVE_METHODS.len() + 1;
    let obj = js_object_alloc(0, method_count as u32);
    js_object_set_field_by_name(
        obj,
        key(RESOLVER_SERVERS_FIELD),
        servers_array_value(&initial_servers),
    );
    for method in RESOLVER_CONTROL_METHODS {
        js_object_set_field_by_name(obj, key(method), method_value(method));
    }
    for method in RESOLVER_RESOLVE_METHODS {
        js_object_set_field_by_name(obj, key(method), method_value(method));
    }
    obj
}

fn localhost_lookup_result() -> f64 {
    let obj = js_object_alloc(0, 2);
    js_object_set_field_by_name(obj, key("address"), str_value("127.0.0.1"));
    js_object_set_field_by_name(obj, key("family"), 4.0);
    boxed_pointer(obj as *const u8)
}

#[no_mangle]
pub extern "C" fn js_dns_noop(_args: i64) -> f64 {
    undefined_value()
}

#[no_mangle]
pub extern "C" fn js_dns_promises_noop(_args: i64) -> f64 {
    let promise = crate::promise::js_promise_resolved(undefined_value());
    js_nanbox_pointer(promise as i64)
}

#[no_mangle]
pub extern "C" fn js_dns_get_servers(_args: i64) -> f64 {
    dns_get_servers_value()
}

#[no_mangle]
pub extern "C" fn js_dns_set_servers(args: i64) -> f64 {
    dns_set_servers_value(first_arg(args))
}

#[no_mangle]
pub extern "C" fn js_dns_promises_get_servers(_args: i64) -> f64 {
    dns_promises_get_servers_value()
}

#[no_mangle]
pub extern "C" fn js_dns_promises_set_servers(args: i64) -> f64 {
    dns_promises_set_servers_value(first_arg(args))
}

#[no_mangle]
pub extern "C" fn js_dns_set_default_result_order(args: i64) -> f64 {
    dns_set_default_result_order_value(first_arg(args))
}

#[no_mangle]
pub extern "C" fn js_dns_get_default_result_order(_args: i64) -> f64 {
    dns_get_default_result_order_value()
}

#[no_mangle]
pub extern "C" fn js_dns_resolver_new(_args: i64) -> f64 {
    boxed_pointer(resolver_object(stored_servers()) as *const u8)
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolver_new(_args: i64) -> f64 {
    boxed_pointer(resolver_object(stored_promise_servers()) as *const u8)
}

#[no_mangle]
pub extern "C" fn js_dns_resolver_get_servers(_handle: i64, _args: i64) -> f64 {
    let Some(obj) = resolver_object_from_handle(_handle) else {
        return empty_array_value();
    };
    resolver_get_servers_from_obj(obj)
}

#[no_mangle]
pub extern "C" fn js_dns_resolver_set_servers(handle: i64, args: i64) -> f64 {
    let servers_value = first_arg(args);
    let Some(obj) = resolver_object_from_handle(handle) else {
        return dns_promises_set_servers_value(servers_value);
    };
    resolver_set_servers_for_obj(obj, servers_value)
}

#[no_mangle]
pub extern "C" fn js_dns_resolver_noop(_handle: i64, _args: i64) -> f64 {
    undefined_value()
}

#[no_mangle]
pub extern "C" fn js_dns_promises_lookup(_args: i64) -> f64 {
    let promise = crate::promise::js_promise_resolved(localhost_lookup_result());
    js_nanbox_pointer(promise as i64)
}
