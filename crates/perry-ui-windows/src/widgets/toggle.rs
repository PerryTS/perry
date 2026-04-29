//! Toggle widget — Win32 BUTTON with BS_AUTOCHECKBOX style

use std::cell::RefCell;
use std::collections::HashMap;

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::*;
#[cfg(target_os = "windows")]
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(target_os = "windows")]
use windows::Win32::UI::Controls::{BST_CHECKED, BST_UNCHECKED};
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::*;

use super::{alloc_control_id, register_widget, WidgetKind};

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
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

thread_local! {
    static TOGGLE_CALLBACKS: RefCell<HashMap<i64, *const u8>> = RefCell::new(HashMap::new());
}

/// Create a Toggle (checkbox). Returns widget handle.
pub fn create(label_ptr: *const u8, on_change: f64) -> i64 {
    let label = str_from_header(label_ptr);
    let callback_ptr = unsafe { js_nanbox_get_pointer(on_change) } as *const u8;
    let control_id = alloc_control_id();

    #[cfg(target_os = "windows")]
    {
        let wide = to_wide(label);
        let class_name = to_wide("BUTTON");
        unsafe {
            let hinstance = GetModuleHandleW(None).unwrap();
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                windows::core::PCWSTR(class_name.as_ptr()),
                windows::core::PCWSTR(wide.as_ptr()),
                WINDOW_STYLE(BS_AUTOCHECKBOX as u32 | WS_CHILD.0 | WS_VISIBLE.0 | WS_TABSTOP.0),
                0,
                0,
                100,
                24,
                super::get_parking_hwnd(),
                HMENU(control_id as *mut _),
                HINSTANCE::from(hinstance),
                None,
            )
            .unwrap();

            let handle = register_widget(hwnd, WidgetKind::Toggle, control_id);
            TOGGLE_CALLBACKS.with(|cb| {
                cb.borrow_mut().insert(handle, callback_ptr);
            });
            #[cfg(feature = "geisterhand")]
            {
                extern "C" {
                    fn perry_geisterhand_register(h: i64, wt: u8, ck: u8, cb: f64, lbl: *const u8);
                }
                unsafe {
                    perry_geisterhand_register(handle, 3, 1, on_change, label_ptr);
                }
            }
            handle
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = label;
        let handle = register_widget(0, WidgetKind::Toggle, control_id);
        TOGGLE_CALLBACKS.with(|cb| {
            cb.borrow_mut().insert(handle, callback_ptr);
        });
        #[cfg(feature = "geisterhand")]
        {
            extern "C" {
                fn perry_geisterhand_register(h: i64, wt: u8, ck: u8, cb: f64, lbl: *const u8);
            }
            unsafe {
                perry_geisterhand_register(handle, 3, 1, on_change, label_ptr);
            }
        }
        handle
    }
}

/// Handle toggle click (BN_CLICKED on checkbox).
pub fn handle_click(handle: i64) {
    #[cfg(target_os = "windows")]
    {
        if let Some(hwnd) = super::get_hwnd(handle) {
            let checked = unsafe {
                SendMessageW(hwnd, BM_GETCHECK, WPARAM(0), LPARAM(0)).0 == BST_CHECKED.0 as isize
            };
            let value = if checked { 1.0 } else { 0.0 };

            let ptr = TOGGLE_CALLBACKS.with(|cb| {
                let callbacks = cb.borrow();
                callbacks.get(&handle).copied()
            });
            if let Some(ptr) = ptr {
                unsafe { js_closure_call1(ptr, value) };
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = handle;
    }
}

/// Set the checked state of a toggle (for two-way state binding).
pub fn set_state(handle: i64, on: i32) {
    #[cfg(target_os = "windows")]
    {
        if let Some(hwnd) = super::get_hwnd(handle) {
            let check = if on != 0 { BST_CHECKED } else { BST_UNCHECKED };
            unsafe {
                SendMessageW(hwnd, BM_SETCHECK, WPARAM(check.0 as usize), LPARAM(0));
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (handle, on);
    }
}
