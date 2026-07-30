//! Win32 client-area and child-control light/dark theming (#6612).
//!
//! DWM owns only the non-client frame. Standard controls need an explicit
//! uxtheme class, while owner/custom-drawn controls need matching GDI colors.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

use windows::core::{BOOL, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateSolidBrush, DeleteObject, FillRect, GetSysColor, GetSysColorBrush, InvalidateRect,
    SetBkColor, SetTextColor, COLOR_WINDOW, COLOR_WINDOWTEXT, HBRUSH, HDC,
};
use windows::Win32::UI::Controls::SetWindowTheme;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, GetClientRect, GetParent, SendMessageW, WM_COMMAND, WM_CONTEXTMENU,
    WM_CTLCOLORBTN, WM_CTLCOLORLISTBOX, WM_CTLCOLORSTATIC, WM_DRAWITEM, WM_ERASEBKGND,
};

const MODE_UNKNOWN: u8 = 0;
const MODE_LIGHT: u8 = 1;
const MODE_DARK: u8 = 2;

/// COLORREF is `0x00BBGGRR`.
const DARK_BACKGROUND: u32 = 0x0020_2020;
const DARK_CONTROL_BACKGROUND: u32 = 0x002B_2B2B;
const DARK_TEXT: u32 = 0x00F0_F0F0;

static CACHED_MODE: AtomicU8 = AtomicU8::new(MODE_UNKNOWN);
static DARK_BACKGROUND_BRUSH: OnceLock<usize> = OnceLock::new();
static DARK_CONTROL_BRUSH: OnceLock<usize> = OnceLock::new();

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn detect_dark_mode() -> bool {
    crate::system::is_dark_mode() != 0
}

/// Return the cached app-theme state, probing the registry on first use.
pub fn is_dark_mode() -> bool {
    match CACHED_MODE.load(Ordering::Relaxed) {
        MODE_DARK => true,
        MODE_LIGHT => false,
        _ => refresh_mode(),
    }
}

/// Re-read the Windows app theme. Called for `WM_SETTINGCHANGE` /
/// `WM_THEMECHANGED` so an already-running Perry app follows the OS setting.
pub fn refresh_mode() -> bool {
    let dark = detect_dark_mode();
    CACHED_MODE.store(if dark { MODE_DARK } else { MODE_LIGHT }, Ordering::Relaxed);
    dark
}

fn permanent_brush(slot: &OnceLock<usize>, color: u32) -> HBRUSH {
    let raw = *slot.get_or_init(|| unsafe { CreateSolidBrush(COLORREF(color)).0 as usize });
    HBRUSH(raw as *mut _)
}

/// Default client/container background brush for the current app theme.
pub fn background_brush() -> HBRUSH {
    if is_dark_mode() {
        permanent_brush(&DARK_BACKGROUND_BRUSH, DARK_BACKGROUND)
    } else {
        unsafe { GetSysColorBrush(COLOR_WINDOW) }
    }
}

/// Default control surface brush (`EDIT`, `BUTTON`, list-like controls).
pub fn control_brush() -> HBRUSH {
    if is_dark_mode() {
        permanent_brush(&DARK_CONTROL_BRUSH, DARK_CONTROL_BACKGROUND)
    } else {
        unsafe { GetSysColorBrush(COLOR_WINDOW) }
    }
}

pub fn text_color() -> COLORREF {
    if is_dark_mode() {
        COLORREF(DARK_TEXT)
    } else {
        COLORREF(unsafe { GetSysColor(COLOR_WINDOWTEXT) })
    }
}

pub fn control_background_color() -> COLORREF {
    if is_dark_mode() {
        COLORREF(DARK_CONTROL_BACKGROUND)
    } else {
        COLORREF(unsafe { GetSysColor(COLOR_WINDOW) })
    }
}

/// Ask uxtheme to render a standard child control in the current app theme.
///
/// `DarkMode_Explorer` is the theme class used by modern Win32 apps;
/// unsupported Windows versions simply ignore the request.
pub fn apply_control_theme(hwnd: HWND) {
    let class = wide(if is_dark_mode() {
        "DarkMode_Explorer"
    } else {
        "Explorer"
    });
    unsafe {
        let _ = SetWindowTheme(hwnd, PCWSTR(class.as_ptr()), PCWSTR::null());
        let _ = InvalidateRect(Some(hwnd), None, true);
    }
}

/// Forward control notifications through lightweight Perry containers.
///
/// Win32 sends `WM_CTLCOLOR*`, `WM_COMMAND`, and `WM_DRAWITEM` only to a
/// control's immediate parent. Form/NavStack/ZStack therefore need to relay
/// them until the top-level Perry window can apply widget-specific styling.
pub unsafe fn handle_container_message(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> Option<LRESULT> {
    if msg == WM_ERASEBKGND {
        let parent = unsafe { GetParent(hwnd) }.ok()?;
        let is_app_root = crate::app::get_main_hwnd() == Some(parent)
            || crate::window::is_perry_window_hwnd(parent.0 as isize);
        if !is_app_root {
            return None;
        }

        let hdc = HDC(wparam.0 as *mut _);
        let mut rect = RECT::default();
        unsafe {
            let _ = GetClientRect(hwnd, &mut rect);
        }
        if crate::widgets::paint_gradient(hwnd, hdc, &rect) {
            return Some(LRESULT(1));
        }
        if let Some(color) = crate::widgets::get_hwnd_bg_color(hwnd) {
            unsafe {
                let brush = CreateSolidBrush(COLORREF(color));
                FillRect(hdc, &rect, brush);
                let _ = DeleteObject(brush.into());
            }
            return Some(LRESULT(1));
        }
        return Some(erase_background(hwnd, hdc));
    }

    let control_surface_message =
        msg == 0x0133 || matches!(msg, WM_CTLCOLORBTN | WM_CTLCOLORLISTBOX); // EDIT / BTN / LISTBOX
    let color_message = matches!(msg, WM_CTLCOLORSTATIC | WM_CTLCOLORBTN | WM_CTLCOLORLISTBOX)
        || control_surface_message;
    let forwarded = matches!(msg, WM_COMMAND | WM_CONTEXTMENU | WM_DRAWITEM) || color_message;
    if !forwarded {
        return None;
    }

    if let Ok(parent) = unsafe { GetParent(hwnd) } {
        if !parent.0.is_null() {
            let result = unsafe { SendMessageW(parent, msg, Some(wparam), Some(lparam)) };
            if result.0 != 0 || !color_message {
                return Some(result);
            }
        }
    }

    if color_message {
        return handle_control_color(HDC(wparam.0 as *mut _), control_surface_message);
    }
    None
}

/// Default fallback for `WM_CTLCOLORSTATIC` / `BTN` / `EDIT` after a widget's
/// explicit foreground/background styling has had the first chance to answer.
pub fn handle_control_color(hdc: HDC, control_surface: bool) -> Option<LRESULT> {
    if !is_dark_mode() {
        return None;
    }
    unsafe {
        SetTextColor(hdc, text_color());
        SetBkColor(
            hdc,
            if control_surface {
                control_background_color()
            } else {
                COLORREF(DARK_BACKGROUND)
            },
        );
    }
    let brush = if control_surface {
        control_brush()
    } else {
        background_brush()
    };
    Some(LRESULT(brush.0 as isize))
}

/// Fill a window's client area with the theme's default background.
pub fn erase_background(hwnd: HWND, hdc: HDC) -> LRESULT {
    unsafe {
        let mut rect = RECT::default();
        let _ = GetClientRect(hwnd, &mut rect);
        FillRect(hdc, &rect, background_brush());
    }
    LRESULT(1)
}

unsafe extern "system" fn refresh_child(hwnd: HWND, _lparam: LPARAM) -> BOOL {
    apply_control_theme(hwnd);
    BOOL(1)
}

/// Re-theme an existing top-level window and every descendant control.
pub fn refresh_window_tree(hwnd: HWND) {
    refresh_mode();
    apply_control_theme(hwnd);
    unsafe {
        let _ = EnumChildWindows(Some(hwnd), Some(refresh_child), LPARAM(0));
        let _ = InvalidateRect(Some(hwnd), None, true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_palette_uses_colorref_byte_order() {
        assert_eq!(DARK_BACKGROUND, 0x0020_2020);
        assert_eq!(DARK_CONTROL_BACKGROUND, 0x002B_2B2B);
        assert_eq!(DARK_TEXT, 0x00F0_F0F0);
    }
}
