//! Save file dialog and alert dialog (Win32)

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
    fn js_nanbox_string(ptr: i64) -> f64;
    fn js_array_get_length(arr: i64) -> i64;
    fn js_array_get_element_f64(arr: i64, index: i64) -> f64;
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

/// Open a save file dialog. Calls callback with selected path or undefined.
pub fn save_file_dialog(callback: f64, default_name_ptr: *const u8, _allowed_types_ptr: *const u8) {
    let _default_name = str_from_header(default_name_ptr);
    let closure_ptr = unsafe { js_nanbox_get_pointer(callback) } as *const u8;

    #[cfg(target_os = "windows")]
    {
        use windows::core::*;
        use windows::Win32::System::Com::*;
        use windows::Win32::UI::Shell::{FileSaveDialog, IFileSaveDialog};

        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let dialog: Result<IFileSaveDialog> =
                CoCreateInstance(&FileSaveDialog, None, CLSCTX_ALL);
            if let Ok(dialog) = dialog {
                if !_default_name.is_empty() {
                    let wide = to_wide(_default_name);
                    let _ = dialog.SetFileName(PCWSTR(wide.as_ptr()));
                }
                if dialog.Show(None).is_ok() {
                    if let Ok(result) = dialog.GetResult() {
                        if let Ok(path) =
                            result.GetDisplayName(windows::Win32::UI::Shell::SIGDN_FILESYSPATH)
                        {
                            let path_str = path.to_string().unwrap_or_default();
                            let bytes = path_str.as_bytes();
                            let str_ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64);
                            let nanboxed = js_nanbox_string(str_ptr as i64);
                            js_closure_call1(closure_ptr, nanboxed);
                            CoTaskMemFree(Some(path.0 as *const _));
                            return;
                        }
                    }
                }
            }
            // Cancelled or error
            js_closure_call1(closure_ptr, f64::from_bits(0x7FFC_0000_0000_0001));
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        unsafe {
            js_closure_call1(closure_ptr, f64::from_bits(0x7FFC_0000_0000_0001));
        }
    }
}

/// Show a simple alert (OK-only). Called from `alert(title, message)`.
pub fn alert_simple(title_ptr: *const u8, message_ptr: *const u8) {
    let title = str_from_header(title_ptr);
    let message = str_from_header(message_ptr);

    #[cfg(target_os = "windows")]
    {
        use windows::core::PCWSTR;
        use windows::Win32::UI::WindowsAndMessaging::*;
        let title_wide = to_wide(title);
        let message_wide = to_wide(message);
        unsafe {
            MessageBoxW(
                None,
                PCWSTR(message_wide.as_ptr()),
                PCWSTR(title_wide.as_ptr()),
                MB_OK,
            );
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (title, message);
    }
}

/// Show an alert dialog with title, message, and buttons array.
pub fn alert(title_ptr: *const u8, message_ptr: *const u8, buttons_ptr: *const u8, callback: f64) {
    let title = str_from_header(title_ptr);
    let message = str_from_header(message_ptr);
    let closure_ptr = unsafe { js_nanbox_get_pointer(callback) } as *const u8;

    #[cfg(target_os = "windows")]
    {
        use windows::core::PCWSTR;
        use windows::Win32::UI::WindowsAndMessaging::*;

        let title_wide = to_wide(title);
        let message_wide = to_wide(message);

        // Parse button labels
        let button_labels = parse_button_labels(buttons_ptr);
        let button_count = button_labels.len();

        // For simple cases, use MessageBoxW
        let style = match button_count {
            0 | 1 => MB_OK,
            2 => MB_OKCANCEL,
            3 => MB_YESNOCANCEL,
            _ => MB_OK,
        };

        unsafe {
            let result = MessageBoxW(
                None,
                PCWSTR(message_wide.as_ptr()),
                PCWSTR(title_wide.as_ptr()),
                style,
            );

            let index = match result {
                IDOK => 0.0,
                IDCANCEL => {
                    if button_count >= 2 {
                        1.0
                    } else {
                        0.0
                    }
                }
                IDYES => 0.0,
                IDNO => 1.0,
                _ => 0.0,
            };
            js_closure_call1(closure_ptr, index);
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (title, message, buttons_ptr);
        unsafe {
            js_closure_call1(closure_ptr, 0.0);
        }
    }
}

fn parse_button_labels(ptr: *const u8) -> Vec<String> {
    if ptr.is_null() {
        return vec!["OK".to_string()];
    }
    let arr = ptr as i64;
    let len = unsafe { js_array_get_length(arr) };
    let mut labels = Vec::new();
    for i in 0..len {
        let elem = unsafe { js_array_get_element_f64(arr, i) };
        let str_ptr = unsafe { js_get_string_pointer_unified(elem) };
        if !str_ptr.is_null() {
            labels.push(str_from_header(str_ptr).to_string());
        }
    }
    labels
}
