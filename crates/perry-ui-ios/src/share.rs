//! System share sheet (#917) — `UIActivityViewController`.
//!
//! On iPad the sheet is a popover and UIKit throws unless it has an anchor,
//! so it is anchored to the bottom centre of the presenting view (no arrow).

use objc2::msg_send;
use objc2::runtime::{AnyClass, AnyObject};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::NSString;

extern "C" {
    static _dispatch_main_q: std::ffi::c_void;
    fn dispatch_async_f(
        queue: *const std::ffi::c_void,
        context: *mut std::ffi::c_void,
        work: unsafe extern "C" fn(*mut std::ffi::c_void),
    );
}

/// Share a URL (`is_url`) or plain text.
pub fn share(item: String, is_url: bool) {
    if item.is_empty() {
        return;
    }
    let pkg = Box::new((item, is_url));
    unsafe {
        dispatch_async_f(
            &_dispatch_main_q as *const _ as *const std::ffi::c_void,
            Box::into_raw(pkg) as *mut std::ffi::c_void,
            share_trampoline,
        );
    }
}

unsafe extern "C" fn share_trampoline(ctx: *mut std::ffi::c_void) {
    let _ = std::panic::catch_unwind(|| {
        let pkg = Box::from_raw(ctx as *mut (String, bool));
        let (item, is_url) = *pkg;
        present(&item, is_url);
    });
}

unsafe fn present(item: &str, is_url: bool) {
    let (Some(avc_cls), Some(array_cls)) = (
        AnyClass::get(c"UIActivityViewController"),
        AnyClass::get(c"NSArray"),
    ) else {
        return;
    };
    let vc = crate::widgets::alert::topmost_view_controller();
    if vc.is_null() {
        return;
    }
    let ns = NSString::from_str(item);
    let mut obj: *mut AnyObject = std::ptr::null_mut();
    if is_url {
        if let Some(url_cls) = AnyClass::get(c"NSURL") {
            obj = msg_send![url_cls, URLWithString: &*ns];
        }
    }
    let items: *mut AnyObject = if obj.is_null() {
        msg_send![array_cls, arrayWithObject: &*ns]
    } else {
        msg_send![array_cls, arrayWithObject: obj]
    };
    let sheet: *mut AnyObject = msg_send![avc_cls, alloc];
    let sheet: *mut AnyObject = msg_send![sheet, initWithActivityItems: items, applicationActivities: std::ptr::null_mut::<AnyObject>()];
    if sheet.is_null() {
        return;
    }
    let popover: *mut AnyObject = msg_send![sheet, popoverPresentationController];
    if !popover.is_null() {
        let view: *mut AnyObject = msg_send![vc, view];
        if !view.is_null() {
            let b: CGRect = msg_send![view, bounds];
            let _: () = msg_send![popover, setSourceView: view];
            let anchor = CGRect {
                origin: CGPoint {
                    x: b.size.width / 2.0,
                    y: b.size.height - 80.0,
                },
                size: CGSize {
                    width: 1.0,
                    height: 1.0,
                },
            };
            let _: () = msg_send![popover, setSourceRect: anchor];
            let _: () = msg_send![popover, setPermittedArrowDirections: 0usize];
        }
    }
    let _: () = msg_send![vc, presentViewController: sheet, animated: true, completion: std::ptr::null_mut::<AnyObject>()];
    objc2::ffi::objc_release(sheet.cast());
}
