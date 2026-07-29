//! Opt-in WinUI 3 (Fluent) Windows backend — issue #4680.
//!
//! Selected via `--target windows-winui`. Motivation (discussion #3486): the
//! default Win32/GDI chrome looks dated; WinUI 3 / Fluent brings rounded
//! corners, Mica/Acrylic materials, smooth animation, and crisp auto-DPI —
//! the closest Windows analog to Perry's iOS/Android look-and-feel.
//!
//! # Why a separate target (not the default)
//!
//! WinUI 3 requires the **Windows App SDK runtime** plus an MSIX/bootstrapper
//! packaging story, which conflicts with Perry's single-native-`.exe` model.
//! So Win32 (`perry-ui-windows`, `--target windows`) stays the default and the
//! Fluent backend is opt-in, mirroring how Perry ships multiple Apple targets.
//!
//! # Scaffold status (this crate today)
//!
//! This crate currently **re-exports `perry-ui-windows` verbatim**. That makes
//! `--target windows-winui` a real, selectable target immediately: apps build,
//! link, and run — rendering through the Win32 backend for now. Building the
//! staticlib bundles every `#[no_mangle] extern "C"` `perry_ui_*` symbol from
//! `perry-ui-windows`, so the FFI surface the perry driver links against is
//! identical to the `windows` target. Nothing regresses, and there is a stable
//! place to grow the real Fluent backend.
//!
//! # The incremental plan (#4680 work breakdown)
//!
//! The XAML widget mapping lands one widget at a time in [`winui`], each
//! replacing the corresponding Win32 widget creation with a
//! `Microsoft.UI.Xaml` control driven through the windows-rs WinRT
//! projections, until the whole ~40-widget set is Fluent. Steps, in order:
//!
//! 1. WinAppSDK + `Microsoft.UI.Xaml` projections wired in (`Cargo.toml`).
//! 2. Bootstrapper / runtime acquisition ([`winui::bootstrap`]) — **done**:
//!    [`winui::bootstrap::initialize`] dynamically loads the Windows App SDK
//!    bootstrapper and reports whether the WinUI path is usable, falling back
//!    to Win32 (this re-export) when the runtime is absent.
//! 3. Widget mapping layer (perry-ui widget set → XAML controls). The
//!    *dispatch seam* is in place ([`winui::backend::active`]): a CRT
//!    static initializer probes the SDK at process start and resolves a
//!    [`winui::backend::RenderBackend`] (`Fluent` when the runtime is ready,
//!    else `Win32`). Each XAML widget lands behind this check; today it always
//!    resolves to `Win32` until the first control is wired in.
//! 4. Window chrome: Mica/Acrylic backdrop, Fluent title bar, light/dark/system.
//! 5. Packaging: MSIX or unpackaged WinAppSDK bootstrap; document the runtime.
//! 6. `apply_style` (geisterhand) dispatcher parity with the other backends.
//!
//! Tracks discussion #3486. Near-term Win32 polish on the default target lives
//! in #4681.

#![cfg(target_os = "windows")]

// Re-export the entire Win32 backend. For Rust consumers this exposes the same
// module API as `perry-ui-windows`; for the C ABI it is a no-op (the
// `#[no_mangle]` entry points are emitted when `perry-ui-windows` is compiled
// and bundled into this crate's staticlib regardless of Rust-level re-export),
// but it documents that this crate IS the Win32 surface until XAML supersedes
// it widget-by-widget.
pub use perry_ui_windows::*;

pub mod winui;
