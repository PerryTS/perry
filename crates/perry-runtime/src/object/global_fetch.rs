//! `globalThis.fetch` callable thunk.
//!
//! Split out of `global_this.rs` so the singleton installer stays under the
//! repository's 2,000-line lint gate.

use super::*;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicPtr, Ordering};

const FETCH_REASON: &str =
    "fetch symbol from perry-stdlib not linked into this binary (runtime-only build)";

type FetchWithOptionsFn = unsafe extern "C" fn(
    *const crate::StringHeader,
    *const crate::StringHeader,
    *const crate::StringHeader,
    *const crate::StringHeader,
) -> *mut crate::promise::Promise;

type FetchBlobNewFn = unsafe extern "C" fn(f64, f64) -> f64;
type FetchHeadersNewFn = extern "C" fn() -> f64;
type FetchHeadersInitFromValueFn = unsafe extern "C" fn(f64, f64) -> f64;
type FetchRequestNewFn = unsafe extern "C" fn(
    *const crate::StringHeader,
    *const crate::StringHeader,
    *const crate::StringHeader,
    f64,
    *const crate::StringHeader,
    *const crate::StringHeader,
    *const crate::StringHeader,
    *const crate::StringHeader,
    *const crate::StringHeader,
    *const crate::StringHeader,
    *const crate::StringHeader,
    f64,
    *const crate::StringHeader,
    f64,
) -> f64;
type FetchResponseNewFn =
    unsafe extern "C" fn(*const crate::StringHeader, f64, *const crate::StringHeader, f64) -> f64;
type FetchResponseStaticJsonFn =
    unsafe extern "C" fn(f64, f64, *const crate::StringHeader, f64) -> f64;
type FetchResponseStaticRedirectFn = unsafe extern "C" fn(*const crate::StringHeader, f64) -> f64;
type FetchResponseStaticErrorFn = extern "C" fn() -> f64;

static GLOBAL_FETCH_WITH_OPTIONS: AtomicPtr<()> = AtomicPtr::new(null_mut());
static GLOBAL_FETCH_BLOB_NEW: AtomicPtr<()> = AtomicPtr::new(null_mut());
static GLOBAL_FETCH_HEADERS_NEW: AtomicPtr<()> = AtomicPtr::new(null_mut());
static GLOBAL_FETCH_HEADERS_INIT_FROM_VALUE: AtomicPtr<()> = AtomicPtr::new(null_mut());
static GLOBAL_FETCH_REQUEST_NEW: AtomicPtr<()> = AtomicPtr::new(null_mut());
static GLOBAL_FETCH_RESPONSE_NEW: AtomicPtr<()> = AtomicPtr::new(null_mut());
static GLOBAL_FETCH_RESPONSE_STATIC_JSON: AtomicPtr<()> = AtomicPtr::new(null_mut());
static GLOBAL_FETCH_RESPONSE_STATIC_REDIRECT: AtomicPtr<()> = AtomicPtr::new(null_mut());
static GLOBAL_FETCH_RESPONSE_STATIC_ERROR: AtomicPtr<()> = AtomicPtr::new(null_mut());

#[no_mangle]
pub extern "C" fn js_register_global_fetch_with_options(f: FetchWithOptionsFn) {
    GLOBAL_FETCH_WITH_OPTIONS.store(f as *mut (), Ordering::Release);
}

#[no_mangle]
pub extern "C" fn js_register_global_fetch_constructors(
    blob_new: FetchBlobNewFn,
    headers_new: FetchHeadersNewFn,
    headers_init_from_value: FetchHeadersInitFromValueFn,
    request_new: FetchRequestNewFn,
    response_new: FetchResponseNewFn,
    response_static_json: FetchResponseStaticJsonFn,
    response_static_redirect: FetchResponseStaticRedirectFn,
    response_static_error: FetchResponseStaticErrorFn,
) {
    GLOBAL_FETCH_BLOB_NEW.store(blob_new as *mut (), Ordering::Release);
    GLOBAL_FETCH_HEADERS_NEW.store(headers_new as *mut (), Ordering::Release);
    GLOBAL_FETCH_HEADERS_INIT_FROM_VALUE
        .store(headers_init_from_value as *mut (), Ordering::Release);
    GLOBAL_FETCH_REQUEST_NEW.store(request_new as *mut (), Ordering::Release);
    GLOBAL_FETCH_RESPONSE_NEW.store(response_new as *mut (), Ordering::Release);
    GLOBAL_FETCH_RESPONSE_STATIC_JSON.store(response_static_json as *mut (), Ordering::Release);
    GLOBAL_FETCH_RESPONSE_STATIC_REDIRECT
        .store(response_static_redirect as *mut (), Ordering::Release);
    GLOBAL_FETCH_RESPONSE_STATIC_ERROR.store(response_static_error as *mut (), Ordering::Release);
}

fn fetch_option(init: f64, name: &[u8]) -> f64 {
    let raw = crate::value::js_nanbox_get_pointer(init);
    if raw < 0x10000 {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    }
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    crate::object::js_object_get_field_by_name_f64(raw as *const ObjectHeader, key)
}

fn fetch_option_string_ptr(init: f64, name: &[u8]) -> *const crate::StringHeader {
    let value = fetch_option(init, name);
    if matches!(
        value.to_bits(),
        crate::value::TAG_UNDEFINED | crate::value::TAG_NULL
    ) {
        return std::ptr::null();
    }
    crate::value::js_get_string_pointer_unified(value) as *const crate::StringHeader
}

fn fetch_headers_json_ptr(init: f64) -> *const crate::StringHeader {
    let headers = fetch_option(init, b"headers");
    if matches!(
        headers.to_bits(),
        crate::value::TAG_UNDEFINED | crate::value::TAG_NULL
    ) {
        return crate::string::js_string_from_bytes(b"{}".as_ptr(), 2);
    }
    let json = unsafe { crate::json::js_json_stringify(headers, 0) };
    if json.is_null() {
        crate::string::js_string_from_bytes(b"{}".as_ptr(), 2)
    } else {
        json
    }
}

#[cfg(feature = "external-fetch-symbols")]
unsafe fn call_fetch_with_options(
    url_ptr: *const crate::StringHeader,
    method_ptr: *const crate::StringHeader,
    body_ptr: *const crate::StringHeader,
    headers_json_ptr: *const crate::StringHeader,
) -> *mut crate::promise::Promise {
    unsafe extern "C" {
        fn js_fetch_with_options(
            url_ptr: *const crate::StringHeader,
            method_ptr: *const crate::StringHeader,
            body_ptr: *const crate::StringHeader,
            headers_json_ptr: *const crate::StringHeader,
        ) -> *mut crate::promise::Promise;
    }

    unsafe { js_fetch_with_options(url_ptr, method_ptr, body_ptr, headers_json_ptr) }
}

#[cfg(not(feature = "external-fetch-symbols"))]
unsafe fn call_fetch_with_options(
    url_ptr: *const crate::StringHeader,
    method_ptr: *const crate::StringHeader,
    body_ptr: *const crate::StringHeader,
    headers_json_ptr: *const crate::StringHeader,
) -> *mut crate::promise::Promise {
    let f = GLOBAL_FETCH_WITH_OPTIONS.load(Ordering::Acquire);
    if !f.is_null() {
        let func: FetchWithOptionsFn = std::mem::transmute(f);
        return unsafe { func(url_ptr, method_ptr, body_ptr, headers_json_ptr) };
    }
    crate::stub_diag::perry_stub_warn("js_fetch_with_options", FETCH_REASON, None);
    null_mut()
}

fn warn_unregistered_fetch_symbol(name: &'static str) -> f64 {
    crate::stub_diag::perry_stub_warn(name, FETCH_REASON, None);
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

pub(super) fn call_global_blob_new(parts: f64, type_value: f64) -> f64 {
    let f = GLOBAL_FETCH_BLOB_NEW.load(Ordering::Acquire);
    if !f.is_null() {
        let func: FetchBlobNewFn = unsafe { std::mem::transmute(f) };
        return unsafe { func(parts, type_value) };
    }
    warn_unregistered_fetch_symbol("js_blob_new")
}

pub(super) fn call_global_headers_new() -> f64 {
    let f = GLOBAL_FETCH_HEADERS_NEW.load(Ordering::Acquire);
    if !f.is_null() {
        let func: FetchHeadersNewFn = unsafe { std::mem::transmute(f) };
        return func();
    }
    warn_unregistered_fetch_symbol("js_headers_new")
}

pub(super) fn call_global_headers_init_from_value(handle: f64, init: f64) -> f64 {
    let f = GLOBAL_FETCH_HEADERS_INIT_FROM_VALUE.load(Ordering::Acquire);
    if !f.is_null() {
        let func: FetchHeadersInitFromValueFn = unsafe { std::mem::transmute(f) };
        return unsafe { func(handle, init) };
    }
    warn_unregistered_fetch_symbol("js_headers_init_from_value")
}

pub(super) fn call_global_request_new(
    url_ptr: *const crate::StringHeader,
    method_ptr: *const crate::StringHeader,
    body_ptr: *const crate::StringHeader,
    headers_handle: f64,
    referrer_ptr: *const crate::StringHeader,
    referrer_policy_ptr: *const crate::StringHeader,
    mode_ptr: *const crate::StringHeader,
    credentials_ptr: *const crate::StringHeader,
    cache_ptr: *const crate::StringHeader,
    redirect_ptr: *const crate::StringHeader,
    integrity_ptr: *const crate::StringHeader,
    keepalive: f64,
    duplex_ptr: *const crate::StringHeader,
    signal: f64,
) -> f64 {
    let f = GLOBAL_FETCH_REQUEST_NEW.load(Ordering::Acquire);
    if !f.is_null() {
        let func: FetchRequestNewFn = unsafe { std::mem::transmute(f) };
        return unsafe {
            func(
                url_ptr,
                method_ptr,
                body_ptr,
                headers_handle,
                referrer_ptr,
                referrer_policy_ptr,
                mode_ptr,
                credentials_ptr,
                cache_ptr,
                redirect_ptr,
                integrity_ptr,
                keepalive,
                duplex_ptr,
                signal,
            )
        };
    }
    warn_unregistered_fetch_symbol("js_request_new")
}

pub(super) fn call_global_response_new(
    body_ptr: *const crate::StringHeader,
    status: f64,
    status_text_ptr: *const crate::StringHeader,
    headers_handle: f64,
) -> f64 {
    let f = GLOBAL_FETCH_RESPONSE_NEW.load(Ordering::Acquire);
    if !f.is_null() {
        let func: FetchResponseNewFn = unsafe { std::mem::transmute(f) };
        return unsafe { func(body_ptr, status, status_text_ptr, headers_handle) };
    }
    warn_unregistered_fetch_symbol("js_response_new")
}

pub(super) fn call_global_response_static_json(
    value: f64,
    init_status: f64,
    init_status_text_ptr: *const crate::StringHeader,
    headers_handle: f64,
) -> f64 {
    let f = GLOBAL_FETCH_RESPONSE_STATIC_JSON.load(Ordering::Acquire);
    if !f.is_null() {
        let func: FetchResponseStaticJsonFn = unsafe { std::mem::transmute(f) };
        return unsafe { func(value, init_status, init_status_text_ptr, headers_handle) };
    }
    warn_unregistered_fetch_symbol("js_response_static_json")
}

pub(super) fn call_global_response_static_redirect(
    url_ptr: *const crate::StringHeader,
    status: f64,
) -> f64 {
    let f = GLOBAL_FETCH_RESPONSE_STATIC_REDIRECT.load(Ordering::Acquire);
    if !f.is_null() {
        let func: FetchResponseStaticRedirectFn = unsafe { std::mem::transmute(f) };
        return unsafe { func(url_ptr, status) };
    }
    warn_unregistered_fetch_symbol("js_response_static_redirect")
}

pub(super) fn call_global_response_static_error() -> f64 {
    let f = GLOBAL_FETCH_RESPONSE_STATIC_ERROR.load(Ordering::Acquire);
    if !f.is_null() {
        let func: FetchResponseStaticErrorFn = unsafe { std::mem::transmute(f) };
        return func();
    }
    warn_unregistered_fetch_symbol("js_response_static_error")
}

pub(super) extern "C" fn global_this_fetch_thunk(
    _closure: *const crate::closure::ClosureHeader,
    input: f64,
    rest: f64,
) -> f64 {
    let init = super::global_this::global_this_rest_array_values(rest)
        .into_iter()
        .next()
        .unwrap_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED));
    let url_ptr = crate::value::js_get_string_pointer_unified(input) as *const crate::StringHeader;
    let method_ptr = fetch_option_string_ptr(init, b"method");
    let body_ptr = fetch_option_string_ptr(init, b"body");
    let headers_json_ptr = fetch_headers_json_ptr(init);

    let promise =
        unsafe { call_fetch_with_options(url_ptr, method_ptr, body_ptr, headers_json_ptr) };
    if promise.is_null() {
        f64::from_bits(crate::value::TAG_NULL)
    } else {
        crate::value::js_nanbox_pointer(promise as i64)
    }
}
