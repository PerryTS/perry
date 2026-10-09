//! Link-routed Socket methods; every reentrant call ends its payload borrow.

use super::payload_transport::{self as p, SocketFields, SOCKET};
use super::{payload_events as events, payload_provider as provider};
use perry_ffi::native_payload::{self as np, OwnerLink, PayloadMiss};
use perry_ffi::turnloop_net as tl;
use perry_ffi::{ArrayHeader, JsThis, JsValue, RawClosureHeader, TransientRootScope};

pub(crate) const HIGH_WATER_MARK: usize = 64 * 1024;

extern "C" {
    fn js_value_is_closure(bits: i64) -> i32;
    fn js_net_validate_connect_port(port: f64);
    fn js_node_stream_method_once(owner: i64, event: f64, callback: f64) -> f64;
    fn js_array_shift_f64(array: *mut ArrayHeader) -> f64;
    fn js_queue_next_tick(callback: i64);
}

pub(crate) fn is_callback(value: f64) -> bool {
    // SAFETY: The classifier accepts NaN-boxed JS values, checks their tag before reading a
    // closure, and cannot collect.
    unsafe { js_value_is_closure(value.to_bits() as i64) != 0 }
}

pub(crate) fn error(code: &str, message: &str) -> f64 {
    let scope = TransientRootScope::enter();
    // SAFETY: Error allocation runs on this agent with no payload borrow; its result is rooted
    // before the code string is allocated.
    let value = scope.root_nanbox(unsafe { crate::build_error_object(message) });
    let code = events::string(code);
    p::own_set(value.get(), "code", code);
    value.get()
}

pub(crate) fn link(owner: f64) -> OwnerLink {
    transport_link(owner).unwrap_or_else(super::payload_prototype::throw_miss)
}

pub(crate) fn transport_link(owner: f64) -> Result<OwnerLink, PayloadMiss> {
    let own = p::socket_link(owner)?;
    // Closed wrappers use their own disposed cell. Their saved completion
    // context can still deliver close, but public methods may not reach a
    // reopened parent's replacement resource through an old parent edge.
    // SAFETY: The retained link belongs to this agent; projection checks family/open lifecycle, and
    // field/core access ends before allocation or JS.
    let Ok(payload) = (unsafe { p::socket_ptr(own) }) else {
        return Ok(own);
    };
    // SAFETY: The preceding successful projection proves this payload is live; this access ends
    // before allocation or JS, and TLS results require a fresh generation check.
    if unsafe { !(*payload).ext.tls_parent } {
        return Ok(own);
    }
    // SAFETY: The owner/link is family-checked on this agent; state comes from its traced cell and
    // is rooted before any later allocation.
    let state = unsafe { p::socket_state(own, false) };
    let parent = p::record_get(state, "tlsParent");
    if JsValue::from_bits(parent.to_bits()).is_undefined() {
        Ok(own)
    } else {
        p::socket_link(parent)
    }
}

pub(crate) fn state(owner: f64) -> f64 {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    // SAFETY: The owner/link is family-checked on this agent; state comes from its traced cell and
    // is rooted before any later allocation.
    unsafe {
        p::socket_state(
            p::socket_link(owner.get()).unwrap_or_else(super::payload_prototype::throw_miss),
            true,
        )
    }
}

/// The wrapper and its parent are ordinary objects, with ordinary traced
/// edges in both directions. Existing read tokens continue naming the parent
/// cell; the TLS route reads its wrapper edge when delivering JS events.
pub(crate) fn new_tls_wrapper(parent: f64) -> f64 {
    let scope = TransientRootScope::enter();
    let parent = scope.root_nanbox(parent);
    let parent_link = link(parent.get());
    let transport_owner =
        // SAFETY: The retained link is validated on this agent; its returned JS owner is rooted
        // immediately, before allocation.
        scope.root_nanbox(unsafe { np::link_event_owner(parent_link) }.unwrap_or(parent.get()));
    // SAFETY: The retained link belongs to this agent; projection checks family/open lifecycle, and
    // field/core access ends before allocation or JS.
    let open = unsafe { p::socket_ptr(parent_link) }.is_ok();
    let owner = scope.root_nanbox(p::alloc_socket(
        super::payload_io::TLS_SUBSYSTEM,
        p::SocketFields {
            tls_parent: true,
            ..Default::default()
        },
    ));
    let state = scope.root_nanbox(state(owner.get()));
    initialize_open_state(owner.get(), state.get());
    p::own_set(owner.get(), "_parent", parent.get());
    let parent_state = scope.root_nanbox(self::state(transport_owner.get()));
    p::record_set(state.get(), "tlsParent", transport_owner.get());
    p::record_set(parent_state.get(), "tlsWrapper", owner.get());
    if !open {
        for key in [
            "bytesRead",
            "bytesWritten",
            "readableEnded",
            "writableEnded",
            "localAddress",
            "localPort",
            "localFamily",
            "peerAddress",
            "peerPort",
            "peerFamily",
        ] {
            let value = scope.root_nanbox(p::record_get(parent_state.get(), key));
            p::record_set(state.get(), key, value.get());
        }
    }
    if open {
        let parent_state = scope.root_nanbox(self::state(transport_owner.get()));
        for key in [
            "allowHalfOpen",
            "refed",
            provider::TCP,
            provider::CONNECT,
            provider::SHUTDOWN,
        ] {
            let value = scope.root_nanbox(p::record_get(parent_state.get(), key));
            p::record_set(state.get(), key, value.get());
        }
        // SAFETY: The parent is rooted; route change freshly validates its live payload and ends
        // before any throw, without replacing the driver or retaining a pointer.
        unsafe {
            if let Err(failure) = p::set_route(parent_link, super::payload_io::TLS_SUBSYSTEM) {
                // Refuse a parent retired while the wrapper's state was built.
                perry_ffi::throw_with_code(
                    &failure.message(),
                    &failure.code,
                    perry_ffi::ErrorKind::Error,
                );
            }
        }
    }
    owner.get()
}

/// Initialize stream state on the ordinary owner. No JS edge enters Rust.
pub(crate) fn new_socket(route: u8, options: f64) -> f64 {
    new_socket_with_trigger(route, options, u64::MAX)
}
pub(crate) fn new_socket_with_trigger(route: u8, options: f64, trigger: u64) -> f64 {
    initialize_socket(route, options, trigger, None)
}
pub(crate) fn initialize_socket(
    route: u8,
    options: f64,
    trigger: u64,
    receiver: Option<f64>,
) -> f64 {
    let scope = TransientRootScope::enter();
    let receiver = receiver.map(|value| scope.root_nanbox(value));
    let options = scope.root_nanbox(options);
    let allow_half_open =
        // SAFETY: The argument is a live JS value on this agent; conversions finish before any
        // payload projection, and allocating paths retain their rooted operands.
        unsafe { crate::get_object_bool_field(options.get(), "allowHalfOpen") }.unwrap_or(false);
    let fields = SocketFields {
        allow_half_open,
        ..SocketFields::default()
    };
    let owner = scope.root_nanbox(match receiver {
        Some(receiver) => {
            if !p::attach_socket(receiver.get(), route, fields) {
                super::payload_prototype::throw_miss::<()>(PayloadMiss::Foreign);
            }
            receiver.get()
        }
        None => p::alloc_socket(route, fields),
    });
    let state = scope.root_nanbox(state(owner.get()));
    p::record_set(
        state.get(),
        "allowHalfOpen",
        f64::from_bits(JsValue::from_bool(allow_half_open).bits()),
    );
    initialize_open_state(owner.get(), state.get());
    let resource = scope.root_nanbox(prepare_tcp(owner.get(), trigger));
    provider::notify(resource.get());
    owner.get()
}

fn prepare_tcp(owner: f64, trigger: u64) -> f64 {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let state = scope.root_nanbox(state(owner.get()));
    let resource = scope.root_nanbox(provider::publish(
        state.get(),
        provider::TCP,
        b"TCPWRAP",
        trigger,
    ));
    // SAFETY: The retained link belongs to this agent; projection checks family/open lifecycle, and
    // field/core access ends before allocation or JS.
    unsafe {
        if let Ok(payload) = p::socket_ptr(link(owner.get())) {
            (*payload).ext.tcp_async_id = provider::id(resource.get());
        }
    }
    resource.get()
}

fn initialize_open_state(owner: f64, state: f64) {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let state = scope.root_nanbox(state);
    for key in [
        "destroyed",
        "hadError",
        "readableEnded",
        "writableEnded",
        "unconnectedWriteFailed",
    ] {
        p::record_set(state.get(), key, f64::from_bits(JsValue::FALSE.bits()));
    }
    for key in ["bytesRead", "bytesWritten"] {
        p::record_set(state.get(), key, 0.0);
    }
    for key in [
        "closeError",
        "server",
        "serverGroup",
        "parser",
        "encrypted",
        "authorized",
        "servername",
        "alpnProtocol",
        "authorizationError",
        "protocol",
        "peerCertificateDer",
        "ownCertificateDer",
        "checkServerIdentity",
        "session",
        "sessionSupplied",
        "sessionReused",
        "tlsConnected",
        "cipher",
        "tlsWrapper",
        "tlsParent",
    ] {
        p::record_set(state.get(), key, p::undefined());
    }
    p::record_set(state.get(), "refed", f64::from_bits(JsValue::TRUE.bits()));
    // SAFETY: This allocation runs on the owning agent; its array result is rooted or installed
    // into a rooted record before further allocation.
    let callbacks = unsafe { perry_ffi::js_array_alloc(0) };
    p::record_set(state.get(), "callbacks", p::boxed_addr(callbacks as i64));
    let _ = owner;
}

pub(crate) fn once(owner: f64, event: &str, callback: f64) {
    if !is_callback(callback) {
        return;
    }
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let callback = scope.root_nanbox(callback);
    let event = events::string(event);
    // SAFETY: The receiver and callback are rooted; listener registration owns their traced edges
    // and runs without a native payload borrow.
    unsafe {
        js_node_stream_method_once(p::raw_owner(owner.get()), event, callback.get());
    }
}

pub(crate) fn connect(owner: f64, arg1: f64, arg2: f64, arg3: f64) -> f64 {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let arg1 = scope.root_nanbox(arg1);
    let arg2 = scope.root_nanbox(arg2);
    let arg3 = scope.root_nanbox(arg3);
    let mut socket = link(owner.get());
    let _transport_owner =
        // SAFETY: The retained link is validated on this agent; its returned JS owner is rooted
        // immediately, before allocation.
        scope.root_nanbox(unsafe { np::link_event_owner(socket) }.unwrap_or(owner.get()));
    // SAFETY: The argument is a live JS value on this agent; conversions finish before any payload
    // projection, and allocating paths retain their rooted operands.
    let (host, port, path, callback) = unsafe {
        if let Some(path) = crate::jsvalue_to_owned_string(arg1.get()) {
            (String::new(), 0.0, Some(path), arg2.get())
        } else if JsValue::from_bits(arg1.get().to_bits()).is_pointer() {
            let path = crate::get_object_string_field(arg1.get(), "path");
            let port = crate::get_object_number_field(arg1.get(), "port").unwrap_or(0.0);
            let host = crate::get_object_string_field(arg1.get(), "host")
                .or_else(|| crate::get_object_string_field(arg1.get(), "hostname"))
                .unwrap_or_else(|| "localhost".into());
            (host, port, path, arg2.get())
        } else {
            let host = crate::jsvalue_to_owned_string(arg2.get());
            let callback = if host.is_some() {
                arg3.get()
            } else {
                arg2.get()
            };
            (
                host.unwrap_or_else(|| "localhost".into()),
                JsValue::from_bits(arg1.get().to_bits()).to_number(),
                None,
                callback,
            )
        }
    };
    let callback = scope.root_nanbox(callback);
    if path.is_none() {
        // SAFETY: The validator consumes scalar arguments on this agent; no payload borrow is held
        // when it throws.
        unsafe {
            js_net_validate_connect_port(port);
        }
    }
    let own = p::socket_link(owner.get()).unwrap_or_else(super::payload_prototype::throw_miss);
    // SAFETY: This owner retains a stable cell on its agent; lifecycle lookup validates the family
    // without dereferencing a disposed payload.
    if own != socket && unsafe { np::link_lifecycle(own, &SOCKET) } == Ok(np::Lifecycle::Closed) {
        let own_state = scope.root_nanbox(state(owner.get()));
        p::own_set(own_state.get(), "tlsParent", p::undefined());
        socket = own;
    }
    let _account = p::AccountSocket(socket);
    once(owner.get(), "connect", callback.get());
    let state = scope.root_nanbox(state(owner.get()));
    // SAFETY: This owner retains a stable cell on its agent; lifecycle lookup validates the family
    // without dereferencing a disposed payload.
    unsafe {
        if np::link_lifecycle(socket, &SOCKET) == Ok(np::Lifecycle::Closed) {
            p::reopen_socket(socket, super::payload_io::ROUTE).unwrap_or_else(|_| {
                super::payload_prototype::throw_miss::<()>(PayloadMiss::Closed)
            });
            let allow =
                JsValue::from_bits(p::record_get(state.get(), "allowHalfOpen").to_bits()).to_bool();
            // Reopen installed this payload; no JS or allocation has intervened.
            (*p::socket_ptr(socket).expect("reopened Socket"))
                .ext
                .allow_half_open = allow;
            initialize_open_state(owner.get(), state.get());
        }
    }
    if JsValue::from_bits(p::record_get(state.get(), "unconnectedWriteFailed").to_bits()).to_bool()
    {
        return owner.get();
    }
    // SAFETY: The retained link belongs to this agent; projection checks family/open lifecycle, and
    // field/core access ends before allocation or JS.
    let busy = unsafe {
        let fields =
            &(*p::socket_ptr(socket).unwrap_or_else(super::payload_prototype::throw_miss)).ext;
        fields.connecting || fields.opened
    };
    if busy {
        let error = error(
            "ERR_SOCKET_ALREADY_CONNECTED",
            "Socket is already connected",
        );
        events::queue_emit(owner.get(), "error", &[error]);
        return owner.get();
    }
    // SAFETY: The retained link belongs to this agent; projection checks family/open lifecycle, and
    // field/core access ends before allocation or JS.
    unsafe {
        let fields =
            &mut (*p::socket_ptr(socket).unwrap_or_else(super::payload_prototype::throw_miss)).ext;
        fields.connecting = true;
        fields.host = host.clone();
        fields.port = port as u16;
    }
    // SAFETY: The retained link belongs to this agent; projection checks family/open lifecycle, and
    // field/core access ends before allocation or JS.
    let needs_tcp = unsafe {
        let Ok(payload) = p::socket_ptr(socket) else {
            return owner.get();
        };
        (*payload).ext.tcp_async_id == 0
    };
    let tcp = scope.root_nanbox(if needs_tcp {
        prepare_tcp(owner.get(), u64::MAX)
    } else {
        p::record_get(state.get(), provider::TCP)
    });
    let connect = scope.root_nanbox(provider::publish(
        state.get(),
        provider::CONNECT,
        b"TCPCONNECTWRAP",
        provider::id(tcp.get()),
    ));
    // SAFETY: The retained link belongs to this agent; projection checks family/open lifecycle, and
    // field/core access ends before allocation or JS.
    unsafe {
        let Ok(payload) = p::socket_ptr(socket) else {
            return owner.get();
        };
        (*payload).ext.connect_async_id = provider::id(connect.get());
    }
    // SAFETY: The retained link belongs to this agent; projection checks family/open lifecycle, and
    // field/core access ends before allocation or JS.
    let result = unsafe {
        let core = p::socket_core(socket).unwrap_or_else(super::payload_prototype::throw_miss);
        match path {
            Some(path) => tl::link_pipe_connect(&mut *core, socket, &path),
            None => tl::link_tcp_connect(&mut *core, socket, &host, port as u16, true),
        }
    };
    if let Err(failure) = result {
        let error = error(&failure.code, &failure.message());
        destroy(owner.get(), error);
        if needs_tcp {
            provider::notify(tcp.get());
        }
        provider::notify(connect.get());
        return owner.get();
    }
    let snapshot = super::payload_io::snapshot(socket);
    if needs_tcp {
        provider::notify(tcp.get());
    }
    if snapshot
        .as_ref()
        .is_some_and(|snapshot| !super::payload_io::matches(socket, snapshot))
    {
        return owner.get();
    }
    provider::notify(connect.get());
    if snapshot
        .as_ref()
        .is_some_and(|snapshot| !super::payload_io::matches(socket, snapshot))
    {
        return owner.get();
    }
    owner.get()
}

pub(crate) fn update_addresses(socket: OwnerLink) {
    // SAFETY: The retained link belongs to this agent; projection checks family/open lifecycle, and
    // field/core access ends before allocation or JS.
    unsafe {
        let Ok(core) = p::socket_core(socket) else {
            return;
        };
        let local = tl::link_local_address(&mut *core, socket);
        let peer = tl::link_peer_address(&mut *core, socket);
        let parse = |endpoint: tl::Endpoint| {
            endpoint
                .address
                .parse()
                .ok()
                .map(|ip| std::net::SocketAddr::new(ip, endpoint.port))
        };
        if let Ok(payload) = p::socket_ptr(socket) {
            (*payload).ext.local = local.and_then(parse);
            (*payload).ext.peer = peer.and_then(parse);
        }
    }
}

/// Copy terminal getters before disposing native memory. The owner and state
/// remain ordinary, usable JS objects after the native resource is released.
pub(crate) fn cache_before_release(owner: f64, socket: OwnerLink) {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    // SAFETY: The retained link belongs to this agent; projection checks family/open lifecycle, and
    // field/core access ends before allocation or JS.
    let cached = unsafe {
        p::socket_ptr(socket).ok().map(|payload| {
            let fields = &(*payload).ext;
            (
                fields.bytes_read,
                fields.bytes_written,
                fields.read_end_emitted,
                fields.write_ended,
                fields.local,
                fields.peer,
            )
        })
    };
    let state = scope.root_nanbox(state(owner.get()));
    if let Some((read, written, read_ended, write_ended, local, peer)) = cached {
        p::record_set(state.get(), "bytesRead", read as f64);
        p::record_set(state.get(), "bytesWritten", written as f64);
        p::record_set(
            state.get(),
            "readableEnded",
            f64::from_bits(JsValue::from_bool(read_ended).bits()),
        );
        p::record_set(
            state.get(),
            "writableEnded",
            f64::from_bits(JsValue::from_bool(write_ended).bits()),
        );
        for (key, address) in [("local", local), ("peer", peer)] {
            if let Some(address) = address {
                let endpoint = scope.root_nanbox(events::string(&address.ip().to_string()));
                p::record_set(state.get(), &format!("{key}Address"), endpoint.get());
                p::record_set(state.get(), &format!("{key}Port"), address.port() as f64);
                p::record_set(
                    state.get(),
                    &format!("{key}Family"),
                    events::string(if address.is_ipv4() { "IPv4" } else { "IPv6" }),
                );
            }
        }
    } else if p::socket_link(owner.get()).is_ok_and(|own| own != socket) {
        // SAFETY: The retained link is validated on this agent; its returned JS owner is rooted
        // immediately, before allocation.
        let Some(parent) = (unsafe { np::link_event_owner(socket) }) else {
            return;
        };
        let parent = scope.root_nanbox(parent);
        let parent_state = scope.root_nanbox(self::state(parent.get()));
        for key in [
            "bytesRead",
            "bytesWritten",
            "readableEnded",
            "writableEnded",
            "localAddress",
            "localPort",
            "localFamily",
            "peerAddress",
            "peerPort",
            "peerFamily",
        ] {
            let value = scope.root_nanbox(p::record_get(parent_state.get(), key));
            p::record_set(state.get(), key, value.get());
        }
    }
    p::record_set(
        state.get(),
        "destroyed",
        f64::from_bits(JsValue::TRUE.bits()),
    );
}

pub(crate) fn destroy(owner: f64, error_value: f64) -> f64 {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let error_value = scope.root_nanbox(error_value);
    let socket = link(owner.get());
    let own = p::socket_link(owner.get()).unwrap_or_else(super::payload_prototype::throw_miss);
    let _account = p::AccountSocket(socket);
    // SAFETY: This owner retains a stable cell on its agent; lifecycle lookup validates the family
    // without dereferencing a disposed payload.
    if unsafe { np::link_lifecycle(own, &SOCKET) } == Ok(np::Lifecycle::Closed) {
        return owner.get();
    }
    let state = scope.root_nanbox(state(owner.get()));
    if own == socket {
        let wrapper = scope.root_nanbox(p::record_get(state.get(), "tlsWrapper"));
        if let Ok(wrapper_link) = p::socket_link(wrapper.get()) {
            if wrapper.get().to_bits() != owner.get().to_bits()
                && link(wrapper.get()) == socket
                // SAFETY: This owner retains a stable cell on its agent; lifecycle lookup validates
                // the family without dereferencing a disposed payload.
                && unsafe { np::link_lifecycle(wrapper_link, &SOCKET) } == Ok(np::Lifecycle::Open)
            {
                destroy(wrapper.get(), error_value.get());
                return owner.get();
            }
        }
    }
    // SAFETY: This owner retains a stable cell on its agent; lifecycle lookup validates the family
    // without dereferencing a disposed payload.
    if own != socket && unsafe { np::link_lifecycle(socket, &SOCKET) } == Ok(np::Lifecycle::Closed)
    {
        // SAFETY: The retained link is validated on this agent; its returned JS owner is rooted
        // immediately, before allocation.
        let Some(parent) = (unsafe { np::link_event_owner(socket) }) else {
            return owner.get();
        };
        let parent = scope.root_nanbox(parent);
        let parent_state = scope.root_nanbox(self::state(parent.get()));
        let pending = scope.root_nanbox(super::payload_closed::for_wrapper(
            parent_state.get(),
            owner.get(),
        ));
        let owed = JsValue::from_bits(pending.get().to_bits()).is_pointer();
        let record = scope.root_nanbox(super::payload_closed::record(if owed {
            super::payload_closed::parts(pending.get())
        } else {
            [0; 4]
        }));
        p::record_set(record.get(), "error", error_value.get());
        p::record_set(
            record.get(),
            "hadHandle",
            f64::from_bits(JsValue::TRUE.bits()),
        );
        cache_before_release(owner.get(), socket);
        let failure = scope.root_nanbox(error("ERR_STREAM_DESTROYED", "Socket is closed"));
        cancel_callbacks(owner.get(), state.get(), failure.get());
        provider::capture_socket(state.get(), record.get());
        // SAFETY: The rooted owner retains these family-checked links; memory-only release follows
        // the native borrow, and owed completions retain their own cell references.
        unsafe {
            np::close_link(own, &SOCKET);
        }
        if owed {
            super::payload_closed::append(state.get(), record.get());
        } else {
            super::payload_io::queue_socket_closed(owner.get(), record.get());
        }
        return owner.get();
    }
    // SAFETY: The retained link belongs to this agent; projection checks family/open lifecycle, and
    // field/core access ends before allocation or JS.
    let parts = unsafe {
        p::socket_core(socket)
            .ok()
            .and_then(|core| tl::link_handle_parts(&mut *core, socket))
    };
    let close_record = scope.root_nanbox(super::payload_closed::record(parts.unwrap_or([0; 4])));
    // SAFETY: The retained link is validated on this agent; its returned JS owner is rooted
    // immediately, before allocation.
    let parent = scope.root_nanbox(unsafe { np::link_event_owner(socket) }.unwrap_or(owner.get()));
    let parent_state = scope.root_nanbox(self::state(parent.get()));
    let wrapper = scope.root_nanbox(p::record_get(parent_state.get(), "tlsWrapper"));
    if p::socket_link(wrapper.get()).is_ok() {
        p::own_set(close_record.get(), "tlsWrapper", wrapper.get());
    }
    let had_handle = own != socket
        || parts.is_some()
        // SAFETY: The retained link belongs to this agent; projection checks family/open lifecycle,
        // and field/core access ends before allocation or JS.
        || unsafe {
            p::socket_ptr(socket)
                .ok()
                .is_some_and(|payload| (*payload).ext.connecting)
        };
    p::own_set(
        close_record.get(),
        "hadHandle",
        f64::from_bits(JsValue::from_bool(had_handle).bits()),
    );
    let had_error = !JsValue::from_bits(error_value.get().to_bits()).is_undefined()
        && !JsValue::from_bits(error_value.get().to_bits()).is_null();
    {
        p::own_set(
            close_record.get(),
            "error",
            if had_error {
                error_value.get()
            } else {
                p::undefined()
            },
        );
        let group = scope.root_nanbox(p::record_get(state.get(), "serverGroup"));
        p::own_set(close_record.get(), "serverGroup", group.get());
    }
    // SAFETY: The retained link belongs to this agent; projection checks family/open lifecycle, and
    // field/core access ends before allocation or JS.
    let result = unsafe {
        let core = p::socket_core(socket).unwrap_or_else(super::payload_prototype::throw_miss);
        tl::link_close(&mut *core, socket)
    };
    let owed = match result {
        Ok(owed) => owed,
        // A refused driver close restored the live handle and its pin. Keep
        // its payload installed; disposing it would strand that capability.
        Err(failure) => {
            let failure = error(&failure.code, &failure.message());
            events::queue_emit(owner.get(), "error", &[failure]);
            return owner.get();
        }
    };
    if had_error {
        p::record_set(
            state.get(),
            "hadError",
            f64::from_bits(JsValue::TRUE.bits()),
        );
        p::record_set(state.get(), "closeError", error_value.get());
    }
    cache_before_release(owner.get(), socket);
    if own != socket {
        cache_before_release(parent.get(), socket);
    }
    let callback_error = if had_error {
        error_value.get()
    } else {
        error(
            "ERR_STREAM_DESTROYED",
            "Cannot call write after a stream was destroyed",
        )
    };
    cancel_callbacks(owner.get(), state.get(), callback_error);
    super::native_transport::release_codec(owner.get());
    #[cfg(test)]
    if std::env::var("PERRY_NET_BINDING_SABOTAGE").as_deref() == Ok("churn_backlog") {
        // The N3 fault retains a real owned buffer after its Socket closes.
        // This byte-only leak exists solely in the test executable.
        static LEAK: std::sync::Mutex<Vec<std::collections::VecDeque<u8>>> =
            std::sync::Mutex::new(Vec::new());
        // SAFETY: The retained link belongs to this agent; projection checks family/open lifecycle,
        // and field/core access ends before allocation or JS.
        if let Ok(payload) = unsafe { p::socket_ptr(socket) } {
            // SAFETY: The preceding successful projection proves this payload is live; this access
            // ends before allocation or JS, and TLS results require a fresh generation check.
            let bytes = unsafe { std::mem::take(&mut (*payload).ext.read_buffer) };
            LEAK.lock().unwrap().push(bytes);
        }
    }
    // SAFETY: The rooted owner retains these family-checked links; memory-only release follows the
    // native borrow, and owed completions retain their own cell references.
    unsafe {
        np::close_link(socket, &SOCKET);
        if own != socket {
            np::close_link(own, &SOCKET);
        }
        // N4 fault (test executable only): drop the owed Closed's cell
        // reference at close() instead of when Closed is delivered.
        #[cfg(test)]
        if std::env::var("PERRY_NET_BINDING_SABOTAGE").as_deref() == Ok("early_unref") {
            np::link_unref(socket);
        }
    }
    provider::capture_socket(state.get(), close_record.get());
    if owed {
        super::payload_closed::append(state.get(), close_record.get());
        if own != socket {
            let parent_record =
                scope.root_nanbox(super::payload_closed::record(parts.unwrap_or([0; 4])));
            p::own_set(parent_record.get(), "tlsWrapper", owner.get());
            let group = scope.root_nanbox(p::record_get(parent_state.get(), "serverGroup"));
            p::own_set(parent_record.get(), "serverGroup", group.get());
            provider::capture_socket(parent_state.get(), parent_record.get());
            super::payload_closed::append(parent_state.get(), parent_record.get());
        }
    }
    if !owed {
        if own != socket {
            let parent_record = scope.root_nanbox(super::payload_closed::record([0; 4]));
            p::own_set(
                parent_record.get(),
                "hadHandle",
                f64::from_bits(JsValue::FALSE.bits()),
            );
            let group = scope.root_nanbox(p::record_get(parent_state.get(), "serverGroup"));
            p::own_set(parent_record.get(), "serverGroup", group.get());
            provider::capture_socket(parent_state.get(), parent_record.get());
            super::payload_io::queue_socket_closed(parent.get(), parent_record.get());
        }
        super::payload_io::queue_socket_closed(owner.get(), close_record.get());
    }
    owner.get()
}

fn array_ptr(array: f64) -> *mut ArrayHeader {
    JsValue::from_bits(array.to_bits()).as_pointer()
}

fn register_callback(owner: f64, callback: f64) -> u64 {
    if !is_callback(callback) {
        return 0;
    }
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let callback = scope.root_nanbox(callback);
    let socket = link(owner.get());
    let _account = p::AccountSocket(socket);
    // SAFETY: The retained link belongs to this agent; projection checks family/open lifecycle, and
    // field/core access ends before allocation or JS.
    let user = unsafe {
        let fields =
            &mut (*p::socket_ptr(socket).unwrap_or_else(super::payload_prototype::throw_miss)).ext;
        fields.callback_seq = fields.callback_seq.checked_add(1).unwrap_or_else(|| {
            perry_ffi::throw_with_code(
                "Socket callback sequence exhausted",
                "ERR_OUT_OF_RANGE",
                perry_ffi::ErrorKind::RangeError,
            )
        });
        fields.callback_seq
    };
    let state = scope.root_nanbox(state(owner.get()));
    let callbacks = scope.root_nanbox(p::record_get(state.get(), "callbacks"));
    // SAFETY: This allocation runs on the owning agent; its array result is rooted or installed
    // into a rooted record before further allocation.
    let entry = scope.root_addr(unsafe { perry_ffi::js_array_alloc(2) } as i64);
    // SAFETY: The binding-created arrays and each element are rooted; push reloads their addresses
    // and its result is rooted before another allocation.
    unsafe {
        let entry_ptr = perry_ffi::js_array_push(
            entry.get() as *mut ArrayHeader,
            JsValue::from_number(user as f64),
        );
        let entry = scope.root_addr(entry_ptr as i64);
        let entry_ptr = perry_ffi::js_array_push(
            entry.get() as *mut ArrayHeader,
            JsValue::from_bits(callback.get().to_bits()),
        );
        let entry = scope.root_addr(entry_ptr as i64);
        let updated = perry_ffi::js_array_push(
            array_ptr(callbacks.get()),
            JsValue::from_object_ptr(entry.get() as *mut ArrayHeader),
        );
        p::record_set(state.get(), "callbacks", p::boxed_addr(updated as i64));
    }
    user
}

/// Writes retire in submission order. Consuming the JS queue's prefix also
/// bounds its retained capacity for long-lived sockets; there is no token map.
pub(crate) fn take_callback(owner: f64, user: u64) -> f64 {
    if user == 0 {
        return p::undefined();
    }
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let state = scope.root_nanbox(state(owner.get()));
    let callbacks = scope.root_nanbox(p::record_get(state.get(), "callbacks"));
    // SAFETY: This binding-created array is reached through a rooted record; reads/shift are
    // callback-free, with in-bounds queue entries and no retained element pointer.
    unsafe {
        let callbacks_ptr = array_ptr(callbacks.get());
        if callbacks_ptr.is_null() || perry_ffi::js_array_length(callbacks_ptr) == 0 {
            return p::undefined();
        }
        let entry = perry_ffi::js_array_get(callbacks_ptr, 0);
        let entry_ptr = entry.as_pointer::<ArrayHeader>();
        if perry_ffi::js_array_get(entry_ptr, 0).to_number() as u64 != user {
            return p::undefined();
        }
        let callback = perry_ffi::js_array_get(entry_ptr, 1);
        js_array_shift_f64(callbacks_ptr);
        f64::from_bits(callback.bits())
    }
}

unsafe extern "C" fn cancel_tick(closure: *const RawClosureHeader, _: JsThis) -> f64 {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(perry_ffi::closure_capture_f64(closure, 0));
    let callbacks = scope.root_nanbox(perry_ffi::closure_capture_f64(closure, 1));
    let error = scope.root_nanbox(perry_ffi::closure_capture_f64(closure, 2));
    let resource = scope.root_nanbox(perry_ffi::closure_capture_f64(closure, 3));
    loop {
        let callbacks_ptr = array_ptr(callbacks.get());
        if perry_ffi::js_array_length(callbacks_ptr) == 0 {
            break;
        }
        let entry = scope.root_nanbox(js_array_shift_f64(callbacks_ptr));
        let callback = scope.root_nanbox(f64::from_bits(
            perry_ffi::js_array_get(array_ptr(entry.get()), 1).bits(),
        ));
        events::call_in(
            resource.get(),
            callback.get(),
            JsThis::UNDEFINED,
            &[error.get()],
        );
    }
    let _ = owner;
    p::undefined()
}

pub(crate) fn cancel_callbacks(owner: f64, state: f64, error: f64) {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let state = scope.root_nanbox(state);
    let error = scope.root_nanbox(error);
    let callbacks = scope.root_nanbox(p::record_get(state.get(), "callbacks"));
    // SAFETY: This binding-created array is reached through a rooted record; reads/shift are
    // callback-free, with in-bounds queue entries and no retained element pointer.
    if unsafe { perry_ffi::js_array_length(array_ptr(callbacks.get())) } == 0 {
        return;
    }
    // SAFETY: This allocation runs on the owning agent; its array result is rooted or installed
    // into a rooted record before further allocation.
    let replacement = unsafe { perry_ffi::js_array_alloc(0) };
    p::record_set(state.get(), "callbacks", p::boxed_addr(replacement as i64));
    let resource = scope.root_nanbox(provider::resource(owner.get()));
    let job = scope.root_addr(perry_ffi::alloc_closure(
        perry_ffi::js_function_info!(cancel_tick, 0; with_flags(perry_ffi::FN_BUILTIN)),
        4,
    ) as i64);
    // SAFETY: The rooted closure was allocated with these capture slots; every JS capture is rooted
    // until the scheduled job owns its traced edge.
    unsafe {
        let job_ptr = job.get() as *mut RawClosureHeader;
        perry_ffi::set_closure_capture_f64(job_ptr, 0, owner.get());
        perry_ffi::set_closure_capture_f64(job_ptr, 1, callbacks.get());
        perry_ffi::set_closure_capture_f64(job_ptr, 2, error.get());
        perry_ffi::set_closure_capture_f64(job_ptr, 3, resource.get());
        js_queue_next_tick(job.get());
    }
}

unsafe extern "C" fn unopened_write_tick(closure: *const RawClosureHeader, _: JsThis) -> f64 {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(perry_ffi::closure_capture_f64(closure, 0));
    let failure = scope.root_nanbox(error("ERR_SOCKET_CLOSED", "Socket is closed"));
    destroy(owner.get(), failure.get())
}

fn unopened_write_failure(owner: f64, callback: f64) {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let callback = scope.root_nanbox(callback);
    register_callback(owner.get(), callback.get());
    let state = scope.root_nanbox(state(owner.get()));
    if JsValue::from_bits(p::record_get(state.get(), "unconnectedWriteFailed").to_bits()).to_bool()
    {
        return;
    }
    p::record_set(
        state.get(),
        "unconnectedWriteFailed",
        f64::from_bits(JsValue::TRUE.bits()),
    );
    let job = scope.root_addr(perry_ffi::alloc_closure(
        perry_ffi::js_function_info!(unopened_write_tick, 0; with_flags(perry_ffi::FN_BUILTIN)),
        1,
    ) as i64);
    // SAFETY: The rooted closure was allocated with these capture slots; every JS capture is rooted
    // until the scheduled job owns its traced edge.
    unsafe {
        perry_ffi::set_closure_capture_f64(job.get() as *mut RawClosureHeader, 0, owner.get());
        js_queue_next_tick(job.get());
    }
}

pub(crate) fn write(owner: f64, chunk: f64, encoding: f64, callback: f64) -> f64 {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let chunk = scope.root_nanbox(chunk);
    let encoding = scope.root_nanbox(encoding);
    let callback = scope.root_nanbox(callback);
    let callback = scope.root_nanbox(if is_callback(encoding.get()) {
        encoding.get()
    } else {
        callback.get()
    });
    // Conversion may materialize an SSO string. Prove the receiver only after
    // that allocation, then retain the same native window until JS can run.
    // SAFETY: The argument is a live JS value on this agent; conversions finish before any payload
    // projection, and allocating paths retain their rooted operands.
    let bytes = unsafe { crate::jsvalue_to_socket_bytes(chunk.get()) }.unwrap_or_default();
    // SAFETY: The receiver is rooted and projection validates its agent, family and open lifecycle;
    // the pointer is consumed before allocation/JS or freshly projected afterward.
    let window = match unsafe { super::native_transport::transport_window(owner.get()) } {
        Ok(window) => window,
        Err(miss) => {
            let socket = link(owner.get());
            // SAFETY: This owner retains a stable cell on its agent; lifecycle lookup validates the
            // family without dereferencing a disposed payload.
            if unsafe { np::link_lifecycle(socket, &SOCKET) } == Ok(np::Lifecycle::Closed) {
                if is_callback(callback.get()) {
                    let error = error(
                        "ERR_STREAM_DESTROYED",
                        "Cannot call write after a stream was destroyed",
                    );
                    events::queue_call(owner.get(), callback.get(), &[error]);
                }
                return f64::from_bits(JsValue::FALSE.bits());
            }
            super::payload_prototype::throw_miss(miss)
        }
    };
    let socket = window.link;
    let _account = p::AccountSocket(socket);
    // SAFETY: The preceding successful projection proves this payload is live; this access ends
    // before allocation or JS, and TLS results require a fresh generation check.
    let (unopened, ended) = unsafe {
        let fields = &(*window.payload).ext;
        (
            !fields.opened && !fields.connecting && fields.cork_depth == 0,
            fields.write_ended,
        )
    };
    if unopened {
        unopened_write_failure(owner.get(), callback.get());
        return f64::from_bits(JsValue::FALSE.bits());
    }
    if ended {
        let error = scope.root_nanbox(error("ERR_STREAM_WRITE_AFTER_END", "write after end"));
        if is_callback(callback.get()) {
            events::queue_call(owner.get(), callback.get(), &[error.get()]);
        }
        destroy(owner.get(), error.get());
        return f64::from_bits(JsValue::FALSE.bits());
    }
    let (user, window) = if is_callback(callback.get()) {
        let user = register_callback(owner.get(), callback.get());
        // Callback registration allocates, so its old projection ends here.
        // SAFETY: The receiver is rooted and projection validates its agent, family and open
        // lifecycle; the pointer is consumed before allocation/JS or freshly projected afterward.
        let window = unsafe { super::native_transport::transport_window(owner.get()) }
            .unwrap_or_else(super::payload_prototype::throw_miss);
        (user, window)
    } else {
        (0, window)
    };
    let payload = window.payload;
    // SAFETY: The preceding successful projection proves this payload is live; this access ends
    // before allocation or JS, and TLS results require a fresh generation check.
    let may_run_js = unsafe { (*payload).ext.tls.is_some() };
    // Only TLS can reenter JS during submission. Retain the driver's existing
    // generation so an identity-preserving reopen cannot receive old results.
    let write_snapshot = may_run_js
        .then(|| super::payload_io::snapshot(socket))
        .flatten();
    // TLS drive returns before any JS when there is no driver snapshot. That
    // submission is callback-free too; a pending resolve has its own snapshot.
    let may_run_js = may_run_js && write_snapshot.is_some();
    // SAFETY: The preceding successful projection proves this payload is live; this access ends
    // before allocation or JS, and TLS results require a fresh generation check.
    let corked = unsafe {
        let fields = &mut (*window.payload).ext;
        if fields.cork_depth != 0 {
            fields.queued = fields.queued.saturating_add(bytes.len());
            fields.cork_bytes.extend_from_slice(&bytes);
            if user != 0 {
                fields.cork_users.push(user);
            }
            true
        } else {
            false
        }
    };
    if !corked {
        // SAFETY: The freshly projected window is current; plain I/O is callback-free and TLS ends
        // the native borrow before JS. Its generation is rechecked before using results.
        let result = unsafe { super::payload_tls::write_proven(owner.get(), window, &bytes, user) };
        match result {
            // SAFETY: The retained link belongs to this agent; projection checks family/open
            // lifecycle, and field/core access ends before allocation or JS.
            Ok(queued) => unsafe {
                if !may_run_js {
                    (*payload).ext.queued = queued;
                } else if write_snapshot
                    .as_ref()
                    .is_some_and(|snapshot| super::payload_io::matches(socket, snapshot))
                {
                    if let Ok(payload) = p::socket_ptr(socket) {
                        (*payload).ext.queued = queued;
                    }
                }
            },
            Err(failure) => {
                // A TLS drive may have reopened the cell while reporting an
                // old write error. That error cannot destroy the new driver.
                if may_run_js
                    && !write_snapshot
                        .as_ref()
                        .is_some_and(|expected| super::payload_io::matches(socket, expected))
                {
                    return f64::from_bits(JsValue::FALSE.bits());
                }
                let error = error(&failure.code, &failure.message());
                destroy(owner.get(), error);
                return f64::from_bits(JsValue::FALSE.bits());
            }
        }
    }
    // A TLS drive can invoke JS and replace the payload. Plain submission and
    // corking cannot; use their proven pointer rather than validating again.
    let payload = if may_run_js && !corked {
        if !write_snapshot
            .as_ref()
            .is_some_and(|expected| super::payload_io::matches(socket, expected))
        {
            return f64::from_bits(JsValue::FALSE.bits());
        }
        // SAFETY: The retained link belongs to this agent; projection checks family/open lifecycle,
        // and field/core access ends before allocation or JS.
        match unsafe { p::socket_ptr(socket) } {
            Ok(payload) => payload,
            Err(_) => return f64::from_bits(JsValue::FALSE.bits()),
        }
    } else {
        payload
    };
    // SAFETY: The preceding successful projection proves this payload is live; this access ends
    // before allocation or JS, and TLS results require a fresh generation check.
    let below = unsafe {
        let fields = &mut (*payload).ext;
        let below = fields.queued < HIGH_WATER_MARK;
        fields.need_drain |= !below;
        below
    };
    f64::from_bits(JsValue::from_bool(below).bits())
}

pub(crate) fn cork(owner: f64, uncork: bool) -> f64 {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let socket = link(owner.get());
    let _account = p::AccountSocket(socket);
    // SAFETY: The retained link belongs to this agent; projection checks family/open lifecycle, and
    // field/core access ends before allocation or JS.
    let flush = unsafe {
        p::socket_ptr(socket).ok().is_some_and(|payload| {
            let fields = &mut (*payload).ext;
            if uncork {
                fields.cork_depth = fields.cork_depth.saturating_sub(1);
            } else {
                fields.cork_depth = fields.cork_depth.saturating_add(1);
            }
            uncork && fields.cork_depth == 0
        })
    };
    if flush {
        flush_cork(owner.get());
    }
    owner.get()
}

fn flush_cork(owner: f64) {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let socket = link(owner.get());
    let _account = p::AccountSocket(socket);
    // SAFETY: The retained link belongs to this agent; projection checks family/open lifecycle, and
    // field/core access ends before allocation or JS.
    let Some((bytes, user, unopened, generation)) = (unsafe {
        p::socket_ptr(socket).ok().and_then(|payload| {
            let fields = &mut (*payload).ext;
            if fields.cork_bytes.is_empty() && fields.cork_users.is_empty() {
                return None;
            }
            let bytes = std::mem::take(&mut fields.cork_bytes);
            let users = std::mem::take(&mut fields.cork_users);
            let user = users.first().copied().unwrap_or(0);
            if users.len() > 1 {
                fields.coalesced_users.push_back((user, users));
            }
            fields.queued = fields.queued.saturating_sub(bytes.len());
            let unopened = !fields.opened && !fields.connecting;
            let generation = if fields.tls.is_some() {
                tl::link_snapshot_handle(&mut (*payload).core, socket)
            } else {
                None
            };
            Some((bytes, user, unopened, generation))
        })
    }) else {
        return;
    };
    if unopened {
        unopened_write_failure(owner.get(), p::undefined());
        return;
    }
    let result = super::payload_tls::write(owner.get(), &bytes, user);
    if generation
        .as_ref()
        .is_some_and(|expected| !super::payload_io::matches(socket, expected))
    {
        return;
    }
    match result {
        // SAFETY: The retained link belongs to this agent; projection checks family/open lifecycle,
        // and field/core access ends before allocation or JS.
        Ok(queued) => unsafe {
            if let Ok(payload) = p::socket_ptr(socket) {
                (*payload).ext.queued = queued;
            }
        },
        Err(failure) => {
            let error = error(&failure.code, &failure.message());
            destroy(owner.get(), error);
        }
    }
}

pub(crate) fn end(owner: f64, chunk: f64, encoding: f64, callback: f64) -> f64 {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let chunk = scope.root_nanbox(chunk);
    let encoding = scope.root_nanbox(encoding);
    let callback = scope.root_nanbox(callback);
    let (chunk_value, callback_value) = if is_callback(chunk.get()) {
        (p::undefined(), chunk.get())
    } else if is_callback(encoding.get()) {
        (chunk.get(), encoding.get())
    } else {
        (chunk.get(), callback.get())
    };
    let chunk = scope.root_nanbox(chunk_value);
    let callback = scope.root_nanbox(callback_value);
    let socket = link(owner.get());
    let _account = p::AccountSocket(socket);
    // SAFETY: This owner retains a stable cell on its agent; lifecycle lookup validates the family
    // without dereferencing a disposed payload.
    if unsafe { np::link_lifecycle(socket, &SOCKET) } == Ok(np::Lifecycle::Closed) {
        if is_callback(callback.get()) {
            let error = error(
                "ERR_STREAM_DESTROYED",
                "Cannot call end after a stream was destroyed",
            );
            events::queue_call(owner.get(), callback.get(), &[error]);
        }
        return owner.get();
    }
    let entry_generation = super::payload_io::snapshot(socket);
    if !JsValue::from_bits(chunk.get().to_bits()).is_undefined()
        && !JsValue::from_bits(chunk.get().to_bits()).is_null()
    {
        write(owner.get(), chunk.get(), encoding.get(), p::undefined());
        if entry_generation
            .as_ref()
            .is_some_and(|expected| !super::payload_io::matches(socket, expected))
        {
            return owner.get();
        }
    }
    // write() may have disposed the resource. Re-project it before continuing.
    // SAFETY: The retained link belongs to this agent; projection checks family/open lifecycle, and
    // field/core access ends before allocation or JS.
    if unsafe { p::socket_ptr(socket) }.is_err() {
        return owner.get();
    }
    // SAFETY: The retained link belongs to this agent; projection checks family/open lifecycle, and
    // field/core access ends before allocation or JS.
    let finished = unsafe {
        let Ok(payload) = p::socket_ptr(socket) else {
            return owner.get();
        };
        (*payload).ext.shutdown_done
    };
    if finished {
        if is_callback(callback.get()) {
            events::queue_call(owner.get(), callback.get(), &[]);
        }
        return owner.get();
    }
    let user = register_callback(owner.get(), callback.get());
    // SAFETY: The retained link belongs to this agent; projection checks family/open lifecycle, and
    // field/core access ends before allocation or JS.
    let already_ended = unsafe {
        let Ok(payload) = p::socket_ptr(socket) else {
            return owner.get();
        };
        let fields = &mut (*payload).ext;
        let ended = fields.write_ended;
        if ended {
            if user != 0 {
                fields.extra_end_users.push(user);
            }
        } else {
            fields.write_ended = true;
            fields.cork_depth = 0;
        }
        ended
    };
    if already_ended {
        return owner.get();
    }
    let snapshot = super::payload_io::snapshot(socket);
    let state = scope.root_nanbox(state(owner.get()));
    let tcp = scope.root_nanbox(p::record_get(state.get(), provider::TCP));
    let shutdown = scope.root_nanbox(provider::publish(
        state.get(),
        provider::SHUTDOWN,
        b"SHUTDOWNWRAP",
        provider::id(tcp.get()),
    ));
    // SAFETY: The retained link belongs to this agent; projection checks family/open lifecycle, and
    // field/core access ends before allocation or JS.
    unsafe {
        let Ok(payload) = p::socket_ptr(socket) else {
            return owner.get();
        };
        (*payload).ext.shutdown_async_id = provider::id(shutdown.get());
    }
    provider::notify(shutdown.get());
    if snapshot
        .as_ref()
        .is_some_and(|snapshot| !super::payload_io::matches(socket, snapshot))
    {
        return owner.get();
    }
    flush_cork(owner.get());
    if snapshot
        .as_ref()
        .is_some_and(|snapshot| !super::payload_io::matches(socket, snapshot))
    {
        return owner.get();
    }
    let result = super::payload_tls::shutdown(owner.get(), user);
    if let Err(failure) = result {
        let error = error(&failure.code, &failure.message());
        destroy(owner.get(), error);
    }
    owner.get()
}

pub(crate) fn read(owner: f64, size: f64) -> f64 {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let socket = link(owner.get());
    let _account = p::AccountSocket(socket);
    // SAFETY: The retained link belongs to this agent; projection checks family/open lifecycle, and
    // field/core access ends before allocation or JS.
    let bytes = unsafe {
        p::socket_ptr(socket).ok().and_then(|payload| {
            let buffer = &mut (*payload).ext.read_buffer;
            if buffer.is_empty() {
                return None;
            }
            let size = JsValue::from_bits(size.to_bits());
            let count = if size.is_number() {
                (size.to_number().max(0.0) as usize).min(buffer.len())
            } else {
                buffer.len()
            };
            if count == 0 {
                return None;
            }
            Some(buffer.drain(..count).collect::<Vec<u8>>())
        })
    };
    queue_read_end(owner.get());
    match bytes {
        Some(bytes) => {
            f64::from_bits(JsValue::from_object_ptr(perry_ffi::alloc_buffer(&bytes)).bits())
        }
        None => f64::from_bits(JsValue::NULL.bits()),
    }
}

/// An EOF delayed by buffered bytes still belongs to the original driver
/// handle. Captures are ordinary JS values; no native continuation registry.
unsafe extern "C" fn read_end_tick(closure: *const RawClosureHeader, _: JsThis) -> f64 {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(perry_ffi::closure_capture_f64(closure, 0));
    let socket = link(owner.get());
    let _account = p::AccountSocket(socket);
    let parts = std::array::from_fn(|index| {
        perry_ffi::closure_capture_f64(closure, index as u32 + 1) as u32
    });
    let current = p::socket_core(socket)
        .ok()
        .and_then(|core| tl::link_handle_parts(&mut *core, socket));
    if current == Some(parts) {
        super::payload_io::eof(owner.get());
    }
    p::undefined()
}

fn queue_read_end(owner: f64) {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let socket = link(owner.get());
    let _account = p::AccountSocket(socket);
    // SAFETY: The retained link belongs to this agent; projection checks family/open lifecycle, and
    // field/core access ends before allocation or JS.
    let ready = unsafe {
        p::socket_ptr(socket).ok().is_some_and(|payload| {
            let fields = &(*payload).ext;
            fields.read_ended && !fields.read_end_emitted && fields.read_buffer.is_empty()
        })
    };
    if !ready {
        return;
    }
    // SAFETY: The retained link belongs to this agent; projection checks family/open lifecycle, and
    // field/core access ends before allocation or JS.
    let Some(parts) = (unsafe {
        p::socket_core(socket)
            .ok()
            .and_then(|core| tl::link_handle_parts(&mut *core, socket))
    }) else {
        return;
    };
    let job = scope.root_addr(perry_ffi::alloc_closure(
        perry_ffi::js_function_info!(read_end_tick, 0; with_flags(perry_ffi::FN_BUILTIN)),
        5,
    ) as i64);
    // SAFETY: The rooted closure was allocated with these capture slots; every JS capture is rooted
    // until the scheduled job owns its traced edge.
    unsafe {
        let closure = job.get() as *mut RawClosureHeader;
        perry_ffi::set_closure_capture_f64(closure, 0, owner.get());
        for (index, part) in parts.into_iter().enumerate() {
            perry_ffi::set_closure_capture_f64(closure, index as u32 + 1, part as f64);
        }
        js_queue_next_tick(job.get());
    }
}

pub(crate) fn set_ref(owner: f64, referenced: bool) -> f64 {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let socket = link(owner.get());
    let _account = p::AccountSocket(socket);
    // SAFETY: The retained link belongs to this agent; projection checks family/open lifecycle, and
    // field/core access ends before allocation or JS.
    unsafe {
        if let Ok(core) = p::socket_core(socket) {
            tl::link_set_ref(&mut *core, socket, referenced);
        }
    }
    let state = scope.root_nanbox(state(owner.get()));
    p::record_set(
        state.get(),
        "refed",
        f64::from_bits(JsValue::from_bool(referenced).bits()),
    );
    owner.get()
}

pub(crate) fn get(owner: f64, key: &str) -> f64 {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let socket = link(owner.get());
    let _account = p::AccountSocket(socket);
    let state = scope.root_nanbox(state(owner.get()));
    if key == "destroyed" {
        return p::record_get(state.get(), key);
    }
    if key == "writableHighWaterMark" || key == "readableHighWaterMark" {
        return HIGH_WATER_MARK as f64;
    }
    if key == "_readableState" || key == "_writableState" {
        let cached = scope.root_nanbox(p::record_get(state.get(), key));
        if JsValue::from_bits(cached.get().to_bits()).is_pointer() {
            return cached.get();
        }
        let object = scope.root_nanbox(f64::from_bits(perry_ffi::alloc_object().bits()));
        p::record_set(state.get(), key, object.get());
        return object.get();
    }
    enum Scalar {
        Value(f64),
        Text(String),
        Cached,
    }
    // SAFETY: The retained link belongs to this agent; projection checks family/open lifecycle, and
    // field/core access ends before allocation or JS.
    let value = unsafe {
        match p::socket_ptr(socket) {
            Ok(payload) => {
                let fields = &(*payload).ext;
                let bool_value =
                    |value| Scalar::Value(f64::from_bits(JsValue::from_bool(value).bits()));
                match key {
                    "connecting" => bool_value(fields.connecting),
                    "pending" => bool_value(!fields.opened),
                    "writable" => bool_value(!fields.write_ended),
                    "readable" => bool_value(!fields.read_ended),
                    "writableEnded" => bool_value(fields.write_ended),
                    "readableEnded" => bool_value(fields.read_end_emitted),
                    "writableFinished" => bool_value(fields.shutdown_done),
                    "isPaused" => bool_value(fields.paused),
                    "readableFlowing" => match fields.flowing {
                        Some(value) => bool_value(value),
                        None => Scalar::Value(f64::from_bits(JsValue::NULL.bits())),
                    },
                    "writableNeedDrain" => bool_value(fields.need_drain),
                    "bytesRead" => Scalar::Value(fields.bytes_read as f64),
                    "bytesWritten" => Scalar::Value(
                        fields.bytes_written.saturating_add(fields.queued as u64) as f64,
                    ),
                    "writableLength" | "bufferSize" => Scalar::Value(fields.queued as f64),
                    "writableCorked" => Scalar::Value(fields.cork_depth as f64),
                    "timeout" => Scalar::Value(if fields.timeout_ms == 0 {
                        p::undefined()
                    } else {
                        fields.timeout_ms as f64
                    }),
                    "typeOfService" => Scalar::Value(fields.type_of_service as f64),
                    "readyState" => Scalar::Text(
                        if fields.connecting {
                            "opening"
                        } else if fields.read_ended {
                            if fields.write_ended {
                                "closed"
                            } else {
                                "writeOnly"
                            }
                        } else if fields.write_ended {
                            "readOnly"
                        } else {
                            "open"
                        }
                        .into(),
                    ),
                    "readableEncoding" => match &fields.encoding {
                        Some(encoding) => Scalar::Text(encoding.clone()),
                        None => Scalar::Value(f64::from_bits(JsValue::NULL.bits())),
                    },
                    "localAddress" | "remoteAddress" => match if key == "localAddress" {
                        fields.local
                    } else {
                        fields.peer
                    } {
                        Some(address) => Scalar::Text(address.ip().to_string()),
                        None => Scalar::Value(p::undefined()),
                    },
                    "localPort" | "remotePort" => Scalar::Value(
                        (if key == "localPort" {
                            fields.local
                        } else {
                            fields.peer
                        })
                        .map_or(p::undefined(), |address| address.port() as f64),
                    ),
                    "localFamily" | "remoteFamily" => match if key == "localFamily" {
                        fields.local
                    } else {
                        fields.peer
                    } {
                        Some(address) => {
                            Scalar::Text(if address.is_ipv4() { "IPv4" } else { "IPv6" }.into())
                        }
                        None => Scalar::Value(p::undefined()),
                    },
                    _ => Scalar::Cached,
                }
            }
            Err(_) => match key {
                "connecting" | "writable" | "readable" | "writableNeedDrain" => {
                    Scalar::Value(f64::from_bits(JsValue::FALSE.bits()))
                }
                "pending" => Scalar::Value(f64::from_bits(JsValue::TRUE.bits())),
                "writableLength" | "writableCorked" => Scalar::Value(0.0),
                "readyState" => Scalar::Text("closed".into()),
                "bufferSize" => Scalar::Value(p::undefined()),
                _ => Scalar::Cached,
            },
        }
    };
    match value {
        Scalar::Value(value) => value,
        Scalar::Text(text) => events::string(&text),
        Scalar::Cached => {
            let key = match key {
                "remoteAddress" => "peerAddress",
                "remotePort" => "peerPort",
                "remoteFamily" => "peerFamily",
                key => key,
            };
            p::record_get(state.get(), key)
        }
    }
}

extern "C" {
    fn js_net_validate_socket_timeout(value: f64);
    fn js_net_validate_tos(value: f64) -> i32;
}

pub(crate) fn set_timeout(owner: f64, ms: f64, callback: f64) -> f64 {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let callback = scope.root_nanbox(callback);
    let socket = link(owner.get());
    let _account = p::AccountSocket(socket);
    // SAFETY: The validator consumes scalar arguments on this agent; no payload borrow is held when
    // it throws.
    unsafe {
        js_net_validate_socket_timeout(ms);
    }
    once(owner.get(), "timeout", callback.get());
    // SAFETY: The retained link belongs to this agent; projection checks family/open lifecycle, and
    // field/core access ends before allocation or JS.
    unsafe {
        if let Ok(payload) = p::socket_ptr(socket) {
            (*payload).ext.timeout_ms = ms.max(0.0) as u64;
            let core = std::ptr::addr_of_mut!((*payload).core);
            if ms > 0.0 {
                let _ = tl::link_deadline_arm(&mut *core, socket, ms as u64);
            } else {
                let _ = tl::link_deadline_cancel(&mut *core, socket);
            }
        }
    }
    owner.get()
}

pub(crate) fn refresh_timeout(socket: OwnerLink) {
    // SAFETY: The retained link belongs to this agent; projection checks family/open lifecycle, and
    // field/core access ends before allocation or JS.
    unsafe {
        let Ok(payload) = p::socket_ptr(socket) else {
            return;
        };
        refresh_timeout_proven(&mut *payload, socket);
    }
}

/// Call only inside a callback-free projected window.
pub(crate) fn refresh_timeout_proven(payload: &mut p::SocketPayload, socket: OwnerLink) {
    let timeout = payload.ext.timeout_ms;
    if timeout != 0 {
        let _ = tl::link_deadline_arm(&mut payload.core, socket, timeout);
    }
}

pub(crate) fn set_tos(owner: f64, value: f64) -> f64 {
    let socket = link(owner);
    // SAFETY: The validator consumes scalar arguments on this agent; no payload borrow is held when
    // it throws.
    let value = unsafe { js_net_validate_tos(value) } as u8;
    // SAFETY: The retained link belongs to this agent; projection checks family/open lifecycle, and
    // field/core access ends before allocation or JS.
    unsafe {
        if let Ok(payload) = p::socket_ptr(socket) {
            (*payload).ext.type_of_service = value;
        }
    }
    owner
}

pub(crate) fn set_encoding(owner: f64, encoding: f64) -> f64 {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let socket = link(owner.get());
    let _account = p::AccountSocket(socket);
    // SAFETY: The argument is a live JS value on this agent; conversions finish before any payload
    // projection, and allocating paths retain their rooted operands.
    let encoding = unsafe { crate::jsvalue_to_owned_string(encoding) }
        .unwrap_or_else(|| "utf8".into())
        .to_ascii_lowercase();
    // SAFETY: The retained link belongs to this agent; projection checks family/open lifecycle, and
    // field/core access ends before allocation or JS.
    unsafe {
        if let Ok(payload) = p::socket_ptr(socket) {
            (*payload).ext.encoding = Some(encoding);
        }
    }
    owner.get()
}

pub(crate) fn address(owner: f64) -> f64 {
    let socket = link(owner);
    // SAFETY: The retained link belongs to this agent; projection checks family/open lifecycle, and
    // field/core access ends before allocation or JS.
    let address = unsafe {
        p::socket_ptr(socket)
            .ok()
            .and_then(|payload| (*payload).ext.local)
    };
    let Some(address) = address else {
        return f64::from_bits(JsValue::NULL.bits());
    };
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let object = scope.root_nanbox(f64::from_bits(perry_ffi::alloc_object().bits()));
    p::own_set(
        object.get(),
        "address",
        events::string(&address.ip().to_string()),
    );
    p::own_set(
        object.get(),
        "family",
        events::string(if address.is_ipv4() { "IPv4" } else { "IPv6" }),
    );
    p::own_set(object.get(), "port", address.port() as f64);
    let _ = owner;
    object.get()
}

pub(crate) fn set_paused(owner: f64, paused: bool) -> f64 {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let socket = link(owner.get());
    let _account = p::AccountSocket(socket);
    // SAFETY: The retained link belongs to this agent; projection checks family/open lifecycle, and
    // field/core access ends before allocation or JS.
    unsafe {
        if let Ok(payload) = p::socket_ptr(socket) {
            (*payload).ext.paused = paused;
            (*payload).ext.flowing = Some(!paused);
        }
    }
    if !paused {
        flow(owner.get());
    }
    owner.get()
}

/// Listener registration and resume use the same path. Read buffers belong
/// to this payload until listeners consume them; no pending-data registry.
pub(crate) fn flow(owner: f64) {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let socket = link(owner.get());
    let _account = p::AccountSocket(socket);
    let listeners = events::listener_count(owner.get(), "data");
    // SAFETY: The retained link belongs to this agent; projection checks family/open lifecycle, and
    // field/core access ends before allocation or JS.
    let action = unsafe {
        p::socket_ptr(socket).ok().and_then(|payload| {
            let fields = &mut (*payload).ext;
            if fields.paused && !fields.raw_consumer {
                return None;
            }
            if listeners != 0 && !fields.raw_consumer {
                fields.flowing = Some(true);
            }
            let start = fields.opened && !fields.read_started;
            if start {
                fields.read_started = true;
            }
            let bytes = if (listeners != 0 || fields.flowing == Some(true)) && !fields.raw_consumer
            {
                fields.read_buffer.drain(..).collect::<Vec<u8>>()
            } else {
                Vec::new()
            };
            Some((start, bytes))
        })
    };
    let Some((start, bytes)) = action else {
        return;
    };
    if start {
        // SAFETY: The retained link belongs to this agent; projection checks family/open lifecycle,
        // and field/core access ends before allocation or JS.
        let result = unsafe {
            p::socket_core(socket)
                .ok()
                .map(|core| tl::link_read_start(&mut *core, socket))
        };
        if let Some(Err(failure)) = result {
            let failure = error(&failure.code, &failure.message());
            destroy(owner.get(), failure);
            return;
        }
    }
    if !bytes.is_empty() {
        data(owner.get(), &bytes);
    }
    queue_read_end(owner.get());
}

/// Owned policy copied out before allocating a chunk or invoking listeners.
enum DataDelivery {
    Silent,
    Readable,
    Chunk(Option<String>),
}

fn data_delivery(payload: &mut p::SocketPayload, bytes: &[u8], listeners: usize) -> DataDelivery {
    let fields = &mut payload.ext;
    if fields.paused || fields.raw_consumer || (listeners == 0 && fields.flowing != Some(true)) {
        fields.read_buffer.extend(bytes.iter().copied());
        return if fields.raw_consumer {
            DataDelivery::Silent
        } else {
            DataDelivery::Readable
        };
    }
    if listeners == 0 {
        DataDelivery::Silent
    } else {
        DataDelivery::Chunk(fields.encoding.clone())
    }
}

/// The completion already proved this plain socket. No native borrow crosses
/// chunk allocation or a listener; its resource is rooted once for dispatch.
/// # Safety
/// window is a live callback-free projection and owner is rooted in scope.
pub(crate) unsafe fn data_proven(
    scope: &TransientRootScope,
    owner: &perry_ffi::TransientRootedNanbox,
    window: np::PayloadWindow<p::SocketPayload>,
    bytes: &[u8],
    listeners: usize,
) {
    let delivery = data_delivery(&mut *window.payload, bytes, listeners);
    if !matches!(delivery, DataDelivery::Chunk(_)) {
        p::account_socket(window.link);
    }
    let resource = scope.root_nanbox(p::record_get(window.state, provider::TCP));
    deliver_data(scope, owner, &resource, delivery, bytes);
}

pub(crate) fn data(owner: f64, bytes: &[u8]) {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let listeners = events::listener_count(owner.get(), "data");
    let socket = link(owner.get());
    let _account = p::AccountSocket(socket);
    // SAFETY: The retained link belongs to this agent; projection checks family/open lifecycle, and
    // field/core access ends before allocation or JS.
    let delivery = unsafe {
        p::socket_ptr(socket)
            .ok()
            .map(|payload| data_delivery(&mut *payload, bytes, listeners))
    };
    if let Some(delivery) = delivery {
        let resource = scope.root_nanbox(provider::resource(owner.get()));
        deliver_data(&scope, &owner, &resource, delivery, bytes);
    }
}

fn deliver_data(
    scope: &TransientRootScope,
    owner: &perry_ffi::TransientRootedNanbox,
    resource: &perry_ffi::TransientRootedNanbox,
    delivery: DataDelivery,
    bytes: &[u8],
) {
    let encoding = match delivery {
        DataDelivery::Silent => return,
        DataDelivery::Readable => {
            events::emit_rooted(scope, resource, owner, "readable", &[]);
            return;
        }
        DataDelivery::Chunk(encoding) => encoding,
    };
    let value = match encoding.as_deref() {
        Some("hex") => events::string(
            &bytes
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>(),
        ),
        Some("base64") => events::string(&base64_encode(bytes)),
        Some(_) => events::string(&String::from_utf8_lossy(bytes)),
        None => f64::from_bits(JsValue::from_object_ptr(perry_ffi::alloc_buffer(bytes)).bits()),
    };
    events::emit_rooted(scope, resource, owner, "data", &[value]);
}

fn base64_encode(bytes: &[u8]) -> String {
    const ALPHA: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let a = chunk[0];
        let b = chunk.get(1).copied().unwrap_or(0);
        let c = chunk.get(2).copied().unwrap_or(0);
        out.push(ALPHA[(a >> 2) as usize] as char);
        out.push(ALPHA[(((a & 3) << 4) | (b >> 4)) as usize] as char);
        out.push(if chunk.len() > 1 {
            ALPHA[(((b & 15) << 2) | (c >> 6)) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHA[(c & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}
