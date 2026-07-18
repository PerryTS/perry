//! Modern window chrome via the Desktop Window Manager (DWM).
//!
//! Win32's default frame looks dated (issue #4681 / discussion #3486). DWM lets
//! us request Windows 11 Fluent chrome — rounded corners and a theme-aware
//! (light/dark) title bar — without switching off the Win32 backend. This is
//! applied by default at window creation in `app::app_create` and
//! `window::create`. On Windows 10 and earlier the unsupported attributes
//! return `E_INVALIDARG` and are silently ignored, so the same call is safe on
//! every supported Windows version.
//!
//! `dwmapi.lib` is already on the final link line (see
//! `crates/perry/src/commands/compile/link/mod.rs`), and `dwmapi.dll` has
//! shipped since Vista, so we bind `DwmSetWindowAttribute` directly — unlike
//! the Win10-only DPI APIs in `dpi_compat`, this import resolves at load time
//! on every supported Windows version.
//!
//! This whole module is `#[cfg(target_os = "windows")]`-gated at the `mod`
//! declaration in `lib.rs`.

use windows::Win32::Foundation::HWND;

// DWM window-attribute identifiers (dwmapi.h). Several of these aren't exposed
// by the `windows` crate at our pinned `0.58`, so we use the raw numeric
// values (matching the previously-inline `extern` blocks in `app.rs`).
const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;
const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
const DWMWA_SYSTEMBACKDROP_TYPE: u32 = 38;

/// `DWM_WINDOW_CORNER_PREFERENCE::DWMWCP_ROUND` — round the window corners
/// (full radius) on Windows 11.
const DWMWCP_ROUND: i32 = 2;

/// `DWM_SYSTEMBACKDROP_TYPE::DWMSBT_MAINWINDOW` — the Mica material, the
/// recommended default backdrop for long-lived top-level app windows
/// (Windows 11 22H2+). Older systems reject it (`E_INVALIDARG`) and we
/// silently ignore that, so requesting it unconditionally is safe.
const DWMSBT_MAINWINDOW: i32 = 2;

// `#[link]` so the import resolves for EVERY link of this crate — the final
// app link always adds dwmapi.lib explicitly, but the in-crate `cargo test`
// harness link did not, failing with LNK2019 and silently blocking the
// crate's whole unit-test suite from ever running.
#[link(name = "dwmapi")]
extern "system" {
    fn DwmSetWindowAttribute(hwnd: isize, attr: u32, value: *const i32, size: u32) -> i32;
}

/// Set a single `BOOL`/`DWORD`-valued DWM attribute. Failures are ignored —
/// versions of Windows that don't recognize `attr` return `E_INVALIDARG`, and
/// the request being unsupported is not an error worth surfacing for cosmetic
/// chrome.
pub fn set_attr_i32(hwnd: HWND, attr: u32, value: i32) {
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd.0 as isize,
            attr,
            &value,
            std::mem::size_of::<i32>() as u32,
        );
    }
}

/// Match the title bar to the current system *app* theme (light/dark). Reuses
/// the same `Personalize\AppsUseLightTheme` probe as `system::is_dark_mode`.
pub fn apply_titlebar_theme(hwnd: HWND) {
    let dark = crate::system::is_dark_mode() != 0;
    set_attr_i32(
        hwnd,
        DWMWA_USE_IMMERSIVE_DARK_MODE,
        if dark { 1 } else { 0 },
    );
}

/// Request rounded corners (Windows 11). No-op on Windows 10 and earlier.
pub fn apply_rounded_corners(hwnd: HWND) {
    set_attr_i32(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND);
}

/// Request the Mica backdrop by default (`DWMSBT_MAINWINDOW`). On Windows 11
/// 22H2+ this gives the DWM-drawn non-client frame (title bar) the Fluent Mica
/// material; older systems ignore it. This deliberately does NOT make the
/// client area transparent — Perry still paints an opaque client background
/// (the class brush / `WM_ERASEBKGND` root-background path), so this can never
/// reintroduce the #1542 "black area after resize" regression. Full
/// client-area Mica blur-through (which requires extending the frame and a
/// transparent client) remains the explicit `app.setVibrancy(...)` opt-in.
pub fn apply_default_backdrop(hwnd: HWND) {
    set_attr_i32(hwnd, DWMWA_SYSTEMBACKDROP_TYPE, DWMSBT_MAINWINDOW);
}

/// The default Fluent-leaning chrome Perry applies to every top-level window:
/// rounded corners, a theme-aware title bar, and the Mica backdrop. Called once
/// at window creation, before the window is shown.
pub fn apply_default_window_chrome(hwnd: HWND) {
    apply_rounded_corners(hwnd);
    apply_titlebar_theme(hwnd);
    apply_default_backdrop(hwnd);
}

/// Set the system backdrop material via `DWMWA_SYSTEMBACKDROP_TYPE`
/// (`0`=Auto, `1`=None, `2`=Mica, `3`=Acrylic, `4`=MicaAlt). Windows 11 22H2+;
/// ignored on older systems. Used by the `app.setVibrancy` opt-in path.
pub fn set_backdrop(hwnd: HWND, backdrop: i32) {
    set_attr_i32(hwnd, DWMWA_SYSTEMBACKDROP_TYPE, backdrop);
}
