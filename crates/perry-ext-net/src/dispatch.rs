//! Runtime handle-dispatch extension for `perry-ext-net` handles.
//!
//! The default debug stdlib archive is built with bundled net enabled, so it
//! cannot reference ext-net symbols directly. Registering this extension lets
//! small external net handles expose method values and alias-call dispatch while
//! still falling through to the primary stdlib dispatcher for every other
//! handle family.

use perry_ffi::{
    ArrayHeader, JsClosure, JsPromise, JsValue, Promise, RawClosureHeader, StringHeader,
};
use std::sync::Once;

const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

extern "C" {
    fn js_class_method_bind(
        instance: f64,
        method_name_ptr: *const u8,
        method_name_len: usize,
    ) -> f64;
    fn js_json_parse_or_null(text_ptr: *const StringHeader) -> JsValue;
    fn js_promise_resolve(promise: *mut Promise, value: f64);
    fn js_register_aux_has_active(f: extern "C" fn() -> i32);
    fn js_register_aux_pump(f: extern "C" fn() -> i32);
    fn js_register_handle_method_dispatch_extension(
        f: unsafe extern "C" fn(i64, *const u8, usize, *const f64, usize, *mut f64) -> i32,
    );
    fn js_register_handle_property_dispatch_extension(
        f: unsafe extern "C" fn(i64, *const u8, usize, *mut f64) -> i32,
    );
    fn js_register_handle_property_set_dispatch_extension(
        f: unsafe extern "C" fn(i64, *const u8, usize, f64) -> i32,
    );
}

extern "C" fn process_pending_aux() -> i32 {
    // Drain via the DISTINCT `js_ext_net_drain_pending` symbol — NOT the
    // `js_net_process_pending` extern, whose symbol the bundled stdlib net
    // twin shadows in a workspace build (that twin drains stdlib's queue,
    // leaving ext-net's own adopted-socket events — e.g. the raw-`'upgrade'`
    // `Close` — stuck and the loop pinned). #5010.
    unsafe { crate::js_ext_net_drain_pending() }
}

pub(crate) fn ensure_runtime_dispatch_registered() {
    static REGISTER: Once = Once::new();
    REGISTER.call_once(|| unsafe {
        js_register_aux_pump(process_pending_aux);
        js_register_aux_has_active(crate::js_ext_net_has_active_handles);
        js_register_handle_method_dispatch_extension(js_ext_net_handle_method_dispatch);
        js_register_handle_property_dispatch_extension(js_ext_net_handle_property_dispatch);
        js_register_handle_property_set_dispatch_extension(js_ext_net_handle_property_set_dispatch);
    });
}

fn undefined() -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

fn null() -> f64 {
    f64::from_bits(TAG_NULL)
}

fn nanbox_handle(handle: i64) -> f64 {
    f64::from_bits(POINTER_TAG | (handle as u64 & POINTER_MASK))
}

fn nanbox_ptr<T>(ptr: *mut T) -> f64 {
    f64::from_bits(POINTER_TAG | (ptr as u64 & POINTER_MASK))
}

fn unbox_to_i64(v: f64) -> i64 {
    (v.to_bits() & POINTER_MASK) as i64
}

/// Coerce a nanboxed JS number to a plain `f64`. perry stores small integers as
/// a tagged int32 (`0x7FFE` high bits), not a raw double, so passing the raw
/// nanboxed value where a real number is expected (e.g. `BlockList.addSubnet`'s
/// `prefix: f64`) yields garbage bits. A non-int-tagged value is already an
/// IEEE double and passes through unchanged.
fn unbox_to_f64(v: f64) -> f64 {
    const INT32_TAG: u64 = 0x7FFE_0000_0000_0000;
    const INT32_MASK: u64 = 0x0000_0000_FFFF_FFFF;
    let bits = v.to_bits();
    if (bits & 0xFFFF_0000_0000_0000) == INT32_TAG {
        ((bits & INT32_MASK) as u32 as i32) as f64
    } else {
        v
    }
}

fn property_name<'a>(ptr: *const u8, len: usize) -> &'a str {
    if ptr.is_null() || len == 0 {
        ""
    } else {
        unsafe { std::str::from_utf8(std::slice::from_raw_parts(ptr, len)).unwrap_or("") }
    }
}

fn json_str_to_value(s: *mut StringHeader) -> f64 {
    if s.is_null() {
        return null();
    }
    f64::from_bits(unsafe { js_json_parse_or_null(s).bits() })
}

fn resolved_promise(value: f64) -> f64 {
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    unsafe { js_promise_resolve(raw, value) };
    nanbox_ptr(raw)
}

fn bind_handle_method(handle: i64, name: &'static [u8]) -> f64 {
    unsafe { js_class_method_bind(nanbox_handle(handle), name.as_ptr(), name.len()) }
}

fn socket_method_name(prop: &str) -> Option<&'static [u8]> {
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
        "getTypeOfService" => Some(b"getTypeOfService"),
        "setTypeOfService" => Some(b"setTypeOfService"),
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

fn server_method_name(prop: &str) -> Option<&'static [u8]> {
    match prop {
        "address" => Some(b"address"),
        "close" => Some(b"close"),
        "getConnections" => Some(b"getConnections"),
        "listen" => Some(b"listen"),
        "ref" => Some(b"ref"),
        "unref" => Some(b"unref"),
        "@@__perry_wk_asyncDispose" => Some(b"@@__perry_wk_asyncDispose"),
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

fn block_list_method_name(prop: &str) -> Option<&'static [u8]> {
    match prop {
        "addAddress" => Some(b"addAddress"),
        "addRange" => Some(b"addRange"),
        "addSubnet" => Some(b"addSubnet"),
        "check" => Some(b"check"),
        "toJSON" => Some(b"toJSON"),
        "fromJSON" => Some(b"fromJSON"),
        _ => None,
    }
}

unsafe fn socket_method(handle: i64, method: &str, args: &[f64]) -> Option<f64> {
    if socket_method_name(method).is_none() || crate::js_ext_net_is_socket_handle(handle) == 0 {
        return None;
    }

    let result = match method {
        "write" if !args.is_empty() => {
            // #5021 — call the DISTINCT, twin-free symbol directly so the
            // write reaches ext-net's registry regardless of link order.
            crate::js_ext_net_socket_write(handle, args[0].to_bits() as i64);
            undefined()
        }
        "end" => {
            let chunk = args.first().copied().unwrap_or_else(undefined);
            crate::js_ext_net_socket_end(handle, chunk.to_bits() as i64);
            undefined()
        }
        "destroy" | "destroySoon" => {
            // Drive teardown through the DISTINCT `js_ext_net_destroy_socket`
            // symbol — NOT the `js_net_socket_destroy` extern, whose symbol the
            // bundled stdlib net twin shadows in a workspace build (it would
            // mark the socket destroyed in stdlib's registry, leaving the
            // adopted raw-`'upgrade'` socket alive in ext-net's). #5010.
            crate::js_ext_net_destroy_socket(handle);
            undefined()
        }
        "on" | "addListener" if args.len() >= 2 => {
            crate::js_net_socket_on(handle, unbox_to_i64(args[0]), unbox_to_i64(args[1]));
            nanbox_handle(handle)
        }
        "connect" if args.len() >= 2 => {
            crate::js_net_socket_method_connect(handle, args[0], unbox_to_i64(args[1]));
            undefined()
        }
        "upgradeToTLS" if !args.is_empty() => {
            let verify = args.get(1).copied().unwrap_or(1.0);
            nanbox_ptr(crate::js_net_socket_upgrade_tls(
                handle,
                unbox_to_i64(args[0]),
                verify,
            ))
        }
        "once" if args.len() >= 2 => {
            crate::js_net_socket_once(handle, unbox_to_i64(args[0]), unbox_to_i64(args[1]));
            nanbox_handle(handle)
        }
        "off" | "removeListener" if args.len() >= 2 => {
            crate::js_net_socket_remove_listener(
                handle,
                unbox_to_i64(args[0]),
                unbox_to_i64(args[1]),
            );
            nanbox_handle(handle)
        }
        "removeAllListeners" => {
            let event = args.first().copied().map(unbox_to_i64).unwrap_or(0);
            crate::js_net_socket_remove_all_listeners(handle, event);
            nanbox_handle(handle)
        }
        "listenerCount" if !args.is_empty() => {
            crate::js_net_socket_listener_count(handle, unbox_to_i64(args[0]))
        }
        "eventNames" => json_str_to_value(crate::js_net_socket_event_names(handle)),
        "listeners" if !args.is_empty() => {
            let arr = crate::js_net_socket_listeners(handle, unbox_to_i64(args[0]));
            nanbox_ptr(arr as *mut ArrayHeader)
        }
        "rawListeners" if !args.is_empty() => {
            let arr = crate::js_net_socket_raw_listeners(handle, unbox_to_i64(args[0]));
            nanbox_ptr(arr as *mut ArrayHeader)
        }
        "address" => json_str_to_value(crate::js_net_socket_address(handle)),
        "getTypeOfService" => crate::js_net_socket_get_type_of_service(handle),
        "setTypeOfService" => {
            let value = args.first().copied().unwrap_or_else(undefined);
            crate::js_net_socket_set_type_of_service(handle, value);
            nanbox_handle(handle)
        }
        "resetAndDestroy" => {
            crate::js_net_socket_reset_and_destroy(handle);
            nanbox_handle(handle)
        }
        "setTimeout" => {
            let msecs = args.first().copied().unwrap_or(0.0);
            let callback = args.get(1).copied().map(unbox_to_i64).unwrap_or(0);
            crate::js_net_socket_set_timeout(handle, msecs, callback);
            nanbox_handle(handle)
        }
        // #4973: real setEncoding — switches 'data' delivery to strings.
        "setEncoding" => {
            let enc_ptr = args
                .first()
                .map(|a| {
                    let bits = a.to_bits();
                    if (bits >> 48) == 0x7FFF {
                        (bits & 0x0000_FFFF_FFFF_FFFF) as i64
                    } else {
                        0
                    }
                })
                .unwrap_or(0);
            crate::js_net_socket_set_encoding(handle, enc_ptr);
            nanbox_handle(handle)
        }
        "setNoDelay" => {
            // Node coerces a missing arg to `true` inside the FFI helper, so
            // pass `undefined` through when called with no argument.
            let arg = args.first().copied().unwrap_or_else(undefined);
            crate::js_net_socket_set_no_delay(handle, arg.to_bits() as i64);
            nanbox_handle(handle)
        }
        "setKeepAlive" | "pause" | "resume" | "ref" | "unref" | "cork" | "uncork"
        | "setDefaultEncoding" => nanbox_handle(handle),
        _ => undefined(),
    };
    Some(result)
}

unsafe fn server_method(handle: i64, method: &str, args: &[f64]) -> Option<f64> {
    if server_method_name(method).is_none() || crate::js_ext_net_is_server_handle(handle) == 0 {
        return None;
    }

    let result = match method {
        "listen" => {
            let port = args.first().copied().unwrap_or(0.0);
            let arg2 = args.get(1).copied().unwrap_or_else(undefined);
            let arg3 = args.get(2).copied().unwrap_or_else(undefined);
            crate::js_net_server_listen(handle, port, arg2, arg3);
            nanbox_handle(handle)
        }
        "close" => {
            let callback = args.first().copied().map(unbox_to_i64).unwrap_or(0);
            crate::js_net_server_close(handle, callback);
            nanbox_handle(handle)
        }
        "@@__perry_wk_asyncDispose" => {
            crate::js_net_server_close(handle, 0);
            resolved_promise(undefined())
        }
        "address" => json_str_to_value(crate::js_net_server_address(handle)),
        "on" | "addListener" if args.len() >= 2 => {
            crate::js_net_server_on(handle, unbox_to_i64(args[0]), unbox_to_i64(args[1]));
            nanbox_handle(handle)
        }
        "once" if args.len() >= 2 => {
            crate::js_net_server_once(handle, unbox_to_i64(args[0]), unbox_to_i64(args[1]));
            nanbox_handle(handle)
        }
        "off" | "removeListener" if args.len() >= 2 => {
            crate::js_net_server_remove_listener(
                handle,
                unbox_to_i64(args[0]),
                unbox_to_i64(args[1]),
            );
            nanbox_handle(handle)
        }
        "removeAllListeners" => {
            let event = args.first().copied().map(unbox_to_i64).unwrap_or(0);
            crate::js_net_server_remove_all_listeners(handle, event);
            nanbox_handle(handle)
        }
        "listenerCount" if !args.is_empty() => {
            crate::js_net_server_listener_count(handle, unbox_to_i64(args[0]))
        }
        "eventNames" => json_str_to_value(crate::js_net_server_event_names(handle)),
        "listeners" if !args.is_empty() => {
            let arr = crate::js_net_server_listeners(handle, unbox_to_i64(args[0]));
            nanbox_ptr(arr as *mut ArrayHeader)
        }
        "rawListeners" if !args.is_empty() => {
            let arr = crate::js_net_server_raw_listeners(handle, unbox_to_i64(args[0]));
            nanbox_ptr(arr as *mut ArrayHeader)
        }
        "ref" | "unref" => nanbox_handle(handle),
        "getConnections" => {
            if let Some(callback) = args.first().copied().map(unbox_to_i64) {
                if callback >= 0x1000 {
                    let cb = JsClosure::from_raw(callback as *const RawClosureHeader);
                    let _ = cb.call2(null(), crate::js_net_server_get_connections(handle));
                }
            }
            undefined()
        }
        _ => undefined(),
    };
    Some(result)
}

unsafe fn block_list_method(handle: i64, method: &str, args: &[f64]) -> Option<f64> {
    if method != "rules" && block_list_method_name(method).is_none()
        || crate::js_ext_net_is_block_list_handle(handle) == 0
    {
        return None;
    }

    let result = match method {
        "addAddress" => {
            let address = args.first().copied().map(unbox_to_i64).unwrap_or(0);
            let family = args.get(1).copied().map(unbox_to_i64).unwrap_or(0);
            crate::js_net_block_list_add_address(handle, address, family)
        }
        "addRange" => {
            let start = args.first().copied().map(unbox_to_i64).unwrap_or(0);
            let end = args.get(1).copied().map(unbox_to_i64).unwrap_or(0);
            let family = args.get(2).copied().map(unbox_to_i64).unwrap_or(0);
            crate::js_net_block_list_add_range(handle, start, end, family)
        }
        "addSubnet" => {
            let address = args.first().copied().map(unbox_to_i64).unwrap_or(0);
            // A missing `prefix` must reach `js_net_validate_block_list_prefix`
            // as a non-numeric value so it throws `ERR_INVALID_ARG_TYPE`, matching
            // Node (the parameter is required). Defaulting to a real number here
            // would silently accept the call as a `/0` subnet.
            let prefix = args
                .get(1)
                .copied()
                .map(unbox_to_f64)
                .unwrap_or_else(undefined);
            let family = args.get(2).copied().map(unbox_to_i64).unwrap_or(0);
            crate::js_net_block_list_add_subnet(handle, address, prefix, family)
        }
        "check" => {
            let address = args.first().copied().map(unbox_to_i64).unwrap_or(0);
            let family = args.get(1).copied().map(unbox_to_i64).unwrap_or(0);
            crate::js_net_block_list_check(handle, address, family)
        }
        "rules" | "toJSON" => crate::js_net_block_list_to_json(handle),
        "fromJSON" => {
            let value = args.first().copied().unwrap_or_else(undefined);
            crate::js_net_block_list_from_json(handle, value)
        }
        _ => undefined(),
    };
    Some(result)
}

#[no_mangle]
pub unsafe extern "C" fn js_ext_net_handle_method_dispatch(
    handle: i64,
    method_name_ptr: *const u8,
    method_name_len: usize,
    args_ptr: *const f64,
    args_len: usize,
    out: *mut f64,
) -> i32 {
    let method = property_name(method_name_ptr, method_name_len);
    let args = if args_ptr.is_null() || args_len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(args_ptr, args_len)
    };
    let value = socket_method(handle, method, args)
        .or_else(|| server_method(handle, method, args))
        .or_else(|| block_list_method(handle, method, args));
    if let Some(value) = value {
        if !out.is_null() {
            *out = value;
        }
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_ext_net_handle_property_dispatch(
    handle: i64,
    property_name_ptr: *const u8,
    property_name_len: usize,
    out: *mut f64,
) -> i32 {
    let prop = property_name(property_name_ptr, property_name_len);
    let value = if matches!(prop, "address" | "family" | "port" | "flowlabel")
        && crate::js_ext_net_is_socket_address_handle(handle) != 0
    {
        Some(match prop {
            "address" => f64::from_bits(
                JsValue::from_string_ptr(crate::js_net_socket_address_get_address(handle)).bits(),
            ),
            "family" => f64::from_bits(
                JsValue::from_string_ptr(crate::js_net_socket_address_get_family(handle)).bits(),
            ),
            "port" => crate::js_net_socket_address_get_port(handle),
            _ => crate::js_net_socket_address_get_flowlabel(handle),
        })
    } else if let Some(name) = socket_method_name(prop) {
        if crate::js_ext_net_is_socket_handle(handle) != 0 {
            Some(bind_handle_method(handle, name))
        } else {
            None
        }
    } else if let Some(name) = server_method_name(prop) {
        if crate::js_ext_net_is_server_handle(handle) != 0 {
            Some(bind_handle_method(handle, name))
        } else {
            None
        }
    } else if let Some(name) = block_list_method_name(prop) {
        if crate::js_ext_net_is_block_list_handle(handle) != 0 {
            Some(bind_handle_method(handle, name))
        } else {
            None
        }
    } else if prop == "rules" && crate::js_ext_net_is_block_list_handle(handle) != 0 {
        Some(nanbox_ptr(crate::js_net_block_list_rules(handle)))
    } else if prop == "listening" && crate::js_ext_net_is_server_handle(handle) != 0 {
        Some(crate::js_net_server_get_listening(handle))
    } else if matches!(prop, "maxConnections" | "dropMaxConnection")
        && crate::js_ext_net_is_server_handle(handle) != 0
    {
        Some(match prop {
            "maxConnections" => crate::js_net_server_get_max_connections(handle),
            _ => crate::js_net_server_get_drop_max_connection(handle),
        })
    } else {
        None
    };

    if let Some(value) = value {
        if !out.is_null() {
            *out = value;
        }
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn js_ext_net_handle_property_set_dispatch(
    handle: i64,
    property_name_ptr: *const u8,
    property_name_len: usize,
    value: f64,
) -> i32 {
    if crate::js_ext_net_is_server_handle(handle) == 0 {
        return 0;
    }

    match property_name(property_name_ptr, property_name_len) {
        "maxConnections" => {
            crate::js_net_server_set_max_connections(handle, value);
            1
        }
        "dropMaxConnection" => {
            crate::js_net_server_set_drop_max_connection(handle, value);
            1
        }
        _ => 0,
    }
}
