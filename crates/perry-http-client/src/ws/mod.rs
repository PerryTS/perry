//! A blocking WebSocket client.
//!
//! The upgrade request and its `101` go through `turnloop_http`'s HTTP/1
//! codec on the same [`crate::transport::Connection`] the HTTP client uses;
//! [`frame`] does RFC 6455 framing on top. See [`frame`]'s header for why the
//! framing is here rather than `turnloop-websocket`.
//!
//! The CLI is the only WebSocket client in the tree that needs no concurrency:
//! it connects, sends one subscribe frame, and reads text until the build
//! finishes — it never sends while a read is outstanding. A blocking
//! `read_message` with a deadline is therefore the whole requirement, not a
//! simplification that loses something.

pub mod frame;

use std::time::Duration;

use turnloop_http::client::Http1Connection;
use turnloop_http::http1::{BodyLength, Event, Head, Header, Limits};
use url::Url;

use crate::transport::{self, Connection as Socket};
use crate::{Error, Result};
pub use frame::Message;

/// How long to wait for the `101` before giving up.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);

/// A connected WebSocket.
pub struct WebSocket {
    socket: Socket,
    decoder: frame::Decoder,
    /// Bytes read from the socket that the decoder has not consumed — starting
    /// with whatever arrived in the same segment as the `101`.
    pending: Vec<u8>,
    closed: bool,
}

impl WebSocket {
    /// Connect to a `ws://` or `wss://` URL.
    pub fn connect(url: &str, timeout: Duration) -> Result<Self> {
        let parsed =
            Url::parse(url).map_err(|e| Error::new(format!("invalid WebSocket URL: {e}")))?;
        let secure = match parsed.scheme() {
            "ws" => false,
            "wss" => true,
            other => return Err(Error::new(format!("unsupported scheme {other:?}"))),
        };
        let host = parsed
            .host_str()
            .ok_or_else(|| Error::new("WebSocket URL has no host"))?
            .to_string();
        let port = parsed
            .port_or_known_default()
            .unwrap_or(if secure { 443 } else { 80 });
        let authority = match parsed.port() {
            Some(p) => format!("{host}:{p}"),
            None => host.clone(),
        };
        let mut target = parsed.path().to_string();
        if target.is_empty() {
            target.push('/');
        }
        if let Some(query) = parsed.query() {
            target.push('?');
            target.push_str(query);
        }

        let addrs = transport::resolve(&host, port).map_err(|e| Error::io("resolve", e))?;
        let mut socket = Socket::connect(&addrs, timeout).map_err(|e| Error::io("connect", e))?;
        let deadline = socket.deadline_in(HANDSHAKE_TIMEOUT.min(timeout));
        if secure {
            let config = crate::tls::client_config()?;
            socket
                .start_tls(config, &host, deadline)
                .map_err(|e| Error::io("TLS handshake", e))?;
        }

        // RFC 6455 §4.1 requires the nonce to be unpredictable: a guessable
        // key lets an attacker who can make this client issue a request
        // convince a cache that the 101 belongs to an ordinary GET.
        let mut nonce = [0u8; 16];
        crate::tls::secure_random(&mut nonce)?;
        let key = frame::encode_key(&nonce);
        let head = Head {
            method: "GET".into(),
            target,
            status: 0,
            version: 1,
            headers: vec![
                Header::new("host", &authority),
                Header::new("connection", "Upgrade"),
                Header::new("upgrade", "websocket"),
                Header::new("sec-websocket-version", "13"),
                Header::new("sec-websocket-key", &key),
            ],
            keep_alive: true,
        };

        let mut http = Http1Connection::new(Limits::default());
        http.start(&head, BodyLength::Empty, Some(deadline), None)
            .map_err(|e| Error::new(format!("WebSocket upgrade: {e}")))?;
        http.finish_body(&[])
            .map_err(|e| Error::new(format!("WebSocket upgrade: {e}")))?;
        let out = http.output().to_vec();
        socket
            .write_all(&out, deadline)
            .map_err(|e| Error::io("WebSocket upgrade", e))?;
        http.consume_output(out.len())
            .map_err(|e| Error::new(format!("WebSocket upgrade: {e}")))?;

        // Read until the response head is complete. Anything after it in the
        // same read is already frame data and must be kept.
        let mut pending: Vec<u8> = Vec::new();
        let mut scratch: Vec<u8> = Vec::new();
        let response = loop {
            scratch.clear();
            let n = socket
                .read(&mut scratch, deadline)
                .map_err(|e| Error::io("WebSocket upgrade response", e))?;
            if n == 0 {
                return Err(Error::new("server closed before completing the upgrade"));
            }
            pending.extend_from_slice(&scratch);
            let mut offset = 0usize;
            let mut found = None;
            while offset < pending.len() {
                let step = http
                    .receive(&pending[offset..])
                    .map_err(|e| Error::new(format!("WebSocket upgrade: {e}")))?;
                offset += step.consumed;
                match step.event {
                    Some(Event::Head(head)) => {
                        found = Some(head);
                        break;
                    }
                    Some(Event::Informational(_)) => continue,
                    None if step.consumed == 0 => break,
                    _ => continue,
                }
            }
            pending.drain(..offset);
            if let Some(head) = found {
                break head;
            }
        };

        verify_upgrade(&response, &key)?;

        Ok(Self {
            socket,
            decoder: frame::Decoder::default(),
            pending,
            closed: false,
        })
    }

    /// Send one text frame.
    pub fn send_text(&mut self, text: &str, timeout: Duration) -> Result<()> {
        if self.closed {
            return Err(Error::new("WebSocket is closed"));
        }
        let deadline = self.socket.deadline_in(timeout);
        let mut out = Vec::new();
        frame::text_frame(text, self.mask()?, &mut out);
        match self.socket.write_all(&out, deadline) {
            Ok(()) => Ok(()),
            Err(e) => {
                // A half-written frame desynchronises the stream for good — the
                // peer would read the remainder as a frame header. Same rule as
                // `read_message`: the connection is finished, not retryable.
                self.closed = true;
                Err(Error::io("WebSocket send", e))
            }
        }
    }

    /// Read the next application message, or `None` once the peer has closed.
    ///
    /// Pings are answered here, so a caller that only wants text never has to
    /// think about keep-alive frames.
    pub fn read_message(&mut self, timeout: Duration) -> Result<Option<Message>> {
        if self.closed {
            return Ok(None);
        }
        let deadline = self.socket.deadline_in(timeout);
        loop {
            // Drain everything already buffered before asking the socket for
            // more: one read can carry several frames.
            loop {
                let (consumed, step) = match self.decoder.step(&self.pending) {
                    Ok(result) => result,
                    Err(e) => {
                        self.closed = true;
                        return Err(Error::new(format!("WebSocket: {e}")));
                    }
                };
                if consumed == 0 {
                    break;
                }
                self.pending.drain(..consumed);
                match step {
                    frame::Step::Message(Message::Close(code)) => {
                        self.closed = true;
                        let mut out = Vec::new();
                        if let Ok(mask) = self.mask() {
                            frame::close_frame(code, mask, &mut out);
                            let _ = self.socket.write_all(&out, deadline);
                        }
                        return Ok(None);
                    }
                    frame::Step::Message(message) => return Ok(Some(message)),
                    frame::Step::Ping(payload) => {
                        let mut out = Vec::new();
                        frame::pong_frame(&payload, self.mask()?, &mut out);
                        self.socket
                            .write_all(&out, deadline)
                            .map_err(|e| Error::io("WebSocket pong", e))?;
                    }
                    frame::Step::Pong | frame::Step::Incomplete => {}
                }
            }

            let mut scratch = Vec::new();
            // A failed read — including a deadline — leaves an operation
            // outstanding on the loop, so the socket must not be read again.
            // Marking it closed here is what makes that impossible: `publish`
            // answers a read failure by reconnecting, and a caller that
            // instead retried would otherwise submit a second read on the
            // same handle.
            let n = match self.socket.read(&mut scratch, deadline) {
                Ok(n) => n,
                Err(e) => {
                    self.closed = true;
                    return Err(Error::io("WebSocket read", e));
                }
            };
            if n == 0 {
                self.closed = true;
                return Ok(None);
            }
            self.pending.extend_from_slice(&scratch);
        }
    }

    /// Send a close frame and tear the connection down. Best-effort: the
    /// caller already has whatever it was reading for.
    pub fn close(&mut self) {
        if !self.closed {
            self.closed = true;
            let deadline = self.socket.deadline_in(Duration::from_millis(500));
            let mut out = Vec::new();
            if let Ok(mask) = self.mask() {
                frame::close_frame(Some(1000), mask, &mut out);
                let _ = self.socket.write_all(&out, deadline);
            }
        }
        self.socket.shutdown();
    }

    /// A fresh masking key per frame (§5.3 requires unpredictability).
    fn mask(&self) -> Result<[u8; 4]> {
        let mut mask = [0u8; 4];
        crate::tls::secure_random(&mut mask)?;
        Ok(mask)
    }
}

impl Drop for WebSocket {
    fn drop(&mut self) {
        self.close();
    }
}

/// RFC 6455 §4.2.2: status 101, `Connection: Upgrade`, `Upgrade: websocket`,
/// a matching accept key, and no extension the client did not offer.
fn verify_upgrade(response: &Head, key: &str) -> Result<()> {
    if response.status != 101 {
        return Err(Error::new(format!(
            "WebSocket upgrade rejected with status {}",
            response.status
        )));
    }
    if !response.token("connection", "upgrade") {
        return Err(Error::new("upgrade response has no Connection: Upgrade"));
    }
    match response.get("upgrade") {
        Some(value) if value.eq_ignore_ascii_case(b"websocket") => {}
        _ => return Err(Error::new("upgrade response is not to websocket")),
    }
    let expected = frame::accept_key(key);
    match response.get("sec-websocket-accept") {
        Some(value) if value == expected.as_bytes() => {}
        _ => return Err(Error::new("upgrade response has a wrong accept key")),
    }
    if response.get("sec-websocket-extensions").is_some() {
        // No extension was offered, so any answer is unsolicited — and
        // accepting one silently would mean decoding frames under rules this
        // client does not implement.
        return Err(Error::new("server selected an extension none was offered"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn head(headers: &[(&str, &str)]) -> Head {
        Head {
            method: "GET".into(),
            target: "/".into(),
            status: 101,
            version: 1,
            headers: headers.iter().map(|(n, v)| Header::new(n, v)).collect(),
            keep_alive: true,
        }
    }

    const KEY: &str = "dGhlIHNhbXBsZSBub25jZQ==";
    const ACCEPT: &str = "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=";

    #[test]
    fn a_correct_upgrade_is_accepted() {
        let response = head(&[
            ("connection", "Upgrade"),
            ("upgrade", "websocket"),
            ("sec-websocket-accept", ACCEPT),
        ]);
        assert!(verify_upgrade(&response, KEY).is_ok());
    }

    #[test]
    fn every_upgrade_requirement_is_actually_checked() {
        let cases: Vec<(&str, Head)> = vec![
            ("wrong status", {
                let mut h = head(&[
                    ("connection", "Upgrade"),
                    ("upgrade", "websocket"),
                    ("sec-websocket-accept", ACCEPT),
                ]);
                h.status = 200;
                h
            }),
            (
                "no Connection: Upgrade",
                head(&[("upgrade", "websocket"), ("sec-websocket-accept", ACCEPT)]),
            ),
            (
                "upgrade to something else",
                head(&[
                    ("connection", "Upgrade"),
                    ("upgrade", "h2c"),
                    ("sec-websocket-accept", ACCEPT),
                ]),
            ),
            (
                "wrong accept key",
                head(&[
                    ("connection", "Upgrade"),
                    ("upgrade", "websocket"),
                    ("sec-websocket-accept", "AAAAAAAAAAAAAAAAAAAAAAAAAAA="),
                ]),
            ),
            (
                "unsolicited extension",
                head(&[
                    ("connection", "Upgrade"),
                    ("upgrade", "websocket"),
                    ("sec-websocket-accept", ACCEPT),
                    ("sec-websocket-extensions", "permessage-deflate"),
                ]),
            ),
        ];
        for (name, response) in cases {
            assert!(
                verify_upgrade(&response, KEY).is_err(),
                "{name} must be refused"
            );
        }
    }
}
