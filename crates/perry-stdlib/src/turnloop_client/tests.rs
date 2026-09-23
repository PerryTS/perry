//! P6 acceptance for the HTTP client engine.
//!
//! Every test here asserts its *subject*, not merely that nothing threw. The
//! codec, the pool, the redirect policy and the decompressor are all sans-I/O,
//! so they can be driven with real bytes and no socket — which is what makes
//! these tests able to fail for the reason they exist.

use std::time::{Duration, Instant};

use turnloop_http::client::{self as tlc, Acquire, PoolKey, RedirectMode};
use turnloop_http::compression::StreamingDecoder;
use turnloop_http::http1;

use super::exchange;

/// The ids this engine names turnloop handles with must not collide with the
/// two handle registries that both run `[1, 0x40000)` — perry-ffi's (which
/// `perry-ext-net` names its sockets from) and perry-stdlib's `common` map.
/// `turnloop_net` keys EVERY handle on a thread in one map, so a collision is
/// one subsystem's completion reaching another's socket.
#[test]
fn ids_are_disjoint_from_the_binding_bands() {
    let common_end = perry_runtime::value::addr_class::COMMON_HANDLE_BAND_END as i64;
    assert!(
        super::ID_BASE > common_end,
        "the HTTP client band must start above the handle registries' shared range"
    );
    assert!(
        super::ID_BASE > crate::turnloop_smtp::id_base_for_test()
            || super::ID_BASE + (1 << 20) < crate::turnloop_smtp::id_base_for_test(),
        "the two P6 engines must not overlap"
    );
    assert!(
        super::ID_CEILING < (1i64 << 56),
        "an id must fit turnloop_net's 56-bit token field"
    );
    assert!(super::SUBSYSTEM != 0, "slot 0 belongs to perry-ext-net");
    assert!(
        (super::SUBSYSTEM as usize) < perry_runtime::turnloop_net::MAX_SUBSYSTEMS,
        "register_sink refuses an out-of-range slot, and a binding that picked \
         one would look like a socket that never produces events"
    );
}

/// A complete, correctly framed response must reach `Event::End` — and it takes
/// one more `receive` call than the bytes require.
///
/// This is the regression test for the defect that made every fetch cost five
/// seconds: `State::End -> Done` is a transition, not a parse, so the `End`
/// event arrives from a step that consumes ZERO bytes. A feed loop that stops
/// at `pos >= input.len()` never asks for it, the request never completes on
/// data alone, and the only thing that finishes it is the server's keep-alive
/// timeout closing the socket — which also makes the connection unreusable.
///
/// The two halves are asserted against each other so a future rewrite of the
/// loop cannot quietly reintroduce it.
#[test]
fn the_end_event_arrives_from_a_step_that_consumes_nothing() {
    let response =
        b"HTTP/1.1 200 OK\r\ncontent-length: 5\r\nconnection: keep-alive\r\n\r\nhello".to_vec();

    // The old rule: stop as soon as every byte has been handed over.
    let mut conn = start_get();
    let mut pos = 0;
    let mut saw_end_old = false;
    loop {
        let step = conn.receive(&response[pos..]).expect("decodes");
        let consumed = step.consumed;
        if matches!(step.event, Some(http1::Event::End)) {
            saw_end_old = true;
        }
        pos += consumed;
        if consumed == 0 || pos >= response.len() {
            break;
        }
    }
    assert!(
        !saw_end_old,
        "if this ever becomes true the defect is no longer reachable and this \
         test has stopped discriminating"
    );

    // The rule the engine uses: keep asking while a step either consumed a byte
    // or produced an event, and stop on `End` (which is what `Produced::End`
    // does — the exchange is over and the connection goes back to the pool).
    let mut conn = start_get();
    let mut pos = 0;
    let mut saw_end_new = false;
    let mut reusable = false;
    loop {
        let step = conn.receive(&response[pos..]).expect("decodes");
        let consumed = step.consumed;
        let produced = step.event.is_some();
        let ended = matches!(step.event, Some(http1::Event::End));
        pos += consumed;
        if ended {
            saw_end_new = true;
            let _ = conn.poll_completion();
            reusable = conn.reusable();
            break;
        }
        if consumed == 0 && !produced {
            break;
        }
    }
    assert!(saw_end_new, "the response must complete on data alone");
    assert!(
        reusable,
        "a keep-alive response that completed must leave the connection reusable"
    );
}

fn start_get() -> tlc::Http1Connection {
    let mut conn = tlc::Http1Connection::new(http1::Limits::default());
    let head = http1::Head {
        method: "GET".into(),
        target: "/x".into(),
        status: 0,
        version: 1,
        headers: vec![http1::Header::new("host", b"example.test")],
        keep_alive: true,
    };
    conn.start(&head, http1::BodyLength::Empty, None, None)
        .expect("start");
    let n = conn.output().len();
    conn.consume_output(n).expect("consume");
    conn.finish_body(&[]).expect("finish");
    conn
}

/// The framing decision the engine makes for a body, against what Node's fetch
/// and reqwest both put on the wire.
#[test]
fn body_framing_matches_the_shape_both_engines_send() {
    let empty_get = exchange::body_length_for_test("GET", 0);
    assert!(matches!(empty_get, http1::BodyLength::Empty));
    let empty_post = exchange::body_length_for_test("POST", 0);
    assert!(matches!(empty_post, http1::BodyLength::Known(0)));
    let sized = exchange::body_length_for_test("POST", 7);
    assert!(matches!(sized, http1::BodyLength::Known(7)));
    // A bodyless DELETE sends no content-length, the same as a GET.
    assert!(matches!(
        exchange::body_length_for_test("DELETE", 0),
        http1::BodyLength::Empty
    ));

    // And the encoder really writes what that implies.
    let mut out = Vec::new();
    let head = http1::Head {
        method: "POST".into(),
        target: "/x".into(),
        status: 0,
        version: 1,
        headers: vec![http1::Header::new("host", b"example.test")],
        keep_alive: true,
    };
    http1::Encoder::start(&head, http1::BodyLength::Known(0), &mut out).expect("encode");
    let text = String::from_utf8(out).expect("ascii");
    assert!(
        text.contains("content-length: 0"),
        "a bodyless POST must be framed, not open-ended: {text:?}"
    );
}

/// The redirect policy Perry relies on, driven through the crate's own
/// `Request::redirect` rather than reimplemented here.
#[test]
fn redirects_rewrite_and_strip_the_way_fetch_requires() {
    // 303 turns any method into GET and drops the body.
    let mut request = tlc::Request::new("http://a.test/one", "POST").expect("url");
    request.body = b"payload".to_vec();
    let again = request
        .redirect(303, Some("/two"), RedirectMode::Follow, 20)
        .expect("redirect");
    assert!(again);
    assert_eq!(request.method, "GET");
    assert!(request.body.is_empty());
    assert_eq!(request.url.path(), "/two");

    // A cross-origin hop strips credentials.
    let mut request = tlc::Request::new("http://a.test/one", "GET").expect("url");
    request.headers = vec![
        http1::Header::new("authorization", b"Bearer secret"),
        http1::Header::new("cookie", b"sid=1"),
        http1::Header::new("x-keep", b"yes"),
    ];
    let again = request
        .redirect(307, Some("http://b.test/two"), RedirectMode::Follow, 20)
        .expect("redirect");
    assert!(again);
    let names: Vec<&str> = request.headers.iter().map(|h| h.name.as_str()).collect();
    assert!(!names.contains(&"authorization"), "{names:?}");
    assert!(!names.contains(&"cookie"), "{names:?}");
    assert!(names.contains(&"x-keep"), "{names:?}");

    // `manual` exposes the 3xx as-is, and a non-redirect status is never one.
    let mut request = tlc::Request::new("http://a.test/one", "GET").expect("url");
    assert!(!request
        .redirect(302, Some("/two"), RedirectMode::Manual, 20)
        .expect("manual"));
    assert!(!request
        .redirect(200, None, RedirectMode::Follow, 20)
        .expect("not a redirect"));

    // The hop limit is enforced rather than looping.
    let mut request = tlc::Request::new("http://a.test/one", "GET").expect("url");
    for _ in 0..3 {
        assert!(request
            .redirect(302, Some("/next"), RedirectMode::Follow, 3)
            .expect("under the limit"));
    }
    assert!(request
        .redirect(302, Some("/next"), RedirectMode::Follow, 3)
        .is_err());
}

/// The pool reuses an idle connection for the same origin, refuses to overbook,
/// and ages one out — the three behaviours that decide how many sockets a
/// long-running service opens.
#[test]
fn the_pool_reuses_within_an_origin_and_ages_out() {
    let mut pool = tlc::Pool::new(2, Duration::from_millis(50));
    let a = PoolKey {
        origin: "http://a.test".into(),
        proxy: None,
    };
    let b = PoolKey {
        origin: "http://b.test".into(),
        proxy: None,
    };
    let now = Instant::now();

    let Acquire::Connect(first) = pool.acquire(&a, now) else {
        panic!("a cold origin must connect");
    };
    pool.connected(first, tlc::Protocol::Http1, 1).expect("up");
    pool.release(first, true, now).expect("release");
    assert!(
        matches!(pool.acquire(&a, now), Acquire::Reuse(id) if id == first),
        "an idle keep-alive connection must be reused, not replaced"
    );
    pool.release(first, true, now).expect("release");

    // A different origin never reuses it.
    assert!(matches!(pool.acquire(&b, now), Acquire::Connect(_)));

    // At the per-host limit the third caller waits instead of opening a socket.
    let Acquire::Reuse(_) = pool.acquire(&a, now) else {
        panic!("reuse");
    };
    let Acquire::Connect(second) = pool.acquire(&a, now) else {
        panic!("a second connection is within max_per_host=2");
    };
    pool.connected(second, tlc::Protocol::Http1, 1).expect("up");
    assert!(matches!(pool.acquire(&a, now), Acquire::Wait));

    // An idle connection past the deadline is handed back for closing.
    pool.release(second, true, now).expect("release");
    let later = now + Duration::from_millis(100);
    assert_eq!(pool.handle_timeout(later), Some(second));
}

/// Every `Content-Encoding` Node's fetch decompresses must round-trip here,
/// asserted on the CONTENT rather than on "no error".
#[test]
fn every_supported_content_encoding_round_trips() {
    let payload: Vec<u8> = (0..4096u32).map(|i| (i % 251) as u8).collect();

    for (name, encoded) in [
        ("gzip", gzip(&payload)),
        ("deflate", zlib_deflate(&payload)),
    ] {
        let mut out = Vec::new();
        turnloop_http::compression::decode(name, &encoded, &mut out, super::BODY_LIMIT)
            .unwrap_or_else(|e| panic!("{name} decode: {e}"));
        assert_eq!(out, payload, "{name} round trip");
    }

    // Incremental decoding produces the same bytes as the whole-body call: that
    // is the path a real response takes, one `NET_DATA` chunk at a time. The
    // loop below is the engine's `absorb` + `on_end` flush, verbatim — chunks
    // with `end = false`, one flush with `end = true` whose error is swallowed,
    // because a decoder that has already produced everything answers an empty
    // `end = true` call with "incomplete body" and the engine must not turn
    // that into a failed fetch.
    let encoded = gzip(&payload);
    let mut decoder = StreamingDecoder::new("gzip", super::BODY_LIMIT).expect("decoder");
    let mut out = Vec::new();
    let mut scratch = [0u8; 97];
    let mut pos = 0;
    while pos < encoded.len() {
        let end = (pos + 13).min(encoded.len());
        let mut chunk = pos;
        loop {
            let step = decoder
                .process(&encoded[chunk..end], &mut scratch, false)
                .expect("step");
            chunk += step.consumed;
            out.extend_from_slice(&scratch[..step.written]);
            if step.finished || (step.consumed == 0 && step.written == 0) {
                break;
            }
        }
        pos = end;
    }
    loop {
        match decoder.process(&[], &mut scratch, true) {
            Ok(step) => {
                out.extend_from_slice(&scratch[..step.written]);
                if step.finished || step.written == 0 {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    assert_eq!(
        out, payload,
        "a 4 KiB body fed in 13-byte chunks must decode to the same bytes as \
         the whole-body call above — this is the assertion, not 'no error'"
    );

    // An encoding the crate does not implement is refused rather than
    // mis-decoded; the engine then leaves the body encoded, which is what the
    // reqwest path did with EVERY encoding.
    assert!(StreamingDecoder::new("snappy", super::BODY_LIMIT).is_err());
    assert!(StreamingDecoder::new("identity", super::BODY_LIMIT).is_ok());
}

fn gzip(input: &[u8]) -> Vec<u8> {
    use flate2::write::GzEncoder;
    use std::io::Write;
    let mut encoder = GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(input).expect("write");
    encoder.finish().expect("finish")
}

fn zlib_deflate(input: &[u8]) -> Vec<u8> {
    use flate2::write::ZlibEncoder;
    use std::io::Write;
    let mut encoder = ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(input).expect("write");
    encoder.finish().expect("finish")
}

/// The completion sink hands back borrowed `&str`s; the engine re-interns them
/// so a Node code can reach the runtime's diagnostics registry, which takes a
/// `&'static str`. A code that falls off the table must degrade to the generic
/// socket error rather than to something that looks like a real errno.
#[test]
fn borrowed_error_codes_are_re_interned_not_invented() {
    assert_eq!(
        exchange::intern_code_for_test(Some("ECONNREFUSED")),
        "ECONNREFUSED"
    );
    assert_eq!(
        exchange::intern_code_for_test(Some("ENOTFOUND")),
        "ENOTFOUND"
    );
    assert_eq!(exchange::intern_code_for_test(Some("EPIPE")), "EPIPE");
    assert_eq!(exchange::intern_code_for_test(None), "UND_ERR_SOCKET");
    assert_eq!(
        exchange::intern_code_for_test(Some("ENOSUCHCODE")),
        "UND_ERR_SOCKET"
    );
    assert_eq!(
        exchange::intern_syscall_for_test(Some("connect")),
        "connect"
    );
    assert_eq!(
        exchange::intern_syscall_for_test(Some("getaddrinfo")),
        "getaddrinfo"
    );
    assert_eq!(exchange::intern_syscall_for_test(Some("nope")), "");
    assert_eq!(exchange::intern_syscall_for_test(None), "");
}

/// The debug trace is a diagnostic, and its OFF state is the one every other
/// run takes (CLAUDE.md's GC-knob kill-policy, applied to a non-GC knob).
#[test]
fn debug_tracing_is_off_by_default() {
    assert!(
        std::env::var_os("PERRY_P6_DEBUG").is_some() || !exchange::tracing(),
        "PERRY_P6_DEBUG must default to off"
    );
}

/// A request the engine cannot serve is refused by the policy layer, which is
/// what makes `submit` return `Declined::Unsupported` — and the caller reject
/// with Node's error for it (`fetch::transport_error::Rejection`).
#[test]
fn an_unsupported_url_is_refused_by_the_policy_layer() {
    for url in [
        "ftp://example.test/x",
        "file:///etc/hosts",
        "http://user:pass@example.test/x",
        "not-a-url",
    ] {
        assert!(
            tlc::Request::new(url, "GET").is_err(),
            "{url} must be refused by the policy layer, which is what makes \
             `submit` decline it"
        );
    }
    // A CONNECT is forbidden for fetch and must not reach the transport.
    assert!(tlc::Request::new("http://example.test/x", "CONNECT").is_err());
}

/// The pool parks a request past `max_per_host`, and something has to admit it
/// again. This asserts the CONTRACT that made the admission bug possible: a
/// seat comes back when a connection is *closed*, not only when one is
/// released — so a host that admits waiters only on release is admitting them
/// on the rarer of the two events.
///
/// The end-to-end half of this is `test_gap_turnloop_fetch_pool_wait.ts`, which
/// asserts the process exits; this half pins the pool behaviour the engine
/// reasons from, so a `turnloop-http` change that altered it would fail here
/// rather than as a hang in a fixture.
#[test]
fn a_closed_connection_frees_a_seat_exactly_as_a_released_one_does() {
    let mut pool = tlc::Pool::new(2, Duration::from_secs(90));
    let key = PoolKey {
        origin: "http://a.test".into(),
        proxy: None,
    };
    let now = Instant::now();

    let Acquire::Connect(first) = pool.acquire(&key, now) else {
        panic!("first connects");
    };
    let Acquire::Connect(second) = pool.acquire(&key, now) else {
        panic!("second is within max_per_host=2");
    };
    assert!(
        matches!(pool.acquire(&key, now), Acquire::Wait),
        "the third must park — without this the rest of the test proves nothing"
    );

    // CLOSING one — the failure path, which never calls `release` — must free a
    // seat just as a release does.
    pool.closed(first).expect("close");
    assert!(
        matches!(pool.acquire(&key, now), Acquire::Connect(_)),
        "a closed connection frees the origin's seat; the engine must therefore \
         admit a waiter from its close path, not only from its release path"
    );

    // And the released one behaves the same way, which is the case the engine
    // already handled.
    pool.connected(second, tlc::Protocol::Http1, 1).expect("up");
    pool.release(second, true, now).expect("release");
    assert!(
        matches!(pool.acquire(&key, now), Acquire::Reuse(id) if id == second),
        "a released keep-alive connection is reused"
    );
}

// ── The proxy CONNECT tunnel ───────────────────────────────────────────────
//
// P6 declined a proxied fetch outright, and P8's inventory named the reason: "a
// CONNECT tunnel driven from a URL rather than from a prebuilt
// `reqwest::Client`". These assert the two halves of what replaced it — the
// routing decision, which is `turnloop_http::client::Route`'s, and the
// transport's reading of the proxy's answer, which is this module's.

/// An `https` target behind an `http` proxy must send `CONNECT host:443` first,
/// and the request that follows must still use origin-form — a proxy that has
/// tunnelled is transparent, so an absolute-form line would reach the ORIGIN
/// and be a protocol error rather than a routing one.
#[test]
fn an_https_target_behind_a_proxy_tunnels_and_keeps_origin_form() {
    let request = tlc::Request::new("https://origin.test/a/b?c=1", "GET").expect("url");
    let proxy = url::Url::parse("http://proxy.test:8080").expect("proxy url");
    let route = tlc::Route::new(request.url.clone(), Some(proxy));

    let connect = route
        .connect_head(None)
        .expect("an https target must tunnel");
    assert_eq!(connect.method, "CONNECT");
    assert_eq!(
        connect.target, "origin.test:443",
        "CONNECT names the ORIGIN authority, not the proxy and not a path"
    );
    assert!(
        connect
            .headers
            .iter()
            .any(|h| h.name.eq_ignore_ascii_case("host") && h.value == b"origin.test:443"),
        "the CONNECT head carries the tunnel target as `host`"
    );

    let head = route.request_head(&request, None);
    assert_eq!(
        head.target, "/a/b?c=1",
        "inside a tunnel the request line is origin-form"
    );
    assert!(
        !head
            .headers
            .iter()
            .any(|h| h.name.eq_ignore_ascii_case("proxy-authorization")),
        "proxy credentials must never be forwarded inside the tunnel"
    );
}

/// An `http` target behind the same proxy must NOT tunnel: it goes to the proxy
/// as an ordinary request with an absolute-form line. Getting this wrong is
/// invisible in a green build — the request still reaches somewhere.
#[test]
fn an_http_target_behind_a_proxy_uses_absolute_form_and_no_tunnel() {
    let request = tlc::Request::new("http://origin.test/a/b", "GET").expect("url");
    let proxy = url::Url::parse("http://user:pw@proxy.test:8080").expect("proxy url");
    let route = tlc::Route::new(request.url.clone(), Some(proxy));

    assert!(
        route.connect_head(None).is_none(),
        "a plaintext target through a proxy needs no tunnel"
    );
    let head = route.request_head(&request, None);
    assert_eq!(
        head.target, "http://origin.test/a/b",
        "a proxied plaintext request line is absolute-form"
    );
    assert!(
        head.headers
            .iter()
            .any(|h| h.name.eq_ignore_ascii_case("proxy-authorization")),
        "the proxy URL's userinfo becomes Proxy-Authorization on the plaintext path"
    );
}

/// A proxy that refuses the tunnel must FAIL the request. The failure mode this
/// guards is the dangerous one: treating a non-2xx as "carry on" would run the
/// TLS handshake against the proxy's error page.
#[test]
fn a_refused_connect_is_an_error_not_a_direct_connection() {
    let request = tlc::Request::new("https://origin.test/", "GET").expect("url");
    let proxy = url::Url::parse("http://proxy.test:8080").expect("proxy url");
    for status in [403u16, 407, 502, 100, 300] {
        let mut route = tlc::Route::new(request.url.clone(), Some(proxy.clone()));
        assert!(
            route.tunnel_response(status).is_err(),
            "status {status} must not establish a tunnel"
        );
    }
    let mut route = tlc::Route::new(request.url.clone(), Some(proxy));
    assert!(
        route.tunnel_response(200).is_ok(),
        "a 2xx is what establishes the tunnel"
    );
}

/// The CONNECT reader stops at the HEAD and reports what is left.
///
/// Two hazards in one test. A CONNECT response has NO body — the socket becomes
/// the tunnel — so a reader that waits for `Event::End` hangs against every
/// correct proxy. And a proxy is allowed to coalesce its `200` with the first
/// bytes the origin sends back, so a reader that discards its buffer loses the
/// first TLS record and the handshake stalls with no error.
#[test]
fn the_connect_reader_stops_at_the_head_and_hands_back_the_tail() {
    let tail: &[u8] = &[0x16, 0x03, 0x03, 0x00, 0x2a];
    let mut wire = b"HTTP/1.1 200 Connection established\r\nproxy-agent: t\r\n\r\n".to_vec();
    wire.extend_from_slice(tail);

    let mut http = tlc::Http1Connection::new(http1::Limits::default());
    http.start(
        &http1::Head {
            method: "CONNECT".into(),
            target: "origin.test:443".into(),
            status: 0,
            version: 1,
            headers: vec![http1::Header::new("host", "origin.test:443")],
            keep_alive: true,
        },
        http1::BodyLength::Empty,
        None,
        None,
    )
    .expect("start CONNECT");
    http.finish_body(&[]).expect("finish CONNECT");

    let (consumed, status) =
        exchange::decode_connect_status_for_test(&mut http, &wire).expect("decode");
    assert_eq!(status, Some(200));
    assert_eq!(
        &wire[consumed..],
        tail,
        "the bytes after the head are the origin's and must survive"
    );
}

/// The proxy's answer arriving one byte at a time must still be read. This is
/// the split-read shape that made the direct path fail against a real origin
/// whose head spanned two TLS records, applied to the tunnel.
#[test]
fn a_connect_answer_split_across_reads_is_reassembled() {
    let wire = b"HTTP/1.1 200 Connection established\r\n\r\n";
    let mut http = tlc::Http1Connection::new(http1::Limits::default());
    http.start(
        &http1::Head {
            method: "CONNECT".into(),
            target: "origin.test:443".into(),
            status: 0,
            version: 1,
            headers: vec![http1::Header::new("host", "origin.test:443")],
            keep_alive: true,
        },
        http1::BodyLength::Empty,
        None,
        None,
    )
    .expect("start CONNECT");
    http.finish_body(&[]).expect("finish CONNECT");

    // Retain unconsumed input the way `Tunnel::input` does, and feed one more
    // byte each turn.
    let mut pending: Vec<u8> = Vec::new();
    let mut status = None;
    for byte in wire.iter() {
        pending.push(*byte);
        let (consumed, got) =
            exchange::decode_connect_status_for_test(&mut http, &pending).expect("decode");
        pending.drain(..consumed.min(pending.len()));
        if let Some(got) = got {
            status = Some(got);
            break;
        }
    }
    assert_eq!(
        status,
        Some(200),
        "a head delivered one byte at a time must still produce its status"
    );
    assert!(
        pending.is_empty(),
        "nothing follows the head in this fixture"
    );
}

// ── turnloop P10: the thread that has no loop of its own ───────────────────
//
// P8's inventory named "per-agent loops for the decline" as the first of group
// G's three blockers. P9 landed those loops, so the thread that still cannot
// submit is a SECOND thread acting for an agent another thread already owns —
// and `perry_ffi::agent_post` lets it hand the whole submission to that owner
// instead of keeping a reqwest future alive for itself.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering as AtomicOrdering};

/// How many times the P10 test's sink has been called, and on which thread.
///
/// A `Sink` is a plain `fn`, so there is nowhere to hang a closure's captured
/// state — which is the same reason the engine holds function pointers in the
/// first place.
static P10_SINK_CALLS: AtomicUsize = AtomicUsize::new(0);
static P10_SINK_THREAD: AtomicU64 = AtomicU64::new(0);

fn thread_fingerprint() -> u64 {
    // `ThreadId` has no stable numeric form on stable Rust, and the test only
    // needs "the same thread or not". The address of a thread-local is exactly
    // that, and is cheap.
    thread_local! {
        static ANCHOR: u8 = const { 0 };
    }
    ANCHOR.with(|a| a as *const u8 as u64)
}

fn p10_sink_done(_ctx: usize, _outcome: super::Outcome) {
    P10_SINK_THREAD.store(thread_fingerprint(), AtomicOrdering::SeqCst);
    P10_SINK_CALLS.fetch_add(1, AtomicOrdering::SeqCst);
}

/// turnloop P10, end to end through `submit`: a thread that cannot get a loop
/// of its own hands the whole request to the thread that owns one, instead of
/// being refused.
///
/// Three discriminating facts, not one "nothing threw":
///
/// * the poster's own `agent_post::dispatched()` stays at zero — if it moved,
///   the work never crossed;
/// * `submitted_total()` does not move while the poster is running and DOES
///   move once the owner has run the job — the counter is bumped by
///   `start_here`, so it is the proof the owner actually entered the request
///   into its engine rather than merely receiving a box;
/// * the sink runs on the OWNER's thread, which is what makes the promise get
///   settled where that agent's JS values live (#1824).
///
/// The request is aimed at a closed loopback port, so it fails fast. The
/// subject here is the crossing, not the response.
#[test]
fn a_thread_with_no_loop_posts_its_fetch_to_the_thread_that_owns_one() {
    let _lease = super::become_the_owner_for_test();
    let owner = thread_fingerprint();
    let before_dispatched = perry_ffi::agent_post::dispatched();
    let before_submitted = super::submitted_total();
    let before_calls = P10_SINK_CALLS.load(AtomicOrdering::SeqCst);

    let poster_ran_its_own = std::thread::spawn(move || {
        // A second thread acting FOR the same agent: it has no agent of its
        // own, so `current_agent()` resolves to the primary agent — the one
        // whose loop the thread above owns.
        assert!(
            !super::tl::available(),
            "this thread must NOT own the loop, or the post under test never \
             happens and the assertions below are vacuous"
        );
        assert!(
            perry_ffi::agent_post::available(),
            "the agent HAS a loop; a second thread of it must be able to post"
        );
        assert_eq!(
            perry_ffi::agent_post::dispatched(),
            0,
            "this thread has run no posted job"
        );

        // Only a decline that belongs to the THREAD may be posted.
        // `Unsupported`, `Proxy` and `NoTls` are properties of the request and
        // of process-wide configuration, so the agent's owner would refuse them
        // for exactly the same reason — and by then the caller has been told
        // the engine took the request and can no longer reject it itself.
        // `submit` therefore prepares the request BEFORE it chooses a
        // transport; this is the assertion that pins that order, and it is made
        // here because this is the thread that would otherwise post. Were the
        // order reversed, the post would be accepted and this would be `Ok`.
        assert_eq!(
            super::submit(
                super::RequestSpec {
                    url: "ftp://example.test/x".to_string(),
                    method: "GET".to_string(),
                    headers: Vec::new(),
                    body: None,
                    redirect: RedirectMode::Follow,
                    abort_key: None,
                },
                super::Sink {
                    ctx: 0,
                    on_head: None,
                    on_chunk: None,
                    on_done: p10_sink_done,
                },
            ),
            Err(super::Declined::Unsupported(turnloop_http::Error::new(
                "ERR_INVALID_URL",
                "unsupported URL scheme"
            ))),
            "an unsupported URL must be refused to the caller, not be \
             handed to a thread that would refuse it identically"
        );
        let spec = super::RequestSpec {
            url: "http://127.0.0.1:1/p10".to_string(),
            method: "GET".to_string(),
            headers: Vec::new(),
            body: None,
            redirect: RedirectMode::Follow,
            abort_key: None,
        };
        let sink = super::Sink {
            ctx: 0,
            on_head: None,
            on_chunk: None,
            on_done: p10_sink_done,
        };
        super::submit(spec, sink).expect(
            "a thread whose agent has a loop must not decline — that decline is \
             exactly what P10 deletes",
        );
        assert_eq!(
            super::submitted_total(),
            before_submitted,
            "the poster must not have entered the request into an engine of its \
             own; it has no loop to drive one"
        );
        perry_ffi::agent_post::dispatched()
    })
    .join()
    .expect("posting thread");

    assert_eq!(
        poster_ran_its_own, 0,
        "the poster must NOT have run the job itself — if it did, the work never \
         crossed and this surface is still doing its own I/O"
    );

    let limit = Instant::now() + Duration::from_secs(10);
    while perry_ffi::agent_post::dispatched() == before_dispatched {
        assert!(
            Instant::now() < limit,
            "the posted request never reached the owner"
        );
        perry_runtime::event_pump::js_loop_turn_bounded(0);
    }
    assert_eq!(
        perry_ffi::agent_post::dispatched(),
        before_dispatched + 1,
        "the owner ran it exactly once — a post is delivered, not retried"
    );
    assert!(
        super::submitted_total() > before_submitted,
        "the owner entered the request into ITS engine; without this the job \
         crossed and did nothing"
    );

    let limit = Instant::now() + Duration::from_secs(10);
    while P10_SINK_CALLS.load(AtomicOrdering::SeqCst) == before_calls {
        assert!(
            Instant::now() < limit,
            "the posted request never settled on the owner"
        );
        perry_runtime::event_pump::js_loop_turn_bounded(0);
    }
    assert_eq!(
        P10_SINK_THREAD.load(AtomicOrdering::SeqCst),
        owner,
        "the sink must run on the agent's OWNER, which is the thread this \
         agent's JS values live on"
    );
}
