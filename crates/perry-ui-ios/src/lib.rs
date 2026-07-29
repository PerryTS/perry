#![cfg(target_os = "ios")]

pub mod app;
pub mod audio;
pub mod audio_playback;
pub mod background;
pub mod camera;
pub mod clipboard;
pub mod crash_log;
pub mod deeplinks;
pub mod drag_drop;
pub mod file_dialog;
pub mod geolocation;
pub mod image_picker;
pub mod keyboard;
pub mod location;
pub mod media_playback;
pub mod menu;
pub mod network;
pub mod notifications;
pub mod pointer;
pub mod screenshot;
pub mod state;
pub mod websocket;
pub mod widgets;

#[cfg(feature = "geisterhand")]
pub mod geisterhand_style;

// FFI exports — identical signatures to perry-ui-macos. Split into topical
// sub-modules for file-size hygiene (originally inline here, ~2,900 LOC).
mod ffi;

// Re-export the FFI entry points at the crate root so intra-crate callers that
// reference `crate::perry_ui_*` paths resolve — mirroring perry-ui-macos. The
// geisterhand glue (`app.rs` registration + `geisterhand_style.rs` dispatch)
// relies on this; without it the `geisterhand` feature build fails with E0425
// "cannot find ... in the crate root" (issue #1311). Linker-visible
// `#[no_mangle]` symbols are unaffected; this is purely Rust path resolution.
pub use ffi::camera::*;
pub use ffi::comms::*;
pub use ffi::dialogs_lifecycle::*;
pub use ffi::misc::*;
pub use ffi::security_notifications::*;
pub use ffi::system::*;
pub use ffi::widgets_advanced::*;
pub use ffi::widgets_basic::*;

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
