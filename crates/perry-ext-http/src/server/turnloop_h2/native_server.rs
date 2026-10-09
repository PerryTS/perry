//! Server HTTP/2 codecs live on the ordinary Socket they parse. ALPN changes
//! that Socket's route and codec; its cell and pending read never change.

use super::conn::{self, H2Conn, Timer};
use super::target::Target;
use crate::server::http2_session_settings::Http2SettingsState;
use perry_ext_net::native_transport::{self as net, RootedSocket};
use perry_ffi::native_payload::{self as np, PayloadFamily};
use perry_ffi::turnloop_net as tl;
use perry_ffi::{JsThis, JsValue, RawClosureHeader, TransientRootScope};
use turnloop_http::http2::Role;

const LINK_SUBSYSTEM: u8 = 15;
static FAMILY_VTABLE: perry_ffi::native_stream::PayloadVTable =
    perry_ffi::native_stream::payload_vtable::<H2Conn>(None);
static FAMILY: PayloadFamily = PayloadFamily::new::<H2Conn>(
    perry_ffi::native_class_ids::HTTP2_CODEC,
    "HTTP2Codec",
    false,
    &FAMILY_VTABLE,
)
.with_constructor_length(0);

fn with_codec<R>(socket: &RootedSocket, f: impl FnOnce(&mut H2Conn) -> R) -> Option<R> {
    if !socket.is_current() {
        return None;
    }
    let scope = TransientRootScope::enter();
    let state = scope.root_nanbox(net::state(socket.value()));
    let codec = scope.root_nanbox(net::own_get(state.get(), "h2"));
    unsafe { np::payload_mut::<H2Conn>(codec.get(), &FAMILY).ok().map(f) }
}

pub(super) fn peek_session<R>(session: i64, f: impl FnOnce(&H2Conn) -> R) -> Option<R> {
    let socket = Target::Session(session).socket()?;
    with_codec(&socket, |codec| f(codec))
}

pub(super) fn with_session<R>(session: i64, f: impl FnOnce(&mut H2Conn) -> R) -> Option<R> {
    let socket = Target::Session(session).socket()?;
    let (result, wire, deadline) = with_codec(&socket, |codec| {
        let result = f(codec);
        (
            result,
            std::mem::take(&mut codec.wire),
            codec.deadline.take(),
        )
    })?;
    // The borrow ends before writes, TLS processing, GC or event callbacks.
    for bytes in wire {
        if !socket.is_current() {
            return Some(result);
        }
        if net::write(socket.value(), &bytes, 0).is_err() {
            net::destroy(socket.value());
            return Some(result);
        }
    }
    if socket.is_current() {
        match deadline {
            Some(Some(ms)) => net::deadline_arm(socket.value(), ms),
            Some(None) => net::deadline_cancel(socket.value()),
            None => (),
        }
    }
    Some(result)
}

unsafe fn release(owner: f64) {
    let socket = RootedSocket::new(owner);
    let terminal = with_codec(&socket, |codec| {
        codec.destroyed = true;
        let streams: Vec<_> = codec.streams.iter().map(|stream| stream.h2_id).collect();
        for stream in streams {
            super::stream::terminate(codec, stream, Some("ECONNRESET"));
        }
        codec.session_handle
    });
    if let Some(session) = terminal {
        crate::server::http2_server::mark_turnloop_session_closed(session);
    }
    let scope = TransientRootScope::enter();
    let state = scope.root_nanbox(net::state(socket.value()));
    let codec = scope.root_nanbox(net::own_get(state.get(), "h2"));
    np::close(codec.get(), &FAMILY);
}

unsafe extern "C" fn accepted(closure: *const RawClosureHeader, _: JsThis, owner: f64) -> f64 {
    let socket = RootedSocket::new(owner);
    let server_handle = perry_ffi::closure_capture_f64(closure, 0) as i64;
    let Some((settings, allow_http1, secure)) = perry_ffi::get_handle::<
        crate::server::http2_server::Http2SecureServer,
    >(server_handle)
    .map(|server| {
        (
            server.settings.clone(),
            server.allow_http1,
            !server.plaintext,
        )
    }) else {
        net::destroy(socket.value());
        return undefined();
    };
    let peer = net::endpoint(socket.value(), true);
    let session = crate::server::http2_server::register_turnloop_server_session(
        server_handle,
        socket.value(),
        peer.as_ref().map(|peer| peer.port).unwrap_or(0),
        secure,
        if secure { "h2" } else { "h2c" },
        conn::advertised_settings(&settings),
    );
    let scope = TransientRootScope::enter();
    let codec = scope.root_nanbox(unsafe {
        np::alloc_in(
            &FAMILY,
            "",
            H2Conn {
                id: 0,
                wire: Vec::new(),
                deadline: None,
                role: Role::Server,
                server_handle,
                session_handle: session,
                core: None,
                input: Vec::with_capacity(16 * 1024),
                streams: Vec::new(),
                secure,
                handshaking: secure,
                connecting: false,
                client_tls: None,
                tls_session: None,
                alpn: None,
                peer_address: peer
                    .as_ref()
                    .map(|peer| peer.address.clone())
                    .unwrap_or_default(),
                peer_port: peer.as_ref().map(|peer| peer.port).unwrap_or(0),
                buffered: 0,
                max_session_memory: 10 * 1024 * 1024,
                timer: Timer::None,
                draining: false,
                closing: false,
                read_eof: false,
                destroyed: false,
                queued_opens: Vec::new(),
                allow_http1,
                settings,
                preface_done: false,
                core_settings_acked: false,
                owed_settings_acks: 0,
                goaway_opaque: Vec::new(),
                peer_settings: None,
                pending_controls: Vec::new(),
            },
            std::mem::size_of::<H2Conn>() + 16 * 1024,
            &[],
        )
    });
    net::set_codec(socket.value(), "h2", codec.get(), release);
    if net::set_route(socket.value(), LINK_SUBSYSTEM).is_err() {
        net::destroy(socket.value());
        return undefined();
    }
    if secure {
        let callback = scope.root_addr(perry_ffi::alloc_closure(
            perry_ffi::js_function_info!(secure_ready, 0; with_flags(perry_ffi::FN_BUILTIN)),
            1,
        ) as i64);
        perry_ffi::set_closure_capture_f64(
            callback.get() as *mut RawClosureHeader,
            0,
            session as f64,
        );
        net::once(
            socket.value(),
            "secure",
            f64::from_bits(JsValue::from_object_ptr(callback.get() as *mut u8).bits()),
        );
    } else {
        with_session(session, |codec| {
            conn::start_core(codec);
            conn::flush(codec);
        });
    }
    crate::server::server::emit_connection(server_handle, socket.value());
    undefined()
}

unsafe extern "C" fn secure_ready(closure: *const RawClosureHeader, _: JsThis) -> f64 {
    let session = perry_ffi::closure_capture_f64(closure, 0) as i64;
    let Some(socket) = Target::Session(session).socket() else {
        return undefined();
    };
    let scope = TransientRootScope::enter();
    let state = scope.root_nanbox(net::state(socket.value()));
    let alpn = net::own_get(state.get(), "alpnProtocol");
    let alpn = crate::server::types::jsvalue_to_owned_string(alpn);
    let choice = with_codec(&socket, |codec| {
        codec.handshaking = false;
        match alpn.as_deref() {
            Some("http/1.1" | "http/1.0") if codec.allow_http1 => {
                Some((codec.server_handle, std::mem::take(&mut codec.input)))
            }
            Some("h2") | None => {
                conn::start_core(codec);
                None
            }
            _ => {
                codec.destroyed = true;
                None
            }
        }
    });
    if let Some(Some((server, leftover))) = choice {
        release(socket.value());
        // The HTTP/1 initializer performs the single route store. No read is resubmitted.
        crate::server::turnloop_serve::adopt_alpn_http1(socket.value(), server, leftover);
    } else if with_codec(&socket, |codec| codec.destroyed).unwrap_or(true) {
        net::destroy(socket.value());
    } else {
        conn::flush_id(Target::Session(session));
    }
    undefined()
}

extern "C" fn sink(event: *const tl::NetCompletion) {
    let Some(event) = (unsafe { event.as_ref() }) else {
        return;
    };
    let Some(link) = event.link() else { return };
    let Some(owner) = (unsafe { np::link_event_owner(link) }) else {
        return;
    };
    let socket = RootedSocket::new(owner);
    let session = with_codec(&socket, |codec| codec.session_handle);
    match event.kind {
        tl::NET_DATA => {
            let bytes = unsafe { event.bytes() };
            if net::tls_installed(socket.value()) && event.flags & tl::NET_FLAG_PLAINTEXT == 0 {
                net::receive_tls(socket.value(), bytes);
            } else if let Some(session) = session {
                net::received(socket.value(), bytes.len());
                conn::feed(Target::Session(session), bytes);
            }
        }
        tl::NET_EOF => {
            if let Some(session) = session {
                conn::on_eof(Target::Session(session));
            }
        }
        tl::NET_TIMER => {
            if let Some(session) = session {
                conn::on_timer(Target::Session(session));
            }
        }
        tl::NET_ERROR => {
            if let Some(session) = session {
                conn::on_error(Target::Session(session), unsafe { event.code() });
            }
        }
        tl::NET_SHUTDOWN => {
            unsafe {
                net::dispatch_common(event);
            }
            if socket.is_current() {
                net::destroy(socket.value());
            }
        }
        _ => unsafe {
            net::dispatch_common(event);
        },
    }
}

fn undefined() -> f64 {
    f64::from_bits(JsValue::UNDEFINED.bits())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn listen(
    server_handle: i64,
    host: &str,
    port: u16,
    backlog: u32,
    tls: Option<std::sync::Arc<rustls::ServerConfig>>,
    _allow_http1: bool,
    _settings: Http2SettingsState,
    _max_session_memory: usize,
    reuse_port: bool,
    no_delay: bool,
) -> Result<(f64, u16, String), tl::NetError> {
    static REGISTERED: std::sync::Once = std::sync::Once::new();
    REGISTERED.call_once(|| assert!(tl::register_link_sink(LINK_SUBSYSTEM, sink)));
    let scope = TransientRootScope::enter();
    let existing =
        crate::server::server::with_base_server(server_handle, |server| server.transport_listener)
            .ok_or_else(|| tl::error_from_os(None, "listen"))?;
    let listener = scope.root_nanbox(if net::server_link(existing).is_ok() {
        existing
    } else {
        let callback = scope.root_addr(perry_ffi::alloc_closure(
            perry_ffi::js_function_info!(accepted, 1; with_flags(perry_ffi::FN_BUILTIN)),
            1,
        ) as i64);
        unsafe {
            perry_ffi::set_closure_capture_f64(
                callback.get() as *mut RawClosureHeader,
                0,
                server_handle as f64,
            );
        }
        net::new_server(
            undefined(),
            f64::from_bits(JsValue::from_object_ptr(callback.get() as *mut u8).bits()),
        )
    });
    crate::server::server::with_base_server_mut(server_handle, |server| {
        server.transport_listener = listener.get()
    });
    let bound = net::listen_tcp(
        listener.get(),
        host,
        port,
        backlog,
        reuse_port,
        no_delay,
        tls,
    )?;
    Ok((listener.get(), bound.port, bound.address))
}
pub(crate) fn close_listener(listener: f64) {
    net::close_server(listener);
}
