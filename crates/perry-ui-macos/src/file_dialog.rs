use crate::ffi::js_string_from_bytes;
use objc2::msg_send;
use objc2_app_kit::NSOpenPanel;
use objc2_foundation::{MainThreadMarker, NSString};

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_nanbox_string(ptr: i64) -> f64;
}

/// Open a file dialog. Calls callback with the selected file path (NaN-boxed string).
/// If user cancels, callback is called with TAG_UNDEFINED.
pub fn open_dialog(callback: f64) {
    let _mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    unsafe {
        let panel: objc2::rc::Retained<NSOpenPanel> = msg_send![
            objc2::runtime::AnyClass::get(c"NSOpenPanel").unwrap(),
            openPanel
        ];
        panel.setCanChooseFiles(true);
        panel.setCanChooseDirectories(false);
        panel.setAllowsMultipleSelection(false);

        // Run modal (blocks until user responds)
        let response: isize = msg_send![&*panel, runModal];
        let closure_ptr = js_nanbox_get_pointer(callback) as *const u8;

        if response == 1 {
            // NSModalResponseOK = 1
            let urls = panel.URLs();
            if !urls.is_empty() {
                let url = &urls.objectAtIndex(0);
                let path_str: objc2::rc::Retained<NSString> = msg_send![url, path];
                let rust_str = path_str.to_string();
                let bytes = rust_str.as_bytes();
                let str_ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
                let nanboxed = js_nanbox_string(str_ptr as i64);
                js_closure_call1(closure_ptr, nanboxed);
            } else {
                js_closure_call1(closure_ptr, f64::from_bits(0x7FFC_0000_0000_0001));
            }
        } else {
            // User cancelled
            js_closure_call1(closure_ptr, f64::from_bits(0x7FFC_0000_0000_0001));
        }
    }
}

/// Open a folder dialog. Calls callback with the selected directory path (NaN-boxed string).
/// If user cancels, callback is called with TAG_UNDEFINED.
pub fn open_folder_dialog(callback: f64) {
    let _mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    unsafe {
        let panel: objc2::rc::Retained<NSOpenPanel> = msg_send![
            objc2::runtime::AnyClass::get(c"NSOpenPanel").unwrap(),
            openPanel
        ];
        panel.setCanChooseFiles(false);
        panel.setCanChooseDirectories(true);
        panel.setAllowsMultipleSelection(false);

        let response: isize = msg_send![&*panel, runModal];
        let closure_ptr = js_nanbox_get_pointer(callback) as *const u8;

        if response == 1 {
            let urls = panel.URLs();
            if !urls.is_empty() {
                let url = &urls.objectAtIndex(0);
                let path_str: objc2::rc::Retained<NSString> = msg_send![url, path];
                let rust_str = path_str.to_string();
                let bytes = rust_str.as_bytes();
                let str_ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
                let nanboxed = js_nanbox_string(str_ptr as i64);
                js_closure_call1(closure_ptr, nanboxed);
            } else {
                js_closure_call1(closure_ptr, f64::from_bits(0x7FFC_0000_0000_0001));
            }
        } else {
            js_closure_call1(closure_ptr, f64::from_bits(0x7FFC_0000_0000_0001));
        }
    }
}

/// Save file dialog. Calls callback with selected path (NaN-boxed string) or TAG_UNDEFINED.
pub fn save_dialog(callback: f64, default_name_ptr: *const u8, _allowed_types_ptr: *const u8) {
    fn str_from_header(ptr: *const u8) -> &'static str {
        if ptr.is_null() {
            return "";
        }
        unsafe {
            let header = ptr as *const crate::string_header::StringHeader;
            let len = (*header).byte_len as usize;
            let data = ptr.add(std::mem::size_of::<crate::string_header::StringHeader>());
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len))
        }
    }

    let _mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    unsafe {
        let panel_cls = objc2::runtime::AnyClass::get(c"NSSavePanel").unwrap();
        let panel: objc2::rc::Retained<objc2::runtime::AnyObject> = msg_send![panel_cls, savePanel];

        // Set default filename if provided
        if !default_name_ptr.is_null() {
            let name = str_from_header(default_name_ptr);
            if !name.is_empty() {
                let ns_name = NSString::from_str(name);
                let _: () = msg_send![&*panel, setNameFieldStringValue: &*ns_name];
            }
        }

        // Run modal
        let response: isize = msg_send![&*panel, runModal];
        let closure_ptr = js_nanbox_get_pointer(callback) as *const u8;

        if response == 1 {
            // NSModalResponseOK = 1
            let url: *mut objc2::runtime::AnyObject = msg_send![&*panel, URL];
            if !url.is_null() {
                let path_str: objc2::rc::Retained<NSString> = msg_send![url, path];
                let rust_str = path_str.to_string();
                let bytes = rust_str.as_bytes();
                let str_ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
                let nanboxed = js_nanbox_string(str_ptr as i64);
                js_closure_call1(closure_ptr, nanboxed);
            } else {
                js_closure_call1(closure_ptr, f64::from_bits(0x7FFC_0000_0000_0001));
            }
        } else {
            js_closure_call1(closure_ptr, f64::from_bits(0x7FFC_0000_0000_0001));
        }
    }
}
