//! `options.lookup`: a caller-supplied resolver for `Socket#connect`. Its
//! answer is reported through `'lookup'` events and becomes the transport's
//! address plan, as in Node's `lookupAndConnect`.

use super::payload_events as events;
use super::payload_socket::{self as socket, error};
use super::payload_transport::{self as p, undefined};
use perry_ffi::turnloop_net as tl;
use perry_ffi::{JsThis, JsValue, RawClosureHeader, TransientRootScope, TransientRootedNanbox};
use std::net::{IpAddr, SocketAddr, SocketAddrV6};

extern "C" {
    fn js_runtime_describe_received(value: f64) -> f64;
    fn js_jsvalue_to_string(value: f64) -> i64;
    fn js_is_truthy(value: f64) -> i32;
    fn js_queue_next_tick(callback: i64);
    fn js_array_is_array(value: f64) -> f64;
    fn js_aggregateerror_new(
        errors: *mut perry_ffi::ArrayHeader,
        message: *mut perry_ffi::StringHeader,
    ) -> *mut u8;
}

/// Whether `options.lookup` is set; a value that is not a function throws.
pub(crate) fn validate(lookup: f64) -> bool {
    let value = JsValue::from_bits(lookup.to_bits());
    if value.is_undefined() || value.is_null() {
        return false;
    }
    if socket::is_callback(lookup) {
        return true;
    }
    // SAFETY: The describer reads a live JS value on this agent and returns a string value.
    let received = unsafe { display(js_runtime_describe_received(lookup)) };
    perry_ffi::throw_with_code(
        &format!("The \"options.lookup\" property must be of type function. Received {received}"),
        "ERR_INVALID_ARG_TYPE",
        perry_ffi::ErrorKind::TypeError,
    )
}

/// What `connect` asks a resolver for, as `dnsopts.all` decides in Node:
/// every address, raced with `autoSelectFamilyAttemptTimeout` milliseconds per
/// attempt, or the first of one family (0 for any).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Wanted {
    All(u32),
    One(u8),
}

impl Default for Wanted {
    fn default() -> Self {
        Wanted::All(crate::ip::auto_select_family_attempt_timeout_default())
    }
}

pub(crate) fn wanted(options: f64) -> Wanted {
    let family = family_number(dns_family(options));
    let auto_select = JsValue::from_bits(field(options, "autoSelectFamily").to_bits());
    let auto_select = if auto_select.is_bool() {
        auto_select.to_bool()
    } else {
        crate::ip::auto_select_family_default()
    };
    match family {
        None if auto_select => Wanted::All(attempt_timeout(options)),
        family => Wanted::One(family.unwrap_or(0)),
    }
}

/// `options.autoSelectFamilyAttemptTimeout`, which Node raises to 10 ms.
fn attempt_timeout(options: f64) -> u32 {
    let value = JsValue::from_bits(field(options, "autoSelectFamilyAttemptTimeout").to_bits());
    if value.is_number() && value.to_number() >= 1.0 {
        (value.to_number() as u32).max(10)
    } else {
        crate::ip::auto_select_family_attempt_timeout_default()
    }
}

/// Node's attempt order: each address once, families alternating from the
/// first address's.
#[derive(Default)]
struct Destinations {
    groups: [Vec<SocketAddr>; 2],
    first_is_v6: Option<bool>,
}

impl Destinations {
    fn push(&mut self, endpoint: SocketAddr, family: u8) {
        let is_v6 = family == 6;
        let group = &mut self.groups[usize::from(is_v6 != *self.first_is_v6.get_or_insert(is_v6))];
        if !group.contains(&endpoint) {
            group.push(endpoint);
        }
    }

    fn is_empty(&self) -> bool {
        self.groups.iter().all(Vec::is_empty)
    }

    fn order(self) -> Vec<SocketAddr> {
        let [preferred, other] = self.groups;
        let mut order = Vec::with_capacity(preferred.len() + other.len());
        for index in 0..preferred.len().max(other.len()) {
            order.extend(preferred.get(index));
            order.extend(other.get(index));
        }
        order
    }
}

/// `options.family` as Node's `socketToDnsFamily` maps it.
fn dns_family(options: f64) -> f64 {
    let family = field(options, "family");
    match string(family).as_deref() {
        Some("IPv4") => 4.0,
        Some("IPv6") => 6.0,
        _ => family,
    }
}

/// Report the runtime resolver's answer for `host` as Node's `'lookup'`
/// events, before the transport tries the first address.
pub(crate) fn resolved(owner: f64, addresses: &[u8]) {
    let Ok(link) = socket::transport_link(owner) else {
        return;
    };
    // SAFETY: The projection checks family and lifecycle; the copy ends before any JS.
    let Ok((host, port, wanted)) = (unsafe {
        p::socket_ptr(link).map(|payload| {
            let fields = &(*payload).ext;
            (fields.host.clone(), fields.port, fields.wanted)
        })
    }) else {
        return;
    };
    let ips: Vec<IpAddr> = String::from_utf8_lossy(addresses)
        .lines()
        .filter_map(|line| line.parse().ok())
        .collect();
    let family = |ip: &IpAddr| if ip.is_ipv4() { 4 } else { 6 };
    let reported: Vec<IpAddr> = match wanted {
        Wanted::All(_) => ips,
        Wanted::One(want) => ips
            .iter()
            .find(|ip| want == 0 || family(ip) == want)
            .or(ips.first())
            .copied()
            .into_iter()
            .collect(),
    };
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let host = scope.root_nanbox(events::string(&host));
    let null = f64::from_bits(JsValue::NULL.bits());
    let mut destinations = Destinations::default();
    for ip in reported {
        if !connecting(owner.get()) {
            return;
        }
        let address = scope.root_nanbox(events::string(&ip.to_string()));
        events::emit(
            owner.get(),
            "lookup",
            &[null, address.get(), f64::from(family(&ip)), host.get()],
        );
        destinations.push(SocketAddr::new(ip, port), family(&ip));
    }
    if connecting(owner.get()) && !destinations.is_empty() {
        connect(owner.get(), &destinations.order(), wanted);
    }
}

/// A failed resolve is reported to `'lookup'` listeners before `'error'`.
pub(crate) fn failed(owner: f64, error: f64) {
    let Ok(link) = socket::transport_link(owner) else {
        return;
    };
    // SAFETY: The projection checks family and lifecycle; the copy ends before any JS.
    let Ok(host) = (unsafe { p::socket_ptr(link).map(|payload| (*payload).ext.host.clone()) })
    else {
        return;
    };
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let error = scope.root_nanbox(error);
    let host = scope.root_nanbox(events::string(&host));
    events::emit(
        owner.get(),
        "lookup",
        &[error.get(), undefined(), undefined(), host.get()],
    );
}

/// The options a client that dials for `options` (a `tls.connect` or
/// `http.request` options object) passes to `Socket#connect`: its endpoint and
/// the resolution options, as Node passes them through.
pub(crate) fn connect_options(options: Option<f64>, port: u16, host: &str) -> f64 {
    let scope = TransientRootScope::enter();
    let options = options.map(|options| scope.root_nanbox(options));
    let connect = scope.root_nanbox(f64::from_bits(perry_ffi::alloc_object().bits()));
    p::own_set(connect.get(), "port", f64::from(port));
    let host = scope.root_nanbox(events::string(host));
    p::own_set(connect.get(), "host", host.get());
    if let Some(options) = options {
        copy_resolution(options.get(), connect.get());
    }
    connect.get()
}

/// A copy of `options`' resolution options, which later changes to the
/// caller's object do not reach.
pub(crate) fn resolution_options(options: f64) -> f64 {
    let scope = TransientRootScope::enter();
    let options = scope.root_nanbox(options);
    let copy = scope.root_nanbox(f64::from_bits(perry_ffi::alloc_object().bits()));
    copy_resolution(options.get(), copy.get());
    copy.get()
}

fn copy_resolution(from: f64, into: f64) {
    let scope = TransientRootScope::enter();
    let from = scope.root_nanbox(from);
    let into = scope.root_nanbox(into);
    for key in [
        "lookup",
        "family",
        "hints",
        "autoSelectFamily",
        "autoSelectFamilyAttemptTimeout",
    ] {
        let value = scope.root_nanbox(field(from.get(), key));
        if !JsValue::from_bits(value.get().to_bits()).is_undefined() {
            p::own_set(into.get(), key, value.get());
        }
    }
}

/// Call `lookup(host, dnsopts, callback)`. The callback may run before this
/// returns, so the caller holds no payload borrow across it.
pub(crate) fn start(owner: f64, options: f64, host: &str, port: u16, lookup: f64) {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let options = scope.root_nanbox(options);
    let lookup = scope.root_nanbox(lookup);
    let family = scope.root_nanbox(dns_family(options.get()));
    let hints = scope.root_nanbox(field(options.get(), "hints"));
    let wanted = wanted(options.get());
    let all = matches!(wanted, Wanted::All(_));
    let dns_options = scope.root_nanbox(f64::from_bits(perry_ffi::alloc_object().bits()));
    p::own_set(dns_options.get(), "family", family.get());
    // SAFETY: Truthiness reads a rooted JS value on this agent.
    let hints = if unsafe { js_is_truthy(hints.get()) } != 0 {
        hints.get()
    } else if cfg!(windows) || family_number(family.get()).is_some() {
        0.0
    } else {
        ADDRCONFIG
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
        let timeout = match wanted {
            Wanted::All(timeout) => f64::from(timeout),
            Wanted::One(_) => -1.0,
        };
        perry_ffi::set_closure_capture_f64(callback_ptr, 3, timeout);
    }
    let state = scope.root_nanbox(socket::state(owner.get()));
    p::record_set(state.get(), ATTEMPT, p::boxed_addr(callback.get()));
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
    let timeout = perry_ffi::closure_capture_f64(closure, 3);
    let wanted = if timeout >= 0.0 {
        Wanted::All(timeout as u32)
    } else {
        Wanted::One(0)
    };
    let attempt = Attempt {
        owner,
        token: scope.root_nanbox(p::boxed_addr(closure as i64)),
    };
    let err = scope.root_nanbox(err);
    let address = scope.root_nanbox(address);
    let family = scope.root_nanbox(family);
    let host_text = string(host.get()).unwrap_or_default();
    if let Wanted::All(_) = wanted {
        answer_all(
            &scope,
            &attempt,
            &host,
            &host_text,
            port,
            wanted,
            &err,
            address.get(),
        );
    } else {
        events::emit(
            attempt.owner.get(),
            "lookup",
            &[err.get(), address.get(), family.get(), host.get()],
        );
        if !attempt.current() {
            return undefined();
        }
        if js_is_truthy(err.get()) != 0 {
            attempt.fail(err.get());
        } else if let Some(problem) = invalid(address.get(), family.get(), &host_text, port) {
            attempt.fail(problem);
        } else if let Some(endpoint) = endpoint(address.get(), port) {
            connect(attempt.owner.get(), &[endpoint], wanted);
        }
    }
    undefined()
}

/// The `connect` a lookup answers, named by its answer callback, which the
/// socket state holds until the socket reopens. A late answer or error for an
/// earlier connect must not touch a later one.
struct Attempt {
    owner: TransientRootedNanbox,
    token: TransientRootedNanbox,
}

impl Attempt {
    fn current(&self) -> bool {
        is_current(self.owner.get(), self.token.get())
    }

    /// The error waits a tick: `net.connect()` callers attach `'error'` after it returns.
    fn fail(&self, err: f64) {
        let scope = TransientRootScope::enter();
        let err = scope.root_nanbox(err);
        let job = scope.root_addr(perry_ffi::alloc_closure(
            perry_ffi::js_function_info!(fail_tick, 0; with_flags(perry_ffi::FN_BUILTIN)),
            3,
        ) as i64);
        // SAFETY: The rooted job was allocated above with three capture slots, which trace the
        // owner and error until it runs.
        unsafe {
            let job_ptr = job.get() as *mut RawClosureHeader;
            perry_ffi::set_closure_capture_f64(job_ptr, 0, self.owner.get());
            perry_ffi::set_closure_capture_f64(job_ptr, 1, err.get());
            perry_ffi::set_closure_capture_f64(job_ptr, 2, self.token.get());
            js_queue_next_tick(job.get());
        }
    }
}

/// `all: true` answers with `{ address, family }` entries. Valid addresses
/// are tried once each, alternating families from the first one's.
fn answer_all(
    scope: &TransientRootScope,
    attempt: &Attempt,
    host: &TransientRootedNanbox,
    host_text: &str,
    port: u16,
    wanted: Wanted,
    err: &TransientRootedNanbox,
    addresses: f64,
) {
    if !attempt.current() {
        return;
    }
    // SAFETY: Truthiness reads a rooted JS value on this agent.
    if unsafe { js_is_truthy(err.get()) } != 0 {
        events::emit(
            attempt.owner.get(),
            "lookup",
            &[err.get(), undefined(), undefined(), host.get()],
        );
        attempt.fail(err.get());
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
    let mut destinations = Destinations::default();
    let mut index = 0;
    while index < length() {
        let (address, family) = entry(index);
        events::emit(
            attempt.owner.get(),
            "lookup",
            &[err.get(), address.get(), family.get(), host.get()],
        );
        if !attempt.current() {
            return;
        }
        if let (Some(endpoint), Some(family)) =
            (endpoint(address.get(), port), family_number(family.get()))
        {
            destinations.push(endpoint, family);
        }
        index += 1;
    }
    if destinations.is_empty() {
        if length() == 0 {
            // Node destructures `addresses[0]`, which throws to whoever called back.
            perry_ffi::throw_with_code(
                &empty_answer_message(addresses.get()),
                "",
                perry_ffi::ErrorKind::TypeError,
            );
        }
        let (address, family) = entry(0);
        if let Some(problem) = invalid(address.get(), family.get(), host_text, port) {
            attempt.fail(problem);
        }
        return;
    }
    connect(attempt.owner.get(), &destinations.order(), wanted);
}

/// Node's rejection of a single answer, checked address first.
fn invalid(address: f64, family: f64, host: &str, port: u16) -> Option<f64> {
    if endpoint(address, port).is_none() {
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

/// Start the transport on `endpoints`. One endpoint is Node's
/// `internalConnect`, whose attempt the binding reports; several race as
/// `internalConnectMultiple`, whose attempts the runtime reports.
fn connect(owner: f64, endpoints: &[SocketAddr], wanted: Wanted) {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let timeout = match (wanted, endpoints) {
        (Wanted::All(timeout), [_, _, ..]) => timeout,
        _ => 0,
    };
    if timeout == 0 {
        let Some(first) = endpoints.first() else {
            return;
        };
        let address = scope.root_nanbox(events::string(&first.ip().to_string()));
        events::emit(
            owner.get(),
            "connectionAttempt",
            &[address.get(), f64::from(first.port()), ip_family(first)],
        );
        if !connecting(owner.get()) {
            return;
        }
    } else {
        // SAFETY: Allocation runs on this agent; each array is installed before the next.
        let attempted = scope.root_addr(unsafe { perry_ffi::js_array_alloc(0) } as i64);
        p::own_set(owner.get(), ATTEMPTED, p::boxed_addr(attempted.get()));
        let errors = scope.root_addr(unsafe { perry_ffi::js_array_alloc(0) } as i64);
        let state = scope.root_nanbox(socket::state(owner.get()));
        p::record_set(state.get(), ERRORS, p::boxed_addr(errors.get()));
    }
    let Ok(link) = socket::transport_link(owner.get()) else {
        return;
    };
    let _account = p::AccountSocket(link);
    let endpoints = if timeout == 0 {
        &endpoints[..1]
    } else {
        endpoints
    };
    // SAFETY: The retained link belongs to this agent; the core access ends before the runtime
    // reports any attempt.
    let result = unsafe {
        p::socket_core(link)
            .map(|core| tl::link_tcp_connect_addrs(&mut *core, link, endpoints, true, timeout))
    };
    if let Ok(Err(failure)) = result {
        socket::destroy(owner.get(), error(&failure.code, &failure.message()));
    }
}

/// The socket's own `autoSelectFamilyAttemptedAddresses`, which Node sets
/// when attempts race.
const ATTEMPTED: &str = "autoSelectFamilyAttemptedAddresses";
/// The socket state key holding a racing connect's attempt errors.
pub(crate) const ERRORS: &str = "attemptErrors";

fn ip_family(endpoint: &SocketAddr) -> f64 {
    if endpoint.is_ipv4() {
        4.0
    } else {
        6.0
    }
}

fn push(array_holder: f64, key: &str, record: bool, value: f64) {
    let scope = TransientRootScope::enter();
    let holder = scope.root_nanbox(array_holder);
    let value = scope.root_nanbox(value);
    let array = if record {
        p::record_get(holder.get(), key)
    } else {
        p::own_get(holder.get(), key)
    };
    let array = JsValue::from_bits(array.to_bits());
    if !array.is_pointer() {
        return;
    }
    // SAFETY: The array was installed by `connect` as an ordinary array; the push may move it,
    // and the moved array is installed again at once.
    let grown = unsafe {
        perry_ffi::js_array_push(
            array.as_pointer(),
            JsValue::from_bits(value.get().to_bits()),
        )
    };
    let grown = p::boxed_addr(grown as i64);
    if record {
        p::record_set(holder.get(), key, grown);
    } else {
        p::own_set(holder.get(), key, grown);
    }
}

/// A racing connect's attempt: `'connectionAttempt'`, `'connectionAttemptFailed'`
/// or `'connectionAttemptTimeout'`, with each failure kept for the final
/// `AggregateError`.
pub(crate) fn attempted(owner: f64, event: &tl::NetCompletion) {
    // SAFETY: The runtime keeps the completion and its spans valid until this sink returns.
    let ip = String::from_utf8_lossy(unsafe { event.bytes() }).into_owned();
    let port = event.conn as u16;
    let family = if ip.contains(':') { 6.0 } else { 4.0 };
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let address = scope.root_nanbox(events::string(&ip));
    let port_value = f64::from(port);
    if event.user == tl::NET_ATTEMPT_START {
        let attempted = scope.root_nanbox(events::string(&format!("{ip}:{port}")));
        push(owner.get(), ATTEMPTED, false, attempted.get());
        events::emit(
            owner.get(),
            "connectionAttempt",
            &[address.get(), port_value, family],
        );
        return;
    }
    let timed_out = event.user == tl::NET_ATTEMPT_TIMEOUT;
    // SAFETY: as above.
    let (syscall, code) = unsafe {
        if timed_out {
            ("connect", "ETIMEDOUT")
        } else {
            (
                event.syscall().unwrap_or("connect"),
                event.code().unwrap_or("EIO"),
            )
        }
    };
    let target = tl::Endpoint {
        address: ip.clone(),
        port,
        family: family as i32,
    };
    let error = scope.root_nanbox(super::payload_io::socket_error_value(
        syscall,
        code,
        Some(&target),
    ));
    let state = scope.root_nanbox(socket::state(owner.get()));
    push(state.get(), ERRORS, true, error.get());
    if timed_out {
        events::emit(
            owner.get(),
            "connectionAttemptTimeout",
            &[address.get(), port_value, family],
        );
    } else {
        events::emit(
            owner.get(),
            "connectionAttemptFailed",
            &[address.get(), port_value, family, error.get()],
        );
    }
}

/// A racing connect destroyed mid-attempt reports that attempt as failed
/// with `ECANCELED`, as Node's cancelled connect request does.
pub(crate) fn cancel_attempt(owner: f64, link: perry_ffi::native_payload::OwnerLink) {
    let errors = p::record_get(socket::state(owner), ERRORS);
    // SAFETY: The projection checks family and lifecycle; the reads end before any allocation.
    let target = unsafe {
        let racing = JsValue::from_bits(errors.to_bits()).is_pointer()
            && p::socket_ptr(link).is_ok_and(|payload| (*payload).ext.connecting);
        if !racing {
            return;
        }
        p::socket_core(link)
            .ok()
            .and_then(|core| tl::link_peer_address(&mut *core, link))
    };
    let Some(target) = target.filter(|target| !target.address.is_empty()) else {
        return;
    };
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let error = scope.root_nanbox(super::payload_io::socket_error_value(
        "connect",
        "ECANCELED",
        Some(&target),
    ));
    let address = scope.root_nanbox(events::string(&target.address));
    events::queue_emit(
        owner.get(),
        "connectionAttemptFailed",
        &[
            address.get(),
            f64::from(target.port),
            f64::from(target.family),
            error.get(),
        ],
    );
}

/// The error a failed connect destroys the socket with: Node's
/// `AggregateError` of every attempt's error when attempts raced, else the
/// attempt's own error after `'connectionAttemptFailed'`.
pub(crate) fn connect_error(owner: f64, error: f64, target: Option<&tl::Endpoint>) -> f64 {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let error = scope.root_nanbox(error);
    let state = scope.root_nanbox(socket::state(owner.get()));
    let errors = scope.root_nanbox(p::record_get(state.get(), ERRORS));
    let errors_value = JsValue::from_bits(errors.get().to_bits());
    if !errors_value.is_pointer() {
        if let Some(target) = target {
            let address = scope.root_nanbox(events::string(&target.address));
            events::emit(
                owner.get(),
                "connectionAttemptFailed",
                &[
                    address.get(),
                    f64::from(target.port),
                    f64::from(target.family),
                    error.get(),
                ],
            );
        }
        return error.get();
    }
    // SAFETY: The errors array is a live array on this agent; the read cannot collect.
    let first = scope.root_nanbox(f64::from_bits(
        unsafe { perry_ffi::js_array_get(errors_value.as_pointer(), 0) }.bits(),
    ));
    let message = scope.root_addr(perry_ffi::alloc_string("").as_raw() as i64);
    // SAFETY: Both operands are rooted; the constructor roots the array across its allocation.
    let aggregate = scope.root_addr(unsafe {
        js_aggregateerror_new(
            errors_value.as_pointer(),
            message.get() as *mut perry_ffi::StringHeader,
        )
    } as i64);
    let aggregate = scope.root_nanbox(p::boxed_addr(aggregate.get()));
    let code = scope.root_nanbox(p::own_get(first.get(), "code"));
    p::own_set(aggregate.get(), "code", code.get());
    // The race is over: no attempt is left for a destroy to cancel.
    p::record_set(state.get(), ERRORS, undefined());
    aggregate.get()
}

/// The socket state key naming the pending lookup's answer callback.
pub(crate) const ATTEMPT: &str = "lookupAttempt";

/// A lookup answered after `destroy()`, or for an earlier connect, is ignored.
fn is_current(owner: f64, token: f64) -> bool {
    connecting(owner) && p::record_get(socket::state(owner), ATTEMPT).to_bits() == token.to_bits()
}

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
    let token = scope.root_nanbox(perry_ffi::closure_capture_f64(closure, 2));
    if is_current(owner.get(), token.get()) {
        socket::destroy(owner.get(), err.get());
    }
    undefined()
}

/// V8's message for destructuring `addresses[0]` of an empty answer, or
/// reading `length` of a nullish one.
fn empty_answer_message(addresses: f64) -> String {
    let value = JsValue::from_bits(addresses.to_bits());
    if value.is_undefined() || value.is_null() {
        format!(
            "Cannot read properties of {} (reading 'length')",
            display(addresses)
        )
    } else {
        "Cannot destructure property 'address' of 'addresses[0]' as it is undefined.".into()
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

/// An IP answer as a connect endpoint; an IPv6 zone becomes its scope id.
fn endpoint(address: f64, port: u16) -> Option<SocketAddr> {
    let text = string(address)?;
    if crate::ip::classify_ip(&text) == 0 {
        return None;
    }
    let (ip, zone) = text.split_once('%').unwrap_or((&text, ""));
    match ip.parse().ok()? {
        IpAddr::V6(v6) if !zone.is_empty() => {
            Some(SocketAddrV6::new(v6, port, 0, scope_id(zone)).into())
        }
        ip => Some(SocketAddr::new(ip, port)),
    }
}

/// An unknown interface name is scope 0, as in libuv's `uv_ip6_addr`.
fn scope_id(zone: &str) -> u32 {
    if let Ok(index) = zone.parse() {
        return index;
    }
    #[cfg(unix)]
    {
        let Ok(name) = std::ffi::CString::new(zone) else {
            return 0;
        };
        // SAFETY: `name` is a NUL-terminated string that outlives the call.
        unsafe { libc::if_nametoindex(name.as_ptr()) }
    }
    #[cfg(not(unix))]
    0
}

/// `dns.ADDRCONFIG`, which Node adds to zero hints off Windows.
#[cfg(unix)]
const ADDRCONFIG: f64 = libc::AI_ADDRCONFIG as f64;
#[cfg(not(unix))]
const ADDRCONFIG: f64 = 0x20 as f64;

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

#[cfg(test)]
mod tests {
    #[test]
    fn a_zone_is_an_index_or_an_interface_name() {
        assert_eq!(super::scope_id("7"), 7);
        assert_eq!(super::scope_id("no-such-interface0"), 0);
        #[cfg(unix)]
        {
            let name = std::ffi::CString::new("lo0").unwrap();
            let lo0 = unsafe { libc::if_nametoindex(name.as_ptr()) };
            assert_eq!(super::scope_id("lo0"), lo0);
        }
    }
}
