//! rustls client config + handshake for `tls.connect` and the
//! `socket.upgradeToTLS` mid-stream upgrade. Split out of `lib.rs` (#1852)
//! to keep that file under the 2000-line gate; the logic is unchanged.

use std::sync::Arc;

use tokio::net::TcpStream;
use tokio_rustls::{client::TlsStream, rustls, TlsConnector};

fn build_tls_connector(verify: bool) -> Result<TlsConnector, String> {
    if !verify {
        return build_tls_connector_insecure();
    }
    let mut root_store = rustls::RootCertStore::empty();
    let native = rustls_native_certs::load_native_certs();
    for cert in native.certs {
        let _ = root_store.add(cert);
    }
    let config = rustls::ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();
    Ok(TlsConnector::from(Arc::new(config)))
}

fn build_tls_connector_insecure() -> Result<TlsConnector, String> {
    use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
    use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
    use rustls::{DigitallySignedStruct, SignatureScheme};

    #[derive(Debug)]
    struct NoVerify;

    impl ServerCertVerifier for NoVerify {
        fn verify_server_cert(
            &self,
            _end_entity: &CertificateDer<'_>,
            _intermediates: &[CertificateDer<'_>],
            _server_name: &ServerName<'_>,
            _ocsp: &[u8],
            _now: UnixTime,
        ) -> Result<ServerCertVerified, rustls::Error> {
            Ok(ServerCertVerified::assertion())
        }
        fn verify_tls12_signature(
            &self,
            _message: &[u8],
            _cert: &CertificateDer<'_>,
            _dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            Ok(HandshakeSignatureValid::assertion())
        }
        fn verify_tls13_signature(
            &self,
            _message: &[u8],
            _cert: &CertificateDer<'_>,
            _dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            Ok(HandshakeSignatureValid::assertion())
        }
        fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
            vec![
                SignatureScheme::RSA_PKCS1_SHA256,
                SignatureScheme::RSA_PKCS1_SHA384,
                SignatureScheme::RSA_PKCS1_SHA512,
                SignatureScheme::ECDSA_NISTP256_SHA256,
                SignatureScheme::ECDSA_NISTP384_SHA384,
                SignatureScheme::RSA_PSS_SHA256,
                SignatureScheme::RSA_PSS_SHA384,
                SignatureScheme::RSA_PSS_SHA512,
                SignatureScheme::ED25519,
            ]
        }
    }

    let config = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(NoVerify))
        .with_no_client_auth();
    Ok(TlsConnector::from(Arc::new(config)))
}

pub(crate) async fn do_tls_handshake(
    tcp: TcpStream,
    servername: &str,
    verify: bool,
) -> Result<TlsStream<TcpStream>, String> {
    let connector = build_tls_connector(verify)?;
    let server_name = rustls::pki_types::ServerName::try_from(servername.to_string())
        .map_err(|e| format!("invalid servername '{}': {}", servername, e))?;
    connector
        .connect(server_name, tcp)
        .await
        .map_err(|e| format!("tls handshake: {}", e))
}
