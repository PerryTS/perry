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
    // is the path a real response takes, one `NET_DATA` chunk at a time, and it
    // is the path the engine's `absorb` drives.
    let encoded = gzip(&payload);
    let mut decoder = StreamingDecoder::new("gzip", super::BODY_LIMIT).expect("decoder");
    let mut out = Vec::new();
    let mut scratch = [0u8; 97];
    let mut pos = 0;
    let mut finished = false;
    while pos < encoded.len() {
        let end = (pos + 13).min(encoded.len());
        let last = end == encoded.len();
        let mut chunk = pos;
        loop {
            let step = decoder
                .process(&encoded[chunk..end], &mut scratch, last)
                .expect("step");
            chunk += step.consumed;
            out.extend_from_slice(&scratch[..step.written]);
            if step.finished {
                finished = true;
                break;
            }
            if step.consumed == 0 && step.written == 0 {
                break;
            }
        }
        pos = end;
    }
    assert_eq!(out, payload, "chunked gzip decode");
    assert!(
        finished,
        "a complete gzip member fed in 13-byte chunks must report finished — \
         without this the length check above would pass on a truncated decode"
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

/// A request the engine cannot serve must DECLINE rather than fail: the caller
/// still has a working reqwest transport, and turning a decline into an error
/// would delete a working configuration.
#[test]
fn an_unsupported_url_declines_rather_than_failing() {
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
