use objc2::rc::Retained;
use objc2::msg_send;
use objc2_ui_kit::UIView;

/// Create a horizontal separator (1px UIView with separator color).
pub fn create() -> i64 {
    unsafe {
        let view: Retained<UIView> = msg_send![
            objc2::runtime::AnyClass::get(c"UIView").unwrap(),
            new
        ];
        let _: () = msg_send![&*view, setTranslatesAutoresizingMaskIntoConstraints: false];

        // Set separator color
        let color: Retained<objc2::runtime::AnyObject> = msg_send![
            objc2::runtime::AnyClass::get(c"UIColor").unwrap(),
            separatorColor
        ];
        let _: () = msg_send![&*view, setBackgroundColor: &*color];

        // Height constraint = 1 pixel
        let height_anchor: *const objc2::runtime::AnyObject = msg_send![&*view, heightAnchor];
        let constraint: Retained<objc2::runtime::AnyObject> = msg_send![
            height_anchor,
            constraintEqualToConstant: 1.0f64
        ];
        let _: () = msg_send![&*constraint, setActive: true];

        super::register_widget(view)
    }
}
