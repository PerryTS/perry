//! Native bindings for the npm `axios` HTTP client.
//!
//! Phase 5 step 13 — first HTTP-client wrapper port. Uses
//! perry-ffi v0.5.x's full surface: handle registry +
//! spawn_blocking + JsPromise + JsValue.
//!
//! Transport is `perry_http_client` since P11 (turnloop), not reqwest — see
//! `run_request`. `crates/perry-stdlib/src/axios.rs` is still the reqwest copy,
//! and is reachable only under `PERRY_DISABLE_WELL_KNOWN=1`.

use std::time::Duration;

use perry_ffi::{
    alloc_string, get_handle, json_stringify, read_string, register_handle, spawn_blocking,
    with_handle, Handle, JsPromise, JsString, JsValue, Promise, StringHeader,
};
use perry_http_client::{Client, Request};

/// #598: read the body argument as a JSON string. axios in npm-land
/// accepts the body as either a string (sent as-is) or any JS value
/// (JSON.stringify'd before send). Pre-fix Perry's FFI took a raw
/// `*const StringHeader`, which the codegen produced by unboxing the
/// caller's NaN-boxed value — for an object literal the unboxed
/// pointer was a real `*mut ObjectHeader`, the runtime read it as a
/// `*mut StringHeader`, and the request body became the byte pattern
/// of the ObjectHeader struct followed by the first character of the
/// stringified field. Same shape under bun: `axios.post(url, {a:1})`
/// sends `{"a":1}`. With the new f64 signature, the codegen passes
/// the NaN-boxed value through; here we route strings unchanged and
/// JSON.stringify everything else.
unsafe fn read_body_as_string(value_bits: f64) -> String {
    const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
    const SHORT_STRING_TAG: u64 = 0x7FFB_0000_0000_0000;
    const TAG_MASK: u64 = 0xFFFF_0000_0000_0000;
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    let bits = value_bits.to_bits();
    if bits == TAG_UNDEFINED || bits == TAG_NULL {
        return String::new();
    }
    let tag = bits & TAG_MASK;
    if tag == STRING_TAG || tag == SHORT_STRING_TAG {
        // String: read as-is, no JSON quoting.
        let ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as *const StringHeader;
        let handle = JsString::from_raw(ptr as *mut StringHeader);
        return read_string(handle).map(String::from).unwrap_or_default();
    }
    // Object / number / array / etc. — JSON.stringify.
    let v = JsValue::from_bits(bits);
    json_stringify(v).unwrap_or_default()
}

/// Response handle wrapper.
pub struct AxiosResponseHandle {
    pub status: u16,
    pub status_text: String,
    pub data: String,
    /// Issue #627: lower-cased Content-Type header value (without
    /// charset suffix), or empty string if absent. `js_axios_response_data_parsed`
    /// consults this to decide whether to JSON-parse the body — matches
    /// npm axios's content-type-based behavior, replacing v0.5.714's
    /// body-shape heuristic which would incorrectly parse a JSON-shaped
    /// string body served with `text/plain`.
    pub content_type: String,
}

unsafe fn read_str(ptr: *const StringHeader) -> Option<String> {
    let handle = JsString::from_raw(ptr as *mut StringHeader);
    read_string(handle).map(String::from)
}

/// The default whole-request budget.
///
/// `reqwest::Client::new()` set none, so a hung server held the blocking-pool
/// thread for the life of the process. Axios itself defaults to no timeout too,
/// which is why this is generous rather than short — but it is finite, and a
/// pool thread is a bounded resource.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

/// Common request driver — runs the HTTP call inside `spawn_blocking`, packages
/// the response into an `AxiosResponseHandle`, registers it, and resolves the
/// promise with a POINTER_TAG-tagged handle value (issue #340 trick from the
/// original perry-stdlib axios — without the explicit NaN-boxing, the awaiter
/// sees a subnormal float that decays to `undefined` on `r.status` accesses).
///
/// The transport is `perry_http_client`, which owns a `turnloop::Loop` for the
/// duration of the call. That is only sound because this body already runs on a
/// blocking-pool thread that is not a JS agent and owns no loop of its own —
/// the same place it used to block on `tokio::runtime::Handle::current()`. See
/// that crate's module docs for why no other Perry caller may do this.
fn run_request<F>(
    method: &'static str,
    url_or_err: Result<String, &'static str>,
    build: F,
) -> *mut Promise
where
    F: FnOnce(String) -> Request + Send + 'static,
{
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    let url = match url_or_err {
        Ok(u) => u,
        Err(msg) => {
            promise.reject_string(msg);
            return raw;
        }
    };

    spawn_blocking(move || {
        let result: Result<AxiosResponseHandle, String> = (|| {
            let client = Client::with_timeout(REQUEST_TIMEOUT);
            let response = client
                .execute(build(url))
                .map_err(|e| format!("{} request failed: {}", method, e))?;
            let status = response.status;
            let status_text = reason_phrase(status).to_string();
            // Issue #627: lower-case the Content-Type and take the part before
            // `;` so `application/json; charset=utf-8` reduces to
            // `application/json` for the JSON-parse decision.
            let content_type = response
                .header("content-type")
                .map(|v| String::from_utf8_lossy(v).into_owned())
                .map(|s| {
                    s.split(';')
                        .next()
                        .unwrap_or(&s)
                        .trim()
                        .to_ascii_lowercase()
                })
                .unwrap_or_default();
            Ok(AxiosResponseHandle {
                status,
                status_text,
                data: response.text(),
                content_type,
            })
        })();
        match result {
            Ok(resp) => {
                let handle = register_handle(resp);
                // POINTER_TAG-tagged handle value — see #340.
                promise.resolve(JsValue::from_object_ptr(handle as *mut ()));
            }
            Err(msg) => promise.reject_string(&msg),
        }
    });
    raw
}

/// `response.statusText`, as `http::StatusCode::canonical_reason` gave it.
///
/// Axios sets `statusText` from the status line, and a JS caller can compare it
/// against a literal, so this is observable rather than cosmetic. Only the codes
/// a real HTTP server sends are listed; an unknown code answers `""`, which is
/// what `canonical_reason()` did for one.
fn reason_phrase(status: u16) -> &'static str {
    match status {
        100 => "Continue",
        101 => "Switching Protocols",
        102 => "Processing",
        103 => "Early Hints",
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        203 => "Non Authoritative Information",
        204 => "No Content",
        205 => "Reset Content",
        206 => "Partial Content",
        207 => "Multi-Status",
        208 => "Already Reported",
        226 => "IM Used",
        300 => "Multiple Choices",
        301 => "Moved Permanently",
        302 => "Found",
        303 => "See Other",
        304 => "Not Modified",
        305 => "Use Proxy",
        307 => "Temporary Redirect",
        308 => "Permanent Redirect",
        400 => "Bad Request",
        401 => "Unauthorized",
        402 => "Payment Required",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        406 => "Not Acceptable",
        407 => "Proxy Authentication Required",
        408 => "Request Timeout",
        409 => "Conflict",
        410 => "Gone",
        411 => "Length Required",
        412 => "Precondition Failed",
        413 => "Payload Too Large",
        414 => "URI Too Long",
        415 => "Unsupported Media Type",
        416 => "Range Not Satisfiable",
        417 => "Expectation Failed",
        418 => "I'm a teapot",
        421 => "Misdirected Request",
        422 => "Unprocessable Entity",
        423 => "Locked",
        424 => "Failed Dependency",
        425 => "Too Early",
        426 => "Upgrade Required",
        428 => "Precondition Required",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        451 => "Unavailable For Legal Reasons",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        505 => "HTTP Version Not Supported",
        506 => "Variant Also Negotiates",
        507 => "Insufficient Storage",
        508 => "Loop Detected",
        510 => "Not Extended",
        511 => "Network Authentication Required",
        _ => "",
    }
}

/// `axios.get(url) -> Promise<Response>`.
///
/// # Safety
///
/// `url_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_axios_get(url_ptr: *const StringHeader) -> *mut Promise {
    let url = read_str(url_ptr).ok_or("Invalid URL");
    run_request("GET", url, Request::get)
}

/// `axios.head(url) -> Promise<Response>`.
///
/// # Safety
///
/// `url_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_axios_head(url_ptr: *const StringHeader) -> *mut Promise {
    let url = read_str(url_ptr).ok_or("Invalid URL");
    run_request("HEAD", url, Request::head)
}

/// `axios.options(url) -> Promise<Response>`.
///
/// # Safety
///
/// `url_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_axios_options(url_ptr: *const StringHeader) -> *mut Promise {
    let url = read_str(url_ptr).ok_or("Invalid URL");
    run_request("OPTIONS", url, Request::options)
}

/// `axios.post(url, data) -> Promise<Response>`.
///
/// # Safety
///
/// `url_ptr` must be null or a Perry-runtime `StringHeader`. `data` is
/// a NaN-boxed JSValue — strings are sent as-is, all other shapes are
/// JSON.stringify'd. See `read_body_as_string` for the routing rule
/// (#598). The signature uses `f64` to match the codegen dispatch's
/// pass-as-double path; Rust's calling convention puts it in d0 / a
/// vector register on AArch64, matching what the codegen emits.
#[no_mangle]
pub unsafe extern "C" fn js_axios_post(url_ptr: *const StringHeader, data: f64) -> *mut Promise {
    let url = read_str(url_ptr).ok_or("Invalid URL");
    let body = read_body_as_string(data);
    run_request("POST", url, move |url| {
        Request::post(url)
            .header("content-type", "application/json")
            .body(body)
    })
}

/// `axios.put(url, data) -> Promise<Response>`. Same body-encoding
/// rule as `axios.post` (#598).
///
/// # Safety
///
/// `url_ptr` must be null or a Perry-runtime `StringHeader`. `data` is
/// a NaN-boxed JSValue.
#[no_mangle]
pub unsafe extern "C" fn js_axios_put(url_ptr: *const StringHeader, data: f64) -> *mut Promise {
    let url = read_str(url_ptr).ok_or("Invalid URL");
    let body = read_body_as_string(data);
    run_request("PUT", url, move |url| {
        Request::put(url)
            .header("content-type", "application/json")
            .body(body)
    })
}

/// `axios.delete(url) -> Promise<Response>`.
///
/// # Safety
///
/// `url_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_axios_delete(url_ptr: *const StringHeader) -> *mut Promise {
    let url = read_str(url_ptr).ok_or("Invalid URL");
    run_request("DELETE", url, Request::delete)
}

/// `axios.patch(url, data) -> Promise<Response>`. Same body-encoding
/// rule as `axios.post` (#598).
///
/// # Safety
///
/// `url_ptr` must be null or a Perry-runtime `StringHeader`. `data` is
/// a NaN-boxed JSValue.
#[no_mangle]
pub unsafe extern "C" fn js_axios_patch(url_ptr: *const StringHeader, data: f64) -> *mut Promise {
    let url = read_str(url_ptr).ok_or("Invalid URL");
    let body = read_body_as_string(data);
    run_request("PATCH", url, move |url| {
        Request::patch(url)
            .header("content-type", "application/json")
            .body(body)
    })
}

/// `response.status -> number`.
#[no_mangle]
pub extern "C" fn js_axios_response_status(handle: Handle) -> f64 {
    if let Some(r) = get_handle::<AxiosResponseHandle>(handle) {
        r.status as f64
    } else {
        0.0
    }
}

/// `response.statusText -> string`.
#[no_mangle]
pub extern "C" fn js_axios_response_status_text(handle: Handle) -> *mut StringHeader {
    with_handle::<AxiosResponseHandle, _, _>(handle, |r| alloc_string(&r.status_text).as_raw())
        .unwrap_or(std::ptr::null_mut())
}

/// `response.data -> string`. Legacy/backwards-compat — returns the
/// raw response body bytes as a perry string. For the JSON-auto-parse
/// path that npm `axios` provides (where `r.data.ok` works directly
/// when the server returns `application/json`), see
/// `js_axios_response_data_parsed` below.
#[no_mangle]
pub extern "C" fn js_axios_response_data(handle: Handle) -> *mut StringHeader {
    with_handle::<AxiosResponseHandle, _, _>(handle, |r| alloc_string(&r.data).as_raw())
        .unwrap_or(std::ptr::null_mut())
}

/// `response.data -> any` — auto-parsed variant. npm `axios` parses
/// the response body as JSON when the response's content-type starts
/// with `application/json`; otherwise it hands back the raw string.
/// Returns an f64 NaN-boxed JSValue: a string for non-JSON, a parsed
/// object/array/number/bool/null for JSON. Returns the string fallback
/// on any parse error so callers don't have to special-case malformed
/// JSON. The TS-side `r.data` getter routes here so `r.data.ok` /
/// `r.data[0]` / etc. work the same way as in node `axios`. Issue
/// #604 followup — only surfaced once the listen() hang was fixed.
#[no_mangle]
pub extern "C" fn js_axios_response_data_parsed(handle: Handle) -> f64 {
    // Issue #627: snapshot body + content-type in one with_handle pass to
    // avoid two registry lookups + leaking the lock across the FFI call to
    // js_json_parse below.
    let snapshot = with_handle::<AxiosResponseHandle, _, _>(handle, |r| {
        (r.data.clone(), r.content_type.clone())
    });
    let (body, content_type) = match snapshot {
        Some(s) => s,
        None => return f64::from_bits(0x7FFC_0000_0000_0001), // TAG_UNDEFINED
    };
    // Issue #627: npm axios parses JSON only when content-type starts with
    // `application/json` (with optional `; charset=...`). Pre-fix, perry
    // used a body-shape heuristic which would incorrectly parse a JSON-
    // looking string body served with `text/plain`. The `+json` suffix
    // form (e.g. `application/vnd.api+json`) also gets parsed by npm
    // axios per the standard, so accept either shape.
    let is_json_ct = content_type == "application/json" || content_type.ends_with("+json");
    if is_json_ct {
        // Cross the FFI boundary into the runtime's JSON parser. The
        // runtime returns `undefined` (TAG_UNDEFINED) on parse error,
        // which we detect and fall through to the raw-string path so
        // the user always gets *something* on `r.data`. Note: the
        // runtime's `js_json_parse` declares its return type as
        // `JSValue` (repr(transparent) over u64), so we declare it
        // here as `u64` rather than `f64` to keep the AArch64 ABI on
        // the integer register (x0) instead of the float register (d0).
        extern "C" {
            fn js_json_parse(ptr: *const StringHeader) -> u64;
        }
        let s = alloc_string(&body);
        let parsed_bits = unsafe { js_json_parse(s.as_raw()) };
        const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
        if parsed_bits != TAG_UNDEFINED {
            return f64::from_bits(parsed_bits);
        }
    }
    // Non-JSON or parse failure — return the raw body as a perry
    // string. NaN-boxed via STRING_TAG so the receiver sees it as a
    // proper JS string.
    let s = alloc_string(&body);
    let bits = 0x7FFF_0000_0000_0000_u64 | (s.as_raw() as u64 & 0x0000_FFFF_FFFF_FFFF);
    f64::from_bits(bits)
}
