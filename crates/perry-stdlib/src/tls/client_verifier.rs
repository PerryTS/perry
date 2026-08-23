//! Node-compatible rustls client-certificate verification.

use super::*;
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};

#[derive(Debug)]
pub(super) struct NodeConfiguredClientVerifier {
    pub(super) inner: Arc<dyn ClientCertVerifier>,
    pub(super) configured: Vec<Vec<u8>>,
}

fn is_ca_used_as_end_entity(error: &rustls::Error) -> bool {
    let rustls::Error::InvalidCertificate(rustls::CertificateError::Other(other)) = error else {
        return false;
    };
    other.0.to_string() == "CaUsedAsEndEntity"
}

impl ClientCertVerifier for NodeConfiguredClientVerifier {
    fn offer_client_auth(&self) -> bool {
        self.inner.offer_client_auth()
    }

    fn client_auth_mandatory(&self) -> bool {
        self.inner.client_auth_mandatory()
    }

    fn root_hint_subjects(&self) -> &[rustls::DistinguishedName] {
        self.inner.root_hint_subjects()
    }

    fn verify_client_cert(
        &self,
        end_entity: &rustls::pki_types::CertificateDer<'_>,
        intermediates: &[rustls::pki_types::CertificateDer<'_>],
        now: rustls::pki_types::UnixTime,
    ) -> Result<ClientCertVerified, rustls::Error> {
        match self
            .inner
            .verify_client_cert(end_entity, intermediates, now)
        {
            Err(error)
                if is_ca_used_as_end_entity(&error)
                    && self
                        .configured
                        .iter()
                        .any(|cert| cert.as_slice() == end_entity.as_ref()) =>
            {
                Ok(ClientCertVerified::assertion())
            }
            result => result,
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}
