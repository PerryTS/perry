//! TextArea widget — Win32 EDIT control with ES_MULTILINE | WS_VSCROLL.
//!
//! macOS uses NSTextView inside an NSScrollView; the Win32 equivalent
//! is the same EDIT class as TextField but with ES_MULTILINE +
//! ES_AUTOVSCROLL + WS_VSCROLL so the control wraps lines and grows a
//! scrollbar past its viewport. ES_WANTRETURN is set so Enter inserts
//! a newline instead of being eaten by the parent dialog's default
//! button (#3 Win32 multiline EDIT control).

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
    static TEXTAREA_CALLBACKS: RefCell<HashMap<i64, *const u8>> = RefCell::new(HashMap::new());
    static SUPPRESS_CHANGE: RefCell<bool> = RefCell::new(false);
}

/// Create a multi-line text area. Returns widget handle.
pub fn create(on_change: f64) -> i64 {
    let callback_ptr = unsafe { js_nanbox_get_pointer(on_change) } as *const u8;
    let control_id = alloc_control_id();

    #[cfg(target_os = "windows")]
    {
        let class_name = to_wide("EDIT");
        let window_text = to_wide("");
        unsafe {
            let hinstance = GetModuleHandleW(None).unwrap();
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                windows::core::PCWSTR(class_name.as_ptr()),
                windows::core::PCWSTR(window_text.as_ptr()),
                WINDOW_STYLE(
                    ES_MULTILINE as u32
                        | ES_AUTOVSCROLL as u32
                        | ES_WANTRETURN as u32
                        | ES_LEFT as u32
                        | WS_CHILD.0
                        | WS_VISIBLE.0
                        | WS_TABSTOP.0
                        | WS_BORDER.0
                        | WS_VSCROLL.0,
                ),
                0,
                0,
                400,
                200,
                super::get_parking_hwnd(),
                HMENU(control_id as *mut _),
                HINSTANCE::from(hinstance),
                None,
            )
            .unwrap();

            let handle = register_widget(hwnd, WidgetKind::TextArea, control_id);
            TEXTAREA_CALLBACKS.with(|cb| {
                cb.borrow_mut().insert(handle, callback_ptr);
            });
            handle
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let handle = register_widget(0, WidgetKind::TextArea, control_id);
        TEXTAREA_CALLBACKS.with(|cb| {
            cb.borrow_mut().insert(handle, callback_ptr);
        });
        handle
    }
}

/// Set the text content of a TextArea (suppresses on_change to avoid
/// re-entrant callback storms during programmatic updates).
pub fn set_string(handle: i64, text_ptr: *const u8) {
    #[cfg(target_os = "windows")]
    {
        if let Some(hwnd) = super::get_hwnd(handle) {
            let text = str_from_header(text_ptr);
            let wide = to_wide(text);
            SUPPRESS_CHANGE.with(|s| *s.borrow_mut() = true);
            unsafe {
                let _ = SetWindowTextW(hwnd, windows::core::PCWSTR(wide.as_ptr()));
            }
            SUPPRESS_CHANGE.with(|s| *s.borrow_mut() = false);
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (handle, text_ptr);
    }
}

/// Read the current text of a TextArea as a NaN-boxed StringHeader pointer.
pub fn get_string(handle: i64) -> i64 {
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
            let bytes = text.as_bytes();
            let str_ptr =
                perry_runtime::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
            return str_ptr as i64;
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = handle;
    }

    // Fallback: empty string.
    let str_ptr = perry_runtime::string::js_string_from_bytes(std::ptr::null(), 0);
    str_ptr as i64
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

            let ptr = TEXTAREA_CALLBACKS.with(|cb| {
                let callbacks = cb.borrow();
                callbacks.get(&handle).copied()
            });
            if let Some(ptr) = ptr {
                if ptr.is_null() {
                    return;
                }
                let bytes = text.as_bytes();
                let str_ptr =
                    perry_runtime::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
                let nanboxed = unsafe { js_nanbox_string(str_ptr as i64) };
                unsafe { js_closure_call1(ptr, nanboxed) };
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = handle;
    }
}
