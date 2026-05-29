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

use perry_ffi::{get_handle, JsValue, StringHeader};

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
    /// Runtime-side JSON.parse — converts the JSON-encoded `address()`
    /// payload into the `{ port, address, family }` object Node returns.
    /// Returns the JSValue bits as u64 (NaN-boxed value).
    fn js_json_parse(text_ptr: *const StringHeader) -> u64;
}

/// Probe: is `handle` a live `HttpServer`?
///
/// Stdlib uses this together with the method-name vocabulary below to gate
/// the dispatch arm so a handle id reused across another registry doesn't
/// misroute.
#[no_mangle]
pub extern "C" fn js_ext_http_server_is_handle(handle: i64) -> i32 {
    if get_handle::<HttpServer>(handle).is_some() {
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
];

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
            js_node_http_server_listen(handle, args_array);
            // Node returns the server for chaining (`createServer(...).listen(p).address()`).
            self_ref
        }
        "close" => {
            let cb = closure_arg(args.first().copied());
            js_node_http_server_close(handle, cb);
            self_ref
        }
        "closeAllConnections" => {
            js_node_http_server_close_all_connections(handle);
            undef
        }
        "closeIdleConnections" => {
            js_node_http_server_close_idle_connections(handle);
            undef
        }
        "address" => {
            // Node returns `{ port, address, family }` or null. The FFI hands
            // back a JSON-encoded string (`"null"` when not listening); run
            // it through JSON.parse so the value the caller sees matches Node.
            let s = js_node_http_server_address_json(handle);
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
            js_node_http_server_on(handle, event_ptr, cb);
            self_ref
        }
        // `listening` is a property on the JS side; the only known property
        // bound through this dispatcher would be `.listening()` — Node doesn't
        // expose it as a method, so fall through to undef.
        _ => undef,
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
