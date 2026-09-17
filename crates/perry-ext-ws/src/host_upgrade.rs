//! Answering an HTTP upgrade on a connection someone else owns.
//!
//! Two hosts take upgrades off [`perry_http_server`] — this crate's own
//! standalone `WebSocketServer({ port })` and `perry-ext-fastify`'s
//! `app.server.on('upgrade', …)` — and both need the same five steps in the
//! same order. They are here once rather than twice because the *order* is the
//! part that is easy to get wrong and impossible to see in a passing test:
//!
//! 1. validate the request and build the `101` (or the refusal);
//! 2. write it;
//! 3. allocate the JS-visible id;
//! 4. let the host publish whatever routes events for that id, and queue its
//!    own `'connection'` / `'upgrade'` event — this is `prepare`;
//! 5. *then* decode whatever the peer pipelined behind the handshake.
//!
//! Step 5 after step 4 is the whole reason for the callback. A client that
//! writes its first frame in the same packet as its `Sec-WebSocket-Key` is
//! ordinary — `ws` itself does it — and decoding that frame before the host
//! has published its routing either loses the message or delivers it *ahead*
//! of the event that announces the connection.

use crate::codec::Role;
use crate::turnloop_link::{self, Transport};

/// Answer an upgrade and install the WebSocket protocol on the connection.
///
/// `transport` is how this connection's bytes reach the wire; it is used for
/// the `101` itself, so the handshake needs no separate writer and this
/// function does no I/O of its own.
///
/// `prepare` runs with the new `ws_id`, after it exists and before any
/// pipelined frame is decoded. See the module header for why that window
/// exists.
///
/// On refusal the connection is **not** torn down: the caller gets the bytes
/// `ws` answers a malformed upgrade with (a `400`, then close) and decides
/// what to do, because a host may have its own error reporting to do first.
pub fn accept_http_upgrade(
    request: &perry_http_server::Request,
    leftover: &[u8],
    transport: Transport,
    protocols: &[&str],
    prepare: impl FnOnce(usize),
) -> Result<usize, Refusal> {
    let head = turnloop_link::request_head(
        &request.method,
        &request.target,
        request.version,
        &request.headers,
    );
    let (response, _protocol) =
        turnloop_link::accept_response(&head, protocols).map_err(|e| Refusal {
            message: e.message,
            response: turnloop_link::reject_response(400, "Bad Request"),
        })?;
    (transport.write)(request.conn_id, &response);

    let ws_id = crate::allocate_client_id();
    let _ = crate::attach_turnloop_client(ws_id, request.conn_id);
    prepare(ws_id);
    turnloop_link::adopt_existing(request.conn_id, ws_id, transport, Role::Server, leftover);
    Ok(ws_id)
}

/// An upgrade this crate will not complete.
#[derive(Debug)]
pub struct Refusal {
    /// Why, in `ws`'s own words.
    pub message: String,
    /// What to put on the wire. `ws` answers a malformed upgrade with a `400`
    /// and closes, rather than dropping the connection silently.
    pub response: Vec<u8>,
}

/// Route one [`perry_http_server::Upgraded`] event into the protocol layer.
///
/// Returns whether the caller should end the connection: true only for a
/// half-close the WebSocket layer did not already answer, which is the one
/// case where the host has to act. Every other event is fully handled here.
///
/// Having this in one place is the point. Four events, each with a different
/// correct response, spread across two hosts is four chances to answer an
/// `Eof` with `destroy` — which cancels the close frame the codec has just
/// queued and makes the peer report 1006 instead of the code it was sent.
pub fn drive_http_upgraded(conn_id: i64, event: perry_http_server::Upgraded<'_>) -> bool {
    match event {
        perry_http_server::Upgraded::Data(bytes) => {
            turnloop_link::on_data(conn_id, bytes);
            false
        }
        perry_http_server::Upgraded::Eof => turnloop_link::on_eof(conn_id),
        perry_http_server::Upgraded::Error(message) => {
            turnloop_link::on_error(conn_id, message);
            false
        }
        perry_http_server::Upgraded::Closed => {
            turnloop_link::on_closed(conn_id);
            false
        }
    }
}

/// The [`Transport`] for a connection `perry-http-server` owns.
///
/// `finish` rather than `destroy` on the graceful path: see
/// [`perry_http_server::finish`].
pub const HTTP_SERVER_TRANSPORT: Transport = Transport {
    write: perry_http_server::write_raw,
    finish: perry_http_server::finish,
    destroy: perry_http_server::destroy,
};

#[cfg(test)]
mod tests {
    use super::*;

    fn request(headers: &[(&str, &str)]) -> perry_http_server::Request {
        perry_http_server::Request {
            conn_id: -4242,
            seq: 1,
            method: "GET".to_string(),
            target: "/socket".to_string(),
            version: 1,
            headers: headers
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            body: Vec::new(),
            peer_address: "127.0.0.1".to_string(),
            peer_port: 1234,
            expects_continue: false,
            upgrade: true,
            request_number: 1,
        }
    }

    fn valid_headers() -> Vec<(&'static str, &'static str)> {
        vec![
            ("host", "example.test"),
            ("upgrade", "websocket"),
            ("connection", "Upgrade"),
            ("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ=="),
            ("sec-websocket-version", "13"),
        ]
    }

    /// `prepare` must run before a pipelined frame is decoded, or a host that
    /// publishes its routing there delivers the first message to nothing.
    #[test]
    fn prepare_runs_before_the_pipelined_leftover_is_decoded() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static ORDER: AtomicUsize = AtomicUsize::new(0);
        static PREPARED_AT: AtomicUsize = AtomicUsize::new(0);
        static WROTE_AT: AtomicUsize = AtomicUsize::new(0);

        ORDER.store(0, Ordering::SeqCst);
        PREPARED_AT.store(0, Ordering::SeqCst);
        WROTE_AT.store(0, Ordering::SeqCst);

        fn note_write(_id: i64, _bytes: &[u8]) {
            WROTE_AT.store(ORDER.fetch_add(1, Ordering::SeqCst) + 1, Ordering::SeqCst);
        }
        fn noop(_id: i64) {}

        // A masked ping: the codec answers it, so the leftover provably
        // reaches a writer rather than being silently dropped.
        let ping = [0x89u8, 0x80, 0, 0, 0, 0];
        let ws_id = accept_http_upgrade(
            &request(&valid_headers()),
            &ping,
            Transport {
                write: note_write,
                finish: noop,
                destroy: noop,
            },
            &[],
            |_| {
                PREPARED_AT.store(ORDER.fetch_add(1, Ordering::SeqCst) + 1, Ordering::SeqCst);
            },
        )
        .expect("a 101");
        assert!(ws_id > 0);
        // 1: the 101. 2: prepare. 3: the pong the leftover ping provoked.
        assert_eq!(PREPARED_AT.load(Ordering::SeqCst), 2);
        assert_eq!(WROTE_AT.load(Ordering::SeqCst), 3);
    }

    /// A request that is not a valid upgrade produces the bytes `ws` answers
    /// with, and never reaches `prepare`.
    #[test]
    fn a_malformed_upgrade_refuses_without_allocating_a_client() {
        fn unreachable_write(_id: i64, _bytes: &[u8]) {
            panic!("a refused upgrade must not write through the transport");
        }
        fn noop(_id: i64) {}

        let refusal = accept_http_upgrade(
            // No `sec-websocket-key`: `turnloop_websocket::accept` refuses.
            &request(&[("host", "example.test"), ("upgrade", "websocket")]),
            &[],
            Transport {
                write: unreachable_write,
                finish: noop,
                destroy: noop,
            },
            &[],
            |_| panic!("prepare must not run for a refused upgrade"),
        )
        .expect_err("a refusal");
        let text = String::from_utf8(refusal.response).expect("ascii");
        assert!(text.starts_with("HTTP/1.1 400 Bad Request\r\n"), "{text}");
        assert!(text.contains("connection: close"), "{text}");
    }
}
