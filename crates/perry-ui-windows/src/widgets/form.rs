//! Form/Section widgets — Form is a VStack-like container, Section is a BS_GROUPBOX frame

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::*;
#[cfg(target_os = "windows")]
use windows::Win32::Graphics::Gdi::HBRUSH;
#[cfg(target_os = "windows")]
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::*;

use super::{alloc_control_id, register_widget_with_layout, WidgetKind};

fn str_from_header(ptr: *const u8) -> &'static str {
    if ptr.is_null() {
        return "";
    }
    unsafe {
        let header = ptr as *const perry_runtime::string::StringHeader;
        let len = (*header).byte_len as usize;
        let data = ptr.add(std::mem::size_of::<perry_runtime::string::StringHeader>());
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len))
    }
}

#[cfg(target_os = "windows")]
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(target_os = "windows")]
static FORM_CLASS_REGISTERED: std::sync::Once = std::sync::Once::new();

#[cfg(target_os = "windows")]
fn ensure_form_class_registered() {
    FORM_CLASS_REGISTERED.call_once(|| unsafe {
        let hinstance = GetModuleHandleW(None).unwrap();
        let class_name = to_wide("PerryForm");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(form_wnd_proc),
            hInstance: hinstance.into(),
            hbrBackground: HBRUSH(std::ptr::null_mut()),
            lpszClassName: windows::core::PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };
        RegisterClassExW(&wc);
    });
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn form_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

/// Create a Form (VStack-like container with default spacing=16 and insets of 20 on all sides).
/// Returns widget handle.
pub fn create() -> i64 {
    #[cfg(target_os = "windows")]
    {
        ensure_form_class_registered();
        let class_name = to_wide("PerryForm");
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

            register_widget_with_layout(hwnd, WidgetKind::Form, 16.0, (20.0, 20.0, 20.0, 20.0))
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        register_widget_with_layout(0, WidgetKind::Form, 16.0, (20.0, 20.0, 20.0, 20.0))
    }
}

/// Create a Section (BS_GROUPBOX frame with title + inner VStack).
/// Returns widget handle of the groupbox container.
pub fn section_create(title_ptr: *const u8) -> i64 {
    let title = str_from_header(title_ptr);

    #[cfg(target_os = "windows")]
    {
        let control_id = alloc_control_id();
        let wide_title = to_wide(title);
        let class_name = to_wide("BUTTON");
        unsafe {
            let hinstance = GetModuleHandleW(None).unwrap();

            // Create the BS_GROUPBOX frame
            let groupbox_hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                windows::core::PCWSTR(class_name.as_ptr()),
                windows::core::PCWSTR(wide_title.as_ptr()),
                WINDOW_STYLE(BS_GROUPBOX as u32 | WS_CHILD.0 | WS_VISIBLE.0 | WS_CLIPCHILDREN.0),
                0,
                0,
                200,
                100,
                super::get_parking_hwnd(),
                HMENU(control_id as *mut _),
                HINSTANCE::from(hinstance),
                None,
            )
            .unwrap();

            // Register the groupbox as a VStack-like container with spacing and insets
            // The top inset is larger (24) to account for the groupbox title bar
            register_widget_with_layout(
                groupbox_hwnd,
                WidgetKind::Section,
                8.0,
                (24.0, 12.0, 12.0, 12.0),
            )
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = title;
        register_widget_with_layout(0, WidgetKind::Section, 8.0, (24.0, 12.0, 12.0, 12.0))
    }
}
