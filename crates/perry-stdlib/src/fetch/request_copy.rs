//! Preserve Request identity until construction, including init overrides.
use super::*;
use perry_runtime::gc::RuntimeHandleScope;

/// Construct from the original JS input, rather than its URL alone (#10380).
///
/// # Safety
/// Both arguments must be valid NaN-boxed JS values.
#[no_mangle]
pub unsafe extern "C" fn js_request_new_from_input(input: f64, init: f64) -> f64 {
    let _fetch_roots = lifecycle::pin_handles(&[input, init]);
    let scope = RuntimeHandleScope::new();
    let input_root = scope.root_nanbox_f64(input);
    let init_root = scope.root_nanbox_f64(init);
    // Subclasses carry their native Request in a hidden field. Unwrapping
    // can allocate, so keep the wrapper and init rooted first; then pin the
    // recovered registry handle for the rest of construction.
    let native_input = perry_runtime::object::js_fetch_unwrap_handle(input_root.get_nanbox_f64());
    let _source_pin = lifecycle::pin_handles(&[native_input]);
    let source_id = handle_id(native_input);
    let source = REQUEST_REGISTRY.lock().unwrap().get(&source_id).cloned();
    let Some(mut request) = source else {
        let url = js_request_input_to_url(input_root.get_nanbox_f64());
        return js_request_new_from_init(url, init_root.get_nanbox_f64());
    };
    let signal_root = scope.root_nanbox_f64(request.signal);
    request.headers = request_headers_snapshot(&request);
    request.cached_headers_id = None;
    request.body_used = false;

    // Re-read the rooted init after allocating the key: a getter may allocate
    // or run user JS, and the following field read must see its relocated value.
    let has_init_member = Cell::new(false);
    let field = |name: &[u8]| -> f64 {
        if matches!(
            init_root.get_nanbox_f64().to_bits(),
            TAG_UNDEFINED | TAG_NULL
        ) {
            return f64::from_bits(TAG_UNDEFINED);
        }
        let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let key = perry_runtime::value::js_nanbox_string(key as i64);
        let value =
            perry_runtime::object::js_object_get_property_key(init_root.get_nanbox_f64(), key);
        if value.to_bits() != TAG_UNDEFINED {
            has_init_member.set(true);
        }
        value
    };
    let string_field = |name: &[u8]| -> Option<String> {
        let value = field(name);
        if value.to_bits() == TAG_UNDEFINED {
            return None;
        }
        let ptr = perry_runtime::value::js_jsvalue_to_string(value);
        string_from_header(ptr)
    };

    // Read each init member once. An absent (or undefined) member inherits;
    // an explicit headers value replaces the whole header list.
    let body_root = scope.root_nanbox_f64(field(b"body"));
    if let Some(value) = string_field(b"cache") {
        request.cache = value;
    }
    if let Some(value) = string_field(b"credentials") {
        request.credentials = value;
    }
    if let Some(value) = string_field(b"duplex") {
        request.duplex = value;
    }
    let headers = field(b"headers");
    if headers.to_bits() != TAG_UNDEFINED {
        let headers_root = scope.root_nanbox_f64(headers);
        let handle = js_headers_new();
        let _headers_pin = lifecycle::pin_handles(&[handle]);
        js_headers_init_from_value(handle, headers_root.get_nanbox_f64());
        request.headers = HEADERS_REGISTRY
            .lock()
            .unwrap()
            .get(&handle_id(handle))
            .map(|h| h.store.clone())
            .unwrap_or_default();
    }
    if let Some(value) = string_field(b"integrity") {
        request.integrity = value;
    }
    let keepalive = field(b"keepalive");
    if keepalive.to_bits() != TAG_UNDEFINED {
        request.keepalive = body_metadata::bool_from_js(keepalive);
    }
    if let Some(value) = string_field(b"method") {
        if is_forbidden_method(&value.to_ascii_uppercase()) {
            throw_fetch_type_error(&format!("'{value}' HTTP method is unsupported."));
        }
        request.method = normalize_method(&value);
    }
    if let Some(value) = string_field(b"mode") {
        request.mode = value;
    }
    if let Some(value) = string_field(b"redirect") {
        request.redirect = value;
    }
    let referrer = string_field(b"referrer");
    let referrer_policy = string_field(b"referrerPolicy");
    let signal = field(b"signal");
    if signal.to_bits() != TAG_UNDEFINED {
        signal_root.set_nanbox_f64(body_metadata::signal_or_default(signal));
    }

    if has_init_member.get() {
        request.referrer = referrer.unwrap_or_else(|| "about:client".to_string());
        request.referrer_policy = referrer_policy.unwrap_or_default();
    }

    let has_override_body = !matches!(
        body_root.get_nanbox_f64().to_bits(),
        TAG_UNDEFINED | TAG_NULL
    );
    if (request.body.is_some() || has_override_body)
        && matches!(request.method.as_str(), "GET" | "HEAD")
    {
        throw_fetch_type_error("Request with GET/HEAD method cannot have body.");
    }
    if has_override_body {
        let form_data = body_metadata::serialize_form_data(handle_id(body_root.get_nanbox_f64()));
        let content_type = if let Some((bytes, content_type)) = form_data {
            request.body = Some(bytes);
            Some(content_type)
        } else {
            let ptr = js_response_body_init_ptr(body_root.get_nanbox_f64()) as *const StringHeader;
            let stream = take_pending_fetch_body_stream_id();
            let content_type = take_pending_fetch_body_content_type().map(str::to_owned);
            // Non-body handles (e.g. Headers) are synthetic addresses, not
            // StringHeaders. Match the URL constructor's guarded fallback.
            // Copy real bytes before draining: a stream pull may allocate.
            let bytes = if perry_runtime::value::addr_class::is_handle_band(ptr as usize) {
                None
            } else {
                dispatch::body_bytes_from_header(ptr)
            };
            request.body = stream
                .map(crate::streams::drain_readable_into_bytes)
                .or(bytes);
            content_type
        };
        if let Some(content_type) = content_type {
            if !request.headers.has("content-type") {
                request.headers.set("content-type", &content_type);
            }
        }
    } else if request.body.is_some() {
        // Transfer only after validation. An override body leaves the input's
        // body untouched; inheriting consumes it, including an empty body.
        let used = {
            let mut registry = REQUEST_REGISTRY.lock().unwrap();
            let source = registry.get_mut(&source_id).expect("pinned Request");
            let used = source.body_used;
            if !used {
                source.body_used = true;
            }
            used
        };
        if used {
            throw_fetch_type_error(
                "Cannot construct a Request with a Request object that has already been used.",
            );
        }
    }
    request.signal = signal_root.get_nanbox_f64();
    let id = alloc_fetch_handle_id();
    gc::ensure_gc_registered();
    REQUEST_REGISTRY.lock().unwrap().insert(id, request);
    handle_to_f64(id)
}
