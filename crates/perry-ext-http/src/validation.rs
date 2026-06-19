//! Node-compatible argument validation for `http.request` / `http.get`
//! (and the `https` twins). Node throws on bad method / path / headers /
//! protocol / URL / option types *before* a connection is attempted; Perry
//! previously accepted the bad input silently, so the corpus'
//! `assert.throws(...)` cases failed with "Missing expected exception"
//! (#4907).
//!
//! Each helper either returns normally or `js_throw`s a `TypeError` carrying
//! the matching `ERR_*` code (the same mechanism `agent.rs` uses for
//! `ERR_OUT_OF_RANGE`). Throwing unwinds through the codegen call site back
//! to the JS `try` / `assert.throws` frame.

use perry_runtime::fs::validate::throw_type_error_with_code;

/// Node HTTP token bytes (RFC 7230 `tchar`, mirrored from
/// `lib/_http_common.js` `tokenRegExp`). Used for both method names and
/// header field names.
fn is_token_byte(b: u8) -> bool {
    matches!(b,
        b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9'
        | b'!' | b'#' | b'$' | b'%' | b'&' | b'\'' | b'*'
        | b'+' | b'-' | b'.' | b'^' | b'_' | b'`' | b'|' | b'~')
}

fn is_valid_token(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(is_token_byte)
}

/// `http.request` / `get` with a string argument treats it as a WHATWG URL:
/// `url = urlToHttpOptions(new URL(input))`. A bare hostname
/// (`'www.nodejs.org'`), a scheme-less address (`'127.0.0.1'`), or a
/// host-less URL (`'http://:80/'`) all make `new URL(...)` throw
/// `TypeError [ERR_INVALID_URL]`. Mirror that here, before Perry's lenient
/// `"{proto}://{raw}"` prepend (#769) would otherwise paper over it.
pub(crate) fn validate_client_url_string(raw: &str) {
    let invalid = match reqwest::Url::parse(raw) {
        Ok(u) => u.host_str().map(|h| h.is_empty()).unwrap_or(true),
        Err(_) => true,
    };
    if invalid {
        throw_type_error_with_code("Invalid URL", "ERR_INVALID_URL");
    }
}

/// Validate the options bag shared by `http.request` / `http.get` /
/// `new ClientRequest(...)`. `default_protocol` is `"http"` or `"https"`,
/// identifying which factory was called (the protocol must match it).
pub(crate) fn validate_client_options(opts: &serde_json::Value, default_protocol: &str) {
    let obj = match opts.as_object() {
        Some(o) => o,
        None => return,
    };

    // `insecureHTTPParser` must be a boolean when present (Node:
    // `validateBoolean(insecureHTTPParser, 'options.insecureHTTPParser')`).
    if let Some(v) = obj.get("insecureHTTPParser") {
        if !v.is_boolean() && !v.is_null() {
            throw_type_error_with_code(
                "The \"options.insecureHTTPParser\" property must be of type boolean.",
                "ERR_INVALID_ARG_TYPE",
            );
        }
    }

    // `timeout`, when present, must be a number (Node:
    // `validateNumber(timeout, 'timeout')`). `timeout: null` throws.
    if let Some(v) = obj.get("timeout") {
        if !v.is_number() {
            throw_type_error_with_code(
                "The \"timeout\" argument must be of type number.",
                "ERR_INVALID_ARG_TYPE",
            );
        }
    }

    // `protocol`, when present, must match the factory's protocol. Node's
    // `http.request` agent only speaks `http:`, `https.request` only
    // `https:`; anything else (incl. `mailto:` / `ftp:` from `url.parse`)
    // throws `ERR_INVALID_PROTOCOL`.
    if let Some(proto) = obj.get("protocol").and_then(|v| v.as_str()) {
        let normalized = format!("{}:", proto.trim_end_matches(':'));
        let expected = format!("{default_protocol}:");
        if normalized != expected {
            throw_type_error_with_code(
                &format!("Protocol \"{normalized}\" not supported. Expected \"{expected}\""),
                "ERR_INVALID_PROTOCOL",
            );
        }
    }

    // `method`, when a *non-empty* string, must be a valid HTTP token. The
    // error message quotes the *raw* (non-upper-cased) method, matching
    // `validateHttpToken(method, 'Method')`. Node only validates a truthy
    // method (`if (methodIsString && method)` in lib/_http_client.js); a
    // falsy one (`''`, `undefined`, `null`) falls through to the `'GET'`
    // default instead of throwing (#4970).
    if let Some(method) = obj.get("method").and_then(|v| v.as_str()) {
        if !method.is_empty() && !is_valid_token(method) {
            throw_type_error_with_code(
                &format!("Method must be a valid HTTP token [\"{method}\"]"),
                "ERR_INVALID_HTTP_TOKEN",
            );
        }
    }

    // `path` must not contain unescaped characters: Node rejects anything
    // outside `!-ÿ` (`INVALID_PATH_REGEX`).
    if let Some(path) = obj.get("path").and_then(|v| v.as_str()) {
        if path.chars().any(|c| {
            let cp = c as u32;
            !(0x21..=0xff).contains(&cp)
        }) {
            throw_type_error_with_code(
                "Request path contains unescaped characters",
                "ERR_UNESCAPED_CHARACTERS",
            );
        }
    }

    // Header field names must be valid HTTP tokens, and the `Host` header may
    // not be an array (Node computes a single Host line from it).
    if let Some(headers) = obj.get("headers").and_then(|v| v.as_object()) {
        for (name, value) in headers {
            if name.eq_ignore_ascii_case("host") && value.is_array() {
                throw_type_error_with_code(
                    "The \"options.headers.host\" property must be of type string.",
                    "ERR_INVALID_ARG_TYPE",
                );
            }
            if !is_valid_token(name) {
                throw_type_error_with_code(
                    &format!("Header name must be a valid HTTP token [\"{name}\"]"),
                    "ERR_INVALID_HTTP_TOKEN",
                );
            }
        }
    }
}
