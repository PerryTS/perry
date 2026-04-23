//! Screenshot capture for iOS (behind geisterhand feature).

use objc2::runtime::{AnyClass, AnyObject};
use objc2::msg_send;
use objc2_core_foundation::{CGRect, CGSize};

#[no_mangle]
pub extern "C" fn perry_ui_screenshot_capture(out_len: *mut usize) -> *mut u8 {
    unsafe {
        *out_len = 0;
    }

    unsafe {
        // Get the shared UIApplication and its key window
        let app_cls = AnyClass::get(c"UIApplication").unwrap();
        let app: *const AnyObject = msg_send![app_cls, sharedApplication];
        if app.is_null() {
            return std::ptr::null_mut();
        }

        let key_window: *const AnyObject = msg_send![app, keyWindow];
        if key_window.is_null() {
            return std::ptr::null_mut();
        }

        // Get window bounds
        let bounds: CGRect = msg_send![key_window, bounds];
        let size: CGSize = bounds.size;

        // UIGraphicsBeginImageContextWithOptions(bounds.size, NO, 0.0)
        extern "C" {
            fn UIGraphicsBeginImageContextWithOptions(size: CGSize, opaque: bool, scale: f64);
            fn UIGraphicsGetImageFromCurrentImageContext() -> *const AnyObject;
            fn UIGraphicsEndImageContext();
            fn UIImagePNGRepresentation(image: *const AnyObject) -> *const AnyObject;
        }

        UIGraphicsBeginImageContextWithOptions(size, false, 0.0);

        // drawViewHierarchyInRect:afterScreenUpdates:
        let _: bool = msg_send![key_window, drawViewHierarchyInRect: bounds afterScreenUpdates: true];

        let image = UIGraphicsGetImageFromCurrentImageContext();
        UIGraphicsEndImageContext();

        if image.is_null() {
            return std::ptr::null_mut();
        }

        let png_data = UIImagePNGRepresentation(image);
        if png_data.is_null() {
            return std::ptr::null_mut();
        }

        // NSData bytes/length
        let length: usize = msg_send![png_data, length];
        let bytes: *const u8 = msg_send![png_data, bytes];

        if length == 0 || bytes.is_null() {
            return std::ptr::null_mut();
        }

        let buf = libc::malloc(length) as *mut u8;
        if buf.is_null() {
            return std::ptr::null_mut();
        }
        std::ptr::copy_nonoverlapping(bytes, buf, length);
        *out_len = length;
        buf
    }
}
