//! Sheet — modal popup window (Win32)

use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    #[cfg(target_os = "windows")]
    static SHEETS: RefCell<HashMap<i64, windows::Win32::Foundation::HWND>> = RefCell::new(HashMap::new());
    #[cfg(not(target_os = "windows"))]
    static SHEETS: RefCell<HashMap<i64, isize>> = RefCell::new(HashMap::new());
    static NEXT_SHEET_ID: RefCell<i64> = RefCell::new(1);
}

extern "C" {
    fn js_get_string_pointer_unified(value: f64) -> *const u8;
}

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
unsafe extern "system" fn sheet_default_wnd_proc(
    hwnd: windows::Win32::Foundation::HWND,
    msg: u32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    windows::Win32::UI::WindowsAndMessaging::DefWindowProcW(hwnd, msg, wparam, lparam)
}

/// Create a sheet (modal popup window).
pub fn create(width: f64, height: f64, title_val: f64) -> i64 {
    let title = {
        let ptr = unsafe { js_get_string_pointer_unified(title_val) };
        if ptr.is_null() {
            "Sheet".to_string()
        } else {
            str_from_header(ptr).to_string()
        }
    };

    let id = NEXT_SHEET_ID.with(|id| {
        let mut id = id.borrow_mut();
        let current = *id;
        *id += 1;
        current
    });

    #[cfg(target_os = "windows")]
    {
        use windows::core::PCWSTR;
        use windows::Win32::Foundation::*;
        use windows::Win32::Graphics::Gdi::{UpdateWindow, COLOR_WINDOW, HBRUSH};
        use windows::Win32::System::LibraryLoader::GetModuleHandleW;
        use windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
        use windows::Win32::UI::WindowsAndMessaging::*;

        unsafe {
            let hinstance = GetModuleHandleW(None).unwrap();
            let class_name = to_wide("PerrySheet");

            // Register class (idempotent)
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(sheet_default_wnd_proc),
                hInstance: hinstance.into(),
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                hbrBackground: HBRUSH((COLOR_WINDOW.0 + 1) as *mut _),
                lpszClassName: PCWSTR(class_name.as_ptr()),
                ..Default::default()
            };
            RegisterClassExW(&wc);

            let title_wide = to_wide(&title);
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                PCWSTR(class_name.as_ptr()),
                PCWSTR(title_wide.as_ptr()),
                WS_OVERLAPPEDWINDOW,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                width as i32,
                height as i32,
                None,
                None,
                HINSTANCE::from(hinstance),
                None,
            )
            .unwrap();

            SHEETS.with(|s| s.borrow_mut().insert(id, hwnd));
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (width, height, title);
        SHEETS.with(|s| s.borrow_mut().insert(id, 0));
    }

    id
}

/// Present (show) a sheet.
pub fn present(sheet_handle: i64) {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::Graphics::Gdi::UpdateWindow;
        use windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
        use windows::Win32::UI::WindowsAndMessaging::*;
        SHEETS.with(|s| {
            if let Some(hwnd) = s.borrow().get(&sheet_handle) {
                unsafe {
                    // Make modal by disabling parent
                    let _ = EnableWindow(*hwnd, true);
                    let _ = ShowWindow(*hwnd, SW_SHOW);
                    let _ = UpdateWindow(*hwnd);
                }
            }
        });
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = sheet_handle;
    }
}

/// Dismiss (close) a sheet.
pub fn dismiss(sheet_handle: i64) {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::UI::WindowsAndMessaging::*;
        SHEETS.with(|s| {
            if let Some(hwnd) = s.borrow().get(&sheet_handle) {
                unsafe {
                    let _ = ShowWindow(*hwnd, SW_HIDE);
                }
            }
        });
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = sheet_handle;
    }
}
