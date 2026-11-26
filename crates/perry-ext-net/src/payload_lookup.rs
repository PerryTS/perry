//! `options.lookup`: a caller-supplied resolver for `Socket#connect`. Its
//! answer is reported through `'lookup'` events and becomes the transport's
//! address plan, as in Node's `lookupAndConnect`.

use super::payload_events as events;
use super::payload_socket::{self as socket, error};
use super::payload_transport::{self as p, undefined};
use perry_ffi::turnloop_net as tl;
use perry_ffi::{JsThis, JsValue, RawClosureHeader, TransientRootScope};
use std::net::IpAddr;

extern "C" {
    fn js_runtime_describe_received(value: f64) -> f64;
    fn js_jsvalue_to_string(value: f64) -> i64;
    fn js_is_truthy(value: f64) -> i32;
    fn js_queue_next_tick(callback: i64);
    fn js_array_is_array(value: f64) -> f64;
}

/// The callable `options.lookup`, or `None` when it is absent.
pub(crate) fn validate(lookup: f64) -> Option<f64> {
    let value = JsValue::from_bits(lookup.to_bits());
    if value.is_undefined() || value.is_null() {
        return None;
    }
    if socket::is_callback(lookup) {
        return Some(lookup);
    }
    // SAFETY: The describer reads a live JS value on this agent and returns a string value.
    let received = unsafe { display(js_runtime_describe_received(lookup)) };
    perry_ffi::throw_with_code(
        &format!("The \"options.lookup\" property must be of type function. Received {received}"),
        "ERR_INVALID_ARG_TYPE",
        perry_ffi::ErrorKind::TypeError,
    )
}

/// Call `lookup(host, dnsopts, callback)`. The callback may run before this
/// returns, so the caller holds no payload borrow across it.
pub(crate) fn start(owner: f64, options: f64, host: &str, port: u16, lookup: f64) {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let options = scope.root_nanbox(options);
    let lookup = scope.root_nanbox(lookup);
    let family = scope.root_nanbox(field(options.get(), "family"));
    let family = scope.root_nanbox(match string(family.get()).as_deref() {
        Some("IPv4") => 4.0,
        Some("IPv6") => 6.0,
        _ => family.get(),
    });
    let hints = scope.root_nanbox(field(options.get(), "hints"));
    let auto_select = JsValue::from_bits(field(options.get(), "autoSelectFamily").to_bits());
    let all = family_number(family.get()).is_none()
        && if auto_select.is_bool() {
            auto_select.to_bool()
        } else {
            crate::ip::auto_select_family_default()
        };
    let dns_options = scope.root_nanbox(f64::from_bits(perry_ffi::alloc_object().bits()));
    p::own_set(dns_options.get(), "family", family.get());
    // shortcut: Node adds dns.ADDRCONFIG to zero hints off Windows; add it once this crate can read the platform constant.
    // SAFETY: Truthiness reads a rooted JS value on this agent.
    let hints = if unsafe { js_is_truthy(hints.get()) } != 0 {
        hints.get()
    } else {
        0.0
    };
    p::own_set(dns_options.get(), "hints", hints);
    if all {
        p::own_set(dns_options.get(), "all", bool_value(true));
    }
    let host = scope.root_nanbox(events::string(host));
    let callback = scope.root_addr(perry_ffi::alloc_closure(
        perry_ffi::js_function_info!(answer, 3; with_flags(perry_ffi::FN_BUILTIN)),
        4,
    ) as i64);
    // SAFETY: The rooted closure was allocated above with four capture slots.
    unsafe {
        let callback_ptr = callback.get() as *mut RawClosureHeader;
        perry_ffi::set_closure_capture_f64(callback_ptr, 0, owner.get());
        perry_ffi::set_closure_capture_f64(callback_ptr, 1, host.get());
        perry_ffi::set_closure_capture_f64(callback_ptr, 2, f64::from(port));
        perry_ffi::set_closure_capture_f64(callback_ptr, 3, if all { 1.0 } else { 0.0 });
    }
    events::call(
        lookup.get(),
        JsThis::UNDEFINED,
        &[host.get(), dns_options.get(), p::boxed_addr(callback.get())],
    );
}

unsafe extern "C" fn answer(
    closure: *const RawClosureHeader,
    _: JsThis,
    err: f64,
    address: f64,
    family: f64,
) -> f64 {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(perry_ffi::closure_capture_f64(closure, 0));
    let host = scope.root_nanbox(perry_ffi::closure_capture_f64(closure, 1));
    let port = perry_ffi::closure_capture_f64(closure, 2) as u16;
    let all = perry_ffi::closure_capture_f64(closure, 3) == 1.0;
    let err = scope.root_nanbox(err);
    let address = scope.root_nanbox(address);
    let family = scope.root_nanbox(family);
    let host_text = string(host.get()).unwrap_or_default();
    if all {
        answer_all(
            &scope,
            owner.get(),
            host.get(),
            &host_text,
            port,
            err.get(),
            address.get(),
        );
    } else {
        events::emit(
            owner.get(),
            "lookup",
            &[err.get(), address.get(), family.get(), host.get()],
        );
        if !connecting(owner.get()) {
            return undefined();
        }
        if js_is_truthy(err.get()) != 0 {
            fail(owner.get(), err.get());
        } else if let Some(problem) = invalid(address.get(), family.get(), &host_text, port) {
            fail(owner.get(), problem);
        } else if let Some(ip) = ip(address.get()) {
            connect(owner.get(), &[ip], port);
        }
    }
    undefined()
}

/// `all: true` answers with `{ address, family }` entries. Valid addresses
/// are tried once each, alternating families from the first one's.
fn answer_all(
    scope: &TransientRootScope,
    owner: f64,
    host: f64,
    host_text: &str,
    port: u16,
    err: f64,
    addresses: f64,
) {
    if !connecting(owner) {
        return;
    }
    // SAFETY: Truthiness reads a rooted JS value on this agent.
    if unsafe { js_is_truthy(err) } != 0 {
        events::emit(owner, "lookup", &[err, undefined(), undefined(), host]);
        fail(owner, err);
        return;
    }
    let addresses = scope.root_nanbox(addresses);
    // SAFETY: The classifier reads a rooted JS value without allocating.
    let is_array =
        JsValue::from_bits(unsafe { js_array_is_array(addresses.get()) }.to_bits()).to_bool();
    // Each read reloads the array: an `address` getter may move or resize it.
    let length = || {
        if !is_array {
            return 0;
        }
        // SAFETY: The rooted value is an array on this agent; the read cannot collect.
        unsafe {
            perry_ffi::js_array_length(JsValue::from_bits(addresses.get().to_bits()).as_pointer())
        }
    };
    let entry = |index: u32| {
        let array = JsValue::from_bits(addresses.get().to_bits()).as_pointer();
        // SAFETY: `index` is below the length just read; the element is rooted at once.
        let entry = scope.root_nanbox(f64::from_bits(
            unsafe { perry_ffi::js_array_get(array, index) }.bits(),
        ));
        let address = scope.root_nanbox(field(entry.get(), "address"));
        let family = scope.root_nanbox(field(entry.get(), "family"));
        (address, family)
    };
    let mut groups: [Vec<IpAddr>; 2] = [Vec::new(), Vec::new()];
    let mut first_is_v6 = None;
    let mut index = 0;
    while index < length() {
        let (address, family) = entry(index);
        events::emit(owner, "lookup", &[err, address.get(), family.get(), host]);
        if !connecting(owner) {
            return;
        }
        if let (Some(ip), Some(family)) = (ip(address.get()), family_number(family.get())) {
            let is_v6 = family == 6;
            let group = &mut groups[usize::from(is_v6 != *first_is_v6.get_or_insert(is_v6))];
            if !group.contains(&ip) {
                group.push(ip);
            }
        }
        index += 1;
    }
    let [preferred, other] = groups;
    if preferred.is_empty() && other.is_empty() {
        if length() > 0 {
            let (address, family) = entry(0);
            if let Some(problem) = invalid(address.get(), family.get(), host_text, port) {
                fail(owner, problem);
            }
        }
        return;
    }
    let mut order = Vec::with_capacity(preferred.len() + other.len());
    for index in 0..preferred.len().max(other.len()) {
        order.extend(preferred.get(index));
        order.extend(other.get(index));
    }
    connect(owner, &order, port);
}

/// Node's rejection of a single answer, checked address first.
fn invalid(address: f64, family: f64, host: &str, port: u16) -> Option<f64> {
    if ip(address).is_none() {
        return Some(error(
            "ERR_INVALID_IP_ADDRESS",
            &format!("Invalid IP address: {}", display(address)),
        ));
    }
    family_number(family).is_none().then(|| {
        error(
            "ERR_INVALID_ADDRESS_FAMILY",
            &format!("Invalid address family: {} {host}:{port}", display(family)),
        )
    })
}

fn connect(owner: f64, ips: &[IpAddr], port: u16) {
    let Ok(link) = socket::transport_link(owner) else {
        return;
    };
    let _account = p::AccountSocket(link);
    // SAFETY: The retained link belongs to this agent; the core access ends before any JS.
    let result = unsafe {
        p::socket_core(link)
            .map(|core| tl::link_tcp_connect_addrs(&mut *core, link, ips, port, true))
    };
    if let Ok(Err(failure)) = result {
        socket::destroy(owner, error(&failure.code, &failure.message()));
    }
}

/// A lookup answered after `destroy()` is ignored.
fn connecting(owner: f64) -> bool {
    socket::transport_link(owner).is_ok_and(|link| {
        // SAFETY: The projection checks family and lifecycle; the field read ends before any JS.
        unsafe { p::socket_ptr(link).is_ok_and(|payload| (*payload).ext.connecting) }
    })
}

unsafe extern "C" fn fail_tick(closure: *const RawClosureHeader, _: JsThis) -> f64 {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(perry_ffi::closure_capture_f64(closure, 0));
    let err = scope.root_nanbox(perry_ffi::closure_capture_f64(closure, 1));
    socket::destroy(owner.get(), err.get());
    undefined()
}

/// The error waits a tick: `net.connect()` callers attach `'error'` after it returns.
fn fail(owner: f64, err: f64) {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let err = scope.root_nanbox(err);
    let job = scope.root_addr(perry_ffi::alloc_closure(
        perry_ffi::js_function_info!(fail_tick, 0; with_flags(perry_ffi::FN_BUILTIN)),
        2,
    ) as i64);
    // SAFETY: The rooted job was allocated above with two capture slots, which trace the owner
    // and error until it runs.
    unsafe {
        let job_ptr = job.get() as *mut RawClosureHeader;
        perry_ffi::set_closure_capture_f64(job_ptr, 0, owner.get());
        perry_ffi::set_closure_capture_f64(job_ptr, 1, err.get());
        js_queue_next_tick(job.get());
    }
}

fn field(object: f64, key: &str) -> f64 {
    // SAFETY: The getter runs on this agent; callers root the result before allocating.
    unsafe { crate::get_object_value_field(object, key) }.unwrap_or_else(undefined)
}

fn string(value: f64) -> Option<String> {
    let value = JsValue::from_bits(value.to_bits());
    value
        .is_any_string()
        .then(|| value.to_owned_string())
        .flatten()
}

fn ip(address: f64) -> Option<IpAddr> {
    let text = string(address)?;
    if crate::ip::classify_ip(&text) == 0 {
        return None;
    }
    // shortcut: a zone index is dropped, so a scoped link-local address connects unscoped; keep it once the transport takes a scope id.
    text.split('%').next()?.parse().ok()
}

fn family_number(family: f64) -> Option<u8> {
    let value = JsValue::from_bits(family.to_bits());
    if !value.is_number() {
        return None;
    }
    match value.to_number() {
        4.0 => Some(4),
        6.0 => Some(6),
        _ => None,
    }
}

fn display(value: f64) -> String {
    // SAFETY: ToString runs on this agent and returns a live string header.
    unsafe { crate::string_from_header_i64(js_jsvalue_to_string(value)) }.unwrap_or_default()
}

fn bool_value(value: bool) -> f64 {
    f64::from_bits(JsValue::from_bool(value).bits())
}
