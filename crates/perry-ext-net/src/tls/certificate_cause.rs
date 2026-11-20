//! Preserve rejection while attaching a precise Node certificate cause.

use std::sync::Arc;
use x509_cert::der::{asn1::AnyRef, Decode, Encode};

struct SelfSignedCa {
    original: rustls::OtherError,
}

// Keep the original diagnostic text; only the exposed cause code changes.
impl std::fmt::Debug for SelfSignedCa {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self.original.0.as_ref(), f)
    }
}

impl std::fmt::Display for SelfSignedCa {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self.original.0.as_ref(), f)
    }
}

impl std::error::Error for SelfSignedCa {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.original.0.as_ref())
    }
}

pub(super) fn classify(error: rustls::Error, certificate: &[u8]) -> rustls::Error {
    if !super::is_ca_used_as_end_entity(&error)
        || !verified_self_signature(certificate).unwrap_or(false)
    {
        return error;
    }
    match error {
        rustls::Error::InvalidCertificate(rustls::CertificateError::Other(original)) => {
            rustls::Error::InvalidCertificate(rustls::CertificateError::Other(rustls::OtherError(
                Arc::new(SelfSignedCa { original }),
            )))
        }
        other => other,
    }
}

pub(crate) fn node_cause_code(error: &rustls::Error) -> Option<&'static str> {
    match error {
        rustls::Error::InvalidCertificate(rustls::CertificateError::Other(other))
            if other.0.downcast_ref::<SelfSignedCa>().is_some() =>
        {
            Some("DEPTH_ZERO_SELF_SIGNED_CERT")
        }
        _ => None,
    }
}

fn verified_self_signature(der: &[u8]) -> Option<bool> {
    let certificate = x509_cert::Certificate::from_der(der).ok()?;
    let tbs = certificate.tbs_certificate();
    if tbs.issuer() != tbs.subject() || tbs.signature() != certificate.signature_algorithm() {
        return Some(false);
    }
    let signed = tbs.to_der().ok()?;
    let public_key = tbs
        .subject_public_key_info()
        .subject_public_key
        .as_bytes()?;
    let signature = certificate.signature().as_bytes()?;
    let key_algorithm = tbs.subject_public_key_info().algorithm.to_der().ok()?;
    let signature_algorithm = certificate.signature_algorithm().to_der().ok()?;
    // rustls algorithm identifiers contain the SEQUENCE contents, not its TLV.
    let key_algorithm = AnyRef::from_der(&key_algorithm).ok()?;
    let signature_algorithm = AnyRef::from_der(&signature_algorithm).ok()?;
    let provider = rustls::crypto::ring::default_provider();
    Some(
        provider
            .signature_verification_algorithms
            .all
            .iter()
            .any(|algorithm| {
                algorithm.public_key_alg_id().as_ref() == key_algorithm.value()
                    && algorithm.signature_alg_id().as_ref() == signature_algorithm.value()
                    && algorithm
                        .verify_signature(public_key, &signed, signature)
                        .is_ok()
            }),
    )
}
