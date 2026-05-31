#[cfg(feature = "external-http-client-pump")]
const PTR_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

#[cfg(feature = "external-http-client-pump")]
pub(super) unsafe fn dispatch_client_incoming_method(
    handle: i64,
    method_name: &str,
    args: &[f64],
) -> Option<f64> {
    if !matches!(method_name, "setEncoding" | "on" | "addListener") {
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
