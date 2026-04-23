use objc2::rc::Retained;
use objc2::msg_send;
use objc2::runtime::AnyObject;
use objc2_ui_kit::UIView;
use objc2_foundation::{NSString, MainThreadMarker};

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

pub fn create_symbol(name_ptr: *const u8) -> i64 {
    let name = str_from_header(name_ptr);
    let _mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    unsafe {
        let ns_name = NSString::from_str(name);
        let image_cls = objc2::runtime::AnyClass::get(c"UIImage").unwrap();
        let image: *mut objc2::runtime::AnyObject = msg_send![image_cls, systemImageNamed: &*ns_name];
        let iv_cls = objc2::runtime::AnyClass::get(c"UIImageView").unwrap();
        let obj: *mut AnyObject = msg_send![iv_cls, alloc];
        let obj: *mut AnyObject = msg_send![obj, initWithImage: image];
        let image_view: Retained<UIView> = Retained::retain(obj as *mut UIView).unwrap();
        super::register_widget(image_view)
    }
}

pub fn create_file(path_ptr: *const u8) -> i64 {
    let path = str_from_header(path_ptr);
    let _mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    unsafe {
        // Resolve relative paths against the app bundle's resource directory
        let resolved = if !path.starts_with('/') {
            let bundle_cls = objc2::runtime::AnyClass::get(c"NSBundle").unwrap();
            let main_bundle: *mut AnyObject = msg_send![bundle_cls, mainBundle];
            let res_path: *mut AnyObject = msg_send![main_bundle, resourcePath];
            if !res_path.is_null() {
                let res_str: *const AnyObject = msg_send![res_path, UTF8String];
                let c_str = std::ffi::CStr::from_ptr(res_str as *const i8);
                format!("{}/{}", c_str.to_str().unwrap_or(""), path)
            } else {
                path.to_string()
            }
        } else {
            path.to_string()
        };
        let ns_path = NSString::from_str(&resolved);
        let image_cls = objc2::runtime::AnyClass::get(c"UIImage").unwrap();
        let image: *mut AnyObject = msg_send![image_cls, imageWithContentsOfFile: &*ns_path];
        if image.is_null() {
            eprintln!("[perry-ui-ios] ImageFile: failed to load image at path: {}", resolved);
        }
        let iv_cls = objc2::runtime::AnyClass::get(c"UIImageView").unwrap();
        let obj: *mut AnyObject = msg_send![iv_cls, alloc];
        let obj: *mut AnyObject = msg_send![obj, initWithImage: image];
        if obj.is_null() {
            // Image not found — create an empty UIImageView instead of crashing
            eprintln!("[perry-ui-ios] ImageFile: initWithImage returned nil for path: {}", resolved);
            let obj: *mut AnyObject = msg_send![iv_cls, new];
            let image_view: Retained<UIView> = Retained::retain(obj as *mut UIView).unwrap();
            return super::register_widget(image_view);
        }
        // Set content mode to ScaleAspectFit so the image scales properly with constraints
        let _: () = msg_send![obj, setContentMode: 1i64]; // UIViewContentModeScaleAspectFit
        let image_view: Retained<UIView> = Retained::retain(obj as *mut UIView).unwrap();
        super::register_widget(image_view)
    }
}

pub fn set_size(handle: i64, width: f64, height: f64) {
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            let frame = objc2_core_foundation::CGRect::new(
                objc2_core_foundation::CGPoint::new(0.0, 0.0),
                objc2_core_foundation::CGSize::new(width, height),
            );
            let _: () = msg_send![&*view, setFrame: frame];
        }
    }
}

pub fn set_tint(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            let color_cls = objc2::runtime::AnyClass::get(c"UIColor").unwrap();
            let color: *mut objc2::runtime::AnyObject = msg_send![
                color_cls, colorWithRed: r as f64, green: g as f64, blue: b as f64, alpha: a as f64
            ];
            let _: () = msg_send![&*view, setTintColor: color];
        }
    }
}
