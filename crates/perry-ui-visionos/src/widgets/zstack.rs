use objc2::rc::Retained;
use objc2::msg_send;
use objc2::runtime::{AnyClass, AnyObject};
use objc2_ui_kit::UIView;
use objc2_foundation::MainThreadMarker;

pub fn create() -> i64 {
    let _mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    unsafe {
        let obj: *mut AnyObject = msg_send![AnyClass::get(c"UIView").unwrap(), new];
        let view: Retained<UIView> = Retained::retain(obj as *mut UIView).unwrap();
        super::register_widget(view)
    }
}

pub fn add_child(parent_handle: i64, child_handle: i64) {
    if let (Some(parent), Some(child)) = (super::get_widget(parent_handle), super::get_widget(child_handle)) {
        unsafe {
            let _: () = msg_send![&*parent, addSubview: &*child];
            let _: () = msg_send![&*child, setTranslatesAutoresizingMaskIntoConstraints: false];
            // Pin to fill parent
            let leading: *mut objc2::runtime::AnyObject = msg_send![&*child, leadingAnchor];
            let p_leading: *mut objc2::runtime::AnyObject = msg_send![&*parent, leadingAnchor];
            let c1: *mut objc2::runtime::AnyObject = msg_send![leading, constraintEqualToAnchor: p_leading];
            let _: () = msg_send![c1, setActive: true];

            let trailing: *mut objc2::runtime::AnyObject = msg_send![&*child, trailingAnchor];
            let p_trailing: *mut objc2::runtime::AnyObject = msg_send![&*parent, trailingAnchor];
            let c2: *mut objc2::runtime::AnyObject = msg_send![trailing, constraintEqualToAnchor: p_trailing];
            let _: () = msg_send![c2, setActive: true];

            let top: *mut objc2::runtime::AnyObject = msg_send![&*child, topAnchor];
            let p_top: *mut objc2::runtime::AnyObject = msg_send![&*parent, topAnchor];
            let c3: *mut objc2::runtime::AnyObject = msg_send![top, constraintEqualToAnchor: p_top];
            let _: () = msg_send![c3, setActive: true];

            let bottom: *mut objc2::runtime::AnyObject = msg_send![&*child, bottomAnchor];
            let p_bottom: *mut objc2::runtime::AnyObject = msg_send![&*parent, bottomAnchor];
            let c4: *mut objc2::runtime::AnyObject = msg_send![bottom, constraintEqualToAnchor: p_bottom];
            let _: () = msg_send![c4, setActive: true];
        }
    }
}
