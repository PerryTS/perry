use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2::{AnyThread, MainThreadOnly};
use objc2_app_kit::{NSImage, NSImageView, NSView};
use objc2_foundation::{MainThreadMarker, NSString};

/// Extract a &str from a *const StringHeader pointer.
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

/// Create an NSImageView displaying a QR code for the given data string.
/// `size` is the display width/height in points (QR codes are square).
/// Returns widget handle, or 0 on failure.
pub fn create(data_ptr: *const u8, size: f64) -> i64 {
    let data_str = str_from_header(data_ptr);
    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    let display_size = if size > 0.0 { size } else { 200.0 };

    unsafe {
        let frame = objc2_core_foundation::CGRect::new(
            objc2_core_foundation::CGPoint::new(0.0, 0.0),
            objc2_core_foundation::CGSize::new(display_size, display_size),
        );
        let image_view: Retained<NSImageView> =
            msg_send![NSImageView::alloc(mtm), initWithFrame: frame];

        // NSImageScaleProportionallyUpOrDown = 3
        let _: () = msg_send![&*image_view, setImageScaling: 3_isize];

        if !data_str.is_empty() {
            if let Some(ns_image) = generate_qr_image(data_str, display_size) {
                let _: () = msg_send![&*image_view, setImage: &*ns_image];
            }
        }

        let view: Retained<NSView> = Retained::cast_unchecked(image_view);
        super::register_widget(view)
    }
}

/// Update the QR code content of an existing widget.
pub fn set_data(handle: i64, data_ptr: *const u8) {
    let data_str = str_from_header(data_ptr);
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            let frame: objc2_core_foundation::CGRect = msg_send![&*view, frame];
            let size = if frame.size.width > 0.0 {
                frame.size.width
            } else {
                200.0
            };
            if let Some(ns_image) = generate_qr_image(data_str, size) {
                let _: () = msg_send![&*view, setImage: &*ns_image];
            }
        }
    }
}

/// Generate an NSImage containing a QR code for the given text.
///
/// Uses CIFilter("CIQRCodeGenerator") → CIContext → CGImage → NSImage.
/// Integer scaling for crisp QR pixels.
unsafe fn generate_qr_image(text: &str, display_size: f64) -> Option<Retained<NSImage>> {
    if text.is_empty() {
        return None;
    }

    // 1. Text → NSData (UTF-8)
    let ns_str = NSString::from_str(text);
    let utf8_data: *mut AnyObject = msg_send![&*ns_str, dataUsingEncoding: 4_u64]; // NSUTF8StringEncoding
    if utf8_data.is_null() {
        return None;
    }

    // 2. CIFilter "CIQRCodeGenerator"
    let ci_filter_cls = AnyClass::get(c"CIFilter")?;
    let filter_name = NSString::from_str("CIQRCodeGenerator");
    let filter: *mut AnyObject = msg_send![ci_filter_cls, filterWithName: &*filter_name];
    if filter.is_null() {
        return None;
    }

    // Set inputMessage
    let key_msg = NSString::from_str("inputMessage");
    let _: () = msg_send![filter, setValue: utf8_data forKey: &*key_msg];

    // Set correction level to M (15% recovery)
    let key_corr = NSString::from_str("inputCorrectionLevel");
    let val_corr = NSString::from_str("M");
    let _: () = msg_send![filter, setValue: &*val_corr forKey: &*key_corr];

    // 3. Get output CIImage
    let key_output = NSString::from_str("outputImage");
    let ci_image: *mut AnyObject = msg_send![filter, valueForKey: &*key_output];
    if ci_image.is_null() {
        return None;
    }

    // 4. Get extent (raw QR pixel dimensions, e.g. 23x23)
    let extent: objc2_core_foundation::CGRect = msg_send![ci_image, extent];
    let qr_width = extent.size.width;
    if qr_width <= 0.0 {
        return None;
    }

    // 5. Create CIContext and render to CGImage
    let ci_context_cls = AnyClass::get(c"CIContext")?;
    let context: *mut AnyObject = msg_send![ci_context_cls, context];
    if context.is_null() {
        return None;
    }

    let cg_image: *mut AnyObject = msg_send![context, createCGImage: ci_image fromRect: extent];
    if cg_image.is_null() {
        return None;
    }

    // 6. Integer scale factor for crisp pixels
    let scale = ((display_size / qr_width).floor() as i64).max(1) as f64;
    let scaled_w = qr_width * scale;
    let scaled_h = extent.size.height * scale;
    let target_size = objc2_core_foundation::CGSize::new(scaled_w, scaled_h);

    // 7. Create NSImage from CGImage at the scaled size
    // Use raw alloc/init to avoid objc2 type constraints on initWithCGImage:
    let ns_image_cls = AnyClass::get(c"NSImage").unwrap();
    let ns_image_raw: *mut AnyObject = msg_send![ns_image_cls, alloc];
    let ns_image_raw: *mut AnyObject =
        msg_send![ns_image_raw, initWithCGImage: cg_image size: target_size];

    // Release the CGImage (we own it from createCGImage:fromRect:)
    extern "C" {
        fn CGImageRelease(image: *mut AnyObject);
    }
    CGImageRelease(cg_image);

    if ns_image_raw.is_null() {
        return None;
    }

    // Wrap in Retained
    let ns_image: Retained<NSImage> = Retained::retain(ns_image_raw as *mut NSImage)?;
    // Balance the retain (init already gives us +1)
    std::mem::forget(Retained::retain(ns_image_raw as *mut NSImage));

    Some(ns_image)
}
