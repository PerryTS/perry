//! Opt-in WinUI 3 / Fluent backend for Perry's Windows target.
//!
//! `--target windows-winui` keeps Perry's stable `perry_ui_*` C ABI but
//! renders the core widget tree with real `Microsoft.UI.Xaml` controls through
//! Microsoft Windows Reactor. `--target windows` continues to compile the
//! unchanged Win32/GDI exports from `perry-ui-windows`.

#![cfg(target_os = "windows")]

pub mod app;
mod gc;
pub mod pointer;
pub mod widgets;
pub mod winui;

// The state implementation is backend-neutral apart from calls through
// `crate::widgets`; compiling it here makes its bindings update the Fluent
// model instead of the Win32 HWND registry.
#[path = "../../perry-ui-windows/src/state.rs"]
pub mod state;

// Platform services that do not render widgets remain shared with Win32.
pub use perry_ui_windows::{
    audio, audio_playback, clipboard, deeplinks_stub, dialog, dpi_compat, drag_drop, dwm,
    file_dialog, folder_dialog, issue_552_stub, keyboard, keychain, layout, media_playback, menu,
    network_stub, screenshot, sheet, system, theme, toolbar, tray, window,
};

#[cfg(feature = "geisterhand")]
pub use perry_ui_windows::geisterhand_style;

// Compile the proven ABI wrappers against this crate's `app`, `state`, and
// `widgets` modules. `perry-ui-windows` disables its own `ffi-exports` feature
// in this dependency graph, so these are the archive's only unmangled exports.
#[path = "../../perry-ui-windows/src/ffi/mod.rs"]
pub mod ffi;
