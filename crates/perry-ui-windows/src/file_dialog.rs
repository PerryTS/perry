//! File open dialog — COM-based IFileOpenDialog

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_nanbox_string(ptr: i64) -> f64;
}

/// Open a file dialog and call the callback with the selected path or TAG_UNDEFINED.
pub fn open_dialog(callback: f64) {
    let callback_ptr = unsafe { js_nanbox_get_pointer(callback) } as *const u8;

    #[cfg(target_os = "windows")]
    {
        use windows::core::PWSTR;
        use windows::Win32::System::Com::*;
        use windows::Win32::UI::Shell::Common::*;
        use windows::Win32::UI::Shell::*;

        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

            let dialog: Result<IFileOpenDialog, _> =
                CoCreateInstance(&FileOpenDialog, None, CLSCTX_ALL);

            if let Ok(dialog) = dialog {
                let hr = dialog.Show(None);
                if hr.is_ok() {
                    if let Ok(result) = dialog.GetResult() {
                        if let Ok(path) = result.GetDisplayName(SIGDN_FILESYSPATH) {
                            let path_str = path.to_string().unwrap_or_default();
                            let bytes = path_str.as_bytes();
                            let str_ptr = perry_runtime::string::js_string_from_bytes(
                                bytes.as_ptr(),
                                bytes.len() as u32,
                            );
                            let nanboxed = js_nanbox_string(str_ptr as i64);
                            js_closure_call1(callback_ptr, nanboxed);
                            CoTaskMemFree(Some(path.0 as *const _));
                            return;
                        }
                    }
                }
            }

            // Cancelled or error — call with undefined
            let undefined = f64::from_bits(0x7FFC_0000_0000_0001);
            js_closure_call1(callback_ptr, undefined);
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let undefined = f64::from_bits(0x7FFC_0000_0000_0001);
        unsafe { js_closure_call1(callback_ptr, undefined) };
    }
}
