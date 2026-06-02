//! WebView widget — `WKWebView` on iOS. Mirrors the macOS impl in
//! `crates/perry-ui-macos/src/widgets/webview.rs` with UIKit-flavored
//! framework wiring (`UIView` instead of `NSView`). Tracking issue #658,
//! Phase 1.
//!
//! Behavior contract is identical across all WKWebView platforms — see the
//! macOS file for the full design + delegate-callback architecture.

use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2::{define_class, AnyThread, DefinedClass};
use objc2_foundation::{NSObject, NSString};
use objc2_ui_kit::UIView;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_closure_call2(closure: *const u8, arg1: f64, arg2: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
    fn js_nanbox_string(ptr: i64) -> f64;
    fn js_is_truthy(value: f64) -> i32;
}

struct WebViewState {
    webview_ptr: *const AnyObject,
    on_should_navigate: f64,
    on_loaded: f64,
    on_error: f64,
    allowed_domains: Vec<String>,
}

thread_local! {
    static WEBVIEW_STATES: RefCell<HashMap<usize, WebViewState>> = RefCell::new(HashMap::new());
    static HANDLE_TO_KEY: RefCell<HashMap<i64, usize>> = RefCell::new(HashMap::new());
}

fn str_from_header(ptr: *const u8) -> &'static str {
    if ptr.is_null() {
        return "";
    }
    unsafe {
        let header = ptr as *const perry_runtime::string::StringHeader;
        let len = (*header).byte_len as usize;
        let data = ptr.add(std::mem::size_of::<perry_runtime::string::StringHeader>());
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len))
    }
}

fn nanbox_str(s: &str) -> f64 {
    let bytes = s.as_bytes();
    unsafe {
        let p = js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64);
        js_nanbox_string(p as i64)
    }
}

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

fn host_of_url_string(s: &str) -> String {
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

pub struct PerryWebViewDelegateIvars {
    callback_key: Cell<usize>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryWebViewDelegate"]
    #[ivars = PerryWebViewDelegateIvars]
    pub struct PerryWebViewDelegate;

    impl PerryWebViewDelegate {
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
            let url_str = unsafe { url_from_action(action) };

            let (on_should_navigate, allowed) = WEBVIEW_STATES.with(|s| {
                let states = s.borrow();
                if let Some(st) = states.get(&key) {
                    (st.on_should_navigate, st.allowed_domains.clone())
                } else {
                    (0.0, Vec::new())
                }
            });

            if !allowed.is_empty() {
                let host = host_of_url_string(&url_str);
                if !host_in_allowlist(&host, &allowed) {
                    if !decision_handler.is_null() {
                        unsafe { (*decision_handler).call((POLICY_CANCEL,)); }
                    }
                    return;
                }
            }

            if on_should_navigate != 0.0 {
                let url_nb = nanbox_str(&url_str);
                let closure_ptr = unsafe { js_nanbox_get_pointer(on_should_navigate) } as *const u8;
                let result_cell = Cell::new(f64::from_bits(0x7FFC_0000_0000_0001));
                if !closure_ptr.is_null() {
                    let result_cell_ref = &result_cell;
                    crate::catch_callback_panic(
                        "webview onShouldNavigate",
                        std::panic::AssertUnwindSafe(|| {
                            let r = unsafe { js_closure_call1(closure_ptr, url_nb) };
                            result_cell_ref.set(r);
                        }),
                    );
                }
                let result = result_cell.get();
                let bits = result.to_bits();
                let is_undefined = bits == 0x7FFC_0000_0000_0001;
                let policy = if is_undefined {
                    POLICY_ALLOW
                } else if unsafe { js_is_truthy(result) != 0 } {
                    POLICY_ALLOW
                } else {
                    POLICY_CANCEL
                };
                if !decision_handler.is_null() {
                    unsafe { (*decision_handler).call((policy,)); }
                }
                return;
            }

            if !decision_handler.is_null() {
                unsafe { (*decision_handler).call((POLICY_ALLOW,)); }
            }
        }

        #[unsafe(method(webView:didFinishNavigation:))]
        fn did_finish(&self, webview: *mut AnyObject, _navigation: *mut AnyObject) {
            let key = self.ivars().callback_key.get();
            let on_loaded = WEBVIEW_STATES.with(|s| {
                s.borrow().get(&key).map(|st| st.on_loaded).unwrap_or(0.0)
            });
            if on_loaded == 0.0 { return; }
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

        #[unsafe(method(webView:didFailNavigation:withError:))]
        fn did_fail(
            &self,
            _webview: *mut AnyObject,
            _navigation: *mut AnyObject,
            error: *mut AnyObject,
        ) {
            self.dispatch_error(error);
        }

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

pub fn create(url_ptr: *const u8, width: f64, height: f64, ephemeral_hint: f64) -> i64 {
    let url = str_from_header(url_ptr).to_string();
    unsafe {
        let frame = objc2_core_foundation::CGRect::new(
            objc2_core_foundation::CGPoint::new(0.0, 0.0),
            objc2_core_foundation::CGSize::new(
                if width > 0.0 { width } else { 600.0 },
                if height > 0.0 { height } else { 400.0 },
            ),
        );

        let cfg_cls = AnyClass::get(c"WKWebViewConfiguration")
            .expect("WKWebViewConfiguration not found — link WebKit.framework");
        let cfg: *mut AnyObject = msg_send![cfg_cls, new];
        // v2-B: ephemeral hint at construction time (mirrors macOS).
        let store_cls = AnyClass::get(c"WKWebsiteDataStore").unwrap();
        let store: *mut AnyObject = if ephemeral_hint > 0.5 {
            msg_send![store_cls, nonPersistentDataStore]
        } else {
            msg_send![store_cls, defaultDataStore]
        };
        let _: () = msg_send![cfg, setWebsiteDataStore: store];

        let wv_cls = AnyClass::get(c"WKWebView").expect("WKWebView not found");
        let wv: *mut AnyObject = msg_send![wv_cls, alloc];
        let wv: *mut AnyObject = msg_send![wv, initWithFrame: frame, configuration: cfg];

        let delegate = PerryWebViewDelegate::new();
        let key = Retained::as_ptr(&delegate) as usize;
        delegate.ivars().callback_key.set(key);
        let _: () = msg_send![wv, setNavigationDelegate: &*delegate];

        if !url.is_empty() {
            load_url_on_webview(wv, &url);
        }

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

        let view: Retained<UIView> = Retained::retain(wv as *mut UIView).unwrap();
        let handle = super::register_widget(view);
        HANDLE_TO_KEY.with(|m| {
            m.borrow_mut().insert(handle, key);
        });
        std::mem::forget(delegate);
        handle
    }
}

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
    if let Some(key) = HANDLE_TO_KEY.with(|m| m.borrow().get(&handle).copied()) {
        WEBVIEW_STATES.with(|s| {
            if let Some(st) = s.borrow_mut().get_mut(&key) {
                st.allowed_domains = domains;
            }
        });
    }
}

pub fn set_ephemeral(handle: i64, ephemeral: i64) {
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
