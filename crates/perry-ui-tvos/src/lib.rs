#![cfg(target_os = "tvos")]

pub mod app;
pub mod audio;
pub mod audio_playback;
pub mod background;
pub mod clipboard;
pub mod crash_log;
pub mod deeplinks_stub;
pub mod drag_drop;
pub mod file_dialog;
pub mod issue_552_stub;
pub mod keyboard;
pub mod location;
pub mod media_playback;
pub mod menu;
pub mod network_stub;
pub mod pointer;
pub mod screenshot;
pub mod state;
pub mod websocket;
pub mod widgets;

#[cfg(feature = "geisterhand")]
pub mod geisterhand_style;

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

// =============================================================================
// FFI exports — identical signatures to perry-ui-macos
//
// The bulk of the FFI surface is split topically under `ffi/`; only the few
// thunks that don't belong to any one widget group are inlined below. Every
// `#[no_mangle] pub extern "C" fn perry_ui_*` signature is preserved exactly
// — the split is purely organizational, no behavior changes.
// =============================================================================

pub mod ffi;

#[no_mangle]
pub extern "C" fn perry_ui_app_create(title_ptr: i64, width: f64, height: f64) -> i64 {
    app::app_create(title_ptr as *const u8, width, height)
}

#[no_mangle]
pub extern "C" fn perry_ui_app_set_body(app_handle: i64, root_handle: i64) {
    app::app_set_body(app_handle, root_handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_app_run(app_handle: i64) {
    app::app_run(app_handle);
}

/// Register an external UIView (from a native library) as a Perry widget.
/// Alias of perry_ui_embed_nsview so cross-platform Perry code works unchanged on iOS.
#[no_mangle]
pub extern "C" fn perry_ui_embed_nsview(uiview_ptr: i64) -> i64 {
    use objc2::rc::Retained;
    use objc2_ui_kit::UIView;
    if uiview_ptr == 0 {
        return 0;
    }
    match unsafe { Retained::retain(uiview_ptr as *mut UIView) } {
        Some(view) => {
            // Disable autoresizing mask → Auto Layout constraint translation.
            // Without this, the embedded view's autoresizing mask conflicts with
            // UIStackView layout constraints, causing black screen in HStack.
            let _: () = unsafe {
                objc2::msg_send![&*view, setTranslatesAutoresizingMaskIntoConstraints: false]
            };
            widgets::register_widget(view)
        }
        None => 0,
    }
}
