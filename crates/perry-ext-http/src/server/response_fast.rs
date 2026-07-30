//! Response-build fast path for HTTP/1.
//!
//! The per-response head build runs for *every* response a perry HTTP/1
//! server emits, so the small-response RPS hot path is dominated by the
//! constant work this module precomputes:
//!
//! - **`status_code_const`** — maps the common status codes to their
//!   pre-validated [`StatusCode`] associated constant, so `into_hyper`
//!   skips `StatusCode::from_u16`'s numeric range-check + `unwrap_or` on
//!   every response. Uncommon / custom codes fall back to the parsing
//!   path, so the resulting status is identical for every code.
//! - **`keep_alive_header_value`** — interns the `Keep-Alive: timeout=N`
//!   value for the timeouts a server actually runs with (Node's 5 s
//!   default, plus 0/10/30/60/120 s), so the per-response `format!` only
//!   fires for an unusual timeout. The string produced is byte-identical
//!   to `format!("timeout={}", secs)`.
//! - **`status_line_bytes`** — precomputed `b"HTTP/1.1 <code> <reason>\r\n"`
//!   slices for the common codes, used by the standalone (`assignSocket`)
//!   flush so it skips `format!("HTTP/1.1 {} {}\r\n", code, reason)` when
//!   there is no custom `statusMessage`. The bytes equal exactly what the
//!   `format!` produced for the same `(code, canonical reason)` pair.
//!
//! Every helper is a pure lookup that returns `None` (or falls through)
//! for any input the slow path would have handled differently, so the
//! bytes on the wire are unchanged. The fast path is a shortcut for the
//! common case, never a replacement for the general one.

use hyper::StatusCode;

/// The HTTP status codes a typical server emits often enough to be worth
/// a const shortcut. Anything outside this set takes the `from_u16` path.
///
/// Returns the pre-validated [`StatusCode`] constant for `code`, or `None`
/// to signal the caller should fall back to `StatusCode::from_u16(code)`.
/// The constant is value-identical to `StatusCode::from_u16(code).unwrap()`
/// for every code listed, so the response status is unchanged.
#[inline]
pub(crate) fn status_code_const(code: u16) -> Option<StatusCode> {
    Some(match code {
        200 => StatusCode::OK,
        201 => StatusCode::CREATED,
        202 => StatusCode::ACCEPTED,
        204 => StatusCode::NO_CONTENT,
        206 => StatusCode::PARTIAL_CONTENT,
        301 => StatusCode::MOVED_PERMANENTLY,
        302 => StatusCode::FOUND,
        303 => StatusCode::SEE_OTHER,
        304 => StatusCode::NOT_MODIFIED,
        307 => StatusCode::TEMPORARY_REDIRECT,
        308 => StatusCode::PERMANENT_REDIRECT,
        400 => StatusCode::BAD_REQUEST,
        401 => StatusCode::UNAUTHORIZED,
        403 => StatusCode::FORBIDDEN,
        404 => StatusCode::NOT_FOUND,
        405 => StatusCode::METHOD_NOT_ALLOWED,
        409 => StatusCode::CONFLICT,
        410 => StatusCode::GONE,
        429 => StatusCode::TOO_MANY_REQUESTS,
        500 => StatusCode::INTERNAL_SERVER_ERROR,
        502 => StatusCode::BAD_GATEWAY,
        503 => StatusCode::SERVICE_UNAVAILABLE,
        504 => StatusCode::GATEWAY_TIMEOUT,
        _ => return None,
    })
}

/// The interned `Keep-Alive: timeout=N` *value* for the keep-alive
/// timeouts servers commonly run with. `secs` is the already-floored
/// `keepAliveTimeout / 1000`, matching `apply_default_connection_headers`.
///
/// Returns a `&'static str` equal to `format!("timeout={}", secs)` for the
/// listed values, or `None` to fall back to the `format!` allocation.
#[inline]
pub(crate) fn keep_alive_header_value(secs: u64) -> Option<&'static str> {
    Some(match secs {
        0 => "timeout=0",
        5 => "timeout=5", // Node's default keepAliveTimeout (5_000 ms).
        10 => "timeout=10",
        15 => "timeout=15",
        30 => "timeout=30",
        60 => "timeout=60",
        120 => "timeout=120",
        _ => return None,
    })
}

/// Precomputed `HTTP/1.1 <code> <reason>\r\n` status line for the common
/// codes, as the exact bytes `format!("HTTP/1.1 {} {}\r\n", code, reason)`
/// would have produced for that code and its canonical reason phrase.
///
/// Used by the standalone (`assignSocket`) flush, which hand-builds the
/// HTTP/1 head. Returns `None` for any code not listed, or when the
/// response carries a custom `statusMessage` — both fall back to the
/// `format!` path so a custom reason phrase still reaches the wire.
#[inline]
pub(crate) fn status_line_bytes(code: u16) -> Option<&'static str> {
    Some(match code {
        200 => "HTTP/1.1 200 OK\r\n",
        201 => "HTTP/1.1 201 Created\r\n",
        202 => "HTTP/1.1 202 Accepted\r\n",
        204 => "HTTP/1.1 204 No Content\r\n",
        206 => "HTTP/1.1 206 Partial Content\r\n",
        301 => "HTTP/1.1 301 Moved Permanently\r\n",
        302 => "HTTP/1.1 302 Found\r\n",
        303 => "HTTP/1.1 303 See Other\r\n",
        304 => "HTTP/1.1 304 Not Modified\r\n",
        307 => "HTTP/1.1 307 Temporary Redirect\r\n",
        308 => "HTTP/1.1 308 Permanent Redirect\r\n",
        400 => "HTTP/1.1 400 Bad Request\r\n",
        401 => "HTTP/1.1 401 Unauthorized\r\n",
        403 => "HTTP/1.1 403 Forbidden\r\n",
        404 => "HTTP/1.1 404 Not Found\r\n",
        405 => "HTTP/1.1 405 Method Not Allowed\r\n",
        409 => "HTTP/1.1 409 Conflict\r\n",
        410 => "HTTP/1.1 410 Gone\r\n",
        429 => "HTTP/1.1 429 Too Many Requests\r\n",
        500 => "HTTP/1.1 500 Internal Server Error\r\n",
        502 => "HTTP/1.1 502 Bad Gateway\r\n",
        503 => "HTTP/1.1 503 Service Unavailable\r\n",
        504 => "HTTP/1.1 504 Gateway Timeout\r\n",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every interned status code must equal the value the slow path
    /// (`StatusCode::from_u16`) produces — a divergence would change the
    /// status line on the wire.
    #[test]
    fn status_code_const_matches_from_u16() {
        for code in 0u16..=999 {
            match status_code_const(code) {
                Some(fast) => {
                    let slow = StatusCode::from_u16(code)
                        .unwrap_or_else(|_| panic!("interned {code} is not a valid status code"));
                    assert_eq!(
                        fast, slow,
                        "interned StatusCode for {code} diverges from from_u16"
                    );
                }
                None => {} // falls back to from_u16 — nothing to check.
            }
        }
    }

    /// The interned `Keep-Alive` value must be byte-identical to the
    /// `format!` the slow path used.
    #[test]
    fn keep_alive_value_matches_format() {
        for secs in [0u64, 5, 10, 15, 30, 60, 120] {
            assert_eq!(
                keep_alive_header_value(secs),
                Some(format!("timeout={secs}").as_str())
            );
        }
        // An unusual timeout falls back to the format! path.
        assert_eq!(keep_alive_header_value(7), None);
    }

    /// Each precomputed status line must equal exactly the bytes
    /// `format!("HTTP/1.1 {} {}\r\n", code, reason)` produced, where
    /// `reason` is the canonical reason phrase hyper would also emit.
    #[test]
    fn status_line_bytes_match_format_with_canonical_reason() {
        for code in 0u16..=999 {
            let Some(fast) = status_line_bytes(code) else {
                continue;
            };
            let reason = StatusCode::from_u16(code)
                .ok()
                .and_then(|s| s.canonical_reason())
                .unwrap_or_else(|| panic!("interned status line {code} has no canonical reason"));
            let slow = format!("HTTP/1.1 {code} {reason}\r\n");
            assert_eq!(
                fast, slow,
                "precomputed status line for {code} diverges from format!"
            );
        }
    }

    /// A code with no interned form returns `None` so the caller keeps the
    /// general `format!` / `from_u16` path (custom codes still work).
    #[test]
    fn uncommon_codes_fall_back() {
        assert_eq!(status_code_const(418), None); // I'm a teapot — not interned.
        assert_eq!(status_line_bytes(418), None);
        assert_eq!(status_code_const(599), None);
        assert_eq!(status_line_bytes(599), None);
    }
}
