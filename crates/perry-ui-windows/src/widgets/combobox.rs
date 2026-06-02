//! Combobox widget — Win32 `COMBOBOX` with `CBS_DROPDOWN` style
//! (editable text + dropdown list). The picker widget already uses
//! `CBS_DROPDOWNLIST` (read-only); this is the editable variant.
//!
//! Edit-text changes route through `CBN_EDITCHANGE` (notify code 5);
//! dropdown picks route through `CBN_SELCHANGE` (notify code 1) which
//! the picker already handles. Both paths land in the central WM_COMMAND
//! dispatcher; we add a Combobox arm there.

use std::cell::RefCell;
use std::collections::HashMap;

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::*;
#[cfg(target_os = "windows")]
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::*;

use super::{alloc_control_id, register_widget, WidgetKind};

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
    fn js_nanbox_string(ptr: i64) -> f64;
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
const CB_ADDSTRING: u32 = 0x0143;
#[cfg(target_os = "windows")]
const CB_GETCURSEL: u32 = 0x0147;
#[cfg(target_os = "windows")]
const CB_GETLBTEXT: u32 = 0x0148;

thread_local! {
    static CALLBACKS: RefCell<HashMap<i64, f64>> = RefCell::new(HashMap::new());
}

pub fn create(initial_ptr: *const u8, on_change: f64) -> i64 {
    let initial = str_from_header(initial_ptr);
    let control_id = alloc_control_id();

    #[cfg(target_os = "windows")]
    {
        let class_name = to_wide("COMBOBOX");
        let initial_w = to_wide(initial);
        unsafe {
            let hinstance = GetModuleHandleW(None).unwrap();
            // CBS_DROPDOWN = 0x0002 (editable text + dropdown list).
            // CBS_AUTOHSCROLL keeps the edit field horizontally scrolled
            // so long values don't visually truncate during typing.
            const CBS_DROPDOWN: u32 = 0x0002;
            const CBS_AUTOHSCROLL: u32 = 0x0040;
            let style = WINDOW_STYLE(
                CBS_DROPDOWN
                    | CBS_AUTOHSCROLL
                    | WS_CHILD.0
                    | WS_VISIBLE.0
                    | WS_TABSTOP.0
                    | WS_VSCROLL.0,
            );
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                windows::core::PCWSTR(class_name.as_ptr()),
                windows::core::PCWSTR(initial_w.as_ptr()),
                style,
                0,
                0,
                220,
                200,
                super::get_parking_hwnd(),
                HMENU(control_id as *mut _),
                HINSTANCE::from(hinstance),
                None,
            );
            let Ok(hwnd) = hwnd else {
                return register_widget(
                    HWND(std::ptr::null_mut()),
                    WidgetKind::Combobox,
                    control_id,
                );
            };

            let handle = register_widget(hwnd, WidgetKind::Combobox, control_id);
            CALLBACKS.with(|m| {
                m.borrow_mut().insert(handle, on_change);
            });
            handle
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (initial, on_change);
        register_widget(0, WidgetKind::Combobox, control_id)
    }
}

pub fn add_item(handle: i64, value_ptr: *const u8) {
    let value = str_from_header(value_ptr);
    #[cfg(target_os = "windows")]
    {
        let Some(hwnd) = super::get_hwnd(handle) else {
            return;
        };
        let wide = to_wide(value);
        unsafe {
            SendMessageW(
                hwnd,
                CB_ADDSTRING,
                WPARAM(0),
                LPARAM(wide.as_ptr() as isize),
            );
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (handle, value);
    }
}

pub fn set_value(handle: i64, value_ptr: *const u8) {
    let value = str_from_header(value_ptr);
    #[cfg(target_os = "windows")]
    {
        let Some(hwnd) = super::get_hwnd(handle) else {
            return;
        };
        let wide = to_wide(value);
        unsafe {
            SetWindowTextW(hwnd, windows::core::PCWSTR(wide.as_ptr())).ok();
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (handle, value);
    }
}

pub fn get_value(handle: i64) -> f64 {
    let undefined = f64::from_bits(0x7FFC_0000_0000_0001);
    #[cfg(target_os = "windows")]
    {
        let Some(hwnd) = super::get_hwnd(handle) else {
            return undefined;
        };
        unsafe {
            // Buffer for the edit-text portion. 1024 covers anything a
            // user would reasonably type; truncate cleanly otherwise.
            let mut buf = [0u16; 1024];
            let len = GetWindowTextW(hwnd, &mut buf) as usize;
            let s = String::from_utf16_lossy(&buf[..len]);
            let bytes = s.as_bytes();
            let header = js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64);
            js_nanbox_string(header as i64)
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = handle;
        undefined
    }
}

/// Called by the WM_COMMAND router when CBN_EDITCHANGE (5) or
/// CBN_SELCHANGE (1) fires on a Combobox handle. Reads the current
/// edit-text value and fires onChange with it (NaN-boxed STRING).
#[cfg(target_os = "windows")]
pub fn handle_change(handle: i64) {
    let on = CALLBACKS.with(|m| m.borrow().get(&handle).copied().unwrap_or(0.0));
    if on == 0.0 {
        return;
    }
    let Some(hwnd) = super::get_hwnd(handle) else {
        return;
    };
    unsafe {
        let mut buf = [0u16; 1024];
        let len = GetWindowTextW(hwnd, &mut buf) as usize;
        let s = String::from_utf16_lossy(&buf[..len]);
        let bytes = s.as_bytes();
        let header = js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64);
        let arg = js_nanbox_string(header as i64);
        let closure_ptr = js_nanbox_get_pointer(on) as *const u8;
        js_closure_call1(closure_ptr, arg);
    }
}

#[cfg(target_os = "windows")]
pub fn handle_dropdown_pick(handle: i64) {
    // Apply the picked item's text to the edit field and fire the same
    // change callback. CB_GETCURSEL gives the index, CB_GETLBTEXT
    // copies the text.
    let Some(hwnd) = super::get_hwnd(handle) else {
        return;
    };
    unsafe {
        let idx = SendMessageW(hwnd, CB_GETCURSEL, WPARAM(0), LPARAM(0)).0 as i32;
        if idx < 0 {
            return;
        }
        let mut buf = [0u16; 1024];
        SendMessageW(
            hwnd,
            CB_GETLBTEXT,
            WPARAM(idx as usize),
            LPARAM(buf.as_mut_ptr() as isize),
        );
        // Find null terminator + write back into edit field.
        let nul = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        if !buf.is_empty() {
            SetWindowTextW(hwnd, windows::core::PCWSTR(buf.as_ptr())).ok();
        }
        let _ = nul;
    }
    handle_change(handle);
}
