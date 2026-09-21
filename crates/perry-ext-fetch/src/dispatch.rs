//! #10310: runtime handle-dispatch extension for `perry-ext-fetch` handles.
//!
//! When `node-fetch`/`undici` pull this crate into a graph, the well-known
//! routing links it alongside `perry-stdlib`, and both crates `#[no_mangle]`
//! the `js_headers_*` / `js_request_*` families. The overlapping symbols
//! resolve to this crate while the ones only stdlib exports — notably
//! `js_headers_method_value` — still resolve to stdlib. One Headers value then
//! lives in THIS crate's registry while dynamic dispatch reads stdlib's.
//!
//! Two things are needed, and they are only useful together:
//!
//! 1. Handles must be NaN-boxed (`POINTER_TAG`) so a property or method access
//!    reaches the runtime's handle tower at all. A bare `f64` id is a JS
//!    *number*, and `(number).delete` is not a function — it never gets there.
//! 2. This crate must register dispatch extensions so the tower answers those
//!    accesses from ITS registries. `perry-ext-net`, `-ws`, `-http` and
//!    `-mysql2` all register; this crate did not, which is the whole bug.
//!
//! Boxing alone is worse than the bug: the throw disappears and every dynamic
//! Headers op goes silently wrong, because the tower routes Headers to
//! stdlib's `dispatch_headers_method`, which reads stdlib's registry while the
//! handle lives here. Silent header loss in an HTTP client beats a loud throw
//! only in the sense that it is harder to find.

use perry_ffi::StringHeader;
use std::sync::Once;

use crate::{
    handle_id, js_headers_append, js_headers_delete, js_headers_entries, js_headers_for_each,
    js_headers_get, js_headers_get_set_cookie, js_headers_has, js_headers_keys, js_headers_set,
    js_headers_values, BLOB_HANDLES, FETCH_RESPONSES, FORM_DATA_HANDLES, HEADERS_HANDLES,
    POINTER_TAG, REQUEST_HANDLES,
};

const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;

unsafe extern "C" {
    fn js_get_string_pointer_unified(value: f64) -> i64;
    fn js_register_handle_method_dispatch_extension(
        f: unsafe extern "C" fn(i64, *const u8, usize, *const f64, usize, *mut f64) -> i32,
    );
    fn js_register_handle_property_dispatch_extension(
        f: unsafe extern "C" fn(i64, *const u8, usize, *mut f64) -> i32,
    );
}

/// NaN-box a registry id the way `perry-stdlib`'s `handle_to_f64` does, so the
/// value is a handle to the runtime rather than a plain number. `handle_id`
/// already decodes both this and the legacy bare-float form, so every existing
/// entry point in this crate keeps working on either.
pub(crate) fn box_handle(id: usize) -> f64 {
    f64::from_bits(POINTER_TAG | ((id as u64) & 0x0000_FFFF_FFFF_FFFF))
}

/// Register the dispatch extensions once. Called from every entry point that
/// can hand a handle to JS, because there is no single init hook this crate is
/// guaranteed to run through.
pub(crate) fn ensure_runtime_dispatch_registered() {
    static REGISTER: Once = Once::new();
    REGISTER.call_once(|| unsafe {
        js_register_handle_method_dispatch_extension(js_ext_fetch_handle_method_dispatch);
        js_register_handle_property_dispatch_extension(js_ext_fetch_handle_property_dispatch);
    });
}

unsafe fn name_of(ptr: *const u8, len: usize) -> &'static str {
    if ptr.is_null() || len == 0 {
        return "";
    }
    std::str::from_utf8(std::slice::from_raw_parts(ptr, len)).unwrap_or("")
}

fn is_headers(id: usize) -> bool {
    HEADERS_HANDLES
        .lock()
        .map(|g| g.contains_key(&id))
        .unwrap_or(false)
}

fn is_request(id: usize) -> bool {
    REQUEST_HANDLES
        .lock()
        .map(|g| g.contains_key(&id))
        .unwrap_or(false)
}

fn is_response(id: usize) -> bool {
    FETCH_RESPONSES
        .lock()
        .map(|g| g.contains_key(&id))
        .unwrap_or(false)
}

// Layers 3-4 of #10310 need a runtime hook this crate cannot reach: the
// runtime exposes `js_register_handle_method_dispatch_extension` and
// `..._property_dispatch_extension`, but `js_register_handle_prototype_dispatch`
// and `..._own_property_names_dispatch` have no `_extension` variant — stdlib
// owns them outright. Until one exists, an ext-owned Headers still reports
// kind 0 from stdlib's `js_fetch_handle_kind`, so `h instanceof Headers` is
// false and an SDK doing `Object.entries(handle)` sees `[]`. The predicates and
// the kind mapping below are the ext side of that fix, kept so the remaining
// work is a runtime hook plus one registration call rather than a rediscovery.
#[allow(dead_code)]
fn is_blob(id: usize) -> bool {
    BLOB_HANDLES
        .lock()
        .map(|g| g.contains_key(&id))
        .unwrap_or(false)
}

#[allow(dead_code)]
fn is_form_data(id: usize) -> bool {
    FORM_DATA_HANDLES
        .lock()
        .map(|g| g.contains_key(&id))
        .unwrap_or(false)
}

/// Mirror of `perry-stdlib`'s `dispatch_headers_method`, reading THIS crate's
/// registry. The string-argument coercion matters: an argument can arrive as a
/// heap string or an SSO/short string, and only the runtime's unified helper
/// turns both into a `StringHeader`.
unsafe fn headers_method(id: usize, method: &str, args: &[f64]) -> Option<f64> {
    if !is_headers(id) {
        return None;
    }
    let h = box_handle(id);
    let str_arg = |i: usize| -> *const StringHeader {
        if i < args.len() {
            js_get_string_pointer_unified(args[i]) as usize as *const StringHeader
        } else {
            std::ptr::null()
        }
    };
    match method {
        // WHATWG `Headers.get` yields null for an absent header, not "".
        // `js_headers_get` signals absence with a null pointer; rendering that
        // as a string would break `headers.get(x) === null`.
        "get" => {
            let p = js_headers_get(h, str_arg(0));
            if p.is_null() {
                Some(f64::from_bits(TAG_NULL))
            } else {
                Some(f64::from_bits(perry_ffi::nanbox_string_bits(p)))
            }
        }
        "set" => Some(js_headers_set(h, str_arg(0), str_arg(1))),
        "append" => Some(js_headers_append(h, str_arg(0), str_arg(1))),
        "has" => Some(js_headers_has(h, str_arg(0))),
        "delete" => Some(js_headers_delete(h, str_arg(0))),
        "getSetCookie" => Some(js_headers_get_set_cookie(h)),
        "forEach" => Some(js_headers_for_each(
            h,
            args.first().copied().unwrap_or(f64::NAN),
        )),
        "keys" => Some(js_headers_keys(h)),
        "values" => Some(js_headers_values(h)),
        "entries" | "Symbol.iterator" | "@@iterator" => Some(js_headers_entries(h)),
        _ => None,
    }
}

/// Property reads on a Request/Response handle reached without a static type
/// (minified JS, `any`-typed receivers). Returns the same values the typed
/// accessors do.
unsafe fn request_property(id: usize, prop: &str) -> Option<f64> {
    if !is_request(id) {
        return None;
    }
    let h = box_handle(id);
    let s = |p: *mut StringHeader| -> Option<f64> {
        if p.is_null() {
            Some(f64::from_bits(TAG_NULL))
        } else {
            Some(f64::from_bits(perry_ffi::nanbox_string_bits(p)))
        }
    };
    match prop {
        "url" => s(crate::js_request_get_url(h)),
        "method" => s(crate::js_request_get_method(h)),
        "headers" => Some(crate::request_fields::js_request_get_headers(h)),
        _ => None,
    }
}

unsafe fn response_property(id: usize, prop: &str) -> Option<f64> {
    if !is_response(id) {
        return None;
    }
    let h = box_handle(id);
    match prop {
        "status" => Some(crate::js_fetch_response_status(h)),
        "ok" => Some(crate::js_fetch_response_ok(h)),
        "redirected" => Some(crate::js_fetch_response_redirected(h)),
        "headers" => Some(crate::js_response_get_headers(h)),
        _ => None,
    }
}

/// # Safety
/// Called by the runtime handle tower with a decoded registry id and a UTF-8
/// method name; `args_ptr` is either null or `args_len` readable `f64`s.
#[no_mangle]
pub unsafe extern "C" fn js_ext_fetch_handle_method_dispatch(
    handle: i64,
    method_name_ptr: *const u8,
    method_name_len: usize,
    args_ptr: *const f64,
    args_len: usize,
    out: *mut f64,
) -> i32 {
    let id = handle_id(f64::from_bits(
        POINTER_TAG | (handle as u64 & 0x0000_FFFF_FFFF_FFFF),
    ));
    let method = name_of(method_name_ptr, method_name_len);
    let args: &[f64] = if args_ptr.is_null() || args_len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(args_ptr, args_len)
    };
    match headers_method(id, method, args) {
        Some(v) => {
            if !out.is_null() {
                *out = v;
            }
            1
        }
        None => 0,
    }
}

/// # Safety
/// See [`js_ext_fetch_handle_method_dispatch`].
#[no_mangle]
pub unsafe extern "C" fn js_ext_fetch_handle_property_dispatch(
    handle: i64,
    prop_name_ptr: *const u8,
    prop_name_len: usize,
    out: *mut f64,
) -> i32 {
    let id = handle_id(f64::from_bits(
        POINTER_TAG | (handle as u64 & 0x0000_FFFF_FFFF_FFFF),
    ));
    let prop = name_of(prop_name_ptr, prop_name_len);
    let value = request_property(id, prop).or_else(|| response_property(id, prop));
    match value {
        Some(v) => {
            if !out.is_null() {
                *out = v;
            }
            1
        }
        None => 0,
    }
}

/// #10310 layer 3: stdlib also exports `js_fetch_handle_kind`, and its version
/// reads stdlib's registries — so an ext-owned handle reported kind 0 and
/// `h instanceof Headers` was false, which made the SDK's `mergeHeaders` take
/// `Object.entries(handle)` (empty) and drop every header silently.
#[allow(dead_code)]
pub(crate) fn ext_handle_kind(id: usize) -> u8 {
    if is_response(id) {
        return 1;
    }
    if is_request(id) {
        return 2;
    }
    if is_headers(id) {
        return 3;
    }
    if is_blob(id) {
        return 4;
    }
    if is_form_data(id) {
        return 6;
    }
    0
}
