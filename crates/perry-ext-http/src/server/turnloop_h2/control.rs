//! `session.settings()`, `.goaway()` and `.ping()` — as frames on the wire.
//!
//! # What was here before
//!
//! Nothing reached a wire. `queue_session_settings` and `queue_session_goaway`
//! enumerated `Http2SessionHandle`s with `iter_handle_ids_of`, picked the ones
//! whose `session_type` was the opposite of the caller's, and pushed a
//! synthetic event into their queues; `queue_session_ping` fired its own
//! callback back without asking anyone. That is a **loopback simulation**, and
//! it is why `test-parity/node-suite/http2/` passes: every case in it is a
//! Perry client talking to a Perry server in one process. So these three are
//! not being ported here — they are being implemented, and the existing tests
//! constrain nothing because both ends were Perry.
//!
//! # What a second SETTINGS frame can and cannot say
//!
//! Every value in a SETTINGS frame is a promise about state inside
//! `turnloop_http::http2::Connection`, and `Connection` exposes no setter for
//! any of it after `new`:
//!
//! | setting | the core's coupling |
//! |---|---|
//! | HEADER_TABLE_SIZE | `hpack::Decoder::new(4096, …)` — a larger table would be advertised and then not honoured, and the peer's next header block would fail to decode |
//! | INITIAL_WINDOW_SIZE | each `Stream`'s `recv_window` is **hard-coded to 65535**; advertising more invites DATA the core answers with FLOW_CONTROL_ERROR |
//! | MAX_CONCURRENT_STREAMS | `Limits::streams` is both the advertisement and the size of the stream table; advertising more produces `REFUSED_STREAM` from inside `receive`, which is a connection error |
//! | MAX_FRAME_SIZE | `decode_frame(input, limits.frame_size)` rejects anything larger as FRAME_SIZE_ERROR |
//! | MAX_HEADER_LIST_SIZE | the decoder's own limit, fixed at construction |
//!
//! So the frame that goes out carries the values **clamped to what the core
//! will actually honour**: a setting may be lowered, never raised. That is a
//! real SETTINGS frame with truthful contents, and `session.localSettings`
//! reports what went on the wire rather than what was asked for. Raising any of
//! them needs a `Connection::set_limits`, which is filed as a turnloop gap.
//!
//! # The acknowledgement
//!
//! `Connection` tracks exactly one outstanding SETTINGS — its own, from the
//! constructor — and answers a second acknowledgement with `protocol(
//! "unsolicited SETTINGS ack")`, a **connection** error. Every frame sent from
//! here therefore increments `owed_settings_acks`, and `conn::prescan` eats
//! exactly that many acks before the core can see them. That is also what fires
//! `session.settings(obj, cb)`'s callback and Node's `'localSettings'`: the
//! core consumes an ack with `event: None`, so there is nothing else to fire on.

use turnloop_http::http2::encode_frame;

use crate::server::http2_session_settings::Http2SettingsState;

use super::conn::{self, H2Conn, PendingControl};

/// SETTINGS identifiers, RFC 9113 §6.5.2.
const HEADER_TABLE_SIZE: u16 = 1;
const ENABLE_PUSH: u16 = 2;
const MAX_CONCURRENT_STREAMS: u16 = 3;
const INITIAL_WINDOW_SIZE: u16 = 4;
const MAX_FRAME_SIZE: u16 = 5;
const MAX_HEADER_LIST_SIZE: u16 = 6;

/// Clamp a requested settings object to what this connection's core can honour.
///
/// Returns the values that will be advertised — which is also what
/// `session.localSettings` must then report.
pub(crate) fn clamp_to_core(
    requested: &Http2SettingsState,
    advertised: &Http2SettingsState,
) -> Http2SettingsState {
    let mut out = requested.clone();
    out.header_table_size = requested
        .header_table_size
        .min(advertised.header_table_size);
    out.initial_window_size = advertised.initial_window_size;
    out.max_concurrent_streams = requested
        .max_concurrent_streams
        .min(advertised.max_concurrent_streams);
    out.max_frame_size = requested
        .max_frame_size
        .clamp(16_384, advertised.max_frame_size);
    out.max_header_list_size = requested
        .max_header_list_size
        .min(advertised.max_header_list_size);
    out.max_header_size = out.max_header_list_size;
    out.enable_push = advertised.enable_push;
    out
}

fn settings_payload(role_is_client: bool, settings: &Http2SettingsState) -> Vec<u8> {
    let mut payload = Vec::with_capacity(6 * 6);
    let mut put = |id: u16, value: u32| {
        payload.extend_from_slice(&id.to_be_bytes());
        payload.extend_from_slice(&value.to_be_bytes());
    };
    put(HEADER_TABLE_SIZE, settings.header_table_size);
    if role_is_client {
        // Only a client may set ENABLE_PUSH to a non-zero value, and RFC 9113
        // §6.5.2 makes a server that raises it a connection error on the
        // client's side. Perry never pushes, so the value is always zero.
        put(ENABLE_PUSH, 0);
    }
    put(MAX_CONCURRENT_STREAMS, settings.max_concurrent_streams);
    put(INITIAL_WINDOW_SIZE, settings.initial_window_size);
    put(MAX_FRAME_SIZE, settings.max_frame_size);
    put(MAX_HEADER_LIST_SIZE, settings.max_header_list_size);
    payload
}

/// `session.settings(obj)` — encode and send a real SETTINGS frame.
///
/// Returns the settings that went on the wire, or `None` when this session is
/// not on turnloop (the caller then keeps the legacy path).
pub(crate) fn send_settings(
    conn_id: i64,
    requested: &Http2SettingsState,
) -> Option<Http2SettingsState> {
    conn::with_owned(conn_id, |c| {
        let effective = clamp_to_core(requested, &c.settings);
        if !conn::transport_ready(c) {
            // `http2.connect()` hands JS a session object synchronously and
            // Node accepts `settings()` on it immediately. Writing the frame
            // now would put it on the wire ahead of the client preface, and the
            // peer answers a connection error — measured as a hung
            // `test_gap_gc_http2_pending_event_callback_rooting`.
            c.pending_controls
                .push(PendingControl::Settings(effective.clone()));
            return Some(effective);
        }
        if !write_settings(c, &effective) {
            return None;
        }
        Some(effective)
    })
    .flatten()
}

/// Encode and send one SETTINGS frame, counting the acknowledgement it owes.
fn write_settings(c: &mut H2Conn, effective: &Http2SettingsState) -> bool {
    let payload = settings_payload(c.role == turnloop_http::http2::Role::Client, effective);
    let mut frame = Vec::with_capacity(9 + payload.len());
    if encode_frame(4, 0, 0, &payload, &mut frame).is_err() {
        return false;
    }
    conn::write_raw(c, &frame);
    // The peer's acknowledgement is intercepted by `conn::prescan`, which is
    // what turns it into `'localSettings'` and the user's callback.
    c.owed_settings_acks = c.owed_settings_acks.saturating_add(1);
    true
}

/// Send the connection-level frames JS asked for before the transport was
/// ready, in the order it asked for them.
///
/// Called from `conn::client_transport_ready`, **before** the queued
/// `session.request()` opens: a connection-level frame the caller issued first
/// must not end up behind a HEADERS it preceded.
pub(crate) fn drain_pending(c: &mut H2Conn) {
    let queued = std::mem::take(&mut c.pending_controls);
    for control in queued {
        match control {
            PendingControl::Settings(settings) => {
                write_settings(c, &settings);
            }
            PendingControl::Ping(data) => {
                if c.core.as_mut().is_some_and(|core| core.ping(data).is_ok()) {
                    conn::flush(c);
                }
            }
            PendingControl::Goaway {
                code,
                last_stream,
                opaque,
            } => {
                write_goaway(c, code, last_stream, &opaque);
            }
            PendingControl::Close => {
                if let Some(core) = c.core.as_mut() {
                    let _ = core.shutdown();
                }
                c.draining = true;
                conn::flush(c);
            }
        }
    }
}

/// `session.goaway(code, lastStreamID, opaqueData)`.
///
/// `Connection::shutdown` always sends NO_ERROR with its own `last_remote` and
/// no opaque data, so the frame is hand-encoded. The session is **not** marked
/// draining: Node's `goaway()` sends a frame and leaves the session usable,
/// unlike `close()`.
pub(crate) fn send_goaway(conn_id: i64, code: u32, last_stream_id: u32, opaque: &[u8]) -> bool {
    conn::with_owned(conn_id, |c| {
        if !conn::transport_ready(c) {
            c.pending_controls.push(PendingControl::Goaway {
                code,
                last_stream: last_stream_id,
                opaque: opaque.to_vec(),
            });
            return true;
        }
        write_goaway(c, code, last_stream_id, opaque)
    })
    .unwrap_or(false)
}

fn write_goaway(c: &mut H2Conn, code: u32, last_stream_id: u32, opaque: &[u8]) -> bool {
    let mut payload = Vec::with_capacity(8 + opaque.len());
    payload.extend_from_slice(&(last_stream_id & 0x7fff_ffff).to_be_bytes());
    payload.extend_from_slice(&code.to_be_bytes());
    payload.extend_from_slice(opaque);
    let mut frame = Vec::with_capacity(9 + payload.len());
    if encode_frame(7, 0, 0, &payload, &mut frame).is_err() {
        return false;
    }
    conn::write_raw(c, &frame);
    true
}

/// `session.ping(payload, cb)` — a real PING frame. The callback fires from
/// `Event::Ping { ack: true }`, not from here.
pub(crate) fn send_ping(conn_id: i64, payload: [u8; 8]) -> bool {
    conn::with_owned(conn_id, |c| {
        if !conn::transport_ready(c) {
            // Node's `ping()` answers true for a session that is still
            // connecting; the frame goes out when the transport is up.
            c.pending_controls.push(PendingControl::Ping(payload));
            return true;
        }
        let sent = c
            .core
            .as_mut()
            .is_some_and(|core| core.ping(payload).is_ok());
        if sent {
            conn::flush(c);
        }
        sent
    })
    .unwrap_or(false)
}

/// `session.close([cb])` — Node's graceful GOAWAY, then close once drained.
pub(crate) fn session_close(conn_id: i64) {
    super::stream::session_close(conn_id);
}

/// `session.destroy()` — no GOAWAY, no drain.
pub(crate) fn session_destroy(conn_id: i64) {
    conn::destroy_connection(conn_id);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn advertised() -> Http2SettingsState {
        Http2SettingsState {
            header_table_size: 4_096,
            enable_push: false,
            initial_window_size: 65_535,
            max_frame_size: 16_384,
            max_concurrent_streams: 128,
            max_header_size: 32_768,
            max_header_list_size: 32_768,
            enable_connect_protocol: false,
        }
    }

    /// A setting may be lowered. This is the half that is genuinely honoured.
    #[test]
    fn lowering_a_setting_survives_the_clamp() {
        let mut requested = advertised();
        requested.max_concurrent_streams = 8;
        requested.max_header_list_size = 4_096;
        let out = clamp_to_core(&requested, &advertised());
        assert_eq!(out.max_concurrent_streams, 8);
        assert_eq!(out.max_header_list_size, 4_096);
        assert_eq!(out.max_header_size, 4_096);
    }

    /// Raising one is clamped back rather than advertised, because the core has
    /// no setter and would then reject what it had just promised. A test rather
    /// than a comment because the failure mode is a connection error minutes
    /// later on the peer's side.
    #[test]
    fn raising_a_setting_is_clamped_to_what_the_core_honours() {
        let mut requested = advertised();
        requested.max_concurrent_streams = 10_000;
        requested.max_frame_size = 1 << 20;
        requested.header_table_size = 65_536;
        requested.initial_window_size = 1 << 20;
        let out = clamp_to_core(&requested, &advertised());
        assert_eq!(out.max_concurrent_streams, 128);
        assert_eq!(out.max_frame_size, 16_384);
        assert_eq!(out.header_table_size, 4_096);
        assert_eq!(out.initial_window_size, 65_535);
    }

    /// A server must never advertise ENABLE_PUSH; a client advertises zero.
    #[test]
    fn enable_push_is_a_client_only_identifier_and_always_zero() {
        let server = settings_payload(false, &advertised());
        assert!(!server
            .chunks_exact(6)
            .any(|c| u16::from_be_bytes([c[0], c[1]]) == ENABLE_PUSH));
        let client = settings_payload(true, &advertised());
        let push = client
            .chunks_exact(6)
            .find(|c| u16::from_be_bytes([c[0], c[1]]) == ENABLE_PUSH)
            .expect("client advertises ENABLE_PUSH");
        assert_eq!(u32::from_be_bytes([push[2], push[3], push[4], push[5]]), 0);
    }

    /// The payload is a whole number of 6-byte entries, which is what keeps the
    /// peer from answering FRAME_SIZE_ERROR.
    #[test]
    fn settings_payload_is_a_whole_number_of_entries() {
        assert_eq!(settings_payload(false, &advertised()).len() % 6, 0);
        assert_eq!(settings_payload(true, &advertised()).len() % 6, 0);
    }
}
