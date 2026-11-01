//! A client configuration that also accepts a server whose certificate is one
//! of the configured extra CAs.
//!
//! Node accepts a self-signed certificate that carries `basicConstraints
//! CA:TRUE` as a server's own certificate when that exact certificate is
//! trusted. webpki rejects the shape as `CaUsedAsEndEntity`; this verifier
//! recovers from that one error.

use std::sync::Arc;

use turnloop_tls::rustls::{
    self,
    client::{
        danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
        WebPkiServerVerifier,
    },
    pki_types::{pem::PemObject, CertificateDer, ServerName, UnixTime},
    server::ParsedCertificate,
    DigitallySignedStruct, Error, RootCertStore, SignatureScheme,
};

#[derive(Debug)]
struct CaLeafVerifier {
    inner: Arc<WebPkiServerVerifier>,
    anchors: Vec<CertificateDer<'static>>,
}

fn is_ca_used_as_end_entity(error: &Error) -> bool {
    let Error::InvalidCertificate(rustls::CertificateError::Other(other)) = error else {
        return false;
    };
    other.0.to_string() == "CaUsedAsEndEntity"
}

impl ServerCertVerifier for CaLeafVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        match self.inner.verify_server_cert(
            end_entity,
            intermediates,
            server_name,
            ocsp_response,
            now,
        ) {
            // webpki checks the leaf's validity period before its basic
            // constraints, so this error means the period already passed.
            Err(error)
                if is_ca_used_as_end_entity(&error)
                    && self
                        .anchors
                        .iter()
                        .any(|anchor| anchor.as_ref() == end_entity.as_ref()) =>
            {
                let parsed = ParsedCertificate::try_from(end_entity)?;
                rustls::client::verify_server_name(&parsed, server_name)?;
                Ok(ServerCertVerified::assertion())
            }
            result => result,
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        self.inner.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        self.inner.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}

/// [`turnloop_tls::ClientConfig::new`] for a verifying configuration, with the
/// PEM certificates in `options.extra_ca_pem` also accepted as a server's own
/// certificate. Without any, it is `ClientConfig::new` unchanged.
pub(super) fn client_config(
    options: turnloop_tls::ClientOptions,
    unix_seconds: u64,
) -> Result<turnloop_tls::ClientConfig, Error> {
    let anchors = CertificateDer::pem_slice_iter(&options.extra_ca_pem)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| Error::General(error.to_string()))?;
    if anchors.is_empty() || options.ca.is_some() {
        return turnloop_tls::ClientConfig::new(options, unix_seconds);
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let time = turnloop_tls::HostTime::new(unix_seconds);
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    for anchor in &anchors {
        roots.add(anchor.clone())?;
    }
    let inner = WebPkiServerVerifier::builder_with_provider(Arc::new(roots), provider.clone())
        .build()
        .map_err(|error| Error::General(error.to_string()))?;
    let mut config = rustls::ClientConfig::builder_with_details(provider, time.time_provider())
        .with_safe_default_protocol_versions()?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(CaLeafVerifier { inner, anchors }))
        .with_no_client_auth();
    config.alpn_protocols = options.alpn;
    config.enable_sni = options.enable_sni;
    Ok(turnloop_tls::ClientConfig::from_rustls(
        Arc::new(config),
        time,
    ))
}
