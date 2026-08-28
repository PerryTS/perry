use super::*;

pub(super) fn alloc_response(
    status: u16,
    status_text: String,
    headers: HeadersStore,
    body: Vec<u8>,
    body_present: bool,
) -> usize {
    let id = alloc_fetch_handle_id();
    FETCH_RESPONSES.lock().unwrap().insert(
        id,
        FetchResponse {
            status,
            status_text,
            headers,
            body,
            body_present,
            body_used: false,
            type_name: "default".to_string(),
            url: String::new(),
            redirected: false,
            cached_headers_id: None,
            cached_body_stream_id: None,
            body_stream_id: None,
        },
    );
    id
}

/// new Response(body, statusOpt, statusTextPtrOpt, headersHandleOpt)
/// - body_ptr: StringHeader for the body, or null for ""
/// - status: f64 (200 default)
/// - status_text_ptr: StringHeader for statusText, or null for ""
/// - headers_handle: f64 numeric handle from js_headers_new, or 0
#[no_mangle]
pub unsafe extern "C" fn js_response_new(
    body_ptr: *const StringHeader,
    status: f64,
    status_text_ptr: *const StringHeader,
    headers_handle: f64,
) -> f64 {
    let body_stream_id = take_pending_fetch_body_stream_id();
    // Consume before validation so a throwing constructor cannot leak body
    // metadata into the next Response construction on this thread.
    let body_content_type = take_pending_fetch_body_content_type();
    // Lossless raw-byte read so binary bodies survive byte-for-byte (#5435).
    let body_opt = dispatch::body_bytes_from_header(body_ptr);
    let body_present = body_opt.is_some() || body_stream_id.is_some();
    let body = body_opt.unwrap_or_default();
    // NaN / 0.0 are the codegen "no status field" sentinels. Node defaults
    // missing status to 200; any explicit value is truncated toward zero
    // then range-checked against 200..=599 (199.9 → RangeError, 599.9 →
    // 599). Refs #2640.
    let status_u16 = if status.is_nan() || status == 0.0 {
        200
    } else {
        let truncated = status.trunc();
        if !(200.0..=599.0).contains(&truncated) {
            throw_fetch_range_error(
                "init[\"status\"] must be in the range of 200 to 599, inclusive.",
            );
        }
        truncated as u16
    };
    // Node defaults statusText to the empty string (NOT the canonical
    // reason phrase) and validates the reason-phrase token. Refs #2640.
    let status_text = match string_from_header(status_text_ptr) {
        Some(s) => {
            if !is_valid_status_text(&s) {
                throw_fetch_type_error("Invalid statusText");
            }
            s
        }
        None => String::new(),
    };
    if body_present && is_null_body_status(status_u16) {
        throw_fetch_type_error(&format!(
            "Response constructor: Invalid response status code {status_u16}"
        ));
    }
    let headers_id = handle_id(headers_handle);
    let registered = (headers_id != 0)
        .then(|| HEADERS_REGISTRY.lock().unwrap().get(&headers_id).cloned())
        .flatten();
    // Non-literal Response init objects can deliver a plain HeadersInit value
    // here. Preserve those records instead of treating them as missing handles.
    let mut headers = registered
        .or_else(|| headers_store_from_record_value(headers_handle))
        .unwrap_or_default();
    if let Some(content_type) = body_content_type {
        if headers.get("content-type").is_none() {
            headers.set("content-type", content_type);
        }
    }
    // A Response owns a private Headers list. The constructor input may be an
    // existing Headers object, so retaining its registry id would make
    // mutations alias in both directions instead of copying the initializer.
    let response_headers_id = (headers_id != 0).then(|| alloc_headers(headers.clone()));
    let id = alloc_response(status_u16, status_text, headers, body, body_present);
    if response_headers_id.is_some() || body_stream_id.is_some() {
        if let Some(resp) = FETCH_RESPONSES.lock().unwrap().get_mut(&id) {
            if let Some(headers_id) = response_headers_id {
                resp.cached_headers_id = Some(headers_id);
            }
            if let Some(stream_id) = body_stream_id {
                resp.body_stream_id = Some(stream_id);
                resp.cached_body_stream_id = Some(stream_id);
            }
        }
    }
    handle_to_f64(id)
}

/// response.headers — returns a Headers handle (f64). Lazily allocates a Headers entry
/// from the response's stored header HashMap if one doesn't exist yet.
#[no_mangle]
pub extern "C" fn js_response_get_headers(handle: f64) -> f64 {
    let id = handle_id(handle);
    response_headers_handle(id)
}

/// response.clone() — duplicates the response (deep copy of body + headers)
#[no_mangle]
pub extern "C" fn js_response_clone(handle: f64) -> f64 {
    let id = handle_id(handle);
    let cloned = {
        let mut guard = FETCH_RESPONSES.lock().unwrap();
        guard.get_mut(&id).map(|resp| {
            if resp.body_present && resp.body_used {
                unsafe {
                    throw_fetch_type_error("Response.clone: Body has already been consumed.")
                };
            }
            let cloned_stream_id = resp.body_stream_id.map(|stream_id| {
                let (original, cloned) =
                    unsafe { crate::streams::tee_readable_stream_ids(stream_id) };
                resp.body_stream_id = Some(original);
                resp.cached_body_stream_id = Some(original);
                cloned
            });
            FetchResponse {
                status: resp.status,
                status_text: resp.status_text.clone(),
                headers: response_headers_snapshot(resp),
                body: resp.body.clone(),
                body_present: resp.body_present,
                body_used: false,
                type_name: resp.type_name.clone(),
                url: resp.url.clone(),
                redirected: resp.redirected,
                cached_headers_id: None,
                cached_body_stream_id: cloned_stream_id,
                body_stream_id: cloned_stream_id,
            }
        })
    };
    if let Some(new_resp) = cloned {
        let new_id = alloc_fetch_handle_id();
        FETCH_RESPONSES.lock().unwrap().insert(new_id, new_resp);
        return handle_to_f64(new_id);
    }
    f64::from_bits(TAG_UNDEFINED)
}
