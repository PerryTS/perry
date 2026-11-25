//! `server.on('clientError', (error, socket) => …)`.
//!
//! The connection layer detects the failure inside the completion sink, which
//! may not run JS, so it routes the socket to `net` and queues the error on its
//! server; the pump emits it here.

use crate::server::request::handle_to_pointer_f64;
use crate::server::server::{take_server_event_listeners, with_base_server_mut};
use crate::server::types::js_promise_run_microtasks;
use perry_ext_net::native_transport as net;
use perry_ffi::{JsValue, TransientRootScope};

/// The decoder refuses a request without exactly one `Host`; node's parser
/// accepts it and the server answers 400 itself.
pub(crate) const MISSING_HOST: &str = "ERR_HTTP_MISSING_HOST";

pub(crate) enum ClientErrorKind {
    /// A parser failure, with the bytes it failed on when there are any.
    Parse {
        reason: &'static str,
        raw_packet: Option<Vec<u8>>,
    },
    Timeout,
    Transport {
        syscall: String,
    },
}

pub(crate) struct ClientError {
    /// Visited by `scan_base_server_roots` while queued.
    pub(crate) socket: f64,
    pub(crate) code: String,
    pub(crate) kind: ClientErrorKind,
    /// The HTTP layer consumed the peer's EOF before the socket moved to `net`.
    pub(crate) read_ended: bool,
}

/// node's answer when no `'clientError'` listener takes the socket.
pub(crate) fn default_response(code: &str) -> &'static [u8] {
    match code {
        "HPE_HEADER_OVERFLOW" => {
            b"HTTP/1.1 431 Request Header Fields Too Large\r\nConnection: close\r\n\r\n"
        }
        "ERR_HTTP_REQUEST_TIMEOUT" => b"HTTP/1.1 408 Request Timeout\r\nConnection: close\r\n\r\n",
        _ => b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n",
    }
}

/// llhttp's code and reason for a request the decoder refused. The decoder
/// reports one code for every malformed head, so the request line is
/// re-examined to tell them apart.
pub(crate) fn parse_failure(
    error: turnloop_http::Error,
    input: &[u8],
    in_body: bool,
) -> (&'static str, &'static str) {
    match error.code {
        "HPE_HEADER_OVERFLOW" => (error.code, "Header overflow"),
        "HPE_INVALID_HEADER_TOKEN" if in_body => {
            ("HPE_INVALID_CHUNK_SIZE", "Invalid character in chunk size")
        }
        "HPE_INVALID_HEADER_TOKEN" if error.message == "HTTP/1.1 requires exactly one Host" => {
            (MISSING_HOST, error.message)
        }
        "HPE_INVALID_HEADER_TOKEN" => malformed_head(input),
        code => (code, error.message),
    }
}

fn malformed_head(input: &[u8]) -> (&'static str, &'static str) {
    let line = input.split(|b| *b == b'\n').next().unwrap_or_default();
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    let mut parts = line.splitn(3, |b| *b == b' ');
    let method = parts.next().unwrap_or_default();
    let target = parts.next();
    let version = parts.next().unwrap_or_default();
    if method.is_empty() || !method.iter().all(|b| b.is_ascii_uppercase() || *b == b'-') {
        return ("HPE_INVALID_METHOD", "Invalid method encountered");
    }
    let Some(target) = target else {
        return ("HPE_INVALID_METHOD", "Invalid method encountered");
    };
    if target.iter().any(|b| *b < b' ' || *b == 0x7f) {
        return ("HPE_INVALID_URL", "Invalid char in url path");
    }
    if !version.starts_with(b"HTTP/") {
        return ("HPE_INVALID_CONSTANT", "Expected HTTP/, RTSP/ or ICE/");
    }
    if version != b"HTTP/1.1" && version != b"HTTP/1.0" {
        return ("HPE_INVALID_VERSION", "Invalid HTTP version");
    }
    ("HPE_INVALID_HEADER_TOKEN", "Invalid header token")
}

fn error_value(error: &ClientError) -> f64 {
    let coded = |message: &str| {
        f64::from_bits(
            perry_ffi::error_value_with_code(message, &error.code, perry_ffi::ErrorKind::Error)
                .bits(),
        )
    };
    match &error.kind {
        ClientErrorKind::Timeout => coded("Request timeout"),
        ClientErrorKind::Transport { syscall } => f64::from_bits(
            perry_ffi::system_error_value(
                &format!("{syscall} {}", error.code),
                &error.code,
                syscall,
                perry_ffi::turnloop_net::errno_for_code(&error.code).into(),
            )
            .bits(),
        ),
        ClientErrorKind::Parse { reason, raw_packet } => {
            let message = match raw_packet {
                Some(_) => format!("Parse Error: {reason}"),
                None => "Parse Error".to_string(),
            };
            let scope = TransientRootScope::enter();
            let value = scope.root_nanbox(coded(&message));
            let reason = scope.root_nanbox(f64::from_bits(
                JsValue::from_string_ptr(perry_ffi::alloc_string(reason).as_raw()).bits(),
            ));
            unsafe { perry_ffi::runtime_set(value.get(), b"reason", reason.get()) };
            if let Some(bytes) = raw_packet {
                let packet = perry_ffi::alloc_buffer(bytes);
                let packet = f64::from_bits(JsValue::from_object_ptr(packet as *mut u8).bits());
                let packet = scope.root_nanbox(packet);
                unsafe { perry_ffi::runtime_set(value.get(), b"rawPacket", packet.get()) };
            }
            value.get()
        }
    }
}

unsafe extern "C" fn ignore_error(
    _: *const perry_ffi::RawClosureHeader,
    _: perry_ffi::JsThis,
    _: f64,
) -> f64 {
    f64::from_bits(JsValue::UNDEFINED.bits())
}

/// node's `this.on('error', noop)`: a listener may `socket.destroy(error)`.
fn ignore_socket_errors(socket: &net::RootedSocket) {
    let scope = TransientRootScope::enter();
    let callback = scope.root_addr(perry_ffi::alloc_closure(
        perry_ffi::js_function_info!(ignore_error, 1; with_flags(perry_ffi::FN_BUILTIN)),
        0,
    ) as i64);
    let event = scope.root_nanbox(f64::from_bits(
        JsValue::from_string_ptr(perry_ffi::alloc_string("error").as_raw()).bits(),
    ));
    let args = [event.get(), handle_to_pointer_f64(callback.get())];
    unsafe { perry_ffi::runtime_call(socket.value(), b"on", &args) };
}

/// Emit every queued `'clientError'` of one server.
pub(crate) fn drain_client_errors(server_handle: i64) -> i32 {
    let mut count = 0;
    while let Some((error, listeners)) = with_base_server_mut(server_handle, |server| {
        let error = server.pending_client_errors.pop_front()?;
        Some((error, take_server_event_listeners(server, "clientError")))
    })
    .flatten()
    {
        count += 1;
        let scope = TransientRootScope::enter();
        let socket = net::RootedSocket::new(error.socket);
        let listeners = scope.root_addrs(&listeners);
        if listeners.is_empty() {
            // The listener went away after the failure was queued.
            if let Some(Ok(_)) = socket.write(default_response(&error.code), 0) {
                let _ = net::shutdown(socket.value(), 0);
            }
        } else {
            let value = scope.root_nanbox(error_value(&error));
            ignore_socket_errors(&socket);
            for callback in &listeners {
                net::call(
                    handle_to_pointer_f64(callback.get()),
                    perry_ffi::JsThis::from_f64(handle_to_pointer_f64(server_handle)),
                    &[value.get(), socket.value()],
                );
                unsafe { js_promise_run_microtasks() };
            }
        }
        // Nothing parses the peer's remaining bytes: discard them so its EOF
        // is seen and the socket can close.
        if net::socket_link(socket.value()).is_ok() && socket.is_current() {
            unsafe { perry_ffi::runtime_call(socket.value(), b"resume", &[]) };
            if error.read_ended {
                // node leaves the write side to the listener.
                let half_open = f64::from_bits(JsValue::from_bool(true).bits());
                net::own_set(socket.value(), "allowHalfOpen", half_open);
                net::eof(socket.value());
            }
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::{default_response, parse_failure};
    use turnloop_http::Error;

    #[test]
    fn a_refused_head_names_the_part_node_refuses() {
        let head = |input: &[u8]| {
            parse_failure(Error::new("HPE_INVALID_HEADER_TOKEN", "head"), input, false).0
        };
        assert_eq!(head(b"GARBAGE\r\n\r\n"), "HPE_INVALID_METHOD");
        assert_eq!(head(b"\x16\x03\x01\r\n\r\n"), "HPE_INVALID_METHOD");
        assert_eq!(head(b"GET /\x01 HTTP/1.1\r\n\r\n"), "HPE_INVALID_URL");
        assert_eq!(head(b"GET / FOO/1.1\r\n\r\n"), "HPE_INVALID_CONSTANT");
        assert_eq!(head(b"GET / HTTP/9.9\r\n\r\n"), "HPE_INVALID_VERSION");
        assert_eq!(
            head(b"GET / HTTP/1.1\r\nb ad: 1\r\n\r\n"),
            "HPE_INVALID_HEADER_TOKEN"
        );
    }

    #[test]
    fn a_refused_chunk_and_an_overflow_keep_their_own_codes() {
        let chunk = Error::new("HPE_INVALID_HEADER_TOKEN", "chunk");
        assert_eq!(
            parse_failure(chunk, b"zz\r\n", true).0,
            "HPE_INVALID_CHUNK_SIZE"
        );
        let overflow = Error::new("HPE_HEADER_OVERFLOW", "head size limit");
        assert_eq!(
            parse_failure(overflow, b"", false),
            ("HPE_HEADER_OVERFLOW", "Header overflow")
        );
    }

    #[test]
    fn the_default_answer_follows_the_code() {
        assert!(default_response("HPE_HEADER_OVERFLOW").starts_with(b"HTTP/1.1 431 "));
        assert!(default_response("ERR_HTTP_REQUEST_TIMEOUT").starts_with(b"HTTP/1.1 408 "));
        assert!(default_response("HPE_INVALID_METHOD").starts_with(b"HTTP/1.1 400 "));
    }
}
