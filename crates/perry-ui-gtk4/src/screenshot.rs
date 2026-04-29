//! Screenshot capture for GTK4 (behind geisterhand feature).
//!
//! Captures the main ApplicationWindow as PNG using GTK4's WidgetPaintable +
//! GskRenderer.render_texture + GdkTexture.save_to_png_bytes pipeline.

use gtk4::prelude::*;

/// Capture the main application window as PNG bytes.
/// Returns a malloc'd buffer (caller frees with libc::free). Sets *out_len to byte count.
/// Returns null on failure.
#[no_mangle]
pub extern "C" fn perry_ui_screenshot_capture(out_len: *mut usize) -> *mut u8 {
    unsafe {
        *out_len = 0;
    }

    // Get the stored application window
    let window = crate::app::APP_WINDOW.with(|aw| aw.borrow().clone());

    let window = match window {
        Some(w) => w,
        None => return std::ptr::null_mut(),
    };

    let width = window.width();
    let height = window.height();
    if width <= 0 || height <= 0 {
        return std::ptr::null_mut();
    }

    // Use WidgetPaintable to capture the window content
    let paintable = gtk4::WidgetPaintable::new(Some(&window));

    // Create a snapshot and render the paintable into it
    let snapshot = gtk4::Snapshot::new();
    paintable.snapshot(&snapshot, width as f64, height as f64);

    // Convert snapshot to a render node
    let node = match snapshot.to_node() {
        Some(n) => n,
        None => return std::ptr::null_mut(),
    };

    // Get the renderer from the window's native surface
    let renderer = match window.renderer() {
        Some(r) => r,
        None => return std::ptr::null_mut(),
    };

    // Render the node to a texture
    let texture = renderer.render_texture(&node, None);

    // Save texture to PNG bytes (requires gtk4 v4_6 feature)
    let bytes = texture.save_to_png_bytes();
    let data: &[u8] = &bytes;

    if data.is_empty() {
        return std::ptr::null_mut();
    }

    // Copy to malloc'd buffer
    let len = data.len();
    let buf = unsafe { libc::malloc(len) as *mut u8 };
    if buf.is_null() {
        return std::ptr::null_mut();
    }
    unsafe {
        std::ptr::copy_nonoverlapping(data.as_ptr(), buf, len);
        *out_len = len;
    }
    buf
}
