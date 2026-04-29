//! Toolbar (Win32 — simple button bar in a container)

use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    static TOOLBARS: RefCell<HashMap<i64, Vec<ToolbarItem>>> = RefCell::new(HashMap::new());
    static NEXT_TOOLBAR_ID: RefCell<i64> = RefCell::new(1);
    #[cfg(target_os = "windows")]
    static TOOLBAR_HWNDS: RefCell<HashMap<i64, windows::Win32::Foundation::HWND>> = RefCell::new(HashMap::new());
    #[cfg(not(target_os = "windows"))]
    static TOOLBAR_HWNDS: RefCell<HashMap<i64, isize>> = RefCell::new(HashMap::new());
}

struct ToolbarItem {
    label: String,
    _icon: String,
    callback: f64,
}

extern "C" {
    fn js_closure_call0(closure: *const u8) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
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
unsafe extern "system" fn toolbar_default_wnd_proc(
    hwnd: windows::Win32::Foundation::HWND,
    msg: u32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    windows::Win32::UI::WindowsAndMessaging::DefWindowProcW(hwnd, msg, wparam, lparam)
}

/// Create a toolbar (horizontal button container).
pub fn create() -> i64 {
    let id = NEXT_TOOLBAR_ID.with(|id| {
        let mut id = id.borrow_mut();
        let current = *id;
        *id += 1;
        current
    });

    #[cfg(target_os = "windows")]
    {
        use windows::core::PCWSTR;
        use windows::Win32::Foundation::*;
        use windows::Win32::Graphics::Gdi::{COLOR_BTNFACE, HBRUSH};
        use windows::Win32::System::LibraryLoader::GetModuleHandleW;
        use windows::Win32::UI::WindowsAndMessaging::*;

        unsafe {
            let hinstance = GetModuleHandleW(None).unwrap();
            let class_name = to_wide("PerryToolbar");
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(toolbar_default_wnd_proc),
                hInstance: hinstance.into(),
                hbrBackground: HBRUSH((COLOR_BTNFACE.0 + 1) as *mut _),
                lpszClassName: PCWSTR(class_name.as_ptr()),
                ..Default::default()
            };
            RegisterClassExW(&wc);

            // WS_CHILD windows always need a parent — Windows refuses with
            // HRESULT 0x8007057E "Cannot create a top-level child window."
            // Park under the hidden message-only parking HWND like every
            // other widget; `attach()` reparents to the real app window.
            let parking = crate::widgets::get_parking_hwnd();
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                PCWSTR(class_name.as_ptr()),
                PCWSTR::null(),
                WS_CHILD | WS_VISIBLE,
                0,
                0,
                800,
                32,
                parking,
                None,
                HINSTANCE::from(hinstance),
                None,
            )
            .unwrap();

            TOOLBAR_HWNDS.with(|t| t.borrow_mut().insert(id, hwnd));
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        TOOLBAR_HWNDS.with(|t| t.borrow_mut().insert(id, 0));
    }

    TOOLBARS.with(|t| t.borrow_mut().insert(id, Vec::new()));
    id
}

/// Add a button item to the toolbar.
pub fn add_item(toolbar_handle: i64, label_ptr: *const u8, icon_ptr: *const u8, callback: f64) {
    let label = str_from_header(label_ptr).to_string();
    let icon = str_from_header(icon_ptr).to_string();

    TOOLBARS.with(|t| {
        if let Some(items) = t.borrow_mut().get_mut(&toolbar_handle) {
            items.push(ToolbarItem {
                label: label.clone(),
                _icon: icon,
                callback,
            });
        }
    });

    #[cfg(target_os = "windows")]
    {
        use windows::core::PCWSTR;
        use windows::Win32::Foundation::*;
        use windows::Win32::System::LibraryLoader::GetModuleHandleW;
        use windows::Win32::UI::WindowsAndMessaging::*;

        TOOLBAR_HWNDS.with(|t| {
            if let Some(parent) = t.borrow().get(&toolbar_handle) {
                unsafe {
                    let hinstance = GetModuleHandleW(None).unwrap();
                    let item_count = TOOLBARS.with(|tb| {
                        tb.borrow()
                            .get(&toolbar_handle)
                            .map(|i| i.len())
                            .unwrap_or(0)
                    });
                    let x = ((item_count - 1) * 80) as i32;
                    let label_wide = to_wide(&label);
                    let btn_class = to_wide("BUTTON");
                    let _btn = CreateWindowExW(
                        WINDOW_EX_STYLE::default(),
                        PCWSTR(btn_class.as_ptr()),
                        PCWSTR(label_wide.as_ptr()),
                        WINDOW_STYLE(BS_PUSHBUTTON as u32 | WS_CHILD.0 | WS_VISIBLE.0),
                        x,
                        2,
                        76,
                        28,
                        *parent,
                        None,
                        HINSTANCE::from(hinstance),
                        None,
                    );
                }
            }
        });
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (toolbar_handle, callback);
    }
}

/// Attach toolbar to the main app window.
pub fn attach(toolbar_handle: i64) {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::UI::WindowsAndMessaging::*;
        TOOLBAR_HWNDS.with(|t| {
            if let Some(toolbar_hwnd) = t.borrow().get(&toolbar_handle) {
                crate::app::APPS.with(|apps| {
                    let apps = apps.borrow();
                    if let Some(app) = apps.first() {
                        unsafe {
                            let _ = SetParent(*toolbar_hwnd, app.hwnd);
                        }
                    }
                });
            }
        });
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = toolbar_handle;
    }
}
