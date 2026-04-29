use objc2::rc::Retained;
use objc2_app_kit::{NSLayoutConstraint, NSView};
use objc2_foundation::MainThreadMarker;

/// Create a transparent NSView that stretches to fill available space.
/// Uses low content-hugging priority so stack views expand it.
pub fn create() -> i64 {
    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    let view = NSView::new(mtm);

    unsafe {
        // Low hugging priority = spacer stretches to fill
        view.setContentHuggingPriority_forOrientation(
            1.0,
            objc2_app_kit::NSLayoutConstraintOrientation::Vertical,
        );
        view.setContentHuggingPriority_forOrientation(
            1.0,
            objc2_app_kit::NSLayoutConstraintOrientation::Horizontal,
        );

        // Minimum height so it's not zero-sized when there's no pressure
        let height = NSLayoutConstraint::constraintWithItem_attribute_relatedBy_toItem_attribute_multiplier_constant(
            &view,
            objc2_app_kit::NSLayoutAttribute::Height,
            objc2_app_kit::NSLayoutRelation::GreaterThanOrEqual,
            None,
            objc2_app_kit::NSLayoutAttribute::NotAnAttribute,
            1.0,
            1.0,
        );
        view.addConstraint(&height);
    }

    super::register_widget(view)
}
