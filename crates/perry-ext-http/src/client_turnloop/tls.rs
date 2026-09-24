//! `https:` for this lane: the rustls configuration a request's Node TLS
//! options describe, and the session that runs it above the turnloop handle.
//!
//! The configuration is `tls_client::TlsOptions::client_config` — the same
//! verifier stack (Node CA semantics, the Common-Name fallback, the exact-leaf
//! self-signed trust case, `rejectUnauthorized: false`, PKCS#12 client
//! identities) reqwest was handed through `use_preconfigured_tls`, now handed
//! to [`perry_tls_session::TlsSession`] instead. Nothing about *what* is
//! verified changes; only who drives the records.
//!
//! # Configs are cached, and that is load-bearing
//!
//! rustls keeps its TLS session-resumption store inside the `ClientConfig`.
//! reqwest's per-agent client cache is what let a second request resume the
//! first one's session (`tls_compat.rs`), so a fresh config per request would
//! silently disable resumption. Configs are therefore memoized per
//! option-identity for the life of the process, like the reqwest clients were.
//!
//! # ALPN
//!
//! None is offered. Node's `https` client sends no ALPN extension by default
//! and only ever speaks HTTP/1.1, while reqwest's default connector offered
//! `h2` — so a server that also spoke HTTP/2 used to negotiate it and report
//! `res.httpVersion === '2.0'` where Node reports `'1.1'`.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, OnceLock};

use perry_tls_session::TlsSession;
use rustls::pki_types::ServerName;

use crate::tls_client::TlsOptions;

/// Everything needed to open a session, decided on the JS thread at dispatch.
#[derive(Clone)]
pub(crate) struct TlsPlan {
    pub(super) config: Arc<rustls::ClientConfig>,
    pub(super) server_name: ServerName<'static>,
}

fn configs() -> &'static Mutex<HashMap<u64, Arc<rustls::ClientConfig>>> {
    static CONFIGS: OnceLock<Mutex<HashMap<u64, Arc<rustls::ClientConfig>>>> = OnceLock::new();
    CONFIGS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The identity a config is cached under. Two option sets that build the same
/// verifier share one config (and one resumption store); anything that could
/// change verification or the client identity separates them.
pub(super) fn identity(options: &TlsOptions) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let environment = perry_ffi::node_tls_client_environment();
    if options.needs_custom_client() {
        // `rejectUnauthorized: true` is the default spelled out; it builds the
        // same verifier as leaving it unset, so it must not split the store.
        let mut canonical = options.clone();
        if canonical.reject_unauthorized == Some(true) {
            canonical.reject_unauthorized = None;
        }
        canonical.hash(&mut hasher);
        environment.accepts_invalid_certificates().hash(&mut hasher);
        environment.ca_pems().hash(&mut hasher);
    } else {
        // Every request with no TLS customization shares one config, the way
        // they all shared reqwest's pooled default client.
        0u8.hash(&mut hasher);
    }
    hasher.finish()
}

/// Build (or fetch) the config for `options` and pair it with the name the
/// handshake verifies and, unless suppressed, sends as SNI.
pub(crate) fn plan(options: &TlsOptions, url_host: &str) -> Result<TlsPlan, String> {
    let key = identity(options);
    let cached = configs()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&key)
        .cloned();
    let config = match cached {
        Some(config) => config,
        None => {
            let mut built = options.client_config()?;
            // `servername: ''` is Node's way of sending no SNI at all.
            if options.servername.as_deref() == Some("") {
                built.enable_sni = false;
            }
            let built = Arc::new(built);
            configs()
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .entry(key)
                .or_insert(built)
                .clone()
        }
    };
    // Node sends `servername` as SNI and verifies against it; an empty one
    // means "no SNI", so the URL host is what is verified.
    let name = options
        .servername
        .as_deref()
        .filter(|name| !name.is_empty())
        .unwrap_or(url_host);
    let server_name = ServerName::try_from(name.to_string())
        .map_err(|_| format!("ERR_TLS_CERT_ALTNAME_INVALID: invalid servername {name:?}"))?;
    Ok(TlsPlan {
        config,
        server_name,
    })
}

/// Open the client session for a plan. The ClientHello is produced by the
/// first `pump`.
pub(super) fn open(plan: &TlsPlan) -> Result<TlsSession, String> {
    TlsSession::client(plan.config.clone(), plan.server_name.clone()).map_err(|e| e.to_string())
}

/// Node's message for a handshake failure's cause code. Node reports these as
/// an `Error` carrying `.code` and OpenSSL's text; rustls has its own text, so
/// the codes Node users test for get Node's words and anything else keeps
/// rustls's.
pub(super) fn node_failure_message(code: &str, rustls_message: &str) -> String {
    match code {
        "UNABLE_TO_VERIFY_LEAF_SIGNATURE" => "unable to verify the first certificate".to_string(),
        "CERT_HAS_EXPIRED" => "certificate has expired".to_string(),
        "CERT_NOT_YET_VALID" => "certificate is not yet valid".to_string(),
        "CERT_REVOKED" => "certificate revoked".to_string(),
        "ERR_TLS_CERT_ALTNAME_INVALID" => {
            format!("Hostname/IP does not match certificate's altnames: {rustls_message}")
        }
        _ => rustls_message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_without_tls_options_share_one_config() {
        let a = TlsOptions::default();
        let b = TlsOptions {
            reject_unauthorized: Some(true),
            ..TlsOptions::default()
        };
        // Both are "no customization": one resumption store, as with reqwest's
        // single pooled client. (Holds whether or not the host environment
        // sets `SSL_CERT_FILE` / `NODE_EXTRA_CA_CERTS`, which makes every
        // request "custom" but still identical.)
        assert_eq!(identity(&a), identity(&b));
        let custom = TlsOptions {
            reject_unauthorized: Some(false),
            ..TlsOptions::default()
        };
        assert_ne!(identity(&a), identity(&custom));
    }

    #[test]
    fn a_plan_is_memoized_so_session_resumption_survives() {
        let options = TlsOptions {
            reject_unauthorized: Some(false),
            ..TlsOptions::default()
        };
        let first = plan(&options, "localhost").expect("plan builds");
        let second = plan(&options, "localhost").expect("plan builds");
        assert!(Arc::ptr_eq(&first.config, &second.config));
        assert!(
            first.config.alpn_protocols.is_empty(),
            "Node offers no ALPN"
        );
    }

    #[test]
    fn an_empty_servername_disables_sni_but_verifies_the_url_host() {
        let options = TlsOptions {
            reject_unauthorized: Some(false),
            servername: Some(String::new()),
            ..TlsOptions::default()
        };
        let plan = plan(&options, "localhost").expect("plan builds");
        assert!(!plan.config.enable_sni);
        assert_eq!(plan.server_name.to_str(), "localhost");
    }

    #[test]
    fn node_codes_get_node_text() {
        assert_eq!(
            node_failure_message("UNABLE_TO_VERIFY_LEAF_SIGNATURE", "x"),
            "unable to verify the first certificate"
        );
        assert_eq!(
            node_failure_message("ERR_SSL_PROTOCOL_ERROR", "rustls text"),
            "rustls text"
        );
    }
}
