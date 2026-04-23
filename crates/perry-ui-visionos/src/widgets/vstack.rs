use objc2::encode::Encode;
use objc2::rc::Retained;
use objc2::msg_send;
use objc2_ui_kit::{UIStackView, UIView};

/// UIEdgeInsets struct matching UIKit layout.
#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct UIEdgeInsets {
    pub top: objc2_core_foundation::CGFloat,
    pub left: objc2_core_foundation::CGFloat,
    pub bottom: objc2_core_foundation::CGFloat,
    pub right: objc2_core_foundation::CGFloat,
}

// Safety: UIEdgeInsets is a simple C struct with 4 CGFloat fields
unsafe impl objc2::encode::Encode for UIEdgeInsets {
    const ENCODING: objc2::encode::Encoding = objc2::encode::Encoding::Struct(
        "UIEdgeInsets",
        &[
            objc2_core_foundation::CGFloat::ENCODING,
            objc2_core_foundation::CGFloat::ENCODING,
            objc2_core_foundation::CGFloat::ENCODING,
            objc2_core_foundation::CGFloat::ENCODING,
        ],
    );
}

unsafe impl objc2::encode::RefEncode for UIEdgeInsets {
    const ENCODING_REF: objc2::encode::Encoding = objc2::encode::Encoding::Pointer(
        &Self::ENCODING,
    );
}

/// Create a UIStackView with vertical axis (no default edge insets — matches macOS).
pub fn create(spacing: f64) -> i64 {
    unsafe {
        let stack: Retained<UIStackView> = msg_send![
            objc2::runtime::AnyClass::get(c"UIStackView").unwrap(),
            new
        ];
        let _: () = msg_send![&*stack, setAxis: 1i64]; // UILayoutConstraintAxisVertical = 1
        let _: () = msg_send![&*stack, setSpacing: spacing as objc2_core_foundation::CGFloat];
        let _: () = msg_send![&*stack, setAlignment: 0i64]; // UIStackViewAlignmentFill = 0
        let _: () = msg_send![&*stack, setDistribution: 0i64]; // UIStackViewDistributionFill = 0
        let _: () = msg_send![&*stack, setTranslatesAutoresizingMaskIntoConstraints: false];

        let view: Retained<UIView> = Retained::cast_unchecked(stack);
        super::register_widget(view)
    }
}

/// Create a UIStackView with vertical axis and custom edge insets.
pub fn create_with_insets(spacing: f64, top: f64, left: f64, bottom: f64, right: f64) -> i64 {
    unsafe {
        let stack: Retained<UIStackView> = msg_send![
            objc2::runtime::AnyClass::get(c"UIStackView").unwrap(),
            new
        ];
        let _: () = msg_send![&*stack, setAxis: 1i64];
        let _: () = msg_send![&*stack, setSpacing: spacing as objc2_core_foundation::CGFloat];
        let _: () = msg_send![&*stack, setAlignment: 0i64]; // UIStackViewAlignmentFill = 0
        let _: () = msg_send![&*stack, setTranslatesAutoresizingMaskIntoConstraints: false];

        let insets = UIEdgeInsets { top, left, bottom, right };
        let _: () = msg_send![&*stack, setLayoutMargins: insets];
        let _: () = msg_send![&*stack, setLayoutMarginsRelativeArrangement: true];
        let _: () = msg_send![&*stack, setInsetsLayoutMarginsFromSafeArea: false];

        let view: Retained<UIView> = Retained::cast_unchecked(stack);
        super::register_widget(view)
    }
}
