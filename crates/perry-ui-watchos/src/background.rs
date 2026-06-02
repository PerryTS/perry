//! Background tasks (issue #538) — watchOS implementation.
//!
//! watchOS uses `WKApplication.scheduleBackgroundRefresh(withPreferredDate:userInfo:scheduledCompletion:)`
//! (watchOS 7+) to enqueue a refresh request. Dispatch happens through
//! `WKApplicationDelegate.handle(_ backgroundTasks: Set<WKRefreshBackgroundTask>)`
//! which the SwiftUI host (PerryWatchApp.swift) wires to
//! `perry_watchos_dispatch_background_task(identifier_cstr)`.
//!
//! API differences from iOS / Android:
//! - Only the `appRefresh` kind has a watchOS equivalent. `processing` is
//!   not exposed by WatchKit; we accept the value but treat it the same.
//! - `requiresNetwork` / `requiresCharging` aren't expressible at
//!   schedule time on watchOS — the OS makes its own decision based on
//!   battery / connectivity. We accept the values but they're advisory.
//! - There is no equivalent of `cancelTaskRequestWithIdentifier` on
//!   watchOS. `cancel` removes our handler from the registry so a fired
//!   refresh becomes a no-op; the OS-side schedule still runs.

use objc2::class;
use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2_foundation::NSString;
use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::CStr;

extern "C" {
    fn js_run_stdlib_pump();
    fn js_promise_run_microtasks() -> i32;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_closure_call0(closure: *const u8) -> f64;
    fn js_value_is_promise(value: f64) -> i32;
}

const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;

thread_local! {
    static HANDLERS: RefCell<HashMap<String, f64>> = RefCell::new(HashMap::new());
}

fn str_from_header(ptr: *const u8) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe {
        let header = ptr as *const perry_runtime::string::StringHeader;
        let len = (*header).byte_len as usize;
        let data = ptr.add(std::mem::size_of::<perry_runtime::string::StringHeader>());
        String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned()
    }
}

unsafe fn invoke_handler(handler: f64) {
    if handler.to_bits() == TAG_UNDEFINED {
        return;
    }
    js_run_stdlib_pump();
    js_promise_run_microtasks();
    let ptr = js_nanbox_get_pointer(handler) as *const u8;
    let result = js_closure_call0(ptr);
    js_promise_run_microtasks();
    let _ = js_value_is_promise(result);
}

pub fn register_task(identifier_ptr: *const u8, handler: f64) {
    let id = str_from_header(identifier_ptr);
    if id.is_empty() {
        return;
    }
    HANDLERS.with(|h| {
        h.borrow_mut().insert(id, handler);
    });
}

pub fn schedule(
    identifier_ptr: *const u8,
    _kind_ptr: *const u8,
    earliest_start_ms: f64,
    _requires_network: f64,
    _requires_charging: f64,
) {
    let id = str_from_header(identifier_ptr);
    if id.is_empty() {
        return;
    }

    unsafe {
        // WKApplication.shared() — available watchOS 7+. Older watchOS
        // would need WKExtension; not supported here.
        let app_cls = match AnyClass::get(c"WKApplication") {
            Some(c) => c,
            None => return,
        };
        let app: *mut AnyObject = msg_send![app_cls, sharedApplication];

        let secs = if earliest_start_ms > 0.0 && earliest_start_ms.is_finite() {
            earliest_start_ms / 1000.0
        } else {
            // Default to 30 minutes in the future — sane minimum for an
            // unspecified earliestStart since watchOS rejects <= now.
            (std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs_f64())
                .unwrap_or(0.0))
                + 1800.0
        };
        let date: *mut AnyObject = msg_send![class!(NSDate), dateWithTimeIntervalSince1970: secs];

        // Build userInfo = @{ @"perry_id": <id> }. NSDictionary with NSString
        // keys + values conforms to NSSecureCoding so WKApplication accepts it.
        let key: Retained<NSString> = NSString::from_str("perry_id");
        let value: Retained<NSString> = NSString::from_str(&id);
        let user_info: *mut AnyObject = msg_send![
            class!(NSDictionary),
            dictionaryWithObject: &*value,
            forKey: &*key
        ];

        // scheduledCompletion block — required, but we have nothing to do.
        let completion = block2::RcBlock::new(|_err: *mut AnyObject| {});

        let _: () = msg_send![
            app,
            scheduleBackgroundRefreshWithPreferredDate: date,
            userInfo: user_info,
            scheduledCompletion: &*completion
        ];
    }
}

pub fn cancel(identifier_ptr: *const u8) {
    let id = str_from_header(identifier_ptr);
    if id.is_empty() {
        return;
    }
    // No native cancel on watchOS. Drop the handler so the next OS-fired
    // refresh becomes a no-op.
    HANDLERS.with(|h| {
        h.borrow_mut().remove(&id);
    });
}

/// Called from PerryWatchApp.swift's `WKApplicationDelegate.handle(_:)`
/// when a `WKRefreshBackgroundTask` is delivered. The Swift side reads
/// the identifier out of the task's userInfo dictionary and forwards
/// the C-string here.
#[no_mangle]
pub extern "C" fn perry_watchos_dispatch_background_task(identifier_cstr: *const i8) {
    if identifier_cstr.is_null() {
        return;
    }
    let id = unsafe { CStr::from_ptr(identifier_cstr) }
        .to_string_lossy()
        .into_owned();
    let handler = HANDLERS.with(|h| h.borrow().get(&id).copied());
    if let Some(h) = handler {
        unsafe {
            invoke_handler(h);
        }
    }
}
