//! JS handle FFI registration + dispatch helpers.
//!
//! perry-jsruntime calls into these `js_set_*` setters to wire up the
//! function pointers used by the dynamic handle dispatchers in the other
//! `value` sub-modules. All `static` storage lives in `tags.rs` so the
//! generated NaN-box helpers can see them through `super::*`.

use super::*;
use std::sync::atomic::Ordering;

/// Set the JS handle array get function (called by perry-jsruntime)
#[no_mangle]
pub extern "C" fn js_set_handle_array_get(func: JsHandleArrayGetFn) {
    JS_HANDLE_ARRAY_GET.store(func as *mut (), Ordering::SeqCst);
}

/// Set the JS handle array length function (called by perry-jsruntime)
#[no_mangle]
pub extern "C" fn js_set_handle_array_length(func: JsHandleArrayLengthFn) {
    JS_HANDLE_ARRAY_LENGTH.store(func as *mut (), Ordering::SeqCst);
}

/// Set the JS handle object get property function (called by perry-jsruntime)
#[no_mangle]
pub extern "C" fn js_set_handle_object_get_property(func: JsHandleObjectGetPropertyFn) {
    JS_HANDLE_OBJECT_GET_PROPERTY.store(func as *mut (), Ordering::SeqCst);
}

/// Set the JS handle to string conversion function (called by perry-jsruntime)
#[no_mangle]
pub extern "C" fn js_set_handle_to_string(func: JsHandleToStringFn) {
    JS_HANDLE_TO_STRING.store(func as *mut (), Ordering::SeqCst);
}

/// Set the JS handle method call function (called by perry-jsruntime)
#[no_mangle]
pub extern "C" fn js_set_handle_call_method(func: JsHandleCallMethodFn) {
    JS_HANDLE_CALL_METHOD.store(func as *mut (), Ordering::SeqCst);
}

/// Set the node:crypto module-method dispatcher (called by perry-stdlib's
/// `js_stdlib_init_dispatch` at program startup). Lets a captured-then-called
/// crypto method (`const f = crypto.createHash; f(...)`) reach the stdlib
/// crypto impls — this crate can't call them directly since perry-stdlib
/// depends on it. Stays null when stdlib isn't linked. (#1577)
#[no_mangle]
pub extern "C" fn js_set_native_crypto_dispatch(func: JsNativeCryptoDispatchFn) {
    JS_NATIVE_CRYPTO_DISPATCH.store(func as *mut (), Ordering::SeqCst);
}

/// Set the WebCrypto `crypto.subtle` module-method dispatcher. This mirrors
/// `js_set_native_crypto_dispatch`, but avoids top-level crypto name
/// collisions such as `crypto.generateKey` versus `crypto.subtle.generateKey`.
#[no_mangle]
pub extern "C" fn js_set_native_webcrypto_dispatch(func: JsNativeWebCryptoDispatchFn) {
    JS_NATIVE_WEBCRYPTO_DISPATCH.store(func as *mut (), Ordering::SeqCst);
}

/// Set the node:zlib module-method dispatcher. Same contract as
/// `js_set_native_crypto_dispatch` above — registered by perry-stdlib at
/// program start so a bound-then-called `zlib.gzip` reaches the stdlib FFI.
#[no_mangle]
pub extern "C" fn js_set_native_zlib_dispatch(func: JsNativeZlibDispatchFn) {
    JS_NATIVE_ZLIB_DISPATCH.store(func as *mut (), Ordering::SeqCst);
}

/// Set the node:querystring module-method dispatcher. Registered by
/// perry-stdlib at startup so a captured `querystring.unescapeBuffer` reaches
/// the stdlib FFI rather than falling through to `undefined`.
#[no_mangle]
pub extern "C" fn js_set_native_querystring_dispatch(func: JsNativeQuerystringDispatchFn) {
    JS_NATIVE_QUERYSTRING_DISPATCH.store(func as *mut (), Ordering::SeqCst);
}

/// Set the node:sqlite module dispatcher. Registered by perry-stdlib at
/// startup so captured and dynamic-imported sqlite exports reach stdlib.
#[no_mangle]
pub extern "C" fn js_set_native_sqlite_dispatch(func: JsNativeSqliteDispatchFn) {
    JS_NATIVE_SQLITE_DISPATCH.store(func as *mut (), Ordering::SeqCst);
}

#[no_mangle]
pub extern "C" fn js_set_native_domain_dispatch(func: JsNativeDomainDispatchFn) {
    JS_NATIVE_DOMAIN_DISPATCH.store(func as *mut (), Ordering::SeqCst);
}

/// Set the node:tls module-method dispatcher. Registered by perry-stdlib at
/// startup so captured helpers like `tls.checkServerIdentity` and property
/// reads like `tls.rootCertificates` reach the rustls-backed TLS helper module.
#[no_mangle]
pub extern "C" fn js_set_native_tls_dispatch(func: JsNativeTlsDispatchFn) {
    JS_NATIVE_TLS_DISPATCH.store(func as *mut (), Ordering::SeqCst);
}

/// Set the node:http/https/http2 server-factory dispatcher. Registered by
/// perry-stdlib at startup (under `external-http-server-pump`) so a captured /
/// aliased `createServer` reaches the perry-ext-http-server impls, which this
/// crate can't call directly. Stays null when the http ext crate isn't linked. (#2533)
#[no_mangle]
pub extern "C" fn js_set_native_http_dispatch(func: JsNativeHttpDispatchFn) {
    JS_NATIVE_HTTP_DISPATCH.store(func as *mut (), Ordering::SeqCst);
}

/// Set the node:events class-constructor dispatcher. Registered by
/// perry-stdlib (`bundled-events`) or perry-ext-events at startup so dynamic
/// `new` on a bound `events.EventEmitter` export value reaches the real
/// emitter constructor. Stays null when no events impl is linked. (#4995)
#[no_mangle]
pub extern "C" fn js_set_native_events_construct(func: JsNativeEventsConstructFn) {
    JS_NATIVE_EVENTS_CONSTRUCT.store(func as *mut (), Ordering::SeqCst);
}

/// Register the async_hooks dynamic-construct dispatcher. Called by perry-stdlib
/// at startup so `new <bound async_hooks.AsyncLocalStorage>()` (the Next.js
/// `new maybeGlobalAsyncLocalStorage()` shape, where the ctor value came from
/// `globalThis.AsyncLocalStorage = AsyncLocalStorage`) builds a real handle
/// instead of a class_id=0 empty object. Shares the `JsNativeEventsConstructFn`
/// (method_ptr, method_len, args_ptr, args_len) -> f64 signature.
#[no_mangle]
pub extern "C" fn js_set_native_async_hooks_construct(func: JsNativeEventsConstructFn) {
    JS_NATIVE_ASYNC_HOOKS_CONSTRUCT.store(func as *mut (), Ordering::SeqCst);
}

/// Set the native module JS property loader (called by perry-jsruntime)
/// This callback loads a native module via V8 and gets a property from it.
#[no_mangle]
pub extern "C" fn js_set_native_module_js_loader(func: JsNativeModuleJsLoaderFn) {
    JS_NATIVE_MODULE_JS_LOADER.store(func as *mut (), Ordering::SeqCst);
}

/// Set the V8 new-from-handle function (called by perry-jsruntime)
/// This callback calls V8's new_instance for JS handle constructors.
#[no_mangle]
pub extern "C" fn js_set_new_from_handle_v8(func: JsNewFromHandleV8Fn) {
    JS_NEW_FROM_HANDLE_V8.store(func as *mut (), Ordering::SeqCst);
}

/// Set the V8 handle typeof discriminator (called by perry-jsruntime).
/// Used by `js_value_typeof` so `typeof someJsFunction` returns `"function"`
/// instead of `"object"` when the handle wraps a V8 callable. (Issue #258.)
#[no_mangle]
pub extern "C" fn js_set_handle_typeof(func: JsHandleTypeofFn) {
    JS_HANDLE_TYPEOF.store(func as *mut (), Ordering::SeqCst);
}

/// Probe a V8 handle's JS `typeof` discriminator. Returns 1 for `"function"`,
/// 0 for `"object"`, and 0 if the V8 callback hasn't been registered (no V8 →
/// fall through to the default "object" classification). Internal helper for
/// `js_value_typeof`.
#[inline]
pub(crate) fn js_handle_is_function(value: f64) -> bool {
    let ptr = JS_HANDLE_TYPEOF.load(Ordering::Relaxed);
    if ptr.is_null() {
        return false;
    }
    let func: JsHandleTypeofFn = unsafe { std::mem::transmute(ptr) };
    unsafe { func(value) == 1 }
}

/// Get element from a JS handle array. Dispatches through the function pointer
/// set by perry-jsruntime, or returns TAG_UNDEFINED if JS runtime not loaded.
#[no_mangle]
pub extern "C" fn js_handle_array_get(array_handle: f64, index: i32) -> f64 {
    let ptr = JS_HANDLE_ARRAY_GET.load(Ordering::Relaxed);
    if ptr.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let func: JsHandleArrayGetFn = unsafe { std::mem::transmute(ptr) };
    func(array_handle, index)
}

/// Get length of a JS handle array. Dispatches through the function pointer
/// set by perry-jsruntime, or returns 0 if JS runtime not loaded.
#[no_mangle]
pub extern "C" fn js_handle_array_length(array_handle: f64) -> i32 {
    let ptr = JS_HANDLE_ARRAY_LENGTH.load(Ordering::Relaxed);
    if ptr.is_null() {
        return 0;
    }
    let func: JsHandleArrayLengthFn = unsafe { std::mem::transmute(ptr) };
    func(array_handle)
}

/// Try to load a property from a native module via V8 JS runtime.
/// Returns TAG_UNDEFINED if JS runtime is not available or property not found.
pub fn native_module_try_js_property(module_name: &str, property_name: &str) -> f64 {
    let loader_ptr = JS_NATIVE_MODULE_JS_LOADER.load(Ordering::Relaxed);
    if loader_ptr.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    let loader: JsNativeModuleJsLoaderFn = unsafe { std::mem::transmute(loader_ptr) };
    unsafe {
        loader(
            module_name.as_ptr(),
            module_name.len(),
            property_name.as_ptr(),
            property_name.len(),
        )
    }
}

/// Check if a NaN-boxed value is a JS handle
#[inline]
pub fn is_js_handle(value: f64) -> bool {
    let bits = value.to_bits();
    (bits & TAG_MASK) == JS_HANDLE_TAG
}
