//! Unit tests for `response.rs`. Split into a sibling file so `response.rs`
//! stays under the repository's 2000-line-per-file lint cap; declared as a
//! `#[path]` child module of `response` so `use super::*` resolves to it.

use super::*;

fn empty_response() -> ServerResponse {
    ServerResponse::new()
}

// `res.once(event, cb)` — one-shot listener semantics. Regression guard for
// the static-file 64 KB truncation: `res.once('drain')` used to be dropped
// entirely (no `once` dispatch arm), so `createReadStream().pipe(res)`
// stalled after the first high-water-mark chunk.

#[test]
fn once_drain_listener_fires_exactly_once() {
    let mut sr = empty_response();
    sr.once_listeners.entry("drain".into()).or_default().push(7);
    // First consumption returns the once listener...
    assert_eq!(take_event_listeners(&mut sr, "drain"), vec![7]);
    // ...and drains it, so a second drain edge sees nothing.
    assert!(take_event_listeners(&mut sr, "drain").is_empty());
}

#[test]
fn on_drain_listener_survives_repeated_edges() {
    let mut sr = empty_response();
    sr.listeners.entry("drain".into()).or_default().push(9);
    // A persistent `on` listener fires on every drain edge.
    assert_eq!(take_event_listeners(&mut sr, "drain"), vec![9]);
    assert_eq!(take_event_listeners(&mut sr, "drain"), vec![9]);
}

#[test]
fn on_and_once_combine_then_once_drops() {
    let mut sr = empty_response();
    sr.listeners.entry("finish".into()).or_default().push(1);
    sr.once_listeners
        .entry("finish".into())
        .or_default()
        .push(2);
    // `on` listeners first, then `once` listeners.
    assert_eq!(take_event_listeners(&mut sr, "finish"), vec![1, 2]);
    // The `once` listener is gone; only the persistent one remains.
    assert_eq!(take_event_listeners(&mut sr, "finish"), vec![1]);
}

// ── P5: `keepAliveTimeout = 0` means "never time out", not "no keep-alive" ──
//
// Node 26.5.1, measured with a raw socket client (`docs/turnloop/p5-report.md`
// carries the numbers): `keepAliveTimeout = 0` answers `Connection:
// keep-alive` with **no** `Keep-Alive` header and never closes the idle
// connection; a finite timeout answers `Keep-Alive: timeout=floor(ms/1000)`
// and FINs at `keepAliveTimeout + keepAliveTimeoutBuffer`. Perry used to fold
// the reuse decision and the timeout together, so a zero timeout produced
// `Connection: close` on every response and no reuse at all.

fn shape_with(headers: Vec<(String, String)>) -> ResponseShape {
    ResponseShape {
        status: 200,
        status_message: None,
        headers,
        trailers: Vec::new(),
        body: Vec::new(),
        auto_content_length: false,
    }
}

fn header_of<'a>(shape: &'a ResponseShape, name: &str) -> Option<&'a str> {
    shape
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

#[test]
fn keep_alive_timeout_zero_keeps_the_connection_and_omits_the_header() {
    let mut shape = shape_with(Vec::new());
    shape.apply_default_connection_headers_for(1, None, 0.0);
    assert_eq!(header_of(&shape, "connection"), Some("keep-alive"));
    assert_eq!(
        header_of(&shape, "keep-alive"),
        None,
        "a disabled timeout advertises none"
    );
}

#[test]
fn a_finite_keep_alive_timeout_advertises_whole_seconds() {
    let mut shape = shape_with(Vec::new());
    shape.apply_default_connection_headers_for(1, None, 5_000.0);
    assert_eq!(header_of(&shape, "connection"), Some("keep-alive"));
    assert_eq!(header_of(&shape, "keep-alive"), Some("timeout=5"));

    // Node floors: 300 ms reports `timeout=0` and still keeps the connection.
    let mut sub_second = shape_with(Vec::new());
    sub_second.apply_default_connection_headers_for(1, None, 300.0);
    assert_eq!(header_of(&sub_second, "connection"), Some("keep-alive"));
    assert_eq!(header_of(&sub_second, "keep-alive"), Some("timeout=0"));
}

#[test]
fn an_explicit_close_request_still_closes_whatever_the_timeout_is() {
    let mut shape = shape_with(Vec::new());
    shape.apply_default_connection_headers_for(1, Some("close"), 0.0);
    assert_eq!(header_of(&shape, "connection"), Some("close"));
    assert_eq!(header_of(&shape, "keep-alive"), None);
}

#[test]
fn http_10_needs_an_explicit_keep_alive_request() {
    let mut implicit = shape_with(Vec::new());
    implicit.apply_default_connection_headers_for(0, None, 5_000.0);
    assert_eq!(header_of(&implicit, "connection"), Some("close"));

    let mut explicit = shape_with(Vec::new());
    explicit.apply_default_connection_headers_for(0, Some("keep-alive"), 5_000.0);
    assert_eq!(header_of(&explicit, "connection"), Some("keep-alive"));
    assert_eq!(header_of(&explicit, "keep-alive"), Some("timeout=5"));
}

#[test]
fn a_handler_set_connection_header_is_never_overridden() {
    let mut shape = shape_with(vec![("Connection".to_string(), "upgrade".to_string())]);
    shape.apply_default_connection_headers_for(1, None, 5_000.0);
    assert_eq!(header_of(&shape, "connection"), Some("upgrade"));
    assert_eq!(header_of(&shape, "keep-alive"), None);
}

#[test]
fn http_2_carries_no_connection_header_at_all() {
    let mut shape = shape_with(Vec::new());
    shape.apply_default_connection_headers_for(2, None, 5_000.0);
    assert_eq!(header_of(&shape, "connection"), None);
    assert_eq!(header_of(&shape, "keep-alive"), None);
}

#[test]
fn buffered_end_does_not_infer_a_length_after_write_head() {
    use crate::server::turnloop_serve::wire::{framing_for, Framing};
    for body in [b"".as_slice(), b"hello".as_slice()] {
        let mut implicit = empty_response();
        implicit.buffered_body.extend_from_slice(body);
        assert!(implicit.ensure_content_length());
        assert_eq!(
            framing_for(&implicit.snapshot_headers(), 200, "GET", 1, None, false),
            Framing::Sized(body.len() as u64)
        );

        let mut committed = empty_response();
        committed.header_committed = true;
        committed.buffered_body.extend_from_slice(body);
        assert!(!committed.ensure_content_length());
        assert!(!committed.native_headers.contains_key("content-length"));
        assert_eq!(
            framing_for(&committed.snapshot_headers(), 200, "GET", 1, None, false),
            Framing::Chunked
        );
        assert_eq!(
            framing_for(&committed.snapshot_headers(), 200, "GET", 0, None, false),
            Framing::UntilClose
        );
    }
}

#[test]
fn buffered_end_preserves_explicit_framing_after_write_head() {
    use crate::server::turnloop_serve::wire::{framing_for, Framing};
    for (header, value, expected) in [
        ("content-length", "5", Framing::Sized(5)),
        ("transfer-encoding", "chunked", Framing::Chunked),
    ] {
        let mut response = empty_response();
        response.header_committed = true;
        response.buffered_body.extend_from_slice(b"hello");
        response.native_headers.insert(header.into(), value.into());
        response.remember_header(header);
        assert!(!response.ensure_content_length());
        assert_eq!(response.native_headers.get(header).unwrap(), value);
        assert_eq!(
            framing_for(&response.snapshot_headers(), 200, "GET", 1, None, false),
            expected
        );
    }
}
