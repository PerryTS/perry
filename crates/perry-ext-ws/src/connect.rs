//! The outbound client's URL, and nothing else.
//!
//! This is what is left of the module that used to *be* the client connect.
//! `tokio_tungstenite::connect_async` did four things in one call — parse the
//! URL, open the TCP connection, negotiate TLS for `wss://`, and run the
//! handshake — and replacing it left four separate pieces, three of which are
//! now somewhere better: the socket and the TLS session are
//! [`crate::turnloop_io`]'s (a turnloop `tcp_connect` with a
//! `perry_tls_session` layer above the same handle), and the handshake is
//! [`crate::handshake`]'s, which needs no transport at all.
//!
//! What remains is the parse, which is pure and therefore testable on its own.

/// Where a `ws://` / `wss://` URL points, in the shape a connect needs.
#[derive(Debug)]
pub(crate) struct Target {
    pub(crate) secure: bool,
    pub(crate) host: String,
    pub(crate) port: u16,
    /// The `Host:` header value. `ws` sends the default port implicitly, like
    /// a browser, so this is not always `host:port`.
    pub(crate) authority: String,
    /// The request target: path plus query.
    pub(crate) path: String,
}

pub(crate) fn parse(url: &str) -> Result<Target, String> {
    let parsed = url::Url::parse(url).map_err(|e| format!("Invalid URL: {e}"))?;
    let secure = match parsed.scheme() {
        "ws" | "http" => false,
        "wss" | "https" => true,
        other => {
            return Err(format!(
                "The URL's protocol must be one of \"ws:\", \"wss:\", \"http:\", \"https:\", or \"ws+unix:\" (got \"{other}:\")"
            ))
        }
    };
    let host = parsed
        .host_str()
        .ok_or_else(|| "Invalid URL: no host".to_string())?
        .to_string();
    let port = parsed
        .port_or_known_default()
        .unwrap_or(if secure { 443 } else { 80 });
    // `ws` sends the default port implicitly, like a browser.
    let authority = match parsed.port() {
        Some(explicit) => format!("{host}:{explicit}"),
        None => host.clone(),
    };
    let mut path = parsed.path().to_string();
    if path.is_empty() {
        path.push('/');
    }
    if let Some(query) = parsed.query() {
        path.push('?');
        path.push_str(query);
    }
    Ok(Target {
        secure,
        host,
        port,
        authority,
        path,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_default_port_is_implicit_in_the_authority() {
        let target = parse("wss://example.test/socket").expect("a target");
        assert!(target.secure);
        assert_eq!(target.port, 443);
        assert_eq!(target.authority, "example.test");
        assert_eq!(target.path, "/socket");
    }

    #[test]
    fn an_explicit_port_is_carried_into_the_host_header() {
        let target = parse("ws://example.test:8080/a?b=c").expect("a target");
        assert!(!target.secure);
        assert_eq!(target.port, 8080);
        assert_eq!(target.authority, "example.test:8080");
        assert_eq!(target.path, "/a?b=c");
    }

    #[test]
    fn an_empty_path_becomes_a_slash() {
        let target = parse("ws://example.test").expect("a target");
        assert_eq!(target.path, "/");
    }

    #[test]
    fn a_protocol_ws_does_not_speak_is_refused_by_name() {
        let error = parse("ftp://example.test").expect_err("a refusal");
        assert!(error.contains("\"ftp:\""), "{error}");
    }
}
