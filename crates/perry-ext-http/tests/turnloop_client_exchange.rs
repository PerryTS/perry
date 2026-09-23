//! The `node:http` client lane carries a real exchange over turnloop
//! (`client_turnloop`, lane 1).
//!
//! # Why this is an integration binary with exactly ONE `#[test]`
//!
//! An agent's turnloop route is claimed **once per thread, by the first thread
//! to ask**, and every other thread acting for that agent is declined for the
//! rest of its life (`event_pump::agent_loop::claim_route`). A multi-threaded
//! `cargo test` therefore hands the route to whichever test thread asks first,
//! so a *unit* test that asserts `client_turnloop::available()` is a lottery.
//! CI pins `RUST_TEST_THREADS=1` for `perry-runtime` only — this crate's
//! `cargo-test` leg runs the default pool — so the lottery is real here.
//! One `#[test]` in its own binary is one thread in its own process, so it is
//! the first asker by construction. This is the same reasoning, and the same
//! shape, as `turnloop_reuse_port.rs` next to it.
//!
//! # What makes this non-vacuous
//!
//! A decline is *invisible*: `try_dispatch` returning `false` for every
//! request would leave the JS surface behaving exactly as it does on reqwest,
//! and a test that only checked "the response arrived" would stay green having
//! never touched turnloop — CLAUDE.md's fourth way a gate cannot fail. So this
//! test asserts the subject was live three times over, and there is no
//! "skip if no loop" arm anywhere:
//!
//! 1. `try_dispatch` returned `true` — the lane ACCEPTED rather than declined.
//! 2. `completed_total()` moved — a response was decoded through to
//!    `Event::End`, not merely attempted.
//! 3. The server, a plain `std::net::TcpListener` that knows nothing about
//!    turnloop, received a well-formed request head. That is the proof the
//!    bytes actually reached a socket.
//!
//! The response is a **307** on purpose. Node's `http.request` must never
//! follow a redirect, and this lane gets that by construction (it runs no
//! redirect policy at all) — so a 307 that arrives as a 307, with the server
//! hit exactly once, is the regression `test_gap_http_client_no_redirect_follow.ts`
//! pins, asserted here at the transport instead of through the JS surface.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use perry_ext_http::client_turnloop;

/// The `perry_ffi_*` async-bridge externs this crate references are normally
/// provided by the host stdlib archive, which a test binary does not link. The
/// lib test binary solves that with `#[cfg(test)] mod test_async_shims`; an
/// integration binary is a separate crate, so it includes the same file rather
/// than carrying a second copy that could drift from it.
#[path = "../src/test_async_shims.rs"]
mod test_async_shims;

/// The Bun-server bridge lives in `perry-stdlib`, which a `perry-ext-http`
/// test binary deliberately does not link (that would be a dependency edge the
/// crate does not have). `server/bun_server.rs` stubs these under
/// `#[cfg(test)]` for the lib test binary; an integration binary links the
/// non-test lib, so it supplies the same stubs here. Nothing in this test
/// reaches the Bun server.
#[no_mangle]
unsafe extern "C" fn js_bun_http_request_from_json(
    _snapshot: *const perry_ffi::StringHeader,
) -> f64 {
    // TAG_UNDEFINED; see the NaN-boxing table in CLAUDE.md.
    f64::from_bits(0x7FFC_0000_0000_0001)
}

#[no_mangle]
unsafe extern "C" fn js_bun_http_response_snapshot_json(
    _response_handle: f64,
) -> *mut perry_ffi::StringHeader {
    std::ptr::null_mut()
}

/// A `ClientRequestHandle` id that is deliberately not in the handle registry.
/// Nothing in this lane dereferences it — it is the address events are queued
/// against — and `js_ext_http_client_inflight` treats an unknown handle as
/// having no socket facade, which is the counted case.
const REQUEST_HANDLE: i64 = 0x5eed_c11e;

#[test]
fn a_cleartext_get_is_carried_end_to_end_and_a_307_is_not_followed() {
    // ── A server that knows nothing about turnloop ───────────────────────
    let listener = TcpListener::bind("127.0.0.1:0").expect("an ephemeral port");
    let port = listener.local_addr().expect("a bound address").port();
    let (heads, received) = mpsc::channel::<String>();
    // Lets the test tell the server when to let go. The server must NOT close
    // first: a lane that only finished on EOF would then pass too, and the
    // point of the Content-Length framing is that `Event::End` arrives while
    // the connection is still open.
    let (release, released) = mpsc::channel::<()>();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("exactly one connection");
        let mut head = Vec::new();
        let mut buf = [0u8; 1024];
        while !head.windows(4).any(|w| w == b"\r\n\r\n") {
            match stream.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => head.extend_from_slice(&buf[..n]),
            }
        }
        // Content-Length delimited, so `Event::End` arrives from the body
        // bytes rather than from a close — which also means a lane that never
        // made the zero-byte `receive` call would hang here instead of
        // finishing, and the deadline below would catch it.
        let _ = stream.write_all(
            b"HTTP/1.1 307 Temporary Redirect\r\n\
              location: /target\r\n\
              content-length: 8\r\n\
              connection: keep-alive\r\n\
              \r\n\
              redirect",
        );
        let _ = stream.flush();
        let _ = heads.send(String::from_utf8_lossy(&head).into_owned());
        // Hold the connection open until the test has seen the exchange
        // finish. Blocking on a read instead would deadlock: the client's
        // `tl::close` completes on a later turn of the loop, and by then the
        // test has stopped turning it.
        let _ = released.recv_timeout(Duration::from_secs(30));
    });

    // ── Become this agent's loop owner ───────────────────────────────────
    // No skip arm: turning the loop is what publishes the route, and if it
    // never becomes available every assertion below would be vacuous.
    let deadline = Instant::now() + Duration::from_secs(20);
    while !client_turnloop::available() {
        perry_runtime::event_pump::js_loop_turn_bounded(1);
        assert!(
            Instant::now() < deadline,
            "this thread never became the agent's turnloop loop owner; the lane \
             cannot be exercised and every assertion below would be vacuous"
        );
    }

    let accepted_before = client_turnloop::accepted_total();
    let completed_before = client_turnloop::completed_total();

    // ── The subject ──────────────────────────────────────────────────────
    let url = format!("http://127.0.0.1:{port}/start");
    let accepted =
        client_turnloop::try_dispatch(REQUEST_HANDLE, "GET", &url, &HashMap::new(), &[], None, 0);
    assert!(
        accepted,
        "lane 1 must ACCEPT a cleartext bodyless GET on the default agent. A \
         decline here is silent — reqwest would serve the request and this test \
         would prove nothing about turnloop"
    );
    assert_eq!(
        client_turnloop::accepted_total(),
        accepted_before + 1,
        "an accepted exchange must be counted"
    );

    // ── Drive it to completion ───────────────────────────────────────────
    let deadline = Instant::now() + Duration::from_secs(30);
    while perry_ext_http::js_ext_http_client_inflight() != 0 {
        perry_runtime::event_pump::js_loop_turn_bounded(5);
        assert!(
            Instant::now() < deadline,
            "the exchange never reached a terminal event: the inflight guard is \
             still held {}s after submission",
            30
        );
    }

    assert_eq!(
        client_turnloop::completed_total(),
        completed_before + 1,
        "the response must have decoded through to Event::End. An exchange that \
         errored also drops the inflight guard, so the loop above alone does not \
         distinguish success from failure"
    );

    // ── The server's view: the bytes really went out ─────────────────────
    let head = received
        .recv_timeout(Duration::from_secs(10))
        .expect("the server must have received a request head");
    let _ = release.send(());
    server.join().expect("the server thread must not panic");
    // Let the socket close this lane submitted at `Event::End` complete, so
    // the binary does not exit with a handle still open on the loop.
    for _ in 0..50 {
        perry_runtime::event_pump::js_loop_turn_bounded(1);
    }

    assert!(
        head.starts_with("GET /start HTTP/1.1\r\n"),
        "origin-form request line expected, got:\n{head}"
    );
    let lower = head.to_ascii_lowercase();
    assert!(
        lower.contains(&format!("host: 127.0.0.1:{port}\r\n")),
        "the authority must carry the port, got:\n{head}"
    );
    // Mirrors what the reqwest path put on the wire, so servers reading
    // `req.headers.connection` see no change from this migration.
    assert!(
        lower.contains("connection: keep-alive\r\n"),
        "Node's default agent advertises keep-alive, got:\n{head}"
    );
}
