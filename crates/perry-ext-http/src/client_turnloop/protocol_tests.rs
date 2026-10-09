//! The framing contract after moving transport ownership to Socket. These
//! witnesses exercise fragmented input, retries and terminal effects directly.
use super::*;
use std::collections::HashMap;

fn request(mode: Mode) -> Outbound {
    Outbound {
        request_handle: 123,
        method: "GET".into(),
        url: url::Url::parse("http://127.0.0.1:9/path").unwrap(),
        headers: HashMap::new(),
        body: Vec::new(),
        timeout_ms: None,
        mode,
        reuse: None,
        key: PoolKey {
            agent: 7,
            https: false,
            host: "127.0.0.1".into(),
            port: 9,
            proxy: None,
            tls: 0,
        },
        tls: None,
        proxy: None,
        extra: Vec::new(),
    }
}
fn writes(effects: &[Effect]) -> Vec<u8> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::Write(bytes) => Some(bytes.as_slice()),
            _ => None,
        })
        .flatten()
        .copied()
        .collect()
}
fn kinds(effects: &[Effect]) -> Vec<&'static str> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::Push(PendingHttpEvent::ResponseHead { .. }) => Some("head"),
            Effect::Push(PendingHttpEvent::ResponseChunk { .. }) => Some("chunk"),
            Effect::Push(PendingHttpEvent::ResponseEnd { .. }) => Some("end"),
            Effect::Push(PendingHttpEvent::Response { .. }) => Some("response"),
            Effect::Push(PendingHttpEvent::Continue { .. }) => Some("continue"),
            Effect::Push(PendingHttpEvent::Timeout { .. }) => Some("timeout"),
            Effect::RetireRequest(_) => Some("retire"),
            Effect::Close => Some("close"),
            _ => None,
        })
        .collect()
}
fn connected(out: Outbound) -> Conn {
    let mut conn = Conn::new(out, false, None);
    let sent = on_connect(&mut conn);
    assert!(writes(&sent).starts_with(b"GET /path HTTP/1.1\r\n"));
    conn
}
#[test]
fn fragmented_headers_stream_one_head_then_body_and_retire_once() {
    let mut conn = connected(request(Mode::Normal));
    assert!(on_data(&mut conn, b"HTTP/1.1 200 Fine By Me\r\ncontent-le").is_empty());
    let effects = on_data(&mut conn, b"ngth: 2\r\n\r\nok");
    assert_eq!(kinds(&effects), ["head", "chunk", "retire", "end", "close"]);
    assert!(effects.iter().any(|effect| matches!(effect,
        Effect::Push(PendingHttpEvent::ResponseHead { status_message, .. }) if status_message == "Fine By Me")));
    assert!(conn.request().is_none());
    assert!(on_closed(&mut conn).is_empty());
}
#[test]
fn chunked_trailers_are_buffered_and_delivered_together() {
    let mut conn = connected(request(Mode::Trailers));
    let effects = on_data(
        &mut conn,
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\nok\r\n0\r\nX-End: yes\r\n\r\n",
    );
    assert_eq!(kinds(&effects), ["retire", "response", "close"]);
    assert!(effects.iter().any(|effect| matches!(effect,
        Effect::Push(PendingHttpEvent::Response { body, trailers, .. })
            if body == b"ok" && trailers == &[("x-end".into(), "yes".into())])));
}
#[test]
fn upgrade_returns_binary_head_once_and_tls_remains_outside_the_parser() {
    let mut out = request(Mode::Upgrade);
    out.key.https = true;
    // A user-supplied TLS socket already owns its session.
    let mut conn = Conn::new(out, true, None);
    on_connect(&mut conn);
    let effects = on_data(&mut conn,
        b"HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: raw\r\n\r\n\0HEAD\xff");
    assert!(effects.iter().any(|effect| matches!(effect,
        Effect::Upgrade { request: 123, status: 101, head, .. } if head == b"\0HEAD\xff")));
    assert!(!effects
        .iter()
        .any(|effect| matches!(effect, Effect::Push(PendingHttpEvent::ResponseHead { .. }))));
    assert!(!effects.iter().any(|effect| matches!(effect, Effect::Close)));
    assert!(conn.request().is_none());
    assert!(on_closed(&mut conn).is_empty());
}
#[test]
fn continue_waits_for_interim_then_sends_the_supplied_body() {
    let mut out = request(Mode::Continue);
    out.body = b"held".to_vec();
    let mut conn = Conn::new(out, false, None);
    let initial = on_connect(&mut conn);
    assert!(!writes(&initial).ends_with(b"held"));
    let mut supplied = Vec::new();
    continue_body(&mut conn, 123, b"held".to_vec(), &mut supplied);
    assert!(supplied.is_empty());
    let interim = on_data(&mut conn, b"HTTP/1.1 100 Continue\r\n\r\n");
    assert_eq!(kinds(&interim), ["continue"]);
    assert_eq!(writes(&interim), b"4\r\nheld\r\n0\r\n\r\n");
    let end = on_data(&mut conn, b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
    assert_eq!(kinds(&end), ["head", "retire", "end", "close"]);
}
#[test]
fn close_delimited_body_finishes_on_eof() {
    let mut conn = connected(request(Mode::Normal));
    let first = on_data(&mut conn, b"HTTP/1.0 200 OK\r\n\r\nbody");
    assert_eq!(kinds(&first), ["head", "chunk"]);
    let end = on_eof(&mut conn);
    assert_eq!(kinds(&end), ["retire", "end", "close"]);
    assert!(on_closed(&mut conn).is_empty());
}
#[test]
fn reused_socket_without_response_bytes_retries_once() {
    let mut out = request(Mode::Normal);
    out.reuse = Some(Reuse {
        max_free: 4,
        idle_ms: 1000,
    });
    let mut conn = connected(out);
    let effects = on_data(&mut conn, b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\n\r\na");
    assert!(effects
        .iter()
        .any(|effect| matches!(effect, Effect::Park(_))));
    assert!(conn.reusable());
    let effects = conn.restart(request(Mode::Normal));
    assert!(!writes(&effects).is_empty());
    let effects = on_eof(&mut conn);
    let out = effects
        .into_iter()
        .find_map(|effect| match effect {
            Effect::Redispatch(out) => Some(*out),
            _ => None,
        })
        .expect("a pooled connection that received no bytes retries");
    assert_eq!(out.request_handle, 123);
    let mut retry = Conn::retry(out);
    on_connect(&mut retry);
    let effects = on_eof(&mut retry);
    assert!(!effects
        .iter()
        .any(|effect| matches!(effect, Effect::Redispatch(_))));
    assert!(effects.iter().any(|effect| matches!(effect,
        Effect::Push(PendingHttpEvent::CodedError { code, .. }) if code == "ECONNRESET")));
}
#[test]
fn deadline_retires_the_request_and_discards_later_response_bytes() {
    let mut conn = connected(request(Mode::Normal));
    let effects = on_timer(&mut conn);
    assert_eq!(kinds(&effects), ["retire", "timeout", "close"]);
    assert!(on_data(&mut conn, b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n").is_empty());
    assert!(on_closed(&mut conn).is_empty());
}
#[test]
fn connect_error_preserves_node_peer_and_transport_fields() {
    let mut conn = Conn::new(request(Mode::Normal), false, None);
    let effects = on_error(&mut conn, "ECONNREFUSED", "connect", -111);
    assert!(effects.iter().any(|effect| matches!(effect,
        Effect::Push(PendingHttpEvent::TransportError { message, code, syscall, errno, .. })
        if message == "connect ECONNREFUSED 127.0.0.1:9" && code == "ECONNREFUSED" && syscall == "connect" && *errno == -111)));
    assert!(conn.request().is_none());
}

#[test]
fn a_cancelled_request_is_closed_without_a_late_event() {
    let mut conn = connected(request(Mode::Normal));
    let fx = cancel(&mut conn, 123);
    assert!(fx.iter().any(|e| matches!(e, Effect::Close)));
    assert!(fx.iter().any(|e| matches!(e, Effect::RetireRequest(123))));
    assert!(!fx.iter().any(|e| matches!(e, Effect::Push(_))));
    assert!(on_closed(&mut conn).is_empty());
}
#[test]
fn a_response_that_closes_is_never_pooled() {
    let mut out = request(Mode::Normal);
    out.reuse = Some(Reuse {
        max_free: 4,
        idle_ms: 1000,
    });
    let mut conn = connected(out);
    let fx = on_data(
        &mut conn,
        b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 1\r\n\r\na",
    );
    assert!(fx.iter().any(|e| matches!(e, Effect::Close)));
    assert!(!fx.iter().any(|e| matches!(e, Effect::Park(_))));
    assert!(!conn.reusable());
}
#[test]
fn a_close_before_the_head_is_a_socket_hang_up() {
    let mut conn = connected(request(Mode::Normal));
    let fx = on_eof(&mut conn);
    assert!(fx.iter().any(|e| matches!(e, Effect::Push(PendingHttpEvent::CodedError { message, code, .. }) if message == "socket hang up" && code == "ECONNRESET")));
    assert!(on_closed(&mut conn).is_empty());
}
