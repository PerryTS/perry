//! Convert reqwest transport failures to the error shape exposed by Node fetch.

use std::error::Error as StdError;

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
    pub(crate) fn from_reqwest(error: &reqwest::Error, url: &str) -> Self {
        let mut chain = String::new();
        let mut cause_message = error.to_string();
        let mut current: Option<&(dyn StdError + 'static)> = Some(error);
        while let Some(source) = current {
            let text = source.to_string();
            if !text.is_empty() {
                cause_message = text.clone();
            }
            chain.push_str(&text.to_ascii_lowercase());
            chain.push(' ');
            current = source.source();
        }
        Self::classify(url, &chain, cause_message)
    }

    /// The turnloop client engine's failure, which already carries Node's
    /// `cause.code` and `syscall` rather than a prose chain to classify.
    /// `getaddrinfo ENOTFOUND` keeps the `errno` and `hostname` Node reports,
    /// because that is the one shape callers match on (the reqwest path
    /// synthesized the same triple).
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

    fn classify(url: &str, chain: &str, fallback_message: String) -> Self {
        let hostname = reqwest::Url::parse(url)
            .ok()
            .and_then(|parsed| parsed.host_str().map(str::to_owned));
        if looks_like_dns_failure(chain) {
            let hostname = hostname.unwrap_or_default();
            return Self {
                cause_message: format!("getaddrinfo ENOTFOUND {hostname}"),
                code: Some("ENOTFOUND"),
                errno: Some(-3008),
                syscall: Some("getaddrinfo"),
                hostname: Some(hostname),
                cause_name: None,
            };
        }
        Self {
            cause_message: fallback_message,
            code: None,
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

fn looks_like_dns_failure(chain: &str) -> bool {
    chain.contains("dns error")
        || chain.contains("failed to lookup address")
        || chain.contains("failed to lookup")
        || chain.contains("name or service not known")
        || chain.contains("nodename nor servname")
        || chain.contains("no such host")
        || chain.contains("name resolution")
        || chain.contains("name not resolved")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dns_failure_has_node_fetch_cause_diagnostics() {
        let failure = FetchFailure::classify(
            "https://example.invalid/mcp",
            "error sending request dns error failed to lookup address information",
            "opaque reqwest error".to_string(),
        );
        assert_eq!(
            failure.cause_message,
            "getaddrinfo ENOTFOUND example.invalid"
        );
        assert_eq!(failure.code, Some("ENOTFOUND"));
        assert_eq!(failure.errno, Some(-3008));
        assert_eq!(failure.syscall, Some("getaddrinfo"));
        assert_eq!(failure.hostname.as_deref(), Some("example.invalid"));
    }

    #[test]
    fn unclassified_failure_keeps_deepest_source_message() {
        let failure = FetchFailure::classify(
            "https://example.com/",
            "opaque transport failure",
            "opaque transport failure".to_string(),
        );
        assert_eq!(failure.cause_message, "opaque transport failure");
        assert_eq!(failure.code, None);
        assert_eq!(failure.errno, None);
        assert_eq!(failure.syscall, None);
        assert_eq!(failure.hostname, None);
    }
}
