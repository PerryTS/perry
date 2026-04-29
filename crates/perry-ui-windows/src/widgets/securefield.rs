//! SecureField widget — Win32 EDIT control with ES_PASSWORD flag

use std::cell::RefCell;
use std::collections::HashMap;

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::*;
#[cfg(target_os = "windows")]
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(target_os = "windows")]
use windows::Win32::UI::Controls::*;
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::*;

use super::{alloc_control_id, register_widget, WidgetKind};

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
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

thread_local! {
    static SECUREFIELD_CALLBACKS: RefCell<HashMap<i64, *const u8>> = RefCell::new(HashMap::new());
    // Guard against re-entrant EN_CHANGE notifications
    static SUPPRESS_CHANGE: RefCell<bool> = RefCell::new(false);
}

/// Create a SecureField (password input). Returns widget handle.
pub fn create(placeholder_ptr: *const u8, on_change: f64) -> i64 {
    let placeholder = str_from_header(placeholder_ptr);
    let callback_ptr = unsafe { js_nanbox_get_pointer(on_change) } as *const u8;
    let control_id = alloc_control_id();

    #[cfg(target_os = "windows")]
    {
        let class_name = to_wide("EDIT");
        let window_text = to_wide("");
        unsafe {
            let hinstance = GetModuleHandleW(None).unwrap();
            let hwnd = CreateWindowExW(
                WS_EX_CLIENTEDGE,
                windows::core::PCWSTR(class_name.as_ptr()),
                windows::core::PCWSTR(window_text.as_ptr()),
                WINDOW_STYLE(
                    ES_PASSWORD as u32
                        | ES_AUTOHSCROLL as u32
                        | ES_LEFT as u32
                        | WS_BORDER.0
                        | WS_CHILD.0
                        | WS_VISIBLE.0
                        | WS_TABSTOP.0,
                ),
                0,
                0,
                200,
                24,
                // WS_CHILD requires a parent HWND; same pattern as picker/
                // button/textfield — use the parking window until
                // layout::relayout() moves the control to its real parent.
                super::get_parking_hwnd(),
                HMENU(control_id as *mut _),
                HINSTANCE::from(hinstance),
                None,
            )
            .unwrap();

            // Set placeholder text (cue banner)
            if !placeholder.is_empty() {
                let wide = to_wide(placeholder);
                SendMessageW(
                    hwnd,
                    EM_SETCUEBANNER,
                    WPARAM(0),
                    LPARAM(wide.as_ptr() as isize),
                );
            }

            let handle = register_widget(hwnd, WidgetKind::SecureField, control_id);
            SECUREFIELD_CALLBACKS.with(|cb| {
                cb.borrow_mut().insert(handle, callback_ptr);
            });
            handle
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = placeholder;
        let handle = register_widget(0, WidgetKind::SecureField, control_id);
        SECUREFIELD_CALLBACKS.with(|cb| {
            cb.borrow_mut().insert(handle, callback_ptr);
        });
        handle
    }
}

/// Handle EN_CHANGE notification — read text and call the on_change callback.
pub fn handle_change(handle: i64) {
    let suppressed = SUPPRESS_CHANGE.with(|s| *s.borrow());
    if suppressed {
        return;
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(hwnd) = super::get_hwnd(handle) {
            let text = unsafe {
                let len = GetWindowTextLengthW(hwnd);
                if len == 0 {
                    String::new()
                } else {
                    let mut buf = vec![0u16; (len + 1) as usize];
                    GetWindowTextW(hwnd, &mut buf);
                    String::from_utf16_lossy(&buf[..len as usize])
                }
            };

            SECUREFIELD_CALLBACKS.with(|cb| {
                let callbacks = cb.borrow();
                if let Some(&ptr) = callbacks.get(&handle) {
                    let bytes = text.as_bytes();
                    let str_ptr = perry_runtime::string::js_string_from_bytes(
                        bytes.as_ptr(),
                        bytes.len() as u32,
                    );
                    let nanboxed = unsafe { js_nanbox_string(str_ptr as i64) };
                    unsafe { js_closure_call1(ptr, nanboxed) };
                }
            });
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = handle;
    }
}
