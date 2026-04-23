use objc2::rc::Retained;
use objc2::msg_send;
use objc2::runtime::{AnyClass, AnyObject};
use objc2_ui_kit::UIView;
use objc2_foundation::NSString;

/// Extract a &str from a *const StringHeader pointer.
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

/// Create a UIImageView displaying a QR code for the given data string.
/// `size` is the display width/height in points (QR codes are square).
/// Returns widget handle, or 0 on failure.
pub fn create(data_ptr: *const u8, size: f64) -> i64 {
    let data_str = str_from_header(data_ptr);
    let display_size = if size > 0.0 { size } else { 200.0 };

    unsafe {
        let frame = objc2_core_foundation::CGRect::new(
            objc2_core_foundation::CGPoint::new(0.0, 0.0),
            objc2_core_foundation::CGSize::new(display_size, display_size),
        );
        let iv_cls = AnyClass::get(c"UIImageView").unwrap();
        let iv_raw: *mut AnyObject = msg_send![iv_cls, alloc];
        let iv_raw: *mut AnyObject = msg_send![iv_raw, initWithFrame: frame];
        if iv_raw.is_null() { return 0; }
        let image_view: Retained<UIView> = Retained::retain(iv_raw as *mut UIView).unwrap();

        // UIViewContentModeScaleAspectFit = 1
        let _: () = msg_send![&*image_view, setContentMode: 1_i64];
        let _: () = msg_send![&*image_view, setTranslatesAutoresizingMaskIntoConstraints: false];

        // Set width/height constraints
        let width_anchor: Retained<AnyObject> = msg_send![&*image_view, widthAnchor];
        let wc: Retained<AnyObject> = msg_send![&*width_anchor, constraintEqualToConstant: display_size];
        let _: () = msg_send![&*wc, setActive: true];

        let height_anchor: Retained<AnyObject> = msg_send![&*image_view, heightAnchor];
        let hc: Retained<AnyObject> = msg_send![&*height_anchor, constraintEqualToConstant: display_size];
        let _: () = msg_send![&*hc, setActive: true];

        if !data_str.is_empty() {
            if let Some(ui_image) = generate_qr_image(data_str, display_size) {
                let _: () = msg_send![&*image_view, setImage: &*ui_image];
            }
        }

        super::register_widget(image_view)
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
            if let Some(ui_image) = generate_qr_image(data_str, size) {
                let _: () = msg_send![&*view, setImage: &*ui_image];
            }
        }
    }
}

/// Generate a UIImage containing a QR code for the given text.
///
/// Uses CIFilter("CIQRCodeGenerator") → CIContext → CGImage → UIImage.
/// CoreImage is shared between macOS and iOS.
unsafe fn generate_qr_image(text: &str, display_size: f64) -> Option<Retained<AnyObject>> {
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
    let _: () = msg_send![filter, setValue: utf8_data, forKey: &*key_msg];

    // Set correction level to M (15% recovery)
    let key_corr = NSString::from_str("inputCorrectionLevel");
    let val_corr = NSString::from_str("M");
    let _: () = msg_send![filter, setValue: &*val_corr, forKey: &*key_corr];

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

    let cg_image: *mut AnyObject = msg_send![context, createCGImage: ci_image, fromRect: extent];
    if cg_image.is_null() {
        return None;
    }

    // 6. Integer scale factor for crisp pixels
    let scale = ((display_size / qr_width).floor() as i64).max(1) as f64;

    // 7. Create UIImage from CGImage
    // UIImage initWithCGImage:scale:orientation:
    // scale=1/scale_factor to render at the right size, orientation=0 (Up)
    let ui_image_cls = AnyClass::get(c"UIImage")?;
    let ui_image: Retained<AnyObject> = msg_send![
        ui_image_cls, imageWithCGImage: cg_image, scale: (1.0 / scale), orientation: 0_i64
    ];

    // Release the CGImage (we own it from createCGImage:fromRect:)
    extern "C" {
        fn CGImageRelease(image: *mut AnyObject);
    }
    CGImageRelease(cg_image);

    Some(ui_image)
}
