//! BloomView — a native render-surface host widget (issue #2395).
//!
//! Reserves a `GtkDrawingArea` in the Perry UI view tree for an external GPU
//! renderer (e.g. the Bloom engine) to draw into. Perry UI only owns the widget
//! and exposes its `GtkWidget*` via `bloomViewGetHwnd`; user TypeScript hands
//! that to the renderer, which targets the widget's surface (the issue's MVP
//! used GTK4 + Vulkan dmabuf). Mirrors the Windows implementation, with the
//! HWND replaced by the raw `GtkWidget*`.

use gtk4::prelude::*;

/// Create a BloomView host. Reserves the requested size, or expands to fill if
/// none is given. Returns the widget handle.
pub fn create(width: f64, height: f64) -> i64 {
    crate::app::ensure_gtk_init();
    let area = gtk4::DrawingArea::new();
    if width > 0.0 && height > 0.0 {
        area.set_size_request(width as i32, height as i32);
    } else {
        area.set_hexpand(true);
        area.set_vexpand(true);
    }
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
