//! Picker widget — Win32 COMBOBOX control (CBS_DROPDOWNLIST)

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

// Combobox messages
#[cfg(target_os = "windows")]
const CB_ADDSTRING: u32 = 0x0143;
#[cfg(target_os = "windows")]
const CB_SETCURSEL: u32 = 0x014E;
#[cfg(target_os = "windows")]
const CB_GETCURSEL: u32 = 0x0147;

thread_local! {
    static PICKER_CALLBACKS: RefCell<HashMap<i64, *const u8>> = RefCell::new(HashMap::new());
}

/// Create a Picker (combo box). Returns widget handle.
/// label_ptr is currently unused (Win32 combobox has no built-in label).
/// on_change is a NaN-boxed closure called with the selected index on CBN_SELCHANGE.
/// style is reserved for future use (e.g., CBS_DROPDOWN vs CBS_DROPDOWNLIST).
pub fn create(label_ptr: *const u8, on_change: f64, _style: i64) -> i64 {
    let _label = str_from_header(label_ptr);
    let callback_ptr = unsafe { js_nanbox_get_pointer(on_change) } as *const u8;
    let control_id = alloc_control_id();

    #[cfg(target_os = "windows")]
    {
        let class_name = to_wide("COMBOBOX");
        let window_text = to_wide("");
        unsafe {
            let hinstance = GetModuleHandleW(None).unwrap();
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                windows::core::PCWSTR(class_name.as_ptr()),
                windows::core::PCWSTR(window_text.as_ptr()),
                WINDOW_STYLE(
                    CBS_DROPDOWNLIST as u32
                        | WS_CHILD.0
                        | WS_VISIBLE.0
                        | WS_TABSTOP.0
                        | WS_VSCROLL.0,
                ),
                0,
                0,
                200,
                200, // height includes dropdown area
                // WS_CHILD requires a parent HWND — the main app window
                // doesn't exist yet when widgets are constructed during
                // the body-builder closure, so use the parking window
                // (same approach as button/textfield/toggle/slider/etc).
                // layout::relayout() moves the widget to its real parent
                // once the App() container hierarchy is resolved.
                super::get_parking_hwnd(),
                HMENU(control_id as *mut _),
                HINSTANCE::from(hinstance),
                None,
            )
            .unwrap();

            let handle = register_widget(hwnd, WidgetKind::Picker, control_id);
            PICKER_CALLBACKS.with(|cb| {
                cb.borrow_mut().insert(handle, callback_ptr);
            });

            #[cfg(feature = "geisterhand")]
            {
                extern "C" {
                    fn perry_geisterhand_register(
                        handle: i64,
                        widget_type: u8,
                        callback_kind: u8,
                        closure_f64: f64,
                        label_ptr: *const u8,
                    );
                }
                perry_geisterhand_register(handle, 4, 1, on_change, std::ptr::null());
            }

            handle
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let handle = register_widget(0, WidgetKind::Picker, control_id);
        PICKER_CALLBACKS.with(|cb| {
            cb.borrow_mut().insert(handle, callback_ptr);
        });

        #[cfg(feature = "geisterhand")]
        {
            extern "C" {
                fn perry_geisterhand_register(
                    handle: i64,
                    widget_type: u8,
                    callback_kind: u8,
                    closure_f64: f64,
                    label_ptr: *const u8,
                );
            }
            unsafe {
                perry_geisterhand_register(handle, 4, 1, on_change, std::ptr::null());
            }
        }

        handle
    }
}

/// Add an item to the picker's dropdown list.
pub fn add_item(handle: i64, title_ptr: *const u8) {
    let title = str_from_header(title_ptr);

    #[cfg(target_os = "windows")]
    {
        if let Some(hwnd) = super::get_hwnd(handle) {
            let wide = to_wide(title);
            unsafe {
                SendMessageW(
                    hwnd,
                    CB_ADDSTRING,
                    WPARAM(0),
                    LPARAM(wide.as_ptr() as isize),
                );
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (handle, title);
    }
}

/// Set the selected index of the picker.
pub fn set_selected(handle: i64, index: i64) {
    #[cfg(target_os = "windows")]
    {
        if let Some(hwnd) = super::get_hwnd(handle) {
            unsafe {
                SendMessageW(hwnd, CB_SETCURSEL, WPARAM(index as usize), LPARAM(0));
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (handle, index);
    }
}

/// Get the currently selected index of the picker. Returns -1 if no selection.
pub fn get_selected(handle: i64) -> i64 {
    #[cfg(target_os = "windows")]
    {
        if let Some(hwnd) = super::get_hwnd(handle) {
            let result = unsafe { SendMessageW(hwnd, CB_GETCURSEL, WPARAM(0), LPARAM(0)) };
            return result.0 as i64;
        }
        -1
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = handle;
        -1
    }
}

/// Handle CBN_SELCHANGE notification (notify_code == 1) from WM_COMMAND.
pub fn handle_selchange(handle: i64) {
    let index = get_selected(handle);

    PICKER_CALLBACKS.with(|cb| {
        let callbacks = cb.borrow();
        if let Some(&ptr) = callbacks.get(&handle) {
            unsafe { js_closure_call1(ptr, index as f64) };
        }
    });
}
