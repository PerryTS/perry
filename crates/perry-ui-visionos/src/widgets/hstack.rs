use objc2::rc::Retained;
use objc2::msg_send;
use objc2_ui_kit::{UIStackView, UIView};

/// Create a UIStackView with horizontal axis.
pub fn create(spacing: f64) -> i64 {
    unsafe {
        let stack: Retained<UIStackView> = msg_send![
            objc2::runtime::AnyClass::get(c"UIStackView").unwrap(),
            new
        ];
        let _: () = msg_send![&*stack, setAxis: 0i64]; // UILayoutConstraintAxisHorizontal = 0
        let _: () = msg_send![&*stack, setSpacing: spacing as objc2_core_foundation::CGFloat];
        let _: () = msg_send![&*stack, setAlignment: 3i64]; // UIStackViewAlignmentCenter = 3 (CenterY)
        let _: () = msg_send![&*stack, setDistribution: 0i64]; // UIStackViewDistributionFill = 0
        let _: () = msg_send![&*stack, setTranslatesAutoresizingMaskIntoConstraints: false];

        let view: Retained<UIView> = Retained::cast_unchecked(stack);
        super::register_widget(view)
    }
}

/// Create a UIStackView with horizontal axis and custom edge insets.
pub fn create_with_insets(spacing: f64, top: f64, left: f64, bottom: f64, right: f64) -> i64 {
    unsafe {
        let stack: Retained<UIStackView> = msg_send![
            objc2::runtime::AnyClass::get(c"UIStackView").unwrap(),
            new
        ];
        let _: () = msg_send![&*stack, setAxis: 0i64];
        let _: () = msg_send![&*stack, setSpacing: spacing as objc2_core_foundation::CGFloat];
        let _: () = msg_send![&*stack, setAlignment: 3i64];
        let _: () = msg_send![&*stack, setDistribution: 0i64]; // UIStackViewDistributionFill = 0
        let _: () = msg_send![&*stack, setTranslatesAutoresizingMaskIntoConstraints: false];

        let insets = super::vstack::UIEdgeInsets { top, left, bottom, right };
        let _: () = msg_send![&*stack, setLayoutMargins: insets];
        let _: () = msg_send![&*stack, setLayoutMarginsRelativeArrangement: true];
        let _: () = msg_send![&*stack, setInsetsLayoutMarginsFromSafeArea: false];

        let view: Retained<UIView> = Retained::cast_unchecked(stack);
        super::register_widget(view)
    }
}
