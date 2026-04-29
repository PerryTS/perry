//! ProgressView widget — Win32 PROGRESS_CLASS (msctls_progress32)

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::*;
#[cfg(target_os = "windows")]
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(target_os = "windows")]
use windows::Win32::UI::Controls::*;
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::*;

use super::{alloc_control_id, register_widget, WidgetKind};

#[cfg(target_os = "windows")]
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

// Progress bar messages
#[cfg(target_os = "windows")]
const PBM_SETPOS: u32 = 0x0402;
#[cfg(target_os = "windows")]
const PBM_SETMARQUEE: u32 = 0x040A;
#[cfg(target_os = "windows")]
const PBS_MARQUEE: u32 = 0x08;

/// Create a ProgressView (progress bar). Returns widget handle.
pub fn create() -> i64 {
    let control_id = alloc_control_id();

    #[cfg(target_os = "windows")]
    {
        let window_text = to_wide("");
        unsafe {
            let hinstance = GetModuleHandleW(None).unwrap();
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                PROGRESS_CLASSW,
                windows::core::PCWSTR(window_text.as_ptr()),
                WINDOW_STYLE(PBS_MARQUEE | WS_CHILD.0 | WS_VISIBLE.0),
                0,
                0,
                200,
                20,
                super::get_parking_hwnd(),
                HMENU(control_id as *mut _),
                HINSTANCE::from(hinstance),
                None,
            )
            .unwrap();

            // Start marquee animation (30ms interval)
            SendMessageW(hwnd, PBM_SETMARQUEE, WPARAM(1), LPARAM(30));

            register_widget(hwnd, WidgetKind::ProgressView, control_id)
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        register_widget(0, WidgetKind::ProgressView, control_id)
    }
}

/// Set the progress value.
/// If value < 0, switch to indeterminate (marquee) mode.
/// Otherwise, set the position to value * 100 (0.0-1.0 mapped to 0-100).
pub fn set_value(handle: i64, value: f64) {
    #[cfg(target_os = "windows")]
    {
        if let Some(hwnd) = super::get_hwnd(handle) {
            unsafe {
                if value < 0.0 {
                    // Switch to marquee (indeterminate) mode
                    let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
                    SetWindowLongW(hwnd, GWL_STYLE, (style | PBS_MARQUEE) as i32);
                    SendMessageW(hwnd, PBM_SETMARQUEE, WPARAM(1), LPARAM(30));
                } else {
                    // Switch to determinate mode
                    let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
                    SetWindowLongW(hwnd, GWL_STYLE, (style & !PBS_MARQUEE) as i32);
                    SendMessageW(hwnd, PBM_SETMARQUEE, WPARAM(0), LPARAM(0));
                    let pos = (value * 100.0) as isize;
                    SendMessageW(hwnd, PBM_SETPOS, WPARAM(pos as usize), LPARAM(0));
                }
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (handle, value);
    }
}
