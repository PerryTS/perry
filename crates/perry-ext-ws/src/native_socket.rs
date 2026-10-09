//! Attached WebSocket protocol on an ordinary net.Socket, from HTTP upgrade.
//! The Socket's `ws` JS edge owns this pure Rust codec. No transport id, link
//! registry, retoken or descriptor handover exists on this path.

use crate::codec::{Codec, Incoming, Role};
use perry_ext_net::native_transport::{self as net, RootedSocket};
use perry_ffi::native_payload::{self as np, PayloadFamily};
use perry_ffi::turnloop_net as tl;
use perry_ffi::TransientRootScope;

const LINK_SUBSYSTEM: u8 = 13;
struct SocketCodec {
    ws_id: usize,
    codec: Codec,
    closing: bool,
    write_shut: bool,
    read_eof: bool,
}
static FAMILY_VTABLE: perry_ffi::native_stream::PayloadVTable =
    perry_ffi::native_stream::payload_vtable::<SocketCodec>(None);
static FAMILY: PayloadFamily = PayloadFamily::new::<SocketCodec>(
    perry_ffi::native_class_ids::WS_CODEC,
    "WebSocketCodec",
    false,
    &FAMILY_VTABLE,
)
.with_constructor_length(0);

fn with_codec<R>(socket: &RootedSocket, f: impl FnOnce(&mut SocketCodec) -> R) -> Option<R> {
    if !socket.is_current() {
        return None;
    }
    let scope = TransientRootScope::enter();
    let state = scope.root_nanbox(net::state(socket.value()));
    let codec = scope.root_nanbox(net::own_get(state.get(), "ws"));
    unsafe {
        np::payload_mut::<SocketCodec>(codec.get(), &FAMILY)
            .ok()
            .map(f)
    }
}
unsafe fn release(owner: f64) {
    let socket = RootedSocket::new(owner);
    if let Some((ws_id, code)) = with_codec(&socket, |codec| (codec.ws_id, codec.codec.eof())) {
        crate::connection_closed(
            ws_id,
            code.unwrap_or(crate::codec::CLOSE_ABNORMAL),
            String::new(),
        );
    }
    let scope = TransientRootScope::enter();
    let state = scope.root_nanbox(net::state(socket.value()));
    let codec = scope.root_nanbox(net::own_get(state.get(), "ws"));
    np::close(codec.get(), &FAMILY);
}

/// The logical WS id remains its application's identity. Its transport is the
/// existing Socket, and only one route store changes where its read goes.
pub fn adopt(owner: f64, leftover: &[u8]) -> i64 {
    static REGISTERED: std::sync::Once = std::sync::Once::new();
    REGISTERED.call_once(|| assert!(tl::register_link_sink(LINK_SUBSYSTEM, sink)));
    let socket = RootedSocket::new(owner);
    let ws_id = crate::allocate_client_id();
    let scope = TransientRootScope::enter();
    let codec = scope.root_nanbox(unsafe {
        np::alloc_in(
            &FAMILY,
            "",
            SocketCodec {
                ws_id,
                codec: Codec::new(Role::Server),
                closing: false,
                write_shut: false,
                read_eof: false,
            },
            std::mem::size_of::<SocketCodec>(),
            &[],
        )
    });
    unsafe { net::set_codec(socket.value(), "ws", codec.get(), release) };
    crate::attach_socket_client(ws_id, socket.value());
    if net::set_route(socket.value(), LINK_SUBSYSTEM).is_err() {
        net::destroy(socket.value());
        return 0;
    }
    if !leftover.is_empty() {
        on_data(socket.value(), leftover)
    }
    ws_id as i64
}

fn finish(socket: &RootedSocket) {
    if !socket.is_current() {
        return;
    }
    with_codec(socket, |codec| codec.closing = true);
    if net::shutdown(socket.value(), 0).is_err() {
        net::destroy(socket.value())
    }
}
fn write(socket: &RootedSocket, bytes: &[u8]) {
    if bytes.is_empty() || !socket.is_current() {
        return;
    }
    if net::write(socket.value(), bytes, 0).is_err() {
        net::destroy(socket.value())
    }
}

pub fn on_data(owner: f64, bytes: &[u8]) {
    let socket = RootedSocket::new(owner);
    let Some((ws_id, events, output, terminal)) = with_codec(&socket, |codec| {
        let events = codec.codec.receive(bytes);
        (
            codec.ws_id,
            events,
            codec.codec.take_output(),
            codec.codec.is_terminal(),
        )
    }) else {
        return;
    };
    write(&socket, &output);
    match events {
        Ok(events) => {
            let mut closed = None;
            for event in events {
                if let Incoming::Close(frame) = &event {
                    closed = Some(
                        frame
                            .clone()
                            .unwrap_or((crate::codec::CLOSE_NO_STATUS, String::new())),
                    );
                }
                crate::emit_incoming(ws_id, event);
            }
            if let Some((code, reason)) = closed {
                crate::connection_closed(ws_id, code, reason);
                finish(&socket);
            } else if terminal {
                crate::connection_closed(ws_id, crate::codec::CLOSE_ABNORMAL, String::new());
                finish(&socket);
            }
        }
        Err(error) => {
            crate::connection_error(ws_id, &crate::codec_error_message(&error));
            crate::connection_closed(ws_id, crate::codec::CLOSE_ABNORMAL, String::new());
            finish(&socket);
        }
    }
}
pub fn on_eof(owner: f64) -> bool {
    let socket = RootedSocket::new(owner);
    let Some((ws_id, code, shut, already)) = with_codec(&socket, |codec| {
        let already = codec.read_eof;
        codec.read_eof = true;
        (codec.ws_id, codec.codec.eof(), codec.write_shut, already)
    }) else {
        return false;
    };
    if let Some(code) = code {
        crate::connection_closed(ws_id, code, String::new())
    }
    if shut {
        net::destroy(socket.value())
    } else if !already {
        finish(&socket)
    }
    true
}
pub fn on_error(owner: f64, message: &str) -> bool {
    let socket = RootedSocket::new(owner);
    let Some(ws_id) = with_codec(&socket, |codec| codec.ws_id) else {
        return false;
    };
    crate::connection_error(ws_id, message);
    net::destroy(socket.value());
    true
}

pub(crate) fn command(owner: f64, command: crate::WsCommand) {
    let socket = RootedSocket::new(owner);
    if matches!(command, crate::WsCommand::Terminate) {
        net::destroy(socket.value());
        return;
    }
    let Some((ws_id, result, output, deadline)) = with_codec(&socket, |codec| {
        let result = match command {
            crate::WsCommand::Send(outgoing) => codec.codec.send(outgoing.into_message()),
            crate::WsCommand::Close(code, reason) => {
                codec.closing = true;
                codec.codec.close(code, &reason)
            }
            crate::WsCommand::Terminate => unreachable!(),
        };
        (
            codec.ws_id,
            result,
            codec.codec.take_output(),
            codec.codec.next_timeout(),
        )
    }) else {
        return;
    };
    match result {
        Ok(()) => write(&socket, &output),
        Err(error) => crate::connection_error(ws_id, &crate::codec_error_message(&error)),
    }
    if let Some(deadline) = deadline {
        net::deadline_arm(
            socket.value(),
            deadline
                .saturating_duration_since(std::time::Instant::now())
                .as_millis()
                .max(1) as u64,
        );
    }
}

extern "C" fn sink(event: *const tl::NetCompletion) {
    if event.is_null() {
        return;
    }
    let event = unsafe { &*event };
    let Some(link) = event.link() else { return };
    let Some(owner) = (unsafe { np::link_event_owner(link) }) else {
        return;
    };
    let socket = RootedSocket::new(owner);
    match event.kind {
        tl::NET_DATA => {
            let bytes = unsafe { event.bytes() };
            if event.flags & tl::NET_FLAG_PLAINTEXT == 0 && net::tls_installed(socket.value()) {
                net::receive_tls(socket.value(), bytes);
            } else {
                net::received(socket.value(), bytes.len());
                on_data(socket.value(), bytes);
            }
        }
        tl::NET_EOF => {
            on_eof(socket.value());
        }
        tl::NET_ERROR => {
            on_error(
                socket.value(),
                unsafe { event.code() }.unwrap_or("WS_ERR_SOCKET"),
            );
        }
        tl::NET_SHUTDOWN => {
            let close = with_codec(&socket, |codec| {
                codec.write_shut = true;
                codec.read_eof
            })
            .unwrap_or(false);
            unsafe {
                net::dispatch_common(event);
            }
            if close {
                net::destroy(socket.value())
            }
        }
        tl::NET_TIMER => {
            if let Some((ws_id, Some(code))) = with_codec(&socket, |codec| {
                (
                    codec.ws_id,
                    codec.codec.handle_timeout(std::time::Instant::now()),
                )
            }) {
                crate::connection_closed(ws_id, code, String::new());
                net::destroy(socket.value());
            }
        }
        _ => unsafe {
            net::dispatch_common(event);
        },
    }
}
