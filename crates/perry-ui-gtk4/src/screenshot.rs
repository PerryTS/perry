//! Screenshot capture for GTK4 (behind geisterhand feature).

use gtk4::prelude::*;

/// Capture the main application window as PNG bytes.
/// Returns a malloc'd buffer (caller frees with libc::free). Sets *out_len to byte count.
/// Returns null on failure.
#[no_mangle]
pub extern "C" fn perry_ui_screenshot_capture(out_len: *mut usize) -> *mut u8 {
    unsafe {
        *out_len = 0;
    }

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

    let paintable = gtk4::WidgetPaintable::new(Some(&window));

    let snapshot = gtk4::Snapshot::new();
    paintable.snapshot(&snapshot, width as f64, height as f64);

    let node = match snapshot.to_node() {
        Some(n) => n,
        None => return std::ptr::null_mut(),
    };

    let renderer = match window.renderer() {
        Some(r) => r,
        None => return std::ptr::null_mut(),
    };

    let texture = renderer.render_texture(&node, None);

    let bytes = texture.save_to_png_bytes();
    let data: &[u8] = &bytes;

    if data.is_empty() {
        return std::ptr::null_mut();
    }

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
