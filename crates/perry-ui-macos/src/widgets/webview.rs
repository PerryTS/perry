//! WebView widget — `WKWebView` (WebKit) for auth flows / payment redirects /
//! embedded HTML pages on macOS. Tracking issue #658, Phase 1.
//!
//! API surface mirrors the cross-platform contract documented in #658:
//! - `WebView({ url, allowedDomains?, userAgent?, ephemeral?, onShouldNavigate?,
//!    onLoaded?, onError?, width?, height? })` — declarative entry.
//! - Imperative ops: `webviewLoadUrl` / `webviewReload` / `webviewGoBack` /
//!   `webviewGoForward` / `webviewCanGoBack` / `webviewEvaluateJs` /
//!   `webviewClearCookies` / `webviewSetUserAgent` / `webviewSetAllowedDomains`.
//!
//! Architecture: a `PerryWebViewDelegate` (NSObject subclass conforming to
//! WKNavigationDelegate) intercepts the three navigation hooks. The delegate's
//! ivar holds a key into `WEBVIEW_CALLBACKS` — a thread-local registry of
//! per-handle closures + the WKWebView pointer. The sync should-navigate
//! intercept calls the user's TS closure (returns f64 — truthy = allow); the
//! `decisionHandler` block fires `WKNavigationActionPolicyCancel` (0) or
//! `WKNavigationActionPolicyAllow` (1) accordingly.
//!
//! Cookie isolation defaults to ephemeral (`WKWebsiteDataStore.nonPersistent()`)
//! per the design — auth flows reusing a logged-in browser session is usually
//! a footgun. Opt out via `ephemeral: false` when needed.

use crate::ffi::js_get_string_pointer_unified;
use crate::ffi::js_string_from_bytes;
use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2::{define_class, AnyThread, DefinedClass};
use objc2_app_kit::NSView;
use objc2_foundation::{MainThreadMarker, NSObject, NSString};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

extern "C" {
    fn js_closure_call0(closure: *const u8) -> f64;
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_closure_call2(closure: *const u8, arg1: f64, arg2: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_nanbox_string(ptr: i64) -> f64;
    fn js_is_truthy(value: f64) -> i32;
}

/// Per-WebView state. Keyed by delegate address (stable for the widget's
/// lifetime) so the WKNavigationDelegate methods can route notifications back
/// to the right user closures.
struct WebViewState {
    /// Raw pointer to the owning WKWebView — used to bridge from delegate
    /// callbacks back to the widget when several WebViews share a process.
    /// Held weakly via `*const AnyObject`; we never deref past widget destroy
    /// because the registry entry is removed on widget destroy too.
    webview_ptr: *const AnyObject,
    /// User's TS closure (NaN-boxed). 0.0 means "not registered".
    on_should_navigate: f64,
    on_loaded: f64,
    on_error: f64,
    /// Hard navigation allowlist. Empty = no restriction. URLs whose host
    /// matches any entry pass; others are rejected without invoking the user
    /// closure (security: prevents hijacked OAuth pages from redirecting to
    /// arbitrary origins).
    allowed_domains: Vec<String>,
}

thread_local! {
    static WEBVIEW_STATES: RefCell<HashMap<usize, WebViewState>> = RefCell::new(HashMap::new());
    /// Map widget handle → delegate address so imperative methods (loadUrl,
    /// reload, …) can locate the WKWebView through the delegate's ivar. We
    /// don't store the WKWebView Retained directly here because `register_widget`
    /// already retains it; this map is just a shortcut.
    static HANDLE_TO_KEY: RefCell<HashMap<i64, usize>> = RefCell::new(HashMap::new());
}

fn str_from_header(ptr: *const u8) -> &'static str {
    if ptr.is_null() {
        return "";
    }
    unsafe {
        let header = ptr as *const crate::string_header::StringHeader;
        let len = (*header).byte_len as usize;
        let data = ptr.add(std::mem::size_of::<crate::string_header::StringHeader>());
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len))
    }
}

fn nanbox_str(s: &str) -> f64 {
    let bytes = s.as_bytes();
    unsafe {
        let p = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
        js_nanbox_string(p as i64)
    }
}

// AVFoundation represents media types as four-character codes:
// `vide` = AVMediaTypeVideo, `soun` = AVMediaTypeAudio.
fn open_system_settings(kind: &str) {
    unsafe {
        let pane = if kind == "vide" {
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Camera"
        } else {
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone"
        };
        let ns_url_str = NSString::from_str(pane);
        let Some(url_cls) = AnyClass::get(c"NSURL") else {
            return;
        };
        let url: *mut AnyObject = msg_send![url_cls, URLWithString: &*ns_url_str];
        if url.is_null() {
            return;
        }
        let Some(workspace_cls) = AnyClass::get(c"NSWorkspace") else {
            return;
        };
        let workspace: *mut AnyObject = msg_send![workspace_cls, sharedWorkspace];
        if !workspace.is_null() {
            let _: bool = msg_send![workspace, openURL: url];
        }
    }
}

fn show_media_permission_denied_alert(kind: &str, status: i64) {
    let (title, noun, settings_name) = if kind == "vide" {
        (
            "Camera access is disabled",
            "camera",
            "Privacy & Security → Camera",
        )
    } else {
        (
            "Microphone access is disabled",
            "microphone",
            "Privacy & Security → Microphone",
        )
    };
    let reason = if status == 1 { "restricted" } else { "denied" };
    unsafe {
        // Bring the app to the front before showing the modal. Otherwise the
        // Settings helper can be created behind the WebView window.
        if let Some(app_cls) = AnyClass::get(c"NSApplication") {
            let app: *mut AnyObject = msg_send![app_cls, sharedApplication];
            if !app.is_null() {
                let _: () = msg_send![app, activateIgnoringOtherApps: true];
            }
        }
        let Some(alert_cls) = AnyClass::get(c"NSAlert") else {
            return;
        };
        let alert: Retained<AnyObject> = msg_send![alert_cls, new];
        let ns_title = NSString::from_str(title);
        let _: () = msg_send![&*alert, setMessageText: &*ns_title];
        let message = format!(
            "This app cannot use the {noun} because macOS reports it as {reason}.\n\nOpen System Settings and enable this app in {settings_name}, then restart it."
        );
        let ns_message = NSString::from_str(&message);
        let _: () = msg_send![&*alert, setInformativeText: &*ns_message];
        let ns_open = NSString::from_str("Open System Settings");
        let ns_cancel = NSString::from_str("Cancel");
        let _: Retained<AnyObject> = msg_send![&*alert, addButtonWithTitle: &*ns_open];
        let _: Retained<AnyObject> = msg_send![&*alert, addButtonWithTitle: &*ns_cancel];
        let response: isize = msg_send![&*alert, runModal];
        // NSAlertFirstButtonReturn = 1000.
        if response == 1000 {
            open_system_settings(kind);
        }
    }
}

fn request_av_capture_access_once() {
    unsafe {
        let Some(device_cls) = AnyClass::get(c"AVCaptureDevice") else {
            return;
        };
        for media in ["vide", "soun"] {
            let media_type = NSString::from_str(media);
            let status: i64 = msg_send![device_cls, authorizationStatusForMediaType: &*media_type];
            // 0 = notDetermined, 1 = restricted, 2 = denied, 3 = authorized.
            if status == 0 {
                let media_label = media.to_string();
                let permission_block =
                    block2::RcBlock::new(move |granted: objc2::runtime::Bool| {
                        eprintln!(
                            "[webview] AV capture permission {}: {}",
                            media_label,
                            if granted.as_bool() {
                                "granted"
                            } else {
                                "denied"
                            }
                        );
                    });
                let _: () = msg_send![device_cls, requestAccessForMediaType: &*media_type completionHandler: &*permission_block];
            } else {
                // Do not show the Settings alert here: WebView widgets are
                // constructed while building the App body, before the window is
                // guaranteed to be visible. The actual getUserMedia permission
                // callback below shows the actionable alert at click time.
                eprintln!(
                    "[webview] AV capture permission {} status: {}",
                    media, status
                );
            }
        }
    }
}

fn av_capture_status(media: &str) -> i64 {
    unsafe {
        let Some(device_cls) = AnyClass::get(c"AVCaptureDevice") else {
            return 3;
        };
        let media_type = NSString::from_str(media);
        msg_send![device_cls, authorizationStatusForMediaType: &*media_type]
    }
}

fn show_denied_alerts_for_media_request() {
    // WKMediaCaptureType can be camera, microphone, or camera+microphone. The
    // raw values are platform-owned, so checking both AVFoundation statuses is
    // safer and keeps the UX correct for combined getUserMedia requests.
    for media in ["vide", "soun"] {
        let status = av_capture_status(media);
        if status == 1 || status == 2 {
            eprintln!(
                "[webview] showing Settings alert for AV capture permission {} status: {}",
                media, status
            );
            show_media_permission_denied_alert(media, status);
        }
    }
}

/// Return the URL string from a WKNavigationAction.
unsafe fn url_from_action(action: *mut AnyObject) -> String {
    if action.is_null() {
        return String::new();
    }
    let request: *mut AnyObject = msg_send![action, request];
    if request.is_null() {
        return String::new();
    }
    let url: *mut AnyObject = msg_send![request, URL];
    if url.is_null() {
        return String::new();
    }
    let abs: *mut AnyObject = msg_send![url, absoluteString];
    if abs.is_null() {
        return String::new();
    }
    let ns: &NSString = &*(abs as *const NSString);
    ns.to_string()
}

/// Return the URL host from a WKWebView (its current URL).
unsafe fn host_of_url_string(s: &str) -> String {
    // Tiny URL host extractor (matches the macOS NSURL scope: scheme://host[:port]/...).
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
    allowlist.iter().any(|d| {
        // Exact match OR subdomain match (host == d OR host ends with ".{d}").
        host == d || host.ends_with(&format!(".{}", d))
    })
}

// =============================================================================
// WKNavigationDelegate
// =============================================================================

pub struct PerryWebViewDelegateIvars {
    callback_key: Cell<usize>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryWebViewDelegate"]
    #[ivars = PerryWebViewDelegateIvars]
    pub struct PerryWebViewDelegate;

    impl PerryWebViewDelegate {
        /// `webView:decidePolicyForNavigationAction:decisionHandler:`
        ///
        /// Sync intercept — the decisionHandler block must be invoked exactly
        /// once before this method returns control. We synchronously call the
        /// user's `onShouldNavigate` TS closure and translate its truthy
        /// return into `WKNavigationActionPolicyAllow` (1) / `Cancel` (0).
        #[unsafe(method(webView:decidePolicyForNavigationAction:decisionHandler:))]
        fn decide_policy(
            &self,
            _webview: *mut AnyObject,
            action: *mut AnyObject,
            decision_handler: *const block2::Block<dyn Fn(i64)>,
        ) {
            const POLICY_CANCEL: i64 = 0;
            const POLICY_ALLOW: i64 = 1;

            let key = self.ivars().callback_key.get();

            // This method is a non-unwinding ObjC callback: a Rust panic that
            // escapes it aborts the whole process (#3129 — WebView aborted on
            // the first navigation). So the entire policy computation — state
            // lookup, allowlist gate, and the user `onShouldNavigate` closure
            // round-trip — runs inside a panic guard, defaulting to ALLOW so
            // navigation proceeds rather than killing the app.
            let policy_cell = std::cell::Cell::new(POLICY_ALLOW);
            let policy_cell_ref = &policy_cell;
            crate::catch_callback_panic(
                "webview decidePolicyForNavigationAction",
                std::panic::AssertUnwindSafe(|| {
                    let url_str = unsafe { url_from_action(action) };

                    let (on_should_navigate, allowed) = WEBVIEW_STATES.with(|s| {
                        let states = s.borrow();
                        if let Some(st) = states.get(&key) {
                            (st.on_should_navigate, st.allowed_domains.clone())
                        } else {
                            (0.0, Vec::new())
                        }
                    });

                    // 1. Allowlist gate (no user-closure round-trip — security).
                    if !allowed.is_empty() {
                        let host = unsafe { host_of_url_string(&url_str) };
                        if !host_in_allowlist(&host, &allowed) {
                            policy_cell_ref.set(POLICY_CANCEL);
                            return;
                        }
                    }

                    // 2. User intercept — only if a closure is registered.
                    if on_should_navigate != 0.0 {
                        let url_nb = nanbox_str(&url_str);
                        let closure_ptr =
                            unsafe { js_nanbox_get_pointer(on_should_navigate) } as *const u8;
                        if closure_ptr.is_null() {
                            return; // default ALLOW
                        }
                        let result = unsafe { js_closure_call1(closure_ptr, url_nb) };

                        // Per the API: undefined / no return = allow (matches JS
                        // "implicit allow"); explicit `false` / 0 / null = cancel.
                        let bits = result.to_bits();
                        let is_undefined = bits == 0x7FFC_0000_0000_0001;
                        if !is_undefined && unsafe { js_is_truthy(result) == 0 } {
                            policy_cell_ref.set(POLICY_CANCEL);
                        }
                    }

                    // 3. No intercept registered — leaves the default ALLOW.
                }),
            );
            let policy = policy_cell.get();

            // Deliver the decision exactly once. Calling back into the
            // WKWebView block also crosses the FFI boundary, so guard it too —
            // a panic here previously aborted the app (#3129, webview.rs:359).
            if !decision_handler.is_null() {
                crate::catch_callback_panic(
                    "webview decisionHandler",
                    std::panic::AssertUnwindSafe(|| {
                        unsafe { (*decision_handler).call((policy,)); }
                    }),
                );
            }
        }

        /// `webView:requestMediaCapturePermissionForOrigin:initiatedByFrame:type:decisionHandler:`
        ///
        /// WKWebView delegates camera/microphone decisions to `WKUIDelegate`.
        /// Without this method `getUserMedia()` rejects in embedded desktop
        /// WebViews even for localhost pages. Grant here; macOS TCC still owns
        /// the OS-level camera/microphone prompt/deny decision.
        #[unsafe(method(webView:requestMediaCapturePermissionForOrigin:initiatedByFrame:type:decisionHandler:))]
        fn request_media_capture_permission(
            &self,
            _webview: *mut AnyObject,
            _origin: *mut AnyObject,
            _frame: *mut AnyObject,
            _capture_type: i64,
            decision_handler: *const block2::Block<dyn Fn(i64)>,
        ) {
            const WK_PERMISSION_DECISION_GRANT: i64 = 1;
            show_denied_alerts_for_media_request();
            if !decision_handler.is_null() {
                unsafe { (*decision_handler).call((WK_PERMISSION_DECISION_GRANT,)); }
            }
        }

        /// `webView:didFinishNavigation:` — page finished loading. Reads the
        /// current URL and invokes `onLoaded(url)`.
        #[unsafe(method(webView:didFinishNavigation:))]
        fn did_finish(&self, webview: *mut AnyObject, _navigation: *mut AnyObject) {
            let key = self.ivars().callback_key.get();
            let on_loaded = WEBVIEW_STATES.with(|s| {
                s.borrow().get(&key).map(|st| st.on_loaded).unwrap_or(0.0)
            });
            if on_loaded == 0.0 {
                return;
            }
            crate::catch_callback_panic(
                "webview onLoaded",
                std::panic::AssertUnwindSafe(|| unsafe {
                    let url_str = if !webview.is_null() {
                        let url: *mut AnyObject = msg_send![webview, URL];
                        if !url.is_null() {
                            let abs: *mut AnyObject = msg_send![url, absoluteString];
                            if !abs.is_null() {
                                let ns: &NSString = &*(abs as *const NSString);
                                ns.to_string()
                            } else { String::new() }
                        } else { String::new() }
                    } else { String::new() };
                    let url_nb = nanbox_str(&url_str);
                    let closure_ptr = js_nanbox_get_pointer(on_loaded) as *const u8;
                    if !closure_ptr.is_null() {
                        js_closure_call1(closure_ptr, url_nb);
                    }
                }),
            );
        }

        /// `webView:didFailNavigation:withError:` — load failed after commit.
        #[unsafe(method(webView:didFailNavigation:withError:))]
        fn did_fail(
            &self,
            _webview: *mut AnyObject,
            _navigation: *mut AnyObject,
            error: *mut AnyObject,
        ) {
            self.dispatch_error(error);
        }

        /// `webView:didFailProvisionalNavigation:withError:` — load failed
        /// before content commit (DNS, TLS, etc.). Common case for users.
        #[unsafe(method(webView:didFailProvisionalNavigation:withError:))]
        fn did_fail_provisional(
            &self,
            _webview: *mut AnyObject,
            _navigation: *mut AnyObject,
            error: *mut AnyObject,
        ) {
            self.dispatch_error(error);
        }
    }
);

impl PerryWebViewDelegate {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryWebViewDelegateIvars {
            callback_key: Cell::new(0),
        });
        unsafe { msg_send![super(this), init] }
    }

    fn dispatch_error(&self, error: *mut AnyObject) {
        let key = self.ivars().callback_key.get();
        let on_error =
            WEBVIEW_STATES.with(|s| s.borrow().get(&key).map(|st| st.on_error).unwrap_or(0.0));
        if on_error == 0.0 {
            return;
        }
        crate::catch_callback_panic(
            "webview onError",
            std::panic::AssertUnwindSafe(|| unsafe {
                let (code, msg) = if !error.is_null() {
                    let c: i64 = msg_send![error, code];
                    let descr: *mut AnyObject = msg_send![error, localizedDescription];
                    let m = if !descr.is_null() {
                        let ns: &NSString = &*(descr as *const NSString);
                        ns.to_string()
                    } else {
                        String::new()
                    };
                    (c, m)
                } else {
                    (0, String::new())
                };
                let msg_nb = nanbox_str(&msg);
                let closure_ptr = js_nanbox_get_pointer(on_error) as *const u8;
                if !closure_ptr.is_null() {
                    js_closure_call2(closure_ptr, code as f64, msg_nb);
                }
            }),
        );
    }
}

// =============================================================================
// Public API
// =============================================================================

/// Create a WebView with `url` initial content. Returns the widget handle.
/// `ephemeral_hint` (1.0 = ephemeral, 0.0 = persistent) is applied to
/// `WKWebViewConfiguration.websiteDataStore` at construction time so
/// the choice takes effect for the very first navigation. v2-B fix.
pub fn create(url_ptr: *const u8, width: f64, height: f64, ephemeral_hint: f64) -> i64 {
    let url = str_from_header(url_ptr).to_string();
    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");

    // Preflight macOS TCC for camera + microphone. WKUIDelegate grants the
    // web-origin permission later, but the process still needs OS-level access.
    request_av_capture_access_once();

    unsafe {
        let frame = objc2_core_foundation::CGRect::new(
            objc2_core_foundation::CGPoint::new(0.0, 0.0),
            objc2_core_foundation::CGSize::new(
                if width > 0.0 { width } else { 600.0 },
                if height > 0.0 { height } else { 400.0 },
            ),
        );

        // 1. WKWebViewConfiguration. v2-B: pass the ephemeral hint up
        //    front so the websiteDataStore is correct from the first
        //    navigation onward. Default (1.0 = ephemeral) maps to
        //    nonPersistentDataStore; 0.0 → defaultDataStore.
        let cfg_cls = AnyClass::get(c"WKWebViewConfiguration")
            .expect("WKWebViewConfiguration not found — link WebKit.framework");
        let cfg: *mut AnyObject = msg_send![cfg_cls, new];
        let store_cls = AnyClass::get(c"WKWebsiteDataStore").unwrap();
        let store: *mut AnyObject = if ephemeral_hint > 0.5 {
            msg_send![store_cls, nonPersistentDataStore]
        } else {
            msg_send![store_cls, defaultDataStore]
        };
        let _: () = msg_send![cfg, setWebsiteDataStore: store];

        // 2. WKWebView.
        let wv_cls = AnyClass::get(c"WKWebView").expect("WKWebView not found");
        let wv: *mut AnyObject = msg_send![wv_cls, alloc];
        let wv: *mut AnyObject = msg_send![wv, initWithFrame: frame, configuration: cfg];

        // 3. Delegate — single delegate object per widget; ivar key is the
        //    delegate's own address.
        let delegate = PerryWebViewDelegate::new();
        let key = Retained::as_ptr(&delegate) as usize;
        delegate.ivars().callback_key.set(key);
        let _: () = msg_send![wv, setNavigationDelegate: &*delegate];
        let _: () = msg_send![wv, setUIDelegate: &*delegate];

        // 4. Initial load.
        if !url.is_empty() {
            load_url_on_webview(wv, &url);
        }

        // 5. Register state. The webview_ptr is held weakly; we never deref
        //    after the widget is destroyed because we drop the entry on
        //    widget destroy (TODO: hook widget-destroy in mod.rs to call
        //    `forget_state(handle)` — for now the entry leaks at widget
        //    destruction, which matches the textarea/picker leak shape).
        WEBVIEW_STATES.with(|s| {
            s.borrow_mut().insert(
                key,
                WebViewState {
                    webview_ptr: wv as *const AnyObject,
                    on_should_navigate: 0.0,
                    on_loaded: 0.0,
                    on_error: 0.0,
                    allowed_domains: Vec::new(),
                },
            );
        });

        // 6. Register as a Perry widget. WKWebView is an NSView subclass.
        let view: Retained<NSView> = Retained::retain(wv as *mut NSView).unwrap();
        let handle = super::register_widget(view);
        HANDLE_TO_KEY.with(|m| {
            m.borrow_mut().insert(handle, key);
        });

        // 7. Leak the delegate Retained so it stays alive as long as the
        //    WKWebView holds it as navigationDelegate (WKWebView holds delegates
        //    weakly per the WKWebView docs).
        std::mem::forget(delegate);

        let _ = mtm;
        handle
    }
}

/// Imperative `loadUrl` — replaces the visible page.
pub fn load_url(handle: i64, url_ptr: *const u8) {
    let url = str_from_header(url_ptr).to_string();
    if url.is_empty() {
        return;
    }
    if let Some(wv) = webview_for_handle(handle) {
        unsafe { load_url_on_webview(wv, &url) };
    }
}

unsafe fn load_url_on_webview(webview: *mut AnyObject, url: &str) {
    let url_cls = AnyClass::get(c"NSURL").unwrap();
    let url_str = NSString::from_str(url);
    let nsurl: *mut AnyObject = msg_send![url_cls, URLWithString: &*url_str];
    if nsurl.is_null() {
        return;
    }
    let req_cls = AnyClass::get(c"NSURLRequest").unwrap();
    let req: *mut AnyObject = msg_send![req_cls, requestWithURL: nsurl];
    if req.is_null() {
        return;
    }
    let _: *mut AnyObject = msg_send![webview, loadRequest: req];
}

pub fn reload(handle: i64) {
    if let Some(wv) = webview_for_handle(handle) {
        unsafe {
            let _: *mut AnyObject = msg_send![wv, reload];
        }
    }
}

pub fn go_back(handle: i64) {
    if let Some(wv) = webview_for_handle(handle) {
        unsafe {
            let _: *mut AnyObject = msg_send![wv, goBack];
        }
    }
}

pub fn go_forward(handle: i64) {
    if let Some(wv) = webview_for_handle(handle) {
        unsafe {
            let _: *mut AnyObject = msg_send![wv, goForward];
        }
    }
}

pub fn can_go_back(handle: i64) -> i64 {
    if let Some(wv) = webview_for_handle(handle) {
        unsafe {
            let v: bool = msg_send![wv, canGoBack];
            return if v { 1 } else { 0 };
        }
    }
    0
}

/// Async JS evaluate — fires `callback(result_string)` once the JS result
/// arrives. Errors and null/undefined results all surface as the empty
/// string for predictable user code (matches `localStorage.getItem` shape).
pub fn evaluate_js(handle: i64, js_ptr: *const u8, callback: f64) {
    let js = str_from_header(js_ptr).to_string();
    let wv = match webview_for_handle(handle) {
        Some(w) => w,
        None => return,
    };
    unsafe {
        let js_str = NSString::from_str(&js);

        let block = block2::RcBlock::new(move |result: *mut AnyObject, _error: *mut AnyObject| {
            crate::catch_callback_panic(
                "webview evaluateJs callback",
                std::panic::AssertUnwindSafe(|| {
                    let s = if !result.is_null() {
                        // result might be NSString, NSNumber, NSDictionary,
                        // NSArray, or NSNull. Use `description` for a stable
                        // string form across all cases.
                        let descr: *mut AnyObject = msg_send![result, description];
                        if !descr.is_null() {
                            let ns: &NSString = &*(descr as *const NSString);
                            ns.to_string()
                        } else {
                            String::new()
                        }
                    } else {
                        String::new()
                    };
                    let nb = nanbox_str(&s);
                    let closure_ptr = js_nanbox_get_pointer(callback) as *const u8;
                    if !closure_ptr.is_null() {
                        js_closure_call1(closure_ptr, nb);
                    }
                }),
            );
        });
        let _: () = msg_send![wv, evaluateJavaScript: &*js_str, completionHandler: &*block];
    }
}

pub fn clear_cookies(handle: i64) {
    let wv = match webview_for_handle(handle) {
        Some(w) => w,
        None => return,
    };
    unsafe {
        let cfg: *mut AnyObject = msg_send![wv, configuration];
        if cfg.is_null() {
            return;
        }
        let store: *mut AnyObject = msg_send![cfg, websiteDataStore];
        if store.is_null() {
            return;
        }
        // WKWebsiteDataStore.allWebsiteDataTypes — class method.
        let store_cls = AnyClass::get(c"WKWebsiteDataStore").unwrap();
        let types: *mut AnyObject = msg_send![store_cls, allWebsiteDataTypes];
        let date_cls = AnyClass::get(c"NSDate").unwrap();
        let epoch: *mut AnyObject = msg_send![date_cls, dateWithTimeIntervalSince1970: 0.0_f64];
        let no_op = block2::RcBlock::new(|| {});
        let _: () = msg_send![
            store,
            removeDataOfTypes: types,
            modifiedSince: epoch,
            completionHandler: &*no_op
        ];
    }
}

pub fn set_user_agent(handle: i64, ua_ptr: *const u8) {
    let ua = str_from_header(ua_ptr).to_string();
    if let Some(wv) = webview_for_handle(handle) {
        unsafe {
            let ns = NSString::from_str(&ua);
            let _: () = msg_send![wv, setCustomUserAgent: &*ns];
        }
    }
}

/// Set the allowlist. `domains_ptr` is a NaN-boxed JS array of strings —
/// we read it through the runtime helpers `js_array_get_length` /
/// `js_array_get_element`. Empty array clears the allowlist.
pub fn set_allowed_domains(handle: i64, domains_arr_handle: i64) {
    extern "C" {
        fn js_array_get_length(arr: i64) -> i64;
        fn js_array_get_element_f64(arr: i64, index: i64) -> f64;
    }

    let mut domains = Vec::new();
    unsafe {
        let len = js_array_get_length(domains_arr_handle);
        for i in 0..len {
            let elem = js_array_get_element_f64(domains_arr_handle, i);
            let str_ptr = js_get_string_pointer_unified(elem) as *const u8;
            if !str_ptr.is_null() {
                domains.push(str_from_header(str_ptr).to_string());
            }
        }
    }
    if let Some(key) = HANDLE_TO_KEY.with(|m| m.borrow().get(&handle).copied()) {
        WEBVIEW_STATES.with(|s| {
            if let Some(st) = s.borrow_mut().get_mut(&key) {
                st.allowed_domains = domains;
            }
        });
    }
}

pub fn set_ephemeral(handle: i64, ephemeral: i64) {
    // Apply to the webview's configuration's websiteDataStore. WKWebView
    // doesn't expose a setter for the dataStore post-creation; the closest
    // thing is replacing the config-on-create path. For v1 we accept this
    // limitation and document it: call set_ephemeral BEFORE the first
    // navigation. (TODO: refactor create() to defer initial load until after
    // ephemeral is applied — would need an explicit `webviewStart` call.)
    if let Some(wv) = webview_for_handle(handle) {
        unsafe {
            let cfg: *mut AnyObject = msg_send![wv, configuration];
            if cfg.is_null() {
                return;
            }
            let store_cls = AnyClass::get(c"WKWebsiteDataStore").unwrap();
            let store: *mut AnyObject = if ephemeral != 0 {
                msg_send![store_cls, nonPersistentDataStore]
            } else {
                msg_send![store_cls, defaultDataStore]
            };
            let _: () = msg_send![cfg, setWebsiteDataStore: store];
        }
    }
}

pub fn set_on_should_navigate(handle: i64, closure: f64) {
    if let Some(key) = HANDLE_TO_KEY.with(|m| m.borrow().get(&handle).copied()) {
        WEBVIEW_STATES.with(|s| {
            if let Some(st) = s.borrow_mut().get_mut(&key) {
                st.on_should_navigate = closure;
            }
        });
    }
}

pub fn set_on_loaded(handle: i64, closure: f64) {
    if let Some(key) = HANDLE_TO_KEY.with(|m| m.borrow().get(&handle).copied()) {
        WEBVIEW_STATES.with(|s| {
            if let Some(st) = s.borrow_mut().get_mut(&key) {
                st.on_loaded = closure;
            }
        });
    }
}

pub fn set_on_error(handle: i64, closure: f64) {
    if let Some(key) = HANDLE_TO_KEY.with(|m| m.borrow().get(&handle).copied()) {
        WEBVIEW_STATES.with(|s| {
            if let Some(st) = s.borrow_mut().get_mut(&key) {
                st.on_error = closure;
            }
        });
    }
}

// =============================================================================
// Internal helpers
// =============================================================================

fn webview_for_handle(handle: i64) -> Option<*mut AnyObject> {
    HANDLE_TO_KEY
        .with(|m| m.borrow().get(&handle).copied())
        .and_then(|key| {
            WEBVIEW_STATES.with(|s| {
                s.borrow()
                    .get(&key)
                    .map(|st| st.webview_ptr as *mut AnyObject)
            })
        })
}

/// Suppress dead_code warnings for `js_closure_call0` import — kept for
/// future event hooks the design names but doesn't ship in Phase 1.
#[allow(dead_code)]
fn _used_js_closure_call0() {
    let _ = js_closure_call0 as unsafe extern "C" fn(_) -> _;
}
