//! Deep links (issue #583) — iOS implementation.
//!
//! Three OS surfaces feed the same `appOnOpenUrl(cb)` callback:
//!
//!   1. Custom URL schemes (`myapp://…`) — `application(_:open:options:)`
//!      on the AppDelegate (when the app was already running, or when the
//!      app is launched and SceneDelegate isn't in use), and
//!      `scene(_:openURLContexts:)` on the SceneDelegate (the modern
//!      iOS 13+ scene-based path that Perry's PerrySceneDelegate uses).
//!   2. Universal Links (`https://yourdomain.com/path?…`) —
//!      `application(_:continue:restorationHandler:)` on the AppDelegate
//!      and `scene(_:continueUserActivity:)` on the SceneDelegate. Both
//!      hand us an `NSUserActivity` whose `webpageURL` is the link.
//!   3. Cold-start launch URL — extracted from the scene-connection
//!      options' `URLContexts` set, or from `application:open` if the
//!      app uses the legacy non-scene path.
//!
//! The callback registry is process-global. Setting a fresh handler via
//! `set_handler` replaces the previous one (deep-link routing is normally
//! a single sink — the app's navigation router). A handler registered
//! AFTER a cold-start URL has already arrived gets the URL replayed
//! synchronously — so `appOnOpenUrl(...)` at module load is enough; no
//! `appGetLaunchUrl()` read is required for the cold-start flow.

use std::cell::RefCell;

extern "C" {
    fn js_run_stdlib_pump();
    fn js_promise_run_microtasks() -> i32;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_closure_call2(closure: *const u8, arg0: f64, arg1: f64) -> f64;
    fn js_string_from_bytes(ptr: *const u8, len: u32) -> *mut u8;
    fn js_nanbox_string(ptr: i64) -> f64;
}

thread_local! {
    /// Registered TS callback (NaN-boxed closure pointer).
    static HANDLER: RefCell<Option<f64>> = const { RefCell::new(None) };
    /// Pending cold-start URL — captured before any handler is registered.
    /// First `set_handler` call drains it synchronously so the JS side
    /// sees the launch URL through the unified callback.
    static PENDING_COLD_START: RefCell<Option<String>> = const { RefCell::new(None) };
    /// Latest cold-start URL, kept around for `appGetLaunchUrl()` readers.
    /// Cleared once a handler has consumed it (so re-launches don't see
    /// the previous run's URL stuck).
    static LAUNCH_URL: RefCell<String> = const { RefCell::new(String::new()) };
}

unsafe fn nanbox_str(s: &str) -> f64 {
    let bytes = s.as_bytes();
    let ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    js_nanbox_string(ptr as i64)
}

unsafe fn invoke_handler(url: &str, source: &str) {
    js_run_stdlib_pump();
    js_promise_run_microtasks();
    let handler = HANDLER.with(|h| *h.borrow());
    if let Some(closure_f64) = handler {
        let ptr = js_nanbox_get_pointer(closure_f64) as *const u8;
        if !ptr.is_null() {
            let url_v = nanbox_str(url);
            let src_v = nanbox_str(source);
            js_closure_call2(ptr, url_v, src_v);
        }
    }
}

pub fn set_handler(callback: f64) {
    HANDLER.with(|h| *h.borrow_mut() = Some(callback));
    let pending = PENDING_COLD_START.with(|p| p.borrow_mut().take());
    if let Some(url) = pending {
        unsafe {
            invoke_handler(&url, "cold-start");
        }
    }
}

pub fn launch_url() -> String {
    LAUNCH_URL.with(|u| u.borrow().clone())
}

/// Public entry point for the AppDelegate / SceneDelegate when a URL
/// arrives during cold start. Caches the URL so a later `appGetLaunchUrl`
/// or `appOnOpenUrl` registration sees it; if a handler is already set,
/// fires it immediately.
pub fn dispatch_cold_start(url: &str) {
    LAUNCH_URL.with(|u| *u.borrow_mut() = url.to_string());
    let has_handler = HANDLER.with(|h| h.borrow().is_some());
    if has_handler {
        unsafe {
            invoke_handler(url, "cold-start");
        }
    } else {
        PENDING_COLD_START.with(|p| *p.borrow_mut() = Some(url.to_string()));
    }
}

/// Public entry point for the AppDelegate / SceneDelegate when a URL
/// arrives while the app is already running (or returning from background).
pub fn dispatch_foreground(url: &str) {
    LAUNCH_URL.with(|u| *u.borrow_mut() = url.to_string());
    unsafe {
        invoke_handler(url, "foreground");
    }
}

// ─── ObjC bridge helpers ─────────────────────────────────────────────────────
// Called from the AppDelegate / SceneDelegate methods registered in app.rs.

use objc2::msg_send;
use objc2::runtime::AnyObject;
use objc2_foundation::NSString;

/// Extract the absolute URL string from an NSURL pointer.
unsafe fn nsurl_to_string(url: *const AnyObject) -> Option<String> {
    if url.is_null() {
        return None;
    }
    let abs_str: *const NSString = msg_send![url, absoluteString];
    if abs_str.is_null() {
        return None;
    }
    Some((*abs_str).to_string())
}

/// Walk the UIOpenURLContext set passed to scene:openURLContexts: and
/// fire the foreground dispatch for each one.
pub unsafe fn dispatch_scene_open_url_contexts(contexts: *const AnyObject) {
    if contexts.is_null() {
        return;
    }
    // [NSSet allObjects] -> NSArray
    let array: *const AnyObject = msg_send![contexts, allObjects];
    if array.is_null() {
        return;
    }
    let count: usize = msg_send![array, count];
    for i in 0..count {
        let ctx: *const AnyObject = msg_send![array, objectAtIndex: i];
        if ctx.is_null() {
            continue;
        }
        let url: *const AnyObject = msg_send![ctx, URL];
        if let Some(s) = nsurl_to_string(url) {
            dispatch_foreground(&s);
        }
    }
}

/// On scene-connect, the connection options carry any URLs that triggered
/// the launch (custom-scheme cold start) and any NSUserActivity for a
/// Universal Link cold start. Drain both into the cold-start dispatch.
pub unsafe fn dispatch_scene_connection_options(options: *const AnyObject) {
    if options.is_null() {
        return;
    }
    // 1. URLContexts — NSSet<UIOpenURLContext> for custom-scheme cold start.
    let contexts: *const AnyObject = msg_send![options, URLContexts];
    if !contexts.is_null() {
        let array: *const AnyObject = msg_send![contexts, allObjects];
        if !array.is_null() {
            let count: usize = msg_send![array, count];
            for i in 0..count {
                let ctx: *const AnyObject = msg_send![array, objectAtIndex: i];
                if ctx.is_null() {
                    continue;
                }
                let url: *const AnyObject = msg_send![ctx, URL];
                if let Some(s) = nsurl_to_string(url) {
                    dispatch_cold_start(&s);
                }
            }
        }
    }
    // 2. userActivities — NSSet<NSUserActivity> for Universal Link cold
    //    start. Each activity's webpageURL is the deep-link target.
    let activities: *const AnyObject = msg_send![options, userActivities];
    if !activities.is_null() {
        let array: *const AnyObject = msg_send![activities, allObjects];
        if !array.is_null() {
            let count: usize = msg_send![array, count];
            for i in 0..count {
                let activity: *const AnyObject = msg_send![array, objectAtIndex: i];
                if activity.is_null() {
                    continue;
                }
                let url: *const AnyObject = msg_send![activity, webpageURL];
                if let Some(s) = nsurl_to_string(url) {
                    dispatch_cold_start(&s);
                }
            }
        }
    }
}

/// AppDelegate `application(_:continue:restorationHandler:)` —
/// Universal Link delivery while running.
pub unsafe fn dispatch_continue_user_activity(activity: *const AnyObject) {
    if activity.is_null() {
        return;
    }
    let url: *const AnyObject = msg_send![activity, webpageURL];
    if let Some(s) = nsurl_to_string(url) {
        dispatch_foreground(&s);
    }
}

/// AppDelegate `application(_:open:options:)` — custom-scheme delivery
/// when the app is already running (legacy non-scene path).
pub unsafe fn dispatch_app_open_url(url: *const AnyObject) {
    if let Some(s) = nsurl_to_string(url) {
        dispatch_foreground(&s);
    }
}
