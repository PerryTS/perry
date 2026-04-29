//! TextField widget — Win32 EDIT control (ES_AUTOHSCROLL)

use std::cell::RefCell;
use std::collections::HashMap;

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::*;
#[cfg(target_os = "windows")]
use windows::Win32::Graphics::Gdi::{CreateSolidBrush, InvalidateRect, SetBkColor, HBRUSH, HDC};
#[cfg(target_os = "windows")]
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(target_os = "windows")]
use windows::Win32::UI::Controls::*;
#[cfg(target_os = "windows")]
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
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
    static TEXTFIELD_CALLBACKS: RefCell<HashMap<i64, *const u8>> = RefCell::new(HashMap::new());
    // Guard against re-entrant EN_CHANGE notifications during set_string_value
    static SUPPRESS_CHANGE: RefCell<bool> = RefCell::new(false);
    /// Background color (COLORREF) and brush per textfield handle
    #[cfg(target_os = "windows")]
    static TEXTFIELD_BG: RefCell<HashMap<i64, (u32, HBRUSH)>> = RefCell::new(HashMap::new());
}

/// Create a TextField. Returns widget handle.
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
                WINDOW_EX_STYLE::default(),
                windows::core::PCWSTR(class_name.as_ptr()),
                windows::core::PCWSTR(window_text.as_ptr()),
                WINDOW_STYLE(
                    ES_AUTOHSCROLL as u32
                        | ES_LEFT as u32
                        | WS_CHILD.0
                        | WS_VISIBLE.0
                        | WS_TABSTOP.0
                        | WS_BORDER.0,
                ),
                0,
                0,
                200,
                30,
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

            let handle = register_widget(hwnd, WidgetKind::TextField, control_id);
            TEXTFIELD_CALLBACKS.with(|cb| {
                cb.borrow_mut().insert(handle, callback_ptr);
            });
            #[cfg(feature = "geisterhand")]
            {
                extern "C" {
                    fn perry_geisterhand_register(h: i64, wt: u8, ck: u8, cb: f64, lbl: *const u8);
                    fn perry_geisterhand_store_hwnd(handle: i64, hwnd: usize);
                }
                unsafe {
                    perry_geisterhand_register(handle, 1, 1, on_change, placeholder_ptr);
                    perry_geisterhand_store_hwnd(handle, hwnd.0 as usize);
                }
            }
            handle
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = placeholder;
        let handle = register_widget(0, WidgetKind::TextField, control_id);
        TEXTFIELD_CALLBACKS.with(|cb| {
            cb.borrow_mut().insert(handle, callback_ptr);
        });
        #[cfg(feature = "geisterhand")]
        {
            extern "C" {
                fn perry_geisterhand_register(h: i64, wt: u8, ck: u8, cb: f64, lbl: *const u8);
            }
            unsafe {
                perry_geisterhand_register(handle, 1, 1, on_change, placeholder_ptr);
            }
        }
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

            let ptr = TEXTFIELD_CALLBACKS.with(|cb| {
                let callbacks = cb.borrow();
                callbacks.get(&handle).copied()
            });
            if let Some(ptr) = ptr {
                let bytes = text.as_bytes();
                let str_ptr =
                    perry_runtime::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
                let nanboxed = unsafe { js_nanbox_string(str_ptr as i64) };
                unsafe { js_closure_call1(ptr, nanboxed) };
            }
        }
    }
}

/// Focus a TextField (SetFocus).
pub fn focus(handle: i64) {
    #[cfg(target_os = "windows")]
    {
        if let Some(hwnd) = super::get_hwnd(handle) {
            unsafe {
                let _ = SetFocus(hwnd);
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = handle;
    }
}

/// Set whether the text field is borderless.
pub fn set_borderless(handle: i64, borderless: f64) {
    #[cfg(target_os = "windows")]
    {
        if let Some(hwnd) = super::get_hwnd(handle) {
            unsafe {
                let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
                if borderless > 0.5 {
                    let new_style = style & !(WS_BORDER.0);
                    SetWindowLongW(hwnd, GWL_STYLE, new_style as i32);
                } else {
                    let new_style = style | WS_BORDER.0;
                    SetWindowLongW(hwnd, GWL_STYLE, new_style as i32);
                }
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (handle, borderless);
    }
}

/// Set the background color of the text field.
pub fn set_background_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    #[cfg(target_os = "windows")]
    {
        let _ = a;
        let ri = (r * 255.0).round().min(255.0).max(0.0) as u32;
        let gi = (g * 255.0).round().min(255.0).max(0.0) as u32;
        let bi = (b * 255.0).round().min(255.0).max(0.0) as u32;
        let colorref = ri | (gi << 8) | (bi << 16);
        let brush = unsafe { CreateSolidBrush(COLORREF(colorref)) };
        TEXTFIELD_BG.with(|bg| {
            bg.borrow_mut().insert(handle, (colorref, brush));
        });
        if let Some(hwnd) = super::get_hwnd(handle) {
            unsafe {
                let _ = InvalidateRect(hwnd, None, true);
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (handle, r, g, b, a);
    }
}

/// Handle WM_CTLCOLOREDIT — set background color for textfield EDIT controls.
/// Returns Some(brush_isize) if the child HWND belongs to a textfield with a custom bg color.
#[cfg(target_os = "windows")]
pub fn handle_ctlcoloredit(hdc: HDC, child_hwnd: HWND) -> Option<isize> {
    // Find the widget handle for this HWND
    let handle = super::find_handle_by_hwnd(child_hwnd);
    if handle <= 0 {
        return None;
    }
    TEXTFIELD_BG.with(|bg| {
        let bg = bg.borrow();
        if let Some(&(colorref, brush)) = bg.get(&handle) {
            unsafe {
                SetBkColor(hdc, COLORREF(colorref));
            }
            Some(brush.0 as isize)
        } else {
            None
        }
    })
}

/// Set the font size of the text field.
pub fn set_font_size(handle: i64, size: f64) {
    #[cfg(target_os = "windows")]
    {
        if let Some(hwnd) = super::get_hwnd(handle) {
            let font = super::text::create_font_with_family_pub(size as i32, 400, "Segoe UI");
            unsafe {
                SendMessageW(hwnd, WM_SETFONT, WPARAM(font.0 as usize), LPARAM(1));
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (handle, size);
    }
}

/// Set the text color of the text field (stub — not implemented on Windows).
pub fn set_text_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    let _ = (handle, r, g, b, a);
}

/// Get the current text from a TextField. Returns a perry string pointer (i64).
pub fn get_string(handle: i64) -> i64 {
    #[cfg(target_os = "windows")]
    {
        if let Some(hwnd) = super::get_hwnd(handle) {
            let text = unsafe {
                let len = GetWindowTextLengthW(hwnd);
                if len == 0 {
                    return perry_runtime::string::js_string_from_bytes(b"".as_ptr(), 0) as i64;
                }
                let mut buf = vec![0u16; (len + 1) as usize];
                GetWindowTextW(hwnd, &mut buf);
                String::from_utf16_lossy(&buf[..len as usize])
            };
            let bytes = text.as_bytes();
            return perry_runtime::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
                as i64;
        }
    }
    0
}

/// Set the text value of a TextField programmatically.
pub fn set_string_value(handle: i64, text_ptr: *const u8) {
    let text = str_from_header(text_ptr);

    #[cfg(target_os = "windows")]
    {
        if let Some(hwnd) = super::get_hwnd(handle) {
            SUPPRESS_CHANGE.with(|s| *s.borrow_mut() = true);
            let wide = to_wide(text);
            unsafe {
                let _ = SetWindowTextW(hwnd, windows::core::PCWSTR(wide.as_ptr()));
            }
            SUPPRESS_CHANGE.with(|s| *s.borrow_mut() = false);
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (handle, text);
    }
}
