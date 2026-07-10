//! Web Fetch `Request` constructors — split out of `mod.rs` to keep it under
//! the 2,000-line lint gate (#5458). The child module sees mod.rs's private
//! items (registries, `string_from_header`, the validation helpers, the
//! `TAG_*` consts, and the re-exported `Headers` FFI) via its `use super::*`,
//! the same contract used by `headers` / `dispatch` / `body_metadata`.

use super::*;

/// new Request(url, methodOpt, bodyOpt, headersHandleOpt)
///
/// # Safety
/// All `*const StringHeader` arguments must be null or valid string headers;
/// `headers_handle` must be 0 or a live Headers registry id. Called only from
/// codegen-emitted FFI.
#[no_mangle]
pub unsafe extern "C" fn js_request_new(
    url_ptr: *const StringHeader,
    method_ptr: *const StringHeader,
    body_ptr: *const StringHeader,
    headers_handle: f64,
    referrer_ptr: *const StringHeader,
    referrer_policy_ptr: *const StringHeader,
    mode_ptr: *const StringHeader,
    credentials_ptr: *const StringHeader,
    cache_ptr: *const StringHeader,
    redirect_ptr: *const StringHeader,
    integrity_ptr: *const StringHeader,
    keepalive: f64,
    duplex_ptr: *const StringHeader,
    signal: f64,
) -> f64 {
    let url = string_from_header(url_ptr).unwrap_or_default();
    let raw_method = string_from_header(method_ptr).unwrap_or_else(|| "GET".to_string());
    // Forbidden methods are rejected case-insensitively; the error message
    // preserves the caller's original casing (Node parity). Refs #2643.
    if is_forbidden_method(&raw_method.to_ascii_uppercase()) {
        throw_fetch_type_error(&format!("'{raw_method}' HTTP method is unsupported."));
    }
    let method = normalize_method(&raw_method);
    // A Buffer / Uint8Array / typed-array / ArrayBuffer body reaches us as a
    // BufferHeader/TypedArrayHeader pointer (codegen ran the value through
    // `js_get_string_pointer_unified`), NOT a StringHeader — the same for both
    // the static-literal path and `js_request_new_from_init`. Reading it via
    // `string_from_header` took the byte length off the right field but the data
    // off the StringHeader data offset (20) instead of the buffer data offset
    // (8), shifting every binary body left by 12 bytes (#5483). Probe the
    // typed-array/buffer registries first and copy the real bytes verbatim; a
    // genuine string body falls through to the lossless StringHeader read so its
    // UTF-8 bytes are preserved.
    // A Blob / File body is a handle-band id (>= 0x40000), NOT a real
    // StringHeader/Buffer pointer, so it must be resolved via the blob registry
    // first: reading it through `body_bytes_from_header` would dereference the
    // synthetic id → SIGSEGV (#6231, the Request-constructor twin).
    let body: Option<Vec<u8>> =
        if perry_runtime::value::addr_class::is_handle_band(body_ptr as usize) {
            crate::fetch::blob_bytes_clone(body_ptr as usize)
        } else {
            dispatch::body_addr_buffer_bytes(body_ptr as usize)
                .or_else(|| dispatch::body_bytes_from_header(body_ptr))
        };
    // GET/HEAD requests may not carry a body (WHATWG fetch). Refs #2643.
    if body.is_some() && (method == "GET" || method == "HEAD") {
        throw_fetch_type_error("Request with GET/HEAD method cannot have body.");
    }
    let headers_id_in = handle_id(headers_handle);
    let headers = if headers_id_in != 0 {
        HEADERS_REGISTRY
            .lock()
            .unwrap()
            .get(&headers_id_in)
            .cloned()
            .unwrap_or_default()
    } else {
        HeadersStore::default()
    };
    let id = alloc_fetch_handle_id();
    REQUEST_REGISTRY.lock().unwrap().insert(
        id,
        RequestRecord {
            url,
            method,
            body,
            body_used: false,
            headers,
            destination: String::new(),
            referrer: string_from_header(referrer_ptr)
                .unwrap_or_else(|| "about:client".to_string()),
            referrer_policy: string_from_header(referrer_policy_ptr).unwrap_or_default(),
            mode: string_from_header(mode_ptr).unwrap_or_else(|| "cors".to_string()),
            credentials: string_from_header(credentials_ptr)
                .unwrap_or_else(|| "same-origin".to_string()),
            cache: string_from_header(cache_ptr).unwrap_or_else(|| "default".to_string()),
            redirect: string_from_header(redirect_ptr).unwrap_or_else(|| "follow".to_string()),
            integrity: string_from_header(integrity_ptr).unwrap_or_default(),
            keepalive: body_metadata::bool_from_js(keepalive),
            duplex: string_from_header(duplex_ptr).unwrap_or_else(|| "half".to_string()),
            signal: body_metadata::signal_or_default(signal),
            cached_headers_id: None,
        },
    );
    handle_to_f64(id)
}

/// `new Request(url, init)` where `init` is a *runtime* object value rather
/// than a statically-analyzable object literal (#5458). Codegen's
/// `extract_options_fields` fast path only recognizes inline `{...}` literals,
/// recorded option-object locals, and `__AnonShape_` synthesis; for any other
/// init shape — a call-expression result (`new Request(url, f())`), a spread
/// literal (`{ ...e }`), or a dynamic object — it previously evaluated and
/// **discarded** the init, silently dropping `method`/`body`/`headers`. That
/// made every non-GET method default back to `"GET"`, mis-dispatching POST
/// requests to GET handlers (or 404) in Hono and any other framework that
/// builds a `RequestInit` indirectly. This helper reads each field off the
/// init object at runtime and delegates to `js_request_new` so all construction
/// and validation logic stays in one place.
///
/// # Safety
/// `url_ptr` must be null or a valid string header; `init` must be a valid
/// NaN-boxed `JSValue`. Called only from codegen-emitted FFI.
#[no_mangle]
pub unsafe extern "C" fn js_request_new_from_init(url_ptr: *const StringHeader, init: f64) -> f64 {
    let raw = perry_runtime::value::js_nanbox_get_pointer(init);
    // Non-object init (undefined / number / small handle): behave like
    // `new Request(url)` with no init — every field keeps its default.
    if raw < 0x10000 {
        return js_request_new(
            url_ptr,
            std::ptr::null(),
            std::ptr::null(),
            0.0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            f64::from_bits(TAG_FALSE),
            std::ptr::null(),
            f64::from_bits(TAG_UNDEFINED),
        );
    }
    let obj = raw as *const perry_runtime::object::ObjectHeader;

    // Read `init[name]` as a NaN-boxed JSValue (TAG_UNDEFINED when absent).
    let field = |name: &[u8]| -> f64 {
        let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
        perry_runtime::object::js_object_get_field_by_name_f64(obj, key)
    };
    // Read `init[name]` as a raw `*const StringHeader`, null for absent /
    // undefined / null so `js_request_new`'s `string_from_header` applies the
    // correct per-field default.
    let str_field = |name: &[u8]| -> *const StringHeader {
        let v = field(name);
        if matches!(v.to_bits(), TAG_UNDEFINED | TAG_NULL) {
            return std::ptr::null();
        }
        perry_runtime::value::js_get_string_pointer_unified(v) as *const StringHeader
    };

    // `headers`: build a fresh Headers store from whatever the init carries
    // (a Headers handle, a plain object, or an iterable of `[name, value]`).
    let headers_val = field(b"headers");
    let headers_handle = if matches!(headers_val.to_bits(), TAG_UNDEFINED | TAG_NULL) {
        0.0
    } else {
        let h = js_headers_new();
        js_headers_init_from_value(h, headers_val);
        h
    };

    let keepalive = field(b"keepalive");
    let keepalive = if keepalive.to_bits() == TAG_UNDEFINED {
        f64::from_bits(TAG_FALSE)
    } else {
        keepalive
    };

    js_request_new(
        url_ptr,
        str_field(b"method"),
        str_field(b"body"),
        headers_handle,
        str_field(b"referrer"),
        str_field(b"referrerPolicy"),
        str_field(b"mode"),
        str_field(b"credentials"),
        str_field(b"cache"),
        str_field(b"redirect"),
        str_field(b"integrity"),
        keepalive,
        str_field(b"duplex"),
        field(b"signal"),
    )
}
