//! BloomView — a native render-surface host widget (issue #2395).
//!
//! Reserves a bare `UIView` in the Perry UI view tree for an external GPU
//! renderer (e.g. the Bloom engine) to draw into. Perry UI only owns the view
//! and exposes its pointer via `bloomViewGetNativeHandle`; user TypeScript hands
//! that pointer to the renderer, which builds its (Metal) surface on it. Mirrors
//! the Windows implementation, with the HWND replaced by the raw `UIView*`.

use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::AnyClass;
use objc2_foundation::MainThreadMarker;
use objc2_ui_kit::UIView;

/// Create a BloomView host sized `width` × `height` points. Returns the widget
/// handle (0 if called off the main thread). The size is pinned via Auto Layout
/// so the renderer's surface comes up non-zero (#5519).
pub fn create(width: f64, height: f64) -> i64 {
    // UIKit views must be created on the main thread; don't panic across the
    // FFI boundary if that contract is violated — return an invalid handle.
    let Some(_mtm) = MainThreadMarker::new() else {
        return 0;
    };
    unsafe {
        let view: Retained<UIView> = msg_send![AnyClass::get(c"UIView").unwrap(), new];
        // Auto Layout drives the size (set_width/set_height below).
        let _: () = msg_send![&view, setTranslatesAutoresizingMaskIntoConstraints: false];
        // Let the host view receive touches so the attached engine can route input.
        let _: () = msg_send![&view, setUserInteractionEnabled: true];
        let handle = super::register_widget(view);
        if width.is_finite() && width >= 1.0 {
            super::set_width(handle, width);
        }
        if height.is_finite() && height >= 1.0 {
            super::set_height(handle, height);
        }
        handle
    }
}

/// Return the raw `UIView*` for a BloomView handle as an integer, for handing
/// to an external GPU renderer. Returns 0 if the handle is unknown. The
/// registry retains the view; the returned pointer is non-owning.
pub fn get_native_handle(handle: i64) -> i64 {
    match super::get_widget(handle) {
        Some(view) => Retained::as_ptr(&view) as i64,
        None => 0,
    }
}
