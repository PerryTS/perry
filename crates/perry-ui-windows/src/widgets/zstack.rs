//! ZStack widget — custom window class where all children are positioned at (0,0)
//! overlapping each other (z-order determines visual stacking).

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::*;
#[cfg(target_os = "windows")]
use windows::Win32::Graphics::Gdi::HBRUSH;
#[cfg(target_os = "windows")]
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::*;

use super::{register_widget_with_layout, WidgetKind};

#[cfg(target_os = "windows")]
static ZSTACK_CLASS_REGISTERED: std::sync::Once = std::sync::Once::new();

#[cfg(target_os = "windows")]
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(target_os = "windows")]
fn ensure_class_registered() {
    ZSTACK_CLASS_REGISTERED.call_once(|| {
        unsafe {
            let hinstance = GetModuleHandleW(None).unwrap();
            let class_name = to_wide("PerryZStack");
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(zstack_wnd_proc),
                hInstance: hinstance.into(),
                hbrBackground: HBRUSH(std::ptr::null_mut()), // transparent
                lpszClassName: windows::core::PCWSTR(class_name.as_ptr()),
                ..Default::default()
            };
            RegisterClassExW(&wc);
        }
    });
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn zstack_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

/// Create a ZStack. Returns widget handle.
/// All children are placed at (0,0) and overlap — z-order determines visual stacking.
pub fn create() -> i64 {
    #[cfg(target_os = "windows")]
    {
        ensure_class_registered();
        let class_name = to_wide("PerryZStack");
        let window_text = to_wide("");
        unsafe {
            let hinstance = GetModuleHandleW(None).unwrap();
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                windows::core::PCWSTR(class_name.as_ptr()),
                windows::core::PCWSTR(window_text.as_ptr()),
                WS_CHILD | WS_VISIBLE | WS_CLIPCHILDREN,
                0,
                0,
                100,
                100,
                super::get_parking_hwnd(),
                None,
                HINSTANCE::from(hinstance),
                None,
            )
            .unwrap();

            register_widget_with_layout(hwnd, WidgetKind::ZStack, 0.0, (0.0, 0.0, 0.0, 0.0))
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        register_widget_with_layout(0, WidgetKind::ZStack, 0.0, (0.0, 0.0, 0.0, 0.0))
    }
}
