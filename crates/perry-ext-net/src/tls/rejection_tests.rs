use super::*;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};

// Public test certificates only: the CA leaf was signed by a distinct issuer.
const ISSUER: &[u8] = include_bytes!("test_data/issuer.der");
const ISSUED_LEAF: &[u8] = include_bytes!("test_data/ca-issued-leaf.der");

fn self_signed_leaf() -> Vec<u8> {
    let mut pem = std::io::Cursor::new(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../test-parity/node-suite/tls/fixtures/localhost-cert.pem"
    )));
    let cert = rustls_pemfile::certs(&mut pem).next().unwrap().unwrap();
    cert.as_ref().to_vec()
}

fn verifier(
    root: &[u8],
    configured: Vec<Vec<u8>>,
    custom_identity: bool,
) -> NodeConfiguredCaVerifier {
    let mut roots = rustls::RootCertStore::empty();
    roots.add(CertificateDer::from(root.to_vec())).unwrap();
    let roots = Arc::new(roots);
    let inner = rustls::client::WebPkiServerVerifier::builder_with_provider(
        roots.clone(),
        Arc::new(rustls::crypto::ring::default_provider()),
    )
    .build()
    .unwrap();
    NodeConfiguredCaVerifier {
        inner,
        roots,
        configured,
        custom_identity,
    }
}

fn verify(
    verifier: &NodeConfiguredCaVerifier,
    leaf: &[u8],
    host: &str,
) -> Result<ServerCertVerified, rustls::Error> {
    verifier.verify_server_cert(
        &CertificateDer::from(leaf),
        &[],
        &ServerName::try_from(host.to_string()).unwrap(),
        &[],
        UnixTime::now(),
    )
}

#[test]
fn untrusted_self_signed_leaf_keeps_rejection_and_cause() {
    let _serial = crate::test_serial();
    let leaf = self_signed_leaf();
    for custom_identity in [false, true] {
        let error = verify(
            &verifier(ISSUER, Vec::new(), custom_identity),
            &leaf,
            "localhost",
        )
        .unwrap_err();
        assert_eq!(
            crate::turnloop_tls::node_code(&error),
            "DEPTH_ZERO_SELF_SIGNED_CERT"
        );
        assert!(
            crate::turnloop_tls::node_message(&error).starts_with("DEPTH_ZERO_SELF_SIGNED_CERT:")
        );
    }
}

#[test]
fn issued_ca_and_invalid_self_signature_are_not_self_signed() {
    let _serial = crate::test_serial();
    let mut altered = self_signed_leaf();
    *altered.last_mut().unwrap() ^= 1;
    for leaf in [ISSUED_LEAF, altered.as_slice()] {
        for custom_identity in [false, true] {
            let error = verify(
                &verifier(ISSUER, Vec::new(), custom_identity),
                leaf,
                "localhost",
            )
            .unwrap_err();
            assert_ne!(
                crate::turnloop_tls::node_code(&error),
                "DEPTH_ZERO_SELF_SIGNED_CERT"
            );
        }
    }
}

#[test]
fn configured_ca_and_hostname_validation_remain_unchanged() {
    let _serial = crate::test_serial();
    let leaf = self_signed_leaf();
    for custom_identity in [false, true] {
        let trusted = verifier(&leaf, vec![leaf.clone()], custom_identity);
        assert!(verify(&trusted, &leaf, "localhost").is_ok());
    }
    let trusted = verifier(&leaf, vec![leaf.clone()], false);
    let error = verify(&trusted, &leaf, "wrong.example").unwrap_err();
    assert_eq!(
        crate::turnloop_tls::node_code(&error),
        "ERR_TLS_CERT_ALTNAME_INVALID"
    );
}

fn handshake(config: Arc<rustls::ClientConfig>, leaf: &[u8], host: &str) -> (bool, Option<String>) {
    let mut key = std::io::Cursor::new(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../test-parity/node-suite/tls/fixtures/localhost-key.pem"
    )));
    let server = rustls::ServerConfig::builder_with_provider(
        rustls::crypto::ring::default_provider().into(),
    )
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(
        vec![CertificateDer::from(leaf.to_vec())],
        rustls_pemfile::private_key(&mut key).unwrap().unwrap(),
    )
    .unwrap();
    let mut server = crate::turnloop_tls::TlsSession::server(Arc::new(server)).unwrap();
    let mut client = crate::turnloop_tls::TlsSession::client(
        config,
        ServerName::try_from(host.to_string()).unwrap(),
    )
    .unwrap();
    let mut completed = false;
    for _ in 0..64 {
        completed |= client.pump().handshake_done;
        server.receive(&client.take_output());
        server.pump();
        client.receive(&server.take_output());
        completed |= client.pump().handshake_done;
        if let Some(error) = client.failure() {
            return (completed, Some(error.to_string()));
        }
        if !client.is_handshaking() && !server.is_handshaking() {
            return (completed, None);
        }
    }
    panic!("the session handshake did not settle");
}

fn factory(root: &[u8], data: Option<&TlsClientConfigData>) -> Arc<rustls::ClientConfig> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let mut roots = rustls::RootCertStore::empty();
    if !root.is_empty() {
        roots.add(CertificateDer::from(root.to_vec())).unwrap();
    }
    finish_verified_client_config(roots, data).unwrap()
}

#[test]
fn default_factory_session_rejects_with_self_signed_cause() {
    let _serial = crate::test_serial();
    let leaf = self_signed_leaf();
    let (completed, failure) = handshake(factory(ISSUER, None), &leaf, "localhost");
    assert!(!completed);
    assert!(failure.unwrap().starts_with("DEPTH_ZERO_SELF_SIGNED_CERT:"));
}

#[test]
fn factory_session_rejection_and_trust_controls() {
    let _serial = crate::test_serial();
    let mut altered = self_signed_leaf();
    *altered.last_mut().unwrap() ^= 1;
    for leaf in [ISSUED_LEAF, altered.as_slice()] {
        let (completed, failure) = handshake(factory(ISSUER, None), leaf, "localhost");
        assert!(!completed);
        assert!(!failure.unwrap().starts_with("DEPTH_ZERO_SELF_SIGNED_CERT:"));
    }
    let leaf = self_signed_leaf();
    let pem = Vec::from(
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../test-parity/node-suite/tls/fixtures/localhost-cert.pem"
        ))
        .as_slice(),
    );
    let trusted = TlsClientConfigData {
        ca: Some(vec![pem]),
        version_mask: 0b11,
        ..TlsClientConfigData::default()
    };
    assert_eq!(
        handshake(factory(&leaf, Some(&trusted)), &leaf, "localhost"),
        (true, None)
    );
    let (completed, failure) = handshake(factory(&leaf, Some(&trusted)), &leaf, "wrong.example");
    assert!(!completed);
    assert!(failure
        .unwrap()
        .starts_with("ERR_TLS_CERT_ALTNAME_INVALID:"));
    let empty = TlsClientConfigData {
        ca: Some(Vec::new()),
        version_mask: 0b11,
        ..TlsClientConfigData::default()
    };
    let (completed, failure) = handshake(factory(&[], Some(&empty)), &leaf, "localhost");
    assert!(!completed && failure.is_some());
    assert_eq!(
        handshake(
            build_client_config(false, None).unwrap(),
            &leaf,
            "localhost"
        ),
        (true, None)
    );
}
