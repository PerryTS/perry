//! net.Socket/net.Server method-value helpers for handle dispatch.

fn nanbox_handle(handle: i64) -> f64 {
    f64::from_bits(0x7FFD_0000_0000_0000u64 | (handle as u64 & 0x0000_FFFF_FFFF_FFFF))
}

#[cfg(all(
    not(feature = "bundled-net"),
    feature = "external-net-pump",
    not(target_os = "ios"),
    not(target_os = "android")
))]
fn undefined() -> f64 {
    f64::from_bits(0x7FFC_0000_0000_0001)
}

#[cfg(all(
    not(feature = "bundled-net"),
    feature = "external-net-pump",
    not(target_os = "ios"),
    not(target_os = "android")
))]
fn null() -> f64 {
    f64::from_bits(0x7FFC_0000_0000_0002)
}

fn bind_handle_method(handle: i64, name: &'static [u8]) -> f64 {
    extern "C" {
        fn js_class_method_bind(
            instance: f64,
            method_name_ptr: *const u8,
            method_name_len: usize,
        ) -> f64;
    }
    unsafe { js_class_method_bind(nanbox_handle(handle), name.as_ptr(), name.len()) }
}

#[cfg(all(
    not(feature = "bundled-net"),
    feature = "external-net-pump",
    not(target_os = "ios"),
    not(target_os = "android")
))]
fn unbox_to_i64(v: f64) -> i64 {
    (v.to_bits() & 0x0000_FFFF_FFFF_FFFF) as i64
}

#[cfg(all(
    not(feature = "bundled-net"),
    feature = "external-net-pump",
    not(target_os = "ios"),
    not(target_os = "android")
))]
fn json_str_to_value(s: *mut perry_runtime::StringHeader) -> f64 {
    if s.is_null() {
        return null();
    }
    f64::from_bits(unsafe { perry_runtime::json::js_json_parse_or_null(s).bits() })
}

fn net_socket_method_name(prop: &str) -> Option<&'static [u8]> {
    match prop {
        "address" => Some(b"address"),
        "connect" => Some(b"connect"),
        "destroy" => Some(b"destroy"),
        "destroySoon" => Some(b"destroySoon"),
        "end" => Some(b"end"),
        "pause" => Some(b"pause"),
        "ref" => Some(b"ref"),
        "resetAndDestroy" => Some(b"resetAndDestroy"),
        "resume" => Some(b"resume"),
        "setEncoding" => Some(b"setEncoding"),
        "setKeepAlive" => Some(b"setKeepAlive"),
        "setNoDelay" => Some(b"setNoDelay"),
        "setTimeout" => Some(b"setTimeout"),
        "unref" => Some(b"unref"),
        "write" => Some(b"write"),
        "on" => Some(b"on"),
        "addListener" => Some(b"addListener"),
        "once" => Some(b"once"),
        "off" => Some(b"off"),
        "removeListener" => Some(b"removeListener"),
        "removeAllListeners" => Some(b"removeAllListeners"),
        "listenerCount" => Some(b"listenerCount"),
        "eventNames" => Some(b"eventNames"),
        "listeners" => Some(b"listeners"),
        "rawListeners" => Some(b"rawListeners"),
        "upgradeToTLS" => Some(b"upgradeToTLS"),
        "setDefaultEncoding" => Some(b"setDefaultEncoding"),
        "cork" => Some(b"cork"),
        "uncork" => Some(b"uncork"),
        _ => None,
    }
}

#[cfg(all(
    not(feature = "bundled-net"),
    feature = "external-net-pump",
    not(target_os = "ios"),
    not(target_os = "android")
))]
fn net_server_method_name(prop: &str) -> Option<&'static [u8]> {
    match prop {
        "address" => Some(b"address"),
        "close" => Some(b"close"),
        "getConnections" => Some(b"getConnections"),
        "listen" => Some(b"listen"),
        "ref" => Some(b"ref"),
        "unref" => Some(b"unref"),
        "on" => Some(b"on"),
        "addListener" => Some(b"addListener"),
        "once" => Some(b"once"),
        "off" => Some(b"off"),
        "removeListener" => Some(b"removeListener"),
        "removeAllListeners" => Some(b"removeAllListeners"),
        "listenerCount" => Some(b"listenerCount"),
        "eventNames" => Some(b"eventNames"),
        "listeners" => Some(b"listeners"),
        "rawListeners" => Some(b"rawListeners"),
        _ => None,
    }
}

pub(crate) fn dispatch_property(handle: i64, property_name: &str) -> Option<f64> {
    if let Some(name) = net_socket_method_name(property_name) {
        #[cfg(all(
            feature = "bundled-net",
            not(target_os = "ios"),
            not(target_os = "android")
        ))]
        if crate::net::is_net_socket_handle(handle) {
            return Some(bind_handle_method(handle, name));
        }

        #[cfg(all(
            not(feature = "bundled-net"),
            feature = "external-net-pump",
            not(target_os = "ios"),
            not(target_os = "android")
        ))]
        {
            extern "C" {
                fn js_ext_net_is_socket_handle(handle: i64) -> i32;
            }
            if unsafe { js_ext_net_is_socket_handle(handle) } != 0 {
                return Some(bind_handle_method(handle, name));
            }
        }
    }

    #[cfg(all(
        not(feature = "bundled-net"),
        feature = "external-net-pump",
        not(target_os = "ios"),
        not(target_os = "android")
    ))]
    if let Some(name) = net_server_method_name(property_name) {
        extern "C" {
            fn js_ext_net_is_server_handle(handle: i64) -> i32;
        }
        if unsafe { js_ext_net_is_server_handle(handle) } != 0 {
            return Some(bind_handle_method(handle, name));
        }
    }

    None
}

#[cfg(all(
    not(feature = "bundled-net"),
    feature = "external-net-pump",
    not(target_os = "ios"),
    not(target_os = "android")
))]
pub(crate) unsafe fn dispatch_external_server_method(
    handle: i64,
    method: &str,
    args: &[f64],
) -> Option<f64> {
    if net_server_method_name(method).is_none() {
        return None;
    }

    extern "C" {
        fn js_ext_net_is_server_handle(handle: i64) -> i32;
        fn js_net_server_listen(handle: i64, port: f64, callback_i64: i64);
        fn js_net_server_close(handle: i64, callback_i64: i64);
        fn js_net_server_address(handle: i64) -> *mut perry_runtime::StringHeader;
        fn js_net_server_on(handle: i64, event_ptr: i64, cb: i64);
        fn js_net_server_once(handle: i64, event_ptr: i64, cb: i64) -> i64;
        fn js_net_server_remove_listener(handle: i64, event_ptr: i64, cb: i64) -> i64;
        fn js_net_server_remove_all_listeners(handle: i64, event_ptr: i64) -> i64;
        fn js_net_server_listener_count(handle: i64, event_ptr: i64) -> f64;
        fn js_net_server_event_names(handle: i64) -> *mut perry_runtime::StringHeader;
        fn js_net_server_listeners(handle: i64, event_ptr: i64) -> i64;
        fn js_net_server_raw_listeners(handle: i64, event_ptr: i64) -> i64;
    }

    if js_ext_net_is_server_handle(handle) == 0 {
        return None;
    }

    let result = match method {
        "listen" => {
            let port = args.first().copied().unwrap_or(0.0);
            let callback = args.get(1).copied().map(unbox_to_i64).unwrap_or(0);
            js_net_server_listen(handle, port, callback);
            nanbox_handle(handle)
        }
        "close" => {
            let callback = args.first().copied().map(unbox_to_i64).unwrap_or(0);
            js_net_server_close(handle, callback);
            nanbox_handle(handle)
        }
        "address" => json_str_to_value(js_net_server_address(handle)),
        "on" | "addListener" if args.len() >= 2 => {
            js_net_server_on(handle, unbox_to_i64(args[0]), unbox_to_i64(args[1]));
            nanbox_handle(handle)
        }
        "once" if args.len() >= 2 => {
            js_net_server_once(handle, unbox_to_i64(args[0]), unbox_to_i64(args[1]));
            nanbox_handle(handle)
        }
        "off" | "removeListener" if args.len() >= 2 => {
            js_net_server_remove_listener(handle, unbox_to_i64(args[0]), unbox_to_i64(args[1]));
            nanbox_handle(handle)
        }
        "removeAllListeners" => {
            let event = args.first().copied().map(unbox_to_i64).unwrap_or(0);
            js_net_server_remove_all_listeners(handle, event);
            nanbox_handle(handle)
        }
        "listenerCount" if !args.is_empty() => {
            js_net_server_listener_count(handle, unbox_to_i64(args[0]))
        }
        "eventNames" => json_str_to_value(js_net_server_event_names(handle)),
        "listeners" if !args.is_empty() => {
            let arr = js_net_server_listeners(handle, unbox_to_i64(args[0]));
            nanbox_handle(arr)
        }
        "rawListeners" if !args.is_empty() => {
            let arr = js_net_server_raw_listeners(handle, unbox_to_i64(args[0]));
            nanbox_handle(arr)
        }
        "ref" | "unref" => nanbox_handle(handle),
        "getConnections" => {
            if let Some(callback) = args.first().copied().map(unbox_to_i64) {
                if callback >= 0x1000 {
                    perry_runtime::closure::js_closure_call2(
                        callback as *const perry_runtime::ClosureHeader,
                        null(),
                        0.0,
                    );
                }
            }
            undefined()
        }
        _ => undefined(),
    };
    Some(result)
}
