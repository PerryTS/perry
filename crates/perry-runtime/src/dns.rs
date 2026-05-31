//! Runtime `node:dns` / `node:dns/promises` support.
//!
//! Constants and error-code aliases live in `object::native_module`
//! (`dns_lookup_flag_constant` / `dns_error_alias`). This module implements
//! the OS-backed `lookup` / `lookupService` surface (#3162) plus the
//! remaining shape stubs the inventory fixtures probe.
//!
//! `lookup` / `lookupService` use the system resolver
//! (`std::net::ToSocketAddrs` = `getaddrinfo`, plus `getnameinfo` for the
//! reverse direction), so loopback names (`localhost` / `127.0.0.1`) resolve
//! deterministically without depending on live external DNS.

use std::ffi::CStr;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::sync::atomic::{AtomicU8, Ordering};

use crate::closure::{js_closure_alloc, js_register_closure_arity, ClosureHeader};
use crate::object::{js_object_alloc, js_object_set_field_by_name, ObjectHeader};
use crate::string::str_bytes_from_jsvalue;
use crate::value::{js_nanbox_pointer, JSValue, TAG_NULL, TAG_UNDEFINED};

const RESULT_ORDER_VERBATIM: u8 = 0;
const RESULT_ORDER_IPV4_FIRST: u8 = 1;

static DEFAULT_RESULT_ORDER: AtomicU8 = AtomicU8::new(RESULT_ORDER_VERBATIM);

const RESOLVER_METHODS: &[&str] = &["cancel", "getServers", "setServers", "setLocalAddress"];

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

extern "C" fn dns_noop_thunk(_closure: *const ClosureHeader) -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

fn method_value(name: &str) -> f64 {
    let func_ptr = dns_noop_thunk as *const u8;
    let closure = js_closure_alloc(func_ptr, 0);
    js_register_closure_arity(func_ptr, 0);
    crate::object::set_bound_native_closure_name(closure, name);
    js_nanbox_pointer(closure as i64)
}

fn resolver_object(include_set_local_address: bool) -> *mut ObjectHeader {
    let method_count = if include_set_local_address {
        RESOLVER_METHODS.len()
    } else {
        RESOLVER_METHODS.len() - 1
    };
    let obj = js_object_alloc(0, method_count as u32);
    for method in RESOLVER_METHODS {
        if !include_set_local_address && *method == "setLocalAddress" {
            continue;
        }
        js_object_set_field_by_name(obj, key(method), method_value(method));
    }
    obj
}

// ---------------------------------------------------------------------------
// Value <-> Rust helpers
// ---------------------------------------------------------------------------

fn read_js_string(value: f64) -> Option<String> {
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let (ptr, len) = str_bytes_from_jsvalue(value, &mut scratch)?;
    if ptr.is_null() {
        return Some(String::new());
    }
    unsafe {
        let bytes = std::slice::from_raw_parts(ptr, len as usize);
        Some(String::from_utf8_lossy(bytes).into_owned())
    }
}

fn read_js_number(value: f64) -> Option<f64> {
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_int32() {
        Some(jv.as_int32() as f64)
    } else if jv.is_number() && !value.is_nan() {
        Some(value)
    } else {
        None
    }
}

fn is_callable(value: f64) -> bool {
    let jv = JSValue::from_bits(value.to_bits());
    jv.is_pointer() && !jv.is_undefined() && !jv.is_null()
}

fn is_object(value: f64) -> bool {
    let jv = JSValue::from_bits(value.to_bits());
    jv.is_pointer() && !jv.is_any_string() && !jv.is_bigint()
}

/// Read an args array (the `NA_VARARGS` `*const ArrayHeader`) into a Vec of
/// NaN-boxed f64 values.
unsafe fn read_args(args: i64) -> Vec<f64> {
    if args == 0 {
        return Vec::new();
    }
    let arr = args as *const crate::array::ArrayHeader;
    if arr.is_null() {
        return Vec::new();
    }
    let n = crate::array::js_array_length(arr);
    let mut out = Vec::with_capacity(n as usize);
    for i in 0..n {
        out.push(crate::array::js_array_get_f64(arr, i));
    }
    out
}

/// Parsed `dns.lookup` options.
#[derive(Default, Clone, Copy)]
struct LookupOptions {
    family: u8, // 0 = any, 4 = IPv4, 6 = IPv6
    all: bool,
}

/// Extract `{ family, all }` from an options value (number = family shorthand,
/// object = `{ family, all }`). Returns `Err` only for invalid `family`.
unsafe fn parse_lookup_options(value: f64) -> Result<LookupOptions, ()> {
    let mut opts = LookupOptions::default();
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_undefined() || jv.is_null() {
        return Ok(opts);
    }
    if let Some(n) = read_js_number(value) {
        // numeric shorthand: dns.lookup(host, family, cb)
        let fam = n as i64;
        if fam != 0 && fam != 4 && fam != 6 {
            return Err(());
        }
        opts.family = fam as u8;
        return Ok(opts);
    }
    if is_object(value) {
        let obj = jv.as_pointer::<ObjectHeader>();
        let fam_v = crate::object::js_object_get_field_by_name_f64(obj, key("family"));
        let fam_jv = JSValue::from_bits(fam_v.to_bits());
        if !fam_jv.is_undefined() && !fam_jv.is_null() {
            if let Some(n) = read_js_number(fam_v) {
                let fam = n as i64;
                if fam != 0 && fam != 4 && fam != 6 {
                    return Err(());
                }
                opts.family = fam as u8;
            } else if let Some(s) = read_js_string(fam_v) {
                // Node accepts "IPv4"/"IPv6" string family too.
                match s.as_str() {
                    "IPv4" => opts.family = 4,
                    "IPv6" => opts.family = 6,
                    _ => return Err(()),
                }
            }
        }
        let all_v = crate::object::js_object_get_field_by_name_f64(obj, key("all"));
        opts.all = crate::value::js_is_truthy(all_v) != 0;
    }
    Ok(opts)
}

fn ip_family(ip: &IpAddr) -> u8 {
    match ip {
        IpAddr::V4(_) => 4,
        IpAddr::V6(_) => 6,
    }
}

/// Resolve a hostname to a list of (address-string, family) pairs using the
/// system resolver. Honors `family` filtering. The order matches the OS
/// `getaddrinfo` order (Node's default "verbatim").
fn resolve_addresses(host: &str, opts: &LookupOptions) -> Result<Vec<(String, u8)>, String> {
    // A literal IP short-circuits — Node returns it verbatim without a query.
    if let Ok(ip) = host.parse::<IpAddr>() {
        let fam = ip_family(&ip);
        if opts.family != 0 && opts.family != fam {
            return Ok(Vec::new());
        }
        return Ok(vec![(ip.to_string(), fam)]);
    }

    // getaddrinfo via ToSocketAddrs. Port 0 is fine; we only want addresses.
    let iter = (host, 0u16).to_socket_addrs().map_err(|e| e.to_string())?;
    let mut out: Vec<(String, u8)> = Vec::new();
    for sa in iter {
        let ip = sa.ip();
        let fam = ip_family(&ip);
        if opts.family != 0 && opts.family != fam {
            continue;
        }
        let addr = ip.to_string();
        if !out.iter().any(|(a, _)| a == &addr) {
            out.push((addr, fam));
        }
    }
    Ok(out)
}

fn lookup_result_object(address: &str, family: u8) -> f64 {
    let obj = js_object_alloc(0, 2);
    js_object_set_field_by_name(obj, key("address"), str_value(address));
    js_object_set_field_by_name(obj, key("family"), family as f64);
    boxed_pointer(obj as *const u8)
}

fn lookup_all_array(addrs: &[(String, u8)]) -> f64 {
    let mut arr = crate::array::js_array_alloc(addrs.len() as u32);
    for (addr, fam) in addrs {
        arr = crate::array::js_array_push_f64(arr, lookup_result_object(addr, *fam));
    }
    js_nanbox_pointer(arr as i64)
}

/// Build a Node-shaped DNS error object `{ message, code, syscall, hostname }`.
fn dns_error_object(code: &str, syscall: &str, hostname: &str) -> f64 {
    let obj = js_object_alloc(0, 4);
    let msg = format!("{syscall} {code} {hostname}");
    js_object_set_field_by_name(obj, key("message"), str_value(&msg));
    js_object_set_field_by_name(obj, key("code"), str_value(code));
    js_object_set_field_by_name(obj, key("syscall"), str_value(syscall));
    js_object_set_field_by_name(obj, key("hostname"), str_value(hostname));
    boxed_pointer(obj as *const u8)
}

fn undefined() -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

fn null() -> f64 {
    f64::from_bits(TAG_NULL)
}

/// Schedule a callback `(err, ...rest)` to run asynchronously, matching Node's
/// "callbacks fire on a later tick" contract. `cb` is a NaN-boxed closure
/// value; `js_set_immediate_callback_args` wants the raw closure pointer, so
/// unbox via `js_timer_validate_callback` (which masks + verifies the pointer).
fn schedule_callback(cb: f64, args: &[f64]) {
    unsafe {
        let raw = crate::timer::js_timer_validate_callback(cb, 2);
        if raw == 0 {
            return;
        }
        crate::timer::js_set_immediate_callback_args(raw, args.as_ptr(), args.len() as i32);
    }
}

// ---------------------------------------------------------------------------
// lookup / lookupService (callback form)
// ---------------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn js_dns_lookup(args: i64) -> f64 {
    let argv = unsafe { read_args(args) };
    // Locate the trailing callback.
    let cb = match argv.last().copied().filter(|v| is_callable(*v)) {
        Some(c) => c,
        // Node throws ERR_INVALID_ARG_TYPE when no callback is given. We don't
        // have a clean throw path from a varargs native here; return undefined.
        None => return undefined(),
    };

    let host = argv
        .first()
        .copied()
        .and_then(read_js_string)
        .unwrap_or_default();

    // options is the middle arg (index 1) when there are 3+ args.
    let opts_val = if argv.len() >= 3 {
        argv[1]
    } else {
        undefined()
    };
    let opts = match unsafe { parse_lookup_options(opts_val) } {
        Ok(o) => o,
        Err(()) => {
            let err = dns_error_object("ERR_INVALID_ARG_VALUE", "lookup", &host);
            schedule_callback(cb, &[err]);
            return undefined();
        }
    };

    match resolve_addresses(&host, &opts) {
        Ok(addrs) if !addrs.is_empty() => {
            if opts.all {
                schedule_callback(cb, &[null(), lookup_all_array(&addrs)]);
            } else {
                let (addr, fam) = &addrs[0];
                schedule_callback(cb, &[null(), str_value(addr), *fam as f64]);
            }
        }
        Ok(_) | Err(_) => {
            let err = dns_error_object("ENOTFOUND", "getaddrinfo", &host);
            schedule_callback(cb, &[err]);
        }
    }
    undefined()
}

/// Reverse-resolve `(address, port)` to `{ hostname, service }` using
/// `getnameinfo`.
fn lookup_service(address: &str, port: u16) -> Result<(String, String), String> {
    let ip: IpAddr = address.parse().map_err(|_| "invalid address".to_string())?;
    let sa = SocketAddr::new(ip, port);
    unsafe { getnameinfo(&sa) }
}

#[cfg(unix)]
unsafe fn getnameinfo(sa: &SocketAddr) -> Result<(String, String), String> {
    let mut host_buf = [0i8; libc::NI_MAXHOST as usize];
    let mut serv_buf = [0i8; libc::NI_MAXSERV as usize];

    let (sa_ptr, sa_len): (*const libc::sockaddr, libc::socklen_t) = match sa {
        SocketAddr::V4(v4) => {
            let raw = Box::new(libc::sockaddr_in {
                #[cfg(any(target_os = "macos", target_os = "ios"))]
                sin_len: std::mem::size_of::<libc::sockaddr_in>() as u8,
                sin_family: libc::AF_INET as libc::sa_family_t,
                sin_port: v4.port().to_be(),
                sin_addr: libc::in_addr {
                    s_addr: u32::from_ne_bytes(v4.ip().octets()),
                },
                sin_zero: [0; 8],
            });
            let ptr = Box::into_raw(raw);
            (
                ptr as *const libc::sockaddr,
                std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
            )
        }
        SocketAddr::V6(v6) => {
            let raw = Box::new(libc::sockaddr_in6 {
                #[cfg(any(target_os = "macos", target_os = "ios"))]
                sin6_len: std::mem::size_of::<libc::sockaddr_in6>() as u8,
                sin6_family: libc::AF_INET6 as libc::sa_family_t,
                sin6_port: v6.port().to_be(),
                sin6_flowinfo: v6.flowinfo(),
                sin6_addr: libc::in6_addr {
                    s6_addr: v6.ip().octets(),
                },
                sin6_scope_id: v6.scope_id(),
            });
            let ptr = Box::into_raw(raw);
            (
                ptr as *const libc::sockaddr,
                std::mem::size_of::<libc::sockaddr_in6>() as libc::socklen_t,
            )
        }
    };

    let rc = libc::getnameinfo(
        sa_ptr,
        sa_len,
        host_buf.as_mut_ptr(),
        host_buf.len() as libc::socklen_t,
        serv_buf.as_mut_ptr(),
        serv_buf.len() as libc::socklen_t,
        0,
    );

    // Free the boxed sockaddr we leaked above.
    match sa {
        SocketAddr::V4(_) => drop(Box::from_raw(sa_ptr as *mut libc::sockaddr_in)),
        SocketAddr::V6(_) => drop(Box::from_raw(sa_ptr as *mut libc::sockaddr_in6)),
    }

    if rc != 0 {
        return Err(format!("getnameinfo failed: {rc}"));
    }
    let hostname = CStr::from_ptr(host_buf.as_ptr())
        .to_string_lossy()
        .into_owned();
    let service = CStr::from_ptr(serv_buf.as_ptr())
        .to_string_lossy()
        .into_owned();
    Ok((hostname, service))
}

#[cfg(not(unix))]
unsafe fn getnameinfo(sa: &SocketAddr) -> Result<(String, String), String> {
    // Non-Unix fallback: deterministic loopback + numeric service.
    let hostname = if sa.ip().is_loopback() {
        "localhost".to_string()
    } else {
        sa.ip().to_string()
    };
    Ok((hostname, sa.port().to_string()))
}

#[no_mangle]
pub extern "C" fn js_dns_lookup_service(args: i64) -> f64 {
    let argv = unsafe { read_args(args) };
    let cb = match argv.last().copied().filter(|v| is_callable(*v)) {
        Some(c) => c,
        None => return undefined(),
    };
    let address = argv
        .first()
        .copied()
        .and_then(read_js_string)
        .unwrap_or_default();
    let port = argv.get(1).copied().and_then(read_js_number).unwrap_or(0.0);

    if !(0.0..=65535.0).contains(&port) || port.fract() != 0.0 {
        let err = dns_error_object("ERR_SOCKET_BAD_PORT", "getnameinfo", &address);
        schedule_callback(cb, &[err]);
        return undefined();
    }

    match lookup_service(&address, port as u16) {
        Ok((hostname, service)) => {
            schedule_callback(cb, &[null(), str_value(&hostname), str_value(&service)]);
        }
        Err(_) => {
            let err = dns_error_object("ENOTFOUND", "getnameinfo", &address);
            schedule_callback(cb, &[err]);
        }
    }
    undefined()
}

// ---------------------------------------------------------------------------
// promises form
// ---------------------------------------------------------------------------

fn resolved_promise(value: f64) -> f64 {
    let promise = crate::promise::js_promise_resolved(value);
    js_nanbox_pointer(promise as i64)
}

fn rejected_promise(reason: f64) -> f64 {
    let promise = crate::promise::js_promise_rejected(reason);
    js_nanbox_pointer(promise as i64)
}

#[no_mangle]
pub extern "C" fn js_dns_promises_lookup(args: i64) -> f64 {
    let argv = unsafe { read_args(args) };
    let host = argv
        .first()
        .copied()
        .and_then(read_js_string)
        .unwrap_or_default();
    let opts_val = argv.get(1).copied().unwrap_or_else(undefined);
    let opts = match unsafe { parse_lookup_options(opts_val) } {
        Ok(o) => o,
        Err(()) => {
            return rejected_promise(dns_error_object("ERR_INVALID_ARG_VALUE", "lookup", &host))
        }
    };

    match resolve_addresses(&host, &opts) {
        Ok(addrs) if !addrs.is_empty() => {
            if opts.all {
                resolved_promise(lookup_all_array(&addrs))
            } else {
                let (addr, fam) = &addrs[0];
                resolved_promise(lookup_result_object(addr, *fam))
            }
        }
        Ok(_) | Err(_) => rejected_promise(dns_error_object("ENOTFOUND", "getaddrinfo", &host)),
    }
}

#[no_mangle]
pub extern "C" fn js_dns_promises_lookup_service(args: i64) -> f64 {
    let argv = unsafe { read_args(args) };
    let address = argv
        .first()
        .copied()
        .and_then(read_js_string)
        .unwrap_or_default();
    let port = argv.get(1).copied().and_then(read_js_number).unwrap_or(0.0);

    if !(0.0..=65535.0).contains(&port) || port.fract() != 0.0 {
        return rejected_promise(dns_error_object(
            "ERR_SOCKET_BAD_PORT",
            "getnameinfo",
            &address,
        ));
    }

    match lookup_service(&address, port as u16) {
        Ok((hostname, service)) => {
            let obj = js_object_alloc(0, 2);
            js_object_set_field_by_name(obj, key("hostname"), str_value(&hostname));
            js_object_set_field_by_name(obj, key("service"), str_value(&service));
            resolved_promise(boxed_pointer(obj as *const u8))
        }
        Err(_) => rejected_promise(dns_error_object("ENOTFOUND", "getnameinfo", &address)),
    }
}

// ---------------------------------------------------------------------------
// remaining shape stubs
// ---------------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn js_dns_noop(_args: i64) -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

#[no_mangle]
pub extern "C" fn js_dns_promises_noop(_args: i64) -> f64 {
    let promise = crate::promise::js_promise_resolved(f64::from_bits(TAG_UNDEFINED));
    js_nanbox_pointer(promise as i64)
}

#[no_mangle]
pub extern "C" fn js_dns_get_servers(_args: i64) -> f64 {
    empty_array_value()
}

#[no_mangle]
pub extern "C" fn js_dns_set_servers(_args: i64) -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

#[no_mangle]
pub extern "C" fn js_dns_promises_get_servers(_args: i64) -> f64 {
    empty_array_value()
}

#[no_mangle]
pub extern "C" fn js_dns_promises_set_servers(_args: i64) -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

#[no_mangle]
pub extern "C" fn js_dns_set_default_result_order(_args: i64) -> f64 {
    DEFAULT_RESULT_ORDER.store(RESULT_ORDER_IPV4_FIRST, Ordering::Relaxed);
    f64::from_bits(TAG_UNDEFINED)
}

#[no_mangle]
pub extern "C" fn js_dns_get_default_result_order(_args: i64) -> f64 {
    let order = if DEFAULT_RESULT_ORDER.load(Ordering::Relaxed) == RESULT_ORDER_IPV4_FIRST {
        "ipv4first"
    } else {
        "verbatim"
    };
    str_value(order)
}

#[no_mangle]
pub extern "C" fn js_dns_resolver_new(_args: i64) -> f64 {
    boxed_pointer(resolver_object(true) as *const u8)
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolver_new(_args: i64) -> f64 {
    boxed_pointer(resolver_object(false) as *const u8)
}

#[no_mangle]
pub extern "C" fn js_dns_resolver_get_servers(_handle: i64, _args: i64) -> f64 {
    empty_array_value()
}

#[no_mangle]
pub extern "C" fn js_dns_resolver_noop(_handle: i64, _args: i64) -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

// Keepalive anchors: these `#[no_mangle]` fns are emitted only from generated
// `.o` via the native-module dispatch table, so the auto-optimize whole-program
// LLVM bitcode rebuild would otherwise dead-strip them (see
// project_auto_optimize_keepalive_3320). `#[used]` survives that pipeline.
#[cfg(not(test))]
#[used]
static KEEP_DNS_LOOKUP: extern "C" fn(i64) -> f64 = js_dns_lookup;
#[cfg(not(test))]
#[used]
static KEEP_DNS_LOOKUP_SERVICE: extern "C" fn(i64) -> f64 = js_dns_lookup_service;
#[cfg(not(test))]
#[used]
static KEEP_DNS_PROMISES_LOOKUP_SERVICE: extern "C" fn(i64) -> f64 = js_dns_promises_lookup_service;
