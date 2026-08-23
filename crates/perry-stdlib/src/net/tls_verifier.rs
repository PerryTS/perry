//! Node-compatible rustls server-certificate verification.

use super::*;

#[cfg(feature = "tls")]
#[derive(Debug)]
pub(super) struct NodeConfiguredCaVerifier {
    pub(super) inner: Arc<rustls::client::WebPkiServerVerifier>,
    pub(super) roots: rustls::RootCertStore,
    pub(super) configured: Vec<Vec<u8>>,
    pub(super) custom_identity: bool,
}

#[cfg(feature = "tls")]
fn is_ca_used_as_end_entity(error: &rustls::Error) -> bool {
    let rustls::Error::InvalidCertificate(rustls::CertificateError::Other(other)) = error else {
        return false;
    };
    other.0.to_string() == "CaUsedAsEndEntity"
}

#[cfg(feature = "tls")]
impl ServerCertVerifier for NodeConfiguredCaVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &rustls::pki_types::CertificateDer<'_>,
        intermediates: &[rustls::pki_types::CertificateDer<'_>],
        server_name: &rustls::pki_types::ServerName<'_>,
        ocsp_response: &[u8],
        now: rustls::pki_types::UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        if self.custom_identity {
            let parsed = rustls::server::ParsedCertificate::try_from(end_entity)?;
            let provider = rustls::crypto::aws_lc_rs::default_provider();
            match rustls::client::verify_server_cert_signed_by_trust_anchor(
                &parsed,
                &self.roots,
                intermediates,
                now,
                provider.signature_verification_algorithms.all,
            ) {
                Ok(()) => return Ok(ServerCertVerified::assertion()),
                Err(error)
                    if is_ca_used_as_end_entity(&error)
                        && self
                            .configured
                            .iter()
                            .any(|cert| cert.as_slice() == end_entity.as_ref()) =>
                {
                    return Ok(ServerCertVerified::assertion());
                }
                Err(error) => return Err(error),
            }
        }
        match self.inner.verify_server_cert(
            end_entity,
            intermediates,
            server_name,
            ocsp_response,
            now,
        ) {
            Err(error)
                if is_ca_used_as_end_entity(&error)
                    && self
                        .configured
                        .iter()
                        .any(|cert| cert.as_slice() == end_entity.as_ref()) =>
            {
                let parsed = rustls::server::ParsedCertificate::try_from(end_entity)?;
                rustls::client::verify_server_name(&parsed, server_name)?;
                Ok(ServerCertVerified::assertion())
            }
            result => result,
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}
