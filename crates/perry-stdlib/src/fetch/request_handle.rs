//! Helpers for the `fetch(Request)` form — recovering a `Request` object's
//! url/method/body/headers from the registry and resolving the final fetch
//! inputs (with `init` overrides). Split out of `mod.rs` to keep it small.

use std::collections::HashMap;

/// Resolved inputs for a fetch call, after applying any `init` overrides over a
/// `Request` object's own fields.
pub(crate) struct FetchInputs {
    pub url: String,
    pub method: String,
    /// Request body as raw bytes — preserved byte-for-byte so binary payloads
    /// are never corrupted by a lossy UTF-8 round-trip.
    pub body: Option<Vec<u8>>,
    pub custom_headers: HashMap<String, String>,
}

/// Fields recovered from a `Request` object for the `fetch(Request)` form.
struct RequestFetchFields {
    url: String,
    method: String,
    body: Option<Vec<u8>>,
    headers: HashMap<String, String>,
}

/// Recover url/method/body/headers from a live `Request` handle, or `None` when
/// the id isn't a live Request handle (e.g. a genuinely bad/undefined first arg).
fn request_fields_from_handle(maybe_handle: usize) -> Option<RequestFetchFields> {
    let guard = super::REQUEST_REGISTRY.lock().unwrap();
    let req = guard.get(&maybe_handle)?;
    let headers = req
        .headers
        .entries
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    Some(RequestFetchFields {
        url: req.url.clone(),
        method: req.method.clone(),
        // Keep the body as raw bytes; never lossy-convert to `String` (which
        // would replace invalid UTF-8 with U+FFFD and corrupt binary bodies).
        body: req.body.clone(),
        headers,
    })
}

/// Resolve the final fetch inputs from the pre-extracted `init` header strings,
/// recovering Request-object fields for the `fetch(Request)` form. `init`
/// members win over the Request's own fields (the WHATWG rule). Returns
/// `Err(error_bits)` for a missing/invalid URL.
pub(crate) fn resolve_fetch_inputs(
    url_from_header: Option<String>,
    method_from_header: Option<String>,
    body_bytes: Option<Vec<u8>>,
    headers_json: Option<String>,
    url_handle: usize,
) -> Result<FetchInputs, u64> {
    let request_fields = if url_from_header.is_none() {
        request_fields_from_handle(url_handle)
    } else {
        None
    };

    let url = match url_from_header {
        Some(u) => u,
        None => match request_fields.as_ref() {
            Some(rf) => rf.url.clone(),
            None => return Err(unsafe { super::fetch_error_bits("Invalid URL") }),
        },
    };

    let method = method_from_header
        .or_else(|| request_fields.as_ref().map(|rf| rf.method.clone()))
        .unwrap_or_else(|| "GET".to_string());

    // `body_bytes` is already raw bytes (binary bodies preserved byte-for-byte by
    // the caller's buffer/typed-array probe; string bodies read as their UTF-8
    // bytes), so use it as-is and only fall back to the Request's own body.
    let body = body_bytes.or_else(|| request_fields.as_ref().and_then(|rf| rf.body.clone()));

    // Start from the `Request`'s own headers (the `fetch(Request)` form), then
    // let any `init.headers` override per-key. WHATWG says init headers win, but
    // an absent/empty init must NOT wipe the Request's headers — codegen always
    // passes a headers JSON ("{}" when the init has none), so the old strict
    // `match` (Some("{}") => empty) silently dropped every header axios sets on
    // its `Request` (auth token, anthropic-version, …), causing 401s.
    let mut custom_headers: HashMap<String, String> = request_fields
        .as_ref()
        .map(|rf| rf.headers.clone())
        .unwrap_or_default();
    if let Some(j) = headers_json {
        if let Ok(init_headers) = serde_json::from_str::<HashMap<String, String>>(&j) {
            // `request_fields.headers` are stored canonically lowercased (the
            // Headers object lowercases keys), but `serde_json` preserves the
            // JSON's original casing. Lowercase the init keys before merging so
            // an init `Authorization` actually overrides the Request's
            // `authorization` instead of both surviving and being forwarded.
            custom_headers.extend(
                init_headers
                    .into_iter()
                    .map(|(k, v)| (k.to_ascii_lowercase(), v)),
            );
        }
    }

    Ok(FetchInputs {
        url,
        method,
        body,
        custom_headers,
    })
}

#[cfg(test)]
mod tests {
    use super::resolve_fetch_inputs;

    #[test]
    fn init_headers_are_lowercased_so_they_override() {
        // `url_from_header` set => the Request registry is not consulted, so
        // this exercises only the `init.headers` merge path. The init keys must
        // be lowercased so they collapse onto (and override) the canonical
        // lowercase keys the Request's Headers object stores.
        let inputs = resolve_fetch_inputs(
            Some("https://example.com/".to_string()),
            Some("GET".to_string()),
            None,
            Some(r#"{"Authorization":"Bearer tok","Content-Type":"application/json"}"#.to_string()),
            0,
        )
        .expect("inputs resolve");

        assert_eq!(
            inputs
                .custom_headers
                .get("authorization")
                .map(String::as_str),
            Some("Bearer tok"),
        );
        assert_eq!(
            inputs
                .custom_headers
                .get("content-type")
                .map(String::as_str),
            Some("application/json"),
        );
        // The original mixed-case keys must not survive as duplicates.
        assert!(!inputs.custom_headers.contains_key("Authorization"));
        assert!(!inputs.custom_headers.contains_key("Content-Type"));
    }
}
