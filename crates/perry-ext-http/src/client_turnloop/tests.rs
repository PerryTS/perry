//! Unit tests for the routing and framing decisions. The end-to-end proof that
//! the transport carries each shape is `tests/turnloop_client_exchange.rs`,
//! which runs in its own process because an agent's loop route is claimed once
//! per thread (see its header).

use super::*;
use turnloop_http::http1;

fn headers(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect()
}

#[test]
fn this_lane_owns_a_slot_no_other_subsystem_claims() {
    // 0 net, 1 this crate's server, 2 stdlib's fetch client, 3 SMTP,
    // 4 fastify, 5 framework server, 7/8 ws, 9-12 the database bindings.
    // The authority for that map is `perry-db-turnloop`'s `subsystem`
    // module header; 6 was the one free slot below the database band.
    assert_eq!(SUBSYSTEM, 6);
    for taken in [0u8, 1, 2, 3, 4, 5, 7, 8, 9, 10, 11, 12] {
        assert_ne!(SUBSYSTEM, taken, "slot {taken} belongs to another lane");
    }
}

/// The three shapes that used to bypass reqwest on raw tokio sockets are
/// selected by the same predicates those modules triggered on.
#[test]
fn a_te_trailers_request_is_left_to_the_raw_socket_bypass() {
    // Name kept from lane 1; the "bypass" is now this module's Trailers mode.
    assert!(wants_trailers(&headers(&[("TE", "trailers")])));
    assert!(wants_trailers(&headers(&[("te", "gzip, trailers")])));
    assert!(!wants_trailers(&headers(&[("te", "gzip")])));
    assert!(!wants_trailers(&headers(&[("accept", "trailers")])));
    assert_eq!(
        mode_for(&headers(&[("TE", "trailers")]), false),
        Mode::Trailers
    );
}

#[test]
fn an_expect_continue_request_is_left_to_the_raw_socket_bypass() {
    // `Expect` does not select a mode by itself: `continue_client` arms the
    // Continue exchange and passes `continue_mode`, exactly as it decided to
    // take the tokio bypass before.
    assert_eq!(
        mode_for(&headers(&[("Expect", "100-continue")]), false),
        Mode::Normal
    );
    assert_eq!(mode_for(&HashMap::new(), true), Mode::Continue);
}

#[test]
fn an_upgrade_request_selects_the_handoff_mode() {
    assert_eq!(
        mode_for(
            &headers(&[("Connection", "Upgrade"), ("Upgrade", "websocket")]),
            false
        ),
        Mode::Upgrade
    );
    // Upgrade wins over trailers, as the old dispatch order did.
    assert_eq!(
        mode_for(
            &headers(&[("Connection", "Upgrade"), ("TE", "trailers")]),
            false
        ),
        Mode::Upgrade
    );
}

/// Lane 1 declined an explicit `Host` because `client::Request::head`
/// rewrites it. This lane serializes its own head, so the caller's `Host`
/// reaches the wire verbatim — pinned here, including the codec behaviour
/// that made the old decline necessary.
#[test]
fn an_explicit_host_header_is_left_to_reqwest() {
    let url = url::Url::parse("http://example.invalid/p").unwrap();
    let serialized = wire::serialize_head(
        "GET",
        "/p",
        &url,
        &headers(&[("Host", "vhost.invalid")]),
        &[],
        0,
        Mode::Normal,
    );
    let text = String::from_utf8(serialized.head).unwrap();
    assert!(text.contains("Host: vhost.invalid\r\n"), "{text}");
    assert!(!text.contains("example.invalid"), "{text}");

    // The codec would have replaced it — the reason `wire.rs` exists.
    let mut request = turnloop_http::client::Request::new("http://example.invalid/p", "GET")
        .expect("valid request");
    request
        .headers
        .push(http1::Header::new("host", "vhost.invalid".as_bytes()));
    let head = request.head(false);
    assert!(head
        .headers
        .iter()
        .all(|h| h.name != "host" || h.value == b"example.invalid"));
}

/// The codec's `Request::new` refuses these; `node:http` does not, and this
/// lane no longer goes through `Request::new`, so they are carried.
#[test]
fn the_codec_refuses_exactly_what_this_lane_declines_on() {
    assert!(turnloop_http::client::Request::new("http://example.invalid/", "TRACE").is_err());
    assert!(turnloop_http::client::Request::new("http://user:pw@example.invalid/", "GET").is_err());
    let url = url::Url::parse("http://example.invalid/").unwrap();
    let text = String::from_utf8(
        wire::serialize_head("TRACE", "/", &url, &HashMap::new(), &[], 0, Mode::Normal).head,
    )
    .unwrap();
    assert!(text.starts_with("TRACE / HTTP/1.1\r\n"), "{text}");
}

/// A GET with no body produces a complete head and nothing else.
#[test]
fn a_bodyless_get_serializes_a_complete_head_and_finishes_its_upload() {
    let url = url::Url::parse("http://example.invalid/start").unwrap();
    let serialized = wire::serialize_head(
        "GET",
        &wire::request_target(&url, false),
        &url,
        &HashMap::new(),
        &[],
        0,
        Mode::Normal,
    );
    let wire_text = String::from_utf8(serialized.head).expect("ascii head");
    assert!(
        wire_text.starts_with("GET /start HTTP/1.1\r\n"),
        "{wire_text}"
    );
    assert!(wire_text.contains("Host: example.invalid\r\n"));
    assert!(!wire_text.contains("Content-Length"));
    assert!(wire_text.ends_with("\r\n\r\n"), "{wire_text}");
}

/// The reason a 3xx needs no redirect policy here: the codec hands the
/// response back verbatim, which is what `node:http` must do.
#[test]
fn a_redirect_response_is_decoded_as_an_ordinary_response() {
    let mut decoder = http1::Decoder::new(http1::Mode::Response, http1::Limits::default());
    decoder.response_to("GET");
    let response =
        b"HTTP/1.1 307 Temporary Redirect\r\nlocation: /target\r\ncontent-length: 8\r\n\r\nredirect";
    let step = decoder.receive(response).expect("a head decodes");
    match step.event {
        Some(http1::Event::Head(head)) => {
            assert_eq!(head.status, 307);
            assert_eq!(
                head.headers
                    .iter()
                    .find(|h| h.name == "location")
                    .map(|h| h.value.clone()),
                Some(b"/target".to_vec())
            );
            assert_eq!(
                wire::reason_phrase(&response[..step.consumed]),
                "Temporary Redirect"
            );
        }
        other => panic!("expected a head, got {other:?}"),
    }
}

/// Keep-alive depends on the decoder's verdict after `End`; a response that
/// asks to close must never be pooled.
#[test]
fn only_a_complete_keep_alive_response_leaves_the_connection_reusable() {
    let mut decoder = http1::Decoder::new(http1::Mode::Response, http1::Limits::default());
    decoder.response_to("GET");
    let mut input: &[u8] = b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\nok";
    loop {
        let step = decoder.receive(input).unwrap();
        let done = matches!(step.event, Some(http1::Event::End));
        input = &input[step.consumed..];
        if done {
            break;
        }
    }
    assert!(decoder.reusable());

    let mut decoder = http1::Decoder::new(http1::Mode::Response, http1::Limits::default());
    decoder.response_to("GET");
    let mut input: &[u8] = b"HTTP/1.1 200 OK\r\nconnection: close\r\ncontent-length: 2\r\n\r\nok";
    loop {
        let step = decoder.receive(input).unwrap();
        let done = matches!(step.event, Some(http1::Event::End));
        input = &input[step.consumed..];
        if done {
            break;
        }
    }
    assert!(!decoder.reusable());
}

#[test]
fn an_ipv6_literal_is_dialed_without_brackets() {
    let url = url::Url::parse("http://[::1]:8080/").unwrap();
    assert_eq!(url.host_str(), Some("[::1]"));
    assert_eq!(protocol::dial_host(&url).as_deref(), Some("::1"));
}

#[test]
fn a_non_http_url_is_refused_with_a_message() {
    let tls = TlsOptions::default();
    let result = prepare(Request {
        request_handle: 1,
        method: "GET",
        url: "ftp://example.invalid/",
        headers: HashMap::new(),
        body: Vec::new(),
        timeout_ms: None,
        agent_handle: 0,
        tls: &tls,
        continue_mode: false,
    });
    assert!(result.is_err());
}
