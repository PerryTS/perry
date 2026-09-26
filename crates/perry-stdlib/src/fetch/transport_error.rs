//! The error shapes a failed `fetch` rejects with, matching Node's.
//!
//! Two families: a transport failure is `TypeError: fetch failed` whose
//! `cause` carries the system error ([`FetchFailure`]); a request undici
//! refuses to *construct* is a bare `TypeError` naming the problem
//! ([`Rejection`]).

use crate::turnloop_client::Declined;

pub(crate) struct FetchFailure {
    cause_message: String,
    code: Option<&'static str>,
    errno: Option<i32>,
    syscall: Option<&'static str>,
    hostname: Option<String>,
    /// `cause.name`. Transport failures are plain `Error`s, which is what Node
    /// reports for them; a request undici refuses to build carries a DOM
    /// exception name instead (`NotSupportedError`).
    cause_name: Option<&'static [u8]>,
}

impl FetchFailure {
    /// The turnloop client engine's failure, which already carries Node's
    /// `cause.code` and `syscall` rather than a prose chain to classify.
    /// `getaddrinfo ENOTFOUND` keeps the `errno` and `hostname` Node reports,
    /// because that is the one shape callers match on.
    pub(crate) fn from_client(
        code: &'static str,
        message: String,
        syscall: Option<&'static str>,
    ) -> Self {
        let hostname = (code == "ENOTFOUND").then(|| {
            message
                .rsplit(' ')
                .next()
                .filter(|h| !h.is_empty() && *h != code)
                .unwrap_or_default()
                .to_string()
        });
        Self {
            cause_message: message,
            code: Some(code),
            errno: (code == "ENOTFOUND").then_some(-3008),
            syscall,
            hostname,
            cause_name: None,
        }
    }

    /// A request undici refuses to construct at all.
    ///
    /// The Fetch standard lists `Expect` among the forbidden request headers,
    /// and undici does not silently drop it the way a "forbidden header" reader
    /// might expect — it throws, so `fetch()` rejects with
    /// `TypeError: fetch failed` whose cause is
    /// `NotSupportedError: expect header not supported` with
    /// `code: 'UND_ERR_NOT_SUPPORTED'`. Measured against Node 26.5.1, not
    /// inferred from the spec text.
    pub(crate) fn forbidden_header(name: &str) -> Self {
        Self {
            cause_message: format!("{name} header not supported"),
            code: Some("UND_ERR_NOT_SUPPORTED"),
            errno: None,
            syscall: None,
            hostname: None,
            cause_name: Some(b"NotSupportedError"),
        }
    }

    /// `fetch failed` whose cause is a plain `Error(message)`, optionally
    /// carrying a `code` — undici's shape for a request it accepted but cannot
    /// carry (`unknown scheme`, an unsupported proxy).
    pub(crate) fn refused(message: &str, code: Option<&'static str>) -> Self {
        Self {
            cause_message: message.to_string(),
            code,
            errno: None,
            syscall: None,
            hostname: None,
            cause_name: None,
        }
    }

    pub(crate) fn into_js_bits(self) -> u64 {
        let cause_message = perry_runtime::js_string_from_bytes(
            self.cause_message.as_ptr(),
            self.cause_message.len() as u32,
        );
        if let Some(code) = self.code {
            perry_runtime::node_submodules::register_error_code_pub(cause_message, code);
        }
        if let Some(errno) = self.errno {
            perry_runtime::node_submodules::register_error_errno(cause_message, errno);
        }
        if let Some(syscall) = self.syscall {
            perry_runtime::node_submodules::register_error_syscall(cause_message, syscall);
        }
        if let Some(hostname) = self.hostname {
            perry_runtime::node_submodules::register_error_hostname(cause_message, hostname);
        }
        let cause = match self.cause_name {
            Some(name) => perry_runtime::error::js_error_new_with_name_message(name, cause_message),
            None => perry_runtime::error::js_error_new_with_message(cause_message),
        };
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        let cause_handle =
            scope.root_nanbox_u64(perry_runtime::JSValue::pointer(cause as *const u8).bits());
        let message = b"fetch failed";
        let message = perry_runtime::js_string_from_bytes(message.as_ptr(), message.len() as u32);
        let error = perry_runtime::error::js_typeerror_new_with_cause(
            message,
            cause_handle.get_nanbox_f64(),
        );
        perry_runtime::JSValue::pointer(error as *const u8).bits()
    }
}

/// How a request the turnloop engine refused to build rejects.
///
/// Before the reqwest fallback was removed, each of these was a *decline*: the
/// caller ran a reqwest future instead, and the rejection text was whatever
/// reqwest produced (or, for embedded credentials and `CONNECT`/`TRACE`, the
/// request simply went out — reqwest is more permissive than undici). Each arm
/// below is Node 26.5.1's answer for the same input, measured rather than
/// inferred; `test_gap_fetch_refused_requests.ts` pins them.
pub(crate) enum Rejection {
    /// `TypeError: fetch failed` with a cause.
    Failure(FetchFailure),
    /// A bare `TypeError(message)` with no cause.
    TypeError(String),
    /// `TypeError(message)` whose cause is a `TypeError(cause)` carrying `code`:
    /// the `Failed to parse URL from …` shape.
    ParseUrl {
        message: String,
        cause: &'static str,
        code: &'static str,
    },
}

impl Rejection {
    /// Map an engine refusal for `url`/`method` to Node's rejection.
    pub(crate) fn for_declined(declined: Declined, url: &str, method: &str) -> Self {
        match declined {
            Declined::Unsupported(error) => match error.message {
                "bad port" => Rejection::Failure(FetchFailure::refused("bad port", None)),
                "invalid URL" => Rejection::ParseUrl {
                    message: format!("Failed to parse URL from {url}"),
                    cause: "Invalid URL",
                    code: "ERR_INVALID_URL",
                },
                "unsupported URL scheme" => {
                    let scheme = url::Url::parse(url)
                        .map(|parsed| parsed.scheme().to_string())
                        .unwrap_or_default();
                    // undici's scheme fetch: `file:` is a named stub, every
                    // other scheme it does not know is `unknown scheme`.
                    let cause = if scheme == "file" {
                        "not implemented... yet..."
                    } else {
                        "unknown scheme"
                    };
                    Rejection::Failure(FetchFailure::refused(cause, None))
                }
                "URL contains credentials" => Rejection::TypeError(format!(
                    "Request cannot be constructed from a URL that includes credentials: {url}"
                )),
                "invalid method" => {
                    Rejection::TypeError(format!("'{method}' is not a valid HTTP method."))
                }
                "forbidden fetch method" => {
                    Rejection::TypeError(format!("'{method}' HTTP method is unsupported."))
                }
                other => Rejection::Failure(FetchFailure::refused(other, Some(error.code))),
            },
            Declined::Proxy(error) => {
                Rejection::Failure(FetchFailure::refused(error.message, Some(error.code)))
            }
            Declined::NoLoop => Rejection::Failure(FetchFailure::refused(
                "no event loop is available for this agent",
                None,
            )),
            Declined::NoTls => Rejection::Failure(FetchFailure::refused(
                "TLS client configuration is unavailable",
                None,
            )),
        }
    }

    /// The one-line text the non-`Response` surfaces (`js_fetch_text`, the SSE
    /// stream poller) report instead of an error object.
    pub(crate) fn message(&self) -> String {
        match self {
            Rejection::Failure(failure) => failure.cause_message.clone(),
            Rejection::TypeError(message) | Rejection::ParseUrl { message, .. } => message.clone(),
        }
    }

    pub(crate) fn into_js_bits(self) -> u64 {
        match self {
            Rejection::Failure(failure) => failure.into_js_bits(),
            Rejection::TypeError(message) => {
                let message =
                    perry_runtime::js_string_from_bytes(message.as_ptr(), message.len() as u32);
                let error = perry_runtime::error::js_typeerror_new(message);
                perry_runtime::JSValue::pointer(error as *const u8).bits()
            }
            Rejection::ParseUrl {
                message,
                cause,
                code,
            } => {
                let cause_message =
                    perry_runtime::js_string_from_bytes(cause.as_ptr(), cause.len() as u32);
                perry_runtime::node_submodules::register_error_code_pub(cause_message, code);
                let cause = perry_runtime::error::js_typeerror_new(cause_message);
                let scope = perry_runtime::gc::RuntimeHandleScope::new();
                let cause_handle = scope
                    .root_nanbox_u64(perry_runtime::JSValue::pointer(cause as *const u8).bits());
                let message =
                    perry_runtime::js_string_from_bytes(message.as_ptr(), message.len() as u32);
                let error = perry_runtime::error::js_typeerror_new_with_cause(
                    message,
                    cause_handle.get_nanbox_f64(),
                );
                perry_runtime::JSValue::pointer(error as *const u8).bits()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unsupported(url: &str, method: &str) -> Rejection {
        let error = turnloop_http::client::Request::new(url, method)
            .err()
            .expect("the policy layer must refuse this request");
        Rejection::for_declined(Declined::Unsupported(error), url, method)
    }

    /// Each refusal the engine can report maps to Node 26.5.1's text. The
    /// policy layer's messages are matched by string, so this also fails if a
    /// `turnloop-http` bump rewords one — which would otherwise silently
    /// degrade the answer to the generic `fetch failed` arm.
    #[test]
    fn engine_refusals_map_to_node_rejections() {
        assert!(matches!(
            unsupported("not-a-url", "GET"),
            Rejection::ParseUrl { ref message, cause: "Invalid URL", code: "ERR_INVALID_URL" }
                if message == "Failed to parse URL from not-a-url"
        ));
        assert_eq!(
            unsupported("ftp://example.test/x", "GET").message(),
            "unknown scheme"
        );
        assert_eq!(
            unsupported("file:///etc/hosts", "GET").message(),
            "not implemented... yet..."
        );
        assert!(matches!(
            unsupported("http://user:pass@example.test/x", "GET"),
            Rejection::TypeError(ref m) if m ==
                "Request cannot be constructed from a URL that includes credentials: \
                 http://user:pass@example.test/x"
        ));
        assert!(matches!(
            unsupported("http://example.test/x", "CONNECT"),
            Rejection::TypeError(ref m) if m == "'CONNECT' HTTP method is unsupported."
        ));
        assert!(matches!(
            unsupported("http://example.test/x", "BAD METHOD"),
            Rejection::TypeError(ref m) if m == "'BAD METHOD' is not a valid HTTP method."
        ));
    }

    #[test]
    fn blocked_port_has_no_javascript_error_code() {
        let rejection = Rejection::for_declined(
            Declined::Unsupported(turnloop_http::Error::new("ERR_BAD_PORT", "bad port")),
            "http://127.0.0.1:22/",
            "GET",
        );
        match rejection {
            Rejection::Failure(failure) => {
                assert_eq!(failure.cause_message, "bad port");
                assert_eq!(failure.code, None);
            }
            _ => panic!("bad port must reject as a fetch failure"),
        }
    }

    #[test]
    fn an_undrivable_proxy_is_a_fetch_failure_carrying_its_code() {
        let rejection = Rejection::for_declined(
            Declined::Proxy(turnloop_http::Error::new(
                "UND_ERR_NOT_SUPPORTED",
                "only HTTP proxies are supported",
            )),
            "https://example.test/",
            "GET",
        );
        match rejection {
            Rejection::Failure(failure) => {
                assert_eq!(failure.cause_message, "only HTTP proxies are supported");
                assert_eq!(failure.code, Some("UND_ERR_NOT_SUPPORTED"));
            }
            _ => panic!("an unsupported proxy is a transport failure, not a TypeError"),
        }
    }
}
