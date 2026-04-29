//! Divider widget — STATIC control with SS_ETCHEDHORZ style (etched horizontal line)

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::*;
#[cfg(target_os = "windows")]
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(target_os = "windows")]
use windows::Win32::System::SystemServices::SS_ETCHEDHORZ;
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::*;

use super::{alloc_control_id, register_widget, WidgetKind};

#[cfg(target_os = "windows")]
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Create a Divider. Returns widget handle.
pub fn create() -> i64 {
    let control_id = alloc_control_id();

    #[cfg(target_os = "windows")]
    {
        let class_name = to_wide("STATIC");
        let window_text = to_wide("");
        unsafe {
            let hinstance = GetModuleHandleW(None).unwrap();
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                windows::core::PCWSTR(class_name.as_ptr()),
                windows::core::PCWSTR(window_text.as_ptr()),
                WINDOW_STYLE(SS_ETCHEDHORZ.0 | WS_CHILD.0 | WS_VISIBLE.0),
                0,
                0,
                100,
                2,
                super::get_parking_hwnd(),
                HMENU(control_id as *mut _),
                HINSTANCE::from(hinstance),
                None,
            )
            .unwrap();

            register_widget(hwnd, WidgetKind::Divider, control_id)
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        register_widget(0, WidgetKind::Divider, control_id)
    }
}
