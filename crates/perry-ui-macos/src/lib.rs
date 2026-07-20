pub mod app;
pub mod audio;
pub mod audio_playback;
pub mod background;
pub mod clipboard;
pub mod crash_log;
pub mod deeplinks;
pub mod drag_drop;
pub mod ffi;
pub mod file_dialog;
pub mod geolocation;
pub mod image_picker;
pub mod keychain;
pub mod location;
pub mod media_playback;
pub mod memory_pressure;
pub mod menu;
pub mod network;
pub mod notifications;
pub mod pointer;
pub mod state;
pub mod string_header;
pub mod tray;
pub mod widgets;

pub mod screenshot;

#[cfg(feature = "geisterhand")]
pub mod geisterhand_style;

/// Run a closure, catching any Rust panics so they don't abort across the FFI boundary.
/// The global panic hook (installed by crash_log) writes to crash.log first;
/// if we catch the panic here (non-fatal), we clear the log so it doesn't
/// get reported as a crash on next launch.
pub fn catch_callback_panic<F: FnOnce() + std::panic::UnwindSafe>(label: &str, f: F) {
    if let Err(e) = std::panic::catch_unwind(f) {
        // Panic hook already wrote to crash.log — clear it since we caught this one
        crash_log::clear_crash_log();

        let msg = if let Some(s) = e.downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = e.downcast_ref::<String>() {
            s.clone()
        } else {
            format!("{:?}", e)
        };
        eprintln!("[perry] panic in {} (caught): {}", label, msg);
    }
}

// =============================================================================
// FFI exports — these are the functions called from codegen-generated code.
// The implementations are split topically across `lib_ffi/*.rs` submodules to
// keep each file under ~600 lines; every `#[no_mangle] pub extern "C" fn
// perry_ui_*` symbol is preserved exactly so cross-module call sites emitted
// by perry-codegen still link by name.
// =============================================================================

mod lib_ffi {
    pub mod advanced_widgets;
    pub mod core_widgets;
    pub mod dialogs_lifecycle;
    pub mod interactivity;
    pub mod style_and_canvas;
    pub mod system;
    pub mod system_aux;
    pub mod window_misc;
}

// Re-export the FFI entry points at the crate root so intra-crate callers
// that referenced the old `super::perry_ui_*` / `super::perry_system_*` paths
// (e.g. `app.rs::perry_system_is_dark_mode` and the `perry_ui_text_create`
// call inside `perry_ui_text_create_with_id`) continue to resolve unchanged.
// Linker-visible `#[no_mangle]` symbols remain unchanged either way; this is
// purely about Rust path resolution inside the crate.
pub use lib_ffi::advanced_widgets::*;
pub use lib_ffi::core_widgets::*;
pub use lib_ffi::dialogs_lifecycle::*;
pub use lib_ffi::interactivity::*;
pub use lib_ffi::style_and_canvas::*;
pub use lib_ffi::system::*;
pub use lib_ffi::system_aux::*;
pub use lib_ffi::window_misc::*;
