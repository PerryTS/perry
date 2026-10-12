//! `ws.handleUpgrade` on the untouched Socket supplied by node:http.
//! The host supplies owned request metadata through a code pointer; ws never
//! depends on HTTP and never retains JS values in a Rust side table.
use perry_ext_net::native_transport::{self as net, RootedSocket};
use perry_ffi::{JsValue, TransientRootScope};
use std::sync::OnceLock;
use turnloop_http::http1::Head;

type RequestReader = fn(f64) -> Option<Head>;
static REQUEST_READER: OnceLock<RequestReader> = OnceLock::new();

/// Install the HTTP host request reader without a reverse crate dependency.
pub fn register_http_upgrade_reader(reader: RequestReader) {
    let _ = REQUEST_READER.set(reader);
}

/// Complete the handshake only after the application's upgrade callback has
/// accepted the request. The raw socket remains the transport, including TLS.
pub(crate) fn accept(request: f64, owner: f64, head: f64) -> Option<usize> {
    let scope = TransientRootScope::enter();
    let head = scope.root_nanbox(head);
    let socket = RootedSocket::new(owner);
    if socket.link().is_err() || !socket.is_current() {
        return None;
    }
    let state = scope.root_nanbox(net::state(socket.value()));
    // A second handleUpgrade must not send a second 101 or replace its codec.
    if !JsValue::from_bits(net::own_get(state.get(), "ws").to_bits()).is_undefined() {
        return None;
    }
    let request = REQUEST_READER.get().and_then(|read| read(request))?;
    let response = match crate::turnloop_link::accept_response(&request, &[]) {
        Ok((response, _)) => response,
        Err(error) => {
            let response = crate::turnloop_link::reject_response(400, &error.message);
            if net::write(socket.value(), &response, 0).is_err()
                || net::shutdown(socket.value(), 0).is_err()
            {
                net::destroy(socket.value());
            }
            return None;
        }
    };
    let leftover = perry_ffi::bytes::no_gc(|borrow| {
        perry_ffi::value_byte_slice(JsValue::from_bits(head.get().to_bits()), borrow)
            .map(|bytes| bytes.to_vec())
    })
    .unwrap_or_default();
    if net::write(socket.value(), &response, 0).is_err() {
        net::destroy(socket.value());
        return None;
    }
    let id = crate::native_socket::adopt(socket.value(), &leftover);
    (id != 0).then_some(id as usize)
}
