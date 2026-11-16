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

/// The default roots plus `anchors`, each anchor also accepted as a server's
/// own certificate.
fn ca_leaf_verifier(
    anchors: Vec<CertificateDer<'static>>,
    provider: Arc<rustls::crypto::CryptoProvider>,
) -> Result<CaLeafVerifier, Error> {
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    for anchor in &anchors {
        roots.add(anchor.clone())?;
    }
    let inner = WebPkiServerVerifier::builder_with_provider(Arc::new(roots), provider)
        .build()
        .map_err(|error| Error::General(error.to_string()))?;
    Ok(CaLeafVerifier { inner, anchors })
}

/// [`turnloop_tls::ClientConfig::new`], with the PEM certificates in
/// `options.extra_ca_pem` also accepted as a server's own certificate. An
/// unverified configuration, an explicit `ca`, or no extra CA is
/// `ClientConfig::new` unchanged.
pub(super) fn client_config(
    options: turnloop_tls::ClientOptions,
    unix_seconds: u64,
) -> Result<turnloop_tls::ClientConfig, Error> {
    let anchors = CertificateDer::pem_slice_iter(&options.extra_ca_pem)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| Error::General(error.to_string()))?;
    if anchors.is_empty() || options.ca.is_some() || !options.reject_unauthorized {
        return turnloop_tls::ClientConfig::new(options, unix_seconds);
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let time = turnloop_tls::HostTime::new(unix_seconds);
    let verifier = ca_leaf_verifier(anchors, provider.clone())?;
    let mut config = rustls::ClientConfig::builder_with_details(provider, time.time_provider())
        .with_safe_default_protocol_versions()?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth();
    config.alpn_protocols = options.alpn;
    config.enable_sni = options.enable_sni;
    Ok(turnloop_tls::ClientConfig::from_rustls(
        Arc::new(config),
        time,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    // Self-signed, `CA:TRUE`, valid 2026-10-31 to 2126-10-07, SAN IP:127.0.0.1.
    const GOOD_CERT: &str = "-----BEGIN CERTIFICATE-----
MIIBwTCCAWigAwIBAgIUOeoespEaUQnK0thx1cyiTGYymuEwCgYIKoZIzj0EAwIw
JTEjMCEGA1UEAwwacGVycnktZ2FwLXNlbGYtc2lnbmVkLWdvb2QwIBcNMjYxMDMx
MDIxMTI0WhgPMjEyNjEwMDcwMjExMjRaMCUxIzAhBgNVBAMMGnBlcnJ5LWdhcC1z
ZWxmLXNpZ25lZC1nb29kMFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAE23vbEhdG
4yoex3zo5W6tUVMJhboCFlLpNWbK7YdTA1CNJUvu/3rmqFJFN5THOR+hb0NbQvtq
7eUyEQyy6bsi36N0MHIwHQYDVR0OBBYEFJwsiYTHG1xeyW8hKm5SL1O4iiUSMB8G
A1UdIwQYMBaAFJwsiYTHG1xeyW8hKm5SL1O4iiUSMA8GA1UdEwEB/wQFMAMBAf8w
DwYDVR0RBAgwBocEfwAAATAOBgNVHQ8BAf8EBAMCAoQwCgYIKoZIzj0EAwIDRwAw
RAIgJAlBPF3ZwD1I1Ifmiy8dkK8zALjS2eGb0SGV6ROH014CIEIIW+WjxUkm5lQR
4Em1FdZhoJYj89USDXxX2MJXHM5E
-----END CERTIFICATE-----
";
    // The same shape with SAN DNS:example.invalid.
    const OTHER_CERT: &str = "-----BEGIN CERTIFICATE-----
MIIBzDCCAXGgAwIBAgIUfLWVpQ1aEAvcuubFzDrr0wPdMwAwCgYIKoZIzj0EAwIw
JDEiMCAGA1UEAwwZcGVycnktZ2FwLXNlbGYtc2lnbmVkLWJhZDAgFw0yNjEwMzEw
MjExMjRaGA8yMTI2MTAwNzAyMTEyNFowJDEiMCAGA1UEAwwZcGVycnktZ2FwLXNl
bGYtc2lnbmVkLWJhZDBZMBMGByqGSM49AgEGCCqGSM49AwEHA0IABBKCKtqiChiB
/dWCeiT31fyZYfhV4cx0C0UjVe8wt8+pOQreikwMmtRygnLFeILGmFqiFi+xKkaE
ng3nR/h1GY6jfzB9MB0GA1UdDgQWBBRjp0c55Zs5W3e/q4OEt3FQbooaKzAfBgNV
HSMEGDAWgBRjp0c55Zs5W3e/q4OEt3FQbooaKzAPBgNVHRMBAf8EBTADAQH/MBoG
A1UdEQQTMBGCD2V4YW1wbGUuaW52YWxpZDAOBgNVHQ8BAf8EBAMCAoQwCgYIKoZI
zj0EAwIDSQAwRgIhALLFBXpUSLcwg1VuLeZWkO4fnONi1MvGr7A29Zeu2JieAiEA
kNf6ZqL0Nnj0/lsfWpm7n0iAr9NWaQhrAq02F22DCCk=
-----END CERTIFICATE-----
";
    const IN_VALIDITY: u64 = 1_900_000_000;
    const BEFORE_VALIDITY: u64 = 1_700_000_000;

    fn der(pem: &str) -> CertificateDer<'static> {
        CertificateDer::from_pem_slice(pem.as_bytes()).unwrap()
    }

    fn verify(anchor: &str, leaf: &str, host: &str, unix_seconds: u64) -> Result<(), Error> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let verifier = ca_leaf_verifier(vec![der(anchor)], provider)?;
        let name = ServerName::try_from(host.to_string()).unwrap();
        let now = UnixTime::since_unix_epoch(Duration::from_secs(unix_seconds));
        verifier
            .verify_server_cert(&der(leaf), &[], &name, &[], now)
            .map(|_| ())
    }

    #[test]
    fn trusted_ca_certificate_is_accepted_as_the_server_certificate() {
        verify(GOOD_CERT, GOOD_CERT, "127.0.0.1", IN_VALIDITY).unwrap();
    }

    #[test]
    fn trusted_ca_leaf_still_checks_the_server_name() {
        assert!(verify(GOOD_CERT, GOOD_CERT, "localhost", IN_VALIDITY).is_err());
        assert!(verify(OTHER_CERT, OTHER_CERT, "127.0.0.1", IN_VALIDITY).is_err());
    }

    #[test]
    fn a_different_trusted_ca_does_not_trust_the_leaf() {
        assert!(verify(OTHER_CERT, GOOD_CERT, "127.0.0.1", IN_VALIDITY).is_err());
    }

    #[test]
    fn trusted_ca_leaf_outside_its_validity_period_is_rejected() {
        assert!(verify(GOOD_CERT, GOOD_CERT, "127.0.0.1", BEFORE_VALIDITY).is_err());
    }
}
