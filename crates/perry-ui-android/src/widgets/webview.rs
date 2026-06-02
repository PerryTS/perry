//! WebView widget — `android.webkit.WebView` (Android) for issue #658
//! Phase 3.
//!
//! v1 ships the load-bearing surface (loadUrl, reload, goBack/goForward,
//! evaluateJavascript) backed by the stock `WebViewClient`. The
//! `onShouldNavigate` / `onLoaded` / `onError` hooks are stored on the
//! widget but not yet delivered — Android `WebViewClient`'s
//! `shouldOverrideUrlLoading` / `onPageFinished` overrides need a
//! custom Java helper class deployed alongside the Perry runtime APK.
//! Stock-WebView v1 unblocks the OAuth flow's main path (open URL,
//! user authenticates, redirect happens — host page detects the
//! callback URL via `evaluateJavascript("window.location.href")`).
//!
//! A future iteration will land a perry-android-helpers JAR with a
//! `PerryWebViewClient` that proxies callbacks to JNI for the same
//! contract as macOS / iOS / Windows.

use crate::jni_bridge;
use jni::objects::{GlobalRef, JValue};
use std::cell::RefCell;
use std::collections::HashMap;

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_nanbox_string(ptr: i64) -> f64;
    fn js_is_truthy(value: f64) -> i32;
}

struct WebViewState {
    on_should_navigate: f64,
    on_loaded: f64,
    on_error: f64,
    allowed_domains: Vec<String>,
}

thread_local! {
    static WEBVIEW_STATES: RefCell<HashMap<i64, WebViewState>> = RefCell::new(HashMap::new());
}

fn str_from_header(ptr: *const u8) -> &'static str {
    crate::app::str_from_header(ptr)
}

fn nanbox_str(s: &str) -> f64 {
    let bytes = s.as_bytes();
    let p = perry_runtime::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    unsafe { js_nanbox_string(p as i64) }
}

/// Create a WebView. Returns the widget handle. `ephemeral_hint` ∈
/// {0.0, 1.0}: when 1.0 (ephemeral), wipes process-wide cookies +
/// WebStorage on init (Android WebViews share storage process-wide,
/// so true per-instance isolation needs a separate process — this is
/// the best-effort substitute).
pub fn create(url_ptr: *const u8, _width: f64, _height: f64, ephemeral_hint: f64) -> i64 {
    let url = str_from_header(url_ptr).to_string();
    if ephemeral_hint > 0.5 {
        // Wipe at init time so the new WebView starts clean.
        // Mirrors set_ephemeral(1) but happens before any nav.
        wipe_process_storage();
    }
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(32);

    let activity = super::get_activity(&mut env);
    let webview = match env.new_object(
        "android/webkit/WebView",
        "(Landroid/content/Context;)V",
        &[JValue::Object(&activity)],
    ) {
        Ok(w) => w,
        Err(_) => {
            unsafe {
                let _ = env.pop_local_frame(&jni::objects::JObject::null());
            }
            return 0;
        }
    };

    // Enable JavaScript so user OAuth pages can run; gated by getSettings().setJavaScriptEnabled(true).
    if let Ok(settings) = env.call_method(
        &webview,
        "getSettings",
        "()Landroid/webkit/WebSettings;",
        &[],
    ) {
        if let Ok(s) = settings.l() {
            let _ = env.call_method(&s, "setJavaScriptEnabled", "(Z)V", &[JValue::Bool(1)]);
            let _ = env.call_method(&s, "setDomStorageEnabled", "(Z)V", &[JValue::Bool(1)]);
        }
    }

    // Construct PerryWebViewClient(widgetHandle) — wires shouldOverrideUrlLoading
    // / onPageFinished / onReceivedError back through PerryBridge to native.
    // The handle isn't allocated until below; we register the WebView first
    // and then attach the client. Use a placeholder of 0 here and rewire
    // once we know the real handle.
    // NOTE: this two-pass shape avoids a chicken-and-egg between handle
    // allocation and client construction — see `attach_perry_client` below.

    if !url.is_empty() {
        if let Ok(jurl) = env.new_string(&url) {
            let _ = env.call_method(
                &webview,
                "loadUrl",
                "(Ljava/lang/String;)V",
                &[JValue::Object(&jurl)],
            );
        }
    }

    let global_ref = match env.new_global_ref(&webview) {
        Ok(g) => g,
        Err(_) => {
            unsafe {
                let _ = env.pop_local_frame(&jni::objects::JObject::null());
            }
            return 0;
        }
    };
    unsafe {
        let _ = env.pop_local_frame(&jni::objects::JObject::null());
    }

    let handle = super::register_widget(global_ref);
    WEBVIEW_STATES.with(|s| {
        s.borrow_mut().insert(
            handle,
            WebViewState {
                on_should_navigate: 0.0,
                on_loaded: 0.0,
                on_error: 0.0,
                allowed_domains: Vec::new(),
            },
        );
    });
    attach_perry_client(handle);
    handle
}

fn attach_perry_client(handle: i64) {
    let view = match super::get_widget(handle) {
        Some(v) => v,
        None => return,
    };
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(8);
    if let Ok(client) = env.new_object(
        "com/perry/app/PerryWebViewClient",
        "(J)V",
        &[JValue::Long(handle)],
    ) {
        let _ = env.call_method(
            &view,
            "setWebViewClient",
            "(Landroid/webkit/WebViewClient;)V",
            &[JValue::Object(&client)],
        );
    }
    unsafe {
        let _ = env.pop_local_frame(&jni::objects::JObject::null());
    }
}

fn call_string_method(handle: i64, method: &str, sig: &str, jstr: &str) {
    let view = match super::get_widget(handle) {
        Some(v) => v,
        None => return,
    };
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(8);
    if let Ok(s) = env.new_string(jstr) {
        let _ = env.call_method(&view, method, sig, &[JValue::Object(&s)]);
    }
    unsafe {
        let _ = env.pop_local_frame(&jni::objects::JObject::null());
    }
}

fn call_void_method(handle: i64, method: &str) {
    let view = match super::get_widget(handle) {
        Some(v) => v,
        None => return,
    };
    let mut env = jni_bridge::get_env();
    let _ = env.call_method(&view, method, "()V", &[]);
}

pub fn load_url(handle: i64, url_ptr: *const u8) {
    let url = str_from_header(url_ptr);
    if url.is_empty() {
        return;
    }
    call_string_method(handle, "loadUrl", "(Ljava/lang/String;)V", url);
}

pub fn reload(handle: i64) {
    call_void_method(handle, "reload");
}

pub fn go_back(handle: i64) {
    call_void_method(handle, "goBack");
}

pub fn go_forward(handle: i64) {
    call_void_method(handle, "goForward");
}

pub fn can_go_back(handle: i64) -> i64 {
    let view = match super::get_widget(handle) {
        Some(v) => v,
        None => return 0,
    };
    let mut env = jni_bridge::get_env();
    match env.call_method(&view, "canGoBack", "()Z", &[]) {
        Ok(v) => match v.z() {
            Ok(b) => {
                if b {
                    1
                } else {
                    0
                }
            }
            Err(_) => 0,
        },
        Err(_) => 0,
    }
}

/// Fire `evaluateJavascript(js, PerryWebViewEvalCallback(callbackKey))`.
/// The Java helper's `ValueCallback<String>.onReceiveValue(value)` calls
/// back into native via `nativeWebViewEvalResult(callbackKey, value)`,
/// where we look up the user's TS closure (stashed in EVAL_CALLBACKS at
/// call time) and invoke it with the result. Android wraps successful
/// returns as JSON-encoded strings; we strip outer quotes for plain
/// string results matching the Windows / WKWebView ergonomics.
pub fn evaluate_js(handle: i64, js_ptr: *const u8, callback: f64) {
    let js = str_from_header(js_ptr);
    let view = match super::get_widget(handle) {
        Some(v) => v,
        None => return,
    };
    let key = next_eval_callback_key();
    EVAL_CALLBACKS.with(|m| {
        m.borrow_mut().insert(key, callback);
    });
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(8);
    if let Ok(jjs) = env.new_string(js) {
        if let Ok(cb) = env.new_object(
            "com/perry/app/PerryWebViewEvalCallback",
            "(J)V",
            &[JValue::Long(key)],
        ) {
            let _ = env.call_method(
                &view,
                "evaluateJavascript",
                "(Ljava/lang/String;Landroid/webkit/ValueCallback;)V",
                &[JValue::Object(&jjs), JValue::Object(&cb)],
            );
        } else {
            // Fallback: stock evaluateJavascript with null callback — the
            // JS still runs, but the user callback never fires. Drop the
            // EVAL_CALLBACKS entry so it doesn't leak.
            EVAL_CALLBACKS.with(|m| {
                m.borrow_mut().remove(&key);
            });
            let null_cb = jni::objects::JObject::null();
            let _ = env.call_method(
                &view,
                "evaluateJavascript",
                "(Ljava/lang/String;Landroid/webkit/ValueCallback;)V",
                &[JValue::Object(&jjs), JValue::Object(&null_cb)],
            );
        }
    }
    unsafe {
        let _ = env.pop_local_frame(&jni::objects::JObject::null());
    }
}

// =============================================================================
// JNI exports — called from PerryWebViewClient / PerryWebViewEvalCallback
// =============================================================================

use std::cell::Cell as StdCell;
use std::sync::atomic::{AtomicI64, Ordering};

thread_local! {
    /// Per-call eval callback registry — populated by `evaluate_js`,
    /// drained by `Java_..._nativeWebViewEvalResult`. Values are the
    /// user's TS closure (NaN-boxed f64).
    static EVAL_CALLBACKS: RefCell<HashMap<i64, f64>> = RefCell::new(HashMap::new());
}

static NEXT_EVAL_KEY: AtomicI64 = AtomicI64::new(1);

fn next_eval_callback_key() -> i64 {
    NEXT_EVAL_KEY.fetch_add(1, Ordering::Relaxed)
}

#[no_mangle]
pub extern "system" fn Java_com_perry_app_PerryBridge_nativeWebViewShouldNavigate(
    mut env: jni::JNIEnv,
    _class: jni::objects::JClass,
    widget_handle: jni::sys::jlong,
    url: jni::objects::JString,
) -> jni::sys::jboolean {
    let url_str: String = match env.get_string(&url) {
        Ok(s) => s.into(),
        Err(_) => return 1, // allow on read failure (open default)
    };

    let (on_should, allowed) = WEBVIEW_STATES.with(|s| {
        s.borrow()
            .get(&(widget_handle as i64))
            .map(|st| (st.on_should_navigate, st.allowed_domains.clone()))
            .unwrap_or((0.0, Vec::new()))
    });

    // Allowlist gate.
    if !allowed.is_empty() {
        let host = host_of_url(&url_str);
        if !host_in_allowlist(&host, &allowed) {
            return 0; // cancel
        }
    }

    if on_should == 0.0 {
        return 1; // allow
    }
    let url_nb = nanbox_str(&url_str);
    let closure_ptr = unsafe { js_nanbox_get_pointer(on_should) } as *const u8;
    if closure_ptr.is_null() {
        return 1;
    }
    let result_cell = StdCell::new(f64::from_bits(0x7FFC_0000_0000_0001));
    let result_cell_ref = &result_cell;
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let r = unsafe { js_closure_call1(closure_ptr, url_nb) };
        result_cell_ref.set(r);
    }));
    let result = result_cell.get();
    let bits = result.to_bits();
    let is_undefined = bits == 0x7FFC_0000_0000_0001;
    let allow = is_undefined || unsafe { js_is_truthy(result) != 0 };
    if allow {
        1
    } else {
        0
    }
}

#[no_mangle]
pub extern "system" fn Java_com_perry_app_PerryBridge_nativeWebViewLoaded(
    mut env: jni::JNIEnv,
    _class: jni::objects::JClass,
    widget_handle: jni::sys::jlong,
    url: jni::objects::JString,
) {
    let url_str: String = env.get_string(&url).map(|s| s.into()).unwrap_or_default();
    let on_loaded = WEBVIEW_STATES.with(|s| {
        s.borrow()
            .get(&(widget_handle as i64))
            .map(|st| st.on_loaded)
            .unwrap_or(0.0)
    });
    if on_loaded == 0.0 {
        return;
    }
    let closure_ptr = unsafe { js_nanbox_get_pointer(on_loaded) } as *const u8;
    if closure_ptr.is_null() {
        return;
    }
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let nb = nanbox_str(&url_str);
        unsafe { js_closure_call1(closure_ptr, nb) };
    }));
}

extern "C" {
    fn js_closure_call2(closure: *const u8, arg1: f64, arg2: f64) -> f64;
}

#[no_mangle]
pub extern "system" fn Java_com_perry_app_PerryBridge_nativeWebViewError(
    mut env: jni::JNIEnv,
    _class: jni::objects::JClass,
    widget_handle: jni::sys::jlong,
    code: jni::sys::jlong,
    message: jni::objects::JString,
) {
    let msg: String = env
        .get_string(&message)
        .map(|s| s.into())
        .unwrap_or_default();
    let on_error = WEBVIEW_STATES.with(|s| {
        s.borrow()
            .get(&(widget_handle as i64))
            .map(|st| st.on_error)
            .unwrap_or(0.0)
    });
    if on_error == 0.0 {
        return;
    }
    let closure_ptr = unsafe { js_nanbox_get_pointer(on_error) } as *const u8;
    if closure_ptr.is_null() {
        return;
    }
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let msg_nb = nanbox_str(&msg);
        unsafe { js_closure_call2(closure_ptr, code as f64, msg_nb) };
    }));
}

#[no_mangle]
pub extern "system" fn Java_com_perry_app_PerryBridge_nativeWebViewEvalResult(
    mut env: jni::JNIEnv,
    _class: jni::objects::JClass,
    callback_key: jni::sys::jlong,
    result: jni::objects::JString,
) {
    let raw: String = env
        .get_string(&result)
        .map(|s| s.into())
        .unwrap_or_default();
    let callback = EVAL_CALLBACKS.with(|m| m.borrow_mut().remove(&(callback_key as i64)));
    let callback = match callback {
        Some(c) if c != 0.0 => c,
        _ => return,
    };
    // Strip outer JSON quotes for plain string returns (matches the
    // Windows / WKWebView ergonomic for `document.cookie`-style reads);
    // `null` becomes empty.
    let s = if raw == "null" {
        String::new()
    } else if raw.starts_with('"') && raw.ends_with('"') && raw.len() >= 2 {
        let inner = &raw[1..raw.len() - 1];
        inner.replace("\\\"", "\"").replace("\\\\", "\\")
    } else {
        raw
    };
    let closure_ptr = unsafe { js_nanbox_get_pointer(callback) } as *const u8;
    if closure_ptr.is_null() {
        return;
    }
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let nb = nanbox_str(&s);
        unsafe { js_closure_call1(closure_ptr, nb) };
    }));
}

fn host_of_url(s: &str) -> String {
    let after_scheme = match s.find("://") {
        Some(i) => &s[i + 3..],
        None => return String::new(),
    };
    let host_end = after_scheme
        .find(|c| c == '/' || c == '?' || c == '#')
        .unwrap_or(after_scheme.len());
    let host_with_port = &after_scheme[..host_end];
    match host_with_port.find(':') {
        Some(i) => host_with_port[..i].to_string(),
        None => host_with_port.to_string(),
    }
}

fn host_in_allowlist(host: &str, allowlist: &[String]) -> bool {
    if allowlist.is_empty() {
        return true;
    }
    allowlist
        .iter()
        .any(|d| host == d || host.ends_with(&format!(".{}", d)))
}

pub fn clear_cookies(_handle: i64) {
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(8);
    // CookieManager.getInstance().removeAllCookies(null) — process-wide.
    if let Ok(mgr_class) = env.find_class("android/webkit/CookieManager") {
        if let Ok(mgr) = env.call_static_method(
            &mgr_class,
            "getInstance",
            "()Landroid/webkit/CookieManager;",
            &[],
        ) {
            if let Ok(mgr_obj) = mgr.l() {
                let null_cb = jni::objects::JObject::null();
                let _ = env.call_method(
                    &mgr_obj,
                    "removeAllCookies",
                    "(Landroid/webkit/ValueCallback;)V",
                    &[JValue::Object(&null_cb)],
                );
            }
        }
    }
    unsafe {
        let _ = env.pop_local_frame(&jni::objects::JObject::null());
    }
}

pub fn set_user_agent(handle: i64, ua_ptr: *const u8) {
    let ua = str_from_header(ua_ptr);
    let view = match super::get_widget(handle) {
        Some(v) => v,
        None => return,
    };
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(8);
    if let Ok(settings) =
        env.call_method(&view, "getSettings", "()Landroid/webkit/WebSettings;", &[])
    {
        if let Ok(s) = settings.l() {
            if let Ok(jua) = env.new_string(ua) {
                let _ = env.call_method(
                    &s,
                    "setUserAgentString",
                    "(Ljava/lang/String;)V",
                    &[JValue::Object(&jua)],
                );
            }
        }
    }
    unsafe {
        let _ = env.pop_local_frame(&jni::objects::JObject::null());
    }
}

pub fn set_allowed_domains(handle: i64, domains_arr_handle: i64) {
    extern "C" {
        fn js_array_get_length(arr: i64) -> i64;
        fn js_array_get_element_f64(arr: i64, index: i64) -> f64;
        fn js_get_string_pointer_unified(value: f64) -> *const u8;
    }
    let mut domains = Vec::new();
    unsafe {
        let len = js_array_get_length(domains_arr_handle);
        for i in 0..len {
            let elem = js_array_get_element_f64(domains_arr_handle, i);
            let str_ptr = js_get_string_pointer_unified(elem);
            if !str_ptr.is_null() {
                domains.push(str_from_header(str_ptr).to_string());
            }
        }
    }
    WEBVIEW_STATES.with(|s| {
        if let Some(st) = s.borrow_mut().get_mut(&handle) {
            st.allowed_domains = domains;
        }
    });
}

pub fn set_ephemeral(_handle: i64, ephemeral: i64) {
    if ephemeral != 0 {
        wipe_process_storage();
    }
}

/// Wipes process-wide cookies + WebStorage. Called both from
/// `set_ephemeral(true)` and from `create()` when `ephemeral_hint` is
/// truthy. Best-effort; errors are swallowed so a missing
/// CookieManager / WebStorage class doesn't take down widget creation.
fn wipe_process_storage() {
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(8);
    if let Ok(mgr_class) = env.find_class("android/webkit/CookieManager") {
        if let Ok(mgr) = env.call_static_method(
            &mgr_class,
            "getInstance",
            "()Landroid/webkit/CookieManager;",
            &[],
        ) {
            if let Ok(mgr_obj) = mgr.l() {
                let null_cb = jni::objects::JObject::null();
                let _ = env.call_method(
                    &mgr_obj,
                    "removeAllCookies",
                    "(Landroid/webkit/ValueCallback;)V",
                    &[JValue::Object(&null_cb)],
                );
            }
        }
    }
    if let Ok(storage_class) = env.find_class("android/webkit/WebStorage") {
        if let Ok(s) = env.call_static_method(
            &storage_class,
            "getInstance",
            "()Landroid/webkit/WebStorage;",
            &[],
        ) {
            if let Ok(storage) = s.l() {
                let _ = env.call_method(&storage, "deleteAllData", "()V", &[]);
            }
        }
    }
    unsafe {
        let _ = env.pop_local_frame(&jni::objects::JObject::null());
    }
}

pub fn set_on_should_navigate(handle: i64, closure: f64) {
    WEBVIEW_STATES.with(|s| {
        if let Some(st) = s.borrow_mut().get_mut(&handle) {
            st.on_should_navigate = closure;
        }
    });
}

pub fn set_on_loaded(handle: i64, closure: f64) {
    WEBVIEW_STATES.with(|s| {
        if let Some(st) = s.borrow_mut().get_mut(&handle) {
            st.on_loaded = closure;
        }
    });
}

pub fn set_on_error(handle: i64, closure: f64) {
    WEBVIEW_STATES.with(|s| {
        if let Some(st) = s.borrow_mut().get_mut(&handle) {
            st.on_error = closure;
        }
    });
}

#[allow(dead_code)]
fn _ref_globalref() {
    let _ = std::marker::PhantomData::<GlobalRef>;
}
