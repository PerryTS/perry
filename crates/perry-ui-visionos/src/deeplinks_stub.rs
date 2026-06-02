//! Deep links stubs (issue #583).
//!
//! tvOS / visionOS / watchOS / GTK4 / Windows don't have a unified
//! "URL handed to a running app" surface comparable to the iOS / macOS
//! / Android pipeline. tvOS does support `application(_:open:)` but
//! Apple TV apps rarely surface as URL targets in practice; the other
//! platforms simply don't route URLs from outside the process.
//!
//! These stubs register the handler (so user code that calls
//! `appOnOpenUrl` doesn't crash) but never invoke it, and
//! `appGetLaunchUrl` always returns the empty string. Apps that need
//! true deep-link routing should target iOS / macOS / Android.

use std::cell::RefCell;

extern "C" {
    fn js_string_from_bytes(ptr: *const u8, len: u32) -> *mut u8;
}

thread_local! {
    static HANDLER: RefCell<Option<f64>> = const { RefCell::new(None) };
}

#[no_mangle]
pub extern "C" fn perry_system_app_on_open_url(callback: f64) {
    HANDLER.with(|h| *h.borrow_mut() = Some(callback));
}

#[no_mangle]
pub extern "C" fn perry_system_app_get_launch_url() -> i64 {
    let bytes: &[u8] = b"";
    unsafe { js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32) as i64 }
}
