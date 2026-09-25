//! The `node:http` / `node:https` client carries every request shape over
//! turnloop (`client_turnloop`) — there is no other transport to fall back to.
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
//! shape, as `turnloop_reuse_port.rs` next to it. The shapes run in sequence
//! inside that one test.
//!
//! # What makes this non-vacuous
//!
//! Every shape is checked from both ends, and there is no "skip if no loop"
//! arm anywhere:
//!
//! * **the transport's own counters** moved — `completed_total()` for a
//!   response decoded to its end, `reused_total()` for a pooled connection,
//!   `tls_handshakes_total()` for a handshake, `timed_out_total()` for a
//!   deadline. An exchange that errored also drops the in-flight guard, so
//!   "the loop went quiet" alone would not distinguish success from failure;
//! * **the server's view** — a plain `std::net::TcpListener` (or a rustls
//!   server over one) that knows nothing about turnloop received the exact
//!   bytes, and, for keep-alive, received both requests on ONE connection.
//!
//! Sabotage-checked while writing this: making `park` close instead of pool
//! fails the keep-alive shape on `reused_total()` and on the server seeing a
//! second connection; handing the session a config without the fixture CA
//! fails the https shape on `tls_handshakes_total()`.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::sync::Arc;
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

/// `ClientRequestHandle` ids that are deliberately not in the handle registry.
/// Nothing in the transport dereferences them — they are the addresses events
/// are queued against — and `js_ext_http_client_inflight` treats an unknown
/// handle as having no socket facade, which is the counted case.
const GET_307: i64 = 0x5eed_c11e;
const POST_BODY: i64 = GET_307 + 1;
const POOLED_A: i64 = GET_307 + 2;
const POOLED_B: i64 = GET_307 + 3;
const DEADLINE: i64 = GET_307 + 4;
const TRAILERS: i64 = GET_307 + 5;
const SECURE: i64 = GET_307 + 6;
const OVER_SOCKET: i64 = GET_307 + 7;
const DEFERRED: i64 = GET_307 + 8;

const CERT_PEM: &[u8] =
    include_bytes!("../../../test-parity/node-suite/tls/fixtures/localhost-cert.pem");
const KEY_PEM: &[u8] =
    include_bytes!("../../../test-parity/node-suite/tls/fixtures/localhost-key.pem");

/// One nonblocking turn plus its dispatch.
///
/// Budget 0 on purpose. A positive budget parks, and a park returns at once
/// without turning while the process-wide `NOTIFIED` flag is set — which every
/// queued `PendingHttpEvent` sets, and which only the JS event loop's
/// `js_wait_for_event` consumes. This binary has no JS event loop, so after the
/// first delivered event a parking turn would never collect another
/// completion. (The single-exchange version of this test never noticed: its
/// only events were pushed after its last turn.)
fn turn() {
    perry_runtime::event_pump::js_loop_turn_bounded(0);
    std::thread::sleep(Duration::from_millis(1));
}

/// Turn the loop until every accepted exchange has settled.
fn drive(what: &str) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while perry_ext_http::js_ext_http_client_inflight() != 0 {
        turn();
        assert!(
            Instant::now() < deadline,
            "{what}: the exchange never reached a terminal event — the in-flight \
             guard is still held 30s after submission"
        );
    }
}

/// Join a server thread while still turning the loop. A server that waits
/// for the client's close would otherwise never see it: `tl::close` is only a
/// submission, and the loop performs it on its next turn.
fn join(server: std::thread::JoinHandle<()>, what: &str) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !server.is_finished() {
        turn();
        assert!(
            Instant::now() < deadline,
            "{what}: the server thread did not finish — the client never closed"
        );
    }
    server.join().expect("the server thread must not panic");
}

/// Let submitted closes complete so no handle is left open on the loop.
fn settle() {
    for _ in 0..50 {
        turn();
    }
}

/// Read one request (head plus a `Content-Length` body) off `stream`.
fn read_request(stream: &mut impl Read) -> Option<(String, Vec<u8>)> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos + 4;
        }
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return None,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let length = head
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    let mut body = buf[head_end..].to_vec();
    while body.len() < length {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => body.extend_from_slice(&chunk[..n]),
        }
    }
    Some((head, body))
}

fn no_headers() -> HashMap<String, String> {
    HashMap::new()
}

#[test]
fn every_client_shape_is_carried_end_to_end_on_turnloop() {
    // ── Become this agent's loop owner ───────────────────────────────────
    // No skip arm: turning the loop is what publishes the route, and if it
    // never becomes available every assertion below would be vacuous.
    let deadline = Instant::now() + Duration::from_secs(20);
    while !client_turnloop::available() {
        turn();
        assert!(
            Instant::now() < deadline,
            "this thread never became the agent's turnloop loop owner; the \
             transport cannot be exercised and every assertion below would be vacuous"
        );
    }

    a_cleartext_get_is_carried_and_a_307_is_not_followed();
    a_post_body_and_an_explicit_host_reach_the_wire();
    an_agent_with_keep_alive_reuses_one_connection();
    a_deadline_tears_down_an_exchange_the_server_never_answers();
    a_te_trailers_response_is_decoded_to_its_end();
    an_https_request_handshakes_with_the_callers_ca();
    a_create_connection_socket_is_read_when_net_says_it_is_ready();
    a_deferred_event_fires_from_a_loop_deadline();
}

fn a_cleartext_get_is_carried_and_a_307_is_not_followed() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("an ephemeral port");
    let port = listener.local_addr().expect("a bound address").port();
    let (heads, received) = mpsc::channel::<String>();
    // The server must NOT close first: a transport that only finished on EOF
    // would then pass too, and the point of the Content-Length framing is that
    // the end arrives while the connection is still open.
    let (release, released) = mpsc::channel::<()>();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("exactly one connection");
        let (head, _) = read_request(&mut stream).expect("a request head");
        let _ = stream.write_all(
            b"HTTP/1.1 307 Temporary Redirect\r\n\
              location: /target\r\n\
              content-length: 8\r\n\
              connection: keep-alive\r\n\
              \r\n\
              redirect",
        );
        let _ = stream.flush();
        let _ = heads.send(head);
        let _ = released.recv_timeout(Duration::from_secs(30));
    });

    let completed_before = client_turnloop::completed_total();
    let url = format!("http://127.0.0.1:{port}/start");
    assert!(
        client_turnloop::try_dispatch(GET_307, "GET", &url, &no_headers(), &[], None, 0),
        "a cleartext GET must be carried"
    );
    drive("GET 307");
    assert_eq!(
        client_turnloop::completed_total(),
        completed_before + 1,
        "the 307 must have decoded through to its end"
    );

    let head = received
        .recv_timeout(Duration::from_secs(10))
        .expect("the server must have received a request head");
    let _ = release.send(());
    join(server, "server");
    settle();

    assert!(
        head.starts_with("GET /start HTTP/1.1\r\n"),
        "origin-form request line expected, got:\n{head}"
    );
    assert!(
        head.contains(&format!("Host: 127.0.0.1:{port}\r\n")),
        "the authority must carry the port, got:\n{head}"
    );
    assert!(
        head.contains("Connection: keep-alive\r\n"),
        "Node's default agent advertises keep-alive, got:\n{head}"
    );
}

fn a_post_body_and_an_explicit_host_reach_the_wire() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("an ephemeral port");
    let port = listener.local_addr().expect("a bound address").port();
    let (seen, received) = mpsc::channel::<(String, Vec<u8>)>();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("exactly one connection");
        let request = read_request(&mut stream).expect("a request");
        let _ = stream.write_all(b"HTTP/1.1 201 Made It\r\ncontent-length: 2\r\n\r\nok");
        let _ = seen.send(request);
        // Hold the socket until the client closes it.
        let mut rest = Vec::new();
        let _ = stream.read_to_end(&mut rest);
    });

    let completed_before = client_turnloop::completed_total();
    let headers: HashMap<String, String> = [
        ("Host".to_string(), "vhost.invalid".to_string()),
        ("Content-Type".to_string(), "text/plain".to_string()),
    ]
    .into();
    let url = format!("http://127.0.0.1:{port}/upload");
    assert!(client_turnloop::try_dispatch(
        POST_BODY, "POST", &url, &headers, b"hello", None, 0
    ));
    drive("POST body");
    assert_eq!(client_turnloop::completed_total(), completed_before + 1);

    let (head, body) = received
        .recv_timeout(Duration::from_secs(10))
        .expect("the server must have received the request");
    join(server, "server");
    settle();
    assert!(head.starts_with("POST /upload HTTP/1.1\r\n"), "{head}");
    assert!(
        head.contains("Host: vhost.invalid\r\n"),
        "a caller's Host must reach the wire verbatim, got:\n{head}"
    );
    assert!(head.contains("Content-Type: text/plain\r\n"), "{head}");
    assert!(head.contains("Content-Length: 5\r\n"), "{head}");
    assert_eq!(body, b"hello");
}

fn an_agent_with_keep_alive_reuses_one_connection() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("an ephemeral port");
    let port = listener.local_addr().expect("a bound address").port();
    let (served, received) = mpsc::channel::<Vec<String>>();
    let (release, released) = mpsc::channel::<()>();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("the first connection");
        let mut heads = Vec::new();
        for index in 0..2 {
            let Some((head, _)) = read_request(&mut stream) else {
                break;
            };
            heads.push(head);
            let body = format!("r{index}");
            let _ = stream.write_all(
                format!(
                    "HTTP/1.1 200 OK\r\ncontent-length: {}\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            );
        }
        // A second connection would mean the pool did not reuse.
        listener
            .set_nonblocking(true)
            .expect("a nonblocking listener");
        if listener.accept().is_ok() {
            heads.push("SECOND CONNECTION".to_string());
        }
        let _ = served.send(heads);
        let _ = released.recv_timeout(Duration::from_secs(30));
    });

    let reused_before = client_turnloop::reused_total();
    let completed_before = client_turnloop::completed_total();
    let url = format!("http://127.0.0.1:{port}/pooled");
    // Agent key 0x7a is not a registered AgentHandle; the pool keys by it all
    // the same, which is all this needs.
    assert!(client_turnloop::try_dispatch_pooled(
        POOLED_A,
        "GET",
        &url,
        &no_headers(),
        &[],
        0x7a,
        4,
        5_000
    ));
    drive("pooled A");
    assert!(client_turnloop::try_dispatch_pooled(
        POOLED_B,
        "GET",
        &url,
        &no_headers(),
        &[],
        0x7a,
        4,
        5_000
    ));
    drive("pooled B");
    assert_eq!(client_turnloop::completed_total(), completed_before + 2);
    assert_eq!(
        client_turnloop::reused_total(),
        reused_before + 1,
        "the second request must run on the connection the first one parked"
    );

    let heads = received
        .recv_timeout(Duration::from_secs(10))
        .expect("the server must report what it served");
    let _ = release.send(());
    join(server, "server");
    assert_eq!(
        heads.len(),
        2,
        "both requests on ONE connection, and no second connection: {heads:?}"
    );
    assert!(heads
        .iter()
        .all(|h| h.starts_with("GET /pooled HTTP/1.1\r\n")));
    // Close the parked connection so the binary exits with no open handle.
    client_turnloop::purge_agent(0x7a);
    settle();
}

fn a_deadline_tears_down_an_exchange_the_server_never_answers() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("an ephemeral port");
    let port = listener.local_addr().expect("a bound address").port();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("one connection");
        let _ = read_request(&mut stream);
        // Never answer; return once the client gives up and closes.
        let mut rest = Vec::new();
        let _ = stream.read_to_end(&mut rest);
    });

    let timed_out_before = client_turnloop::timed_out_total();
    let completed_before = client_turnloop::completed_total();
    let url = format!("http://127.0.0.1:{port}/silent");
    let started = Instant::now();
    assert!(client_turnloop::try_dispatch(
        DEADLINE,
        "GET",
        &url,
        &no_headers(),
        &[],
        Some(150),
        0
    ));
    drive("deadline");
    assert_eq!(
        client_turnloop::timed_out_total(),
        timed_out_before + 1,
        "the deadline, not anything else, must have ended the exchange"
    );
    assert_eq!(client_turnloop::completed_total(), completed_before);
    assert!(started.elapsed() >= Duration::from_millis(150));
    join(
        server,
        "the server sees the connection close after the deadline",
    );
    settle();
}

fn a_te_trailers_response_is_decoded_to_its_end() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("an ephemeral port");
    let port = listener.local_addr().expect("a bound address").port();
    let (heads, received) = mpsc::channel::<String>();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("one connection");
        let (head, _) = read_request(&mut stream).expect("a request head");
        let _ = stream.write_all(
            b"HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\ntrailer: x-checksum\r\n\r\n\
              2\r\nok\r\n0\r\nx-checksum: abc\r\n\r\n",
        );
        let _ = heads.send(head);
        let mut rest = Vec::new();
        let _ = stream.read_to_end(&mut rest);
    });
    let completed_before = client_turnloop::completed_total();
    let headers: HashMap<String, String> = [("TE".to_string(), "trailers".to_string())].into();
    let url = format!("http://127.0.0.1:{port}/trailers");
    assert!(client_turnloop::try_dispatch(
        TRAILERS,
        "GET",
        &url,
        &headers,
        &[],
        None,
        0
    ));
    drive("trailers");
    assert_eq!(client_turnloop::completed_total(), completed_before + 1);
    let head = received
        .recv_timeout(Duration::from_secs(10))
        .expect("a request head");
    join(server, "server");
    settle();
    assert!(head.contains("TE: trailers\r\n"), "{head}");
    assert!(
        head.ends_with("Connection: close\r\n\r\n"),
        "the trailers shape keeps its bypass's close framing, got:\n{head}"
    );
}

fn an_https_request_handshakes_with_the_callers_ca() {
    let certs: Vec<rustls::pki_types::CertificateDer<'static>> =
        rustls_pemfile::certs(&mut std::io::Cursor::new(CERT_PEM))
            .collect::<Result<_, _>>()
            .expect("the fixture certificate parses");
    let key = rustls_pemfile::private_key(&mut std::io::Cursor::new(KEY_PEM))
        .expect("the fixture key parses")
        .expect("the fixture has a key");
    let config = Arc::new(
        rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .expect("protocol versions")
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .expect("a server config"),
    );

    let listener = TcpListener::bind("127.0.0.1:0").expect("an ephemeral port");
    let port = listener.local_addr().expect("a bound address").port();
    let (heads, received) = mpsc::channel::<(String, Option<Vec<u8>>)>();
    let server = std::thread::spawn(move || {
        let (stream, _): (TcpStream, _) = listener.accept().expect("one connection");
        let connection = rustls::ServerConnection::new(config).expect("a server session");
        let mut tls = rustls::StreamOwned::new(connection, stream);
        let Some((head, _)) = read_request(&mut tls) else {
            let _ = heads.send((String::new(), None));
            return;
        };
        let alpn = tls.conn.alpn_protocol().map(<[u8]>::to_vec);
        let _ = tls.write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 6\r\n\r\nsecure");
        let _ = tls.flush();
        let _ = heads.send((head, alpn));
        let mut rest = Vec::new();
        let _ = tls.read_to_end(&mut rest);
    });

    let handshakes_before = client_turnloop::tls_handshakes_total();
    let completed_before = client_turnloop::completed_total();
    let url = format!("https://127.0.0.1:{port}/secure");
    assert!(client_turnloop::try_dispatch_tls(
        SECURE,
        "GET",
        &url,
        &no_headers(),
        &[],
        None,
        0,
        vec![CERT_PEM.to_vec()],
    ));
    drive("https");
    assert_eq!(
        client_turnloop::tls_handshakes_total(),
        handshakes_before + 1,
        "the handshake must have completed on the transport"
    );
    assert_eq!(
        client_turnloop::completed_total(),
        completed_before + 1,
        "the response must have been decrypted and decoded to its end"
    );
    let (head, alpn) = received
        .recv_timeout(Duration::from_secs(10))
        .expect("the TLS server must have received a request");
    join(server, "server");
    settle();
    assert!(head.starts_with("GET /secure HTTP/1.1\r\n"), "{head}");
    assert_eq!(alpn, None, "Node's https client offers no ALPN");
}

/// `agent.createConnection`: the exchange runs over a socket `perry-ext-net`
/// owns, through the raw-net vtable. The socket here is a real ext-net socket
/// (a connected stream adopted onto the loop exactly as an HTTP upgrade hands
/// one over), in raw mode, so this is the production read path end to end.
///
/// What it proves beyond "a response arrived": the response is written by the
/// server only AFTER the request has been read, so the one drain `start`
/// schedules up front finds nothing, and every byte after that is read only
/// because `perry-ext-net` called `perry_ffi::raw_net_notify`. Sabotage-checked:
/// with the notify registration removed, this shape never completes.
fn a_create_connection_socket_is_read_when_net_says_it_is_ready() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("an ephemeral port");
    let port = listener.local_addr().expect("a bound address").port();
    let (heads, received) = mpsc::channel::<String>();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("one connection");
        let (head, _) = read_request(&mut stream).expect("a request head");
        // Two writes with a gap: the second arrives on a later read, so the
        // exchange needs a second notification to finish.
        let _ = stream.write_all(b"HTTP/1.1 200 Over Your Socket\r\ncontent-length: 5\r\n\r\nhe");
        let _ = stream.flush();
        std::thread::sleep(Duration::from_millis(50));
        let _ = stream.write_all(b"llo");
        let _ = heads.send(head);
        // Close: this path reads to EOF (it sends `Connection: close`).
    });

    let stream = TcpStream::connect(("127.0.0.1", port)).expect("the client side connects");
    let socket = perry_ext_net::adopt_upgraded_tcp_stream(stream);
    assert_ne!(
        socket,
        perry_ffi::INVALID_HANDLE,
        "ext-net adopted the socket"
    );
    // Publish the raw-net vtable (the adoption itself already ran, here).
    perry_ext_net::ensure_adopted_socket_dispatch();
    assert!(
        perry_ffi::raw_net().is_some(),
        "perry-ext-net's raw-net vtable must be published, or this shape tests nothing"
    );

    let completed_before = client_turnloop::raw_completed_total();
    let url = format!("http://127.0.0.1:{port}/over-socket");
    client_turnloop::try_dispatch_over_socket(
        OVER_SOCKET,
        "GET",
        &url,
        &no_headers(),
        &[],
        None,
        socket,
    );
    drive("createConnection");
    assert_eq!(
        client_turnloop::raw_completed_total(),
        completed_before + 1,
        "the response must have been read to EOF and delivered"
    );
    let head = received
        .recv_timeout(Duration::from_secs(10))
        .expect("the server must have received the request over the adopted socket");
    join(server, "createConnection server");
    settle();
    assert!(head.starts_with("GET /over-socket HTTP/1.1\r\n"), "{head}");
    assert!(head.contains("Connection: close\r\n"), "{head}");
}

/// The Agent facade's idle expiry and `req.setTimeout`'s early `'timeout'` are
/// deadlines on the loop now (they were tokio sleeps).
fn a_deferred_event_fires_from_a_loop_deadline() {
    let fired_before = client_turnloop::deferred_fired_total();
    let started = Instant::now();
    client_turnloop::schedule_timeout_for_test(DEFERRED, 40);
    let deadline = Instant::now() + Duration::from_secs(10);
    while client_turnloop::deferred_fired_total() == fired_before {
        turn();
        assert!(Instant::now() < deadline, "the loop deadline never fired");
    }
    assert!(
        started.elapsed() >= Duration::from_millis(40),
        "it fired early: {:?}",
        started.elapsed()
    );
}
