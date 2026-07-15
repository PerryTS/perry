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

use perry_ffi::{alloc_string, get_handle, js_object_alloc_with_shape, JsValue, StringHeader};

use crate::http2_server::Http2SecureServer;
use crate::https_server::HttpsServer;
use crate::request::IncomingMessage;
use crate::response::ServerResponse;
use crate::server::HttpServer;
use crate::types::{read_string_header, POINTER_TAG, PTR_MASK, TAG_NULL, TAG_UNDEFINED};

#[repr(C)]
struct ErrorHeader {
    _opaque: [u8; 0],
}

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
    fn js_node_http_server_remove_all_listeners(
        handle: i64,
        event_name_ptr: *const StringHeader,
    ) -> f64;
    fn js_node_http_server_remove_listener(
        handle: i64,
        event_name_ptr: *const StringHeader,
        callback: i64,
    ) -> f64;
    fn js_node_http_server_set_timeout_method(handle: i64, msecs: f64, callback: i64) -> i64;
    fn js_node_http_server_ref(handle: i64) -> i64;
    fn js_node_http_server_unref(handle: i64) -> i64;
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
    fn js_node_https_server_ref(handle: i64) -> i64;
    fn js_node_https_server_unref(handle: i64) -> i64;
    fn js_node_http2_server_listen(server_handle: i64, args_array: i64) -> i64;
    fn js_node_http2_server_close(server_handle: i64, callback: i64);
    fn js_node_http2_server_address_json(handle: i64) -> *mut StringHeader;
    fn js_node_http2_server_on(
        handle: i64,
        event_name_ptr: *const StringHeader,
        callback: i64,
    ) -> f64;
    /// Runtime-side JSON.parse — converts the JSON-encoded `address()`
    /// payload into the `{ port, address, family }` object Node returns.
    /// Returns the JSValue bits as u64 (NaN-boxed value).
    fn js_json_parse(text_ptr: *const StringHeader) -> u64;
    fn js_class_method_bind(
        instance: f64,
        method_name_ptr: *const u8,
        method_name_len: usize,
    ) -> f64;
    fn js_error_new_with_message(message: *mut StringHeader) -> *mut ErrorHeader;
    fn js_nanbox_pointer(ptr: i64) -> f64;
    fn js_object_set_field_by_name(
        obj: *mut perry_ffi::ObjectHeader,
        key: *const StringHeader,
        value: f64,
    );
    fn js_promise_rejected(reason: f64) -> *mut crate::types::Promise;
    fn js_promise_resolved(value: f64) -> *mut crate::types::Promise;

    fn js_node_http_im_method(handle: i64) -> *mut StringHeader;
    fn js_node_http_im_url(handle: i64) -> *mut StringHeader;
    fn js_node_http_im_http_version(handle: i64) -> *mut StringHeader;
    fn js_node_http_im_headers_json(handle: i64) -> *mut StringHeader;
    fn js_node_http_im_raw_headers_json(handle: i64) -> *mut StringHeader;
    fn js_node_http_im_headers_distinct_json(handle: i64) -> *mut StringHeader;
    fn js_node_http_im_trailers_json(handle: i64) -> *mut StringHeader;
    fn js_node_http_im_raw_trailers_json(handle: i64) -> *mut StringHeader;
    fn js_node_http_im_trailers_distinct_json(handle: i64) -> *mut StringHeader;
    fn js_node_http_im_complete(handle: i64) -> i32;
    fn js_node_http_im_aborted(handle: i64) -> i32;
    fn js_node_http_im_destroyed(handle: i64) -> i32;
    fn js_node_http_im_readable(handle: i64) -> i32;
    fn js_node_http_im_add_header_line(handle: i64, field: f64, value: f64, dest: f64);
    fn js_node_http_im_signal(handle: i64) -> f64;
    fn js_node_http_im_remote_address(handle: i64) -> *mut StringHeader;
    fn js_node_http_im_remote_port(handle: i64) -> f64;
    fn js_node_http_im_raw_body(handle: i64) -> f64;
    fn js_node_http_im_pause(handle: i64);
    fn js_node_http_im_resume(handle: i64);
    fn js_node_http_im_destroy(handle: i64);
    fn js_node_http_im_on(handle: i64, event_name_ptr: *const StringHeader, callback: i64) -> f64;
    fn js_node_http_im_set_encoding(handle: i64, encoding_ptr: *const StringHeader) -> i64;
    fn js_node_http_im_set_timeout(handle: i64, msecs: f64, callback: i64) -> i64;
    fn js_node_http_im_read(handle: i64) -> f64;

    fn js_node_http_res_set_status(handle: i64, code: f64);
    fn js_node_http_res_get_status(handle: i64) -> f64;
    fn js_node_http_res_set_status_message(handle: i64, msg_ptr: *const StringHeader);
    fn js_node_http_res_set_header(handle: i64, name_ptr: *const StringHeader, value: f64);
    fn js_node_http_res_get_header(handle: i64, name_ptr: *const StringHeader) -> f64;
    fn js_node_http_res_remove_header(handle: i64, name_ptr: *const StringHeader);
    fn js_node_http_res_has_header(handle: i64, name_ptr: *const StringHeader) -> i32;
    fn js_node_http_res_get_headers_json(handle: i64) -> *mut StringHeader;
    fn js_node_http_res_get_header_names_json(handle: i64) -> *mut StringHeader;
    fn js_node_http_res_append_header(
        handle: i64,
        name_ptr: *const StringHeader,
        value_ptr: *const StringHeader,
    ) -> i64;
    fn js_node_http_res_set_headers(handle: i64, headers_value: f64) -> i64;
    fn js_node_http_res_get_status_message(handle: i64) -> f64;
    fn js_node_http_res_headers_sent(handle: i64) -> i32;
    fn js_node_http_res_writable_ended(handle: i64) -> i32;
    fn js_node_http_res_writable_finished(handle: i64) -> i32;
    fn js_node_http_res_finished(handle: i64) -> i32;
    fn js_node_http_res_send_date(handle: i64) -> i32;
    fn js_node_http_res_set_send_date(handle: i64, value: f64);
    fn js_node_http_res_strict_content_length(handle: i64) -> i32;
    fn js_node_http_res_set_strict_content_length(handle: i64, value: f64);
    fn js_node_http_res_req_handle(handle: i64) -> i64;
    fn js_node_http_res_write_head(handle: i64, status: f64, arg2: i64, arg3: i64);
    fn js_node_http_res_write(handle: i64, chunk: f64) -> i32;
    fn js_node_http_res_add_trailers(handle: i64, headers_value: f64);
    fn js_node_http_res_end(handle: i64, chunk: f64);
    fn js_node_http_res_flush_headers(handle: i64);
    fn js_node_http_res_cork(handle: i64);
    fn js_node_http_res_uncork(handle: i64);
    fn js_node_http_res_set_timeout(handle: i64, msecs: f64, callback: i64) -> i64;
    fn js_node_http_res_write_early_hints(handle: i64, headers: f64, callback: i64);
    fn js_node_http_res_write_continue(handle: i64);
    fn js_node_http_res_write_processing(handle: i64);
    fn js_node_http_res_on(handle: i64, event_name_ptr: *const StringHeader, callback: i64) -> f64;
    fn js_node_http_res_once(
        handle: i64,
        event_name_ptr: *const StringHeader,
        callback: i64,
    ) -> f64;
}

/// Probe: is `handle` a live `HttpServer`?
///
/// Stdlib uses this together with the method-name vocabulary below to gate
/// the dispatch arm so a handle id reused across another registry doesn't
/// misroute.
#[no_mangle]
pub extern "C" fn js_ext_http_server_is_handle(handle: i64) -> i32 {
    if get_handle::<HttpServer>(handle).is_some()
        || get_handle::<HttpsServer>(handle).is_some()
        || get_handle::<Http2SecureServer>(handle).is_some()
    {
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
    "removeAllListeners",
    "removeListener",
    "off",
    "setTimeout",
    "ref",
    "unref",
    "@@__perry_wk_asyncDispose",
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
        "removeAllListeners" => Some(b"removeAllListeners"),
        "removeListener" => Some(b"removeListener"),
        "off" => Some(b"off"),
        "setTimeout" => Some(b"setTimeout"),
        "ref" => Some(b"ref"),
        "unref" => Some(b"unref"),
        "@@__perry_wk_asyncDispose" => Some(b"@@__perry_wk_asyncDispose"),
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
    let is_h2 = get_handle::<Http2SecureServer>(handle).is_some();
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
            if is_h2 {
                js_node_http2_server_listen(handle, args_array);
            } else if is_https {
                js_node_https_server_listen(handle, args_array);
            } else {
                js_node_http_server_listen(handle, args_array);
            }
            // Node returns the server for chaining (`createServer(...).listen(p).address()`).
            self_ref
        }
        "close" => {
            let cb = closure_arg(args.first().copied());
            if is_h2 {
                js_node_http2_server_close(handle, cb);
            } else if is_https {
                js_node_https_server_close(handle, cb);
            } else {
                js_node_http_server_close(handle, cb);
            }
            self_ref
        }
        "closeAllConnections" => {
            if is_h2 {
                // HTTP/2 close-all is represented by close() in this surface.
                js_node_http2_server_close(handle, 0);
            } else if is_https {
                js_node_https_server_close_all_connections(handle);
            } else {
                js_node_http_server_close_all_connections(handle);
            }
            undef
        }
        "closeIdleConnections" => {
            if is_h2 {
                // No separate idle-connection tracking for HTTP/2 yet.
            } else if is_https {
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
            let s = if is_h2 {
                js_node_http2_server_address_json(handle)
            } else if is_https {
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
            if is_h2 {
                js_node_http2_server_on(handle, event_ptr, cb);
            } else if is_https {
                js_node_https_server_on(handle, event_ptr, cb);
            } else {
                js_node_http_server_on(handle, event_ptr, cb);
            }
            self_ref
        }
        // #4973: `server.removeAllListeners([event])` — http/1 only for now
        // (https/h2 listener registries wrap an HttpServer base; the http
        // entry covers the plain `node:http` surface the upgrade tests use).
        "removeAllListeners" => {
            let event_ptr = args
                .first()
                .map(|&a| string_arg(a))
                .unwrap_or(std::ptr::null());
            if !is_h2 && !is_https {
                js_node_http_server_remove_all_listeners(handle, event_ptr);
            }
            self_ref
        }
        "removeListener" | "off" if args.len() >= 2 => {
            let event_ptr = string_arg(args[0]);
            if event_ptr.is_null() {
                return self_ref;
            }
            let cb = closure_arg(Some(args[1]));
            if !is_h2 && !is_https {
                js_node_http_server_remove_listener(handle, event_ptr, cb);
            }
            self_ref
        }
        "setTimeout" => {
            let msecs = args.first().copied().unwrap_or(0.0);
            let cb = closure_arg(args.get(1).copied());
            if is_h2 {
                // Timeout storage is not surfaced separately on the HTTP/2 handle yet.
            } else if is_https {
                js_node_https_server_set_timeout_method(handle, msecs, cb);
            } else {
                js_node_http_server_set_timeout_method(handle, msecs, cb);
            }
            self_ref
        }
        // #5011 — `server.ref()` / `server.unref()` return `this` (the
        // server) for chaining; `unref()` drops the server out of the
        // event-loop keepalive set. h2's keepalive is tracked separately
        // (`has_active_h2_clients`), so for h2 we just return the receiver.
        "ref" => {
            if is_https {
                js_node_https_server_ref(handle);
            } else if !is_h2 {
                js_node_http_server_ref(handle);
            }
            self_ref
        }
        "unref" => {
            if is_https {
                js_node_https_server_unref(handle);
            } else if !is_h2 {
                js_node_http_server_unref(handle);
            }
            self_ref
        }
        "@@__perry_wk_asyncDispose" => {
            if server_is_listening(handle, is_https, is_h2) {
                if is_h2 {
                    js_node_http2_server_close(handle, 0);
                } else if is_https {
                    js_node_https_server_close(handle, 0);
                } else {
                    js_node_http_server_close(handle, 0);
                }
                promise_resolved_undefined()
            } else {
                promise_rejected_server_not_running()
            }
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
    let is_https = get_handle::<HttpsServer>(handle).is_some();
    let is_h2 = get_handle::<Http2SecureServer>(handle).is_some();
    match property.as_str() {
        "listening" => bool_value(server_is_listening(handle, is_https, is_h2)),
        // #4974: `server[kConnectionsCheckingInterval]` — the
        // `_http_server` introspection key resolves to Node's
        // connections-checking interval timer; tests assert on its
        // `_destroyed` flag after `close()`. Perry has no such timer,
        // so synthesize the minimal Timeout shape from the tracked flag.
        "@@kConnectionsCheckingInterval" => {
            let destroyed = if is_h2 {
                get_handle::<Http2SecureServer>(handle)
                    .map(|s| s.base.connections_checking_interval_destroyed)
            } else if is_https {
                get_handle::<HttpsServer>(handle)
                    .map(|s| s.base.connections_checking_interval_destroyed)
            } else {
                get_handle::<HttpServer>(handle).map(|s| s.connections_checking_interval_destroyed)
            };
            match destroyed {
                Some(d) => {
                    let json = format!("{{\"_destroyed\":{}}}", d);
                    json_string_value(alloc_string(&json).as_raw())
                }
                None => undef,
            }
        }
        "headersTimeout" => {
            server_base_property(handle, is_https, is_h2, |s| s.headers_timeout).unwrap_or(undef)
        }
        "keepAliveTimeout" => {
            server_base_property(handle, is_https, is_h2, |s| s.keep_alive_timeout).unwrap_or(undef)
        }
        "keepAliveTimeoutBuffer" => {
            server_base_property(handle, is_https, is_h2, |s| s.keep_alive_timeout_buffer)
                .unwrap_or(undef)
        }
        "requestTimeout" => {
            server_base_property(handle, is_https, is_h2, |s| s.request_timeout).unwrap_or(undef)
        }
        "timeout" => {
            server_base_property(handle, is_https, is_h2, |s| s.idle_timeout).unwrap_or(undef)
        }
        "maxHeadersCount" => {
            server_base_property(handle, is_https, is_h2, |s| s.max_headers_count).unwrap_or(undef)
        }
        "maxRequestsPerSocket" => {
            server_base_property(handle, is_https, is_h2, |s| s.max_requests_per_socket)
                .unwrap_or(undef)
        }
        _ => undef,
    }
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
        "setTimeout" => {
            js_node_http_im_set_timeout(
                handle,
                number_arg(args.first().copied(), 0.0),
                closure_arg(args.get(1).copied()),
            );
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
        "httpVersionMajor" | "__get_httpVersionMajor" => {
            crate::request::incoming_http_version_part(handle, false)
        }
        "httpVersionMinor" | "__get_httpVersionMinor" => {
            crate::request::incoming_http_version_part(handle, true)
        }
        "__get_complete" => bool_value(js_node_http_im_complete(handle) != 0),
        "__get_aborted" => bool_value(js_node_http_im_aborted(handle) != 0),
        "__get_destroyed" => bool_value(js_node_http_im_destroyed(handle) != 0),
        "__get_headers" | "headers" => json_string_value(js_node_http_im_headers_json(handle)),
        "__get_rawHeaders" | "rawHeaders" => {
            json_string_value(js_node_http_im_raw_headers_json(handle))
        }
        "__get_headersDistinct" | "headersDistinct" => {
            json_string_value(js_node_http_im_headers_distinct_json(handle))
        }
        "__get_trailers" | "trailers" => {
            json_string_value_empty_object(js_node_http_im_trailers_json(handle))
        }
        "__get_rawTrailers" | "rawTrailers" => {
            json_string_value(js_node_http_im_raw_trailers_json(handle))
        }
        "__get_trailersDistinct" | "trailersDistinct" => {
            json_string_value_empty_object(js_node_http_im_trailers_distinct_json(handle))
        }
        "__get_socket" | "socket" | "__get_connection" | "connection" => {
            crate::request::incoming_socket_override(handle).unwrap_or(self_ref)
        }
        "__set_socket" | "__set_connection" if !args.is_empty() => {
            crate::request::incoming_socket_assign(handle, args[0]);
            undef
        }
        "_addHeaderLine" if args.len() >= 3 => {
            js_node_http_im_add_header_line(handle, args[0], args[1], args[2]);
            undef
        }
        "__get_signal" | "signal" => js_node_http_im_signal(handle),
        "__get_remoteAddress" | "remoteAddress" => {
            string_ptr_value(js_node_http_im_remote_address(handle))
        }
        "__get_remotePort" | "remotePort" => js_node_http_im_remote_port(handle),
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

    if server_response_method_bytes(&method).is_some()
        && server_response_method_bytes_for_handle(handle, &method).is_none()
    {
        return undef;
    }

    match method.as_str() {
        "setHeader" if args.len() >= 2 => {
            let name = string_value_arg(args[0]);
            if !name.is_null() {
                // Pass the raw JSValue so array values (Set-Cookie) keep their
                // per-element structure for one-line-per-element wire output.
                js_node_http_res_set_header(handle, name, args[1]);
            }
            self_ref
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
        "getHeaders" => json_string_value(js_node_http_res_get_headers_json(handle)),
        "getHeaderNames" => json_string_value(js_node_http_res_get_header_names_json(handle)),
        "appendHeader" if args.len() >= 2 => {
            let name = string_value_arg(args[0]);
            if !name.is_null() {
                js_node_http_res_append_header(handle, name, string_value_arg(args[1]));
            }
            self_ref
        }
        "setHeaders" if !args.is_empty() => {
            js_node_http_res_set_headers(handle, args[0]);
            self_ref
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
        "write" if !args.is_empty() => {
            // `write(chunk[, encoding][, callback])` — the callback is the
            // last closure-valued arg (#4904).
            let cb = args[1..]
                .iter()
                .rev()
                .map(|a| closure_arg(Some(*a)))
                .find(|c| *c != 0)
                .unwrap_or(0);
            bool_value(crate::response::js_node_http_res_write_with_cb(handle, args[0], cb) != 0)
        }
        "addTrailers" if !args.is_empty() => {
            js_node_http_res_add_trailers(handle, args[0]);
            undef
        }
        "end" => {
            // `end([chunk][, encoding][, callback])` — `end(cb)` passes the
            // callback first (#4904).
            let first = args.first().copied().unwrap_or(undef);
            let first_cb = closure_arg(Some(first));
            let (chunk, cb) = if first_cb != 0 {
                (undef, first_cb)
            } else {
                let cb = args
                    .get(1..)
                    .unwrap_or(&[])
                    .iter()
                    .rev()
                    .map(|a| closure_arg(Some(*a)))
                    .find(|c| *c != 0)
                    .unwrap_or(0);
                (first, cb)
            };
            crate::response::js_node_http_res_end_with_cb(handle, chunk, cb);
            self_ref
        }
        "flushHeaders" => {
            js_node_http_res_flush_headers(handle);
            undef
        }
        "cork" => {
            js_node_http_res_cork(handle);
            undef
        }
        "uncork" => {
            js_node_http_res_uncork(handle);
            undef
        }
        "setTimeout" => {
            js_node_http_res_set_timeout(
                handle,
                number_arg(args.first().copied(), 0.0),
                closure_arg(args.get(1).copied()),
            );
            self_ref
        }
        "writeEarlyHints" => {
            js_node_http_res_write_early_hints(
                handle,
                args.first().copied().unwrap_or(undef),
                closure_arg(args.get(1).copied()),
            );
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
        "destroy" => self_ref,
        "assignSocket" if !args.is_empty() => {
            crate::response::js_node_http_res_assign_socket(handle, args[0]);
            undef
        }
        "detachSocket" => {
            crate::response::js_node_http_res_detach_socket(
                handle,
                args.first().copied().unwrap_or(undef),
            );
            undef
        }
        "pipe" => undef,
        "on" | "addListener" if args.len() >= 2 => {
            let event_ptr = string_arg(args[0]);
            if event_ptr.is_null() {
                return self_ref;
            }
            js_node_http_res_on(handle, event_ptr, closure_arg(Some(args[1])));
            self_ref
        }
        "once" | "prependOnceListener" if args.len() >= 2 => {
            let event_ptr = string_arg(args[0]);
            if event_ptr.is_null() {
                return self_ref;
            }
            js_node_http_res_once(handle, event_ptr, closure_arg(Some(args[1])));
            self_ref
        }
        "setStatus" | "__set_statusCode" if !args.is_empty() => {
            js_node_http_res_set_status(handle, number_arg(Some(args[0]), 200.0));
            undef
        }
        "getStatus" | "__get_statusCode" => js_node_http_res_get_status(handle),
        "__get_statusMessage" | "statusMessage" => js_node_http_res_get_status_message(handle),
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
        "__get_finished" | "finished" => bool_value(js_node_http_res_finished(handle) != 0),
        "__get_sendDate" | "sendDate" => bool_value(js_node_http_res_send_date(handle) != 0),
        "__set_sendDate" if !args.is_empty() => {
            js_node_http_res_set_send_date(handle, args[0]);
            undef
        }
        "__get_strictContentLength" | "strictContentLength" => {
            bool_value(js_node_http_res_strict_content_length(handle) != 0)
        }
        "__set_strictContentLength" if !args.is_empty() => {
            js_node_http_res_set_strict_content_length(handle, args[0]);
            undef
        }
        "__get_req" | "req" => handle_value_or_undefined(js_node_http_res_req_handle(handle)),
        "__get_socket" | "socket" | "__get_connection" | "connection" => {
            response_socket_value(handle)
        }
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
        "httpVersionMajor" => crate::request::incoming_http_version_part(handle, false),
        "httpVersionMinor" => crate::request::incoming_http_version_part(handle, true),
        "headers" => json_string_value(js_node_http_im_headers_json(handle)),
        "rawHeaders" => json_string_value(js_node_http_im_raw_headers_json(handle)),
        "headersDistinct" => json_string_value(js_node_http_im_headers_distinct_json(handle)),
        "trailers" => json_string_value_empty_object(js_node_http_im_trailers_json(handle)),
        "rawTrailers" => json_string_value(js_node_http_im_raw_trailers_json(handle)),
        "trailersDistinct" => {
            json_string_value_empty_object(js_node_http_im_trailers_distinct_json(handle))
        }
        "complete" => bool_value(js_node_http_im_complete(handle) != 0),
        "aborted" => bool_value(js_node_http_im_aborted(handle) != 0),
        "destroyed" => bool_value(js_node_http_im_destroyed(handle) != 0),
        // `req.readable` — true while the request stream can still yield body
        // bytes (not yet complete, destroyed, or aborted). Node exposes this on
        // the Readable; `req.socket` aliases the request handle, so on-finished's
        // `isFinished()` reads `socket.readable` here. Without it `readable` was
        // `undefined`, making `!socket.readable` true → on-finished reported the
        // request finished → express body-parser skipped JSON/urlencoded parsing
        // and `@Body()` resolved to `undefined`.
        "readable" => bool_value(js_node_http_im_readable(handle) != 0),
        "readableEnded" => bool_value(js_node_http_im_complete(handle) != 0),
        // `req.writable` — Node's real `req.socket` is the Duplex TCP socket,
        // whose `writable` is true while the RESPONSE side of the connection is
        // still open. `res.socket` aliases the request handle (see
        // `response_socket_value`), so `on-finished`'s `isFinished(res)` —
        // `Boolean(res.finished || (socket && !socket.writable))` — reads
        // `socket.writable` HERE. Without it `writable` was `undefined`, so
        // `!socket.writable` was true → on-finished reported the response
        // finished the instant a stream was piped → `send` (Next.js static
        // serving via `serve-static`) destroyed the read stream after the first
        // high-water-mark (64 KB) chunk, truncating every static file past that
        // size. Symmetric with the `readable` accessor above (request-body side).
        "writable" => bool_value(
            js_node_http_im_destroyed(handle) == 0 && js_node_http_im_aborted(handle) == 0,
        ),
        "socket" | "connection" => crate::request::incoming_socket_override(handle)
            .unwrap_or_else(|| handle_to_pointer_f64(handle)),
        "signal" => js_node_http_im_signal(handle),
        "remoteAddress" => string_ptr_value(js_node_http_im_remote_address(handle)),
        "remotePort" => js_node_http_im_remote_port(handle),
        "rawBody" => js_node_http_im_raw_body(handle),
        "constructor" => constructor_object("IncomingMessage"),
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

    if let Some(name) = server_response_method_bytes_for_handle(handle, &property) {
        return bind_handle_method(handle, name);
    }

    match property.as_str() {
        "statusCode" => js_node_http_res_get_status(handle),
        "statusMessage" => js_node_http_res_get_status_message(handle),
        "headersSent" => bool_value(js_node_http_res_headers_sent(handle) != 0),
        "writableEnded" => bool_value(js_node_http_res_writable_ended(handle) != 0),
        "writableFinished" => bool_value(js_node_http_res_writable_finished(handle) != 0),
        "finished" => bool_value(js_node_http_res_finished(handle) != 0),
        "writableCorked" => 0.0,
        "writableHighWaterMark" => 65_536.0,
        "writableLength" => get_handle::<ServerResponse>(handle)
            .map(|sr| sr.buffered_body.len() as f64)
            .unwrap_or(0.0),
        "writableObjectMode" => bool_value(false),
        "writableNeedDrain" => bool_value(false),
        "sendDate" => bool_value(js_node_http_res_send_date(handle) != 0),
        "strictContentLength" => bool_value(js_node_http_res_strict_content_length(handle) != 0),
        "req" => handle_value_or_undefined(js_node_http_res_req_handle(handle)),
        "socket" | "connection" => response_socket_value(handle),
        // #4909 — `out.constructor.name` discrimination (corpus
        // outgoing-message tests branch on it).
        "constructor" => constructor_object("ServerResponse"),
        _ => undef,
    }
}

/// `{ name: <class name> }` — stands in for `<handle>.constructor` so
/// `out.constructor.name` reads "ServerResponse"/"IncomingMessage" the way
/// the corpus outgoing-message tests expect (#4909).
fn constructor_object(name: &str) -> f64 {
    let (packed, shape_id) = perry_ffi::build_object_shape(&["name"]);
    let obj =
        unsafe { js_object_alloc_with_shape(shape_id, 1, packed.as_ptr(), packed.len() as u32) };
    if obj.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let value = JsValue::from_string_ptr(alloc_string(name).as_raw());
    unsafe {
        perry_ffi::js_object_set_field(obj, 0, value);
    }
    f64::from_bits(JsValue::from_object_ptr(obj as *mut u8).bits())
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
        "sendDate" => {
            js_node_http_res_set_send_date(handle, value);
            1
        }
        "strictContentLength" => {
            js_node_http_res_set_strict_content_length(handle, value);
            1
        }
        _ => 0,
    }
}

/// Dispatch a property write on a registered server-side `IncomingMessage`
/// (#4904). Returns 1 when the property was claimed.
///
/// # Safety
/// FFI entry; pointers must be valid for their stated lengths.
#[no_mangle]
pub unsafe extern "C" fn js_ext_http_incoming_message_dispatch_property_set(
    handle: i64,
    property_ptr: *const u8,
    property_len: usize,
    value: f64,
) -> i32 {
    let property = method_name(property_ptr, property_len);
    match property.as_str() {
        // Node's `connection` accessor writes `this.socket`; both aliases
        // land on the same slot.
        "socket" | "connection" => {
            if crate::request::incoming_socket_assign(handle, value) {
                1
            } else {
                0
            }
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
fn handle_value_or_undefined(handle: i64) -> f64 {
    if handle == 0 {
        f64::from_bits(TAG_UNDEFINED)
    } else {
        handle_to_pointer_f64(handle)
    }
}

#[inline]
fn response_socket_value(handle: i64) -> f64 {
    // #4904: a standalone response's socket is whatever `assignSocket`
    // installed (undefined reads as Node's pre-assignment `null`).
    if let Some(sr) = get_handle::<ServerResponse>(handle) {
        if sr.standalone {
            let v = sr.standalone_socket;
            return if JsValue::from_bits(v.to_bits()).is_undefined() {
                f64::from_bits(TAG_NULL)
            } else {
                v
            };
        }
    }
    let req_handle = unsafe { js_node_http_res_req_handle(handle) };
    if req_handle == 0 {
        f64::from_bits(TAG_NULL)
    } else {
        handle_to_pointer_f64(req_handle)
    }
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
fn json_string_value_empty_object(ptr: *mut StringHeader) -> f64 {
    if ptr.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    if matches!(read_string_header(ptr), Some(text) if text == "{}") {
        let obj = unsafe { js_object_alloc_with_shape(0, 0, std::ptr::null(), 0) };
        return f64::from_bits(JsValue::from_object_ptr(obj).bits());
    }
    unsafe { f64::from_bits(js_json_parse(ptr)) }
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
        "setTimeout" => Some(b"setTimeout"),
        "pause" => Some(b"pause"),
        "resume" => Some(b"resume"),
        "destroy" => Some(b"destroy"),
        "read" => Some(b"read"),
        // #4904: internal-by-convention header-merge API, exercised
        // directly by Node's own tests on standalone IncomingMessages.
        "_addHeaderLine" => Some(b"_addHeaderLine"),
        _ => None,
    }
}

fn server_response_method_bytes(name: &str) -> Option<&'static [u8]> {
    match name {
        "setHeader" => Some(b"setHeader"),
        "getHeader" => Some(b"getHeader"),
        "removeHeader" => Some(b"removeHeader"),
        "hasHeader" => Some(b"hasHeader"),
        "getHeaders" => Some(b"getHeaders"),
        "getHeaderNames" => Some(b"getHeaderNames"),
        "appendHeader" => Some(b"appendHeader"),
        "setHeaders" => Some(b"setHeaders"),
        "writeHead" => Some(b"writeHead"),
        "write" => Some(b"write"),
        "addTrailers" => Some(b"addTrailers"),
        "end" => Some(b"end"),
        // #4904: standalone-response wiring.
        "assignSocket" => Some(b"assignSocket"),
        "detachSocket" => Some(b"detachSocket"),
        "flushHeaders" => Some(b"flushHeaders"),
        "cork" => Some(b"cork"),
        "uncork" => Some(b"uncork"),
        "destroy" => Some(b"destroy"),
        "setTimeout" => Some(b"setTimeout"),
        "writeEarlyHints" => Some(b"writeEarlyHints"),
        "writeContinue" => Some(b"writeContinue"),
        "writeProcessing" => Some(b"writeProcessing"),
        "pipe" => Some(b"pipe"),
        "on" => Some(b"on"),
        "addListener" => Some(b"addListener"),
        "once" => Some(b"once"),
        "prependOnceListener" => Some(b"prependOnceListener"),
        _ => None,
    }
}

fn server_response_method_bytes_for_handle(handle: i64, name: &str) -> Option<&'static [u8]> {
    if get_handle::<ServerResponse>(handle)
        .map(|sr| sr.outgoing_message_only)
        .unwrap_or(false)
        && matches!(
            name,
            "writeHead" | "writeEarlyHints" | "writeContinue" | "writeProcessing"
        )
    {
        return None;
    }
    server_response_method_bytes(name)
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
    // #4909 — a Buffer chunk is POINTER_TAG too; `end(buf, cb)` used to
    // treat the buffer as the `end(cb)` callback form, drop the chunk, and
    // then call the buffer ("TypeError: value is not a function").
    if unsafe { crate::types::js_value_is_closure(bits as i64) } == 0 {
        return 0;
    }
    (bits & PTR_MASK) as i64
}

fn server_is_listening(handle: i64, is_https: bool, is_h2: bool) -> bool {
    if is_h2 {
        get_handle::<Http2SecureServer>(handle)
            .map(|server| server.base.listening)
            .unwrap_or(false)
    } else if is_https {
        get_handle::<HttpsServer>(handle)
            .map(|server| server.base.listening)
            .unwrap_or(false)
    } else {
        get_handle::<HttpServer>(handle)
            .map(|server| server.listening)
            .unwrap_or(false)
    }
}

fn server_base_property<F>(handle: i64, is_https: bool, is_h2: bool, f: F) -> Option<f64>
where
    F: Fn(&HttpServer) -> f64,
{
    if is_h2 {
        get_handle::<Http2SecureServer>(handle).map(|server| f(&server.base))
    } else if is_https {
        get_handle::<HttpsServer>(handle).map(|server| f(&server.base))
    } else {
        get_handle::<HttpServer>(handle).map(|server| f(server))
    }
}

fn promise_resolved_undefined() -> f64 {
    unsafe { js_nanbox_pointer(js_promise_resolved(f64::from_bits(TAG_UNDEFINED)) as i64) }
}

fn promise_rejected_server_not_running() -> f64 {
    unsafe {
        let message = alloc_string("Server is not running.");
        let err = js_error_new_with_message(message.as_raw());
        let code_key = alloc_string("code");
        let code_value = alloc_string("ERR_SERVER_NOT_RUNNING");
        js_object_set_field_by_name(
            err as *mut perry_ffi::ObjectHeader,
            code_key.as_raw(),
            f64::from_bits(JsValue::from_string_ptr(code_value.as_raw()).bits()),
        );
        let reason = js_nanbox_pointer(err as i64);
        js_nanbox_pointer(js_promise_rejected(reason) as i64)
    }
}
