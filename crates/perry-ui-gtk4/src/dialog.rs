use gtk4::prelude::*;
use gtk4::{FileChooserAction, FileChooserDialog, ResponseType, Window};

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

/// Open a save file dialog. callback receives the selected path or undefined.
/// default_name_ptr = suggested file name, allowed_types_ptr = unused on GTK4.
pub fn save_file_dialog(callback: f64, default_name_ptr: *const u8, _allowed_types_ptr: *const u8) {
    let default_name = str_from_header(default_name_ptr);
    let window: Option<Window> = None;

    let dialog = FileChooserDialog::new(
        Some("Save File"),
        window.as_ref(),
        FileChooserAction::Save,
        &[
            ("Cancel", ResponseType::Cancel),
            ("Save", ResponseType::Accept),
        ],
    );
    dialog.set_modal(true);
    if !default_name.is_empty() {
        dialog.set_current_name(default_name);
    }

    let callback_f64 = callback;
    dialog.connect_response(move |dialog, response| {
        let closure_ptr = unsafe { js_nanbox_get_pointer(callback_f64) } as *const u8;
        if response == ResponseType::Accept {
            if let Some(file) = dialog.file() {
                if let Some(path) = file.path() {
                    let path_str = path.to_string_lossy().to_string();
                    let bytes = path_str.as_bytes();
                    let str_ptr =
                        unsafe { js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64) };
                    let nanboxed = unsafe { js_nanbox_string(str_ptr as i64) };
                    unsafe {
                        js_closure_call1(closure_ptr, nanboxed);
                    }
                    dialog.close();
                    return;
                }
            }
        }
        unsafe {
            js_closure_call1(closure_ptr, f64::from_bits(0x7FFC_0000_0000_0001));
        }
        dialog.close();
    });

    dialog.show();
}

/// Show a simple alert dialog with an OK button. Called from `alert(title, message)`.
pub fn alert_simple(title_ptr: *const u8, message_ptr: *const u8) {
    let title = str_from_header(title_ptr);
    let message = str_from_header(message_ptr);
    let window: Option<Window> = None;
    let dialog = gtk4::MessageDialog::new(
        window.as_ref(),
        gtk4::DialogFlags::MODAL,
        gtk4::MessageType::Info,
        gtk4::ButtonsType::Ok,
        title,
    );
    dialog.set_secondary_text(Some(message));
    dialog.connect_response(|dialog, _| dialog.close());
    dialog.show();
}

/// Show an alert dialog with title, message, and buttons.
/// buttons_ptr is a NaN-boxed array of strings. callback receives the button index.
pub fn alert(title_ptr: *const u8, message_ptr: *const u8, buttons_ptr: *const u8, callback: f64) {
    let title = str_from_header(title_ptr);
    let message = str_from_header(message_ptr);

    // Parse button labels from the Perry array
    let button_labels = parse_button_labels(buttons_ptr);

    let window: Option<Window> = None;
    let dialog = gtk4::MessageDialog::new(
        window.as_ref(),
        gtk4::DialogFlags::MODAL,
        gtk4::MessageType::Info,
        gtk4::ButtonsType::None,
        title,
    );
    dialog.set_secondary_text(Some(message));

    for (i, label) in button_labels.iter().enumerate() {
        dialog.add_button(label, ResponseType::Other(i as u16));
    }
    if button_labels.is_empty() {
        dialog.add_button("OK", ResponseType::Other(0));
    }

    let callback_f64 = callback;
    dialog.connect_response(move |dialog, response| {
        let index = match response {
            ResponseType::Other(i) => i as f64,
            _ => 0.0,
        };
        let closure_ptr = unsafe { js_nanbox_get_pointer(callback_f64) } as *const u8;
        unsafe {
            js_closure_call1(closure_ptr, index);
        }
        dialog.close();
    });

    dialog.show();
}

/// Parse button labels from a Perry array pointer.
/// The array is stored as: [length (f64), element0 (f64), element1 (f64), ...]
/// Each element is a NaN-boxed string.
fn parse_button_labels(ptr: *const u8) -> Vec<String> {
    if ptr.is_null() {
        return vec!["OK".to_string()];
    }
    // Read the array header to get the length
    extern "C" {
        fn js_array_get_length(arr: i64) -> i64;
        fn js_array_get_element_f64(arr: i64, index: i64) -> f64;
        fn js_get_string_pointer_unified(value: f64) -> *const u8;
    }
    let arr = ptr as i64;
    let len = unsafe { js_array_get_length(arr) };
    let mut labels = Vec::new();
    for i in 0..len {
        let elem = unsafe { js_array_get_element_f64(arr, i) };
        let str_ptr = unsafe { js_get_string_pointer_unified(elem) };
        if !str_ptr.is_null() {
            let s = str_from_header(str_ptr);
            labels.push(s.to_string());
        }
    }
    labels
}
