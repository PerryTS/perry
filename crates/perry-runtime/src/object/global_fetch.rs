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

type FetchBlobNewFn = unsafe extern "C" fn(f64, f64) -> f64;
type FetchFileNewFn = unsafe extern "C" fn(f64, f64, f64, f64) -> f64;
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
static GLOBAL_FETCH_FILE_NEW: AtomicPtr<()> = AtomicPtr::new(null_mut());
static GLOBAL_FETCH_HEADERS_NEW: AtomicPtr<()> = AtomicPtr::new(null_mut());
static GLOBAL_FETCH_HEADERS_INIT_FROM_VALUE: AtomicPtr<()> = AtomicPtr::new(null_mut());
static GLOBAL_FETCH_REQUEST_NEW: AtomicPtr<()> = AtomicPtr::new(null_mut());
static GLOBAL_FETCH_RESPONSE_NEW: AtomicPtr<()> = AtomicPtr::new(null_mut());
static GLOBAL_FETCH_RESPONSE_STATIC_JSON: AtomicPtr<()> = AtomicPtr::new(null_mut());
static GLOBAL_FETCH_RESPONSE_STATIC_REDIRECT: AtomicPtr<()> = AtomicPtr::new(null_mut());
static GLOBAL_FETCH_RESPONSE_STATIC_ERROR: AtomicPtr<()> = AtomicPtr::new(null_mut());
static GLOBAL_FETCH_BODY_INIT_PTR: AtomicPtr<()> = AtomicPtr::new(null_mut());
/// #4965: perry-stdlib's `Headers` → `[name, value]` entries-JSON producer,
/// used by `res.setHeaders(headers)`. Registered separately from the fetch
/// constructors because the http-server crate (not the fetch crate) is the
/// consumer; routing through the always-linked runtime keeps http-server free
/// of a direct perry-stdlib symbol dependency (which would link-break a
/// stdlib-less build — the #5112 regression class).
static GLOBAL_HEADERS_ENTRIES_JSON: AtomicPtr<()> = AtomicPtr::new(null_mut());
/// perry-stdlib's `Headers` → flat `{name: value}` object-JSON producer, used by
/// the `fetch(url, { headers })` request path. A `Headers` instance is a
/// fetch-band registry *handle*, not a heap pointer, so feeding it straight to
/// `js_json_stringify` faults on the `gc_obj_type` back-read (the `claude -p`
/// SIGSEGV). Routing handle-band `headers` values here reads the registry
/// instead of dereferencing the id. Registered separately from the constructors
/// so a stdlib-less runtime build stays link-clean (the #5112 regression class).
static GLOBAL_HEADERS_OBJECT_JSON: AtomicPtr<()> = AtomicPtr::new(null_mut());

type HeadersEntriesJsonFn = extern "C" fn(f64) -> *mut crate::StringHeader;

/// Register the stdlib body-init coercion (`js_response_body_init_ptr`), which
/// drains a `ReadableStream` body to a `*const StringHeader` (and falls back to
/// the ordinary string coercion for non-stream values). Used by the
/// `Request`/`Response` thunks so a streamed request body (`@hono/node-server`
/// wraps the incoming body as a `ReadableStream`) resolves to its bytes instead
/// of a stringified stream handle id.
#[no_mangle]
pub extern "C" fn js_register_global_fetch_body_init_ptr(f: extern "C" fn(f64) -> i64) {
    GLOBAL_FETCH_BODY_INIT_PTR.store(f as *mut (), Ordering::Release);
}

/// Coerce a fetch body-init value to a `*const StringHeader`, draining a
/// `ReadableStream` body via the registered stdlib helper when present. Falls
/// back to the plain unified string coercion (unchanged behavior for string
/// bodies) when no helper is registered.
pub(crate) fn call_global_body_init_ptr(value: f64) -> *const crate::StringHeader {
    let f = GLOBAL_FETCH_BODY_INIT_PTR.load(Ordering::Acquire);
    if !f.is_null() {
        let func: extern "C" fn(f64) -> i64 = unsafe { std::mem::transmute(f) };
        return func(value) as *const crate::StringHeader;
    }
    crate::value::js_get_string_pointer_unified(value) as *const crate::StringHeader
}

#[no_mangle]
pub extern "C" fn js_register_global_fetch_with_options(f: FetchWithOptionsFn) {
    GLOBAL_FETCH_WITH_OPTIONS.store(f as *mut (), Ordering::Release);
}

#[no_mangle]
pub extern "C" fn js_register_global_fetch_constructors(
    blob_new: FetchBlobNewFn,
    file_new: FetchFileNewFn,
    headers_new: FetchHeadersNewFn,
    headers_init_from_value: FetchHeadersInitFromValueFn,
    request_new: FetchRequestNewFn,
    response_new: FetchResponseNewFn,
    response_static_json: FetchResponseStaticJsonFn,
    response_static_redirect: FetchResponseStaticRedirectFn,
    response_static_error: FetchResponseStaticErrorFn,
) {
    GLOBAL_FETCH_BLOB_NEW.store(blob_new as *mut (), Ordering::Release);
    GLOBAL_FETCH_FILE_NEW.store(file_new as *mut (), Ordering::Release);
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

/// Codegen entry point for the `fetch(url, { headers })` request path: stringify
/// the already-evaluated `headers` value into the flat `{name:value}` JSON that
/// `js_fetch_with_options` parses, treating a `Headers` handle safely (no
/// dereference of a fetch-band id). Mirrors the runtime thunk's
/// `headers_init_json_ptr`; returned as an i64 `*const StringHeader` so the
/// codegen call site can pass it straight into `js_fetch_with_options`.
#[no_mangle]
pub extern "C" fn js_fetch_headers_to_json(headers: f64) -> i64 {
    headers_init_json_ptr(headers) as i64
}

fn fetch_headers_json_ptr(init: f64) -> *const crate::StringHeader {
    let headers = fetch_option(init, b"headers");
    // `init.headers` may be a `Headers` instance (a fetch-band handle), a plain
    // object, or null/undefined — `headers_init_json_ptr` normalizes all three
    // (Headers handle read from its registry, null/undefined → `{}`).
    headers_init_json_ptr(headers)
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

#[cfg(not(feature = "external-fetch-symbols"))]
fn warn_unregistered_fetch_symbol(name: &'static str) -> f64 {
    crate::stub_diag::perry_stub_warn(name, FETCH_REASON, None);
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

// When `external-fetch-symbols` is set, perry-stdlib's `web-fetch`
// implementations are statically linked into this binary — the same invariant
// `call_fetch_with_options` above relies on (and the same one perry's codegen
// fast path uses when it emits a direct `js_headers_new` call). In that build
// the fetch constructor wrappers below call those symbols directly instead of
// routing through the `GLOBAL_FETCH_*` registry. This keeps the registry-backed
// construction path (reflective `new Headers()` / `new Request()` via the class
// registry) consistent with the direct codegen path, and avoids a startup
// hazard where the registry pointer is observed null in the auto-optimized,
// bundled-runtime build even though `js_stdlib_init_dispatch` already stored it.
// The `not(external-fetch-symbols)` variants keep today's registry-load +
// no-op-stub-warn behavior for runtime-only builds (where these externs are not
// linked and a runtime stub provides the symbol instead).
#[cfg(feature = "external-fetch-symbols")]
unsafe extern "C" {
    fn js_blob_new(parts: f64, type_value: f64) -> f64;
    fn js_file_new(parts: f64, name: f64, type_value: f64, last_modified: f64) -> f64;
    fn js_headers_new() -> f64;
    fn js_headers_init_from_value(handle: f64, init: f64) -> f64;
    fn js_request_new(
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
    ) -> f64;
    fn js_response_new(
        body_ptr: *const crate::StringHeader,
        status: f64,
        status_text_ptr: *const crate::StringHeader,
        headers_handle: f64,
    ) -> f64;
    fn js_response_static_json(
        value: f64,
        init_status: f64,
        init_status_text_ptr: *const crate::StringHeader,
        headers_handle: f64,
    ) -> f64;
    fn js_response_static_redirect(url_ptr: *const crate::StringHeader, status: f64) -> f64;
    fn js_response_static_error() -> f64;
}

#[cfg(feature = "external-fetch-symbols")]
pub(super) fn call_global_blob_new(parts: f64, type_value: f64) -> f64 {
    unsafe { js_blob_new(parts, type_value) }
}

#[cfg(not(feature = "external-fetch-symbols"))]
pub(super) fn call_global_blob_new(parts: f64, type_value: f64) -> f64 {
    let f = GLOBAL_FETCH_BLOB_NEW.load(Ordering::Acquire);
    if !f.is_null() {
        let func: FetchBlobNewFn = unsafe { std::mem::transmute(f) };
        return unsafe { func(parts, type_value) };
    }
    warn_unregistered_fetch_symbol("js_blob_new")
}

#[cfg(feature = "external-fetch-symbols")]
pub(super) fn call_global_file_new(
    parts: f64,
    name: f64,
    type_value: f64,
    last_modified: f64,
) -> f64 {
    unsafe { js_file_new(parts, name, type_value, last_modified) }
}

#[cfg(not(feature = "external-fetch-symbols"))]
pub(super) fn call_global_file_new(
    parts: f64,
    name: f64,
    type_value: f64,
    last_modified: f64,
) -> f64 {
    let f = GLOBAL_FETCH_FILE_NEW.load(Ordering::Acquire);
    if !f.is_null() {
        let func: FetchFileNewFn = unsafe { std::mem::transmute(f) };
        return unsafe { func(parts, name, type_value, last_modified) };
    }
    warn_unregistered_fetch_symbol("js_file_new")
}

#[cfg(feature = "external-fetch-symbols")]
pub(super) fn call_global_headers_new() -> f64 {
    unsafe { js_headers_new() }
}

#[cfg(not(feature = "external-fetch-symbols"))]
pub(super) fn call_global_headers_new() -> f64 {
    let f = GLOBAL_FETCH_HEADERS_NEW.load(Ordering::Acquire);
    if !f.is_null() {
        let func: FetchHeadersNewFn = unsafe { std::mem::transmute(f) };
        return func();
    }
    warn_unregistered_fetch_symbol("js_headers_new")
}

#[cfg(feature = "external-fetch-symbols")]
pub(super) fn call_global_headers_init_from_value(handle: f64, init: f64) -> f64 {
    unsafe { js_headers_init_from_value(handle, init) }
}

#[cfg(not(feature = "external-fetch-symbols"))]
pub(super) fn call_global_headers_init_from_value(handle: f64, init: f64) -> f64 {
    let f = GLOBAL_FETCH_HEADERS_INIT_FROM_VALUE.load(Ordering::Acquire);
    if !f.is_null() {
        let func: FetchHeadersInitFromValueFn = unsafe { std::mem::transmute(f) };
        return unsafe { func(handle, init) };
    }
    warn_unregistered_fetch_symbol("js_headers_init_from_value")
}

/// Register perry-stdlib's `Headers` → entries-JSON producer (#4965). The
/// producer takes a NaN-boxed `Headers` handle and returns a fresh
/// `StringHeader` holding a JSON array of `[name, value]` pairs (value is a
/// string, or an array of strings for multi-valued headers like `Set-Cookie`),
/// or null for an unknown handle.
#[no_mangle]
pub extern "C" fn js_register_global_headers_entries_json(f: HeadersEntriesJsonFn) {
    GLOBAL_HEADERS_ENTRIES_JSON.store(f as *mut (), Ordering::Release);
}

fn call_global_headers_entries_json(value: f64) -> *mut crate::StringHeader {
    let f = GLOBAL_HEADERS_ENTRIES_JSON.load(Ordering::Acquire);
    if f.is_null() {
        return null_mut();
    }
    let func: HeadersEntriesJsonFn = unsafe { std::mem::transmute(f) };
    func(value)
}

/// Register perry-stdlib's `Headers` → flat `{name: value}` object-JSON producer
/// (used by the `fetch(url, { headers: Headers })` request path).
#[no_mangle]
pub extern "C" fn js_register_global_headers_object_json(f: HeadersEntriesJsonFn) {
    GLOBAL_HEADERS_OBJECT_JSON.store(f as *mut (), Ordering::Release);
}

fn call_global_headers_object_json(value: f64) -> *mut crate::StringHeader {
    let f = GLOBAL_HEADERS_OBJECT_JSON.load(Ordering::Acquire);
    if f.is_null() {
        return null_mut();
    }
    let func: HeadersEntriesJsonFn = unsafe { std::mem::transmute(f) };
    func(value)
}

/// JSON-stringify a fetch `init.headers` value into the flat `{name: value}`
/// object that `js_fetch_with_options` parses, WITHOUT ever dereferencing a
/// `Headers` registry handle.
///
/// A `Headers` instance is a fetch-band POINTER_TAG handle (its first id is
/// `0x40000`), not a heap object. The generic `js_json_stringify` walker reaches
/// `gc_obj_type` and back-reads `id - 8` as a `GcHeader`, faulting on unmapped
/// memory — the consistent `claude -p` SIGSEGV during request setup. Classify by
/// address band first: a handle-band `Headers` value is delegated to the
/// registered stdlib producer (which reads its own registry); everything else (a
/// plain `{ … }` object, a `Map`, …) is a real heap value and stringifies
/// safely. Same family as #5559/#5560 (handle-band ids mis-dereferenced as heap
/// pointers).
fn headers_init_json_ptr(headers: f64) -> *const crate::StringHeader {
    // Normalize null/undefined to an empty object so BOTH entry points — the
    // codegen `js_fetch_headers_to_json` and the runtime `fetch_headers_json_ptr`
    // — serialize `headers: null` as `{}` rather than the literal `"null"` that
    // `js_fetch_with_options` cannot parse.
    if matches!(
        headers.to_bits(),
        crate::value::TAG_UNDEFINED | crate::value::TAG_NULL
    ) {
        return crate::string::js_string_from_bytes(b"{}".as_ptr(), 2);
    }
    let jsv = crate::value::JSValue::from_bits(headers.to_bits());
    if jsv.is_pointer() {
        let addr = (headers.to_bits() & 0x0000_FFFF_FFFF_FFFF) as usize;
        if crate::value::addr_class::is_handle_band(addr) {
            let p = call_global_headers_object_json(headers);
            if !p.is_null() {
                return p;
            }
            return crate::string::js_string_from_bytes(b"{}".as_ptr(), 2);
        }
    }
    let json = unsafe { crate::json::js_json_stringify(headers, 0) };
    if json.is_null() {
        crate::string::js_string_from_bytes(b"{}".as_ptr(), 2)
    } else {
        json
    }
}

/// Normalize a `res.setHeaders(x)` argument into a JSON array of
/// `[name, value]` entries. Node accepts only `Headers` and `Map`; this
/// returns null for anything else so the http layer can raise
/// `ERR_INVALID_ARG_TYPE`.
///
/// #4965: the previous http-server path JSON-stringified `x` directly. A
/// `Headers` value is a fetch-band registry *handle* (its first id is
/// `0x40000`), not a heap pointer, so the generic stringify walker
/// dereferenced `id - 8` as a `GcHeader` and segfaulted nondeterministically.
/// Classify by address band BEFORE any dereference: a `Map` is a real heap
/// `MapHeader` (its entries are pair-arrays of real heap values — safe to
/// stringify), and a `Headers` handle is delegated to the registered
/// perry-stdlib producer which reads its own registry. No path ever
/// dereferences a handle id.
#[no_mangle]
pub extern "C" fn js_node_setheaders_entries_json(value: f64) -> *mut crate::StringHeader {
    let bits = value.to_bits();
    if let Some(map) = crate::map::map_ptr_from_receiver_bits(bits) {
        let entries = crate::map::js_map_entries(map);
        let boxed = crate::value::js_nanbox_pointer(entries as i64);
        return unsafe { crate::json::js_json_stringify(f64::from_bits(boxed.to_bits()), 0) };
    }
    let jsv = crate::value::JSValue::from_bits(bits);
    if jsv.is_pointer() {
        let addr = (bits & 0x0000_FFFF_FFFF_FFFF) as usize;
        if crate::value::addr_class::is_handle_band(addr) {
            return call_global_headers_entries_json(value);
        }
    }
    null_mut()
}

#[cfg(feature = "external-fetch-symbols")]
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
    unsafe {
        js_request_new(
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
    }
}

#[cfg(not(feature = "external-fetch-symbols"))]
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

#[cfg(feature = "external-fetch-symbols")]
pub(super) fn call_global_response_new(
    body_ptr: *const crate::StringHeader,
    status: f64,
    status_text_ptr: *const crate::StringHeader,
    headers_handle: f64,
) -> f64 {
    unsafe { js_response_new(body_ptr, status, status_text_ptr, headers_handle) }
}

#[cfg(not(feature = "external-fetch-symbols"))]
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

#[cfg(feature = "external-fetch-symbols")]
pub(super) fn call_global_response_static_json(
    value: f64,
    init_status: f64,
    init_status_text_ptr: *const crate::StringHeader,
    headers_handle: f64,
) -> f64 {
    unsafe { js_response_static_json(value, init_status, init_status_text_ptr, headers_handle) }
}

#[cfg(not(feature = "external-fetch-symbols"))]
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

#[cfg(feature = "external-fetch-symbols")]
pub(super) fn call_global_response_static_redirect(
    url_ptr: *const crate::StringHeader,
    status: f64,
) -> f64 {
    unsafe { js_response_static_redirect(url_ptr, status) }
}

#[cfg(not(feature = "external-fetch-symbols"))]
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

#[cfg(feature = "external-fetch-symbols")]
pub(super) fn call_global_response_static_error() -> f64 {
    unsafe { js_response_static_error() }
}

#[cfg(not(feature = "external-fetch-symbols"))]
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
