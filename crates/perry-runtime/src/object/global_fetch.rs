//! `globalThis.fetch` callable thunk.
//!
//! Split out of `global_this.rs` so the singleton installer stays under the
//! repository's 2,000-line lint gate.

use super::*;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicPtr, Ordering};

#[cfg(not(feature = "external-fetch-symbols"))]
const FETCH_REASON: &str =
    "fetch symbol from perry-stdlib not linked into this binary (runtime-only build)";

type FetchWithOptionsFn = unsafe extern "C" fn(
    *const crate::StringHeader,
    *const crate::StringHeader,
    *const crate::StringHeader,
    *const crate::StringHeader,
) -> *mut crate::promise::Promise;

static GLOBAL_FETCH_WITH_OPTIONS: AtomicPtr<()> = AtomicPtr::new(null_mut());

#[no_mangle]
pub extern "C" fn js_register_global_fetch_with_options(f: FetchWithOptionsFn) {
    GLOBAL_FETCH_WITH_OPTIONS.store(f as *mut (), Ordering::Release);
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
