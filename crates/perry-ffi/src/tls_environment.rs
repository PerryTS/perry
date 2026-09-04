//! Shared client-side TLS environment policy.
//!
//! Fetch and the `node:https` extension use separate reqwest clients, but the
//! process environment is one Node-compatible contract. Keeping resolution in
//! their shared FFI support crate prevents those HTTP surfaces from drifting.

use std::ffi::OsStr;
use std::path::Path;
use std::sync::OnceLock;

static ENV_CA_PEMS: OnceLock<Vec<Vec<u8>>> = OnceLock::new();

/// Process-wide TLS settings consumed by HTTP client implementations.
#[derive(Clone, Copy, Debug)]
pub struct NodeTlsClientEnvironment {
    accept_invalid_certificates: bool,
    ca_pems: &'static [Vec<u8>],
}

impl NodeTlsClientEnvironment {
    /// Whether `NODE_TLS_REJECT_UNAUTHORIZED=0` disabled certificate checks.
    pub fn accepts_invalid_certificates(self) -> bool {
        self.accept_invalid_certificates
    }

    /// PEM bundles configured through `SSL_CERT_FILE` and
    /// `NODE_EXTRA_CA_CERTS`, in that order.
    pub fn ca_pems(self) -> &'static [Vec<u8>] {
        self.ca_pems
    }
}

/// Resolve Node-compatible TLS client environment variables.
///
/// CA files are cached on first use, matching Node's startup-scoped
/// `NODE_EXTRA_CA_CERTS` behavior. `NODE_TLS_REJECT_UNAUTHORIZED` remains a
/// live lookup because assignments through `process.env` must affect later
/// `node:https` requests.
pub fn node_tls_client_environment() -> NodeTlsClientEnvironment {
    NodeTlsClientEnvironment {
        accept_invalid_certificates: reject_unauthorized_from_env_value(
            std::env::var_os("NODE_TLS_REJECT_UNAUTHORIZED").as_deref(),
        ),
        ca_pems: ENV_CA_PEMS
            .get_or_init(|| {
                load_ca_pems(
                    std::env::var_os("SSL_CERT_FILE").as_deref(),
                    std::env::var_os("NODE_EXTRA_CA_CERTS").as_deref(),
                    |path| std::fs::read(path).ok(),
                )
            })
            .as_slice(),
    }
}

fn reject_unauthorized_from_env_value(value: Option<&OsStr>) -> bool {
    value == Some(OsStr::new("0"))
}

fn load_ca_pems(
    ssl_cert_file: Option<&OsStr>,
    node_extra_ca_certs: Option<&OsStr>,
    mut read: impl FnMut(&Path) -> Option<Vec<u8>>,
) -> Vec<Vec<u8>> {
    [ssl_cert_file, node_extra_ca_certs]
        .into_iter()
        .flatten()
        .filter_map(|path| read(Path::new(path)))
        .filter(|pem| !pem.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reject_unauthorized_requires_exact_zero() {
        assert!(reject_unauthorized_from_env_value(Some(OsStr::new("0"))));
        assert!(!reject_unauthorized_from_env_value(None));
        assert!(!reject_unauthorized_from_env_value(Some(OsStr::new("1"))));
        assert!(!reject_unauthorized_from_env_value(Some(OsStr::new(
            "false"
        ))));
        assert!(!reject_unauthorized_from_env_value(Some(OsStr::new("00"))));
    }

    #[test]
    fn loads_ssl_cert_file_then_node_extra_ca_certs() {
        let certs = load_ca_pems(
            Some(OsStr::new("ssl.pem")),
            Some(OsStr::new("extra.pem")),
            |path| match path.to_str() {
                Some("ssl.pem") => Some(b"ssl-cert".to_vec()),
                Some("extra.pem") => Some(b"extra-cert".to_vec()),
                _ => None,
            },
        );
        assert_eq!(certs, vec![b"ssl-cert".to_vec(), b"extra-cert".to_vec()]);
    }

    #[test]
    fn skips_missing_and_empty_ca_files() {
        let certs = load_ca_pems(
            Some(OsStr::new("missing.pem")),
            Some(OsStr::new("empty.pem")),
            |path| match path.to_str() {
                Some("empty.pem") => Some(Vec::new()),
                _ => None,
            },
        );
        assert!(certs.is_empty());
    }
}
