//! Runtime-only `node:dns` / `node:dns/promises` support.
//!
//! These helpers intentionally avoid external name resolution. They provide the
//! Node-compatible surface Perry's fixtures need using deterministic loopback
//! answers plus empty record sets for names we do not control.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{LazyLock, Mutex};

use crate::closure::{js_closure_alloc, js_register_closure_arity, ClosureHeader};
use crate::object::{js_object_alloc, js_object_set_field_by_name, ObjectHeader};
use crate::value::{js_nanbox_pointer, JSValue, TAG_NULL, TAG_UNDEFINED};

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

#[derive(Clone, Copy)]
enum RecordKind {
    A,
    Aaaa,
    Any,
    Caa,
    Cname,
    Mx,
    Naptr,
    Ns,
    Ptr,
    Soa,
    Srv,
    Tlsa,
    Txt,
}

#[derive(Clone)]
struct ResolvedAddress {
    address: String,
    family: i32,
}

#[derive(Clone, Copy)]
struct LookupOptions {
    family: i32,
    all: bool,
}

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

fn null_value() -> f64 {
    f64::from_bits(TAG_NULL)
}

fn first_arg(args: i64) -> f64 {
    let arr = args as *const crate::array::ArrayHeader;
    if arr.is_null() || crate::array::js_array_length(arr) == 0 {
        return undefined_value();
    }
    crate::array::js_array_get_f64(arr, 0)
}

fn args_len(args: i64) -> u32 {
    let arr = args as *const crate::array::ArrayHeader;
    if arr.is_null() {
        0
    } else {
        crate::array::js_array_length(arr)
    }
}

fn arg(args: i64, index: u32) -> f64 {
    let arr = args as *const crate::array::ArrayHeader;
    if arr.is_null() || index >= crate::array::js_array_length(arr) {
        undefined_value()
    } else {
        crate::array::js_array_get_f64(arr, index)
    }
}

fn array_value_from_values(values: &[f64]) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let arr = crate::array::js_array_alloc(values.len() as u32);
    let arr_handle = scope.root_raw_mut_ptr(arr);
    for value in values {
        let next = crate::array::js_array_push_f64(
            arr_handle.get_raw_mut_ptr::<crate::array::ArrayHeader>(),
            *value,
        );
        arr_handle.set_raw_mut_ptr::<crate::array::ArrayHeader>(next);
    }
    boxed_pointer(arr_handle.get_raw_const_ptr::<crate::array::ArrayHeader>() as *const u8)
}

fn string_array_value(values: &[&str]) -> f64 {
    let values: Vec<f64> = values.iter().map(|value| str_value(value)).collect();
    array_value_from_values(&values)
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

fn numeric_value(value: f64) -> Option<f64> {
    let js_value = JSValue::from_bits(value.to_bits());
    if js_value.is_int32() {
        Some(js_value.as_int32() as f64)
    } else if js_value.is_number() {
        Some(js_value.as_number())
    } else {
        None
    }
}

fn closure_ptr_from_value(value: f64) -> Option<*const ClosureHeader> {
    let js_value = JSValue::from_bits(value.to_bits());
    if !js_value.is_pointer() {
        return None;
    }
    let ptr = js_value.as_pointer::<u8>() as usize;
    crate::closure::is_closure_ptr(ptr).then_some(ptr as *const ClosureHeader)
}

fn is_callable_value(value: f64) -> bool {
    closure_ptr_from_value(value).is_some()
}

fn value_gc_type(value: f64) -> Option<u8> {
    let js_value = JSValue::from_bits(value.to_bits());
    if !js_value.is_pointer() {
        return None;
    }
    let ptr = js_value.as_pointer::<u8>();
    if ptr.is_null() || (ptr as usize) < 0x10000 || ((ptr as u64) >> 48) != 0 {
        return None;
    }
    unsafe {
        let gc_header = ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        Some((*gc_header).obj_type)
    }
}

fn is_plain_object_value(value: f64) -> bool {
    value_gc_type(value) == Some(crate::gc::GC_TYPE_OBJECT)
}

fn option_field(options: f64, name: &str) -> f64 {
    if !is_plain_object_value(options) {
        return undefined_value();
    }
    let obj = JSValue::from_bits(options.to_bits()).as_pointer::<ObjectHeader>();
    crate::object::js_object_get_field_by_name_f64(obj, key(name))
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

fn type_error_value(message: &str, code: &'static str) -> f64 {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    crate::node_submodules::register_error_code_pub(msg, code);
    let err = crate::error::js_typeerror_new(msg);
    boxed_pointer(err as *const u8)
}

fn range_error_value(message: &str, code: &'static str) -> f64 {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    crate::node_submodules::register_error_code_pub(msg, code);
    let err = crate::error::js_rangeerror_new(msg);
    boxed_pointer(err as *const u8)
}

fn plain_error_value(message: &str, code: &'static str) -> f64 {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    crate::node_submodules::register_error_code_pub(msg, code);
    let err = crate::error::js_error_new_with_message(msg);
    boxed_pointer(err as *const u8)
}

fn throw_error_value(value: f64) -> ! {
    crate::exception::js_throw(value)
}

fn invalid_callback_error(value: f64) -> f64 {
    let message = format!(
        "The \"callback\" argument must be of type function. Received {}",
        crate::fs::validate::describe_received(value)
    );
    type_error_value(&message, "ERR_INVALID_ARG_TYPE")
}

fn invalid_hostname_error(value: f64) -> f64 {
    let message = format!(
        "The \"hostname\" argument must be of type string. Received {}",
        crate::fs::validate::describe_received(value)
    );
    type_error_value(&message, "ERR_INVALID_ARG_TYPE")
}

fn invalid_address_error(value: f64) -> f64 {
    let message = format!(
        "The argument 'address' is invalid. Received {}",
        invalid_arg_value_received(value)
    );
    type_error_value(&message, "ERR_INVALID_ARG_VALUE")
}

fn invalid_family_error(value: f64) -> f64 {
    let message = format!(
        "The property 'options.family' must be one of: 0, 4, 6. Received {}",
        invalid_arg_value_received(value)
    );
    type_error_value(&message, "ERR_INVALID_ARG_VALUE")
}

fn lookup_service_missing_args_error() -> f64 {
    type_error_value(
        "The \"address\", \"port\", and \"callback\" arguments must be specified",
        "ERR_MISSING_ARGS",
    )
}

fn bad_port_error(value: f64) -> f64 {
    let message = format!(
        "Port should be >= 0 and < 65536. Received {}.",
        crate::fs::validate::describe_received(value)
    );
    range_error_value(&message, "ERR_SOCKET_BAD_PORT")
}

fn dns_not_found_error(hostname: &str) -> f64 {
    plain_error_value(&format!("getaddrinfo ENOTFOUND {hostname}"), "ENOTFOUND")
}

fn throw_invalid_dns_order(value: f64) -> ! {
    let message = format!(
        "The argument 'dnsOrder' must be one of: 'verbatim', 'ipv4first', 'ipv6first'. Received {}",
        invalid_arg_value_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_VALUE");
}

fn invalid_name_error(value: f64) -> f64 {
    let message = format!(
        "The \"name\" argument must be of type string. Received {}",
        crate::fs::validate::describe_received(value)
    );
    type_error_value(&message, "ERR_INVALID_ARG_TYPE")
}

fn throw_invalid_name(value: f64) -> ! {
    throw_error_value(invalid_name_error(value));
}

fn throw_invalid_callback(value: f64) -> ! {
    let message = format!(
        "The \"callback\" argument must be of type function. Received {}",
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
}

fn invalid_rrtype_type_error(value: f64) -> f64 {
    let message = format!(
        "The \"rrtype\" argument must be of type string. Received {}",
        crate::fs::validate::describe_received(value)
    );
    type_error_value(&message, "ERR_INVALID_ARG_TYPE")
}

fn invalid_rrtype_value_error(value: f64) -> f64 {
    let message = format!(
        "The argument 'rrtype' is invalid. Received {}",
        invalid_arg_value_received(value)
    );
    type_error_value(&message, "ERR_INVALID_ARG_VALUE")
}

fn parse_record_kind_result(value: f64) -> Result<RecordKind, f64> {
    let Some(rrtype) = js_string_to_rust(value) else {
        return Err(invalid_rrtype_type_error(value));
    };
    match rrtype.as_str() {
        "A" => Ok(RecordKind::A),
        "AAAA" => Ok(RecordKind::Aaaa),
        "ANY" => Ok(RecordKind::Any),
        "CAA" => Ok(RecordKind::Caa),
        "CNAME" => Ok(RecordKind::Cname),
        "MX" => Ok(RecordKind::Mx),
        "NAPTR" => Ok(RecordKind::Naptr),
        "NS" => Ok(RecordKind::Ns),
        "PTR" => Ok(RecordKind::Ptr),
        "SOA" => Ok(RecordKind::Soa),
        "SRV" => Ok(RecordKind::Srv),
        "TLSA" => Ok(RecordKind::Tlsa),
        "TXT" => Ok(RecordKind::Txt),
        _ => Err(invalid_rrtype_value_error(value)),
    }
}

fn parse_record_kind(value: f64) -> RecordKind {
    parse_record_kind_result(value).unwrap_or_else(|error| throw_error_value(error))
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

fn object_value(fields: &[(&str, f64)]) -> f64 {
    let obj = js_object_alloc(0, fields.len() as u32);
    for (name, value) in fields {
        js_object_set_field_by_name(obj, key(name), *value);
    }
    boxed_pointer(obj as *const u8)
}

fn mx_record(exchange: &str, priority: f64) -> f64 {
    object_value(&[("exchange", str_value(exchange)), ("priority", priority)])
}

fn any_address_record(address: &str, record_type: &str) -> f64 {
    object_value(&[
        ("address", str_value(address)),
        ("ttl", 0.0),
        ("type", str_value(record_type)),
    ])
}

fn naptr_record() -> f64 {
    object_value(&[
        ("flags", str_value("")),
        ("service", str_value("")),
        ("regexp", str_value("")),
        ("replacement", str_value("localhost")),
        ("order", 0.0),
        ("preference", 0.0),
    ])
}

fn soa_record() -> f64 {
    object_value(&[
        ("nsname", str_value("localhost")),
        ("hostmaster", str_value("root.localhost")),
        ("serial", 1.0),
        ("refresh", 0.0),
        ("retry", 0.0),
        ("expire", 0.0),
        ("minttl", 0.0),
    ])
}

fn srv_record() -> f64 {
    object_value(&[
        ("name", str_value("localhost")),
        ("port", 0.0),
        ("priority", 0.0),
        ("weight", 0.0),
    ])
}

fn tlsa_record() -> f64 {
    object_value(&[
        ("usage", 0.0),
        ("selector", 0.0),
        ("matchingType", 0.0),
        ("certificate", str_value("")),
    ])
}

fn localhost_name(name: &str) -> bool {
    name.eq_ignore_ascii_case("localhost") || name.eq_ignore_ascii_case("localhost.")
}

fn resolve_records(kind: RecordKind, name: &str) -> f64 {
    if !localhost_name(name) {
        return empty_array_value();
    }

    match kind {
        RecordKind::A => string_array_value(&["127.0.0.1"]),
        RecordKind::Aaaa => string_array_value(&["::1"]),
        RecordKind::Any => array_value_from_values(&[
            any_address_record("127.0.0.1", "A"),
            any_address_record("::1", "AAAA"),
        ]),
        RecordKind::Caa => empty_array_value(),
        RecordKind::Cname => string_array_value(&["localhost"]),
        RecordKind::Mx => array_value_from_values(&[mx_record("localhost", 0.0)]),
        RecordKind::Naptr => array_value_from_values(&[naptr_record()]),
        RecordKind::Ns => string_array_value(&["localhost"]),
        RecordKind::Ptr => string_array_value(&["localhost"]),
        RecordKind::Soa => soa_record(),
        RecordKind::Srv => array_value_from_values(&[srv_record()]),
        RecordKind::Tlsa => array_value_from_values(&[tlsa_record()]),
        RecordKind::Txt => array_value_from_values(&[string_array_value(&["localhost"])]),
    }
}

fn reverse_records(name: &str) -> f64 {
    match name.parse::<IpAddr>() {
        Ok(IpAddr::V4(ip)) if ip.is_loopback() => string_array_value(&["localhost"]),
        Ok(IpAddr::V6(ip)) if ip.is_loopback() => string_array_value(&["localhost"]),
        _ => empty_array_value(),
    }
}

fn validate_family(value: f64) -> Result<i32, f64> {
    let Some(n) = numeric_value(value) else {
        return Err(invalid_family_error(value));
    };
    if !n.is_finite() || n.fract() != 0.0 {
        return Err(invalid_family_error(value));
    }
    let family = n as i32;
    if matches!(family, 0 | 4 | 6) {
        Ok(family)
    } else {
        Err(invalid_family_error(value))
    }
}

fn ipv4_loopback_address() -> ResolvedAddress {
    ResolvedAddress {
        address: "127.0.0.1".to_string(),
        family: 4,
    }
}

fn ipv6_loopback_address() -> ResolvedAddress {
    ResolvedAddress {
        address: "::1".to_string(),
        family: 6,
    }
}

fn default_loopback_addresses() -> Vec<ResolvedAddress> {
    match DEFAULT_RESULT_ORDER.load(Ordering::Relaxed) {
        RESULT_ORDER_IPV6_FIRST => vec![ipv6_loopback_address(), ipv4_loopback_address()],
        _ => vec![ipv4_loopback_address(), ipv6_loopback_address()],
    }
}

fn parse_lookup_options(value: f64) -> Result<LookupOptions, f64> {
    let js_value = JSValue::from_bits(value.to_bits());
    if js_value.is_undefined() || js_value.is_null() {
        return Ok(LookupOptions {
            family: 0,
            all: false,
        });
    }
    if numeric_value(value).is_some() {
        return Ok(LookupOptions {
            family: validate_family(value)?,
            all: false,
        });
    }
    if is_plain_object_value(value) && !is_callable_value(value) {
        let family_value = option_field(value, "family");
        let all_value = option_field(value, "all");
        let family = if JSValue::from_bits(family_value.to_bits()).is_undefined() {
            0
        } else {
            validate_family(family_value)?
        };
        return Ok(LookupOptions {
            family,
            all: JSValue::from_bits(all_value.to_bits()).to_bool(),
        });
    }
    Err(type_error_value(
        &format!(
            "The \"options\" argument must be of type object or number. Received {}",
            crate::fs::validate::describe_received(value)
        ),
        "ERR_INVALID_ARG_TYPE",
    ))
}

fn lookup_addresses(hostname: &str, family: i32) -> Result<Vec<ResolvedAddress>, f64> {
    if localhost_name(hostname) {
        return Ok(match family {
            4 => vec![ipv4_loopback_address()],
            6 => vec![ipv6_loopback_address()],
            _ => default_loopback_addresses(),
        });
    }
    if let Ok(addr) = hostname.parse::<IpAddr>() {
        let resolved_family = if addr.is_ipv4() { 4 } else { 6 };
        if family == 0 || family == resolved_family {
            return Ok(vec![ResolvedAddress {
                address: hostname.to_string(),
                family: resolved_family,
            }]);
        }
    }
    Err(dns_not_found_error(hostname))
}

fn lookup_result(address: &ResolvedAddress) -> f64 {
    object_value(&[
        ("address", str_value(&address.address)),
        ("family", address.family as f64),
    ])
}

fn lookup_all_result(addresses: &[ResolvedAddress]) -> f64 {
    let values: Vec<f64> = addresses.iter().map(lookup_result).collect();
    array_value_from_values(&values)
}

fn queue_callback(callback_value: f64, args: &[f64]) {
    let callback = closure_ptr_from_value(callback_value)
        .unwrap_or_else(|| throw_error_value(invalid_callback_error(callback_value)));
    unsafe {
        crate::builtins::js_queue_next_tick_args(callback as i64, args.as_ptr(), args.len() as i32);
    }
}

fn lookup_value(hostname: &str, options: LookupOptions) -> Result<f64, f64> {
    let addresses = lookup_addresses(hostname, options.family)?;
    if options.all {
        Ok(lookup_all_result(&addresses))
    } else {
        Ok(lookup_result(&addresses[0]))
    }
}

fn lookup_callback_values(hostname: &str, options: LookupOptions) -> Result<Vec<f64>, f64> {
    let addresses = lookup_addresses(hostname, options.family)?;
    if options.all {
        Ok(vec![null_value(), lookup_all_result(&addresses)])
    } else {
        let first = &addresses[0];
        Ok(vec![
            null_value(),
            str_value(&first.address),
            first.family as f64,
        ])
    }
}

fn parse_lookup_service_port(value: f64) -> Result<u16, f64> {
    let n = if let Some(n) = numeric_value(value) {
        n
    } else if let Some(s) = js_string_to_rust(value) {
        match s.parse::<f64>() {
            Ok(n) => n,
            Err(_) => return Err(bad_port_error(value)),
        }
    } else {
        return Err(bad_port_error(value));
    };
    if !n.is_finite() || n.fract() != 0.0 || !(0.0..65536.0).contains(&n) {
        return Err(bad_port_error(value));
    }
    Ok(n as u16)
}

fn service_for_port(port: u16) -> String {
    match port {
        22 => "ssh".to_string(),
        53 => "domain".to_string(),
        80 => "http".to_string(),
        443 => "https".to_string(),
        _ => port.to_string(),
    }
}

fn lookup_service_result(address: &str, port: u16) -> Result<(String, String), f64> {
    match address.parse::<IpAddr>() {
        Ok(addr) if addr.is_loopback() => Ok(("localhost".to_string(), service_for_port(port))),
        Ok(_) => Ok((address.to_string(), service_for_port(port))),
        Err(_) => Err(invalid_address_error(str_value(address))),
    }
}

fn lookup_service_object(hostname: &str, service: &str) -> f64 {
    object_value(&[
        ("hostname", str_value(hostname)),
        ("service", str_value(service)),
    ])
}

fn callback_record_args(args: i64, default_kind: Option<RecordKind>) -> (String, RecordKind, f64) {
    let name_value = arg(args, 0);
    let Some(name) = js_string_to_rust(name_value) else {
        throw_invalid_name(name_value);
    };

    match default_kind {
        Some(kind) => {
            let callback = if args_len(args) > 2 {
                arg(args, 2)
            } else {
                arg(args, 1)
            };
            if !is_callable_value(callback) {
                throw_invalid_callback(callback);
            }
            (name, kind, callback)
        }
        None => {
            let rrtype_or_callback = arg(args, 1);
            if is_callable_value(rrtype_or_callback) {
                return (name, RecordKind::A, rrtype_or_callback);
            }
            let kind = parse_record_kind(rrtype_or_callback);
            let callback = arg(args, 2);
            if !is_callable_value(callback) {
                throw_invalid_callback(callback);
            }
            (name, kind, callback)
        }
    }
}

fn promise_record_args(
    args: i64,
    default_kind: Option<RecordKind>,
) -> Result<(String, RecordKind), f64> {
    let name_value = arg(args, 0);
    let Some(name) = js_string_to_rust(name_value) else {
        return Err(invalid_name_error(name_value));
    };
    let kind = if let Some(kind) = default_kind {
        kind
    } else if args_len(args) < 2 {
        RecordKind::A
    } else {
        parse_record_kind_result(arg(args, 1))?
    };
    Ok((name, kind))
}

fn callback_reverse_args(args: i64) -> (String, f64) {
    let name_value = arg(args, 0);
    let Some(name) = js_string_to_rust(name_value) else {
        throw_invalid_name(name_value);
    };
    let callback = arg(args, 1);
    if !is_callable_value(callback) {
        throw_invalid_callback(callback);
    }
    (name, callback)
}

fn promise_reverse_args(args: i64) -> String {
    let name_value = arg(args, 0);
    let Some(name) = js_string_to_rust(name_value) else {
        throw_invalid_name(name_value);
    };
    name
}

fn call_success_callback(callback: f64, value: f64) {
    let args = [null_value(), value];
    unsafe {
        crate::closure::js_native_call_value(callback, args.as_ptr(), args.len());
    }
}

fn dns_callback_resolve(args: i64, default_kind: Option<RecordKind>) -> f64 {
    let (name, kind, callback) = callback_record_args(args, default_kind);
    let result = resolve_records(kind, &name);
    call_success_callback(callback, result);
    undefined_value()
}

fn dns_callback_reverse(args: i64) -> f64 {
    let (name, callback) = callback_reverse_args(args);
    let result = reverse_records(&name);
    call_success_callback(callback, result);
    undefined_value()
}

fn promise_value(value: f64) -> f64 {
    let promise = crate::promise::js_promise_resolved(value);
    js_nanbox_pointer(promise as i64)
}

fn promise_rejected_value(reason: f64) -> f64 {
    let promise = crate::promise::js_promise_rejected(reason);
    js_nanbox_pointer(promise as i64)
}

fn dns_promise_resolve(args: i64, default_kind: Option<RecordKind>) -> f64 {
    let (name, kind) = match promise_record_args(args, default_kind) {
        Ok(parsed) => parsed,
        Err(error) => return promise_rejected_value(error),
    };
    promise_value(resolve_records(kind, &name))
}

fn dns_promise_reverse(args: i64) -> f64 {
    let name = promise_reverse_args(args);
    promise_value(reverse_records(&name))
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

#[no_mangle]
pub extern "C" fn js_dns_noop(_args: i64) -> f64 {
    undefined_value()
}

#[no_mangle]
pub extern "C" fn js_dns_lookup(args: i64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let hostname_value = arg(args, 0);
    let hostname = match js_string_to_rust(hostname_value) {
        Some(hostname) => hostname,
        None => throw_error_value(invalid_hostname_error(hostname_value)),
    };

    let second = arg(args, 1);
    let (options_value, callback_value) = if is_callable_value(second) {
        (undefined_value(), second)
    } else {
        (second, arg(args, 2))
    };
    let callback_handle = scope.root_nanbox_f64(callback_value);
    if !is_callable_value(callback_value) {
        throw_error_value(invalid_callback_error(callback_value));
    }

    let options = match parse_lookup_options(options_value) {
        Ok(options) => options,
        Err(error) => throw_error_value(error),
    };
    let callback_args = match lookup_callback_values(&hostname, options) {
        Ok(values) => values,
        Err(error) => vec![error],
    };
    queue_callback(callback_handle.get_nanbox_f64(), &callback_args);
    undefined_value()
}

#[no_mangle]
pub extern "C" fn js_dns_lookup_service(args: i64) -> f64 {
    if args_len(args) < 3 || JSValue::from_bits(arg(args, 2).to_bits()).is_undefined() {
        throw_error_value(lookup_service_missing_args_error());
    }

    let address_value = arg(args, 0);
    let address = match js_string_to_rust(address_value) {
        Some(address) => address,
        None => throw_error_value(invalid_address_error(address_value)),
    };
    let port = match parse_lookup_service_port(arg(args, 1)) {
        Ok(port) => port,
        Err(error) => throw_error_value(error),
    };
    let callback_value = arg(args, 2);
    if !is_callable_value(callback_value) {
        throw_error_value(invalid_callback_error(callback_value));
    }
    let callback_args = match lookup_service_result(&address, port) {
        Ok((hostname, service)) => vec![null_value(), str_value(&hostname), str_value(&service)],
        Err(error) => vec![error],
    };
    queue_callback(callback_value, &callback_args);
    undefined_value()
}

#[no_mangle]
pub extern "C" fn js_dns_resolve(args: i64) -> f64 {
    dns_callback_resolve(args, None)
}

#[no_mangle]
pub extern "C" fn js_dns_resolve4(args: i64) -> f64 {
    dns_callback_resolve(args, Some(RecordKind::A))
}

#[no_mangle]
pub extern "C" fn js_dns_resolve6(args: i64) -> f64 {
    dns_callback_resolve(args, Some(RecordKind::Aaaa))
}

#[no_mangle]
pub extern "C" fn js_dns_resolve_any(args: i64) -> f64 {
    dns_callback_resolve(args, Some(RecordKind::Any))
}

#[no_mangle]
pub extern "C" fn js_dns_resolve_caa(args: i64) -> f64 {
    dns_callback_resolve(args, Some(RecordKind::Caa))
}

#[no_mangle]
pub extern "C" fn js_dns_resolve_cname(args: i64) -> f64 {
    dns_callback_resolve(args, Some(RecordKind::Cname))
}

#[no_mangle]
pub extern "C" fn js_dns_resolve_mx(args: i64) -> f64 {
    dns_callback_resolve(args, Some(RecordKind::Mx))
}

#[no_mangle]
pub extern "C" fn js_dns_resolve_naptr(args: i64) -> f64 {
    dns_callback_resolve(args, Some(RecordKind::Naptr))
}

#[no_mangle]
pub extern "C" fn js_dns_resolve_ns(args: i64) -> f64 {
    dns_callback_resolve(args, Some(RecordKind::Ns))
}

#[no_mangle]
pub extern "C" fn js_dns_resolve_ptr(args: i64) -> f64 {
    dns_callback_resolve(args, Some(RecordKind::Ptr))
}

#[no_mangle]
pub extern "C" fn js_dns_resolve_soa(args: i64) -> f64 {
    dns_callback_resolve(args, Some(RecordKind::Soa))
}

#[no_mangle]
pub extern "C" fn js_dns_resolve_srv(args: i64) -> f64 {
    dns_callback_resolve(args, Some(RecordKind::Srv))
}

#[no_mangle]
pub extern "C" fn js_dns_resolve_tlsa(args: i64) -> f64 {
    dns_callback_resolve(args, Some(RecordKind::Tlsa))
}

#[no_mangle]
pub extern "C" fn js_dns_resolve_txt(args: i64) -> f64 {
    dns_callback_resolve(args, Some(RecordKind::Txt))
}

#[no_mangle]
pub extern "C" fn js_dns_reverse(args: i64) -> f64 {
    dns_callback_reverse(args)
}

#[no_mangle]
pub extern "C" fn js_dns_promises_noop(_args: i64) -> f64 {
    let promise = crate::promise::js_promise_resolved(undefined_value());
    js_nanbox_pointer(promise as i64)
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolve(args: i64) -> f64 {
    dns_promise_resolve(args, None)
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolve4(args: i64) -> f64 {
    dns_promise_resolve(args, Some(RecordKind::A))
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolve6(args: i64) -> f64 {
    dns_promise_resolve(args, Some(RecordKind::Aaaa))
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolve_any(args: i64) -> f64 {
    dns_promise_resolve(args, Some(RecordKind::Any))
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolve_caa(args: i64) -> f64 {
    dns_promise_resolve(args, Some(RecordKind::Caa))
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolve_cname(args: i64) -> f64 {
    dns_promise_resolve(args, Some(RecordKind::Cname))
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolve_mx(args: i64) -> f64 {
    dns_promise_resolve(args, Some(RecordKind::Mx))
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolve_naptr(args: i64) -> f64 {
    dns_promise_resolve(args, Some(RecordKind::Naptr))
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolve_ns(args: i64) -> f64 {
    dns_promise_resolve(args, Some(RecordKind::Ns))
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolve_ptr(args: i64) -> f64 {
    dns_promise_resolve(args, Some(RecordKind::Ptr))
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolve_soa(args: i64) -> f64 {
    dns_promise_resolve(args, Some(RecordKind::Soa))
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolve_srv(args: i64) -> f64 {
    dns_promise_resolve(args, Some(RecordKind::Srv))
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolve_tlsa(args: i64) -> f64 {
    dns_promise_resolve(args, Some(RecordKind::Tlsa))
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolve_txt(args: i64) -> f64 {
    dns_promise_resolve(args, Some(RecordKind::Txt))
}

#[no_mangle]
pub extern "C" fn js_dns_promises_reverse(args: i64) -> f64 {
    dns_promise_reverse(args)
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
pub extern "C" fn js_dns_resolver_resolve(_handle: i64, args: i64) -> f64 {
    dns_callback_resolve(args, None)
}

#[no_mangle]
pub extern "C" fn js_dns_resolver_resolve4(_handle: i64, args: i64) -> f64 {
    dns_callback_resolve(args, Some(RecordKind::A))
}

#[no_mangle]
pub extern "C" fn js_dns_resolver_resolve6(_handle: i64, args: i64) -> f64 {
    dns_callback_resolve(args, Some(RecordKind::Aaaa))
}

#[no_mangle]
pub extern "C" fn js_dns_resolver_resolve_any(_handle: i64, args: i64) -> f64 {
    dns_callback_resolve(args, Some(RecordKind::Any))
}

#[no_mangle]
pub extern "C" fn js_dns_resolver_resolve_caa(_handle: i64, args: i64) -> f64 {
    dns_callback_resolve(args, Some(RecordKind::Caa))
}

#[no_mangle]
pub extern "C" fn js_dns_resolver_resolve_cname(_handle: i64, args: i64) -> f64 {
    dns_callback_resolve(args, Some(RecordKind::Cname))
}

#[no_mangle]
pub extern "C" fn js_dns_resolver_resolve_mx(_handle: i64, args: i64) -> f64 {
    dns_callback_resolve(args, Some(RecordKind::Mx))
}

#[no_mangle]
pub extern "C" fn js_dns_resolver_resolve_naptr(_handle: i64, args: i64) -> f64 {
    dns_callback_resolve(args, Some(RecordKind::Naptr))
}

#[no_mangle]
pub extern "C" fn js_dns_resolver_resolve_ns(_handle: i64, args: i64) -> f64 {
    dns_callback_resolve(args, Some(RecordKind::Ns))
}

#[no_mangle]
pub extern "C" fn js_dns_resolver_resolve_ptr(_handle: i64, args: i64) -> f64 {
    dns_callback_resolve(args, Some(RecordKind::Ptr))
}

#[no_mangle]
pub extern "C" fn js_dns_resolver_resolve_soa(_handle: i64, args: i64) -> f64 {
    dns_callback_resolve(args, Some(RecordKind::Soa))
}

#[no_mangle]
pub extern "C" fn js_dns_resolver_resolve_srv(_handle: i64, args: i64) -> f64 {
    dns_callback_resolve(args, Some(RecordKind::Srv))
}

#[no_mangle]
pub extern "C" fn js_dns_resolver_resolve_tlsa(_handle: i64, args: i64) -> f64 {
    dns_callback_resolve(args, Some(RecordKind::Tlsa))
}

#[no_mangle]
pub extern "C" fn js_dns_resolver_resolve_txt(_handle: i64, args: i64) -> f64 {
    dns_callback_resolve(args, Some(RecordKind::Txt))
}

#[no_mangle]
pub extern "C" fn js_dns_resolver_reverse(_handle: i64, args: i64) -> f64 {
    dns_callback_reverse(args)
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolver_resolve(_handle: i64, args: i64) -> f64 {
    dns_promise_resolve(args, None)
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolver_resolve4(_handle: i64, args: i64) -> f64 {
    dns_promise_resolve(args, Some(RecordKind::A))
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolver_resolve6(_handle: i64, args: i64) -> f64 {
    dns_promise_resolve(args, Some(RecordKind::Aaaa))
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolver_resolve_any(_handle: i64, args: i64) -> f64 {
    dns_promise_resolve(args, Some(RecordKind::Any))
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolver_resolve_caa(_handle: i64, args: i64) -> f64 {
    dns_promise_resolve(args, Some(RecordKind::Caa))
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolver_resolve_cname(_handle: i64, args: i64) -> f64 {
    dns_promise_resolve(args, Some(RecordKind::Cname))
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolver_resolve_mx(_handle: i64, args: i64) -> f64 {
    dns_promise_resolve(args, Some(RecordKind::Mx))
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolver_resolve_naptr(_handle: i64, args: i64) -> f64 {
    dns_promise_resolve(args, Some(RecordKind::Naptr))
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolver_resolve_ns(_handle: i64, args: i64) -> f64 {
    dns_promise_resolve(args, Some(RecordKind::Ns))
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolver_resolve_ptr(_handle: i64, args: i64) -> f64 {
    dns_promise_resolve(args, Some(RecordKind::Ptr))
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolver_resolve_soa(_handle: i64, args: i64) -> f64 {
    dns_promise_resolve(args, Some(RecordKind::Soa))
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolver_resolve_srv(_handle: i64, args: i64) -> f64 {
    dns_promise_resolve(args, Some(RecordKind::Srv))
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolver_resolve_tlsa(_handle: i64, args: i64) -> f64 {
    dns_promise_resolve(args, Some(RecordKind::Tlsa))
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolver_resolve_txt(_handle: i64, args: i64) -> f64 {
    dns_promise_resolve(args, Some(RecordKind::Txt))
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolver_reverse(_handle: i64, args: i64) -> f64 {
    dns_promise_reverse(args)
}

#[no_mangle]
pub extern "C" fn js_dns_promises_lookup(args: i64) -> f64 {
    let hostname_value = arg(args, 0);
    let hostname = match js_string_to_rust(hostname_value) {
        Some(hostname) => hostname,
        None => return promise_rejected_value(invalid_hostname_error(hostname_value)),
    };
    let options = match parse_lookup_options(arg(args, 1)) {
        Ok(options) => options,
        Err(error) => return promise_rejected_value(error),
    };
    match lookup_value(&hostname, options) {
        Ok(value) => promise_value(value),
        Err(error) => promise_rejected_value(error),
    }
}

#[no_mangle]
pub extern "C" fn js_dns_promises_lookup_service(args: i64) -> f64 {
    if args_len(args) < 2 {
        return promise_rejected_value(lookup_service_missing_args_error());
    }
    let address_value = arg(args, 0);
    let address = match js_string_to_rust(address_value) {
        Some(address) => address,
        None => return promise_rejected_value(invalid_address_error(address_value)),
    };
    let port = match parse_lookup_service_port(arg(args, 1)) {
        Ok(port) => port,
        Err(error) => return promise_rejected_value(error),
    };
    match lookup_service_result(&address, port) {
        Ok((hostname, service)) => promise_value(lookup_service_object(&hostname, &service)),
        Err(error) => promise_rejected_value(error),
    }
}
