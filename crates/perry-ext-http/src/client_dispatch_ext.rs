//! Runtime handle-dispatch EXTENSION registration for HTTP-client handles
//! (`ClientRequest`, the client `IncomingMessage`, `Agent`).
//!
//! The client-side twin of `server/dispatch_ext.rs` (Wall 10). perry-stdlib's
//! `js_handle_{method,property,property_set}_dispatch` carry these arms behind
//! `external-http-client-pump`, which only an auto-optimized stdlib has. The
//! prebuilt `full` archive `PERRY_NO_AUTO_OPTIMIZE=1` links compiles them OUT,
//! so a client handle reached through an erased receiver — the `req` returned
//! by `const request = http.request; request(opts)` in ws, or any handle passed
//! through untyped code — answered `undefined` for `req.on` / `req.end` and the
//! request was never sent (#10428).
//!
//! Registered extensions run before the stdlib primary regardless of its
//! features. Each one claims a call only when the handle belongs to this crate
//! AND the name is in the same vocabulary the stdlib arms gate on (keep these
//! lists in sync with `perry-stdlib/src/common/dispatch_http.rs` and the
//! `external-http-client-pump` arms in `dispatch/{method,property}_dispatch.rs`
//! and `dispatch/init.rs`); any other name falls through unchanged.

use std::sync::Once;

use perry_ffi::StringHeader;

const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
const PTR_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

extern "C" {
    fn js_register_handle_method_dispatch_extension(
        f: unsafe extern "C" fn(i64, *const u8, usize, *const f64, usize, *mut f64) -> i32,
    );
    fn js_register_handle_property_dispatch_extension(
        f: unsafe extern "C" fn(i64, *const u8, usize, *mut f64) -> i32,
    );
    fn js_register_handle_property_set_dispatch_extension(
        f: unsafe extern "C" fn(i64, *const u8, usize, f64) -> i32,
    );
    fn js_class_method_bind(
        instance: f64,
        method_name_ptr: *const u8,
        method_name_len: usize,
    ) -> f64;
}

/// Register the three client handle-dispatch extensions. Called from the
/// client's one-time initialization, which every request and agent factory
/// runs before a handle exists.
pub(crate) fn ensure_registered() {
    static REGISTER: Once = Once::new();
    REGISTER.call_once(|| unsafe {
        js_register_handle_method_dispatch_extension(http_client_method_dispatch_ext);
        js_register_handle_property_dispatch_extension(http_client_property_dispatch_ext);
        js_register_handle_property_set_dispatch_extension(http_client_property_set_dispatch_ext);
    });
}

fn is_agent_method(name: &str) -> bool {
    matches!(
        name,
        "getName" | "destroy" | "keepSocketAlive" | "reuseSocket" | "createConnection"
    )
}

fn is_agent_property(name: &str) -> bool {
    matches!(
        name,
        "createConnection"
            | "createSocket"
            | "keepSocketAlive"
            | "reuseSocket"
            | "getName"
            | "destroy"
            | "maxSockets"
            | "maxFreeSockets"
            | "maxTotalSockets"
            | "totalSocketCount"
            | "keepAliveMsecs"
            | "agentKeepAliveTimeoutBuffer"
            | "keepAlive"
            | "destroyed"
            | "defaultPort"
            | "protocol"
            | "sockets"
            | "freeSockets"
            | "requests"
            | "_sessionCache"
    )
}

fn is_agent_writable(name: &str) -> bool {
    matches!(
        name,
        "maxSockets"
            | "maxFreeSockets"
            | "maxTotalSockets"
            | "keepAliveMsecs"
            | "agentKeepAliveTimeoutBuffer"
            | "keepAlive"
            | "createConnection"
            | "createSocket"
    )
}

fn is_client_request_method(name: &str) -> bool {
    matches!(
        name,
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
            | "on"
            | "once"
            | "addListener"
            | "prependListener"
            | "removeListener"
            | "off"
            | "removeAllListeners"
    )
}

fn is_client_request_property(name: &str) -> bool {
    is_client_request_method(name)
        || matches!(
            name,
            "method"
                | "protocol"
                | "host"
                | "path"
                | "aborted"
                | "destroyed"
                | "finished"
                | "reusedSocket"
                | "maxHeadersCount"
                | "writableEnded"
                | "writableFinished"
                | "socket"
                | "connection"
                | "constructor"
        )
}

fn is_incoming_message_property(name: &str) -> bool {
    matches!(
        name,
        "statusCode"
            | "statusMessage"
            | "headers"
            | "trailers"
            | "setEncoding"
            | "socket"
            | "connection"
            | "req"
    )
}

#[inline]
unsafe fn name_str<'a>(ptr: *const u8, len: usize) -> &'a str {
    if ptr.is_null() || len == 0 {
        ""
    } else {
        std::str::from_utf8(std::slice::from_raw_parts(ptr, len)).unwrap_or("")
    }
}

#[inline]
unsafe fn claim(out: *mut f64, value: f64) -> i32 {
    if !out.is_null() {
        *out = value;
    }
    1
}

/// Mirrors perry-stdlib's `dispatch_client_incoming_method` plus the
/// `pause`/`resume` arm (Node's `Readable.pause()/resume()` return `this`).
unsafe fn incoming_message_method(handle: i64, name: &str, args: &[f64]) -> Option<f64> {
    if !matches!(
        name,
        "setEncoding" | "on" | "once" | "addListener" | "pipe" | "pause" | "resume"
    ) || crate::client_surface::js_http_is_incoming_message(handle) == 0
    {
        return None;
    }
    let self_ref = f64::from_bits(POINTER_TAG | (handle as u64 & PTR_MASK));
    let arg_ptr = |n: usize| (args[n].to_bits() & PTR_MASK) as *const StringHeader;
    Some(match name {
        "pause" | "resume" => self_ref,
        "setEncoding" if !args.is_empty() => {
            crate::client_surface::js_http_incoming_message_set_encoding(handle, arg_ptr(0));
            self_ref
        }
        "on" | "addListener" if args.len() >= 2 => {
            crate::js_http_on(handle, arg_ptr(0), (args[1].to_bits() & PTR_MASK) as i64);
            self_ref
        }
        "once" if args.len() >= 2 => {
            crate::js_http_once(handle, arg_ptr(0), (args[1].to_bits() & PTR_MASK) as i64);
            self_ref
        }
        "pipe" if !args.is_empty() => {
            crate::client_surface::js_http_incoming_message_pipe(handle, args[0])
        }
        _ => f64::from_bits(TAG_UNDEFINED),
    })
}

unsafe extern "C" fn http_client_method_dispatch_ext(
    handle: i64,
    method_ptr: *const u8,
    method_len: usize,
    args_ptr: *const f64,
    args_len: usize,
    out: *mut f64,
) -> i32 {
    let name = name_str(method_ptr, method_len);
    if name.is_empty() {
        return 0;
    }
    if is_agent_method(name) && crate::js_ext_http_agent_is_handle(handle) != 0 {
        let v = crate::js_ext_http_agent_dispatch_method(
            handle, method_ptr, method_len, args_ptr, args_len,
        );
        return claim(out, v);
    }
    if is_client_request_method(name)
        && crate::client_request_surface::js_ext_http_client_request_is_handle(handle) != 0
    {
        let v = crate::client_request_surface::js_ext_http_client_request_dispatch_method(
            handle, method_ptr, method_len, args_ptr, args_len,
        );
        return claim(out, v);
    }
    let args: &[f64] = if args_ptr.is_null() || args_len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(args_ptr, args_len)
    };
    match incoming_message_method(handle, name, args) {
        Some(v) => claim(out, v),
        None => 0,
    }
}

unsafe extern "C" fn http_client_property_dispatch_ext(
    handle: i64,
    property_ptr: *const u8,
    property_len: usize,
    out: *mut f64,
) -> i32 {
    let name = name_str(property_ptr, property_len);
    if name.is_empty() {
        return 0;
    }
    if is_agent_property(name) && crate::js_ext_http_agent_is_handle(handle) != 0 {
        let v = crate::js_ext_http_agent_dispatch_property(handle, property_ptr, property_len);
        return claim(out, v);
    }
    if is_client_request_property(name)
        && crate::client_request_surface::js_ext_http_client_request_is_handle(handle) != 0
    {
        let v = crate::client_request_surface::js_ext_http_client_request_dispatch_property(
            handle,
            property_ptr,
            property_len,
        );
        return claim(out, v);
    }
    if !is_incoming_message_property(name)
        || crate::client_surface::js_http_is_incoming_message(handle) == 0
    {
        return 0;
    }
    use crate::client_surface as im;
    let v = match name {
        "setEncoding" => {
            // Same bound-method value perry-stdlib's arm produces.
            js_class_method_bind(handle as f64, name.as_ptr(), name.len())
        }
        "statusCode" => im::js_http_status_code(handle),
        "statusMessage" => {
            let ptr = im::js_http_status_message(handle);
            if ptr.is_null() {
                f64::from_bits(TAG_UNDEFINED)
            } else {
                f64::from_bits(0x7FFF_0000_0000_0000 | (ptr as u64 & PTR_MASK))
            }
        }
        "headers" => im::js_http_response_headers(handle),
        "trailers" => im::js_http_response_trailers(handle),
        "socket" | "connection" => im::js_http_incoming_message_socket(handle),
        _ => im::js_http_incoming_message_req(handle),
    };
    claim(out, v)
}

unsafe extern "C" fn http_client_property_set_dispatch_ext(
    handle: i64,
    property_ptr: *const u8,
    property_len: usize,
    value: f64,
) -> i32 {
    let name = name_str(property_ptr, property_len);
    if !is_agent_writable(name) || crate::js_ext_http_agent_is_handle(handle) == 0 {
        return 0;
    }
    crate::js_ext_http_agent_dispatch_property_set(handle, property_ptr, property_len, value);
    1
}
