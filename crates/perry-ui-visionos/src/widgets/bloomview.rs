//! BloomView — a native render-surface host widget (issue #2395).
//!
//! Reserves a bare `UIView` in the Perry UI view tree for an external GPU
//! renderer (e.g. the Bloom engine) to draw into. Perry UI only owns the view
//! and exposes its pointer via `bloomViewGetHwnd`; user TypeScript hands that
//! pointer to the renderer, which builds its (Metal) surface on it. Mirrors the
//! Windows implementation, with the HWND replaced by the raw `UIView*`.

use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::AnyClass;
use objc2_foundation::MainThreadMarker;
use objc2_ui_kit::UIView;

/// Create a BloomView host. `width`/`height` are advisory — the layout engine
/// sizes the view. Returns the widget handle (0 if called off the main thread).
pub fn create(width: f64, height: f64) -> i64 {
    let _ = (width, height);
    // UIKit views must be created on the main thread; don't panic across the
    // FFI boundary if that contract is violated — return an invalid handle.
    let Some(_mtm) = MainThreadMarker::new() else {
        return 0;
    };
    unsafe {
        let view: Retained<UIView> = msg_send![AnyClass::get(c"UIView").unwrap(), new];
        super::register_widget(view)
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
