#![cfg(target_os = "linux")]

pub mod app;
pub mod audio;
pub mod audio_playback;
pub mod camera;
pub mod clipboard;
pub mod deeplinks_stub;
pub mod dialog;
pub mod drag_drop;
pub mod file_dialog;
pub mod issue_552_stub;
pub mod keyboard;
pub mod keychain;
pub mod location;
pub mod media_playback;
pub mod menu;
pub mod network_stub;
pub mod sheet;
pub mod state;
pub mod system;
pub mod toolbar;
pub mod widgets;
pub mod window;

pub mod screenshot;

// Tray icon (issue #490). The body uses `ksni` (which transitively
// pulls `zbus` + `tokio`) — all gated to `cfg(target_os = "linux")` in
// Cargo.toml (mirrors `media_playback`'s mpris module). The FFI
// exports themselves stay unconditional below so the link surface is
// stable on macOS / Windows hosts that build the gtk4 crate without a
// real GTK FFI wired up.
#[cfg(target_os = "linux")]
pub mod tray;

#[cfg(feature = "geisterhand")]
pub mod geisterhand_style;

// FFI exports — split topically into `ffi/*` sub-modules. Each `#[no_mangle]
// pub extern "C" fn perry_ui_<...>` / `perry_system_<...>` / `perry_media_<...>`
// / `__wrapper_perry_<...>` symbol is preserved exactly so codegen-generated
// callsites resolve at link time.
pub mod ffi;
