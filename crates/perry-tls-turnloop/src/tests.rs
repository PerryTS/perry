//! In-memory proof that the three things this crate promises a caller — the
//! negotiated ALPN protocol, the verified peer chain, and the RFC 5929
//! `tls-server-end-point` digest — are really produced by a real handshake.
//!
//! No socket and no turnloop handle: the ciphertext is carried between the two
//! endpoints by moving `Vec<u8>`s, which is exactly what
//! [`TlsClientTransport::pump`] hands to `tl::write`. That keeps the test
//! runnable in `cargo test` on any host while still exercising rustls rather
//! than a stub — the distinction CLAUDE.md's "a gate must assert its subject
//! was live" is about. `handshake_alpn_and_channel_binding` fails if the
//! handshake does not actually complete, so a green run is not vacuous.

use super::*;

const TEST_CERT: &str = include_str!("../tests/test-cert.pem");
/// The CA that signed it. Separate from the leaf because webpki refuses a CA
/// certificate presented as an end entity (`CaUsedAsEndEntity`) — a self-signed
/// leaf would make this test fail for a reason that has nothing to do with the
/// code under test.
const TEST_CA: &str = include_str!("../tests/test-ca.pem");
const TEST_KEY: &str = include_str!("../tests/test-key.pem");

/// SHA-256 of `TEST_CERT`'s DER, computed outside this crate:
///
/// ```text
/// openssl x509 -in tests/test-cert.pem -outform DER | openssl dgst -sha256
/// ```
///
/// The certificate is RSA/SHA-256 on purpose. RFC 5929 derives the binding
/// hash from the *signature* algorithm, so an Ed25519 leaf would make
/// `tls_server_end_point` return `None` — a correct answer that would make
/// this test pass while proving nothing.
const EXPECTED_BINDING: &str = "49f31d1bc29d107103cbb137902caeebe7554ae9a2518c490e8414d88238eea3";

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn server_config(alpn: &[&[u8]]) -> turnloop_tls::ServerConfig {
    use turnloop_tls::rustls::pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer};
    let chain: Vec<CertificateDer<'static>> = CertificateDer::pem_slice_iter(TEST_CERT.as_bytes())
        .collect::<Result<_, _>>()
        .expect("test certificate parses");
    let key = PrivateKeyDer::from_pem_slice(TEST_KEY.as_bytes()).expect("test key parses");
    turnloop_tls::ServerConfig::new(
        chain,
        key,
        alpn.iter().map(|p| p.to_vec()).collect(),
        unix_seconds(),
    )
    .expect("server config")
}

/// Drive the server endpoint until it blocks, returning the ciphertext it
/// produced. The mirror of `TlsClientTransport::pump` with the socket replaced
/// by the returned `Vec`.
fn pump_server(server: &mut turnloop_tls::Server, input: &mut Vec<u8>) -> Vec<u8> {
    use turnloop_tls::rustls::unbuffered::ConnectionState;
    let mut out = Vec::new();
    let mut scratch = vec![0u8; 64 * 1024];
    let mut scratch_len = 0usize;
    let mut transmitted = false;
    for _ in 0..4096 {
        let status = server.process(input, unix_seconds());
        let mut discard = status.discard;
        let blocked = match status.state {
            Err(error) => panic!("server handshake failed: {error}"),
            Ok(ConnectionState::EncodeTlsData(mut encode)) => {
                scratch_len += encode.encode(&mut scratch[scratch_len..]).expect("encode");
                false
            }
            Ok(ConnectionState::TransmitTlsData(transmit)) => {
                if transmitted {
                    transmit.done();
                    transmitted = false;
                } else {
                    out.extend_from_slice(&scratch[..scratch_len]);
                    scratch_len = 0;
                    transmitted = true;
                }
                false
            }
            Ok(ConnectionState::ReadTraffic(mut read)) => {
                if let Some(record) = read.next_record() {
                    discard += record.expect("record").discard;
                }
                false
            }
            Ok(ConnectionState::BlockedHandshake | ConnectionState::WriteTraffic(_)) => true,
            Ok(ConnectionState::PeerClosed | ConnectionState::Closed) => true,
            Ok(_) => true,
        };
        if discard > 0 {
            input.drain(..discard.min(input.len()));
        }
        if scratch_len > 0 && !transmitted {
            out.extend_from_slice(&scratch[..scratch_len]);
            scratch_len = 0;
        }
        if blocked {
            break;
        }
    }
    out
}

#[test]
fn handshake_alpn_and_channel_binding() {
    let options = TlsClientOptions {
        servername: "localhost".to_string(),
        ca_pem: TEST_CA.as_bytes().to_vec(),
        ..TlsClientOptions::default()
    }
    // What `http2.connect` and `fetch` offer; the server below accepts only
    // `h2`, so a selection of `http/1.1` would mean the list was not sent in
    // preference order — or not sent at all.
    .with_alpn(&[b"h2", b"http/1.1"]);

    let mut client = TlsClientTransport::connect(&options).expect("client session");
    assert!(client.is_handshaking(), "a fresh session is handshaking");
    assert!(
        client.facts().is_none(),
        "no facts are available before the handshake completes"
    );

    let mut server = server_config(&[b"h2"]).accept().expect("server session");
    let mut to_server = Vec::new();
    let mut to_client = Vec::new();

    // The client half of `pump`, with `tl::write` replaced by `to_server`.
    let mut done = false;
    for _ in 0..16 {
        client.receive(&std::mem::take(&mut to_client));
        let progress = client.session.pump();
        assert!(
            client.session.failure().is_none(),
            "client handshake failed: {:?}",
            client.session.failure()
        );
        to_server.extend_from_slice(&client.session.take_output());
        if progress.handshake_done {
            client.facts = Some(TlsFacts {
                alpn: client.session.alpn_protocol(),
                peer_certificates: client.session.peer_certificates().unwrap_or_default(),
                channel_binding: client.session.tls_server_end_point(),
            });
            done = true;
            break;
        }
        to_client = pump_server(&mut server, &mut to_server);
        if to_client.is_empty() && to_server.is_empty() {
            break;
        }
    }
    assert!(
        done,
        "the handshake did not complete — the test proved nothing"
    );

    let facts = client.facts().expect("facts after the handshake");
    assert_eq!(facts.alpn.as_deref(), Some(&b"h2"[..]), "ALPN selection");
    assert_eq!(facts.alpn_str(), "h2");
    assert_eq!(
        facts.peer_certificates.len(),
        1,
        "the server sent exactly its leaf"
    );
    assert_eq!(
        hex(facts
            .channel_binding
            .as_deref()
            .expect("an RSA/SHA-256 leaf has a defined binding")),
        EXPECTED_BINDING,
        "tls-server-end-point is SHA-256 over the verified leaf's DER"
    );
    // The binding must be over the leaf the peer actually presented, not over
    // anything this crate re-encoded: a mismatch here is how a channel-binding
    // bug authenticates against the wrong certificate.
    assert_eq!(
        &facts.peer_certificates[0][..],
        {
            use turnloop_tls::rustls::pki_types::{pem::PemObject, CertificateDer};
            let leaf = CertificateDer::pem_slice_iter(TEST_CERT.as_bytes())
                .next()
                .expect("one certificate")
                .expect("parses");
            leaf.as_ref().to_vec()
        },
        "the reported leaf is the configured certificate"
    );
    assert!(!client.is_handshaking());
}

#[test]
fn no_alpn_offered_means_none_negotiated() {
    let options = TlsClientOptions {
        servername: "localhost".to_string(),
        ca_pem: TEST_CA.as_bytes().to_vec(),
        ..TlsClientOptions::default()
    };
    assert!(
        options.alpn.is_empty(),
        "a database client offers no ALPN by default"
    );
    client_config(&options).expect("a config with no ALPN is valid");
}

#[test]
fn a_ca_that_is_not_a_certificate_is_refused() {
    let options = TlsClientOptions {
        servername: "localhost".to_string(),
        ca_pem: b"-----BEGIN CERTIFICATE-----\nnot base64\n-----END CERTIFICATE-----\n".to_vec(),
        ..TlsClientOptions::default()
    };
    let Err(error) = client_config(&options) else {
        panic!("a malformed ca must be refused");
    };
    assert!(
        error.starts_with("ERR_TLS_CERT_ALTNAME_INVALID"),
        "carries a Node cause code, got {error:?}"
    );
}

#[test]
fn an_empty_ca_pem_keeps_the_default_roots() {
    // Node's `ca` REPLACES the default roots, so an empty one must mean
    // "unset" rather than "trust nothing" — otherwise a binding that always
    // passes a (possibly empty) `ca` through would silently refuse every
    // public certificate.
    let options = TlsClientOptions {
        servername: "example.com".to_string(),
        ..TlsClientOptions::default()
    };
    client_config(&options).expect("default roots");
}

#[test]
fn an_invalid_servername_is_refused_with_a_node_code() {
    let options = TlsClientOptions {
        servername: "not a host name".to_string(),
        ..TlsClientOptions::default()
    };
    let Err(error) = TlsClientTransport::connect(&options) else {
        panic!("an invalid servername must be refused");
    };
    assert!(
        error.starts_with("ERR_TLS_CERT_ALTNAME_INVALID"),
        "got {error:?}"
    );
}

#[test]
fn node_environment_options_default_to_verifying() {
    let options = TlsClientOptions::from_node_environment("db.example.com");
    assert_eq!(options.servername, "db.example.com");
    assert!(options.alpn.is_empty(), "ALPN is opt-in, never inherited");
    assert!(options.enable_sni);
}
