//! ClientRequest metadata and client IncomingMessage FFI surface.

use super::*;

/// `IncomingMessage.setEncoding(encoding)` for client responses. The same
/// static `IncomingMessage` class tag is used for server requests, so a client
/// registry miss is forwarded to the server-side handle implementation.
#[no_mangle]
pub unsafe extern "C" fn js_http_incoming_message_set_encoding(
    handle: Handle,
    encoding_ptr: *const StringHeader,
) -> Handle {
    let encoding = read_str(encoding_ptr).unwrap_or_else(|| "utf8".to_string());
    let mut matched = false;
    with_handle_mut::<IncomingMessageHandle, _, _>(handle, |res| {
        res.encoding = Some(encoding.clone());
        matched = true;
    });
    if matched {
        return handle;
    }

    extern "C" {
        fn js_ext_http_incoming_message_is_handle(handle: i64) -> i32;
        fn js_node_http_im_set_encoding(handle: i64, encoding_ptr: *const StringHeader) -> i64;
    }
    if js_ext_http_incoming_message_is_handle(handle) != 0 {
        js_node_http_im_set_encoding(handle, encoding_ptr);
    }
    handle
}

/// `res.pipe(dest)` for a client `IncomingMessage`: remember the destination
/// writable so the body-delivery handlers forward each chunk to `dest.write()`
/// and finish it with `dest.end()`. Returns `dest` per Node's
/// pipe-returns-destination contract (node-fetch keeps only the return value:
/// `const body = res.pipe(new PassThrough())`). Without this the destination
/// never receives data and `response.text()` hangs forever.
#[no_mangle]
pub unsafe extern "C" fn js_http_incoming_message_pipe(handle: Handle, dest: f64) -> f64 {
    with_handle_mut::<IncomingMessageHandle, _, _>(handle, |res| {
        res.pipes.push(dest.to_bits());
    });
    dest
}

/// Distinct external-client setter for stdlib fallback dispatch. The legacy
/// `js_http_incoming_message_set_encoding` symbol is shared with perry-stdlib.
#[no_mangle]
pub unsafe extern "C" fn js_ext_http_client_incoming_message_set_encoding(
    handle: Handle,
    encoding_ptr: *const StringHeader,
) -> Handle {
    let encoding = read_str(encoding_ptr).unwrap_or_else(|| "utf8".to_string());
    with_handle_mut::<IncomingMessageHandle, _, _>(handle, |res| {
        res.encoding = Some(encoding);
    });
    handle
}

#[no_mangle]
pub extern "C" fn js_http_client_request_method(handle: Handle) -> *mut StringHeader {
    let method = with_handle_mut::<ClientRequestHandle, _, _>(handle, |req| req.method.clone())
        .unwrap_or_default();
    alloc_string(&method).as_raw()
}

#[no_mangle]
pub extern "C" fn js_http_client_request_protocol(handle: Handle) -> *mut StringHeader {
    let protocol = with_handle_mut::<ClientRequestHandle, _, _>(handle, |req| {
        reqwest::Url::parse(&req.url)
            .map(|u| format!("{}:", u.scheme()))
            .unwrap_or_default()
    })
    .unwrap_or_default();
    alloc_string(&protocol).as_raw()
}

#[no_mangle]
pub extern "C" fn js_http_client_request_host(handle: Handle) -> *mut StringHeader {
    let host = with_handle_mut::<ClientRequestHandle, _, _>(handle, |req| {
        reqwest::Url::parse(&req.url)
            .ok()
            .and_then(|u| u.host_str().map(|s| s.to_string()))
            .unwrap_or_default()
    })
    .unwrap_or_default();
    alloc_string(&host).as_raw()
}

#[no_mangle]
pub extern "C" fn js_http_client_request_path(handle: Handle) -> *mut StringHeader {
    let path = with_handle_mut::<ClientRequestHandle, _, _>(handle, |req| {
        reqwest::Url::parse(&req.url)
            .map(|u| {
                let mut path = u.path().to_string();
                if path.is_empty() {
                    path.push('/');
                }
                if let Some(q) = u.query() {
                    path.push('?');
                    path.push_str(q);
                }
                path
            })
            .unwrap_or_default()
    })
    .unwrap_or_default();
    alloc_string(&path).as_raw()
}

#[no_mangle]
pub unsafe extern "C" fn js_http_client_request_listener_count(
    handle: Handle,
    event_ptr: *const StringHeader,
) -> f64 {
    let event = match read_str(event_ptr) {
        Some(e) => e,
        None => return 0.0,
    };
    with_handle_mut::<ClientRequestHandle, _, _>(handle, |req| {
        let explicit = req.listeners.get(&event).map(|v| v.len()).unwrap_or(0);
        let implicit_response = if event == "response" && req.response_callback != 0 {
            1
        } else {
            0
        };
        (explicit + implicit_response) as f64
    })
    .unwrap_or(0.0)
}

// ------------------------------------------------------------------
// FFI: IncomingMessage accessors
// ------------------------------------------------------------------

/// `1` if `handle` is registered as an `IncomingMessageHandle`,
/// `0` otherwise. Used by perry-stdlib's `js_handle_property_dispatch`
/// to gate the `res.statusCode` / `res.headers` arms — keeps the
/// property-name match from accidentally returning IncomingMessage
/// fields for an unrelated handle whose id collides.
#[no_mangle]
pub extern "C" fn js_http_is_incoming_message(handle: Handle) -> i32 {
    with_handle_mut::<IncomingMessageHandle, _, _>(handle, |_| ())
        .map(|_| 1)
        .unwrap_or(0)
}

/// Distinct external-client probe for stdlib fallback dispatch.
#[no_mangle]
pub extern "C" fn js_ext_http_client_incoming_message_is_handle(handle: Handle) -> i32 {
    js_http_is_incoming_message(handle)
}

/// `res.statusCode`.
#[no_mangle]
pub extern "C" fn js_http_status_code(handle: Handle) -> f64 {
    let mut out = 0.0;
    with_handle_mut::<IncomingMessageHandle, _, _>(handle, |res| {
        out = res.status_code as f64;
    });
    out
}

/// `res.statusMessage`.
#[no_mangle]
pub extern "C" fn js_http_status_message(handle: Handle) -> *mut StringHeader {
    let mut out: *mut StringHeader = std::ptr::null_mut();
    with_handle_mut::<IncomingMessageHandle, _, _>(handle, |res| {
        out = alloc_string(&res.status_message).as_raw();
    });
    if out.is_null() {
        alloc_string("").as_raw()
    } else {
        out
    }
}

/// `res.headers` — returns a NaN-boxed object (bits returned as f64).
/// The receiving codegen-side `f64`-typed slot stores the bits, so
/// the user's TS code sees an Object as expected.
#[no_mangle]
pub extern "C" fn js_http_response_headers(handle: Handle) -> f64 {
    let mut out = f64::from_bits(TAG_UNDEFINED);
    with_handle_mut::<IncomingMessageHandle, _, _>(handle, |res| {
        out = build_response_headers_object(&res.headers);
    });
    if out.to_bits() == TAG_UNDEFINED {
        if let Some(server_out) = server_incoming_property(handle, "headers") {
            return server_out;
        }
    }
    out
}

/// `res.trailers` — HTTP trailers populated after the body completes.
#[no_mangle]
pub extern "C" fn js_http_response_trailers(handle: Handle) -> f64 {
    let mut out = f64::from_bits(TAG_UNDEFINED);
    with_handle_mut::<IncomingMessageHandle, _, _>(handle, |res| {
        out = map_to_js_object(&res.trailers);
    });
    if out.to_bits() == TAG_UNDEFINED {
        if let Some(server_out) = server_incoming_property(handle, "trailers") {
            return server_out;
        }
    }
    out
}

#[no_mangle]
pub extern "C" fn js_http_incoming_message_socket(handle: Handle) -> f64 {
    with_handle_mut::<IncomingMessageHandle, _, _>(handle, |response| {
        if response.socket_handle == 0 {
            f64::from_bits(TAG_UNDEFINED)
        } else {
            f64::from_bits(POINTER_TAG | (response.socket_handle as u64 & PTR_MASK))
        }
    })
    .unwrap_or_else(|| f64::from_bits(TAG_UNDEFINED))
}

/// `res.req` — the ClientRequest paired with a client IncomingMessage.
#[no_mangle]
pub extern "C" fn js_http_incoming_message_req(handle: Handle) -> f64 {
    with_handle_mut::<IncomingMessageHandle, _, _>(handle, |response| {
        if response.request_handle == 0 {
            f64::from_bits(TAG_UNDEFINED)
        } else {
            f64::from_bits(POINTER_TAG | (response.request_handle as u64 & PTR_MASK))
        }
    })
    .unwrap_or_else(|| f64::from_bits(TAG_UNDEFINED))
}

fn server_incoming_property(handle: Handle, property_name: &str) -> Option<f64> {
    extern "C" {
        fn js_ext_http_incoming_message_is_handle(handle: i64) -> i32;
        fn js_ext_http_incoming_message_dispatch_property(
            handle: i64,
            property_ptr: *const u8,
            property_len: usize,
        ) -> f64;
    }
    unsafe {
        if js_ext_http_incoming_message_is_handle(handle) == 0 {
            return None;
        }
        Some(js_ext_http_incoming_message_dispatch_property(
            handle,
            property_name.as_ptr(),
            property_name.len(),
        ))
    }
}

pub(crate) fn body_chunk_value(body: &[u8], encoding: Option<&str>) -> f64 {
    match encoding {
        Some(_) => {
            let s = String::from_utf8_lossy(body).into_owned();
            let header = alloc_string(&s);
            f64::from_bits(STRING_TAG | (header.as_raw() as u64 & PTR_MASK))
        }
        None => {
            let buf = perry_ffi::alloc_buffer(body);
            if buf.is_null() {
                f64::from_bits(TAG_UNDEFINED)
            } else {
                f64::from_bits(POINTER_TAG | (buf as u64 & PTR_MASK))
            }
        }
    }
}
