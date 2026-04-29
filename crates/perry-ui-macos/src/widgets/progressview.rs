use objc2::msg_send;
use objc2::rc::Retained;
use objc2::MainThreadOnly;
use objc2_app_kit::{NSProgressIndicator, NSView};
use objc2_foundation::MainThreadMarker;

/// Create a spinning NSProgressIndicator. Returns widget handle.
pub fn create() -> i64 {
    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    unsafe {
        let indicator: Retained<NSProgressIndicator> = msg_send![
            NSProgressIndicator::alloc(mtm), initWithFrame: objc2_core_foundation::CGRect::new(
                objc2_core_foundation::CGPoint::new(0.0, 0.0),
                objc2_core_foundation::CGSize::new(32.0, 32.0),
            )
        ];
        let _: () = msg_send![&*indicator, setStyle: 1i64]; // NSProgressIndicatorStyleSpinning
        let _: () = msg_send![&*indicator, setIndeterminate: true];
        let _: () =
            msg_send![&*indicator, startAnimation: std::ptr::null::<objc2::runtime::AnyObject>()];

        let view: Retained<NSView> = Retained::cast_unchecked(indicator);
        super::register_widget(view)
    }
}

/// Set the progress value (0.0 to 1.0). Switches to determinate mode.
pub fn set_value(handle: i64, value: f64) {
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            let _: () = msg_send![&*view, setIndeterminate: false];
            let _: () = msg_send![&*view, setDoubleValue: value * 100.0];
        }
    }
}

/// Set the label for a progress view.
/// NSProgressIndicator doesn't have a built-in label; this is a no-op on macOS.
/// In SwiftUI-like usage, the label would be a separate Text widget.
pub fn set_label(_handle: i64, _label_ptr: *const u8) {
    // no-op
}
