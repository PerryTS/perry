//! The WebSocket codec: `turnloop_websocket`'s sans-I/O `Connection`, wrapped
//! so the `ws` module's tokio transport can drive it.
//!
//! This duplicates `perry-ext-ws/src/codec.rs` on purpose. perry-stdlib is the
//! BUNDLED `ws` binding and perry-ext-ws is the external one; a dependency
//! from here to there would be backwards, so the two are deliberately
//! independent implementations of the same protocol wrapper.
//!
//! # The `Received` contract (PerryTS/turnloop#86)
//!
//! `Connection::receive` returns `Received { consumed, message }`. The reading
//! is **not** "an event came back, so keep going":
//!
//! | `consumed` | `message` | meaning |
//! |---|---|---|
//! | `0` | `None` | **wait.** No progress is possible until more bytes arrive. |
//! | `> 0` | `None` | **keep going.** Bytes were absorbed — a partial frame, or a control frame answered internally — and the next call may well produce a message from what is left. |
//! | `0` | `Some` | **keep going.** tungstenite had a whole frame buffered from an earlier call and needed no new bytes for it. |
//! | `> 0` | `Some` | **keep going.** One call yields at most one message, so a read carrying several needs several calls. |
//!
//! Only the first row terminates the loop. A host that stops as soon as
//! `message` is `None` stalls on a partial frame; a host that stops as soon as
//! `consumed` is `0` drops a message that was already decoded.
//! [`Codec::receive`] is the one place in this crate that gets it right, and
//! `receive_loop_handles_both_zero_cases` pins it.

pub(super) use turnloop_http::http1::Mode;
pub(super) use turnloop_websocket::{Message, Role};

use turnloop_http::http1::{BodyLength, Decoder, Encoder, Event, Head, Limits};
use turnloop_websocket::{Error as WsError, WebSocketConfig};

/// A decoded, application-visible WebSocket event.
///
/// Deliberately not `turnloop_websocket::Message`: `ws`'s JS surface
/// distinguishes a close carrying a status code from one without.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Incoming {
    Text(String),
    Binary(Vec<u8>),
    Ping(Vec<u8>),
    Pong(Vec<u8>),
    /// The peer's close frame; `None` when it sent no status code.
    Close(Option<(u16, String)>),
}

/// How long a `close()` waits for the peer's answering close frame. `ws`'s own
/// `closeTimeout` is 30 s.
const CLOSE_TIMEOUT_MS: u64 = 30_000;

/// A WebSocket connection's protocol state, with no I/O of its own.
pub(super) struct Codec {
    conn: turnloop_websocket::Connection,
    /// Wire bytes received and not yet consumed by the state machine.
    inbox: Vec<u8>,
    /// Wire bytes the state machine produced and the transport has not sent.
    outbox: Vec<u8>,
    terminal: bool,
}

impl Codec {
    pub(super) fn new(role: Role) -> Self {
        Self {
            conn: turnloop_websocket::Connection::new(role, WebSocketConfig::default()),
            inbox: Vec::new(),
            outbox: Vec::new(),
            terminal: false,
        }
    }

    /// Feed wire bytes in and drain every message they complete.
    ///
    /// Bytes that do not complete a frame stay in `inbox` for the next call, so
    /// a transport may hand over whatever a single read produced. Automatic
    /// replies (a pong for a ping, the answering close) land in `outbox`; the
    /// caller must `take_output` after every call.
    pub(super) fn receive(&mut self, bytes: &[u8]) -> Result<Vec<Incoming>, WsError> {
        if !bytes.is_empty() {
            self.inbox.extend_from_slice(bytes);
        }
        let mut events = Vec::new();
        if self.terminal {
            return Ok(events);
        }
        let mut offset = 0usize;
        loop {
            let Codec {
                conn,
                inbox,
                outbox,
                ..
            } = self;
            let received = match conn.receive(&inbox[offset..], outbox) {
                Ok(received) => received,
                Err(WsError::ConnectionClosed | WsError::AlreadyClosed) => {
                    self.terminal = true;
                    break;
                }
                Err(e) => {
                    self.terminal = true;
                    self.inbox.drain(..offset);
                    return Err(e);
                }
            };
            offset += received.consumed;
            // The whole point of this module. `consumed == 0 &&
            // message.is_none()` is the ONLY case that means "wait": everything
            // else made progress and the state machine may have more to give.
            let progressed = received.consumed > 0 || received.message.is_some();
            if let Some(message) = received.message {
                let terminal = matches!(message, Message::Close(_));
                events.push(convert(message));
                if terminal {
                    // A close frame ends the message stream. Anything after it
                    // on the wire is a protocol error, not our business.
                    self.terminal = true;
                    break;
                }
            }
            if !progressed {
                break;
            }
        }
        self.inbox.drain(..offset);
        // tungstenite queues its pong/close answers inside `read`; they are only
        // encoded by a flush, and a transport that never flushed would answer a
        // ping only when the application happened to send something.
        match self.conn.flush(&mut self.outbox) {
            Ok(()) => {}
            Err(WsError::ConnectionClosed | WsError::AlreadyClosed) => self.terminal = true,
            Err(e) => {
                self.terminal = true;
                return Err(e);
            }
        }
        Ok(events)
    }

    /// Encode an application message. `ws` sends a message as one frame and so
    /// does this.
    pub(super) fn send(&mut self, message: Message) -> Result<(), WsError> {
        if self.terminal {
            return Err(WsError::AlreadyClosed);
        }
        self.conn.send(message, &mut self.outbox)
    }

    /// Begin the closing handshake. The peer's answering close arrives through
    /// [`Codec::receive`].
    pub(super) fn close(&mut self, code: Option<u16>, reason: &str) -> Result<(), WsError> {
        if self.terminal {
            return Ok(());
        }
        let frame = code.map(|code| turnloop_websocket::CloseFrame {
            code: code.into(),
            reason: reason.to_string().into(),
        });
        let deadline =
            std::time::Instant::now() + std::time::Duration::from_millis(CLOSE_TIMEOUT_MS);
        match self.conn.close(frame, deadline, &mut self.outbox) {
            Ok(()) => Ok(()),
            // Closing an already-closed connection is what `ws.close()` does
            // after the peer closed first, and it is not an error there.
            Err(WsError::ConnectionClosed | WsError::AlreadyClosed) => {
                self.terminal = true;
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    /// Bytes to put on the wire. Always call this after `receive`, `send` or
    /// `close` — the state machine has no other way out.
    pub(super) fn take_output(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.outbox)
    }

    pub(super) fn is_terminal(&self) -> bool {
        self.terminal
    }
}

fn convert(message: Message) -> Incoming {
    match message {
        Message::Text(text) => Incoming::Text(text.as_str().to_string()),
        Message::Binary(bytes) => Incoming::Binary(bytes.to_vec()),
        Message::Ping(bytes) => Incoming::Ping(bytes.to_vec()),
        Message::Pong(bytes) => Incoming::Pong(bytes.to_vec()),
        Message::Close(frame) => {
            Incoming::Close(frame.map(|f| (u16::from(f.code), f.reason.as_str().to_string())))
        }
        // Only the raw frame API produces this, and this codec never uses it.
        Message::Frame(_) => Incoming::Binary(Vec::new()),
    }
}

/// Reads exactly one HTTP head out of a byte stream, keeping whatever followed
/// it — which for an upgrade is already WebSocket frame data and must not be
/// dropped. Used in `Mode::Response` by the client and `Mode::Request` by the
/// server.
pub(super) struct HeadReader {
    decoder: Decoder,
    buffer: Vec<u8>,
    done: bool,
}

impl HeadReader {
    pub(super) fn new(mode: Mode) -> Self {
        let mut decoder = Decoder::new(mode, Limits::default());
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
    /// The loop has the same shape as [`Codec::receive`]'s: `consumed == 0`
    /// with no event is the only "wait", and an `Informational` head (a `1xx`
    /// before the `101`) is skipped rather than returned.
    pub(super) fn receive(&mut self, bytes: &[u8]) -> Result<Option<Head>, String> {
        self.buffer.extend_from_slice(bytes);
        if self.done {
            return Ok(None);
        }
        let mut offset = 0usize;
        let mut head = None;
        while offset < self.buffer.len() {
            let step = self
                .decoder
                .receive(&self.buffer[offset..])
                .map_err(|e| format!("invalid upgrade head: {}", e))?;
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
    pub(super) fn into_leftover(self) -> Vec<u8> {
        self.buffer
    }
}

/// Encode a bodyless HTTP head: the upgrade request, and the `101`.
pub(super) fn encode_head(head: &Head) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut encoder = Encoder::start(head, BodyLength::Empty, &mut out)
        .map_err(|e| format!("cannot encode upgrade head: {}", e))?;
    encoder
        .finish(&[], &mut out)
        .map_err(|e| format!("cannot encode upgrade head: {}", e))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A client-role codec whose output is a server-role codec's input, so the
    /// masking direction is real rather than assumed.
    fn pair() -> (Codec, Codec) {
        (Codec::new(Role::Client), Codec::new(Role::Server))
    }

    /// The whole reason this module exists. A message split across two reads
    /// must not be lost, and a read carrying two messages must yield both.
    #[test]
    fn receive_loop_handles_both_zero_cases() {
        let (mut client, mut server) = pair();
        client.send(Message::text("first")).unwrap();
        client.send(Message::text("second")).unwrap();
        let wire = client.take_output();

        // Case A: `consumed > 0, message: None` — a partial frame. The first
        // three bytes must be absorbed and produce nothing, WITHOUT the loop
        // concluding that the connection is idle.
        assert!(server.receive(&wire[..3]).unwrap().is_empty());

        // Case B: the rest completes both messages. A loop that stopped at the
        // first `consumed == 0` would return only "first".
        let events = server.receive(&wire[3..]).unwrap();
        assert_eq!(
            events,
            vec![
                Incoming::Text("first".into()),
                Incoming::Text("second".into())
            ]
        );

        // Case C: no bytes at all is the genuine "wait" case and must
        // terminate.
        assert!(server.receive(&[]).unwrap().is_empty());
    }

    /// A message arriving one byte at a time exercises the partial-frame path
    /// on every boundary, which is where an off-by-one in the offset shows up.
    #[test]
    fn byte_at_a_time_delivery_loses_nothing() {
        let (mut client, mut server) = pair();
        client
            .send(Message::text("fragmented-by-the-transport"))
            .unwrap();
        let wire = client.take_output();
        let mut seen = Vec::new();
        for byte in &wire {
            seen.extend(server.receive(&[*byte]).unwrap());
        }
        assert_eq!(
            seen,
            vec![Incoming::Text("fragmented-by-the-transport".into())]
        );
    }

    #[test]
    fn text_and_binary_round_trip() {
        let (mut client, mut server) = pair();
        client.send(Message::text("hello")).unwrap();
        client
            .send(Message::binary(vec![0u8, 159, 146, 150]))
            .unwrap();
        let events = server.receive(&client.take_output()).unwrap();
        assert_eq!(
            events,
            vec![
                Incoming::Text("hello".into()),
                Incoming::Binary(vec![0u8, 159, 146, 150]),
            ]
        );
    }

    #[test]
    fn a_ping_is_answered_by_the_flush_inside_receive() {
        let (mut client, mut server) = pair();
        client.send(Message::Ping(b"beat".to_vec().into())).unwrap();
        let events = server.receive(&client.take_output()).unwrap();
        assert_eq!(events, vec![Incoming::Ping(b"beat".to_vec())]);
        // The pong must be on the wire already: nothing else is going to flush.
        let back = server.take_output();
        assert!(
            !back.is_empty(),
            "a ping must be answered by the flush inside receive"
        );
        assert_eq!(
            client.receive(&back).unwrap(),
            vec![Incoming::Pong(b"beat".to_vec())]
        );
    }

    #[test]
    fn close_carries_its_code_and_reason() {
        let (mut client, mut server) = pair();
        client.close(Some(4001), "going away").unwrap();
        let events = server.receive(&client.take_output()).unwrap();
        assert_eq!(
            events,
            vec![Incoming::Close(Some((4001, "going away".into())))]
        );
    }

    #[test]
    fn a_close_with_no_code_is_reported_as_none() {
        let (mut client, mut server) = pair();
        client.close(None, "").unwrap();
        assert_eq!(
            server.receive(&client.take_output()).unwrap(),
            vec![Incoming::Close(None)]
        );
    }

    /// The two handshake halves meet, and the part that matters for a transport
    /// holds: the bytes that rode along with the `101` survive.
    #[test]
    fn a_client_handshake_keeps_the_bytes_after_the_101() {
        let (handshake, request_head) =
            turnloop_websocket::ClientHandshake::new("example.com", "/chat", [7u8; 16], Vec::new())
                .unwrap();
        let request = encode_head(&request_head).unwrap();

        let mut server_reader = HeadReader::new(Mode::Request);
        let head = server_reader
            .receive(&request)
            .unwrap()
            .expect("a complete head");
        assert_eq!(head.method, "GET");
        assert_eq!(head.target, "/chat");
        let (response_head, _) = turnloop_websocket::accept(&head, &[]).unwrap();
        let response = encode_head(&response_head).unwrap();

        // Split the response so the client sees a partial head first: the
        // `consumed == 0, no event` wait case has to hold here too.
        let mut client_reader = HeadReader::new(Mode::Response);
        assert!(client_reader.receive(&response[..12]).unwrap().is_none());
        let mut tail = response[12..].to_vec();
        tail.extend_from_slice(b"\x81\x03abc"); // an unmasked text frame riding along
        let verified = client_reader.receive(&tail).unwrap().expect("the 101");
        assert_eq!(handshake.verify(&verified).unwrap(), None);
        assert_eq!(client_reader.into_leftover(), b"\x81\x03abc");
    }
}
