//! Axios module
//!
//! Native implementation of the 'axios' npm package: a promise-based HTTP
//! client surface over Perry's outbound transport.
//!
//! # Transport
//!
//! Every request is offered to the turnloop client engine
//! ([`crate::turnloop_client`]) first — the same engine `fetch` uses, with the
//! same connection pool, proxy handling and TLS. Only what that engine declines
//! runs on `reqwest`, and P8's inventory names what is left of that list: a
//! proxy scheme the engine cannot drive, a URL the fetch policy layer rejects,
//! and a genuine absence of an agent loop (the `tokio-wait-driver` A/B arm, or
//! a host where `Loop::new` failed). A thread that merely does not *own* its
//! agent's loop is no longer one of them: turnloop P10 posts the submission to
//! the thread that does.
//!
//! Before this lane the axios surface took `reqwest` unconditionally, and built
//! a fresh `reqwest::Client` per call — so no two axios requests to the same
//! host ever shared a connection. Riding the engine fixes that as a side
//! effect; `turnloop_client`'s pool is per agent, not per request.

use crate::common::async_bridge::{queue_deferred_resolution, queue_promise_resolution};
use crate::common::{
    get_handle, register_handle, spawn_for_promise, string_from_header_lossy as string_from_header,
    Handle,
};
use crate::turnloop_client::{self, Outcome, RequestSpec, Sink};
use perry_runtime::{js_promise_new_cross_thread, js_string_from_bytes, Promise, StringHeader};

/// POINTER_TAG. The handle is NaN-boxed as an object so the awaiter sees a
/// proper handle value rather than a subnormal float that decays to `undefined`
/// on `r.status` / `r.data` (#340).
const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;

/// #598: read the body argument as a JSON string. Strings pass
/// through as-is; everything else is JSON.stringify'd via the
/// runtime's `js_json_stringify`. See perry-ext-axios's parallel
/// helper for the full rationale.
unsafe fn body_string_from_value(value_bits: f64) -> String {
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
        let ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as *const StringHeader;
        return string_from_header(ptr).unwrap_or_default();
    }
    // Object / array / number / etc. — JSON.stringify (type_hint=0
    // = auto-detect from NaN-box tag).
    extern "C" {
        fn js_json_stringify(value: f64, type_hint: u32) -> *mut StringHeader;
    }
    let str_ptr = js_json_stringify(value_bits, 0);
    string_from_header(str_ptr).unwrap_or_default()
}

/// Response handle wrapper
pub struct AxiosResponseHandle {
    pub status: u16,
    pub status_text: String,
    pub data: String,
    /// Populated but not yet observable: `property_dispatch` answers
    /// `r.headers` with `undefined` pending header-object materialisation.
    pub headers: Vec<(String, String)>,
}

/// One axios call, fully materialized on the calling thread before a transport
/// is chosen. Owned data only — every JS value has already been read out, which
/// is what makes either transport free to cross a thread with it (#598).
struct AxiosRequest {
    url: String,
    method: &'static str,
    /// `None` for the bodyless verbs. A body always goes out as JSON, which is
    /// what this surface has always done.
    body: Option<String>,
}

impl AxiosRequest {
    fn headers(&self) -> Vec<(String, String)> {
        match self.body {
            Some(_) => vec![("Content-Type".to_string(), "application/json".to_string())],
            None => Vec::new(),
        }
    }
}

/// Run one request, on the engine if it will take it and on reqwest otherwise.
unsafe fn dispatch(request: AxiosRequest) -> *mut Promise {
    let promise = js_promise_new_cross_thread();
    if try_turnloop(&request, promise as usize) {
        return promise;
    }
    let AxiosRequest { url, method, body } = request;
    let headers = match body {
        Some(_) => vec![("Content-Type", "application/json")],
        None => Vec::new(),
    };
    spawn_for_promise(promise as *mut u8, async move {
        let method = reqwest::Method::from_bytes(method.as_bytes())
            .map_err(|_| format!("Request failed: unsupported method {method}"))?;
        let client = reqwest::Client::new();
        let mut builder = client.request(method, &url);
        for (name, value) in headers {
            builder = builder.header(name, value);
        }
        if let Some(body) = body {
            builder = builder.body(body);
        }
        let response = builder
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?;
        let status = response.status().as_u16();
        let status_text = response
            .status()
            .canonical_reason()
            .unwrap_or("")
            .to_string();
        let headers: Vec<(String, String)> = response
            .headers()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
            .collect();
        let data = response
            .text()
            .await
            .map_err(|e| format!("Failed to read response body: {}", e))?;
        Ok(store(status, status_text, data, headers))
    });
    promise
}

/// Offer the request to the turnloop engine. `true` means it accepted and will
/// settle the promise exactly once.
fn try_turnloop(request: &AxiosRequest, promise_ptr: usize) -> bool {
    submit_with(
        request,
        Sink {
            ctx: promise_ptr,
            on_head: None,
            on_chunk: None,
            on_done: settle,
        },
    )
}

/// The half a test can drive with a sink of its own: build the engine's request
/// from an [`AxiosRequest`] and offer it. Separate so "axios asked the engine"
/// can be asserted without a live JS promise for the real sink to settle.
fn submit_with(request: &AxiosRequest, sink: Sink) -> bool {
    let spec = RequestSpec {
        url: request.url.clone(),
        method: request.method.to_string(),
        headers: request.headers(),
        body: request.body.clone().map(String::into_bytes),
        // axios follows redirects by default (`maxRedirects: 5`), as did the
        // reqwest client this replaced.
        redirect: turnloop_http::client::RedirectMode::Follow,
        abort_key: None,
    };
    match turnloop_client::submit(spec, sink) {
        Ok(()) => true,
        Err(_) => {
            turnloop_client::note_declined();
            false
        }
    }
}

/// The engine's completion, on the thread that owns this agent's loop.
fn settle(ctx: usize, outcome: Outcome) {
    match outcome {
        Outcome::Ok(response) => {
            let data = String::from_utf8_lossy(&response.body).into_owned();
            let bits = store(
                response.status,
                response.status_text,
                data,
                response.headers,
            );
            queue_promise_resolution(ctx, true, bits);
        }
        Outcome::Err(error) => {
            // The message prefix the reqwest path used, kept: this surface
            // rejects with a plain string, not an Error, and callers match on
            // its text.
            let message = format!("Request failed: {}", error.message);
            queue_deferred_resolution(ctx, false, move || {
                let ptr = js_string_from_bytes(message.as_ptr(), message.len() as u32);
                perry_runtime::JSValue::string_ptr(ptr).bits()
            });
        }
    }
}

/// Register the response and NaN-box its handle, the one way both transports
/// hand a result back.
fn store(status: u16, status_text: String, data: String, headers: Vec<(String, String)>) -> u64 {
    let handle = register_handle(AxiosResponseHandle {
        status,
        status_text,
        data,
        headers,
    });
    (handle as u64) | POINTER_TAG
}

/// Read the URL argument, or reject the promise the way this surface always
/// has. `Err` carries the already-settled promise.
unsafe fn url_or_reject(url_ptr: *const StringHeader) -> Result<String, *mut Promise> {
    match string_from_header(url_ptr) {
        Some(url) => Ok(url),
        None => {
            let promise = js_promise_new_cross_thread();
            spawn_for_promise(promise as *mut u8, async move {
                Err::<u64, _>("Invalid URL".to_string())
            });
            Err(promise)
        }
    }
}

/// The bodyless verbs.
unsafe fn request_without_body(url_ptr: *const StringHeader, method: &'static str) -> *mut Promise {
    let url = match url_or_reject(url_ptr) {
        Ok(url) => url,
        Err(promise) => return promise,
    };
    dispatch(AxiosRequest {
        url,
        method,
        body: None,
    })
}

/// The verbs that carry a JSON body.
unsafe fn request_with_body(
    url_ptr: *const StringHeader,
    method: &'static str,
    data: f64,
) -> *mut Promise {
    let url = match url_or_reject(url_ptr) {
        Ok(url) => url,
        Err(promise) => return promise,
    };
    // #598: stringify on Perry's main thread BEFORE either transport takes the
    // request. `js_json_stringify` reads from perry-runtime's thread-local
    // arena, so doing it inside a spawned future would read the wrong arena.
    let body = body_string_from_value(data);
    dispatch(AxiosRequest {
        url,
        method,
        body: Some(body),
    })
}

/// axios.get(url) -> Promise<AxiosResponse>
#[no_mangle]
pub unsafe extern "C" fn js_axios_get(url_ptr: *const StringHeader) -> *mut Promise {
    request_without_body(url_ptr, "GET")
}

/// axios.head(url) -> Promise<AxiosResponse>
#[no_mangle]
pub unsafe extern "C" fn js_axios_head(url_ptr: *const StringHeader) -> *mut Promise {
    request_without_body(url_ptr, "HEAD")
}

/// axios.options(url) -> Promise<AxiosResponse>
#[no_mangle]
pub unsafe extern "C" fn js_axios_options(url_ptr: *const StringHeader) -> *mut Promise {
    request_without_body(url_ptr, "OPTIONS")
}

/// axios.delete(url) -> Promise<AxiosResponse>
#[no_mangle]
pub unsafe extern "C" fn js_axios_delete(url_ptr: *const StringHeader) -> *mut Promise {
    request_without_body(url_ptr, "DELETE")
}

/// axios.post(url, data) -> Promise<AxiosResponse>
#[no_mangle]
pub unsafe extern "C" fn js_axios_post(url_ptr: *const StringHeader, data: f64) -> *mut Promise {
    request_with_body(url_ptr, "POST", data)
}

/// axios.put(url, data) -> Promise<AxiosResponse>
#[no_mangle]
pub unsafe extern "C" fn js_axios_put(url_ptr: *const StringHeader, data: f64) -> *mut Promise {
    request_with_body(url_ptr, "PUT", data)
}

/// axios.patch(url, data) -> Promise<AxiosResponse>
#[no_mangle]
pub unsafe extern "C" fn js_axios_patch(url_ptr: *const StringHeader, data: f64) -> *mut Promise {
    request_with_body(url_ptr, "PATCH", data)
}

/// response.status -> number
#[no_mangle]
pub unsafe extern "C" fn js_axios_response_status(handle: Handle) -> f64 {
    if let Some(response) = get_handle::<AxiosResponseHandle>(handle) {
        response.status as f64
    } else {
        0.0
    }
}

/// response.statusText -> string
#[no_mangle]
pub unsafe extern "C" fn js_axios_response_status_text(handle: Handle) -> *mut StringHeader {
    if let Some(response) = get_handle::<AxiosResponseHandle>(handle) {
        js_string_from_bytes(
            response.status_text.as_ptr(),
            response.status_text.len() as u32,
        )
    } else {
        std::ptr::null_mut()
    }
}

/// response.data -> string
#[no_mangle]
pub unsafe extern "C" fn js_axios_response_data(handle: Handle) -> *mut StringHeader {
    if let Some(response) = get_handle::<AxiosResponseHandle>(handle) {
        js_string_from_bytes(response.data.as_ptr(), response.data.len() as u32)
    } else {
        std::ptr::null_mut()
    }
}

#[cfg(test)]
#[path = "axios/tests.rs"]
mod tests;
