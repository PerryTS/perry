#[cfg(feature = "external-http-client-pump")]
const PTR_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

#[cfg(feature = "external-http-client-pump")]
pub(super) unsafe fn dispatch_client_request_method(
    handle: i64,
    method_name: &str,
    args: &[f64],
) -> Option<f64> {
    if !matches!(
        method_name,
        "end"
            | "write"
            | "setHeader"
            | "setTimeout"
            | "listenerCount"
            | "getHeader"
            | "hasHeader"
            | "removeHeader"
            | "getHeaderNames"
            | "getHeaders"
            | "getRawHeaderNames"
            | "abort"
            | "destroy"
            | "flushHeaders"
            | "cork"
            | "uncork"
            | "setNoDelay"
            | "setSocketKeepAlive"
            // #4909 — listener management: `.on('finish'|'timeout'|…, cb)`
            // on an untyped receiver silently dropped the listener (the
            // ext dispatcher handles these now; the gate hid them).
            | "on"
            | "once"
            | "addListener"
            | "prependListener"
            | "removeListener"
            | "off"
            | "removeAllListeners"
    ) {
        return None;
    }

    extern "C" {
        fn js_ext_http_client_request_is_handle(handle: i64) -> i32;
        fn js_ext_http_client_request_dispatch_method(
            handle: i64,
            method_ptr: *const u8,
            method_len: usize,
            args_ptr: *const f64,
            args_len: usize,
        ) -> f64;
    }

    if unsafe { js_ext_http_client_request_is_handle(handle) } == 0 {
        return None;
    }

    Some(unsafe {
        js_ext_http_client_request_dispatch_method(
            handle,
            method_name.as_ptr(),
            method_name.len(),
            args.as_ptr(),
            args.len(),
        )
    })
}

#[cfg(feature = "external-http-client-pump")]
pub(super) unsafe fn dispatch_client_request_property(
    handle: i64,
    property_name: &str,
) -> Option<f64> {
    if !matches!(
        property_name,
        "on" | "end"
            | "write"
            | "setHeader"
            | "setTimeout"
            | "listenerCount"
            | "method"
            | "protocol"
            | "host"
            | "path"
            | "getHeader"
            | "hasHeader"
            | "removeHeader"
            | "getHeaderNames"
            | "getHeaders"
            | "getRawHeaderNames"
            | "abort"
            | "destroy"
            | "flushHeaders"
            | "cork"
            | "uncork"
            | "setNoDelay"
            | "setSocketKeepAlive"
            | "aborted"
            | "destroyed"
            | "finished"
            | "reusedSocket"
            | "maxHeadersCount"
            | "writableEnded"
            | "writableFinished"
            | "socket"
            | "connection"
            // #4909 — listener management binds + `constructor.name`.
            | "once"
            | "addListener"
            | "prependListener"
            | "removeListener"
            | "off"
            | "removeAllListeners"
            | "constructor"
    ) {
        return None;
    }

    extern "C" {
        fn js_ext_http_client_request_is_handle(handle: i64) -> i32;
        fn js_ext_http_client_request_dispatch_property(
            handle: i64,
            property_ptr: *const u8,
            property_len: usize,
        ) -> f64;
    }

    if unsafe { js_ext_http_client_request_is_handle(handle) } == 0 {
        return None;
    }

    Some(unsafe {
        js_ext_http_client_request_dispatch_property(
            handle,
            property_name.as_ptr(),
            property_name.len(),
        )
    })
}

#[cfg(feature = "external-http-client-pump")]
pub(super) unsafe fn dispatch_client_incoming_method(
    handle: i64,
    method_name: &str,
    args: &[f64],
) -> Option<f64> {
    if !matches!(method_name, "setEncoding" | "on" | "addListener" | "pipe") {
        return None;
    }

    extern "C" {
        fn js_http_is_incoming_message(handle: i64) -> i32;
        fn js_http_incoming_message_set_encoding(
            handle: i64,
            encoding_ptr: *const perry_runtime::StringHeader,
        ) -> i64;
        fn js_http_on(
            handle: i64,
            event_ptr: *const perry_runtime::StringHeader,
            callback: i64,
        ) -> i64;
        fn js_http_incoming_message_pipe(handle: i64, dest: f64) -> f64;
    }

    if unsafe { js_http_is_incoming_message(handle) } == 0 {
        return None;
    }

    let self_ref = f64::from_bits(0x7FFD_0000_0000_0000u64 | (handle as u64 & PTR_MASK));
    let value = match method_name {
        "setEncoding" if !args.is_empty() => {
            let ptr = (args[0].to_bits() & PTR_MASK) as *const perry_runtime::StringHeader;
            unsafe {
                js_http_incoming_message_set_encoding(handle, ptr);
            }
            self_ref
        }
        "on" | "addListener" if args.len() >= 2 => {
            let event = (args[0].to_bits() & PTR_MASK) as *const perry_runtime::StringHeader;
            let callback = (args[1].to_bits() & PTR_MASK) as i64;
            unsafe {
                js_http_on(handle, event, callback);
            }
            self_ref
        }
        // `res.pipe(dest)` — register the destination and return it (Node's
        // pipe-returns-destination contract; node-fetch reads the response
        // body via `res.pipe(new PassThrough())`).
        "pipe" if !args.is_empty() => unsafe { js_http_incoming_message_pipe(handle, args[0]) },
        _ => f64::from_bits(0x7FFC_0000_0000_0001),
    };
    Some(value)
}

#[cfg(feature = "external-http-client-pump")]
pub(super) unsafe fn dispatch_client_incoming_property(
    handle: i64,
    property_name: &str,
) -> Option<f64> {
    if !matches!(
        property_name,
        "statusCode" | "statusMessage" | "headers" | "trailers" | "setEncoding"
    ) {
        return None;
    }

    extern "C" {
        fn js_class_method_bind(
            instance: f64,
            method_name_ptr: *const u8,
            method_name_len: usize,
        ) -> f64;
        fn js_http_is_incoming_message(handle: i64) -> i32;
        fn js_http_status_code(handle: i64) -> f64;
        fn js_http_status_message(handle: i64) -> *mut perry_runtime::StringHeader;
        fn js_http_response_headers(handle: i64) -> f64;
        fn js_http_response_trailers(handle: i64) -> f64;
    }

    if unsafe { js_http_is_incoming_message(handle) } == 0 {
        return None;
    }

    if property_name == "setEncoding" {
        let name = b"setEncoding";
        return Some(unsafe { js_class_method_bind(handle as f64, name.as_ptr(), name.len()) });
    }

    use perry_runtime::JSValue;
    let value = match property_name {
        "statusCode" => unsafe { js_http_status_code(handle) },
        "statusMessage" => {
            let ptr = unsafe { js_http_status_message(handle) };
            if ptr.is_null() {
                f64::from_bits(0x7FFC_0000_0000_0001)
            } else {
                f64::from_bits(JSValue::string_ptr(ptr).bits())
            }
        }
        "headers" => unsafe { js_http_response_headers(handle) },
        "trailers" => unsafe { js_http_response_trailers(handle) },
        _ => f64::from_bits(0x7FFC_0000_0000_0001),
    };
    Some(value)
}
