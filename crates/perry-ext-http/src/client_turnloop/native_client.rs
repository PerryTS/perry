//! The HTTP client above an ordinary Socket. A separate parser owns only
//! framing bytes; Socket owns the core, TLS session and all driver refs.
//! There is no connection, request-to-connection, timer or pool identity map.
//! Request/Agent ownership stays in their existing traced app records.

use std::collections::VecDeque;
use std::sync::atomic::Ordering;

use perry_ext_net::native_transport::{self as net, RootedSocket};
use perry_ffi::native_payload::{self as np, PayloadFamily};
use perry_ffi::turnloop_net as tl;
use perry_ffi::{Handle, JsValue, TransientRootScope};

use super::protocol::{self, Conn, Effect};
use super::{Outbound, PoolKey, SUBSYSTEM};
use crate::{push_event, ClientRequestHandle, PendingHttpEvent};

static PARSER_VTABLE: perry_ffi::native_stream::PayloadVTable =
    perry_ffi::native_stream::payload_vtable::<Conn>(None);
static PARSER: PayloadFamily = PayloadFamily::new::<Conn>(
    perry_ffi::native_class_ids::HTTP_CLIENT_PARSER,
    "HTTPClientParser",
    false,
    &PARSER_VTABLE,
)
.with_constructor_length(0);
const PARSER_EDGE: &str = "httpClientParser";

pub(super) fn available() -> bool {
    static REGISTER: std::sync::Once = std::sync::Once::new();
    REGISTER.call_once(|| {
        assert!(
            tl::register_link_sink(SUBSYSTEM, sink),
            "HTTP client link sink registration refused"
        );
    });
    net::enabled() && tl::available(SUBSYSTEM)
}

/// `f` returns owned Rust data. End every projection before performing an
/// effect, allocating JS, closing a parser or touching the driver.
fn with_conn<R>(socket: &RootedSocket, f: impl FnOnce(&mut Conn) -> R) -> Option<R> {
    unsafe {
        socket
            .with_current(|_, state| {
                let parser = net::record_get(state, PARSER_EDGE);
                np::payload_mut::<Conn>(parser, &PARSER).ok().map(f)
            })
            .flatten()
    }
}

pub(super) fn reusable(owner: f64, key: &PoolKey) -> bool {
    with_conn(&RootedSocket::new(owner), |conn| {
        conn.reusable() && conn.key() == key
    })
    .unwrap_or(false)
}

fn mark_request(request: Handle) {
    crate::CLIENT_REQUESTS_INFLIGHT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert((perry_ffi::agent_post::current_agent(), request));
}
fn retire_request(request: Handle) {
    crate::CLIENT_REQUESTS_INFLIGHT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .remove(&(perry_ffi::agent_post::current_agent(), request));
    perry_ffi::notify_main_thread();
}

/// Memory-only explicit parser release. It must not run JS or the driver:
/// Socket destroy is already retiring the actual transport capability.
unsafe fn close_parser(owner: f64) {
    let socket = RootedSocket::new(owner);
    let scope = TransientRootScope::enter();
    let state = scope.root_nanbox(net::state(socket.value()));
    let error = scope.root_nanbox(net::record_get(state.get(), "closeError"));
    let text =
        |key: &str| JsValue::from_bits(net::own_get(error.get(), key).to_bits()).to_owned_string();
    let code = text("code").unwrap_or_else(|| "ECONNRESET".into());
    let message = text("message").unwrap_or_else(|| "socket hang up".into());
    let effects =
        with_conn(&socket, |conn| protocol::on_destroy(conn, &code, &message)).unwrap_or_default();
    let parser = scope.root_nanbox(net::record_get(state.get(), PARSER_EDGE));
    np::close(parser.get(), &PARSER);
    for effect in effects {
        match effect {
            Effect::RetireRequest(request) => retire_request(request),
            Effect::Push(event) => push_event(event),
            _ => {} // The caller already closes/cancels all transport operations.
        }
    }
}

fn dispose_parser(socket: &RootedSocket) {
    let scope = TransientRootScope::enter();
    let state = scope.root_nanbox(net::state(socket.value()));
    let parser = scope.root_nanbox(net::record_get(state.get(), PARSER_EDGE));
    np::close(parser.get(), &PARSER);
}

/// Begin on the actual Socket assigned by request/Agent ownership. A supplied
/// Socket keeps its existing read and TLS state. A fresh Socket dials once.
pub(super) fn start_on_socket(
    owner: f64,
    outbound: Outbound,
    supplied_head: Option<Vec<u8>>,
    retry: bool,
) -> Result<(), String> {
    let socket = RootedSocket::new(owner);
    if net::socket_link(socket.value()).is_err() {
        return Err("createConnection must return a net.Socket".into());
    }
    if !available() {
        return Err("connect ENOTSUP".into());
    }
    let supplied = supplied_head.is_some();
    let request = outbound.request_handle;
    mark_request(request);
    if !supplied && reusable(socket.value(), &outbound.key) {
        net::deadline_cancel(socket.value());
        net::set_ref(socket.value(), true);
        super::REUSED.fetch_add(1, Ordering::Relaxed);
        remember_capability(request, &socket);
        let effects = with_conn(&socket, |conn| conn.restart(outbound)).unwrap_or_default();
        run(&socket, effects);
        return Ok(());
    }
    if !supplied && net::snapshot(socket.value()).is_some() {
        // An idle Socket with another TLS/pool identity cannot be redialed.
        // Replace the actual request/Agent edge, then close the old owner.
        return replace_socket(&socket, outbound, retry);
    }
    let timeout = outbound.timeout_ms;
    let conn = if retry {
        Conn::retry(outbound)
    } else {
        Conn::new(outbound, net::tls_installed(socket.value()), supplied_head)
    };
    let (host, port) = conn.peer();
    let (host, port) = (host.to_owned(), port);
    let scope = TransientRootScope::enter();
    let parser = scope
        .root_nanbox(unsafe { np::alloc_in(&PARSER, "", conn, std::mem::size_of::<Conn>(), &[]) });
    if np::lifecycle(parser.get(), &PARSER) != Ok(np::Lifecycle::Open) {
        retire_request(request);
        return Err("HTTP client parser allocation refused".into());
    }
    let state = scope.root_nanbox(net::state(socket.value()));
    net::own_set(
        state.get(),
        "httpClientSupplied",
        f64::from_bits(JsValue::from_bool(supplied).bits()),
    );
    unsafe { net::set_codec(socket.value(), PARSER_EDGE, parser.get(), close_parser) };
    attach_http_errors(&socket);
    net::set_route(socket.value(), SUBSYSTEM).map_err(|e| e.message())?;
    if supplied {
        remember_capability(request, &socket);
        let connecting =
            JsValue::from_bits(net::get(socket.value(), "connecting").to_bits()).to_bool();
        if !connecting {
            let effects = with_conn(&socket, protocol::on_connect).unwrap_or_default();
            run(&socket, effects);
            if socket.is_current() {
                net::flow(socket.value());
            }
        }
    } else {
        let host = scope.root_nanbox(f64::from_bits(
            JsValue::from_string_ptr(perry_ffi::alloc_string(&host).as_raw()).bits(),
        ));
        net::connect(
            socket.value(),
            port as f64,
            host.get(),
            f64::from_bits(JsValue::UNDEFINED.bits()),
        );
    }
    if !supplied {
        remember_capability(request, &socket);
    }
    if let Some(ms) = timeout {
        if with_conn(&socket, |conn| conn.request() == Some(request)).unwrap_or(false) {
            net::deadline_arm(socket.value(), ms);
        }
    }
    Ok(())
}

fn remember_capability(request: Handle, socket: &RootedSocket) {
    let capability = net::snapshot(socket.value());
    if let Some(req) = perry_ffi::get_handle_mut::<ClientRequestHandle>(request) {
        req.socket_snapshot = capability;
    }
}

pub(super) fn continue_body(_owner: f64, request: Handle, body: Vec<u8>) {
    let Some(socket) = crate::current_request_socket(request) else {
        return;
    };
    let effects = with_conn(&socket, |conn| {
        let mut effects = Vec::new();
        protocol::continue_body(conn, request, body, &mut effects);
        effects
    })
    .unwrap_or_default();
    run(&socket, effects);
}
pub(super) fn cancel(_owner: f64, request: Handle) {
    let Some(socket) = crate::current_request_socket(request) else {
        return;
    };
    let effects = with_conn(&socket, |conn| protocol::cancel(conn, request)).unwrap_or_default();
    run(&socket, effects);
}

fn run(socket: &RootedSocket, effects: Vec<Effect>) {
    let supplied = JsValue::from_bits(
        net::record_get(net::state(socket.value()), "httpClientSupplied").to_bits(),
    )
    .to_bool();
    let mut effects = VecDeque::from(effects);
    while !effects.is_empty() {
        // Native effects cannot run JS. Keep one proven socket through the
        // batch, then end it before close, TLS hooks, upgrade or retry.
        let failure = if matches!(
            effects.front(),
            Some(Effect::Write(_) | Effect::Deadline(_) | Effect::Park(_))
        ) {
            unsafe {
                socket
                    .with_native_io(|io| {
                        loop {
                            match effects.front() {
                                Some(Effect::Write(_)) if io.can_write_without_js() => {
                                    let Some(Effect::Write(bytes)) = effects.pop_front() else {
                                        unreachable!()
                                    };
                                    if let Err(error) = io.write(&bytes, 0) {
                                        return Some(error);
                                    }
                                }
                                Some(Effect::Deadline(_)) => {
                                    let Some(Effect::Deadline(ms)) = effects.pop_front() else {
                                        unreachable!()
                                    };
                                    io.deadline(ms);
                                }
                                Some(Effect::Park(policy)) => {
                                    if !io.set_ref(false) {
                                        break;
                                    }
                                    let ms = policy.idle_ms;
                                    effects.pop_front();
                                    io.deadline(Some(ms));
                                    #[cfg(test)]
                                    if std::env::var("PERRY_NET_A_CLIENT_SABOTAGE").as_deref()
                                        == Ok("idlepark")
                                    {
                                        // Keep this sabotage at the same semantic boundary.
                                        io.park_deadline();
                                    }
                                }
                                _ => break,
                            }
                        }
                        None
                    })
                    .flatten()
            }
        } else {
            None
        };
        if let Some(error) = failure {
            effects.extend(
                with_conn(socket, |conn| {
                    protocol::on_error(conn, &error.code, &error.syscall, error.errno as i64)
                })
                .unwrap_or_default(),
            );
        }
        let Some(effect) = effects.pop_front() else {
            break;
        };

        // App outcomes still settle the original request if a Socket listener
        // closed/reopened it; the generational check forbids I/O on its new use.
        match effect {
            Effect::RetireRequest(request) => retire_request(request),
            Effect::Push(event) => {
                if matches!(
                    event,
                    PendingHttpEvent::ResponseEnd { .. } | PendingHttpEvent::Response { .. }
                ) {
                    super::COMPLETED.fetch_add(1, Ordering::Relaxed);
                    if supplied {
                        super::RAW_COMPLETED.fetch_add(1, Ordering::Relaxed);
                    }
                }
                if matches!(event, PendingHttpEvent::Timeout { .. }) {
                    super::TIMED_OUT.fetch_add(1, Ordering::Relaxed);
                }
                push_event(event);
            }
            other if !socket.is_current() => {
                drop(other);
            }
            Effect::Write(bytes) => {
                if let Err(error) = net::write(socket.value(), &bytes, 0) {
                    effects.extend(
                        with_conn(socket, |conn| {
                            protocol::on_error(
                                conn,
                                &error.code,
                                &error.syscall,
                                error.errno as i64,
                            )
                        })
                        .unwrap_or_default(),
                    );
                }
            }
            Effect::InstallTls(plan) => {
                let scope = TransientRootScope::enter();
                let callback = scope.root_addr(perry_ffi::alloc_closure(
                    perry_ffi::js_function_info!(handshaken, 0; with_flags(perry_ffi::FN_BUILTIN)),
                    0,
                ) as i64);
                net::once(
                    socket.value(),
                    "secureConnect",
                    f64::from_bits(JsValue::from_object_ptr(callback.get() as *mut u8).bits()),
                );
                if let Err(error) = net::install_client_tls(
                    socket.value(),
                    plan.config,
                    plan.server_name,
                    plan.metadata,
                ) {
                    effects.extend(
                        with_conn(socket, |conn| {
                            protocol::on_destroy(conn, "ERR_TLS_HANDSHAKE_FAILED", &error)
                        })
                        .unwrap_or_default(),
                    );
                }
            }
            Effect::Ciphertext(bytes) => net::receive_tls(socket.value(), &bytes),
            Effect::Close => net::destroy(socket.value()),
            Effect::Deadline(Some(ms)) => net::deadline_arm(socket.value(), ms),
            Effect::Deadline(None) => net::deadline_cancel(socket.value()),
            Effect::Park(policy) => {
                net::set_ref(socket.value(), false);
                net::deadline_arm(socket.value(), policy.idle_ms);
                #[cfg(test)]
                if std::env::var("PERRY_NET_A_CLIENT_SABOTAGE").as_deref() == Ok("idlepark") {
                    net::deadline_park(socket.value());
                }
            }
            Effect::Upgrade {
                request,
                status,
                reason,
                headers,
                head,
            } => {
                if supplied {
                    super::RAW_COMPLETED.fetch_add(1, Ordering::Relaxed);
                }
                #[cfg(test)]
                let store_route =
                    std::env::var("PERRY_NET_A_CLIENT_SABOTAGE").as_deref() != Ok("route");
                #[cfg(not(test))]
                let store_route = true;
                if let Err(error) = if store_route {
                    net::set_route(socket.value(), net::ROUTE)
                } else {
                    Ok(())
                } {
                    push_event(PendingHttpEvent::CodedError {
                        request_handle: request,
                        message: error.message(),
                        code: error.code,
                    });
                    net::destroy(socket.value());
                    continue;
                }
                dispose_parser(socket);
                detach_http_errors(socket);
                #[cfg(test)]
                let head = if std::env::var("PERRY_NET_A_CLIENT_SABOTAGE").as_deref() == Ok("head")
                {
                    Vec::new()
                } else {
                    head
                };
                push_event(PendingHttpEvent::Upgrade {
                    request_handle: request,
                    status,
                    status_message: reason,
                    headers,
                    socket_handle: JsValue::from_bits(socket.value().to_bits()).as_pointer::<u8>()
                        as i64,
                    head,
                });
            }
            Effect::Redispatch(out) => retry(socket, *out),
        }
    }
}

// HTTP owns Socket errors until upgrade, just as Node's connection handler
// does. Protocol effects already deliver the request error. The ordinary
// Socket listener prevents a second unhandled emitter error; it leaves with
// HTTP, so an upgraded Socket follows the user's own error listeners.
unsafe extern "C" fn http_socket_error(
    _: *const perry_ffi::RawClosureHeader,
    _: perry_ffi::JsThis,
    _: f64,
) -> f64 {
    f64::from_bits(JsValue::UNDEFINED.bits())
}

fn detach_http_errors(socket: &RootedSocket) {
    let scope = TransientRootScope::enter();
    let state = scope.root_nanbox(net::state(socket.value()));
    let error = scope.root_nanbox(net::record_get(state.get(), "httpErrorListener"));
    let close = scope.root_nanbox(net::record_get(state.get(), "httpCloseListener"));
    net::own_set(
        state.get(),
        "httpErrorListener",
        f64::from_bits(JsValue::UNDEFINED.bits()),
    );
    net::own_set(
        state.get(),
        "httpCloseListener",
        f64::from_bits(JsValue::UNDEFINED.bits()),
    );
    net::remove_listener(socket.value(), "error", error.get());
    net::remove_listener(socket.value(), "close", close.get());
}

unsafe extern "C" fn http_socket_closed(
    closure: *const perry_ffi::RawClosureHeader,
    _: perry_ffi::JsThis,
    _: f64,
) -> f64 {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(perry_ffi::closure_capture_f64(closure, 0));
    let error = scope.root_nanbox(perry_ffi::closure_capture_f64(closure, 1));
    let state = scope.root_nanbox(net::state(owner.get()));
    net::remove_listener(owner.get(), "error", error.get());
    // A previous listener may already have reopened the Socket and installed
    // another handler. Remove only the handler this closure actually owns.
    if net::record_get(state.get(), "httpErrorListener").to_bits() == error.get().to_bits() {
        net::own_set(
            state.get(),
            "httpErrorListener",
            f64::from_bits(JsValue::UNDEFINED.bits()),
        );
        net::own_set(
            state.get(),
            "httpCloseListener",
            f64::from_bits(JsValue::UNDEFINED.bits()),
        );
    }
    f64::from_bits(JsValue::UNDEFINED.bits())
}

fn attach_http_errors(socket: &RootedSocket) {
    detach_http_errors(socket);
    let scope = TransientRootScope::enter();
    let error = scope.root_addr(perry_ffi::alloc_closure(
        perry_ffi::js_function_info!(http_socket_error, 1; with_flags(perry_ffi::FN_BUILTIN)),
        0,
    ) as i64);
    let close = scope.root_addr(perry_ffi::alloc_closure(
        perry_ffi::js_function_info!(http_socket_closed, 1; with_flags(perry_ffi::FN_BUILTIN)),
        2,
    ) as i64);
    let error_value = || f64::from_bits(JsValue::from_object_ptr(error.get() as *mut u8).bits());
    unsafe {
        perry_ffi::set_closure_capture_f64(
            close.get() as *mut perry_ffi::RawClosureHeader,
            0,
            socket.value(),
        );
        perry_ffi::set_closure_capture_f64(
            close.get() as *mut perry_ffi::RawClosureHeader,
            1,
            error_value(),
        );
    }
    let state = scope.root_nanbox(net::state(socket.value()));
    net::own_set(state.get(), "httpErrorListener", error_value());
    net::own_set(
        state.get(),
        "httpCloseListener",
        f64::from_bits(JsValue::from_object_ptr(close.get() as *mut u8).bits()),
    );
    net::once(socket.value(), "error", error_value());
    net::once(
        socket.value(),
        "close",
        f64::from_bits(JsValue::from_object_ptr(close.get() as *mut u8).bits()),
    );
}

unsafe extern "C" fn handshaken(
    _: *const perry_ffi::RawClosureHeader,
    _: perry_ffi::JsThis,
) -> f64 {
    super::HANDSHAKES.fetch_add(1, Ordering::Relaxed);
    f64::from_bits(JsValue::UNDEFINED.bits())
}

fn replace_socket(previous: &RootedSocket, outbound: Outbound, retry: bool) -> Result<(), String> {
    let request = outbound.request_handle;
    let Some((agent, key)) = perry_ffi::get_handle_mut::<ClientRequestHandle>(request)
        .filter(|req| !req.completed)
        .map(|req| (req.agent_handle, req.agent_key.clone()))
    else {
        return Err("request already completed".into());
    };
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(net::new_socket(
        SUBSYSTEM,
        f64::from_bits(JsValue::UNDEFINED.bits()),
    ));
    let old = JsValue::from_bits(previous.value().to_bits()).as_pointer::<u8>() as i64;
    let raw = JsValue::from_bits(owner.get().to_bits()).as_pointer::<u8>() as i64;
    if let Some(req) = perry_ffi::get_handle_mut::<ClientRequestHandle>(request) {
        req.socket_handle = raw;
        req.socket_snapshot = None;
        req.reused_socket = false;
    }
    if let Some(owner_agent) = perry_ffi::get_handle_mut::<crate::agent::AgentHandle>(agent) {
        if let Some(sockets) = owner_agent.active_socket_handles.get_mut(&key) {
            for socket in sockets {
                if *socket == old {
                    *socket = raw;
                }
            }
        }
    }
    if agent != 0 {
        crate::agent::track_agent_socket(agent, raw);
    }
    if previous.is_current() {
        net::destroy(previous.value());
    }
    push_event(PendingHttpEvent::Socket {
        request_handle: request,
    });
    start_on_socket(owner.get(), outbound, None, retry)
}

fn retry(previous: &RootedSocket, outbound: Outbound) {
    let request = outbound.request_handle;
    if let Err(message) = replace_socket(previous, outbound, true) {
        retire_request(request);
        push_event(PendingHttpEvent::Error {
            request_handle: request,
            error_message: message,
        });
    }
}

extern "C" fn sink(completion: *const tl::NetCompletion) {
    if completion.is_null() {
        return;
    }
    let event = unsafe { &*completion };
    let Some(link) = event.link() else { return };
    let Some(owner) = (unsafe { np::link_event_owner(link) }) else {
        return;
    };
    if event.kind == tl::NET_DATA {
        let bytes = unsafe { event.bytes() };
        let received = unsafe {
            RootedSocket::receive(
                owner,
                event.flags & tl::NET_FLAG_PLAINTEXT != 0,
                bytes.len(),
                false,
                |_, state| {
                    let parser = net::record_get(state, PARSER_EDGE);
                    np::payload_mut::<Conn>(parser, &PARSER)
                        .ok()
                        .map(|conn| protocol::on_data(conn, bytes))
                        .unwrap_or_default()
                },
            )
        };
        match received {
            Some((socket, Some(effects))) => run(&socket, effects),
            Some((socket, None)) => net::receive_tls(socket.value(), bytes),
            None => {}
        }
        return;
    }
    let socket = RootedSocket::new(owner);
    match event.kind {
        tl::NET_CONNECT => {
            unsafe {
                net::dispatch_common(event);
            }
            let effects = with_conn(&socket, protocol::on_connect).unwrap_or_default();
            run(&socket, effects);
        }
        tl::NET_EOF => {
            let effects = with_conn(&socket, protocol::on_eof).unwrap_or_default();
            net::eof(socket.value());
            run(&socket, effects);
        }
        tl::NET_TIMER => {
            let effects = with_conn(&socket, protocol::on_timer).unwrap_or_default();
            run(&socket, effects);
        }
        tl::NET_ERROR => {
            let code = unsafe { event.code() }.unwrap_or("EIO");
            let syscall = unsafe { event.syscall() }.unwrap_or("read");
            let effects = with_conn(&socket, |conn| {
                protocol::on_error(conn, code, syscall, event.errno as i64)
            })
            .unwrap_or_default();
            net::destroy_error(socket.value(), code, &format!("{syscall} {code}"));
            run(&socket, effects);
        }
        tl::NET_CLOSED => {
            if event.flags & tl::NET_FLAG_STALE == 0 {
                let effects = with_conn(&socket, protocol::on_closed).unwrap_or_default();
                dispose_parser(&socket);
                run(&socket, effects);
            }
            unsafe {
                net::dispatch_common(event);
            }
        }
        _ => unsafe {
            net::dispatch_common(event);
        },
    }
}

#[cfg(test)]
#[path = "native_client_tests.rs"]
mod tests;
