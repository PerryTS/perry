//! RFC 6455 client-side framing.
//!
//! # Why this is here instead of `turnloop-websocket`
//!
//! `turnloop-websocket` is the right long-term home and this module should be
//! deleted in its favour. Two things stop that today, and both are recorded in
//! `docs/turnloop/p11-report.md` rather than worked around:
//!
//! 1. **The soak window.** This repository refuses any crate release younger
//!    than `SOAK_DAYS` (`scripts/soak/constants.mts`, enforced by cargo's
//!    `global-min-publish-age` and the `soak-gate` job). Every published
//!    `turnloop-websocket` is inside that window, so adopting it now means an
//!    env-var bypass — which the soak skill says is deliberately not available,
//!    because opting out must be a committed, reviewable change.
//! 2. **It would add a fourth tungstenite major.** `turnloop-websocket`
//!    re-exports tungstenite 0.30; the tree already carries 0.24
//!    (`perry-ui-android`), 0.29 (`perry-ext-ws`, `perry-ext-http`,
//!    `perry-stdlib`) and would gain a third live major for one CLI caller.
//!    P8's removal plan puts that migration in group **E**, where all three
//!    move together.
//!
//! # Scope
//!
//! Exactly what the CLI's two WebSocket call sites need, and nothing else: a
//! client that sends short text frames and reads text frames, answering pings.
//! There is no extension negotiation (`permessage-deflate` is never offered,
//! so a conforming server never sends a compressed frame), no server role, and
//! no fragmented *send* — an outgoing message is always one frame. Incoming
//! fragmentation **is** handled, because a server may fragment freely.

use std::fmt;

/// RFC 6455 §1.3. Fixed by the specification, not a choice.
const HANDSHAKE_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

/// A message big enough to be a bug rather than a build log line. The CLI's
/// hub sends progress JSON; the largest observed is a few kilobytes.
const MAX_MESSAGE: usize = 8 * 1024 * 1024;

/// Opcodes this client understands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OpCode {
    Continuation,
    Text,
    Binary,
    Close,
    Ping,
    Pong,
}

impl OpCode {
    fn from_bits(bits: u8) -> Option<Self> {
        Some(match bits {
            0x0 => Self::Continuation,
            0x1 => Self::Text,
            0x2 => Self::Binary,
            0x8 => Self::Close,
            0x9 => Self::Ping,
            0xA => Self::Pong,
            _ => return None,
        })
    }

    fn is_control(self) -> bool {
        matches!(self, Self::Close | Self::Ping | Self::Pong)
    }
}

/// A complete application message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Message {
    Text(String),
    Binary(Vec<u8>),
    /// The peer closed, with its status code when it sent one.
    Close(Option<u16>),
}

/// A protocol violation. The connection is not recoverable after one.
#[derive(Clone, Debug)]
pub struct ProtocolError(pub String);

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

fn err(text: impl Into<String>) -> ProtocolError {
    ProtocolError(text.into())
}

/// `Sec-WebSocket-Accept` for a given `Sec-WebSocket-Key` (RFC 6455 §4.2.2).
pub fn accept_key(key: &str) -> String {
    use perry_base64::Engine as _;
    use sha1::{Digest, Sha1};
    let mut hasher = Sha1::new();
    hasher.update(key.as_bytes());
    hasher.update(HANDSHAKE_GUID.as_bytes());
    perry_base64::engine::general_purpose::STANDARD.encode(hasher.finalize())
}

/// Encode `Sec-WebSocket-Key` from a 16-byte nonce.
pub fn encode_key(nonce: &[u8; 16]) -> String {
    use perry_base64::Engine as _;
    perry_base64::engine::general_purpose::STANDARD.encode(nonce)
}

/// Serialize one client frame. A client frame is always masked (§5.3) and this
/// client never fragments what it sends.
fn write_frame(opcode: u8, payload: &[u8], mask: [u8; 4], out: &mut Vec<u8>) {
    out.push(0x80 | opcode); // FIN | opcode
    let len = payload.len();
    if len < 126 {
        out.push(0x80 | len as u8);
    } else if len <= u16::MAX as usize {
        out.push(0x80 | 126);
        out.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        out.push(0x80 | 127);
        out.extend_from_slice(&(len as u64).to_be_bytes());
    }
    out.extend_from_slice(&mask);
    let start = out.len();
    out.extend_from_slice(payload);
    for (i, byte) in out[start..].iter_mut().enumerate() {
        *byte ^= mask[i % 4];
    }
}

pub fn text_frame(text: &str, mask: [u8; 4], out: &mut Vec<u8>) {
    write_frame(0x1, text.as_bytes(), mask, out);
}

pub fn pong_frame(payload: &[u8], mask: [u8; 4], out: &mut Vec<u8>) {
    write_frame(0xA, payload, mask, out);
}

pub fn close_frame(code: Option<u16>, mask: [u8; 4], out: &mut Vec<u8>) {
    let payload = code.map(|c| c.to_be_bytes().to_vec()).unwrap_or_default();
    write_frame(0x8, &payload, mask, out);
}

/// What one `Decoder::step` produced.
pub enum Step {
    /// A complete application or close message.
    Message(Message),
    /// A ping that must be answered with this payload.
    Ping(Vec<u8>),
    /// A pong; nothing to do.
    Pong,
    /// Not enough bytes yet.
    Incomplete,
}

/// Reassembles incoming frames into messages.
#[derive(Default)]
pub struct Decoder {
    /// Partial message across continuation frames: the first frame's opcode
    /// and the bytes so far.
    fragment: Option<(OpCode, Vec<u8>)>,
}

impl Decoder {
    /// Consume one frame from the front of `input`.
    ///
    /// Returns the number of bytes consumed alongside the step, so the caller
    /// keeps ownership of the buffer and never has to guess how much to drain.
    pub fn step(&mut self, input: &[u8]) -> Result<(usize, Step), ProtocolError> {
        let Some((header, payload_offset, payload_len)) = parse_header(input)? else {
            return Ok((0, Step::Incomplete));
        };
        let total = payload_offset + payload_len;
        if input.len() < total {
            return Ok((0, Step::Incomplete));
        }
        let payload = &input[payload_offset..total];

        if header.masked {
            // §5.1: a server MUST NOT mask. Accepting one would mean silently
            // handing the caller XORed bytes.
            return Err(err("server sent a masked frame"));
        }

        let opcode = OpCode::from_bits(header.opcode)
            .ok_or_else(|| err(format!("unknown opcode {:#x}", header.opcode)))?;

        if opcode.is_control() {
            if !header.fin {
                return Err(err("fragmented control frame"));
            }
            if payload_len > 125 {
                return Err(err("control frame longer than 125 bytes"));
            }
            let step = match opcode {
                OpCode::Ping => Step::Ping(payload.to_vec()),
                OpCode::Pong => Step::Pong,
                OpCode::Close => {
                    let code = if payload.len() >= 2 {
                        Some(u16::from_be_bytes([payload[0], payload[1]]))
                    } else {
                        None
                    };
                    Step::Message(Message::Close(code))
                }
                _ => unreachable!("is_control covers exactly these three"),
            };
            return Ok((total, step));
        }

        // A data frame. Either it starts a message or it continues one.
        let (kind, mut buffer) = match (opcode, self.fragment.take()) {
            (OpCode::Continuation, Some(state)) => state,
            (OpCode::Continuation, None) => {
                return Err(err("continuation frame with nothing to continue"));
            }
            (kind, None) => (kind, Vec::new()),
            (_, Some(_)) => {
                return Err(err("new data frame while a message is unfinished"));
            }
        };

        if buffer.len() + payload.len() > MAX_MESSAGE {
            return Err(err("message exceeds 8 MiB"));
        }
        buffer.extend_from_slice(payload);

        if !header.fin {
            self.fragment = Some((kind, buffer));
            return Ok((total, Step::Incomplete));
        }

        let message = match kind {
            OpCode::Text => Message::Text(
                String::from_utf8(buffer).map_err(|_| err("text frame is not valid UTF-8"))?,
            ),
            _ => Message::Binary(buffer),
        };
        Ok((total, Step::Message(message)))
    }
}

struct Header {
    fin: bool,
    opcode: u8,
    masked: bool,
}

/// Parse a frame header. `Ok(None)` means "not enough bytes yet".
fn parse_header(input: &[u8]) -> Result<Option<(Header, usize, usize)>, ProtocolError> {
    if input.len() < 2 {
        return Ok(None);
    }
    let first = input[0];
    let second = input[1];
    if first & 0x70 != 0 {
        // RSV1-3. No extension was negotiated, so a set bit is a violation
        // rather than something to ignore — silently ignoring RSV1 would mean
        // handing a caller a deflate-compressed payload as if it were text.
        return Err(err("reserved frame bits set with no extension negotiated"));
    }
    let masked = second & 0x80 != 0;
    let short_len = (second & 0x7F) as usize;
    let (payload_len, mut offset) = match short_len {
        126 => {
            if input.len() < 4 {
                return Ok(None);
            }
            (u16::from_be_bytes([input[2], input[3]]) as usize, 4)
        }
        127 => {
            if input.len() < 10 {
                return Ok(None);
            }
            let len = u64::from_be_bytes(input[2..10].try_into().expect("10 bytes checked"));
            if len > MAX_MESSAGE as u64 {
                return Err(err("frame exceeds 8 MiB"));
            }
            (len as usize, 10)
        }
        n => (n, 2),
    };
    if masked {
        offset += 4;
    }
    Ok(Some((
        Header {
            fin: first & 0x80 != 0,
            opcode: first & 0x0F,
            masked,
        },
        offset,
        payload_len,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The one value in RFC 6455 with a worked example (§1.3).
    #[test]
    fn accept_key_matches_the_rfc_example() {
        assert_eq!(
            accept_key("dGhlIHNhbXBsZSBub25jZQ=="),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
    }

    /// Build a server frame (unmasked) for the decoder to read.
    fn server_frame(fin: bool, opcode: u8, payload: &[u8]) -> Vec<u8> {
        let mut out = vec![if fin { 0x80 | opcode } else { opcode }];
        let len = payload.len();
        if len < 126 {
            out.push(len as u8);
        } else if len <= u16::MAX as usize {
            out.push(126);
            out.extend_from_slice(&(len as u16).to_be_bytes());
        } else {
            out.push(127);
            out.extend_from_slice(&(len as u64).to_be_bytes());
        }
        out.extend_from_slice(payload);
        out
    }

    fn decode_one(decoder: &mut Decoder, bytes: &[u8]) -> (usize, Step) {
        decoder.step(bytes).expect("valid frame")
    }

    #[test]
    fn a_short_text_frame_round_trips() {
        let frame = server_frame(true, 0x1, b"hello");
        let mut decoder = Decoder::default();
        let (n, step) = decode_one(&mut decoder, &frame);
        assert_eq!(n, frame.len());
        assert!(matches!(step, Step::Message(Message::Text(t)) if t == "hello"));
    }

    #[test]
    fn a_medium_frame_uses_the_16_bit_length() {
        let payload = "x".repeat(1000);
        let frame = server_frame(true, 0x1, payload.as_bytes());
        assert_eq!(frame[1], 126);
        let mut decoder = Decoder::default();
        let (n, step) = decode_one(&mut decoder, &frame);
        assert_eq!(n, frame.len());
        assert!(matches!(step, Step::Message(Message::Text(t)) if t.len() == 1000));
    }

    #[test]
    fn a_fragmented_message_is_reassembled_in_order() {
        let mut decoder = Decoder::default();
        let first = server_frame(false, 0x1, b"par");
        let second = server_frame(true, 0x0, b"tial");
        let (_, step) = decode_one(&mut decoder, &first);
        assert!(matches!(step, Step::Incomplete));
        let (_, step) = decode_one(&mut decoder, &second);
        assert!(matches!(step, Step::Message(Message::Text(t)) if t == "partial"));
    }

    /// A control frame may be interleaved *between* fragments (§5.4) and must
    /// not disturb the message being reassembled.
    #[test]
    fn a_ping_between_fragments_does_not_break_the_message() {
        let mut decoder = Decoder::default();
        let (_, step) = decode_one(&mut decoder, &server_frame(false, 0x1, b"a"));
        assert!(matches!(step, Step::Incomplete));
        let (_, step) = decode_one(&mut decoder, &server_frame(true, 0x9, b"pong me"));
        assert!(matches!(step, Step::Ping(p) if p == b"pong me"));
        let (_, step) = decode_one(&mut decoder, &server_frame(true, 0x0, b"b"));
        assert!(matches!(step, Step::Message(Message::Text(t)) if t == "ab"));
    }

    #[test]
    fn a_partial_frame_consumes_nothing() {
        let frame = server_frame(true, 0x1, b"hello");
        let mut decoder = Decoder::default();
        for cut in 0..frame.len() {
            let (n, step) = decode_one(&mut decoder, &frame[..cut]);
            assert_eq!(n, 0, "no bytes consumed from a partial frame at {cut}");
            assert!(matches!(step, Step::Incomplete));
        }
    }

    #[test]
    fn a_close_frame_carries_its_code() {
        let mut decoder = Decoder::default();
        let (_, step) = decode_one(
            &mut decoder,
            &server_frame(true, 0x8, &1000u16.to_be_bytes()),
        );
        assert!(matches!(step, Step::Message(Message::Close(Some(1000)))));
    }

    #[test]
    fn a_bodyless_close_is_still_a_close() {
        let mut decoder = Decoder::default();
        let (_, step) = decode_one(&mut decoder, &server_frame(true, 0x8, b""));
        assert!(matches!(step, Step::Message(Message::Close(None))));
    }

    /// The four rejections that matter. Each one, unchecked, hands the caller
    /// bytes that are not what they claim to be.
    #[test]
    fn protocol_violations_are_refused() {
        let cases: Vec<(&str, Vec<u8>)> = vec![
            ("masked server frame", {
                let mut f = vec![0x81, 0x80 | 1, 1, 2, 3, 4];
                f.push(b'x' ^ 1);
                f
            }),
            ("reserved bit set", vec![0xC1, 0x01, b'x']),
            ("unknown opcode", server_frame(true, 0x3, b"")),
            ("fragmented control frame", {
                let mut f = server_frame(true, 0x9, b"");
                f[0] &= 0x7F;
                f
            }),
            ("orphan continuation", server_frame(true, 0x0, b"x")),
        ];
        for (name, bytes) in cases {
            let mut decoder = Decoder::default();
            assert!(decoder.step(&bytes).is_err(), "{name} must be refused");
        }
    }

    #[test]
    fn invalid_utf8_in_a_text_frame_is_refused() {
        let mut decoder = Decoder::default();
        assert!(decoder
            .step(&server_frame(true, 0x1, &[0xFF, 0xFE]))
            .is_err());
    }

    #[test]
    fn a_client_frame_is_masked_and_decodes_back() {
        let mut out = Vec::new();
        text_frame("subscribe", [0xDE, 0xAD, 0xBE, 0xEF], &mut out);
        assert_eq!(out[0], 0x81, "FIN | text");
        assert_eq!(out[1] & 0x80, 0x80, "mask bit set");
        assert_eq!(out[1] & 0x7F, 9);
        let mask = &out[2..6];
        let unmasked: Vec<u8> = out[6..]
            .iter()
            .enumerate()
            .map(|(i, b)| b ^ mask[i % 4])
            .collect();
        assert_eq!(unmasked, b"subscribe");
    }

    /// A zero mask would look correct in every round-trip test while leaving
    /// the payload in the clear, which is the one thing masking exists to
    /// prevent. This asserts the bytes actually change.
    #[test]
    fn masking_actually_transforms_the_payload() {
        let mut out = Vec::new();
        text_frame("aaaaaaaa", [1, 2, 3, 4], &mut out);
        assert_ne!(&out[6..], b"aaaaaaaa");
    }
}
