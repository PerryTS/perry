//! The `node:tls` server on turnloop, end to end over real sockets: a blocking
//! rustls client on a plain thread against the listener this thread's loop
//! owns. Each test asserts its subject ran — the handshake completed, the
//! bytes arrived, the sink was installed — rather than only that nothing
//! failed.

use super::*;
use rustls::pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer, ServerName};
use std::io::{Read, Write};
use std::time::{Duration, Instant};

const TEST_CERT: &str = include_str!("../../../perry-tls-session/tests/test-cert.pem");
const TEST_CA: &str = include_str!("../../../perry-tls-session/tests/test-ca.pem");
const TEST_KEY: &str = include_str!("../../../perry-tls-session/tests/test-key.pem");

fn server_config() -> Arc<rustls::ServerConfig> {
    ensure_crypto_provider_installed();
    let chain: Vec<CertificateDer<'static>> = CertificateDer::pem_slice_iter(TEST_CERT.as_bytes())
        .collect::<Result<_, _>>()
        .unwrap();
    let key = PrivateKeyDer::from_pem_slice(TEST_KEY.as_bytes()).unwrap();
    Arc::new(
        rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(chain, key)
            .unwrap(),
    )
}

fn client_config(trust_test_ca: bool) -> Arc<rustls::ClientConfig> {
    ensure_crypto_provider_installed();
    let mut roots = rustls::RootCertStore::empty();
    if trust_test_ca {
        for cert in CertificateDer::pem_slice_iter(TEST_CA.as_bytes()) {
            roots.add(cert.unwrap()).unwrap();
        }
    }
    Arc::new(
        rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth(),
    )
}

/// A short, comparable description of each queued event, taken before
/// `js_tls_process_pending` consumes them.
fn snapshot_events() -> Vec<String> {
    pending_events()
        .lock()
        .unwrap()
        .iter()
        .map(|event| match event {
            PendingTlsEvent::ServerListening(_) => "listening".to_string(),
            PendingTlsEvent::ServerSecureConnection(_, socket) => format!("secure:{socket}"),
            PendingTlsEvent::ServerClose(_) => "server-close".to_string(),
            PendingTlsEvent::ServerError(_, message) => format!("server-error:{message}"),
            PendingTlsEvent::ServerTlsClientError(_, _, message, _) => {
                format!("tls-client-error:{message}")
            }
            PendingTlsEvent::SocketData(_, bytes) => {
                format!("data:{}", String::from_utf8_lossy(bytes))
            }
            PendingTlsEvent::SocketEnd(_) => "end".to_string(),
            PendingTlsEvent::SocketClose(_) => "close".to_string(),
            PendingTlsEvent::SocketError(_, message) => format!("error:{message}"),
        })
        .collect()
}

/// Turn the loop and deliver events until `done` says so, recording every
/// event seen. `react` may submit work (a write) in response to one.
fn drive_until(
    seen: &mut Vec<String>,
    mut react: impl FnMut(&str),
    mut done: impl FnMut(&[String]) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !done(seen) {
        assert!(
            Instant::now() < deadline,
            "timed out; events so far: {seen:?}"
        );
        perry_runtime::event_pump::js_loop_turn_bounded(5);
        let batch = snapshot_events();
        // SAFETY: this test thread is the pump; no user closures are installed.
        unsafe {
            js_tls_process_pending();
        }
        for event in &batch {
            react(event);
        }
        seen.extend(batch);
    }
}

fn listening_server() -> (i64, u16) {
    let undefined = TAG_UNDEFINED_BITS as i64;
    // SAFETY: undefined options/callbacks are valid API arguments.
    let server = unsafe { js_tls_create_server(undefined, undefined) };
    servers().lock().unwrap().get_mut(&server).unwrap().config = Some(server_config());
    // SAFETY: as above; `server` came from `js_tls_create_server`.
    unsafe { js_tls_server_listen(server, 0.0, undefined, undefined) };
    let port = servers().lock().unwrap().get(&server).unwrap().bound_port;
    assert_ne!(port, 0, "the listener must be bound before listen() returns");
    assert!(
        perry_runtime::turnloop_net::sink_installed(turnloop_server::SUBSYSTEM),
        "the TLS server's completion sink was never installed"
    );
    assert!(
        perry_runtime::turnloop_net::is_live(turnloop_server::tl_id(server)),
        "the listener is not a turnloop handle"
    );
    (server, port)
}

fn close_server(server: i64) {
    let undefined = TAG_UNDEFINED_BITS as i64;
    // SAFETY: `server` came from `js_tls_create_server`.
    unsafe { js_tls_server_close(server, undefined) };
    let mut seen = Vec::new();
    drive_until(&mut seen, |_| {}, |_| !servers().lock().unwrap().contains_key(&server));
}

#[test]
fn handshake_data_both_ways_and_close_notify_end_then_close() {
    let _owner = crate::turnloop_client::become_the_owner_for_test();
    let (server, port) = listening_server();

    let client = std::thread::spawn(move || {
        let tcp = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        let conn = rustls::ClientConnection::new(
            client_config(true),
            ServerName::try_from("localhost").unwrap(),
        )
        .unwrap();
        let mut tls = rustls::StreamOwned::new(conn, tcp);
        tls.write_all(b"hello").unwrap();
        tls.flush().unwrap();
        let mut reply = [0u8; 5];
        tls.read_exact(&mut reply).unwrap();
        let protocol = tls.conn.protocol_version();
        tls.conn.send_close_notify();
        tls.flush().unwrap();
        // The server answers our close_notify with its own: a clean EOF.
        let mut rest = Vec::new();
        let tail = tls.read_to_end(&mut rest);
        (reply, protocol, tail.map(|_| rest))
    });

    let mut seen = Vec::new();
    let mut socket_id = None;
    drive_until(
        &mut seen,
        |event| {
            if let Some(id) = event.strip_prefix("secure:") {
                socket_id = Some(id.parse::<i64>().unwrap());
            }
            if event == "data:hello" {
                turnloop_server::write(socket_id.expect("data before secure"), b"world".to_vec());
            }
        },
        |seen| seen.iter().any(|e| e == "close"),
    );
    let (reply, protocol, tail) = client.join().unwrap();
    assert_eq!(&reply, b"world");
    assert!(protocol.is_some(), "the client never negotiated a protocol");
    assert_eq!(tail.unwrap(), Vec::<u8>::new(), "expected a clean TLS EOF");
    assert_eq!(
        seen,
        vec![
            "listening".to_string(),
            format!("secure:{}", socket_id.unwrap()),
            "data:hello".to_string(),
            "end".to_string(),
            "close".to_string(),
        ]
    );
    close_server(server);
}

#[test]
fn a_rejected_certificate_is_a_tls_client_error_not_a_connection() {
    let _owner = crate::turnloop_client::become_the_owner_for_test();
    let (server, port) = listening_server();
    let client = std::thread::spawn(move || {
        let tcp = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        let conn = rustls::ClientConnection::new(
            client_config(false),
            ServerName::try_from("localhost").unwrap(),
        )
        .unwrap();
        let mut tls = rustls::StreamOwned::new(conn, tcp);
        // Fails: the client does not trust the test CA and sends an alert.
        tls.write_all(b"x").and_then(|_| tls.flush()).is_err()
    });
    let mut seen = Vec::new();
    drive_until(
        &mut seen,
        |_| {},
        |seen| seen.iter().any(|e| e.starts_with("tls-client-error:")),
    );
    assert!(client.join().unwrap(), "the untrusting client connected");
    let error = seen
        .iter()
        .find(|e| e.starts_with("tls-client-error:"))
        .unwrap();
    assert!(
        error.starts_with("tls-client-error:tls handshake: "),
        "unexpected handshake error text: {error}"
    );
    assert!(!seen.iter().any(|e| e.starts_with("secure:")), "{seen:?}");
    close_server(server);
}

#[test]
fn tcp_eof_mid_handshake_reports_tls_handshake_eof() {
    let _owner = crate::turnloop_client::become_the_owner_for_test();
    let (server, port) = listening_server();
    drop(std::net::TcpStream::connect(("127.0.0.1", port)).unwrap());
    let mut seen = Vec::new();
    drive_until(
        &mut seen,
        |_| {},
        |seen| seen.iter().any(|e| e.starts_with("tls-client-error:")),
    );
    assert!(
        seen.iter()
            .any(|e| e == "tls-client-error:tls handshake: tls handshake eof"),
        "{seen:?}"
    );
    close_server(server);
}
