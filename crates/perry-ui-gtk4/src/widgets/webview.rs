//! WebView widget — `WebKitGTK 6.0` (webkit6 crate) for issue #658
//! Phase 4 / Linux. Mirrors the WKWebView impl on macOS / iOS / visionOS
//! one-for-one: `decide-policy::navigation-action` is the sync intercept
//! point for `onShouldNavigate` (return `false` from the user closure
//! → `policy_decision.ignore()`); `load-changed` fires `onLoaded` when
//! the load completes; `load-failed` fires `onError`.
//!
//! Build dep: `libwebkitgtk-6.0-dev` (Ubuntu 22.10+ / Debian 12+).
//! WebKitNetworkSession::new_ephemeral isolates per-WebView storage.

use gtk4::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;
use webkit6::prelude::*;

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_closure_call2(closure: *const u8, arg1: f64, arg2: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_nanbox_string(ptr: i64) -> f64;
    fn js_is_truthy(value: f64) -> i32;
}

struct WebViewState {
    webview: webkit6::WebView,
    on_should_navigate: f64,
    on_loaded: f64,
    on_error: f64,
    allowed_domains: Vec<String>,
}

thread_local! {
    static WEBVIEW_STATES: RefCell<HashMap<i64, WebViewState>> = RefCell::new(HashMap::new());
}

pub(crate) fn str_from_header(ptr: *const u8) -> &'static str {
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
    let p = perry_runtime::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    unsafe { js_nanbox_string(p as i64) }
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

pub fn create(url_ptr: *const u8, width: f64, height: f64, ephemeral_hint: f64) -> i64 {
    crate::app::ensure_gtk_init();
    let url = str_from_header(url_ptr).to_string();

    // v2-B: ephemeral_hint chooses ephemeral vs persistent NetworkSession
    // at construction time. WebKitNetworkSession can't be replaced after
    // the WebKitWebView is built, so this must happen here.
    let session = if ephemeral_hint > 0.5 {
        webkit6::NetworkSession::new_ephemeral()
    } else {
        // Persistent session under XDG_DATA_HOME/perry-webview so cookies
        // survive across app launches.
        let mut data = dirs::data_dir().unwrap_or_else(std::env::temp_dir);
        data.push("perry-webview");
        let mut cache = dirs::cache_dir().unwrap_or_else(std::env::temp_dir);
        cache.push("perry-webview");
        let _ = std::fs::create_dir_all(&data);
        let _ = std::fs::create_dir_all(&cache);
        webkit6::NetworkSession::new(
            Some(data.to_string_lossy().as_ref()),
            Some(cache.to_string_lossy().as_ref()),
        )
    };
    let webview = webkit6::WebView::builder()
        .network_session(&session)
        .build();

    if width > 0.0 && height > 0.0 {
        webview.set_size_request(width as i32, height as i32);
    } else {
        webview.set_hexpand(true);
        webview.set_vexpand(true);
    }

    if !url.is_empty() {
        webview.load_uri(&url);
    }

    let widget: gtk4::Widget = webview.clone().upcast();
    let handle = super::register_widget(widget);

    WEBVIEW_STATES.with(|s| {
        s.borrow_mut().insert(
            handle,
            WebViewState {
                webview: webview.clone(),
                on_should_navigate: 0.0,
                on_loaded: 0.0,
                on_error: 0.0,
                allowed_domains: Vec::new(),
            },
        );
    });

    install_signal_handlers(handle, &webview);

    handle
}

fn install_signal_handlers(handle: i64, webview: &webkit6::WebView) {
    // decide-policy::navigation-action — sync intercept.
    webview.connect_decide_policy(move |_wv, decision, decision_type| {
        if decision_type != webkit6::PolicyDecisionType::NavigationAction {
            return false; // let WebKit handle it
        }
        let action_decision: &webkit6::NavigationPolicyDecision = match decision.downcast_ref() {
            Some(d) => d,
            None => return false,
        };
        let mut action = match action_decision.navigation_action() {
            Some(a) => a,
            None => return false,
        };
        let request = match action.request() {
            Some(r) => r,
            None => return false,
        };
        let url = request.uri().map(|u| u.to_string()).unwrap_or_default();

        let (on_should, allowed) = WEBVIEW_STATES.with(|s| {
            s.borrow()
                .get(&handle)
                .map(|st| (st.on_should_navigate, st.allowed_domains.clone()))
                .unwrap_or((0.0, Vec::new()))
        });

        // Allowlist gate.
        if !allowed.is_empty() {
            let host = host_of_url(&url);
            if !host_in_allowlist(&host, &allowed) {
                decision.ignore();
                return true;
            }
        }

        if on_should != 0.0 {
            let url_nb = nanbox_str(&url);
            let closure_ptr = unsafe { js_nanbox_get_pointer(on_should) } as *const u8;
            if !closure_ptr.is_null() {
                let result = unsafe { js_closure_call1(closure_ptr, url_nb) };
                let bits = result.to_bits();
                let is_undefined = bits == 0x7FFC_0000_0000_0001;
                let allow = is_undefined || unsafe { js_is_truthy(result) != 0 };
                if !allow {
                    decision.ignore();
                    return true;
                }
            }
        }
        false // let WebKit proceed
    });

    // load-changed → onLoaded when the load reaches `Finished`.
    webview.connect_load_changed(move |wv, event| {
        if event != webkit6::LoadEvent::Finished {
            return;
        }
        let on_loaded = WEBVIEW_STATES.with(|s| {
            s.borrow()
                .get(&handle)
                .map(|st| st.on_loaded)
                .unwrap_or(0.0)
        });
        if on_loaded == 0.0 {
            return;
        }
        let url = wv.uri().map(|u| u.to_string()).unwrap_or_default();
        let url_nb = nanbox_str(&url);
        let closure_ptr = unsafe { js_nanbox_get_pointer(on_loaded) } as *const u8;
        if !closure_ptr.is_null() {
            unsafe {
                js_closure_call1(closure_ptr, url_nb);
            }
        }
    });

    // load-failed → onError. Returning `true` prevents WebKit's default
    // error page from rendering; we return `false` so the user sees the
    // standard WebKit error page if they want.
    webview.connect_load_failed(move |_wv, _event, _failing_uri, error| {
        let on_error =
            WEBVIEW_STATES.with(|s| s.borrow().get(&handle).map(|st| st.on_error).unwrap_or(0.0));
        if on_error == 0.0 {
            return false;
        }
        // glib::Error doesn't expose `code()` in glib 0.20+; reach into the
        // underlying GError struct via ToGlibPtr to read the raw i32 code.
        let code = unsafe {
            use gtk4::glib::translate::ToGlibPtr;
            let ptr: *const gtk4::glib::ffi::GError = error.to_glib_none().0;
            (*ptr).code as f64
        };
        let msg = error.message().to_string();
        let msg_nb = nanbox_str(&msg);
        let closure_ptr = unsafe { js_nanbox_get_pointer(on_error) } as *const u8;
        if !closure_ptr.is_null() {
            unsafe {
                js_closure_call2(closure_ptr, code, msg_nb);
            }
        }
        false
    });
}

pub fn load_url(handle: i64, url_ptr: *const u8) {
    let url = str_from_header(url_ptr);
    if url.is_empty() {
        return;
    }
    WEBVIEW_STATES.with(|s| {
        if let Some(st) = s.borrow().get(&handle) {
            st.webview.load_uri(url);
        }
    });
}

pub fn reload(handle: i64) {
    WEBVIEW_STATES.with(|s| {
        if let Some(st) = s.borrow().get(&handle) {
            st.webview.reload();
        }
    });
}

pub fn go_back(handle: i64) {
    WEBVIEW_STATES.with(|s| {
        if let Some(st) = s.borrow().get(&handle) {
            st.webview.go_back();
        }
    });
}

pub fn go_forward(handle: i64) {
    WEBVIEW_STATES.with(|s| {
        if let Some(st) = s.borrow().get(&handle) {
            st.webview.go_forward();
        }
    });
}

pub fn can_go_back(handle: i64) -> i64 {
    WEBVIEW_STATES.with(|s| {
        if let Some(st) = s.borrow().get(&handle) {
            return if st.webview.can_go_back() { 1 } else { 0 };
        }
        0
    })
}

/// Async JS evaluate via WebKit's `evaluate_javascript`. The result is
/// stringified via `JSCValue::to_string`; if the script throws or
/// returns null/undefined, the empty string is delivered.
pub fn evaluate_js(handle: i64, js_ptr: *const u8, callback: f64) {
    let js = str_from_header(js_ptr).to_string();
    let webview = WEBVIEW_STATES.with(|s| s.borrow().get(&handle).map(|st| st.webview.clone()));
    let webview = match webview {
        Some(w) => w,
        None => return,
    };

    let cancellable: Option<&gtk4::gio::Cancellable> = None;
    webview.evaluate_javascript(&js, None, None, cancellable, move |result| {
        let s = match result {
            Ok(value) => value.to_string(),
            Err(_) => String::new(),
        };
        let nb = nanbox_str(&s);
        let closure_ptr = unsafe { js_nanbox_get_pointer(callback) } as *const u8;
        if !closure_ptr.is_null() {
            unsafe {
                js_closure_call1(closure_ptr, nb);
            }
        }
    });
}

pub fn clear_cookies(handle: i64) {
    WEBVIEW_STATES.with(|s| {
        if let Some(st) = s.borrow().get(&handle) {
            if let Some(session) = st.webview.network_session() {
                // webkit6 0.4 removed CookieManager::delete_all_cookies in
                // favor of WebsiteDataManager::clear with a type bitmask.
                // COOKIES bitflag + duration 0 = "all cookies since epoch".
                if let Some(dm) = session.website_data_manager() {
                    let cancellable: Option<&gtk4::gio::Cancellable> = None;
                    // glib::TimeSpan is microseconds. 0 = "from epoch" =
                    // clear all cookies regardless of age.
                    dm.clear(
                        webkit6::WebsiteDataTypes::COOKIES,
                        gtk4::glib::TimeSpan::from_seconds(0),
                        cancellable,
                        |_| {},
                    );
                }
            }
        }
    });
}

pub fn set_user_agent(handle: i64, ua_ptr: *const u8) {
    let ua = str_from_header(ua_ptr).to_string();
    WEBVIEW_STATES.with(|s| {
        if let Some(st) = s.borrow().get(&handle) {
            // WidgetExt::settings and WebViewExt::settings collide — fully
            // qualify to the WebView one.
            if let Some(settings) = webkit6::prelude::WebViewExt::settings(&st.webview) {
                settings.set_user_agent(Some(&ua));
            }
        }
    });
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

pub fn set_ephemeral(_handle: i64, _ephemeral: i64) {
    // WebKitNetworkSession is set at WebView construction time. Per the
    // design doc this is documented as a v1 limitation matching the
    // Windows backend; deferred-init opt-out lands as a follow-up.
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
