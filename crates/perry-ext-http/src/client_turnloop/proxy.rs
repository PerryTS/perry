//! `NODE_USE_ENV_PROXY=1`: which proxy, if any, a request goes through.
//!
//! Node's `http`/`https` clients ignore `HTTP_PROXY`/`HTTPS_PROXY`/`NO_PROXY`
//! unless `NODE_USE_ENV_PROXY=1` (or `--use-env-proxy`) is set, and then read
//! the lowercase spelling first (`http_proxy || HTTP_PROXY`). The matching
//! and the `NO_PROXY` grammar are `turnloop_http::client::ProxyEnvironment`'s,
//! the same policy `fetch` applies.
//!
//! Two transports follow from the answer (see `conn.rs`): an `http://` target
//! is sent to the proxy in absolute-form with `Proxy-Authorization` from the
//! proxy URL's credentials; an `https://` target opens a `CONNECT` tunnel and
//! runs TLS inside it, with the credentials on the `CONNECT` only.

use turnloop_http::client::ProxyEnvironment;

/// Read one variable the way Node does: lowercase first, then uppercase.
fn env_pair(lower: &str, upper: &str) -> Option<String> {
    std::env::var(lower)
        .ok()
        .filter(|v| !v.is_empty())
        .or_else(|| std::env::var(upper).ok().filter(|v| !v.is_empty()))
}

fn environment() -> ProxyEnvironment {
    ProxyEnvironment {
        http_proxy: env_pair("http_proxy", "HTTP_PROXY"),
        https_proxy: env_pair("https_proxy", "HTTPS_PROXY"),
        no_proxy: env_pair("no_proxy", "NO_PROXY").unwrap_or_default(),
    }
}

/// The proxy for `url`, if the process opted in and one applies.
///
/// `Err` carries the message for a proxy variable that is set but unusable
/// (not an `http://` URL); the request fails with it rather than silently
/// going direct.
pub(super) fn proxy_for(url: &url::Url) -> Result<Option<url::Url>, String> {
    if !crate::node_env_proxy_enabled() {
        return Ok(None);
    }
    resolve(&environment(), url)
}

fn resolve(env: &ProxyEnvironment, url: &url::Url) -> Result<Option<url::Url>, String> {
    env.proxy_for(url)
        .map_err(|error| format!("{}: {}", error.code, error.message))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(http: Option<&str>, https: Option<&str>, no: &str) -> ProxyEnvironment {
        ProxyEnvironment {
            http_proxy: http.map(str::to_string),
            https_proxy: https.map(str::to_string),
            no_proxy: no.to_string(),
        }
    }

    fn url(s: &str) -> url::Url {
        url::Url::parse(s).unwrap()
    }

    #[test]
    fn the_scheme_selects_the_variable() {
        let e = env(Some("http://p1.invalid:1"), Some("http://p2.invalid:2"), "");
        assert_eq!(
            resolve(&e, &url("http://a.invalid/"))
                .unwrap()
                .unwrap()
                .host_str(),
            Some("p1.invalid")
        );
        assert_eq!(
            resolve(&e, &url("https://a.invalid/"))
                .unwrap()
                .unwrap()
                .host_str(),
            Some("p2.invalid")
        );
    }

    #[test]
    fn no_proxy_bypasses_by_domain_suffix() {
        let e = env(
            Some("http://p.invalid:1"),
            None,
            "internal.invalid,127.0.0.1",
        );
        assert!(resolve(&e, &url("http://api.internal.invalid/"))
            .unwrap()
            .is_none());
        assert!(resolve(&e, &url("http://127.0.0.1:9/")).unwrap().is_none());
        assert!(resolve(&e, &url("http://other.invalid/"))
            .unwrap()
            .is_some());
        assert!(resolve(&e, &url("https://other.invalid/"))
            .unwrap()
            .is_none());
    }

    #[test]
    fn a_non_http_proxy_is_an_error_not_a_silent_direct_connection() {
        let e = env(Some("socks5://p.invalid:1"), None, "");
        assert!(resolve(&e, &url("http://a.invalid/")).is_err());
    }
}
