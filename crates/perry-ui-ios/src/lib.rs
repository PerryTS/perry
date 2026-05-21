pub mod app;
pub mod audio;
pub mod background;
pub mod camera;
pub mod clipboard;
pub mod crash_log;
pub mod deeplinks;
pub mod file_dialog;
pub mod geolocation;
pub mod image_picker;
pub mod location;
pub mod media_playback;
pub mod menu;
pub mod network;
pub mod notifications;
pub mod screenshot;
pub mod state;
pub mod websocket;
pub mod widgets;

#[cfg(feature = "geisterhand")]
pub mod geisterhand_style;

// FFI exports — identical signatures to perry-ui-macos. Split into topical
// sub-modules for file-size hygiene (originally inline here, ~2,900 LOC).
mod ffi;

/// Debug logging macro that writes to a file (NSLog/eprintln don't work reliably on iOS)
#[macro_export]
macro_rules! ws_log {
    ($($arg:tt)*) => {{
        use std::io::Write;
        let msg = format!($($arg)*);
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open("/tmp/hone-ws-ios.log") {
            let _ = writeln!(f, "{}", msg);
        }
    }};
}

/// Run a closure, catching any Rust panics so they don't abort across the FFI boundary.
/// Clears the crash log since the panic was caught (non-fatal).
pub fn catch_callback_panic<F: FnOnce() + std::panic::UnwindSafe>(label: &str, f: F) {
    if let Err(e) = std::panic::catch_unwind(f) {
        crash_log::clear_crash_log();

        let msg = if let Some(s) = e.downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = e.downcast_ref::<String>() {
            s.clone()
        } else {
            format!("{:?}", e)
        };
        // Log to file since iOS eprintln is invisible
        ws_log!("[perry] panic in {} (caught): {}", label, msg);
    }
}
