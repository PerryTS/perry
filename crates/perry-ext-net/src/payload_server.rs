//! Listener methods and accepted child ownership on ordinary JS objects.

use super::payload_transport::{self as p, ServerFields, SERVER};
use super::{
    payload_closed as closed, payload_events as events, payload_provider as provider,
    payload_socket as socket,
};
use perry_ffi::native_payload::{self as np, OwnerLink, PayloadMiss};
use perry_ffi::turnloop_net as tl;
use perry_ffi::{ArrayHeader, JsThis, JsValue, RawClosureHeader, TransientRootScope};

extern "C" {
    fn js_net_validate_listen_port(port: f64);
    fn js_node_stream_method_once(owner: i64, event: f64, callback: f64) -> f64;
}

pub(crate) fn link(owner: f64) -> OwnerLink {
    p::server_link(owner).unwrap_or_else(super::payload_prototype::throw_miss)
}
pub(crate) fn state(owner: f64) -> f64 {
    unsafe { p::server_state(link(owner), true) }
}

pub(crate) fn new_server(route: u8, options: f64, callback: f64) -> f64 {
    initialize_server(route, options, callback, None)
}
pub(crate) fn initialize_server(
    route: u8,
    options: f64,
    callback: f64,
    receiver: Option<f64>,
) -> f64 {
    let scope = TransientRootScope::enter();
    let receiver = receiver.map(|value| scope.root_nanbox(value));
    let options = scope.root_nanbox(options);
    let callback = scope.root_nanbox(if socket::is_callback(options.get()) {
        options.get()
    } else {
        callback
    });
    let fields = ServerFields {
        allow_half_open: unsafe { crate::get_object_bool_field(options.get(), "allowHalfOpen") }
            .unwrap_or(false),
        pause_on_connect: unsafe { crate::get_object_bool_field(options.get(), "pauseOnConnect") }
            .unwrap_or(false),
        ..ServerFields::default()
    };
    let options = (fields.allow_half_open, fields.pause_on_connect);
    let owner = scope.root_nanbox(match receiver {
        Some(receiver) => {
            if !p::attach_server(receiver.get(), route, fields) {
                super::payload_prototype::throw_miss::<()>(PayloadMiss::Foreign);
            }
            receiver.get()
        }
        None => p::alloc_server(route, fields),
    });
    let state = scope.root_nanbox(state(owner.get()));
    p::own_set(owner.get(), "_connections", 0.0);
    p::record_set(state.get(), "refed", f64::from_bits(JsValue::TRUE.bits()));
    p::record_set(
        state.get(),
        "allowHalfOpen",
        f64::from_bits(JsValue::from_bool(options.0).bits()),
    );
    p::record_set(
        state.get(),
        "pauseOnConnect",
        f64::from_bits(JsValue::from_bool(options.1).bits()),
    );
    if socket::is_callback(callback.get()) {
        extern "C" {
            fn js_node_stream_method_on(owner: i64, event: f64, callback: f64) -> f64;
        }
        unsafe {
            js_node_stream_method_on(
                p::raw_owner(owner.get()),
                events::string("connection"),
                callback.get(),
            );
        }
    }
    owner.get()
}

fn once(owner: f64, event: &str, callback: f64) {
    if !socket::is_callback(callback) {
        return;
    }
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let callback = scope.root_nanbox(callback);
    unsafe {
        js_node_stream_method_once(
            p::raw_owner(owner.get()),
            events::string(event),
            callback.get(),
        );
    }
}

pub(crate) fn listen(owner: f64, a: f64, b: f64, c: f64) -> f64 {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let a = scope.root_nanbox(a);
    let b = scope.root_nanbox(b);
    let c = scope.root_nanbox(c);
    let server = link(owner.get());
    let _account = p::AccountServer(server);
    let (path, host, port, backlog, callback) = unsafe {
        if let Some(path) = crate::jsvalue_to_owned_string(a.get()) {
            (Some(path), String::new(), 0.0, 511, b.get())
        } else if JsValue::from_bits(a.get().to_bits()).is_pointer() {
            (
                crate::get_object_string_field(a.get(), "path"),
                crate::get_object_string_field(a.get(), "host").unwrap_or_else(|| "::".into()),
                crate::get_object_number_field(a.get(), "port").unwrap_or(0.0),
                crate::get_object_number_field(a.get(), "backlog").unwrap_or(511.0) as u32,
                b.get(),
            )
        } else {
            let host = crate::jsvalue_to_owned_string(b.get());
            let callback = if host.is_some() { c.get() } else { b.get() };
            (
                None,
                host.unwrap_or_else(|| "::".into()),
                JsValue::from_bits(a.get().to_bits()).to_number(),
                511,
                callback,
            )
        }
    };
    let callback = scope.root_nanbox(callback);
    if path.is_none() {
        unsafe {
            js_net_validate_listen_port(port);
        }
    }
    once(owner.get(), "listening", callback.get());
    unsafe {
        if np::link_lifecycle(server, &SERVER) == Ok(np::Lifecycle::Closed) {
            let state = scope.root_nanbox(state(owner.get()));
            let allow_half_open =
                JsValue::from_bits(p::record_get(state.get(), "allowHalfOpen").to_bits()).to_bool();
            let pause_on_connect =
                JsValue::from_bits(p::record_get(state.get(), "pauseOnConnect").to_bits())
                    .to_bool();
            p::reopen_server(
                server,
                super::payload_io::ROUTE,
                ServerFields {
                    allow_half_open,
                    pause_on_connect,
                    ..ServerFields::default()
                },
            )
            .unwrap_or_else(|_| super::payload_prototype::throw_miss::<()>(PayloadMiss::Closed));
        }
        let listening = (*p::server_ptr(server)
            .unwrap_or_else(super::payload_prototype::throw_miss))
        .ext
        .listening;
        if listening {
            perry_ffi::throw_with_code(
                "Listen method has been called more than once without closing.",
                "ERR_SERVER_ALREADY_LISTEN",
                perry_ffi::ErrorKind::Error,
            );
        }
    }
    let result = unsafe {
        let core = p::server_core(server).unwrap_or_else(super::payload_prototype::throw_miss);
        let result = match &path {
            Some(path) => tl::link_pipe_listen(&mut *core, server, path, backlog),
            None => {
                tl::link_tcp_listen(&mut *core, server, &host, port as u16, backlog, false, true)
            }
        };
        result.and_then(|_| tl::link_accept_start(&mut *core, server))
    };
    if let Err(failure) = result {
        let error = socket::error(&failure.code, &failure.message());
        events::queue_emit(owner.get(), "error", &[error]);
        close(owner.get(), p::undefined());
        return owner.get();
    }
    finish_listen(owner.get(), path);
    owner.get()
}

/// The HTTP listener uses the same Server acceptance and child ownership,
/// retaining this ordinary object on its logical HTTP server. Binding errors
/// return synchronously to its existing deferred error path.
pub(crate) fn listen_tcp(
    owner: f64,
    host: &str,
    port: u16,
    backlog: u32,
    reuse_port: bool,
    no_delay: bool,
    tls_config: Option<std::sync::Arc<rustls::ServerConfig>>,
) -> Result<tl::Endpoint, tl::NetError> {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let server = link(owner.get());
    let _account = p::AccountServer(server);
    unsafe {
        if np::link_lifecycle(server, &SERVER) == Ok(np::Lifecycle::Closed) {
            let state = scope.root_nanbox(state(owner.get()));
            p::reopen_server(
                server,
                super::payload_io::ROUTE,
                ServerFields {
                    allow_half_open: JsValue::from_bits(
                        p::record_get(state.get(), "allowHalfOpen").to_bits(),
                    )
                    .to_bool(),
                    pause_on_connect: JsValue::from_bits(
                        p::record_get(state.get(), "pauseOnConnect").to_bits(),
                    )
                    .to_bool(),
                    ..ServerFields::default()
                },
            )
            .unwrap_or_else(|_| super::payload_prototype::throw_miss::<()>(PayloadMiss::Closed));
        }
        (*p::server_ptr(server).unwrap_or_else(super::payload_prototype::throw_miss))
            .ext
            .tls_config = tls_config;
    }
    let endpoint = unsafe {
        let core = p::server_core(server).unwrap_or_else(super::payload_prototype::throw_miss);
        tl::link_tcp_listen(
            &mut *core, server, host, port, backlog, reuse_port, no_delay,
        )?;
        tl::link_accept_start(&mut *core, server)?;
        tl::link_local_address(&mut *core, server).expect("bound listener has an address")
    };
    finish_listen(owner.get(), None);
    Ok(endpoint)
}

fn finish_listen(owner: f64, path: Option<String>) {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let server = link(owner.get());
    let parts = unsafe {
        let core = p::server_core(server).unwrap();
        let endpoint = tl::link_local_address(&mut *core, server);
        let local = endpoint.and_then(|endpoint| {
            endpoint
                .address
                .parse()
                .ok()
                .map(|ip| std::net::SocketAddr::new(ip, endpoint.port))
        });
        let fields = &mut (*p::server_ptr(server).unwrap()).ext;
        fields.local = local;
        fields.path = path;
        fields.listening = true;
        tl::link_handle_parts(&mut *core, server).expect("bound listener has a driver handle")
    };
    let state = scope.root_nanbox(state(owner.get()));
    let group = scope.root_nanbox(closed::record(parts));
    p::record_set(group.get(), "server", owner.get());
    p::record_set(
        group.get(),
        "closing",
        f64::from_bits(JsValue::FALSE.bits()),
    );
    p::record_set(
        group.get(),
        "listenerClosed",
        f64::from_bits(JsValue::FALSE.bits()),
    );
    let children = unsafe { perry_ffi::js_array_alloc(0) };
    p::record_set(group.get(), "children", p::boxed_addr(children as i64));
    p::record_set(state.get(), "activeGroup", group.get());
    let resource = scope.root_nanbox(provider::publish(
        state.get(),
        provider::SERVER,
        b"TCPSERVERWRAP",
        u64::MAX,
    ));
    unsafe {
        (*p::server_ptr(server).unwrap()).ext.async_id = provider::id(resource.get());
    }
    p::record_set(group.get(), provider::SERVER, resource.get());
    events::queue_emit(owner.get(), "listening", &[]);
    provider::notify(resource.get());
}

pub(crate) fn close(owner: f64, callback: f64) -> f64 {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let callback = scope.root_nanbox(callback);
    let server = link(owner.get());
    let _account = p::AccountServer(server);
    let state = scope.root_nanbox(state(owner.get()));
    let listening = unsafe {
        p::server_ptr(server)
            .ok()
            .is_some_and(|payload| (*payload).ext.listening)
    };
    if !listening {
        if socket::is_callback(callback.get()) {
            let error = scope.root_nanbox(socket::error(
                "ERR_SERVER_NOT_RUNNING",
                "Server is not running.",
            ));
            let wrapper = scope.root_addr(perry_ffi::alloc_closure(
                perry_ffi::js_function_info!(not_running_callback, 0; with_flags(perry_ffi::FN_BUILTIN)), 2,
            ) as i64);
            unsafe {
                let wrapper = wrapper.get() as *mut RawClosureHeader;
                perry_ffi::set_closure_capture_f64(wrapper, 0, callback.get());
                perry_ffi::set_closure_capture_f64(wrapper, 1, error.get());
            }
            once(owner.get(), "close", p::boxed_addr(wrapper.get()));
        }
        if unsafe { p::server_ptr(server) }.is_ok() {
            unsafe {
                np::close_link(server, &SERVER);
            }
        }
        if JsValue::from_bits(p::own_get(owner.get(), "_connections").to_bits()).to_number() == 0.0
        {
            events::queue_emit(owner.get(), "close", &[]);
        }
        return owner.get();
    }
    once(owner.get(), "close", callback.get());
    let group = scope.root_nanbox(p::record_get(state.get(), "activeGroup"));
    let (path, options) = unsafe {
        let fields = &(*p::server_ptr(server).unwrap()).ext;
        (
            fields.path.clone(),
            (fields.allow_half_open, fields.pause_on_connect),
        )
    };
    // Unlink synchronously. A late old Closed must never unlink a new bind.
    #[cfg(unix)]
    if let Some(path) = path {
        let _ = std::fs::remove_file(path);
    }
    #[cfg(not(unix))]
    let _ = path;
    let owed = unsafe { tl::link_close(&mut *p::server_core(server).unwrap(), server) };
    let owed = match owed {
        Ok(owed) => owed,
        Err(failure) => {
            let error = socket::error(&failure.code, &failure.message());
            events::queue_emit(owner.get(), "error", &[error]);
            return owner.get();
        }
    };
    unsafe {
        np::close_link(server, &SERVER);
    }
    p::record_set(
        state.get(),
        "allowHalfOpen",
        f64::from_bits(JsValue::from_bool(options.0).bits()),
    );
    p::record_set(
        state.get(),
        "pauseOnConnect",
        f64::from_bits(JsValue::from_bool(options.1).bits()),
    );
    if JsValue::from_bits(group.get().to_bits()).is_pointer() {
        p::record_set(group.get(), "closing", f64::from_bits(JsValue::TRUE.bits()));
        if owed {
            closed::append(state.get(), group.get());
        } else {
            p::record_set(
                group.get(),
                "listenerClosed",
                f64::from_bits(JsValue::TRUE.bits()),
            );
            finish_group(group.get(), true);
        }
    } else if !owed {
        events::queue_emit(owner.get(), "close", &[]);
    }
    owner.get()
}

unsafe extern "C" fn not_running_callback(closure: *const RawClosureHeader, this: JsThis) -> f64 {
    // The enclosing emitter owns the trap. Nothing owned is live in this
    // forwarding frame when the user callback throws through it.
    let callback = perry_ffi::closure_capture_f64(closure, 0);
    let error = perry_ffi::closure_capture_f64(closure, 1);
    perry_ffi::call_value(callback, this, &[error])
}

/// `server.address()` as JSON (`null` when not listening), for callers that
/// hold the server as a raw address rather than a JS value.
pub(crate) fn address_json(owner: f64) -> String {
    let server = link(owner);
    let address = unsafe {
        p::server_ptr(server).ok().and_then(|payload| {
            let fields = &(*payload).ext;
            fields
                .listening
                .then(|| (fields.path.clone(), fields.local))
        })
    };
    match address {
        Some((Some(path), _)) => {
            serde_json::to_string(&path).unwrap_or_else(|_| "null".to_string())
        }
        Some((None, Some(local))) => format!(
            "{{\"port\":{},\"address\":\"{}\",\"family\":\"{}\"}}",
            local.port(),
            local.ip(),
            if local.is_ipv4() { "IPv4" } else { "IPv6" }
        ),
        _ => "null".to_string(),
    }
}

pub(crate) fn address(owner: f64) -> f64 {
    let server = link(owner);
    let address = unsafe {
        p::server_ptr(server).ok().and_then(|payload| {
            let fields = &(*payload).ext;
            fields
                .listening
                .then(|| (fields.path.clone(), fields.local))
        })
    };
    let Some((path, local)) = address else {
        return f64::from_bits(JsValue::NULL.bits());
    };
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    if let Some(path) = path {
        return events::string(&path);
    }
    let Some(local) = local else {
        return f64::from_bits(JsValue::NULL.bits());
    };
    let object = scope.root_nanbox(f64::from_bits(perry_ffi::alloc_object().bits()));
    p::own_set(
        object.get(),
        "address",
        events::string(&local.ip().to_string()),
    );
    p::own_set(
        object.get(),
        "family",
        events::string(if local.is_ipv4() { "IPv4" } else { "IPv6" }),
    );
    p::own_set(object.get(), "port", local.port() as f64);
    let _ = owner;
    object.get()
}

pub(crate) fn get(owner: f64, key: &str) -> f64 {
    let server = link(owner);
    if key == "listening" {
        return f64::from_bits(
            JsValue::from_bool(unsafe {
                p::server_ptr(server)
                    .ok()
                    .is_some_and(|payload| (*payload).ext.listening)
            })
            .bits(),
        );
    }
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let state = scope.root_nanbox(state(owner.get()));
    if key == "_connections" || key == "maxConnections" || key == "dropMaxConnection" {
        p::own_get(owner.get(), key)
    } else {
        p::record_get(state.get(), key)
    }
}

pub(crate) fn get_connections(owner: f64, callback: f64) -> f64 {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let callback = scope.root_nanbox(callback);
    let count = get(owner.get(), "_connections");
    if socket::is_callback(callback.get()) {
        events::queue_call(
            owner.get(),
            callback.get(),
            &[f64::from_bits(JsValue::NULL.bits()), count],
        );
    }
    p::undefined()
}

pub(crate) fn set_ref(owner: f64, referenced: bool) -> f64 {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let server = link(owner.get());
    let _account = p::AccountServer(server);
    unsafe {
        if let Ok(core) = p::server_core(server) {
            tl::link_set_ref(&mut *core, server, referenced);
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

fn array_ptr(value: f64) -> *mut ArrayHeader {
    JsValue::from_bits(value.to_bits()).as_pointer()
}

fn finish_group(group: f64, synthetic: bool) {
    let scope = TransientRootScope::enter();
    let group = scope.root_nanbox(group);
    let children = scope.root_nanbox(p::record_get(group.get(), "children"));
    if unsafe { perry_ffi::js_array_length(array_ptr(children.get())) } != 0 {
        return;
    }
    if !JsValue::from_bits(p::record_get(group.get(), "closing").to_bits()).to_bool()
        || !JsValue::from_bits(p::record_get(group.get(), "listenerClosed").to_bits()).to_bool()
    {
        return;
    }
    // Consume the pending close before running user listeners. This is the
    // ordinary close callback's lifecycle, not a transport-generation latch.
    p::record_set(
        group.get(),
        "closing",
        f64::from_bits(JsValue::FALSE.bits()),
    );
    let owner = scope.root_nanbox(p::record_get(group.get(), "server"));
    let resource = scope.root_nanbox(p::record_get(group.get(), provider::SERVER));
    if synthetic {
        events::queue_emit_in(resource.get(), owner.get(), "close", &[], true);
    } else {
        events::emit_in(resource.get(), owner.get(), "close", &[]);
        provider::retire(resource.get());
    }
}

fn finish_pending(owner: f64, synthetic: bool) {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    if JsValue::from_bits(p::own_get(owner.get(), "_connections").to_bits()).to_number() != 0.0 {
        return;
    }
    let state = scope.root_nanbox(state(owner.get()));
    let array = scope.root_nanbox(p::record_get(state.get(), "closeCallbacks"));
    if !JsValue::from_bits(array.get().to_bits()).is_pointer() {
        return;
    }
    let count = unsafe { perry_ffi::js_array_length(array_ptr(array.get())) };
    let groups: Vec<_> = (0..count)
        .map(|index| {
            scope.root_nanbox(f64::from_bits(unsafe {
                perry_ffi::js_array_get(array_ptr(array.get()), index).bits()
            }))
        })
        .collect();
    for group in groups {
        if !JsValue::from_bits(p::record_get(group.get(), "listenerClosed").to_bits()).to_bool() {
            continue;
        }
        let group = scope.root_nanbox(closed::take(state.get(), closed::parts(group.get())));
        if JsValue::from_bits(group.get().to_bits()).is_pointer() {
            finish_group(group.get(), synthetic);
        }
    }
}

pub(crate) fn listener_closed(owner: f64, event: &tl::NetCompletion) {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let state = scope.root_nanbox(state(owner.get()));
    let Some(parts) = (unsafe { event.closed_handle_parts() }) else {
        return;
    };
    let group = scope.root_nanbox(closed::take(state.get(), parts));
    if !JsValue::from_bits(group.get().to_bits()).is_pointer() {
        return;
    }
    p::record_set(
        group.get(),
        "listenerClosed",
        f64::from_bits(JsValue::TRUE.bits()),
    );
    closed::append(state.get(), group.get());
    finish_pending(owner.get(), false);
}

pub(crate) fn child_closed(child: f64, group: f64) {
    let scope = TransientRootScope::enter();
    let child = scope.root_nanbox(child);
    let group = scope.root_nanbox(group);
    if !JsValue::from_bits(group.get().to_bits()).is_pointer() {
        return;
    }
    let children = scope.root_nanbox(p::record_get(group.get(), "children"));
    let count = unsafe { perry_ffi::js_array_length(array_ptr(children.get())) };
    let values: Vec<_> = (0..count)
        .map(|index| {
            scope.root_nanbox(f64::from_bits(unsafe {
                perry_ffi::js_array_get(array_ptr(children.get()), index).bits()
            }))
        })
        .collect();
    let Some(index) = values
        .iter()
        .position(|value| value.get().to_bits() == child.get().to_bits())
    else {
        return;
    };
    let mut remaining =
        scope.root_addr(unsafe { perry_ffi::js_array_alloc(count.saturating_sub(1)) } as i64);
    for (position, value) in values.iter().enumerate() {
        if position != index {
            remaining = scope.root_addr(unsafe {
                perry_ffi::js_array_push(
                    remaining.get() as *mut ArrayHeader,
                    JsValue::from_bits(value.get().to_bits()),
                )
            } as i64);
        }
    }
    p::record_set(group.get(), "children", p::boxed_addr(remaining.get()));
    let server = scope.root_nanbox(p::record_get(group.get(), "server"));
    let count = JsValue::from_bits(p::own_get(server.get(), "_connections").to_bits()).to_number();
    p::own_set(server.get(), "_connections", (count - 1.0).max(0.0));
    finish_pending(server.get(), true);
}

pub(crate) fn accepted(owner: f64, completion: *const tl::NetCompletion) {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let server = link(owner.get());
    let _account = p::AccountServer(server);
    let state = scope.root_nanbox(state(owner.get()));
    let group = scope.root_nanbox(p::record_get(state.get(), "activeGroup"));
    let Some((allow_half_open, pause_on_connect, tls_config, snapshot)) = (unsafe {
        p::server_ptr(server).ok().and_then(|payload| {
            let fields = &(*payload).ext;
            let core = std::ptr::addr_of_mut!((*payload).core);
            tl::link_snapshot_handle(&mut *core, server).map(|snapshot| {
                (
                    fields.allow_half_open,
                    fields.pause_on_connect,
                    fields.tls_config.clone(),
                    snapshot,
                )
            })
        })
    }) else {
        return;
    };
    let count = JsValue::from_bits(p::own_get(owner.get(), "_connections").to_bits()).to_number();
    let max = JsValue::from_bits(p::own_get(owner.get(), "maxConnections").to_bits());
    if max.is_number() && count >= max.to_number().max(0.0) {
        events::emit(owner.get(), "drop", &[]);
        return; // The runtime closes the accepted install slot on return.
    }
    let resource = scope.root_nanbox(p::record_get(group.get(), provider::SERVER));
    let child = scope.root_nanbox(socket::new_socket_with_trigger(
        super::payload_io::ROUTE,
        p::undefined(),
        provider::id(resource.get()),
    ));
    let child_link = socket::link(child.get());
    let _child_account = p::AccountSocket(child_link);
    unsafe {
        let Ok(core) = p::server_core(server) else {
            return;
        };
        if !tl::link_handle_matches(&mut *core, server, &snapshot) {
            return;
        }
        let Ok(child_core) = p::socket_core(child_link) else {
            return;
        };
        if tl::link_install_accepted(&mut *child_core, child_link, &*completion).is_err() {
            return;
        }
        let fields = &mut (*p::socket_ptr(child_link).unwrap()).ext;
        fields.opened = true;
        fields.allow_half_open = allow_half_open;
        fields.paused = pause_on_connect;
    }
    socket::update_addresses(child_link);
    let child_state = scope.root_nanbox(socket::state(child.get()));
    p::own_set(
        child_state.get(),
        "allowHalfOpen",
        f64::from_bits(JsValue::from_bool(allow_half_open).bits()),
    );
    p::own_set(child_state.get(), "server", owner.get());
    p::own_set(child_state.get(), "serverGroup", group.get());
    let children = scope.root_nanbox(p::record_get(group.get(), "children"));
    let updated = unsafe {
        perry_ffi::js_array_push(
            array_ptr(children.get()),
            JsValue::from_bits(child.get().to_bits()),
        )
    };
    p::record_set(group.get(), "children", p::boxed_addr(updated as i64));
    p::own_set(owner.get(), "_connections", count + 1.0);
    if let Some(config) = tls_config {
        if let Err(message) = super::payload_tls::install_server(child.get(), config) {
            let error = socket::error("ERR_TLS_HANDSHAKE_FAILED", &message);
            socket::destroy(child.get(), error);
            return;
        }
    }
    let child_snapshot = super::payload_io::snapshot(child_link);
    let child_resource = scope.root_nanbox(provider::resource(child.get()));
    events::emit_in(
        child_resource.get(),
        owner.get(),
        "connection",
        &[child.get()],
    );
    // Listener callbacks can close/reopen the child. Starting its read belongs
    // to whatever owner listeners requested; flow re-fetches its current T.
    if child_snapshot
        .as_ref()
        .is_some_and(|snapshot| super::payload_io::matches(child_link, snapshot))
    {
        socket::flow(child.get());
    }
    let _ = JsThis::UNDEFINED;
}
