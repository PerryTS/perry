//! BloomView — a native render-surface host widget (issue #2395).
//!
//! Reserves a bare `NSView` in the Perry UI view tree for an external GPU
//! renderer (e.g. the Bloom engine) to draw into. Perry UI only owns the view
//! and exposes its pointer via `bloomViewGetHwnd`; user TypeScript hands that
//! pointer to the renderer, which builds its (Metal) surface on it. Mirrors the
//! Windows implementation (`perry-ui-windows`), with the HWND replaced by the
//! raw `NSView*`.

use objc2_app_kit::NSView;
use objc2_foundation::MainThreadMarker;

/// Create a BloomView host. `width`/`height` are advisory — the layout engine
/// sizes the view. Returns the widget handle.
pub fn create(width: f64, height: f64) -> i64 {
    let _ = (width, height);
    // Public C ABI entry — don't panic across the FFI boundary if called off
    // the main thread; return an invalid (0) handle instead.
    let Some(mtm) = MainThreadMarker::new() else {
        return 0;
    };
    let view = NSView::new(mtm);
    super::register_widget(view)
}

/// Return the raw `NSView*` for a BloomView handle as an integer, for handing
/// to an external GPU renderer. Returns 0 if the handle is unknown. The
/// registry retains the view; the returned pointer is non-owning.
pub fn get_native_handle(handle: i64) -> i64 {
    match super::get_widget(handle) {
        Some(view) => objc2::rc::Retained::as_ptr(&view) as i64,
        None => 0,
    }
}
