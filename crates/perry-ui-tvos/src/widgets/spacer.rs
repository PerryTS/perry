use objc2::msg_send;
use objc2::rc::Retained;
use objc2_ui_kit::UIView;

/// Create a flexible spacer UIView with low content-hugging priority.
pub fn create() -> i64 {
    unsafe {
        let view: Retained<UIView> =
            msg_send![objc2::runtime::AnyClass::get(c"UIView").unwrap(), new];
        let _: () = msg_send![&*view, setTranslatesAutoresizingMaskIntoConstraints: false];
        // Set low content hugging priority so spacer stretches
        // UILayoutPriorityDefaultLow = 250
        let _: () = msg_send![&*view, setContentHuggingPriority: 1.0f32, forAxis: 1i64]; // Vertical
        let _: () = msg_send![&*view, setContentHuggingPriority: 1.0f32, forAxis: 0i64]; // Horizontal

        super::register_widget(view)
    }
}
