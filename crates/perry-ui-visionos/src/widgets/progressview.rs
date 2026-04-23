use objc2::rc::Retained;
use objc2::msg_send;
use objc2::runtime::AnyObject;
use objc2_ui_kit::UIView;
use objc2_foundation::MainThreadMarker;

pub fn create() -> i64 {
    let _mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    unsafe {
        let cls = objc2::runtime::AnyClass::get(c"UIActivityIndicatorView").unwrap();
        let obj: *mut AnyObject = msg_send![cls, alloc];
        let obj: *mut AnyObject = msg_send![obj, initWithActivityIndicatorStyle: 100i64]; // UIActivityIndicatorViewStyleMedium
        let indicator: Retained<UIView> = Retained::retain(obj as *mut UIView).unwrap();
        let _: () = msg_send![&*indicator, startAnimating];
        super::register_widget(indicator)
    }
}

pub fn set_value(handle: i64, value: f64) {
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            // Switch to UIProgressView for determinate mode
            let _: () = msg_send![&*view, setProgress: value as f32, animated: true];
        }
    }
}

pub fn set_label(_handle: i64, _label_ptr: *const u8) {
    // No-op on iOS
}
