//! VStack widget — custom window class for vertical layout container

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::*;
#[cfg(target_os = "windows")]
use windows::Win32::Graphics::Gdi::{
    CreateSolidBrush, FillRect, SetBkMode, COLOR_WINDOW, HBRUSH, HDC, TRANSPARENT,
};
#[cfg(target_os = "windows")]
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::*;

use super::{register_widget_with_layout, WidgetKind};

#[cfg(target_os = "windows")]
static VSTACK_CLASS_REGISTERED: std::sync::Once = std::sync::Once::new();

#[cfg(target_os = "windows")]
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(target_os = "windows")]
fn ensure_class_registered() {
    VSTACK_CLASS_REGISTERED.call_once(|| {
        unsafe {
            let hinstance = GetModuleHandleW(None).unwrap();
            let class_name = to_wide("PerryVStack");
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(container_wnd_proc),
                hInstance: HINSTANCE::from(hinstance),
                // Default window-color class brush. With a null brush, the
                // gaps between intrinsic-sized children and any area exposed by
                // enlarging the window erase to nothing → black (#1542). The
                // WM_ERASEBKGND handler below still fills an explicit/ancestor
                // .background() color or gradient and returns early, so this
                // class brush is only used when no background is set anywhere.
                hbrBackground: HBRUSH((COLOR_WINDOW.0 + 1) as *mut _),
                lpszClassName: windows::core::PCWSTR(class_name.as_ptr()),
                ..Default::default()
            };
            RegisterClassExW(&wc);
        }
    });
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn container_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        // Forward child notifications to the top-level window so button clicks,
        // text color, and context menus are handled by the main wnd_proc.
        WM_CTLCOLORSTATIC | WM_CTLCOLORBTN => {
            // For color messages, try forwarding to parent first. If the result
            // is the default (no custom brush), provide our own ancestor brush
            // so child controls get the correct background with WS_CLIPCHILDREN.
            if let Ok(parent) = GetParent(hwnd) {
                let result = SendMessageW(parent, msg, Some(wparam), Some(lparam));
                if result.0 != 0 {
                    return result;
                }
            }
            // Fallback: find our own bg or ancestor bg and return that brush
            if let Some(color) = super::get_hwnd_bg_color(hwnd)
                .or_else(|| super::find_ancestor_hwnd_bg_color(hwnd))
            {
                let hdc = HDC(wparam.0 as *mut _);
                unsafe {
                    SetBkMode(hdc, TRANSPARENT);
                }
                let brush = unsafe { CreateSolidBrush(COLORREF(color)) };
                return LRESULT(brush.0 as isize);
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_COMMAND | WM_CONTEXTMENU | WM_DRAWITEM => {
            if let Ok(parent) = GetParent(hwnd) {
                return SendMessageW(parent, msg, Some(wparam), Some(lparam));
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        x if x == 0x0133 /* WM_CTLCOLOREDIT */ => {
            if let Ok(parent) = GetParent(hwnd) {
                return SendMessageW(parent, msg, Some(wparam), Some(lparam));
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_ERASEBKGND | WM_PAINT => {
            // Try gradient fill first (real GDI GradientFill)
            if msg == WM_ERASEBKGND {
                let hdc = windows::Win32::Graphics::Gdi::HDC(wparam.0 as *mut _);
                let mut rect = RECT::default();
                let _ = GetClientRect(hwnd, &mut rect);
                if crate::widgets::paint_gradient(hwnd, hdc, &rect) {
                    return LRESULT(1);
                }
            } else {
                // WM_PAINT — check if gradient exists before BeginPaint
                let mut rect = RECT::default();
                let _ = GetClientRect(hwnd, &mut rect);
                let has_gradient = crate::widgets::GRADIENT_MAP.lock()
                    .map(|map| map.iter().any(|(k, _)| *k == hwnd.0 as isize))
                    .unwrap_or(false);
                if has_gradient {
                    let mut ps = windows::Win32::Graphics::Gdi::PAINTSTRUCT::default();
                    let hdc = windows::Win32::Graphics::Gdi::BeginPaint(hwnd, &mut ps);
                    crate::widgets::paint_gradient(hwnd, hdc, &rect);
                    windows::Win32::Graphics::Gdi::EndPaint(hwnd, &ps);
                    return LRESULT(0);
                }
            }
            // Fall through to solid color fill
            let color = super::get_hwnd_bg_color(hwnd)
                .or_else(|| super::find_ancestor_hwnd_bg_color(hwnd));
            if let Some(color) = color {
                let brush = windows::Win32::Graphics::Gdi::CreateSolidBrush(COLORREF(color));
                if msg == WM_ERASEBKGND {
                    let hdc = windows::Win32::Graphics::Gdi::HDC(wparam.0 as *mut _);
                    let mut rect = RECT::default();
                    let _ = GetClientRect(hwnd, &mut rect);
                    let _ = FillRect(hdc, &rect, brush);
                    let _ = windows::Win32::Graphics::Gdi::DeleteObject(brush.into());
                    return LRESULT(1);
                } else {
                    let mut ps = windows::Win32::Graphics::Gdi::PAINTSTRUCT::default();
                    let hdc = windows::Win32::Graphics::Gdi::BeginPaint(hwnd, &mut ps);
                    let mut rect = RECT::default();
                    let _ = GetClientRect(hwnd, &mut rect);
                    let _ = FillRect(hdc, &rect, brush);
                    let _ = windows::Win32::Graphics::Gdi::DeleteObject(brush.into());
                    windows::Win32::Graphics::Gdi::EndPaint(hwnd, &ps);
                    return LRESULT(0);
                }
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// Walk the HWND parent chain to find the nearest ancestor with a background brush.
#[cfg(target_os = "windows")]
fn find_ancestor_brush(mut hwnd: HWND) -> Option<HBRUSH> {
    for _ in 0..10 {
        // max 10 levels deep
        if let Ok(parent) = unsafe { GetParent(hwnd) } {
            if parent.0.is_null() {
                break;
            }
            let parent_handle = super::find_handle_by_hwnd(parent);
            if parent_handle > 0 {
                if let Some(brush) = super::get_bg_brush(parent_handle) {
                    return Some(brush);
                }
            }
            hwnd = parent;
        } else {
            break;
        }
    }
    None
}

/// Create a VStack. Returns widget handle.
pub fn create(spacing: f64) -> i64 {
    create_with_insets(spacing, 0.0, 0.0, 0.0, 0.0)
}

/// Create a VStack with custom insets. Returns widget handle.
pub fn create_with_insets(spacing: f64, top: f64, left: f64, bottom: f64, right: f64) -> i64 {
    #[cfg(target_os = "windows")]
    {
        ensure_class_registered();
        let class_name = to_wide("PerryVStack");
        let window_text = to_wide("");
        unsafe {
            let hinstance = GetModuleHandleW(None).unwrap();
            let hwnd = CreateWindowExW(
                // WS_EX_CONTROLPARENT: the dialog manager (IsDialogMessageW
                // in the message pump) must recurse INTO containers to find
                // WS_TABSTOP controls — without it, Tab stops dead at the
                // container level.
                WINDOW_EX_STYLE(WS_EX_CONTROLPARENT.0),
                windows::core::PCWSTR(class_name.as_ptr()),
                windows::core::PCWSTR(window_text.as_ptr()),
                WS_CHILD | WS_VISIBLE | WS_CLIPCHILDREN,
                0,
                0,
                100,
                100,
                Some(super::get_parking_hwnd()),
                None,
                Some(HINSTANCE::from(hinstance)),
                None,
            )
            .unwrap();

            register_widget_with_layout(
                hwnd,
                WidgetKind::VStack,
                spacing,
                (top, left, bottom, right),
            )
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        register_widget_with_layout(0, WidgetKind::VStack, spacing, (top, left, bottom, right))
    }
}
