use objc2::msg_send;
use objc2::rc::Retained;
use objc2_app_kit::{
    NSLayoutAttribute, NSStackView, NSStackViewGravity, NSUserInterfaceLayoutOrientation, NSView,
};
use objc2_foundation::MainThreadMarker;

/// Set distribution to Fill (0) so children fill available space based on
/// their content hugging priorities. Children with low hugging priority (1)
/// stretch to fill remaining space; children with high hugging priority
/// (750+) stay at their intrinsic size. This matches the behavior expected
/// by Hone IDE's layout where `widgetSetHugging(mainRow, 1)` means "stretch
/// to fill". The previous GravityAreas (-1) distribution packed children
/// tightly from the top without stretching, causing the workbench to occupy
/// only ~40% of the window height.
fn set_gravity_distribution(stack: &NSStackView) {
    unsafe {
        let _: () = msg_send![stack, setDistribution: 0i64]; // NSStackViewDistributionFill
    }
}

/// Create an NSStackView with vertical orientation (no default edge insets).
pub fn create(spacing: f64) -> i64 {
    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    let stack = NSStackView::new(mtm);
    stack.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
    stack.setSpacing(spacing);
    // Width alignment: children stretch to fill the full cross-axis width
    stack.setAlignment(NSLayoutAttribute::Leading);
    set_gravity_distribution(&stack);
    let view: Retained<NSView> = unsafe { Retained::cast_unchecked(stack) };
    super::register_widget(view)
}

/// Create an NSStackView with vertical orientation and custom edge insets.
pub fn create_with_insets(spacing: f64, top: f64, left: f64, bottom: f64, right: f64) -> i64 {
    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    let stack = NSStackView::new(mtm);
    stack.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
    stack.setSpacing(spacing);
    stack.setAlignment(NSLayoutAttribute::Leading);
    set_gravity_distribution(&stack);
    unsafe {
        stack.setEdgeInsets(objc2_foundation::NSEdgeInsets {
            top,
            left,
            bottom,
            right,
        });
    }
    let view: Retained<NSView> = unsafe { Retained::cast_unchecked(stack) };
    super::register_widget(view)
}
