use objc2::rc::Retained;
use objc2::msg_send;
use objc2::runtime::AnyClass;
use objc2_ui_kit::UIView;

/// Create a flexible spacer UIView with low content-hugging priority.
/// Works inside UIStackView with Fill distribution: the low hugging priority
/// and explicit zero-height constraint (at low priority) let the stack view
/// expand this view to fill remaining space.
pub fn create() -> i64 {
    unsafe {
        let view: Retained<UIView> = msg_send![
            AnyClass::get(c"UIView").unwrap(),
            new
        ];
        let _: () = msg_send![&*view, setTranslatesAutoresizingMaskIntoConstraints: false];
        // Set low content hugging priority so spacer stretches in both axes
        // UILayoutPriority is Float (f32) on iOS
        let _: () = msg_send![&*view, setContentHuggingPriority: 1.0f32, forAxis: 0i64]; // Horizontal
        let _: () = msg_send![&*view, setContentHuggingPriority: 1.0f32, forAxis: 1i64]; // Vertical
        // Set low compression resistance so spacer can be shrunk if needed
        let _: () = msg_send![&*view, setContentCompressionResistancePriority: 1.0f32, forAxis: 0i64];
        let _: () = msg_send![&*view, setContentCompressionResistancePriority: 1.0f32, forAxis: 1i64];

        super::register_widget(view)
    }
}
