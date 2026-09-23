//! In-memory handshakes between a client and a server [`TlsSession`]: the
//! ciphertext moves between them as `Vec<u8>`s, so every assertion below is
//! about real rustls records, not a stub.

use super::*;
use turnloop_tls::rustls::pki_types::{pem::PemObject, PrivateKeyDer, ServerName};

const TEST_CERT: &str = include_str!("../../tests/test-cert.pem");
const TEST_CA: &str = include_str!("../../tests/test-ca.pem");
const TEST_KEY: &str = include_str!("../../tests/test-key.pem");

fn provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

fn server_config(alpn: &[&[u8]]) -> Arc<rustls::ServerConfig> {
    let chain: Vec<CertificateDer<'static>> = CertificateDer::pem_slice_iter(TEST_CERT.as_bytes())
        .collect::<Result<_, _>>()
        .expect("test certificate parses");
    let key = PrivateKeyDer::from_pem_slice(TEST_KEY.as_bytes()).expect("test key parses");
    let mut config = rustls::ServerConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .expect("versions")
        .with_no_client_auth()
        .with_single_cert(chain, key)
        .expect("server config");
    config.alpn_protocols = alpn.iter().map(|p| p.to_vec()).collect();
    Arc::new(config)
}

fn client_config(trust_test_ca: bool, alpn: &[&[u8]]) -> Arc<rustls::ClientConfig> {
    let mut roots = rustls::RootCertStore::empty();
    if trust_test_ca {
        for cert in CertificateDer::pem_slice_iter(TEST_CA.as_bytes()) {
            roots.add(cert.expect("ca parses")).expect("ca added");
        }
    }
    let mut config = rustls::ClientConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .expect("versions")
        .with_root_certificates(roots)
        .with_no_client_auth();
    config.alpn_protocols = alpn.iter().map(|p| p.to_vec()).collect();
    Arc::new(config)
}

fn localhost() -> ServerName<'static> {
    ServerName::try_from("localhost").expect("server name")
}

/// Shuttle ciphertext both ways until neither side produces any more.
fn exchange(client: &mut TlsSession, server: &mut TlsSession) {
    for _ in 0..64 {
        client.pump();
        let to_server = client.take_output();
        server.receive(&to_server);
        server.pump();
        let to_client = server.take_output();
        client.receive(&to_client);
        client.pump();
        if to_server.is_empty() && to_client.is_empty() && !client.has_output() {
            return;
        }
    }
    panic!("the exchange did not settle");
}

#[test]
fn handshake_data_and_close_notify_round_trip() {
    let mut client =
        TlsSession::client(client_config(true, &[b"h2", b"http/1.1"]), localhost()).unwrap();
    let mut server = TlsSession::server(server_config(&[b"http/1.1"])).unwrap();
    // Written before the handshake finishes: must not be lost.
    client.write(b"hello from the client");
    exchange(&mut client, &mut server);

    assert!(client.failure().is_none(), "{:?}", client.failure());
    assert!(server.failure().is_none(), "{:?}", server.failure());
    assert!(!client.is_handshaking() && !server.is_handshaking());
    assert_eq!(client.alpn_protocol(), Some(&b"http/1.1"[..]));
    assert_eq!(server.alpn_protocol(), Some(&b"http/1.1"[..]));
    assert_eq!(server.server_name(), Some("localhost"));
    assert_eq!(client.server_name(), None);
    assert!(client.protocol_version().is_some());
    assert!(client.peer_certificates().is_some_and(|c| !c.is_empty()));
    assert_eq!(server.take_plaintext(), b"hello from the client");

    server.write(b"reply");
    exchange(&mut client, &mut server);
    assert_eq!(client.take_plaintext(), b"reply");

    // Client half-closes; the server sees EOF but can still answer.
    client.close_notify();
    exchange(&mut client, &mut server);
    assert!(client.close_sent());
    assert!(server.peer_closed(), "server must observe close_notify");
    server.write(b"after half-close");
    exchange(&mut client, &mut server);
    assert_eq!(
        client.take_plaintext(),
        b"after half-close",
        "a half-open server must still be able to write"
    );
    server.close_notify();
    exchange(&mut client, &mut server);
    assert!(client.peer_closed());
}

#[test]
fn rejected_certificate_fails_and_sends_the_alert() {
    let mut client = TlsSession::client(client_config(false, &[]), localhost()).unwrap();
    let mut server = TlsSession::server(server_config(&[])).unwrap();
    for _ in 0..8 {
        client.pump();
        server.receive(&client.take_output());
        server.pump();
        client.receive(&server.take_output());
        client.pump();
        if client.failure().is_some() {
            break;
        }
    }
    let failure = client
        .failure()
        .cloned()
        .expect("an untrusted chain must fail");
    assert_eq!(failure.code, "UNABLE_TO_VERIFY_LEAF_SIGNATURE");
    assert!(
        failure.message.contains("UnknownIssuer"),
        "rustls's own text is kept: {}",
        failure.message
    );
    // The alert rustls queued with the error reached the output, and the
    // server reads it as the reason — not as a bare EOF.
    let alert = client.take_output();
    assert!(!alert.is_empty(), "the fatal alert must be flushed");
    server.receive(&alert);
    server.pump();
    let server_failure = server.failure().cloned().expect("server sees the alert");
    assert!(
        server_failure.message.contains("received fatal alert"),
        "{}",
        server_failure.message
    );
    // A failed session refuses further writes.
    client.write(b"ignored");
    assert_eq!(client.pending_plaintext(), 0);
}

#[test]
fn server_name_survives_a_client_hello_split_into_single_bytes() {
    let mut client = TlsSession::client(client_config(true, &[]), localhost()).unwrap();
    let mut server = TlsSession::server(server_config(&[])).unwrap();
    client.pump();
    let hello = client.take_output();
    assert!(hello.len() > 5);
    for byte in &hello {
        server.receive(std::slice::from_ref(byte));
    }
    server.pump();
    assert!(server.failure().is_none(), "{:?}", server.failure());
    assert_eq!(server.server_name(), Some("localhost"));
}

#[test]
fn an_ip_literal_is_not_a_server_name() {
    let ip = ServerName::try_from("127.0.0.1").unwrap();
    let mut client = TlsSession::client(client_config(true, &[]), ip).unwrap();
    let mut server = TlsSession::server(server_config(&[])).unwrap();
    client.pump();
    server.receive(&client.take_output());
    server.pump();
    assert_eq!(server.server_name(), None);
}
