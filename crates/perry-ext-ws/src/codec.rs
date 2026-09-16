//! The one WebSocket codec in the tree: `turnloop_websocket`'s sans-I/O
//! `Connection`, wrapped so that every transport drives it the same way.
//!
//! # Why a wrapper at all
//!
//! `turnloop_websocket::Connection` is a pure state machine over
//! `(&[u8] in, &mut Vec<u8> out)`. That is exactly what makes it usable from
//! *both* of Perry's transports — a turnloop handle id and a tokio stream —
//! and it is why the WebSocket handshake never needed an owned stream in the
//! first place (see `docs/turnloop/ws-report.md`). What it does **not** give
//! you is a loop, and its `Received` type has two zero cases that a naive one
//! is wrong about.
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
//! | `> 0` | `Some` | **keep going.** tungstenite reads at most one chunk per pass, so a segment carrying several messages yields them one call at a time. |
//!
//! Only the first row terminates the loop. A host that stops as soon as
//! `message` is `None` stalls on a partial frame; a host that stops as soon as
//! `consumed` is `0` drops a message that was already decoded. [`Codec::receive`]
//! is the single place in Perry that gets this right, and
//! `receive_loop_handles_both_zero_cases` pins it.

use std::time::Instant;

pub use turnloop_websocket::{CloseFrame, Error as WsError, Message, Role, WebSocketConfig};

/// A decoded, application-visible WebSocket event.
///
/// This is deliberately *not* `turnloop_websocket::Message`: `ws`'s JS surface
/// distinguishes a close with a code from one without, and carries ping/pong
/// payloads that `Message` models as `Bytes`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Incoming {
    Text(String),
    Binary(Vec<u8>),
    Ping(Vec<u8>),
    Pong(Vec<u8>),
    /// The peer's close frame. `None` when it sent no status code, which `ws`
    /// reports to JS as code 1005 with an empty reason.
    Close(Option<(u16, String)>),
}

/// How long a `close()` waits for the peer's answering close frame before the
/// connection is declared dead. `ws`'s own `closeTimeout` is 30 s.
pub const CLOSE_TIMEOUT_MS: u64 = 30_000;

/// A WebSocket connection's protocol state, with no I/O of its own.
pub struct Codec {
    conn: turnloop_websocket::Connection,
    /// Wire bytes received and not yet consumed by the state machine.
    inbox: Vec<u8>,
    /// Wire bytes the state machine produced and the transport has not sent.
    outbox: Vec<u8>,
    terminal: bool,
}

impl Codec {
    pub fn new(role: Role) -> Self {
        Self::with_config(role, WebSocketConfig::default())
    }

    pub fn with_config(role: Role, config: WebSocketConfig) -> Self {
        Self {
            conn: turnloop_websocket::Connection::new(role, config),
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
    pub fn receive(&mut self, bytes: &[u8]) -> Result<Vec<Incoming>, WsError> {
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
            // The whole point of this module. `consumed == 0 && message.is_none()`
            // is the ONLY case that means "wait": everything else made progress
            // and the state machine may have more to give.
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

    /// Encode an application message. Fragmentation is tungstenite's to choose;
    /// `ws` sends a message as one frame and so does this.
    pub fn send(&mut self, message: Message) -> Result<(), WsError> {
        if self.terminal {
            return Err(WsError::AlreadyClosed);
        }
        self.conn.send(message, &mut self.outbox)
    }

    /// Begin the closing handshake. The peer's answering close arrives through
    /// [`Codec::receive`]; `deadline_ms` bounds the wait.
    pub fn close(&mut self, code: Option<u16>, reason: &str) -> Result<(), WsError> {
        if self.terminal {
            return Ok(());
        }
        let frame = code.map(|code| CloseFrame {
            code: code.into(),
            reason: reason.to_string().into(),
        });
        let deadline = Instant::now() + std::time::Duration::from_millis(CLOSE_TIMEOUT_MS);
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
    pub fn take_output(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.outbox)
    }

    pub fn has_output(&self) -> bool {
        !self.outbox.is_empty()
    }

    /// The close deadline armed by [`Codec::close`], if any.
    pub fn next_timeout(&self) -> Option<Instant> {
        self.conn.next_timeout()
    }

    /// Answer a fired close deadline. `Some(code)` means the peer never replied
    /// and the connection is now dead with that status.
    pub fn handle_timeout(&mut self, now: Instant) -> Option<u16> {
        let code = self.conn.handle_timeout(now);
        if code.is_some() {
            self.terminal = true;
        }
        code
    }

    /// The transport saw EOF. `Some(code)` is the status to report to JS —
    /// the peer's own code when it sent one, else 1006 (abnormal closure).
    pub fn eof(&mut self) -> Option<u16> {
        let code = self.conn.eof();
        self.terminal = true;
        code
    }

    pub fn is_terminal(&self) -> bool {
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
        // `Message::Frame` is only produced by the raw frame API, which this
        // codec never uses.
        Message::Frame(_) => Incoming::Binary(Vec::new()),
    }
}

/// `ws`'s `CloseEvent.code` when the peer closed with no status code.
pub const CLOSE_NO_STATUS: u16 = 1005;
/// `ws`'s `CloseEvent.code` when the connection dropped without a close frame.
pub const CLOSE_ABNORMAL: u16 = 1006;

#[cfg(test)]
mod tests {
    use super::*;

    /// A client-role codec whose output is a server-role codec's input, so the
    /// masking direction is real rather than assumed.
    fn pair() -> (Codec, Codec) {
        (Codec::new(Role::Client), Codec::new(Role::Server))
    }

    #[test]
    fn text_and_binary_round_trip() {
        let (mut client, mut server) = pair();
        client.send(Message::text("hello")).unwrap();
        client
            .send(Message::binary(vec![0u8, 159, 146, 150]))
            .unwrap();
        let wire = client.take_output();
        let events = server.receive(&wire).unwrap();
        assert_eq!(
            events,
            vec![
                Incoming::Text("hello".into()),
                // The bytes that `String::from_utf8_lossy` used to destroy.
                Incoming::Binary(vec![0u8, 159, 146, 150]),
            ]
        );
    }

    /// The whole reason this module exists. A message split across two reads
    /// must not be lost, and a read carrying two messages must yield both.
    #[test]
    fn receive_loop_handles_both_zero_cases() {
        let (mut client, mut server) = pair();
        client.send(Message::text("first")).unwrap();
        client.send(Message::text("second")).unwrap();
        let wire = client.take_output();

        // Case A: `consumed > 0, message: None` — a partial frame. Feeding the
        // first three bytes must absorb them and produce nothing, WITHOUT the
        // loop concluding that the connection is idle.
        let head = &wire[..3];
        assert!(server.receive(head).unwrap().is_empty());

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

        // Case C: no bytes at all is the genuine "wait" case and must terminate.
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
    fn a_ping_is_reported_and_answered_without_the_application_sending() {
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
        let events = server.receive(&client.take_output()).unwrap();
        assert_eq!(events, vec![Incoming::Close(None)]);
    }

    /// A peer-fragmented message is reassembled by the codec, which is what
    /// `ws` promises: `'message'` fires once, with the whole payload.
    #[test]
    fn peer_fragmentation_is_reassembled_into_one_message() {
        let mut server = Codec::new(Role::Server);
        // Hand-built client frames: text "abc" (fin=0), cont "def" (fin=0),
        // cont "ghi" (fin=1). tungstenite has no fragmented-send API, so the
        // wire is written by hand — which is also what the gap fixture does.
        let mut wire = Vec::new();
        wire.extend_from_slice(&masked_frame(0x01, false, b"abc"));
        wire.extend_from_slice(&masked_frame(0x00, false, b"def"));
        wire.extend_from_slice(&masked_frame(0x00, true, b"ghi"));
        let events = server.receive(&wire).unwrap();
        assert_eq!(events, vec![Incoming::Text("abcdefghi".into())]);
    }

    #[test]
    fn eof_without_a_close_frame_is_1006() {
        let mut server = Codec::new(Role::Server);
        assert_eq!(server.eof(), Some(CLOSE_ABNORMAL));
    }

    #[test]
    fn eof_after_the_peer_closed_keeps_the_peer_code() {
        let (mut client, mut server) = pair();
        client.close(Some(4002), "bye").unwrap();
        server.receive(&client.take_output()).unwrap();
        assert_eq!(server.eof(), None, "a closed codec is already terminal");
    }

    fn masked_frame(opcode: u8, fin: bool, payload: &[u8]) -> Vec<u8> {
        let key = [0x12u8, 0x34, 0x56, 0x78];
        let mut out = vec![
            if fin { 0x80 | opcode } else { opcode },
            0x80 | payload.len() as u8,
        ];
        out.extend_from_slice(&key);
        for (i, b) in payload.iter().enumerate() {
            out.push(b ^ key[i % 4]);
        }
        out
    }
}
