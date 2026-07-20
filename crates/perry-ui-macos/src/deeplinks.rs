//! Deep links (issue #583) — macOS implementation.
//!
//! macOS routes URL deliveries through TWO surfaces depending on how the
//! app was launched:
//!
//!   1. `application(_:open:)` — AppKit calls this on the
//!      `NSApplicationDelegate` for both custom-scheme launches
//!      (`myapp://…` clicked from another app) and Universal-Link
//!      launches (`https://yourdomain.com/…` clicked from Mail /
//!      Safari). Wired via the `application:openURLs:` selector on the
//!      existing PerryAppDelegate.
//!
//!   2. The `kAEGetURL` Apple Event handler — a legacy AppleEvent
//!      surface that ALSO carries custom-scheme URL launches. Some
//!      launchers (older Spotlight pathways, AppleScript `open
//!      location`) fire this instead of the AppKit method, so wiring
//!      both is the conservative choice.
//!
//! In both cases the URL is routed through `dispatch_*` here, which
//! mirrors the iOS module's caching behaviour: a URL that arrives
//! before the JS module's `appOnOpenUrl(...)` has registered its
//! handler is held in `PENDING_COLD_START` and replayed synchronously
//! when the handler eventually arrives.

use crate::ffi::js_string_from_bytes;
use std::cell::RefCell;

use objc2::msg_send;
use objc2::runtime::AnyObject;
use objc2_foundation::NSString;

extern "C" {
    fn js_run_stdlib_pump();
    fn js_promise_run_microtasks() -> i32;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_closure_call2(closure: *const u8, arg0: f64, arg1: f64) -> f64;
    fn js_nanbox_string(ptr: i64) -> f64;
}

thread_local! {
    static HANDLER: RefCell<Option<f64>> = const { RefCell::new(None) };
    static PENDING_COLD_START: RefCell<Option<String>> = const { RefCell::new(None) };
    static LAUNCH_URL: RefCell<String> = const { RefCell::new(String::new()) };
    /// `false` until `applicationDidFinishLaunching:` fires. Before then,
    /// any URL delivery is treated as cold-start; after, as foreground.
    /// AppKit's order is: AppleEvent handler → openURLs → didFinish, so
    /// catching pre-didFinish deliveries gives us cold-start semantics
    /// without an extra source-tagging plumb.
    static APP_LAUNCHED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
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

/// Called by the AppDelegate's `applicationDidFinishLaunching:` so future
/// URL deliveries are treated as `foreground` rather than `cold-start`.
pub fn mark_launched() {
    APP_LAUNCHED.with(|c| c.set(true));
}

fn current_source() -> &'static str {
    if APP_LAUNCHED.with(|c| c.get()) {
        "foreground"
    } else {
        "cold-start"
    }
}

/// Dispatch a single URL string through the cold-start / foreground gate.
pub fn dispatch_url(url: &str) {
    LAUNCH_URL.with(|u| *u.borrow_mut() = url.to_string());
    let source = current_source();
    let has_handler = HANDLER.with(|h| h.borrow().is_some());
    if has_handler {
        unsafe {
            invoke_handler(url, source);
        }
    } else if source == "cold-start" {
        PENDING_COLD_START.with(|p| *p.borrow_mut() = Some(url.to_string()));
    }
    // If the handler isn't registered yet but we're already past launch,
    // we drop the URL: foreground deliveries can't be replayed without a
    // listener (the OS doesn't keep them around) and stashing them would
    // mask logic bugs in user code (forgetting to register the handler).
}

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

/// Bridge from AppDelegate `application:openURLs:` (NSArray<NSURL *>).
pub unsafe fn dispatch_open_urls(urls: *const AnyObject) {
    if urls.is_null() {
        return;
    }
    let count: usize = msg_send![urls, count];
    for i in 0..count {
        let url: *const AnyObject = msg_send![urls, objectAtIndex: i];
        if let Some(s) = nsurl_to_string(url) {
            dispatch_url(&s);
        }
    }
}

/// Bridge from the `kAEGetURL` Apple Event handler. The event's direct
/// parameter is a string; AppKit gives us back the URL via
/// `[event paramDescriptorForKeyword:keyDirectObject].stringValue`.
pub unsafe fn dispatch_apple_event_url(event: *const AnyObject) {
    if event.is_null() {
        return;
    }
    // keyDirectObject = '----' = 0x2D2D2D2D (FourCharCode).
    let key_direct_object: u32 = u32::from_be_bytes(*b"----");
    let descriptor: *const AnyObject =
        msg_send![event, paramDescriptorForKeyword: key_direct_object];
    if descriptor.is_null() {
        return;
    }
    let s_obj: *const NSString = msg_send![descriptor, stringValue];
    if s_obj.is_null() {
        return;
    }
    let s = (*s_obj).to_string();
    if !s.is_empty() {
        dispatch_url(&s);
    }
}

/// Install the `kAEGetURL` handler on the shared NSAppleEventManager.
/// Called once from `app_run` after the AppDelegate is in place. The
/// `selector` we register fires `apple_event_get_url:withReplyEvent:`
/// on the AppDelegate, which forwards to `dispatch_apple_event_url`.
pub unsafe fn install_apple_event_handler(delegate: *const AnyObject) {
    use objc2::runtime::AnyClass;
    let mgr_cls = AnyClass::get(c"NSAppleEventManager").unwrap();
    let mgr: *const AnyObject = msg_send![mgr_cls, sharedAppleEventManager];
    if mgr.is_null() {
        return;
    }
    // setEventHandler:andSelector:forEventClass:andEventID:
    // event class = kInternetEventClass = 'GURL' = 0x4755524C
    // event id = kAEGetURL = 'GURL'
    let internet_event_class: u32 = u32::from_be_bytes(*b"GURL");
    let ae_get_url: u32 = u32::from_be_bytes(*b"GURL");
    let sel = objc2::sel!(apple_event_get_url:withReplyEvent:);
    let _: () = msg_send![
        mgr,
        setEventHandler: delegate,
        andSelector: sel,
        forEventClass: internet_event_class,
        andEventID: ae_get_url,
    ];
}
