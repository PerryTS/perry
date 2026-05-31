//! Runtime method dispatch for `HttpServer` handles whose static type
//! the codegen lost (e.g. `const s: any = http.createServer(...)`).
//!
//! When the static class of the receiver is unknown, codegen emits
//! `js_typed_feedback_native_call_method` which forwards to perry-runtime's
//! `js_native_call_method`. That dispatcher walks several handle registries
//! (Buffer, TypedArray, Fastify, ioredis, zlib, …) but had no arm for the
//! HTTP-server handles registered by `js_node_http_create_server`. The
//! call therefore returned undefined-or-NaN even though the
//! `("http", "HttpServer", "listen"|"close"|"on"|…)` rows in
//! `crates/perry-codegen/src/lower_call/native_table/http.rs` describe a
//! valid direct dispatch.
//!
//! This module exposes a probe + dispatcher that mirror the zlib stream
//! dispatch pattern (`crates/perry-ext-zlib/src/stream.rs`). perry-stdlib's
//! `js_handle_method_dispatch` (gated on the `external-http-server-pump`
//! feature, which `optimized_libs.rs` already auto-activates whenever
//! `node:http` / `node:https` / `node:http2` is imported) calls
//! `js_ext_http_server_is_handle`; on a hit it forwards to
//! `js_ext_http_server_dispatch_method`, which routes to the same
//! `js_node_http_server_*` externs that the static native_table path uses.
//!
//! Issue #2153.

use perry_ffi::{alloc_string, get_handle, JsValue, StringHeader};

use crate::https_server::HttpsServer;
use crate::request::IncomingMessage;
use crate::response::ServerResponse;
use crate::server::HttpServer;
use crate::types::{POINTER_TAG, PTR_MASK, TAG_UNDEFINED};

extern "C" {
    fn js_node_http_server_listen(server_handle: i64, args_array: i64);
    fn js_node_http_server_close(server_handle: i64, callback: i64);
    fn js_node_http_server_close_all_connections(handle: i64);
    fn js_node_http_server_close_idle_connections(handle: i64);
    fn js_node_http_server_address_json(handle: i64) -> *mut StringHeader;
    fn js_node_http_server_on(
        handle: i64,
        event_name_ptr: *const StringHeader,
        callback: i64,
    ) -> f64;
    fn js_node_http_server_set_timeout_method(handle: i64, msecs: f64, callback: i64) -> i64;
    fn js_node_https_server_listen(server_handle: i64, args_array: i64) -> i64;
    fn js_node_https_server_close(server_handle: i64, callback: i64);
    fn js_node_https_server_close_all_connections(handle: i64);
    fn js_node_https_server_close_idle_connections(handle: i64);
    fn js_node_https_server_address_json(handle: i64) -> *mut StringHeader;
    fn js_node_https_server_on(
        handle: i64,
        event_name_ptr: *const StringHeader,
        callback: i64,
    ) -> f64;
    fn js_node_https_server_set_timeout_method(handle: i64, msecs: f64, callback: i64) -> i64;
    /// Runtime-side JSON.parse — converts the JSON-encoded `address()`
    /// payload into the `{ port, address, family }` object Node returns.
    /// Returns the JSValue bits as u64 (NaN-boxed value).
    fn js_json_parse(text_ptr: *const StringHeader) -> u64;
    fn js_class_method_bind(
        instance: f64,
        method_name_ptr: *const u8,
        method_name_len: usize,
    ) -> f64;

    fn js_node_http_im_method(handle: i64) -> *mut StringHeader;
    fn js_node_http_im_url(handle: i64) -> *mut StringHeader;
    fn js_node_http_im_http_version(handle: i64) -> *mut StringHeader;
    fn js_node_http_im_headers_json(handle: i64) -> *mut StringHeader;
    fn js_node_http_im_raw_headers_json(handle: i64) -> *mut StringHeader;
    fn js_node_http_im_complete(handle: i64) -> i32;
    fn js_node_http_im_aborted(handle: i64) -> i32;
    fn js_node_http_im_destroyed(handle: i64) -> i32;
    fn js_node_http_im_pause(handle: i64);
    fn js_node_http_im_resume(handle: i64);
    fn js_node_http_im_destroy(handle: i64);
    fn js_node_http_im_on(handle: i64, event_name_ptr: *const StringHeader, callback: i64) -> f64;
    fn js_node_http_im_set_encoding(handle: i64, encoding_ptr: *const StringHeader) -> i64;
    fn js_node_http_im_read(handle: i64) -> f64;

    fn js_node_http_res_set_status(handle: i64, code: f64);
    fn js_node_http_res_get_status(handle: i64) -> f64;
    fn js_node_http_res_set_status_message(handle: i64, msg_ptr: *const StringHeader);
    fn js_node_http_res_set_header(
        handle: i64,
        name_ptr: *const StringHeader,
        value_ptr: *const StringHeader,
    );
    fn js_node_http_res_get_header(handle: i64, name_ptr: *const StringHeader) -> f64;
    fn js_node_http_res_remove_header(handle: i64, name_ptr: *const StringHeader);
    fn js_node_http_res_has_header(handle: i64, name_ptr: *const StringHeader) -> i32;
    fn js_node_http_res_headers_sent(handle: i64) -> i32;
    fn js_node_http_res_writable_ended(handle: i64) -> i32;
    fn js_node_http_res_writable_finished(handle: i64) -> i32;
    fn js_node_http_res_write_head(handle: i64, status: f64, arg2: i64, arg3: i64);
    fn js_node_http_res_write(handle: i64, chunk: f64) -> i32;
    fn js_node_http_res_add_trailers(handle: i64, headers_value: f64);
    fn js_node_http_res_end(handle: i64, chunk: f64);
    fn js_node_http_res_flush_headers(handle: i64);
    fn js_node_http_res_write_continue(handle: i64);
    fn js_node_http_res_write_processing(handle: i64);
    fn js_node_http_res_on(handle: i64, event_name_ptr: *const StringHeader, callback: i64) -> f64;
}

/// Probe: is `handle` a live `HttpServer`?
///
/// Stdlib uses this together with the method-name vocabulary below to gate
/// the dispatch arm so a handle id reused across another registry doesn't
/// misroute.
#[no_mangle]
pub extern "C" fn js_ext_http_server_is_handle(handle: i64) -> i32 {
    if get_handle::<HttpServer>(handle).is_some() || get_handle::<HttpsServer>(handle).is_some() {
        1
    } else {
        0
    }
}

/// Probe: is `handle` a live server-side `IncomingMessage`?
#[no_mangle]
pub extern "C" fn js_ext_http_incoming_message_is_handle(handle: i64) -> i32 {
    if get_handle::<IncomingMessage>(handle).is_some() {
        1
    } else {
        0
    }
}

/// Probe: is `handle` a live server-side `ServerResponse`?
#[no_mangle]
pub extern "C" fn js_ext_http_server_response_is_handle(handle: i64) -> i32 {
    if get_handle::<ServerResponse>(handle).is_some() {
        1
    } else {
        0
    }
}

/// Methods this dispatcher claims. Kept in sync with the
/// `class_filter: Some("HttpServer")` rows in
/// `crates/perry-codegen/src/lower_call/native_table/http.rs`.
pub const HTTP_SERVER_METHODS: &[&str] = &[
    "listen",
    "close",
    "closeAllConnections",
    "closeIdleConnections",
    "address",
    "on",
    "addListener",
    "setTimeout",
];

fn http_server_method_bytes(name: &str) -> Option<&'static [u8]> {
    match name {
        "listen" => Some(b"listen"),
        "close" => Some(b"close"),
        "closeAllConnections" => Some(b"closeAllConnections"),
        "closeIdleConnections" => Some(b"closeIdleConnections"),
        "address" => Some(b"address"),
        "on" => Some(b"on"),
        "addListener" => Some(b"addListener"),
        "setTimeout" => Some(b"setTimeout"),
        _ => None,
    }
}

/// Build a transient `ArrayHeader`-shaped buffer carrying NaN-boxed args.
/// `js_node_http_server_listen` reads its `args_array` arg as a raw
/// `*const ArrayHeader`; the codegen's `NA_VARARGS` path packs one for the
/// direct dispatch, so we mimic that layout here. The buffer lives only
/// for the duration of the call.
#[repr(C)]
struct InlineArgsHeader {
    length: u32,
    capacity: u32,
    // up to 8 packed u64 args follow inline
    args: [u64; 8],
}

/// Dispatch a method on a registered `HttpServer` handle. Method name is a
/// UTF-8 ptr+len; args are NaN-boxed f64s (the perry-runtime
/// `js_native_call_method` shape). Returns NaN-boxed undefined for methods
/// outside the table above.
///
/// # Safety
/// FFI entry; pointers must be valid for their stated lengths.
#[no_mangle]
pub unsafe extern "C" fn js_ext_http_server_dispatch_method(
    handle: i64,
    method_ptr: *const u8,
    method_len: usize,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    let undef = f64::from_bits(TAG_UNDEFINED);
    if method_ptr.is_null() || method_len == 0 {
        return undef;
    }
    let method =
        String::from_utf8_lossy(std::slice::from_raw_parts(method_ptr, method_len)).into_owned();
    let args: &[f64] = if args_len > 0 && !args_ptr.is_null() {
        std::slice::from_raw_parts(args_ptr, args_len)
    } else {
        &[]
    };
    let is_https = get_handle::<HttpsServer>(handle).is_some();
    // Server re-boxed as POINTER_TAG so chained calls (`server.on(...).on(...)`,
    // `server.listen(...).address()`) keep flowing through this same dispatcher.
    let self_ref = f64::from_bits(POINTER_TAG | (handle as u64 & PTR_MASK));

    match method.as_str() {
        "listen" => {
            let n = args.len().min(8);
            let mut inline = InlineArgsHeader {
                length: n as u32,
                capacity: n as u32,
                args: [0; 8],
            };
            for i in 0..n {
                inline.args[i] = args[i].to_bits();
            }
            let args_array = &inline as *const _ as i64;
            if is_https {
                js_node_https_server_listen(handle, args_array);
            } else {
                js_node_http_server_listen(handle, args_array);
            }
            // Node returns the server for chaining (`createServer(...).listen(p).address()`).
            self_ref
        }
        "close" => {
            let cb = closure_arg(args.first().copied());
            if is_https {
                js_node_https_server_close(handle, cb);
            } else {
                js_node_http_server_close(handle, cb);
            }
            self_ref
        }
        "closeAllConnections" => {
            if is_https {
                js_node_https_server_close_all_connections(handle);
            } else {
                js_node_http_server_close_all_connections(handle);
            }
            undef
        }
        "closeIdleConnections" => {
            if is_https {
                js_node_https_server_close_idle_connections(handle);
            } else {
                js_node_http_server_close_idle_connections(handle);
            }
            undef
        }
        "address" => {
            // Node returns `{ port, address, family }` or null. The FFI hands
            // back a JSON-encoded string (`"null"` when not listening); run
            // it through JSON.parse so the value the caller sees matches Node.
            let s = if is_https {
                js_node_https_server_address_json(handle)
            } else {
                js_node_http_server_address_json(handle)
            };
            if s.is_null() {
                f64::from_bits(crate::types::TAG_NULL)
            } else {
                f64::from_bits(js_json_parse(s))
            }
        }
        "on" | "addListener" if args.len() >= 2 => {
            let event_ptr = string_arg(args[0]);
            if event_ptr.is_null() {
                return self_ref;
            }
            let cb = closure_arg(Some(args[1]));
            if is_https {
                js_node_https_server_on(handle, event_ptr, cb);
            } else {
                js_node_http_server_on(handle, event_ptr, cb);
            }
            self_ref
        }
        "setTimeout" => {
            let msecs = args.first().copied().unwrap_or(0.0);
            let cb = closure_arg(args.get(1).copied());
            if is_https {
                js_node_https_server_set_timeout_method(handle, msecs, cb);
            } else {
                js_node_http_server_set_timeout_method(handle, msecs, cb);
            }
            self_ref
        }
        // `listening` is a property on the JS side; the only known property
        // bound through this dispatcher would be `.listening()` — Node doesn't
        // expose it as a method, so fall through to undef.
        _ => undef,
    }
}

/// Dispatch a property read on a registered `HttpServer` / `HttpsServer`
/// handle. Bare method-value reads bind to a callable closure.
#[no_mangle]
pub unsafe extern "C" fn js_ext_http_server_dispatch_property(
    handle: i64,
    property_ptr: *const u8,
    property_len: usize,
) -> f64 {
    let undef = f64::from_bits(TAG_UNDEFINED);
    let property = method_name(property_ptr, property_len);
    if property.is_empty() {
        return undef;
    }
    if let Some(name) = http_server_method_bytes(&property) {
        return bind_handle_method(handle, name);
    }
    undef
}

/// Dispatch a method on a registered server-side `IncomingMessage` handle.
///
/// # Safety
/// FFI entry; pointers must be valid for their stated lengths.
#[no_mangle]
pub unsafe extern "C" fn js_ext_http_incoming_message_dispatch_method(
    handle: i64,
    method_ptr: *const u8,
    method_len: usize,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    let undef = f64::from_bits(TAG_UNDEFINED);
    let method = method_name(method_ptr, method_len);
    if method.is_empty() {
        return undef;
    }
    let args = args_slice(args_ptr, args_len);
    let self_ref = handle_to_pointer_f64(handle);

    match method.as_str() {
        "on" | "addListener" if args.len() >= 2 => {
            let event_ptr = string_arg(args[0]);
            if event_ptr.is_null() {
                return self_ref;
            }
            js_node_http_im_on(handle, event_ptr, closure_arg(Some(args[1])));
            self_ref
        }
        "setEncoding" if !args.is_empty() => {
            let encoding_ptr = string_value_arg(args[0]);
            if !encoding_ptr.is_null() {
                js_node_http_im_set_encoding(handle, encoding_ptr);
            }
            self_ref
        }
        "pause" => {
            js_node_http_im_pause(handle);
            self_ref
        }
        "resume" => {
            js_node_http_im_resume(handle);
            self_ref
        }
        "destroy" => {
            js_node_http_im_destroy(handle);
            self_ref
        }
        "read" => js_node_http_im_read(handle),
        "method" | "__get_method" => string_ptr_value(js_node_http_im_method(handle)),
        "url" | "__get_url" => string_ptr_value(js_node_http_im_url(handle)),
        "httpVersion" | "__get_httpVersion" => {
            string_ptr_value(js_node_http_im_http_version(handle))
        }
        "__get_complete" => bool_value(js_node_http_im_complete(handle) != 0),
        "__get_aborted" => bool_value(js_node_http_im_aborted(handle) != 0),
        "__get_destroyed" => bool_value(js_node_http_im_destroyed(handle) != 0),
        "__get_headers" | "headers" => json_string_value(js_node_http_im_headers_json(handle)),
        "__get_rawHeaders" | "rawHeaders" => {
            json_string_value(js_node_http_im_raw_headers_json(handle))
        }
        _ => undef,
    }
}

/// Dispatch a method on a registered server-side `ServerResponse` handle.
///
/// # Safety
/// FFI entry; pointers must be valid for their stated lengths.
#[no_mangle]
pub unsafe extern "C" fn js_ext_http_server_response_dispatch_method(
    handle: i64,
    method_ptr: *const u8,
    method_len: usize,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    let undef = f64::from_bits(TAG_UNDEFINED);
    let method = method_name(method_ptr, method_len);
    if method.is_empty() {
        return undef;
    }
    let args = args_slice(args_ptr, args_len);
    let self_ref = handle_to_pointer_f64(handle);

    match method.as_str() {
        "setHeader" if args.len() >= 2 => {
            let name = string_value_arg(args[0]);
            if !name.is_null() {
                js_node_http_res_set_header(handle, name, string_value_arg(args[1]));
            }
            undef
        }
        "getHeader" if !args.is_empty() => {
            let name = string_value_arg(args[0]);
            if name.is_null() {
                undef
            } else {
                js_node_http_res_get_header(handle, name)
            }
        }
        "removeHeader" if !args.is_empty() => {
            let name = string_value_arg(args[0]);
            if !name.is_null() {
                js_node_http_res_remove_header(handle, name);
            }
            undef
        }
        "hasHeader" if !args.is_empty() => {
            let name = string_value_arg(args[0]);
            bool_value(!name.is_null() && js_node_http_res_has_header(handle, name) != 0)
        }
        "writeHead" if !args.is_empty() => {
            js_node_http_res_write_head(
                handle,
                number_arg(Some(args[0]), 200.0),
                raw_arg(args.get(1).copied()),
                raw_arg(args.get(2).copied()),
            );
            self_ref
        }
        "write" if !args.is_empty() => bool_value(js_node_http_res_write(handle, args[0]) != 0),
        "addTrailers" if !args.is_empty() => {
            js_node_http_res_add_trailers(handle, args[0]);
            undef
        }
        "end" => {
            js_node_http_res_end(handle, args.first().copied().unwrap_or(undef));
            self_ref
        }
        "flushHeaders" => {
            js_node_http_res_flush_headers(handle);
            undef
        }
        "writeContinue" => {
            js_node_http_res_write_continue(handle);
            undef
        }
        "writeProcessing" => {
            js_node_http_res_write_processing(handle);
            undef
        }
        "on" | "addListener" if args.len() >= 2 => {
            let event_ptr = string_arg(args[0]);
            if event_ptr.is_null() {
                return self_ref;
            }
            js_node_http_res_on(handle, event_ptr, closure_arg(Some(args[1])));
            self_ref
        }
        "setStatus" | "__set_statusCode" if !args.is_empty() => {
            js_node_http_res_set_status(handle, number_arg(Some(args[0]), 200.0));
            undef
        }
        "getStatus" | "__get_statusCode" => js_node_http_res_get_status(handle),
        "__set_statusMessage" if !args.is_empty() => {
            let msg = string_value_arg(args[0]);
            if !msg.is_null() {
                js_node_http_res_set_status_message(handle, msg);
            }
            undef
        }
        "__get_headersSent" => bool_value(js_node_http_res_headers_sent(handle) != 0),
        "__get_writableEnded" => bool_value(js_node_http_res_writable_ended(handle) != 0),
        "__get_writableFinished" => bool_value(js_node_http_res_writable_finished(handle) != 0),
        _ => undef,
    }
}

/// Dispatch a property read on a registered server-side `IncomingMessage`.
///
/// # Safety
/// FFI entry; pointers must be valid for their stated lengths.
#[no_mangle]
pub unsafe extern "C" fn js_ext_http_incoming_message_dispatch_property(
    handle: i64,
    property_ptr: *const u8,
    property_len: usize,
) -> f64 {
    let undef = f64::from_bits(TAG_UNDEFINED);
    let property = method_name(property_ptr, property_len);
    if property.is_empty() {
        return undef;
    }

    if let Some(name) = incoming_method_bytes(&property) {
        return bind_handle_method(handle, name);
    }

    match property.as_str() {
        "method" => string_ptr_value(js_node_http_im_method(handle)),
        "url" => string_ptr_value(js_node_http_im_url(handle)),
        "httpVersion" => string_ptr_value(js_node_http_im_http_version(handle)),
        "headers" => json_string_value(js_node_http_im_headers_json(handle)),
        "rawHeaders" => json_string_value(js_node_http_im_raw_headers_json(handle)),
        "complete" => bool_value(js_node_http_im_complete(handle) != 0),
        "aborted" => bool_value(js_node_http_im_aborted(handle) != 0),
        "destroyed" => bool_value(js_node_http_im_destroyed(handle) != 0),
        _ => undef,
    }
}

/// Dispatch a property read on a registered server-side `ServerResponse`.
///
/// # Safety
/// FFI entry; pointers must be valid for their stated lengths.
#[no_mangle]
pub unsafe extern "C" fn js_ext_http_server_response_dispatch_property(
    handle: i64,
    property_ptr: *const u8,
    property_len: usize,
) -> f64 {
    let undef = f64::from_bits(TAG_UNDEFINED);
    let property = method_name(property_ptr, property_len);
    if property.is_empty() {
        return undef;
    }

    if let Some(name) = server_response_method_bytes(&property) {
        return bind_handle_method(handle, name);
    }

    match property.as_str() {
        "statusCode" => js_node_http_res_get_status(handle),
        "headersSent" => bool_value(js_node_http_res_headers_sent(handle) != 0),
        "writableEnded" => bool_value(js_node_http_res_writable_ended(handle) != 0),
        "writableFinished" => bool_value(js_node_http_res_writable_finished(handle) != 0),
        _ => undef,
    }
}

/// Dispatch a property write on a registered server-side `ServerResponse`.
///
/// Returns 1 when the property was claimed.
///
/// # Safety
/// FFI entry; pointers must be valid for their stated lengths.
#[no_mangle]
pub unsafe extern "C" fn js_ext_http_server_response_dispatch_property_set(
    handle: i64,
    property_ptr: *const u8,
    property_len: usize,
    value: f64,
) -> i32 {
    let property = method_name(property_ptr, property_len);
    match property.as_str() {
        "statusCode" => {
            js_node_http_res_set_status(handle, number_arg(Some(value), 200.0));
            1
        }
        "statusMessage" => {
            let msg = string_value_arg(value);
            if !msg.is_null() {
                js_node_http_res_set_status_message(handle, msg);
            }
            1
        }
        _ => 0,
    }
}

#[inline]
unsafe fn method_name(ptr: *const u8, len: usize) -> String {
    if ptr.is_null() || len == 0 {
        String::new()
    } else {
        String::from_utf8_lossy(std::slice::from_raw_parts(ptr, len)).into_owned()
    }
}

#[inline]
unsafe fn args_slice<'a>(args_ptr: *const f64, args_len: usize) -> &'a [f64] {
    if args_len > 0 && !args_ptr.is_null() {
        std::slice::from_raw_parts(args_ptr, args_len)
    } else {
        &[]
    }
}

#[inline]
fn handle_to_pointer_f64(handle: i64) -> f64 {
    f64::from_bits(POINTER_TAG | (handle as u64 & PTR_MASK))
}

#[inline]
fn string_ptr_value(ptr: *mut StringHeader) -> f64 {
    if ptr.is_null() {
        f64::from_bits(TAG_UNDEFINED)
    } else {
        f64::from_bits(JsValue::from_string_ptr(ptr).bits())
    }
}

#[inline]
fn json_string_value(ptr: *mut StringHeader) -> f64 {
    if ptr.is_null() {
        f64::from_bits(TAG_UNDEFINED)
    } else {
        unsafe { f64::from_bits(js_json_parse(ptr)) }
    }
}

#[inline]
fn bool_value(value: bool) -> f64 {
    f64::from_bits(JsValue::from_bool(value).bits())
}

#[inline]
fn number_arg(value: Option<f64>, fallback: f64) -> f64 {
    let Some(value) = value else { return fallback };
    let v = JsValue::from_bits(value.to_bits());
    if v.is_number() {
        v.to_number()
    } else {
        fallback
    }
}

#[inline]
fn raw_arg(value: Option<f64>) -> i64 {
    value
        .unwrap_or_else(|| f64::from_bits(TAG_UNDEFINED))
        .to_bits() as i64
}

#[inline]
fn string_value_arg(value: f64) -> *const StringHeader {
    let v = JsValue::from_bits(value.to_bits());
    if v.is_string() {
        return v.as_string_ptr();
    }
    match crate::types::jsvalue_to_owned_string(value) {
        Some(s) => alloc_string(&s).as_raw(),
        None => std::ptr::null(),
    }
}

#[inline]
fn bind_handle_method(handle: i64, name: &'static [u8]) -> f64 {
    unsafe { js_class_method_bind(handle_to_pointer_f64(handle), name.as_ptr(), name.len()) }
}

fn incoming_method_bytes(name: &str) -> Option<&'static [u8]> {
    match name {
        "on" => Some(b"on"),
        "addListener" => Some(b"addListener"),
        "setEncoding" => Some(b"setEncoding"),
        "pause" => Some(b"pause"),
        "resume" => Some(b"resume"),
        "destroy" => Some(b"destroy"),
        "read" => Some(b"read"),
        _ => None,
    }
}

fn server_response_method_bytes(name: &str) -> Option<&'static [u8]> {
    match name {
        "setHeader" => Some(b"setHeader"),
        "getHeader" => Some(b"getHeader"),
        "removeHeader" => Some(b"removeHeader"),
        "hasHeader" => Some(b"hasHeader"),
        "writeHead" => Some(b"writeHead"),
        "write" => Some(b"write"),
        "addTrailers" => Some(b"addTrailers"),
        "end" => Some(b"end"),
        "flushHeaders" => Some(b"flushHeaders"),
        "writeContinue" => Some(b"writeContinue"),
        "writeProcessing" => Some(b"writeProcessing"),
        "on" => Some(b"on"),
        "addListener" => Some(b"addListener"),
        _ => None,
    }
}

/// Strip a NaN-boxed string arg to the raw `*const StringHeader` pointer the
/// existing `js_node_http_server_on` / `_im_on` FFI expects.
#[inline]
fn string_arg(value: f64) -> *const StringHeader {
    let v = JsValue::from_bits(value.to_bits());
    if !v.is_string() {
        return std::ptr::null();
    }
    (value.to_bits() & PTR_MASK) as *const StringHeader
}

/// Strip a NaN-boxed closure/function arg to the raw closure-pointer i64 the
/// existing close/on FFI expects. Returns 0 when the arg is undefined / null
/// / non-pointer.
#[inline]
fn closure_arg(value: Option<f64>) -> i64 {
    let Some(v) = value else { return 0 };
    let bits = v.to_bits();
    let tag = bits >> 48;
    if tag != 0x7FFD {
        return 0;
    }
    (bits & PTR_MASK) as i64
}
