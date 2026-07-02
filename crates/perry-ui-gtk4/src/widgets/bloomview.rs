//! BloomView — a native render-surface host widget (issue #2395).
//!
//! Reserves a `GtkDrawingArea` in the Perry UI view tree for an external GPU
//! renderer (e.g. the Bloom engine) to draw into. Perry UI only owns the widget
//! and exposes its `GtkWidget*` via `bloomViewGetNativeHandle`; user TypeScript
//! hands that to the renderer, which targets the widget's surface (the issue's
//! MVP used GTK4 + Vulkan dmabuf). Mirrors the Windows implementation, with the
//! HWND replaced by the raw `GtkWidget*`.

use gtk4::prelude::*;

/// Create a BloomView host. Reserves the requested size, or expands to fill if
/// none is given. Returns the widget handle.
pub fn create(width: f64, height: f64) -> i64 {
    crate::app::ensure_gtk_init();
    let area = gtk4::DrawingArea::new();
    // Only honor finite, sensibly-bounded sizes — NaN/inf or sub-pixel
    // fractions would produce a bogus GTK size request. Otherwise expand.
    if width.is_finite() && height.is_finite() && width >= 1.0 && height >= 1.0 {
        let w = (width as i32).clamp(1, 16384);
        let h = (height as i32).clamp(1, 16384);
        area.set_size_request(w, h);
    } else {
        area.set_hexpand(true);
        area.set_vexpand(true);
    }
    // Let the host widget take keyboard focus + pointer events so the attached
    // engine can route input (#5519).
    area.set_focusable(true);
    area.set_can_target(true);
    super::register_widget(area.upcast())
}

/// Return the raw `GtkWidget*` for a BloomView handle as an integer, for handing
/// to an external GPU renderer. Returns 0 if the handle is unknown.
pub fn get_native_handle(handle: i64) -> i64 {
    use gtk4::glib::translate::ToGlibPtr;
    match super::get_widget(handle) {
        Some(w) => {
            let ptr: *mut gtk4::ffi::GtkWidget = w.to_glib_none().0;
            ptr as i64
        }
        None => 0,
    }
}
