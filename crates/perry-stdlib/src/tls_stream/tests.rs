//! The `tokio_rustls` contract [`TlsStream`] keeps, over an in-memory duplex
//! pipe: real rustls records on both ends, no socket.

use super::*;
use rustls::pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const TEST_CERT: &str = include_str!("../../../perry-tls-session/tests/test-cert.pem");
const TEST_CA: &str = include_str!("../../../perry-tls-session/tests/test-ca.pem");
const TEST_KEY: &str = include_str!("../../../perry-tls-session/tests/test-key.pem");

fn provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::aws_lc_rs::default_provider())
}

fn server_config() -> Arc<rustls::ServerConfig> {
    let chain: Vec<CertificateDer<'static>> = CertificateDer::pem_slice_iter(TEST_CERT.as_bytes())
        .collect::<Result<_, _>>()
        .unwrap();
    let key = PrivateKeyDer::from_pem_slice(TEST_KEY.as_bytes()).unwrap();
    Arc::new(
        rustls::ServerConfig::builder_with_provider(provider())
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(chain, key)
            .unwrap(),
    )
}

fn client_config(trust_test_ca: bool) -> Arc<rustls::ClientConfig> {
    let mut roots = rustls::RootCertStore::empty();
    if trust_test_ca {
        for cert in CertificateDer::pem_slice_iter(TEST_CA.as_bytes()) {
            roots.add(cert.unwrap()).unwrap();
        }
    }
    Arc::new(
        rustls::ClientConfig::builder_with_provider(provider())
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_root_certificates(roots)
            .with_no_client_auth(),
    )
}

fn localhost() -> ServerName<'static> {
    ServerName::try_from("localhost").unwrap()
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

#[test]
fn round_trip_and_close_notify_is_a_clean_eof() {
    runtime().block_on(async {
        let (client_io, server_io) = tokio::io::duplex(1024);
        let server = tokio::spawn(async move {
            let mut tls = TlsStream::accept(server_io, server_config()).await.unwrap();
            assert_eq!(tls.session().server_name(), Some("localhost"));
            let mut request = vec![0u8; 5];
            tls.read_exact(&mut request).await.unwrap();
            assert_eq!(&request, b"hello");
            // Larger than the duplex buffer and one TLS record: exercises
            // backpressure and record splitting.
            let big = vec![7u8; 100_000];
            tls.write_all(&big).await.unwrap();
            tls.flush().await.unwrap();
            // The client's close_notify reads as a clean EOF.
            let mut rest = Vec::new();
            tls.read_to_end(&mut rest).await.unwrap();
            assert!(rest.is_empty());
            tls.shutdown().await.unwrap();
        });
        let mut tls = TlsStream::connect(client_io, client_config(true), localhost())
            .await
            .unwrap();
        assert!(tls.session().protocol_version().is_some());
        tls.write_all(b"hello").await.unwrap();
        tls.flush().await.unwrap();
        let mut big = vec![0u8; 100_000];
        tls.read_exact(&mut big).await.unwrap();
        assert!(big.iter().all(|b| *b == 7));
        tls.shutdown().await.unwrap();
        let mut rest = Vec::new();
        tls.read_to_end(&mut rest).await.unwrap();
        assert!(rest.is_empty());
        server.await.unwrap();
    });
}

#[test]
fn rejected_certificate_reports_rustls_text_and_the_server_sees_the_alert() {
    runtime().block_on(async {
        let (client_io, server_io) = tokio::io::duplex(64 * 1024);
        let server = tokio::spawn(async move {
            TlsStream::accept(server_io, server_config())
                .await
                .err()
                .expect("the server handshake must fail")
        });
        let client_error = TlsStream::connect(client_io, client_config(false), localhost())
            .await
            .err()
            .expect("an untrusted chain must fail");
        assert_eq!(client_error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(
            client_error.to_string(),
            "invalid peer certificate: UnknownIssuer"
        );
        let server_error = server.await.unwrap();
        assert_eq!(server_error.kind(), io::ErrorKind::InvalidData);
        assert!(
            server_error.to_string().starts_with("received fatal alert"),
            "the alert must reach the peer, not a bare EOF: {server_error}"
        );
    });
}

#[test]
fn eof_mid_handshake_is_tls_handshake_eof() {
    runtime().block_on(async {
        let (client_io, server_io) = tokio::io::duplex(1024);
        drop(client_io);
        let error = TlsStream::accept(server_io, server_config())
            .await
            .err()
            .unwrap();
        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
        assert_eq!(error.to_string(), "tls handshake eof");
    });
}

#[test]
fn eof_without_close_notify_is_unexpected_eof() {
    runtime().block_on(async {
        let (client_io, server_io) = tokio::io::duplex(64 * 1024);
        let (read_tx, read_rx) = tokio::sync::oneshot::channel::<()>();
        let server = tokio::spawn(async move {
            let mut tls = TlsStream::accept(server_io, server_config()).await.unwrap();
            let mut byte = [0u8; 1];
            tls.read_exact(&mut byte).await.unwrap();
            read_tx.send(()).unwrap();
            let mut rest = Vec::new();
            tls.read_to_end(&mut rest).await.err().unwrap()
        });
        let mut tls = TlsStream::connect(client_io, client_config(true), localhost())
            .await
            .unwrap();
        tls.write_all(b"x").await.unwrap();
        tls.flush().await.unwrap();
        // Once the server has the byte, drop the transport without
        // close_notify. (Dropping earlier would make the server's post-handshake
        // session tickets hit a closed pipe — as they would with tokio_rustls.)
        read_rx.await.unwrap();
        drop(tls);
        let error = server.await.unwrap();
        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
        assert_eq!(error.to_string(), UNEXPECTED_EOF_MESSAGE);
    });
}
