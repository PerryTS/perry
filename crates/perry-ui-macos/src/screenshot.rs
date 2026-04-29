//! Screenshot capture for macOS (behind geisterhand feature).

use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};

/// Capture the main application window as PNG bytes.
/// Returns a malloc'd buffer (caller frees with libc::free). Sets *out_len to byte count.
/// Returns null on failure.
#[no_mangle]
pub extern "C" fn perry_ui_screenshot_capture(out_len: *mut usize) -> *mut u8 {
    unsafe {
        *out_len = 0;
    }

    // Get the main window's windowNumber for CGWindowListCreateImage
    // Check APPS first (main app window), then WINDOWS (multi-window)
    let window_id: u32 = crate::app::APPS.with(|a| {
        let apps = a.borrow();
        if !apps.is_empty() {
            let num: isize = unsafe { msg_send![&*apps[0].window, windowNumber] };
            return num as u32;
        }
        crate::app::WINDOWS.with(|w| {
            let windows = w.borrow();
            if windows.is_empty() {
                return 0u32;
            }
            let num: isize = unsafe { msg_send![&*windows[0].window, windowNumber] };
            num as u32
        })
    });

    if window_id == 0 {
        return std::ptr::null_mut();
    }

    unsafe {
        // CGRectNull — capture the window's bounds automatically
        #[repr(C)]
        #[derive(Copy, Clone)]
        struct CGPoint {
            x: f64,
            y: f64,
        }
        #[repr(C)]
        #[derive(Copy, Clone)]
        struct CGSize {
            width: f64,
            height: f64,
        }
        #[repr(C)]
        #[derive(Copy, Clone)]
        struct CGRect {
            origin: CGPoint,
            size: CGSize,
        }

        let cg_rect_null = CGRect {
            origin: CGPoint { x: 0.0, y: 0.0 },
            size: CGSize {
                width: 0.0,
                height: 0.0,
            },
        };

        extern "C" {
            fn CGWindowListCreateImage(
                screenBounds: CGRect,
                listOption: u32,
                windowID: u32,
                imageOption: u32,
            ) -> *mut AnyObject; // CGImageRef
            fn CGImageRelease(image: *mut AnyObject);
        }

        // kCGWindowListOptionIncludingWindow = 1 << 3 = 8
        // kCGWindowImageBoundsIgnoreFraming = 1 << 0 = 1
        let cg_image = CGWindowListCreateImage(
            cg_rect_null,
            8, // kCGWindowListOptionIncludingWindow
            window_id,
            1, // kCGWindowImageBoundsIgnoreFraming
        );

        if cg_image.is_null() {
            return std::ptr::null_mut();
        }

        // Create NSBitmapImageRep from CGImage
        let bitmap_cls = AnyClass::get(c"NSBitmapImageRep").unwrap();
        let bitmap: *mut AnyObject = msg_send![bitmap_cls, alloc];
        let bitmap: *mut AnyObject = msg_send![bitmap, initWithCGImage: cg_image];

        if bitmap.is_null() {
            CGImageRelease(cg_image);
            return std::ptr::null_mut();
        }

        // Convert to PNG: representationUsingType:properties:
        // NSBitmapImageFileType.png = 4
        let empty_dict_cls = AnyClass::get(c"NSDictionary").unwrap();
        let empty_dict: Retained<AnyObject> = msg_send![empty_dict_cls, dictionary];
        let png_data: *const AnyObject =
            msg_send![bitmap, representationUsingType: 4_usize, properties: &*empty_dict];

        if png_data.is_null() {
            CGImageRelease(cg_image);
            return std::ptr::null_mut();
        }

        // Get bytes from NSData
        let length: usize = msg_send![png_data, length];
        let bytes: *const u8 = msg_send![png_data, bytes];

        if length == 0 || bytes.is_null() {
            CGImageRelease(cg_image);
            return std::ptr::null_mut();
        }

        // Copy to malloc'd buffer (NSData/CGImage will be autoreleased)
        let buf = libc::malloc(length) as *mut u8;
        if buf.is_null() {
            CGImageRelease(cg_image);
            return std::ptr::null_mut();
        }
        std::ptr::copy_nonoverlapping(bytes, buf, length);
        *out_len = length;

        // Release CGImage (NSBitmapImageRep and NSData are autoreleased)
        CGImageRelease(cg_image);

        buf
    }
}
