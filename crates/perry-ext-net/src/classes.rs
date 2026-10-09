use perry_ffi::native_payload::{self as np, PayloadFamily, PayloadMiss};
use perry_ffi::TransientRootScope;
use perry_ffi::{
    alloc_string, js_array_alloc, js_array_get, js_array_push, ArrayHeader, JsValue, StringHeader,
};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
#[path = "classes_prototype.rs"]
mod prototype;

const BLOCK_LIST_ID: u32 = perry_ffi::native_class_ids::NET_BLOCK_LIST;
const SOCKET_ADDRESS_ID: u32 = perry_ffi::native_class_ids::NET_SOCKET_ADDRESS;
static BLOCK_LIST_VTABLE: perry_ffi::native_stream::PayloadVTable = perry_ffi::native_stream::payload_vtable::<BlockListState>(None);
static BLOCK_LIST: PayloadFamily = PayloadFamily::new::<BlockListState>(BLOCK_LIST_ID, "BlockList", false, &BLOCK_LIST_VTABLE)
    .with_constructor_length(0)
    .with_installer(prototype::install_block_list);
static SOCKET_ADDRESS_VTABLE: perry_ffi::native_stream::PayloadVTable = perry_ffi::native_stream::payload_vtable::<SocketAddressState>(None);
static SOCKET_ADDRESS: PayloadFamily = PayloadFamily::new::<SocketAddressState>(SOCKET_ADDRESS_ID, "SocketAddress", false, &SOCKET_ADDRESS_VTABLE)
    .with_constructor_length(0)
    .with_installer(prototype::install_socket_address);

use crate::{get_object_number_field, get_object_string_field, string_from_header_i64};

const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;

#[derive(Clone)]
struct BlockListState {
    rules: Vec<BlockRule>,
}

#[derive(Clone)]
enum BlockRule {
    Address(IpAddr),
    Range(IpAddr, IpAddr),
    Subnet(IpAddr, u8),
}

#[derive(Clone)]
struct SocketAddressState {
    address: IpAddr,
    port: u16,
    flowlabel: u32,
}

// The static native-call ABI passes a raw object address. State is reached
// through that ordinary object's class-checked cell, never through an id.
unsafe fn block_list<'a>(handle: i64) -> &'a mut BlockListState {
    np::payload_mut(boxed_handle(handle), &BLOCK_LIST).unwrap_or_else(throw_miss)
}

fn throw_miss<T>(miss: PayloadMiss) -> T {
    extern "C" {
        fn js_typeerror_new(message: *mut StringHeader) -> *mut u8;
    }
    // Release transports JS exceptions as raw foreign unwind records. Its
    // panic=abort C-unwind call guard would abort before the generated catch.
    #[cfg(panic = "abort")]
    extern "C" {
        fn js_throw(value: f64) -> !;
    }
    #[cfg(not(panic = "abort"))]
    extern "C-unwind" {
        fn js_throw(value: f64) -> !;
    }
    let text = match miss {
        PayloadMiss::Foreign => "Illegal receiver",
        PayloadMiss::Closed => "Native payload is closed",
    };
    let message = alloc_string(text);
    unsafe {
        js_throw(f64::from_bits(
            JsValue::from_object_ptr(js_typeerror_new(message.as_raw())).bits(),
        ))
    }
}

extern "C" {
    fn js_get_string_pointer_unified(value: f64) -> i64;
    fn js_net_throw_invalid_address() -> !;
    fn js_net_validate_block_list_prefix(prefix: f64, max: f64) -> i32;
    fn js_net_validate_listen_port(value: f64);
}

fn undefined() -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

fn js_bool(value: bool) -> f64 {
    f64::from_bits(JsValue::from_bool(value).bits())
}

fn boxed_handle(handle: i64) -> f64 {
    f64::from_bits(0x7FFD_0000_0000_0000 | (handle as u64 & 0x0000_FFFF_FFFF_FFFF))
}

fn handle_from_value(value: f64) -> Option<i64> {
    let value = JsValue::from_bits(value.to_bits());
    if value.is_pointer() {
        Some(value.as_pointer::<u8>() as i64)
    } else {
        None
    }
}

fn optional_string(ptr: i64) -> Option<String> {
    if ptr == 0 {
        None
    } else {
        unsafe { string_from_header_i64(ptr) }
    }
}

fn family_name(ip: &IpAddr) -> &'static str {
    if ip.is_ipv4() {
        "IPv4"
    } else {
        "IPv6"
    }
}

fn family_matches(ip: &IpAddr, family: Option<&str>) -> bool {
    match family {
        Some("ipv4") => ip.is_ipv4(),
        Some("ipv6") => ip.is_ipv6(),
        Some(_) => unsafe { js_net_throw_invalid_address() },
        None => true,
    }
}

fn parse_ip(ptr: i64, family_ptr: i64) -> IpAddr {
    let Some(s) = optional_string(ptr) else {
        unsafe { js_net_throw_invalid_address() }
    };
    let family = optional_string(family_ptr).map(|s| s.to_ascii_lowercase());
    let Ok(ip) = s.parse::<IpAddr>() else {
        unsafe { js_net_throw_invalid_address() }
    };
    if !family_matches(&ip, family.as_deref()) {
        unsafe { js_net_throw_invalid_address() }
    }
    ip
}

fn rule_to_string(rule: &BlockRule) -> String {
    match rule {
        BlockRule::Address(ip) => format!("Address: {} {}", family_name(ip), ip),
        BlockRule::Range(start, end) => {
            format!("Range: {} {}-{}", family_name(start), start, end)
        }
        BlockRule::Subnet(ip, prefix) => format!("Subnet: {} {}/{}", family_name(ip), ip, prefix),
    }
}

fn rules_array(rules: &[BlockRule]) -> *mut ArrayHeader {
    let scope = TransientRootScope::enter();
    let mut arr = scope.root_addr(unsafe { js_array_alloc(rules.len() as u32) } as i64);
    for rule in rules.iter().rev() {
        let s = alloc_string(&rule_to_string(rule));
        let next = unsafe {
            js_array_push(
                arr.get() as *mut ArrayHeader,
                JsValue::from_string_ptr(s.as_raw()),
            )
        };
        arr = scope.root_addr(next as i64);
    }
    arr.get() as *mut ArrayHeader
}

fn block_list_rules_array(handle: i64) -> *mut ArrayHeader {
    // Snapshot Rust-only data before allocating; no payload borrow crosses GC.
    let rules = unsafe { block_list(handle).rules.clone() };
    rules_array(&rules)
}

fn addr_to_u128(ip: IpAddr) -> u128 {
    match ip {
        IpAddr::V4(v4) => u32::from(v4) as u128,
        IpAddr::V6(v6) => u128::from(v6),
    }
}

fn same_family(a: IpAddr, b: IpAddr) -> bool {
    matches!(
        (a, b),
        (IpAddr::V4(_), IpAddr::V4(_)) | (IpAddr::V6(_), IpAddr::V6(_))
    )
}

fn rule_contains(rule: &BlockRule, ip: IpAddr) -> bool {
    match *rule {
        BlockRule::Address(addr) => addr == ip,
        BlockRule::Range(start, end) => {
            same_family(start, ip)
                && addr_to_u128(start) <= addr_to_u128(ip)
                && addr_to_u128(ip) <= addr_to_u128(end)
        }
        BlockRule::Subnet(base, prefix) => {
            if !same_family(base, ip) {
                return false;
            }
            let bits = if base.is_ipv4() { 32 } else { 128 };
            let shift = bits - prefix as u32;
            shift == bits || (addr_to_u128(base) >> shift) == (addr_to_u128(ip) >> shift)
        }
    }
}

fn parse_rule_string(rule: &str) -> Option<BlockRule> {
    let (kind, rest) = rule.split_once(": ")?;
    let (family, value) = rest.split_once(' ')?;
    let family = family.to_ascii_lowercase();
    match kind {
        "Address" => Some(BlockRule::Address(parse_ip_string(value, Some(&family))?)),
        "Range" => {
            let (start, end) = value.split_once('-')?;
            Some(BlockRule::Range(
                parse_ip_string(start, Some(&family))?,
                parse_ip_string(end, Some(&family))?,
            ))
        }
        "Subnet" => {
            let (addr, prefix) = value.split_once('/')?;
            Some(BlockRule::Subnet(
                parse_ip_string(addr, Some(&family))?,
                prefix.parse().ok()?,
            ))
        }
        _ => None,
    }
}

fn parse_ip_string(value: &str, family: Option<&str>) -> Option<IpAddr> {
    let ip = value.parse::<IpAddr>().ok()?;
    if family_matches(&ip, family) {
        Some(ip)
    } else {
        None
    }
}

fn string_from_js_value(value: JsValue) -> Option<String> {
    if value.is_string() {
        unsafe { string_from_header_i64(value.as_string_ptr() as i64) }
    } else {
        let ptr = unsafe { js_get_string_pointer_unified(f64::from_bits(value.bits())) };
        unsafe { string_from_header_i64(ptr) }
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_net_block_list_new() -> i64 {
    handle_from_value(unsafe { np::alloc_in(&BLOCK_LIST, "net",
        BlockListState { rules: Vec::new() },
        0,
        &[],
    ) })
    .unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn js_ext_net_is_block_list_handle(handle: i64) -> i32 {
    np::lifecycle(boxed_handle(handle), &BLOCK_LIST).is_ok() as i32
}

#[no_mangle]
pub extern "C" fn js_net_block_list_is_block_list(value: f64) -> f64 {
    js_bool(np::lifecycle(value, &BLOCK_LIST).is_ok())
}

#[no_mangle]
pub unsafe extern "C" fn js_net_block_list_add_address(
    handle: i64,
    address_ptr: i64,
    family_ptr: i64,
) -> f64 {
    let scope = TransientRootScope::enter();
    let owner = scope.root_addr(handle);
    let ip = parse_ip(address_ptr, family_ptr);
    block_list(owner.get()).rules.push(BlockRule::Address(ip));
    let bytes = block_list(owner.get()).rules.capacity() * std::mem::size_of::<BlockRule>();
    np::set_external_bytes(boxed_handle(owner.get()), &BLOCK_LIST_VTABLE, bytes);
    undefined()
}

#[no_mangle]
pub unsafe extern "C" fn js_net_block_list_add_range(
    handle: i64,
    start_ptr: i64,
    end_ptr: i64,
    family_ptr: i64,
) -> f64 {
    let scope = TransientRootScope::enter();
    let owner = scope.root_addr(handle);
    let start = parse_ip(start_ptr, family_ptr);
    let end = parse_ip(end_ptr, family_ptr);
    if !same_family(start, end) {
        js_net_throw_invalid_address();
    }
    block_list(owner.get())
        .rules
        .push(BlockRule::Range(start, end));
    let bytes = block_list(owner.get()).rules.capacity() * std::mem::size_of::<BlockRule>();
    np::set_external_bytes(boxed_handle(owner.get()), &BLOCK_LIST_VTABLE, bytes);
    undefined()
}

#[no_mangle]
pub unsafe extern "C" fn js_net_block_list_add_subnet(
    handle: i64,
    address_ptr: i64,
    prefix: f64,
    family_ptr: i64,
) -> f64 {
    let scope = TransientRootScope::enter();
    let owner = scope.root_addr(handle);
    let ip = parse_ip(address_ptr, family_ptr);
    let max = if ip.is_ipv4() { 32.0 } else { 128.0 };
    let prefix = js_net_validate_block_list_prefix(prefix, max) as u8;
    block_list(owner.get())
        .rules
        .push(BlockRule::Subnet(ip, prefix));
    let bytes = block_list(owner.get()).rules.capacity() * std::mem::size_of::<BlockRule>();
    np::set_external_bytes(boxed_handle(owner.get()), &BLOCK_LIST_VTABLE, bytes);
    undefined()
}

#[no_mangle]
pub unsafe extern "C" fn js_net_block_list_check(
    handle: i64,
    address_ptr: i64,
    family_ptr: i64,
) -> f64 {
    let scope = TransientRootScope::enter();
    let owner = scope.root_addr(handle);
    let ip = parse_ip(address_ptr, family_ptr);
    let contains = block_list(owner.get())
        .rules
        .iter()
        .any(|rule| rule_contains(rule, ip));
    js_bool(contains)
}

#[no_mangle]
pub extern "C" fn js_net_block_list_to_json(handle: i64) -> f64 {
    f64::from_bits(JsValue::from_object_ptr(block_list_rules_array(handle)).bits())
}

#[no_mangle]
pub extern "C" fn js_net_block_list_rules(handle: i64) -> *mut ArrayHeader {
    block_list_rules_array(handle)
}

#[no_mangle]
pub unsafe extern "C" fn js_net_block_list_from_json(handle: i64, value: f64) -> f64 {
    let scope = TransientRootScope::enter();
    let receiver = scope.root_addr(handle);
    let value = JsValue::from_bits(value.to_bits());
    if !value.is_pointer() {
        return undefined();
    }
    let arr = value.as_pointer::<ArrayHeader>();
    if arr.is_null() {
        return undefined();
    }
    let arr = scope.root_addr(arr as i64);
    let mut parsed = Vec::new();
    let length = (*(arr.get() as *mut ArrayHeader)).length;
    for i in 0..length {
        if let Some(rule) = string_from_js_value(js_array_get(arr.get() as *mut ArrayHeader, i))
            .and_then(|s| parse_rule_string(&s))
        {
            parsed.push(rule);
        }
    }
    block_list(receiver.get()).rules = parsed;
    let bytes = block_list(receiver.get()).rules.capacity() * std::mem::size_of::<BlockRule>();
    np::set_external_bytes(boxed_handle(receiver.get()), &BLOCK_LIST_VTABLE, bytes);
    undefined()
}

fn socket_address_new(address: IpAddr, port: u16, flowlabel: u32) -> f64 {
    unsafe { np::alloc_in(&SOCKET_ADDRESS, "net",
        SocketAddressState {
            address,
            port,
            flowlabel,
        },
        0,
        &[],
    ) }
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_address_new(options: f64) -> i64 {
    let scope = TransientRootScope::enter();
    let options = scope.root_nanbox(options);
    let address =
        get_object_string_field(options.get(), "address").unwrap_or_else(|| "127.0.0.1".into());
    let family = get_object_string_field(options.get(), "family").map(|s| s.to_ascii_lowercase());
    let Ok(ip) = address.parse::<IpAddr>() else {
        js_net_throw_invalid_address()
    };
    if !family_matches(&ip, family.as_deref()) {
        js_net_throw_invalid_address();
    }
    let port = get_object_number_field(options.get(), "port").unwrap_or(0.0);
    js_net_validate_listen_port(port);
    let flowlabel = get_object_number_field(options.get(), "flowlabel")
        .unwrap_or(0.0)
        .max(0.0) as u32;
    handle_from_value(socket_address_new(ip, port as u16, flowlabel)).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn js_ext_net_is_socket_address_handle(handle: i64) -> i32 {
    np::lifecycle(boxed_handle(handle), &SOCKET_ADDRESS).is_ok() as i32
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_address_parse(input_ptr: i64) -> f64 {
    let Some(input) = string_from_header_i64(input_ptr) else {
        return undefined();
    };
    let parsed = if let Some(rest) = input.strip_prefix('[') {
        let Some((addr, port_part)) = rest.split_once("]:") else {
            return undefined();
        };
        let Ok(ip) = addr.parse::<Ipv6Addr>() else {
            return undefined();
        };
        port_part
            .parse::<u16>()
            .ok()
            .map(|port| (IpAddr::V6(ip), port))
    } else {
        let Some((addr, port_part)) = input.rsplit_once(':') else {
            return undefined();
        };
        if addr.contains(':') {
            return undefined();
        }
        let Ok(ip) = addr.parse::<Ipv4Addr>() else {
            return undefined();
        };
        port_part
            .parse::<u16>()
            .ok()
            .map(|port| (IpAddr::V4(ip), port))
    };
    match parsed {
        Some((ip, port)) => socket_address_new(ip, port, 0),
        None => undefined(),
    }
}

fn with_socket_address<T>(handle: i64, _default: T, f: impl FnOnce(&SocketAddressState) -> T) -> T {
    let state =
        unsafe { np::payload_mut::<SocketAddressState>(boxed_handle(handle), &SOCKET_ADDRESS) }
            .unwrap_or_else(throw_miss);
    f(state)
}

#[no_mangle]
pub extern "C" fn js_net_socket_address_get_address(handle: i64) -> *mut StringHeader {
    alloc_string(&with_socket_address(handle, String::new(), |s| {
        s.address.to_string()
    }))
    .as_raw()
}

#[no_mangle]
pub extern "C" fn js_net_socket_address_get_family(handle: i64) -> *mut StringHeader {
    let family = with_socket_address(handle, "ipv4", |s| {
        if s.address.is_ipv6() {
            "ipv6"
        } else {
            "ipv4"
        }
    });
    alloc_string(family).as_raw()
}

#[no_mangle]
pub extern "C" fn js_net_socket_address_get_port(handle: i64) -> f64 {
    with_socket_address(handle, 0, |s| s.port) as f64
}

#[no_mangle]
pub extern "C" fn js_net_socket_address_get_flowlabel(handle: i64) -> f64 {
    with_socket_address(handle, 0, |s| s.flowlabel) as f64
}

#[no_mangle]
pub unsafe extern "C" fn js_net_socket_address_parse_value(input: f64) -> f64 {
    extern "C" {
        fn js_net_validate_socket_address_parse_input(input: f64) -> i64;
    }
    js_net_socket_address_parse(js_net_validate_socket_address_parse_input(input))
}

#[no_mangle]
pub extern "C" fn js_net_socket_address_to_json(handle: i64) -> f64 {
    let state = with_socket_address(handle, None, |s| Some(s.clone())).unwrap();
    let scope = TransientRootScope::enter();
    let address = scope.root_nanbox(f64::from_bits(
        JsValue::from_string_ptr(alloc_string(&state.address.to_string()).as_raw()).bits(),
    ));
    let family = scope.root_nanbox(f64::from_bits(
        JsValue::from_string_ptr(
            alloc_string(if state.address.is_ipv6() {
                "ipv6"
            } else {
                "ipv4"
            })
            .as_raw(),
        )
        .bits(),
    ));
    let keys = ["address", "port", "family", "flowlabel"];
    let (packed, shape_id) = perry_ffi::build_object_shape(&keys);
    let object = scope.root_addr(unsafe {
        perry_ffi::js_object_alloc_with_shape(shape_id, 4, packed.as_ptr(), packed.len() as u32)
    } as i64);
    for (i, value) in [
        address.get(),
        state.port as f64,
        family.get(),
        state.flowlabel as f64,
    ]
    .into_iter()
    .enumerate()
    {
        unsafe {
            perry_ffi::js_object_set_field(
                object.get() as *mut perry_ffi::ObjectHeader,
                i as u32,
                JsValue::from_bits(value.to_bits()),
            );
        }
    }
    boxed_handle(object.get())
}
