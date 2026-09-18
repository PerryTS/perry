//! The WebSocket opening handshake, with no I/O of its own.
//!
//! # The answer to "does the handshake need an owned stream?"
//!
//! No. RFC 6455's opening handshake is an HTTP/1.1 request and a `101`, and
//! both directions here are pure functions over bytes:
//!
//! * the client builds a request head ([`ClientUpgrade::start`]) and verifies
//!   the response head ([`ClientUpgrade::receive`]);
//! * the server validates a request head and returns the `101` to write
//!   ([`accept`]).
//!
//! `turnloop_websocket` supplies the protocol decisions (nonce encoding,
//! `Sec-WebSocket-Accept` derivation, subprotocol negotiation) and
//! `turnloop_http::http1` the framing. Neither touches a socket. What used to
//! need an owned stream was `tokio_tungstenite::WebSocketStream<S>`, whose
//! `S: AsyncRead + AsyncWrite` bound is an API shape of that crate rather than
//! a requirement of the protocol — which is why this module can sit equally on
//! a turnloop handle id and on a tokio stream.

use turnloop_http::http1::{self, BodyLength, Encoder, Event, Head, Limits, Mode};
use turnloop_websocket::ClientHandshake;

/// A failed handshake, in the shape `ws` reports to JS.
#[derive(Debug, Clone)]
pub struct HandshakeError {
    pub code: &'static str,
    pub message: String,
}

impl HandshakeError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            code: "WS_ERR_INVALID_HANDSHAKE",
            message: message.into(),
        }
    }
}

impl std::fmt::Display for HandshakeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// Reads exactly one HTTP head out of a byte stream, keeping whatever followed
/// it — which for an upgrade is already WebSocket frame data and must not be
/// dropped. Used in `Mode::Response` by the client and `Mode::Request` by the
/// server.
pub struct HeadReader {
    decoder: http1::Decoder,
    buffer: Vec<u8>,
    done: bool,
}

impl HeadReader {
    pub fn new(mode: Mode) -> Self {
        let mut decoder = http1::Decoder::new(mode, Limits::default());
        if mode == Mode::Response {
            // The upgrade request is a GET, so the decoder must not expect a
            // HEAD response's framing.
            decoder.response_to("GET");
        }
        Self {
            decoder,
            buffer: Vec::new(),
            done: false,
        }
    }

    /// Feed bytes. `Ok(Some(head))` once the head is complete; the bytes that
    /// followed it are then available from [`HeadReader::into_leftover`].
    ///
    /// The loop has the same shape as the codec's: `consumed == 0` with no
    /// event is the only "wait", and an `Informational` head (a `1xx` before
    /// the `101`) is skipped rather than returned.
    pub fn receive(&mut self, bytes: &[u8]) -> Result<Option<Head>, HandshakeError> {
        if self.done {
            self.buffer.extend_from_slice(bytes);
            return Ok(None);
        }
        self.buffer.extend_from_slice(bytes);
        let mut offset = 0usize;
        let mut head = None;
        while offset < self.buffer.len() {
            let step = self
                .decoder
                .receive(&self.buffer[offset..])
                .map_err(|e| HandshakeError::new(format!("invalid upgrade response: {e}")))?;
            offset += step.consumed;
            match step.event {
                Some(Event::Head(h)) => {
                    head = Some(h);
                    break;
                }
                Some(Event::Informational(_)) => continue,
                None if step.consumed == 0 => break,
                _ => continue,
            }
        }
        self.buffer.drain(..offset);
        if head.is_some() {
            self.done = true;
        }
        Ok(head)
    }

    /// The bytes that arrived after the head — the first WebSocket frames.
    pub fn into_leftover(self) -> Vec<u8> {
        self.buffer
    }
}

/// The client half of the handshake.
pub struct ClientUpgrade {
    handshake: ClientHandshake,
    reader: HeadReader,
}

/// What a completed client handshake yields.
pub struct Upgraded {
    /// The subprotocol the server selected, if any.
    pub protocol: Option<String>,
    /// Bytes that followed the `101` in the same read: already frame data.
    pub leftover: Vec<u8>,
}

impl ClientUpgrade {
    /// Build the upgrade request. `nonce` must be 16 cryptographically random
    /// bytes — RFC 6455 §4.1 requires it to be unpredictable, because a
    /// guessable key lets an attacker who can make this client issue a request
    /// convince a cache that the `101` belongs to an ordinary GET.
    pub fn start(
        authority: &str,
        target: &str,
        nonce: [u8; 16],
        protocols: Vec<String>,
        extra_headers: &[(String, String)],
    ) -> Result<(Self, Vec<u8>), HandshakeError> {
        let (handshake, mut head) = ClientHandshake::new(authority, target, nonce, protocols)
            .map_err(|e| HandshakeError {
                code: "WS_ERR_INVALID_HANDSHAKE",
                message: e.to_string(),
            })?;
        for (name, value) in extra_headers {
            // A caller header never replaces a handshake header: `ws` lets
            // `options.headers` add to the request, not rewrite the protocol.
            let lower = name.to_ascii_lowercase();
            if matches!(
                lower.as_str(),
                "host"
                    | "connection"
                    | "upgrade"
                    | "sec-websocket-key"
                    | "sec-websocket-version"
                    | "sec-websocket-protocol"
                    | "sec-websocket-extensions"
            ) {
                continue;
            }
            head.headers.push(http1::Header::new(&lower, value));
        }
        let mut out = Vec::new();
        let mut encoder = Encoder::start(&head, BodyLength::Empty, &mut out)
            .map_err(|e| HandshakeError::new(format!("invalid upgrade request: {e}")))?;
        encoder
            .finish(&[], &mut out)
            .map_err(|e| HandshakeError::new(format!("invalid upgrade request: {e}")))?;
        Ok((
            Self {
                handshake,
                reader: HeadReader::new(Mode::Response),
            },
            out,
        ))
    }

    /// Feed response bytes. `Ok(Some(_))` once the `101` has been verified.
    pub fn receive(&mut self, bytes: &[u8]) -> Result<Option<Upgraded>, HandshakeError> {
        let Some(head) = self.reader.receive(bytes)? else {
            return Ok(None);
        };
        let protocol = self.handshake.verify(&head).map_err(|e| HandshakeError {
            code: "WS_ERR_INVALID_HANDSHAKE",
            message: format!("Unexpected server response: {} ({})", head.status, e),
        })?;
        let reader = std::mem::replace(&mut self.reader, HeadReader::new(Mode::Response));
        Ok(Some(Upgraded {
            protocol,
            leftover: reader.into_leftover(),
        }))
    }
}

/// The server half: validate an upgrade request head and encode the `101`.
///
/// `protocols` is the server's offered subprotocol list, in preference order.
/// Returns the response bytes to write and the selected subprotocol.
pub fn accept(
    request: &Head,
    protocols: &[&str],
) -> Result<(Vec<u8>, Option<String>), HandshakeError> {
    let (head, selected) =
        turnloop_websocket::accept(request, protocols).map_err(|e| HandshakeError {
            code: "WS_ERR_INVALID_HANDSHAKE",
            message: e.to_string(),
        })?;
    let mut out = Vec::new();
    let mut encoder = Encoder::start(&head, BodyLength::Empty, &mut out)
        .map_err(|e| HandshakeError::new(format!("cannot encode 101: {e}")))?;
    encoder
        .finish(&[], &mut out)
        .map_err(|e| HandshakeError::new(format!("cannot encode 101: {e}")))?;
    Ok((out, selected))
}

/// The canned response `ws` sends when a handshake is refused.
pub fn reject(status: u16, message: &str) -> Vec<u8> {
    let body = message.as_bytes();
    let head = Head {
        method: String::new(),
        target: String::new(),
        status,
        version: 1,
        headers: vec![
            http1::Header::new("connection", "close"),
            http1::Header::new("content-type", "text/html"),
        ],
        keep_alive: false,
    };
    let mut out = Vec::new();
    match Encoder::start(&head, BodyLength::Known(body.len() as u64), &mut out) {
        Ok(mut encoder) => {
            let _ = encoder.body(body, &mut out);
            let _ = encoder.finish(&[], &mut out);
        }
        Err(_) => {
            out.clear();
            out.extend_from_slice(b"HTTP/1.1 400 Bad Request\r\nconnection: close\r\n\r\n");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request_head(bytes: &[u8]) -> Head {
        let mut reader = HeadReader::new(Mode::Request);
        reader.receive(bytes).unwrap().expect("a complete head")
    }

    /// The acceptance case: a real client request in, a real `101` out, with
    /// no socket anywhere in the call.
    #[test]
    fn a_server_accept_is_a_pure_function_of_the_request_head() {
        let head = request_head(
            b"GET /chat HTTP/1.1\r\nHost: h\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
              Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n",
        );
        let (bytes, protocol) = accept(&head, &[]).unwrap();
        let text = String::from_utf8(bytes).unwrap();
        assert!(
            text.starts_with("HTTP/1.1 101 Switching Protocols\r\n"),
            "{text}"
        );
        // RFC 6455 §1.3's worked example.
        assert!(
            text.contains("sec-websocket-accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo="),
            "{text}"
        );
        assert_eq!(protocol, None);
    }

    #[test]
    fn a_subprotocol_is_negotiated_in_the_servers_preference_order() {
        let head = request_head(
            b"GET / HTTP/1.1\r\nHost: h\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
              Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\
              Sec-WebSocket-Protocol: chat, superchat\r\n\r\n",
        );
        let (bytes, protocol) = accept(&head, &["superchat", "chat"]).unwrap();
        assert_eq!(protocol.as_deref(), Some("superchat"));
        assert!(String::from_utf8(bytes)
            .unwrap()
            .contains("sec-websocket-protocol: superchat"));
    }

    #[test]
    fn a_request_without_the_websocket_version_is_refused() {
        let head = request_head(
            b"GET / HTTP/1.1\r\nHost: h\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
              Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n",
        );
        assert!(accept(&head, &[]).is_err());
    }

    /// The client half, end to end against the server half — and the part that
    /// matters for a transport: bytes after the `101` survive.
    #[test]
    fn a_client_handshake_verifies_the_101_and_keeps_the_first_frame_bytes() {
        let (mut client, request) =
            ClientUpgrade::start("example.com", "/chat", [7u8; 16], vec![], &[]).unwrap();
        let head = request_head(&request);
        assert_eq!(head.method, "GET");
        assert_eq!(head.target, "/chat");
        let (response, _) = accept(&head, &[]).unwrap();

        // Split the response so the client sees a partial head first: the
        // `consumed == 0, no event` wait case has to hold here too.
        assert!(client.receive(&response[..12]).unwrap().is_none());
        let mut tail = response[12..].to_vec();
        tail.extend_from_slice(b"\x81\x03abc"); // an unmasked text frame riding along
        let upgraded = client.receive(&tail).unwrap().expect("the 101");
        assert_eq!(upgraded.protocol, None);
        assert_eq!(upgraded.leftover, b"\x81\x03abc");
    }

    #[test]
    fn a_client_rejects_a_wrong_accept_key() {
        let (mut client, _) =
            ClientUpgrade::start("example.com", "/", [1u8; 16], vec![], &[]).unwrap();
        let bad = b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n\
                    Connection: Upgrade\r\nSec-WebSocket-Accept: AAAAAAAAAAAAAAAAAAAAAAAAAAA=\r\n\r\n";
        assert!(client.receive(bad).is_err());
    }

    #[test]
    fn extra_headers_are_added_but_cannot_rewrite_the_protocol_ones() {
        let (_, request) = ClientUpgrade::start(
            "example.com",
            "/",
            [3u8; 16],
            vec![],
            &[
                ("Cookie".into(), "a=b".into()),
                ("Sec-WebSocket-Key".into(), "spoofed".into()),
            ],
        )
        .unwrap();
        let text = String::from_utf8(request).unwrap();
        assert!(text.contains("cookie: a=b"), "{text}");
        assert!(!text.contains("spoofed"), "{text}");
        assert_eq!(text.matches("sec-websocket-key").count(), 1, "{text}");
    }
}
