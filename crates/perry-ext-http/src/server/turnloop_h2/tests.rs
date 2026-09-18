//! Unit tests for the parts of the HTTP/2 binding that have no transport.
//!
//! The transport itself is covered by h2spec against a real Perry server
//! (`docs/turnloop/http2b-report.md`); what is tested here is the arithmetic
//! and the header translation, because both have failure modes that a passing
//! h2spec run would not distinguish from a passing one.

use turnloop_http::http1::Header;
use turnloop_http::http2::{Connection, Limits, Role};

use super::conn::advertised_settings;
use super::stream::{body_forbidden, response_headers};
use crate::server::http2_session_settings::Http2SettingsState;

fn header_value<'a>(headers: &'a [Header], name: &str) -> Option<&'a [u8]> {
    headers
        .iter()
        .find(|h| h.name == name)
        .map(|h| h.value.as_slice())
}

/// `:status` must be the first header in the block: `validate_headers` rejects
/// a pseudo-header that follows a regular one, as a **connection** error, so a
/// handler that set one ordinary header would take the whole session down.
#[test]
fn status_leads_the_response_block() {
    let headers = response_headers(200, &[("X-Thing".to_string(), "1".to_string())], Some(3));
    assert_eq!(headers[0].name, ":status");
    assert_eq!(headers[0].value, b"200");
}

/// Connection-specific fields are legal in a Node handler and illegal on the
/// wire. Dropping them is what keeps `res.setHeader('Connection', 'close')`
/// from being a PROTOCOL_ERROR that kills every sibling stream.
#[test]
fn connection_specific_fields_are_dropped() {
    let headers = response_headers(
        200,
        &[
            ("Connection".to_string(), "close".to_string()),
            ("Keep-Alive".to_string(), "timeout=5".to_string()),
            ("Transfer-Encoding".to_string(), "chunked".to_string()),
            ("Upgrade".to_string(), "h2c".to_string()),
            ("Proxy-Connection".to_string(), "keep-alive".to_string()),
            ("X-Kept".to_string(), "yes".to_string()),
        ],
        None,
    );
    for banned in [
        "connection",
        "keep-alive",
        "transfer-encoding",
        "upgrade",
        "proxy-connection",
    ] {
        assert!(
            header_value(&headers, banned).is_none(),
            "{banned} reached the wire"
        );
    }
    assert_eq!(header_value(&headers, "x-kept"), Some(&b"yes"[..]));
}

/// The block the response path produces is one the core will actually accept.
/// This is the assertion that ties the two previous tests to the thing that
/// matters, rather than to this module's own idea of the rules.
#[test]
fn a_produced_response_block_passes_the_core() {
    let mut server = Connection::new(Role::Server, Limits::default()).expect("core");
    let mut client = Connection::new(Role::Client, Limits::default()).expect("core");
    // Handshake far enough for the server to hold an open stream.
    let mut bytes = client.output().to_vec();
    let n = client.output().len();
    client.consume_output(n).expect("consume");
    while let Ok(step) = server.receive(&bytes) {
        if step.consumed == 0 && step.event.is_none() {
            break;
        }
        bytes.drain(..step.consumed);
    }
    let id = client
        .open(
            &[
                Header::new(":method", "GET"),
                Header::new(":scheme", "http"),
                Header::new(":path", "/"),
                Header::new(":authority", "x"),
            ],
            true,
        )
        .expect("open");
    let mut bytes = client.output().to_vec();
    let n = client.output().len();
    client.consume_output(n).expect("consume");
    while let Ok(step) = server.receive(&bytes) {
        if step.consumed == 0 && step.event.is_none() {
            break;
        }
        bytes.drain(..step.consumed);
    }
    let headers = response_headers(
        200,
        &[
            ("Connection".to_string(), "close".to_string()),
            ("X-Thing".to_string(), "1".to_string()),
        ],
        Some(0),
    );
    server
        .send_headers(id, &headers, true)
        .expect("the core accepts what response_headers produced");
}

/// A HEAD request and the bodyless statuses must not advertise a body, and
/// `send_data` on such a stream is a PROTOCOL_ERROR inside the core.
#[test]
fn bodyless_responses_are_recognized() {
    assert!(body_forbidden(200, true));
    assert!(body_forbidden(204, false));
    assert!(body_forbidden(304, false));
    assert!(body_forbidden(100, false));
    assert!(!body_forbidden(200, false));
    assert!(!body_forbidden(404, false));
}

/// `content-length` is only synthesized when the caller did not set one — two
/// `content-length` fields is `protocol("invalid content-length")`.
#[test]
fn content_length_is_not_duplicated() {
    let headers = response_headers(
        200,
        &[("Content-Length".to_string(), "7".to_string())],
        Some(3),
    );
    let lengths: Vec<_> = headers
        .iter()
        .filter(|h| h.name == "content-length")
        .collect();
    assert_eq!(lengths.len(), 1);
    assert_eq!(lengths[0].value, b"7");
}

/// `session.localSettings` has to report what the core advertises, not what the
/// options object asked for: `Limits` clamps, and a session that reported the
/// unclamped request would be telling JS something the peer was never told.
#[test]
fn advertised_settings_report_the_clamped_values() {
    let mut requested = Http2SettingsState::default();
    requested.max_concurrent_streams = u32::MAX;
    requested.max_frame_size = 1024;
    requested.max_header_list_size = 16;
    let out = advertised_settings(&requested);
    assert_eq!(out.max_concurrent_streams, 128);
    assert_eq!(out.max_frame_size, 16_384);
    assert_eq!(out.max_header_list_size, 4_096);
    // A server never offers push.
    assert!(!out.enable_push);
}

/// `Limits::streams` is both the advertisement and the size of the core's
/// stream table, so an unlimited request cannot be honoured literally — and
/// `Connection::new` refuses `streams == 0` outright.
#[test]
fn clamped_stream_limits_are_constructible() {
    for requested in [0u32, 1, 100, u32::MAX] {
        let mut settings = Http2SettingsState::default();
        settings.max_concurrent_streams = requested;
        let advertised = advertised_settings(&settings);
        let mut limits = Limits::default();
        limits.streams = advertised.max_concurrent_streams as usize;
        limits.frame_size = advertised.max_frame_size as usize;
        limits.header_list = advertised.max_header_list_size as usize;
        Connection::new(Role::Server, limits)
            .unwrap_or_else(|e| panic!("streams={requested} rejected: {}", e.code));
    }
}
