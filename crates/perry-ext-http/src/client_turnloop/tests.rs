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
    assert_eq!(conn::dial_host(&url).as_deref(), Some("::1"));
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

// ── The state machine, driven by hand ───────────────────────────────────────
//
// These feed the sink handlers directly and inspect the effects they return
// instead of performing them, so no loop is needed and nothing is left to a
// scheduling lottery. `GC_TEST_LOCK` serializes them with the crate tests that
// read the process-wide in-flight count and event queue.

fn lock() -> std::sync::MutexGuard<'static, ()> {
    crate::tests::GC_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn outbound(request_handle: Handle, url: &str, pairs: &[(&str, &str)]) -> Outbound {
    let tls = TlsOptions::default();
    prepare(Request {
        request_handle,
        method: "GET",
        url,
        headers: headers(pairs),
        body: Vec::new(),
        timeout_ms: None,
        agent_handle: 0,
        tls: &tls,
        continue_mode: false,
    })
    .unwrap_or_else(|message| panic!("request prepares: {message}"))
}

fn connect_id(fx: &[Effect]) -> i64 {
    fx.iter()
        .find_map(|e| match e {
            Effect::Connect { id, .. } => Some(*id),
            _ => None,
        })
        .expect("a new connection is dialed")
}

fn written(fx: &[Effect]) -> Vec<u8> {
    fx.iter()
        .filter_map(|e| match e {
            Effect::Write(_, bytes) => Some(bytes.clone()),
            _ => None,
        })
        .flatten()
        .collect()
}

fn pushed(fx: &[Effect]) -> Vec<&'static str> {
    fx.iter()
        .filter_map(|e| match e {
            Effect::Push(event) => Some(match event {
                PendingHttpEvent::ResponseHead { .. } => "head",
                PendingHttpEvent::ResponseChunk { .. } => "chunk",
                PendingHttpEvent::ResponseEnd { .. } => "end",
                PendingHttpEvent::Response { .. } => "response",
                PendingHttpEvent::Continue { .. } => "continue",
                PendingHttpEvent::Timeout { .. } => "timeout",
                PendingHttpEvent::Error { .. } => "error",
                PendingHttpEvent::CodedError { .. } => "coded-error",
                PendingHttpEvent::TransportError { .. } => "transport-error",
                _ => "other",
            }),
            _ => None,
        })
        .collect()
}

fn closes(fx: &[Effect], id: i64) -> bool {
    fx.iter().any(|e| matches!(e, Effect::Close(c) if *c == id))
}

/// Release whatever a test left in the shared state, as `NET_CLOSED` would.
fn forget_conn(id: i64) {
    let fx = with_state(|st| conn::on_closed(st, id));
    for effect in fx {
        if let Effect::FreeId(freed) = effect {
            perry_ffi::free_handle_id(freed);
        }
    }
}

/// Was a `tests.rs` test of the reqwest dispatch (#5892 remainder /
/// issue_4909 early exit): from the moment a request is dispatched until its
/// response events are queued, the exchange must be visible to the exit gate.
/// The in-flight guard is now owned by the exchange itself, so it is held
/// from `start` — before the connect even completes — and released exactly
/// when the terminal event is produced.
#[test]
fn dispatch_request_stays_visible_to_exit_gate_until_response_queued() {
    let _lock = lock();
    let request_handle = 0x7e57_0001;
    let baseline = crate::js_ext_http_client_inflight();
    let fx =
        with_state(|st| start_locked(st, outbound(request_handle, "http://127.0.0.1:9/", &[])));
    let id = connect_id(&fx);
    assert!(
        crate::js_ext_http_client_inflight() > baseline,
        "in-flight guard must be held from dispatch, before the connect completes"
    );
    let fx = with_state(|st| conn::on_connect(st, id));
    assert!(written(&fx).starts_with(b"GET / HTTP/1.1\r\n"));
    assert!(crate::js_ext_http_client_inflight() > baseline);

    // The head arrives split mid-line across two reads: the first must be
    // retained, not dropped.
    let fx = with_state(|st| conn::on_data(st, id, b"HTTP/1.1 200 Fine By Me\r\ncontent-le"));
    assert!(pushed(&fx).is_empty(), "no event from half a head");
    assert!(crate::js_ext_http_client_inflight() > baseline);
    let fx = with_state(|st| conn::on_data(st, id, b"ngth: 2\r\n\r\nok"));
    assert_eq!(pushed(&fx), ["head", "chunk", "end"]);
    let reason = fx.iter().find_map(|e| match e {
        Effect::Push(PendingHttpEvent::ResponseHead { status_message, .. }) => {
            Some(status_message.clone())
        }
        _ => None,
    });
    assert_eq!(
        reason.as_deref(),
        Some("Fine By Me"),
        "the server's own reason phrase"
    );
    assert!(closes(&fx, id), "no agent: the connection is not pooled");
    assert_eq!(
        crate::js_ext_http_client_inflight(),
        baseline,
        "the guard must release with the terminal event"
    );
    forget_conn(id);
}

#[test]
fn a_kept_alive_connection_is_parked_after_end_and_reused() {
    let _lock = lock();
    let agent = 0x7e57_a9e1;
    let mut first = outbound(0x7e57_0010, "http://127.0.0.1:9/a", &[]);
    first.key.agent = agent;
    first.reuse = Some(Reuse {
        max_free: 2,
        idle_ms: 60_000,
    });
    let fx = with_state(|st| start_locked(st, first));
    let id = connect_id(&fx);
    with_state(|st| conn::on_connect(st, id));
    let fx =
        with_state(|st| conn::on_data(st, id, b"HTTP/1.1 200 OK\r\ncontent-length: 1\r\n\r\na"));
    assert_eq!(pushed(&fx), ["head", "chunk", "end"]);
    assert!(
        !closes(&fx, id),
        "a complete keep-alive response parks the connection"
    );
    assert!(fx
        .iter()
        .any(|e| matches!(e, Effect::SetRef(c, false) if *c == id)));

    let reused_before = reused_total();
    let mut second = outbound(0x7e57_0011, "http://127.0.0.1:9/b", &[]);
    second.key.agent = agent;
    second.reuse = Some(Reuse {
        max_free: 2,
        idle_ms: 60_000,
    });
    let fx = with_state(|st| start_locked(st, second));
    assert!(
        !fx.iter().any(|e| matches!(e, Effect::Connect { .. })),
        "the parked connection is reused, not a new one dialed"
    );
    assert!(written(&fx).starts_with(b"GET /b HTTP/1.1\r\n"));
    assert_eq!(reused_total(), reused_before + 1);

    // The stale keep-alive race: the peer closes before any response byte.
    // The request goes again on a fresh connection instead of failing.
    let fx = with_state(|st| conn::on_eof(st, id));
    assert!(
        fx.iter()
            .any(|e| matches!(e, Effect::Redispatch(out, _) if out.request_handle == 0x7e57_0011)),
        "a reused connection that dies before the response is retried once"
    );
    assert!(pushed(&fx).is_empty(), "and no error reaches the request");
    forget_conn(id);
}

#[test]
fn a_response_that_closes_is_never_pooled() {
    let _lock = lock();
    let mut out = outbound(0x7e57_0020, "http://127.0.0.1:9/", &[]);
    out.key.agent = 0x7e57_a9e2;
    out.reuse = Some(Reuse {
        max_free: 2,
        idle_ms: 60_000,
    });
    let fx = with_state(|st| start_locked(st, out));
    let id = connect_id(&fx);
    with_state(|st| conn::on_connect(st, id));
    let fx = with_state(|st| {
        conn::on_data(
            st,
            id,
            b"HTTP/1.1 200 OK\r\nconnection: close\r\ncontent-length: 1\r\n\r\na",
        )
    });
    assert_eq!(pushed(&fx), ["head", "chunk", "end"]);
    assert!(closes(&fx, id));
    forget_conn(id);
}

#[test]
fn a_deadline_times_the_exchange_out_and_closes_it() {
    let _lock = lock();
    let mut out = outbound(0x7e57_0030, "http://127.0.0.1:9/", &[]);
    out.timeout_ms = Some(50);
    let fx = with_state(|st| start_locked(st, out));
    let id = connect_id(&fx);
    let timer = fx
        .iter()
        .find_map(|e| match e {
            Effect::ArmTimer(timer, 50) => Some(*timer),
            _ => None,
        })
        .expect("the deadline is armed at dispatch");
    let fx = with_state(|st| conn::on_timer(st, timer));
    assert_eq!(pushed(&fx), ["timeout"]);
    assert!(closes(&fx, id));
    // A response racing the deadline is dropped, not delivered twice.
    let fx =
        with_state(|st| conn::on_data(st, id, b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\n\r\n"));
    assert!(pushed(&fx).is_empty());
    forget_conn(id);
}

#[test]
fn a_close_before_the_head_is_a_socket_hang_up() {
    let _lock = lock();
    let fx = with_state(|st| start_locked(st, outbound(0x7e57_0040, "http://127.0.0.1:9/", &[])));
    let id = connect_id(&fx);
    with_state(|st| conn::on_connect(st, id));
    let fx = with_state(|st| conn::on_eof(st, id));
    assert_eq!(pushed(&fx), ["coded-error"]);
    let message = fx.iter().find_map(|e| match e {
        Effect::Push(PendingHttpEvent::CodedError { message, code, .. }) => {
            Some((message.clone(), code.clone()))
        }
        _ => None,
    });
    assert_eq!(
        message,
        Some(("socket hang up".to_string(), "ECONNRESET".to_string()))
    );
    forget_conn(id);
}

#[test]
fn a_body_delimited_by_the_close_ends_at_eof() {
    let _lock = lock();
    let fx = with_state(|st| start_locked(st, outbound(0x7e57_0050, "http://127.0.0.1:9/", &[])));
    let id = connect_id(&fx);
    with_state(|st| conn::on_connect(st, id));
    let fx = with_state(|st| conn::on_data(st, id, b"HTTP/1.0 200 OK\r\n\r\npartial"));
    assert_eq!(pushed(&fx), ["head", "chunk"]);
    let fx = with_state(|st| conn::on_eof(st, id));
    assert_eq!(pushed(&fx), ["end"]);
    forget_conn(id);
}

#[test]
fn a_connect_failure_names_the_peer_as_node_does() {
    let _lock = lock();
    let fx = with_state(|st| start_locked(st, outbound(0x7e57_0060, "http://127.0.0.1:1/", &[])));
    let id = connect_id(&fx);
    let fx = with_state(|st| conn::on_error(st, id, "ECONNREFUSED", "connect", -111));
    let message = fx.iter().find_map(|e| match e {
        Effect::Push(PendingHttpEvent::TransportError { message, .. }) => Some(message.clone()),
        _ => None,
    });
    assert_eq!(message.as_deref(), Some("connect ECONNREFUSED 127.0.0.1:1"));
    forget_conn(id);
}

#[test]
fn trailers_are_delivered_with_the_buffered_response() {
    let _lock = lock();
    let fx = with_state(|st| {
        start_locked(
            st,
            outbound(0x7e57_0070, "http://127.0.0.1:9/", &[("TE", "trailers")]),
        )
    });
    let id = connect_id(&fx);
    let fx = with_state(|st| conn::on_connect(st, id));
    assert!(String::from_utf8_lossy(&written(&fx)).contains("Connection: close\r\n"));
    let fx = with_state(|st| {
        conn::on_data(
            st,
            id,
            b"HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\n\r\n2\r\nok\r\n0\r\nx-sum: 7\r\n\r\n",
        )
    });
    assert_eq!(pushed(&fx), ["response"]);
    let delivered = fx.iter().find_map(|e| match e {
        Effect::Push(PendingHttpEvent::Response { body, trailers, .. }) => {
            Some((body.clone(), trailers.clone()))
        }
        _ => None,
    });
    assert_eq!(
        delivered,
        Some((b"ok".to_vec(), vec![("x-sum".to_string(), "7".to_string())]))
    );
    forget_conn(id);
}

#[test]
fn an_expect_continue_body_waits_for_the_interim_response() {
    let _lock = lock();
    let tls = TlsOptions::default();
    let out = prepare(Request {
        request_handle: 0x7e57_0080,
        method: "POST",
        url: "http://127.0.0.1:9/up",
        headers: headers(&[("Expect", "100-continue")]),
        body: Vec::new(),
        timeout_ms: None,
        agent_handle: 0,
        tls: &tls,
        continue_mode: true,
    })
    .unwrap_or_else(|message| panic!("{message}"));
    let fx = with_state(|st| start_locked(st, out));
    let id = connect_id(&fx);
    let fx = with_state(|st| conn::on_connect(st, id));
    let head = String::from_utf8(written(&fx)).unwrap();
    assert!(head.contains("Transfer-Encoding: chunked\r\n"), "{head}");
    assert!(head.ends_with("\r\n\r\n"), "only the head goes out: {head}");

    // `end()` runs before the server's `100`: the body is held.
    let fx = with_state(|st| {
        let mut fx = Vec::new();
        conn::continue_body(st, 0x7e57_0080, b"data".to_vec(), &mut fx);
        fx
    });
    assert!(
        written(&fx).is_empty(),
        "the body waits for the interim 100"
    );
    let fx = with_state(|st| conn::on_data(st, id, b"HTTP/1.1 100 Continue\r\n\r\n"));
    assert_eq!(pushed(&fx), ["continue"]);
    assert_eq!(written(&fx), b"4\r\ndata\r\n0\r\n\r\n");
    let fx =
        with_state(|st| conn::on_data(st, id, b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\n\r\n"));
    assert_eq!(pushed(&fx), ["head", "end"]);
    forget_conn(id);
}

#[test]
fn a_101_hands_the_connection_to_net_with_the_bytes_after_the_head() {
    let _lock = lock();
    let fx = with_state(|st| {
        start_locked(
            st,
            outbound(
                0x7e57_0090,
                "http://127.0.0.1:9/ws",
                &[("Connection", "Upgrade"), ("Upgrade", "websocket")],
            ),
        )
    });
    let id = connect_id(&fx);
    with_state(|st| conn::on_connect(st, id));
    let fx = with_state(|st| {
        conn::on_data(
            st,
            id,
            b"HTTP/1.1 101 Switching Protocols\r\nupgrade: websocket\r\nconnection: upgrade\r\n\r\n\x81\x00",
        )
    });
    let handoff = fx.iter().find_map(|e| match e {
        Effect::Handoff(c, PendingHttpEvent::Upgrade { head, status, .. }) if *c == id => {
            Some((*status, head.clone()))
        }
        _ => None,
    });
    assert_eq!(handoff, Some((101, vec![0x81, 0x00])));
    assert!(!closes(&fx, id), "the handle now belongs to net");
    assert!(
        with_state(|st| !st.conns.contains_key(&id)),
        "the transport forgets the connection without closing it"
    );
    perry_ffi::free_handle_id(id);
}

#[test]
fn a_cancelled_request_is_closed_without_an_event() {
    let _lock = lock();
    let fx = with_state(|st| start_locked(st, outbound(0x7e57_00a0, "http://127.0.0.1:9/", &[])));
    let id = connect_id(&fx);
    let fx = with_state(|st| {
        let mut fx = Vec::new();
        conn::cancel(st, 0x7e57_00a0, &mut fx);
        fx
    });
    assert!(pushed(&fx).is_empty());
    assert!(closes(&fx, id));
    let fx = with_state(|st| conn::on_closed(st, id));
    assert!(pushed(&fx).is_empty(), "no late event after a cancel");
    for effect in fx {
        if let Effect::FreeId(freed) = effect {
            perry_ffi::free_handle_id(freed);
        }
    }
}
