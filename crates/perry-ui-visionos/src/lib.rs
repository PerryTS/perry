#![cfg(target_os = "visionos")]

pub mod app;
pub mod audio;
pub mod audio_playback;
pub mod background;
pub mod camera;
pub mod clipboard;
pub mod crash_log;
pub mod deeplinks_stub;
pub mod drag_drop;
pub mod file_dialog;
#[cfg(feature = "geisterhand")]
pub mod geisterhand_style;
pub mod issue_552_stub;
pub mod location;
pub mod media_playback;
pub mod menu;
pub mod network_stub;
pub mod pointer;
pub mod screenshot;
pub mod state;
pub mod websocket;
pub mod widgets;

// FFI surface — same `#[no_mangle] perry_ui_*` / `perry_system_*` /
// `perry_media_*` / `hone_*` entry points as the pre-split lib.rs, just
// regrouped by topic so no single file balloons past ~2k LOC. Each
// sub-module starts with `use super::*;` so module references resolve
// against this root.
mod ffi_canvas;
mod ffi_core;
mod ffi_cross;
mod ffi_focus_menu;
mod ffi_hone;
mod ffi_keychain;
mod ffi_layout;
mod ffi_media;
mod ffi_misc;
mod ffi_notifications;
mod ffi_system;
mod ffi_widgets_extra;

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
