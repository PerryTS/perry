//! Explicit embedding authority. These options are separate from guest WASI
//! outbound grants and never inherit host files, environment or TLS credentials.
use super::*;
use serde::Deserialize;
use std::{
    net::SocketAddr,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use turnloop_tls::{
    rustls::pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer},
    ServerConfig,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct ServerOptions {
    pub address: SocketAddr,
    #[serde(default = "limit")]
    pub max_connections: usize,
    pub handshake_timeout_ms: Option<u64>,
    pub request_head_timeout_ms: Option<u64>,
    pub response_head_timeout_ms: Option<u64>,
    pub between_bytes_timeout_ms: Option<u64>,
    pub linger_timeout_ms: Option<u64>,
    pub tls: Option<TlsOptions>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct TlsOptions {
    certificate_pem: String,
    private_key_pem: String,
}
fn limit() -> usize {
    64
}
impl ServerOptions {
    pub fn transport(&self) -> Result<server::Options> {
        ensure!(self.max_connections > 0, "maxConnections must be positive");
        let mut options = server::Options::default();
        for (value, target) in [
            (self.handshake_timeout_ms, &mut options.handshake_timeout),
            (
                self.request_head_timeout_ms,
                &mut options.request_head_timeout,
            ),
            (
                self.response_head_timeout_ms,
                &mut options.response_head_timeout,
            ),
            (
                self.between_bytes_timeout_ms,
                &mut options.between_bytes_timeout,
            ),
            (self.linger_timeout_ms, &mut options.linger_timeout),
        ] {
            if let Some(value) = value {
                *target = Duration::from_millis(value);
            }
        }
        if let Some(tls) = &self.tls {
            let certificates = CertificateDer::pem_slice_iter(tls.certificate_pem.as_bytes())
                .collect::<std::result::Result<Vec<_>, _>>()
                .context("invalid HTTP TLS certificate PEM")?;
            let key = PrivateKeyDer::from_pem_slice(tls.private_key_pem.as_bytes())
                .context("invalid HTTP TLS private key PEM")?;
            options.tls = Some(
                ServerConfig::new(
                    certificates,
                    key,
                    vec![b"http/1.1".to_vec()],
                    SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
                )
                .context("invalid HTTP TLS configuration")?,
            );
        }
        Ok(options)
    }
}
