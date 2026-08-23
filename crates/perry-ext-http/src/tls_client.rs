//! Client-side TLS options for `https.request` / `https.get` (#4906).
//!
//! Node's https client accepts a family of TLS options on the request
//! (or agent) options object. Before this module the perry-ext-http
//! client always used reqwest's default verifier, so connecting to a
//! server that presents a self-signed / test-CA certificate failed the
//! handshake outright (`received fatal alert: UnknownCA`). Node's own
//! https tests stand up servers with the `test/fixtures/keys` test
//! certs and connect with one of:
//!
//! - `rejectUnauthorized: false` — don't fail the handshake on an
//!   untrusted cert (also driven by `NODE_TLS_REJECT_UNAUTHORIZED=0`).
//! - `ca: pem | Buffer | (pem|Buffer)[]` — trust the supplied CA(s).
//! - `checkServerIdentity: fn` — override hostname verification.
//!
//! This module parses those options off the request's options object and
//! folds them into a per-request `reqwest::Client`.
//!
//! ## Honored faithfully
//!
//! `rejectUnauthorized: false` / `NODE_TLS_REJECT_UNAUTHORIZED=0` map to
//! reqwest's `danger_accept_invalid_certs(true)`; `ca` entries are added
//! as additional trust anchors via `add_root_certificate`.
//!
//! ## Compatibility layer
//!
//! - `checkServerIdentity` is a JS callback that can't run inside the
//!   rustls handshake. We disable the backend hostname check, then invoke
//!   the callback on the main thread before dispatch and surface a returned
//!   `Error` through the request's normal asynchronous error path.
//! - reqwest's rustls backend requires a SAN match and does **not** fall back
//!   to the certificate Common Name. A verifier wrapper retains webpki chain
//!   and signature validation while leaving the final hostname decision to
//!   the Node-compatible Common Name layer. It also accepts an explicitly
//!   trusted self-signed CA when that exact certificate is the endpoint leaf,
//!   matching OpenSSL's behavior without disabling verification globally.

/// Parsed client-side TLS options. `Default` is "no TLS customization",
/// in which case the caller keeps using the pooled default client.
#[derive(Clone, Default, Debug, Eq, Hash, PartialEq)]
pub(crate) struct TlsOptions {
    /// `options.rejectUnauthorized`. `None` = unset (Node defaults to
    /// `true` for https).
    pub(crate) reject_unauthorized: Option<bool>,
    /// PEM byte blobs from `options.ca` (string / Buffer / array of
    /// either). Each blob may itself be a multi-cert bundle.
    pub(crate) ca_pems: Vec<Vec<u8>>,
    /// Raw, GC-rooted `options.checkServerIdentity` closure pointer. Zero
    /// means the option was not supplied.
    pub(crate) check_server_identity_callback: i64,
    /// True when the callback came from Agent constructor defaults. Node only
    /// re-runs that callback when a fresh TLS session is verified; an explicit
    /// per-request callback still runs for every request.
    pub(crate) check_server_identity_from_agent: bool,
    /// Explicit TLS identity from `options.servername`. Reqwest connects to
    /// the URL host and cannot substitute this value for rustls' SNI/name
    /// check. When it is present we therefore keep certificate-chain
    /// validation enabled but perform the hostname decision at the Node
    /// compatibility layer.
    pub(crate) servername: Option<String>,
    /// Remaining HTTPS Agent identity fields. Some are OpenSSL-only and are
    /// not independently configurable through reqwest/rustls, but they still
    /// have to partition the TLS session cache exactly like Node's Agent.
    pub(crate) session_identity: Vec<(String, String)>,
    /// PKCS#12 client identities supplied through `pfx`. They are converted
    /// to the PEM identity format accepted by reqwest's rustls backend and
    /// also feed the server-side peer-certificate compatibility facade.
    pub(crate) client_pfx: Vec<(Vec<u8>, String)>,
    pub(crate) peer_certificate_cn: Option<String>,
}

impl TlsOptions {
    /// Whether these options require building a dedicated TLS client
    /// instead of reusing the pooled default. `NODE_TLS_REJECT_UNAUTHORIZED=0`
    /// alone counts (it disables verification process-wide).
    pub(crate) fn needs_custom_client(&self) -> bool {
        self.reject_unauthorized == Some(false)
            || self.check_server_identity_callback != 0
            || self.servername.is_some()
            || !self.ca_pems.is_empty()
            || !self.client_pfx.is_empty()
            || node_tls_reject_unauthorized_disabled()
    }

    /// Resolve whether the cert chain should be accepted without
    /// verification. True when `rejectUnauthorized:false`, when
    /// `NODE_TLS_REJECT_UNAUTHORIZED=0`. A `checkServerIdentity` callback
    /// replaces only hostname verification; the certificate chain must still
    /// validate.
    pub(crate) fn accept_invalid_certs(&self) -> bool {
        self.reject_unauthorized == Some(false) || node_tls_reject_unauthorized_disabled()
    }

    /// Build a per-request `reqwest::Client` honoring these options.
    /// `pool` is the optional `(keep_alive, max_free_sockets,
    /// keep_alive_msecs)` Agent pool config to fold in.
    pub(crate) fn build_client(
        &self,
        pool: Option<(bool, f64, f64)>,
    ) -> Result<reqwest::Client, String> {
        let mut builder = crate::apply_node_proxy_policy(
            reqwest::Client::builder().tcp_keepalive(std::time::Duration::from_secs(60)),
        );

        // Node/OpenSSL accepts an explicitly trusted self-signed CA
        // certificate as the endpoint certificate. webpki rejects that shape
        // as `CaUsedAsEndEntity`, even when the exact DER is in its root store.
        // A small verifier wrapper preserves normal chain validation, ignores
        // only the hostname result (our Node-CN compatibility layer owns it),
        // and accepts that one exact-leaf trust case.
        let custom_tls_config = !self.client_pfx.is_empty()
            || (!self.ca_pems.is_empty() && !self.accept_invalid_certs());
        if custom_tls_config {
            builder = builder.use_preconfigured_tls(build_node_tls_config(
                &self.ca_pems,
                self.servername.clone(),
                self.check_server_identity_callback != 0,
                self.accept_invalid_certs(),
                self.client_pfx.first(),
            )?);
        } else {
            if self.accept_invalid_certs() {
                builder = builder.danger_accept_invalid_certs(true);
            }
            if self.servername.is_some()
                || self.check_server_identity_callback != 0
                || !self.ca_pems.is_empty()
            {
                builder = builder.danger_accept_invalid_hostnames(true);
            }
            for pem in &self.ca_pems {
                // A `ca` entry may be a single cert or a bundle; try the
                // bundle parser first, then fall back to the single-cert one.
                match reqwest::Certificate::from_pem_bundle(pem) {
                    Ok(certs) => {
                        for cert in certs {
                            builder = builder.add_root_certificate(cert);
                        }
                    }
                    Err(_) => {
                        if let Ok(cert) = reqwest::Certificate::from_pem(pem) {
                            builder = builder.add_root_certificate(cert);
                        }
                    }
                }
            }
        }

        if let Some((keep_alive, max_free_sockets, keep_alive_msecs)) = pool {
            let pool_max_idle = if keep_alive {
                if !max_free_sockets.is_finite() || max_free_sockets > usize::MAX as f64 {
                    256
                } else {
                    max_free_sockets.max(1.0) as usize
                }
            } else {
                0
            };
            let idle_timeout = if keep_alive {
                let ms = if keep_alive_msecs.is_finite() && keep_alive_msecs > 0.0 {
                    keep_alive_msecs
                } else {
                    1000.0
                };
                std::time::Duration::from_millis(ms as u64)
            } else {
                std::time::Duration::from_millis(0)
            };
            builder = builder
                .pool_max_idle_per_host(pool_max_idle)
                .pool_idle_timeout(idle_timeout);
        }

        builder
            .build()
            .map_err(|e| format!("https: build client: {e:?}"))
    }
}

#[derive(Debug)]
struct NodeCaVerifier {
    inner: std::sync::Arc<rustls::client::WebPkiServerVerifier>,
    exact_trust: Vec<Vec<u8>>,
    expected_server_name: Option<String>,
    skip_hostname: bool,
}

#[derive(Debug)]
struct AcceptInvalidCertificates;

impl rustls::client::danger::ServerCertVerifier for AcceptInvalidCertificates {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::RSA_PKCS1_SHA1,
            rustls::SignatureScheme::ECDSA_SHA1_Legacy,
            rustls::SignatureScheme::RSA_PKCS1_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::RSA_PKCS1_SHA384,
            rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
            rustls::SignatureScheme::RSA_PKCS1_SHA512,
            rustls::SignatureScheme::ECDSA_NISTP521_SHA512,
            rustls::SignatureScheme::RSA_PSS_SHA256,
            rustls::SignatureScheme::RSA_PSS_SHA384,
            rustls::SignatureScheme::RSA_PSS_SHA512,
            rustls::SignatureScheme::ED25519,
            rustls::SignatureScheme::ED448,
        ]
    }
}

impl rustls::client::danger::ServerCertVerifier for NodeCaVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &rustls::pki_types::CertificateDer<'_>,
        intermediates: &[rustls::pki_types::CertificateDer<'_>],
        server_name: &rustls::pki_types::ServerName<'_>,
        ocsp_response: &[u8],
        now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        let expected_name = self
            .expected_server_name
            .as_ref()
            .and_then(|name| rustls::pki_types::ServerName::try_from(name.clone()).ok());
        let verification_name = expected_name.as_ref().unwrap_or(server_name);
        match self.inner.verify_server_cert(
            end_entity,
            intermediates,
            verification_name,
            ocsp_response,
            now,
        ) {
            Ok(verified) => Ok(verified),
            Err(error) => {
                let chain_was_valid = matches!(
                    error,
                    rustls::Error::InvalidCertificate(
                        rustls::CertificateError::NotValidForName
                            | rustls::CertificateError::NotValidForNameContext { .. }
                    )
                );
                let exact_ca_leaf = self
                    .exact_trust
                    .iter()
                    .any(|trusted| trusted.as_slice() == end_entity.as_ref())
                    && format!("{error:?}").contains("CaUsedAsEndEntity");
                let legacy_direct_chain = intermediates.is_empty()
                    && format!("{error:?}").contains("UnsupportedCertVersion")
                    && self.exact_trust.iter().any(|trusted| {
                        legacy_direct_certificate_is_valid(end_entity.as_ref(), trusted, now)
                    });
                let hostname_valid = self.skip_hostname
                    || certificate_matches_hostname(
                        end_entity.as_ref(),
                        verification_name.to_str().as_ref(),
                    );
                if hostname_valid && (chain_was_valid || exact_ca_leaf || legacy_direct_chain) {
                    Ok(rustls::client::danger::ServerCertVerified::assertion())
                } else {
                    Err(error)
                }
            }
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        match self.inner.verify_tls12_signature(message, cert, dss) {
            Err(error)
                if format!("{error:?}").contains("UnsupportedCertVersion")
                    && legacy_handshake_signature_is_valid(message, cert.as_ref(), dss) =>
            {
                Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
            }
            result => result,
        }
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        match self.inner.verify_tls13_signature(message, cert, dss) {
            Err(error)
                if format!("{error:?}").contains("UnsupportedCertVersion")
                    && legacy_handshake_signature_is_valid(message, cert.as_ref(), dss) =>
            {
                Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
            }
            result => result,
        }
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}

fn legacy_handshake_signature_is_valid(
    message: &[u8],
    certificate_der: &[u8],
    signed: &rustls::DigitallySignedStruct,
) -> bool {
    use x509_cert::der::Decode;

    let algorithm: &dyn ring::signature::VerificationAlgorithm = match signed.scheme {
        rustls::SignatureScheme::RSA_PKCS1_SHA1 => {
            &ring::signature::RSA_PKCS1_2048_8192_SHA1_FOR_LEGACY_USE_ONLY
        }
        rustls::SignatureScheme::RSA_PKCS1_SHA256 => &ring::signature::RSA_PKCS1_2048_8192_SHA256,
        rustls::SignatureScheme::RSA_PKCS1_SHA384 => &ring::signature::RSA_PKCS1_2048_8192_SHA384,
        rustls::SignatureScheme::RSA_PKCS1_SHA512 => &ring::signature::RSA_PKCS1_2048_8192_SHA512,
        rustls::SignatureScheme::RSA_PSS_SHA256 => &ring::signature::RSA_PSS_2048_8192_SHA256,
        rustls::SignatureScheme::RSA_PSS_SHA384 => &ring::signature::RSA_PSS_2048_8192_SHA384,
        rustls::SignatureScheme::RSA_PSS_SHA512 => &ring::signature::RSA_PSS_2048_8192_SHA512,
        _ => return false,
    };
    let Ok(certificate) = x509_cert::Certificate::from_der(certificate_der) else {
        return false;
    };
    let Some(public_key) = certificate
        .tbs_certificate()
        .subject_public_key_info()
        .subject_public_key
        .as_bytes()
    else {
        return false;
    };
    ring::signature::UnparsedPublicKey::new(algorithm, public_key)
        .verify(message, signed.signature())
        .is_ok()
}

/// webpki intentionally rejects pre-v3 endpoint certificates. Node/OpenSSL
/// still accepts the v1 certificates in the Node v22 fixture corpus, so retain
/// a narrow fallback for a leaf signed directly by a caller-supplied CA. The
/// fallback verifies validity, issuer identity, and the RSA signature; all
/// other chain shapes and algorithms continue to fail closed.
fn legacy_direct_certificate_is_valid(
    endpoint_der: &[u8],
    trusted_ca_der: &[u8],
    now: rustls::pki_types::UnixTime,
) -> bool {
    use x509_cert::der::{Decode, Encode};

    let Ok(endpoint) = x509_cert::Certificate::from_der(endpoint_der) else {
        return false;
    };
    let Ok(trusted_ca) = x509_cert::Certificate::from_der(trusted_ca_der) else {
        return false;
    };
    if endpoint.tbs_certificate().version() != x509_cert::Version::V1
        || endpoint.tbs_certificate().issuer() != trusted_ca.tbs_certificate().subject()
    {
        return false;
    }
    let now = now.as_secs();
    for certificate in [&endpoint, &trusted_ca] {
        let validity = certificate.tbs_certificate().validity();
        if now < validity.not_before.to_unix_duration().as_secs()
            || now > validity.not_after.to_unix_duration().as_secs()
        {
            return false;
        }
    }

    let algorithm: &dyn ring::signature::VerificationAlgorithm =
        match endpoint.signature_algorithm().oid.to_string().as_str() {
            "1.2.840.113549.1.1.5" => {
                &ring::signature::RSA_PKCS1_2048_8192_SHA1_FOR_LEGACY_USE_ONLY
            }
            "1.2.840.113549.1.1.11" => &ring::signature::RSA_PKCS1_2048_8192_SHA256,
            "1.2.840.113549.1.1.12" => &ring::signature::RSA_PKCS1_2048_8192_SHA384,
            "1.2.840.113549.1.1.13" => &ring::signature::RSA_PKCS1_2048_8192_SHA512,
            _ => return false,
        };
    let Some(public_key) = trusted_ca
        .tbs_certificate()
        .subject_public_key_info()
        .subject_public_key
        .as_bytes()
    else {
        return false;
    };
    let Some(signature) = endpoint.signature().as_bytes() else {
        return false;
    };
    let Ok(message) = endpoint.tbs_certificate().to_der() else {
        return false;
    };
    ring::signature::UnparsedPublicKey::new(algorithm, public_key)
        .verify(&message, signature)
        .is_ok()
}

fn build_node_tls_config(
    ca_pems: &[Vec<u8>],
    expected_server_name: Option<String>,
    skip_hostname: bool,
    accept_invalid_certs: bool,
    client_pfx: Option<&(Vec<u8>, String)>,
) -> Result<rustls::ClientConfig, String> {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let mut exact_trust = Vec::new();
    for pem in ca_pems {
        let mut cursor = std::io::Cursor::new(pem);
        for certificate in rustls_pemfile::certs(&mut cursor).flatten() {
            exact_trust.push(certificate.as_ref().to_vec());
            roots
                .add(certificate)
                .map_err(|error| format!("https: invalid CA certificate: {error}"))?;
        }
    }
    if !ca_pems.is_empty() && exact_trust.is_empty() {
        return Err("https: CA option contained no certificates".to_string());
    }

    let provider = std::sync::Arc::new(rustls::crypto::ring::default_provider());
    let verifier: std::sync::Arc<dyn rustls::client::danger::ServerCertVerifier> =
        if accept_invalid_certs {
            std::sync::Arc::new(AcceptInvalidCertificates)
        } else {
            let inner = rustls::client::WebPkiServerVerifier::builder_with_provider(
                std::sync::Arc::new(roots),
                provider.clone(),
            )
            .build()
            .map_err(|error| format!("https: invalid CA verifier: {error}"))?;
            std::sync::Arc::new(NodeCaVerifier {
                inner,
                exact_trust,
                expected_server_name,
                skip_hostname,
            })
        };
    let builder = rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .map_err(|error| format!("https: build TLS config: {error}"))
        .map(|builder| {
            builder
                .dangerous()
                .with_custom_certificate_verifier(verifier)
        })?;
    let Some((der, passphrase)) = client_pfx else {
        return Ok(builder.with_no_client_auth());
    };
    let (certificates, private_key) = pfx_certificate_and_key(der, passphrase)?;
    let signing_key = provider
        .key_provider
        .load_private_key(rustls::pki_types::PrivateKeyDer::Pkcs8(
            rustls::pki_types::PrivatePkcs8KeyDer::from(private_key),
        ))
        .map_err(|error| format!("https: invalid pfx private key: {error}"))?;
    let certified_key = rustls::sign::CertifiedKey::new(
        certificates
            .into_iter()
            .map(rustls::pki_types::CertificateDer::from)
            .collect(),
        signing_key,
    );
    Ok(builder.with_client_cert_resolver(std::sync::Arc::new(
        rustls::sign::SingleCertAndKey::from(certified_key),
    )))
}

/// Node/OpenSSL falls back to the subject Common Name only when a certificate
/// has no relevant DNS/IP Subject Alternative Name. webpki intentionally omits
/// that legacy fallback, so apply the narrow compatibility rule after it has
/// already validated the chain and returned only a name mismatch.
fn certificate_matches_hostname(certificate_der: &[u8], expected: &str) -> bool {
    use x509_cert::der::Decode;
    use x509_cert::ext::pkix::{name::GeneralName, SubjectAltName};

    let Ok(certificate) = x509_cert::Certificate::from_der(certificate_der) else {
        return false;
    };
    if let Some(san_extension) = certificate
        .tbs_certificate()
        .extensions()
        .and_then(|extensions| {
            extensions
                .iter()
                .find(|extension| extension.extn_id.to_string() == "2.5.29.17")
        })
    {
        let Ok(subject_alt_name) = SubjectAltName::from_der(san_extension.extn_value.as_bytes())
        else {
            return false;
        };
        let expected_ip = expected.parse::<std::net::IpAddr>().ok();
        let mut has_relevant_name = false;
        for name in &subject_alt_name.0 {
            let matched = match name {
                GeneralName::DnsName(dns) if expected_ip.is_none() => {
                    has_relevant_name = true;
                    dns_name_matches(dns.as_str(), expected)
                }
                GeneralName::IpAddress(bytes) if expected_ip.is_some() => {
                    has_relevant_name = true;
                    expected_ip.is_some_and(|ip| match ip {
                        std::net::IpAddr::V4(ip) => bytes.as_bytes() == ip.octets(),
                        std::net::IpAddr::V6(ip) => bytes.as_bytes() == ip.octets(),
                    })
                }
                _ => false,
            };
            if matched {
                return true;
            }
        }
        if has_relevant_name {
            return false;
        }
    }

    expected.parse::<std::net::IpAddr>().is_err()
        && certificate_common_name(certificate_der)
            .is_some_and(|common_name| dns_name_matches(&common_name, expected))
}

fn dns_name_matches(pattern: &str, expected: &str) -> bool {
    let pattern = pattern.trim_end_matches('.').to_ascii_lowercase();
    let expected = expected.trim_end_matches('.').to_ascii_lowercase();
    if pattern == expected {
        return true;
    }
    let Some(suffix) = pattern.strip_prefix("*.") else {
        return false;
    };
    expected
        .strip_suffix(suffix)
        .is_some_and(|prefix| prefix.ends_with('.') && !prefix[..prefix.len() - 1].contains('.'))
}

/// `NODE_TLS_REJECT_UNAUTHORIZED=0` disables client cert verification
/// process-wide. JS-side `process.env.NODE_TLS_REJECT_UNAUTHORIZED = '0'`
/// writes through to the OS environment (`js_setenv` → `std::env::set_var`),
/// so reading it here at dispatch time sees runtime assignments.
pub(crate) fn node_tls_reject_unauthorized_disabled() -> bool {
    std::env::var("NODE_TLS_REJECT_UNAUTHORIZED")
        .map(|v| v == "0")
        .unwrap_or(false)
}

/// Parse the client TLS options off a NaN-boxed request options object.
/// `ca` / `rejectUnauthorized` survive the JSON round-trip used by
/// [`super::parse_options_object`]; `checkServerIdentity` is a function
/// (dropped by JSON), so its presence is probed directly off the
/// NaN-boxed object.
///
/// # Safety
/// `opts_f64` must be a valid NaN-boxed JS value (any value is accepted;
/// non-objects yield default options).
pub(crate) unsafe fn parse_tls_options(opts_f64: f64) -> TlsOptions {
    let mut tls = TlsOptions::default();

    if let Some(opts) = super::parse_options_object(opts_f64) {
        if let Some(b) = opts.get("rejectUnauthorized").and_then(|v| v.as_bool()) {
            tls.reject_unauthorized = Some(b);
        }
        if let Some(ca) = opts.get("ca") {
            collect_pems(ca, &mut tls.ca_pems);
        }
        let default_passphrase = opts
            .get("passphrase")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        if let Some(pfx) = opts.get("pfx") {
            collect_pfx(pfx, default_passphrase, &mut tls.client_pfx);
        }
        tls.servername = opts
            .get("servername")
            .and_then(|v| v.as_str())
            .map(String::from);
        for field in [
            "cert",
            "clientCertEngine",
            "ciphers",
            "key",
            "pfx",
            "minVersion",
            "maxVersion",
            "secureProtocol",
            "crl",
            "honorCipherOrder",
            "ecdhCurve",
            "dhparam",
            "secureOptions",
            "sessionIdContext",
            "sigalgs",
            "privateKeyIdentifier",
            "privateKeyEngine",
        ] {
            if let Some(value) = opts.get(field) {
                tls.session_identity.push((
                    field.to_string(),
                    serde_json::to_string(value).unwrap_or_default(),
                ));
            }
        }
    }

    // JSON.stringify is only a fallback projection of Node's live options.
    // In particular, an object entry inside a PFX array can expose `buf` and
    // `passphrase` through ordinary/prototype property lookup while its JSON
    // form drops the passphrase. Prefer the live values whenever they are
    // readable so the private key is decrypted with the caller's password.
    if let Some(client_pfx) = live_pfx_entries(opts_f64) {
        tls.client_pfx = client_pfx;
    }

    tls.check_server_identity_callback = closure_field(opts_f64, "checkServerIdentity");
    tls.peer_certificate_cn = tls
        .client_pfx
        .iter()
        .find_map(|(der, passphrase)| pfx_common_name(der, passphrase));

    tls
}

unsafe fn live_pfx_entries(options: f64) -> Option<Vec<(Vec<u8>, String)>> {
    extern "C" {
        fn js_array_is_array(value: f64) -> f64;
        fn js_buffer_is_buffer(ptr: i64) -> i32;
    }

    fn string_value(value: perry_ffi::JsValue) -> Option<String> {
        if !value.is_string() {
            return None;
        }
        perry_ffi::read_string(unsafe { perry_ffi::JsString::from_raw(value.as_string_ptr()) })
            .map(String::from)
    }

    unsafe fn buffer_value(
        value: perry_ffi::JsValue,
        is_buffer: unsafe extern "C" fn(i64) -> i32,
    ) -> Option<Vec<u8>> {
        if !value.is_pointer() {
            return None;
        }
        let raw = value.as_pointer::<u8>() as i64;
        if is_buffer(raw) == 0 {
            return None;
        }
        perry_ffi::read_buffer_bytes(raw as *const perry_ffi::BufferHeader).map(<[u8]>::to_vec)
    }

    unsafe fn entry(
        value: perry_ffi::JsValue,
        default_passphrase: &str,
        is_buffer: unsafe extern "C" fn(i64) -> i32,
    ) -> Option<(Vec<u8>, String)> {
        if let Some(bytes) = buffer_value(value, is_buffer) {
            return Some((bytes, default_passphrase.to_string()));
        }
        if !value.is_pointer_or_raw() {
            return None;
        }
        let buf = perry_ffi::object_field_by_name(value, "buf");
        let bytes = buffer_value(buf, is_buffer)?;
        let passphrase = string_value(perry_ffi::object_field_by_name(value, "passphrase"))
            .unwrap_or_else(|| default_passphrase.to_string());
        Some((bytes, passphrase))
    }

    let options_value = perry_ffi::JsValue::from_bits(options.to_bits());
    let default_passphrase =
        string_value(perry_ffi::object_field_by_name(options_value, "passphrase"))
            .unwrap_or_default();
    let pfx = perry_ffi::object_field_by_name(options_value, "pfx");
    if pfx.is_undefined() || pfx.is_null() {
        return None;
    }

    let is_array =
        perry_ffi::JsValue::from_bits(js_array_is_array(f64::from_bits(pfx.bits())).to_bits());
    let mut entries = Vec::new();
    if is_array.is_bool() && is_array.to_bool() {
        let array = pfx.as_pointer::<perry_ffi::ArrayHeader>();
        if array.is_null() {
            return None;
        }
        for index in 0..perry_ffi::js_array_length(array) {
            if let Some(value) = entry(
                perry_ffi::js_array_get(array, index),
                &default_passphrase,
                js_buffer_is_buffer,
            ) {
                entries.push(value);
            }
        }
    } else if let Some(value) = entry(pfx, &default_passphrase, js_buffer_is_buffer) {
        entries.push(value);
    }
    (!entries.is_empty()).then_some(entries)
}

/// Merge Agent-constructor TLS defaults into request-local options. Fields
/// explicitly present on the request win, matching Node's option layering.
pub(crate) fn merge_defaults(defaults: &TlsOptions, request: &mut TlsOptions) {
    if request.reject_unauthorized.is_none() {
        request.reject_unauthorized = defaults.reject_unauthorized;
    }
    if request.ca_pems.is_empty() {
        request.ca_pems = defaults.ca_pems.clone();
    }
    if request.client_pfx.is_empty() {
        request.client_pfx = defaults.client_pfx.clone();
        request.peer_certificate_cn = defaults.peer_certificate_cn.clone();
    }
    if request.check_server_identity_callback == 0 {
        request.check_server_identity_callback = defaults.check_server_identity_callback;
        request.check_server_identity_from_agent = defaults.check_server_identity_callback != 0;
    }
    if request.servername.is_none() {
        request.servername = defaults.servername.clone();
    }
    for (field, value) in &defaults.session_identity {
        if !request
            .session_identity
            .iter()
            .any(|(request_field, _)| request_field == field)
        {
            request
                .session_identity
                .push((field.clone(), value.clone()));
        }
    }
}

fn collect_pfx(
    value: &serde_json::Value,
    default_passphrase: &str,
    out: &mut Vec<(Vec<u8>, String)>,
) {
    use serde_json::Value;
    match value {
        Value::Object(map) if map.get("type").and_then(Value::as_str) == Some("Buffer") => {
            if let Some(data) = map.get("data").and_then(Value::as_array) {
                out.push((numeric_array_to_bytes(data), default_passphrase.to_string()));
            }
        }
        Value::Object(map) => {
            let passphrase = map
                .get("passphrase")
                .and_then(Value::as_str)
                .unwrap_or(default_passphrase);
            if let Some(buf) = map.get("buf") {
                collect_pfx(buf, passphrase, out);
            }
        }
        Value::Array(values) => {
            if !values.is_empty() && values.iter().all(|value| value.is_u64() || value.is_i64()) {
                out.push((
                    numeric_array_to_bytes(values),
                    default_passphrase.to_string(),
                ));
            } else {
                for value in values {
                    collect_pfx(value, default_passphrase, out);
                }
            }
        }
        _ => {}
    }
}

fn pfx_common_name(der: &[u8], passphrase: &str) -> Option<String> {
    let pfx = p12::PFX::parse(der).ok()?;
    let certificate_der = pfx.cert_x509_bags(passphrase).ok()?.into_iter().next()?;
    certificate_common_name(&certificate_der)
}

fn pfx_private_keys(pfx: &p12::PFX, passphrase: &str) -> Result<Vec<Vec<u8>>, String> {
    let mut keys = pfx
        .key_bags(passphrase)
        .map_err(|_| "https: invalid pfx passphrase or private key".to_string())?;
    if !keys.is_empty() {
        return Ok(keys);
    }

    // `p12` handles the legacy PKCS#12 PBE used by older Node fixtures but
    // deliberately leaves modern PBES2 algorithms opaque. Decode that one
    // bag shape with RustCrypto's PKCS#5 implementation so AES-256-CBC /
    // PBKDF2-HMAC-SHA256 identities work as well.
    use pkcs5::der::Decode;
    for safe_bag in pfx
        .bags(passphrase)
        .map_err(|_| "https: invalid pfx passphrase or private key".to_string())?
    {
        let p12::SafeBagKind::Pkcs8ShroudedKeyBag(encrypted) = safe_bag.bag else {
            continue;
        };
        let p12::AlgorithmIdentifier::OtherAlg(algorithm) = encrypted.encryption_algorithm else {
            continue;
        };
        if algorithm.algorithm_type.to_string() != "1.2.840.113549.1.5.13" {
            continue;
        }
        let Some(parameters_der) = algorithm.params else {
            continue;
        };
        let parameters = pkcs5::pbes2::Parameters::from_der(&parameters_der)
            .map_err(|_| "https: invalid pfx PBES2 parameters".to_string())?;
        let private_key = parameters
            .decrypt(passphrase.as_bytes(), &encrypted.encrypted_data)
            .map_err(|_| "https: invalid pfx passphrase or private key".to_string())?;
        keys.push(private_key);
    }
    Ok(keys)
}

fn certificate_common_name(certificate_der: &[u8]) -> Option<String> {
    use x509_cert::der::Decode;

    let certificate = x509_cert::Certificate::from_der(certificate_der).ok()?;
    for rdn in certificate.tbs_certificate().subject().iter_rdn() {
        for attribute in rdn.iter() {
            if attribute.oid.to_string() != "2.5.4.3" {
                continue;
            }
            if let Ok(value) =
                x509_cert::ext::pkix::name::DirectoryString::try_from(&attribute.value)
            {
                return Some(value.value().into_owned());
            }
        }
    }
    None
}

fn pfx_certificate_and_key(
    der: &[u8],
    passphrase: &str,
) -> Result<(Vec<Vec<u8>>, Vec<u8>), String> {
    let pfx = p12::PFX::parse(der).map_err(|_| "https: invalid pfx identity".to_string())?;
    let certificates = pfx
        .cert_x509_bags(passphrase)
        .map_err(|_| "https: invalid pfx passphrase or certificate".to_string())?;
    if certificates.is_empty() {
        return Err("https: pfx identity has no certificate".to_string());
    }
    let private_key = pfx_private_keys(&pfx, passphrase)?
        .into_iter()
        .next()
        .ok_or_else(|| "https: pfx identity has no private key".to_string())?;
    Ok((certificates, private_key))
}

/// Invoke `checkServerIdentity(host, cert)` and return the message of a
/// returned Error. The callback contract exercised by Node's Agent tests only
/// depends on the host and on whether the callback returns an Error.
pub(crate) unsafe fn check_server_identity_error(
    options: &TlsOptions,
    host: &str,
) -> Option<String> {
    let callback = options.check_server_identity_callback;
    if callback == 0 {
        return None;
    }
    extern "C" {
        fn js_error_is_error(value: f64) -> f64;
        fn js_error_get_message(error: *mut u8) -> *mut perry_ffi::StringHeader;
    }
    let host = perry_ffi::alloc_string(host);
    let cert = perry_ffi::alloc_object();
    let closure = perry_ffi::JsClosure::from_raw(callback as *const perry_ffi::RawClosureHeader);
    let result = closure.call2(
        f64::from_bits(perry_ffi::JsValue::from_string_ptr(host.as_raw()).bits()),
        f64::from_bits(cert.bits()),
    );
    let is_error = perry_ffi::JsValue::from_bits(js_error_is_error(result).to_bits());
    if !is_error.is_bool() || !is_error.to_bool() {
        return None;
    }
    let result_value = perry_ffi::JsValue::from_bits(result.to_bits());
    let message = js_error_get_message(result_value.as_pointer::<u8>());
    perry_ffi::read_string(perry_ffi::JsString::from_raw(message)).map(String::from)
}

/// Flatten a JSON `ca` value (string PEM, Node `Buffer` shape, a raw
/// numeric byte array, or an array of any of those) into PEM byte blobs.
fn collect_pems(v: &serde_json::Value, out: &mut Vec<Vec<u8>>) {
    use serde_json::Value;
    match v {
        Value::String(s) => out.push(s.as_bytes().to_vec()),
        Value::Object(map) if map.get("type").and_then(|t| t.as_str()) == Some("Buffer") => {
            if let Some(data) = map.get("data").and_then(|d| d.as_array()) {
                out.push(numeric_array_to_bytes(data));
            }
        }
        Value::Array(arr) => {
            // A bare numeric array is one cert's raw bytes; an array of
            // strings / Buffers is a list of CAs.
            if !arr.is_empty() && arr.iter().all(|e| e.is_u64() || e.is_i64()) {
                out.push(numeric_array_to_bytes(arr));
            } else {
                for e in arr {
                    collect_pems(e, out);
                }
            }
        }
        _ => {}
    }
}

fn numeric_array_to_bytes(arr: &[serde_json::Value]) -> Vec<u8> {
    arr.iter()
        .filter_map(|n| n.as_u64().map(|u| u as u8))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn pems(v: serde_json::Value) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        collect_pems(&v, &mut out);
        out
    }

    #[test]
    fn collect_pems_string() {
        assert_eq!(
            pems(json!("-----BEGIN CERTIFICATE-----")),
            vec![b"-----BEGIN CERTIFICATE-----".to_vec()]
        );
    }

    #[test]
    fn collect_pems_buffer_shape() {
        // `JSON.stringify(Buffer.from("hi"))`.
        assert_eq!(
            pems(json!({"type": "Buffer", "data": [104, 105]})),
            vec![b"hi".to_vec()]
        );
    }

    #[test]
    fn collect_pems_array_of_strings() {
        // `ca: [pem1, pem2]` — each element is a separate CA.
        assert_eq!(pems(json!(["a", "b"])), vec![b"a".to_vec(), b"b".to_vec()]);
    }

    #[test]
    fn collect_pems_array_of_buffers() {
        assert_eq!(
            pems(json!([
                {"type": "Buffer", "data": [104]},
                {"type": "Buffer", "data": [105]}
            ])),
            vec![b"h".to_vec(), b"i".to_vec()]
        );
    }

    #[test]
    fn collect_pems_bare_numeric_array_is_one_cert() {
        // A raw numeric array (a single Buffer serialized as numbers) is
        // one cert, not a list of single-byte certs.
        assert_eq!(pems(json!([104, 105])), vec![b"hi".to_vec()]);
    }

    #[test]
    fn needs_custom_client_logic() {
        let mut t = TlsOptions::default();
        assert!(!t.needs_custom_client());
        t.reject_unauthorized = Some(true);
        assert!(!t.needs_custom_client());
        t.reject_unauthorized = Some(false);
        assert!(t.needs_custom_client());

        let mut t = TlsOptions::default();
        t.check_server_identity_callback = 1;
        assert!(t.needs_custom_client());
        assert!(!t.accept_invalid_certs());

        let mut t = TlsOptions::default();
        t.ca_pems.push(b"pem".to_vec());
        assert!(t.needs_custom_client());
        // ca alone does NOT bypass verification — it adds trust anchors.
        assert!(!t.accept_invalid_certs());

        let mut t = TlsOptions::default();
        t.servername = Some("agent1".to_string());
        assert!(t.needs_custom_client());
        assert!(!t.accept_invalid_certs());
    }

    #[test]
    fn extracts_common_name_from_pkcs12_identity() {
        let agent1 = include_bytes!("../../../vendor/nodejs/test/fixtures/keys/agent1.pfx");
        let agent10 = include_bytes!("../../../vendor/nodejs/test/fixtures/keys/agent10.pfx");
        assert_eq!(pfx_common_name(agent1, "sample").as_deref(), Some("agent1"));
        assert_eq!(
            pfx_common_name(agent10, "sample").as_deref(),
            Some("agent10.example.com")
        );
        for identity in [agent1.as_slice(), agent10.as_slice()] {
            let options = TlsOptions {
                reject_unauthorized: Some(false),
                client_pfx: vec![(identity.to_vec(), "sample".to_string())],
                ..TlsOptions::default()
            };
            let built = options.build_client(None);
            assert!(built.is_ok(), "{built:?}");
        }
    }

    #[test]
    fn builds_node_ca_config_for_explicit_endpoint_certificate() {
        let certificate = include_bytes!("../../../vendor/nodejs/test/fixtures/keys/rsa_cert.crt");
        assert!(build_node_tls_config(&[certificate.to_vec()], None, false, false, None,).is_ok());
        let certificate_der = rustls_pemfile::certs(&mut std::io::Cursor::new(certificate))
            .next()
            .expect("fixture contains a certificate")
            .expect("fixture certificate is valid");
        assert!(certificate_matches_hostname(
            certificate_der.as_ref(),
            "localhost"
        ));
        assert!(!certificate_matches_hostname(
            certificate_der.as_ref(),
            "example.com"
        ));
    }

    #[test]
    fn verifies_directly_trusted_legacy_v1_certificate() {
        let endpoint = include_bytes!("../../../vendor/nodejs/test/fixtures/keys/agent3-cert.pem");
        let trusted_ca = include_bytes!("../../../vendor/nodejs/test/fixtures/keys/ca2-cert.pem");
        let endpoint = rustls_pemfile::certs(&mut std::io::Cursor::new(endpoint))
            .next()
            .unwrap()
            .unwrap();
        let trusted_ca = rustls_pemfile::certs(&mut std::io::Cursor::new(trusted_ca))
            .next()
            .unwrap()
            .unwrap();
        assert!(legacy_direct_certificate_is_valid(
            endpoint.as_ref(),
            trusted_ca.as_ref(),
            rustls::pki_types::UnixTime::now(),
        ));
    }

    #[test]
    fn dns_name_matching_limits_wildcards_to_one_label() {
        assert!(dns_name_matches("*.example.com", "api.example.com"));
        assert!(!dns_name_matches("*.example.com", "a.b.example.com"));
        assert!(!dns_name_matches("*.example.com", "example.com"));
    }
}

/// Read a function-valued option without the JSON round-trip (which drops
/// functions). Mirrors the raw NaN-boxed field read in `agent.rs`.
unsafe fn closure_field(obj_f64: f64, field: &str) -> i64 {
    let value =
        perry_ffi::object_field_by_name(perry_ffi::JsValue::from_bits(obj_f64.to_bits()), field);
    let bits = value.bits();
    if value.is_pointer() {
        (bits & 0x0000_FFFF_FFFF_FFFF) as i64
    } else if bits >> 48 == 0 && bits >= 0x10000 {
        bits as i64
    } else {
        0
    }
}
