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
    body_from_header: Option<String>,
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

    let body = body_from_header
        .map(String::into_bytes)
        .or_else(|| request_fields.as_ref().and_then(|rf| rf.body.clone()));

    let custom_headers: HashMap<String, String> = match headers_json {
        Some(j) => serde_json::from_str(&j).unwrap_or_default(),
        None => request_fields
            .as_ref()
            .map(|rf| rf.headers.clone())
            .unwrap_or_default(),
    };

    Ok(FetchInputs {
        url,
        method,
        body,
        custom_headers,
    })
}
