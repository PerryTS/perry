use gtk4::prelude::*;
use gtk4::{FileChooserAction, FileChooserDialog, ResponseType, Window};

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
    fn js_nanbox_string(ptr: i64) -> f64;
}

/// Open a file dialog. Calls callback with the selected file path (NaN-boxed string).
/// If user cancels, callback is called with TAG_UNDEFINED.
pub fn open_dialog(callback: f64) {
    open_dialog_with_action(callback, "Open File", FileChooserAction::Open, "Open");
}

/// Open a folder picker. Calls callback with the selected directory path
/// (NaN-boxed string). If user cancels, callback is called with TAG_UNDEFINED.
/// Mirrors macOS `perry_ui_open_folder_dialog` (NSOpenPanel with
/// `canChooseDirectories: YES, canChooseFiles: NO`).
pub fn open_folder_dialog(callback: f64) {
    open_dialog_with_action(
        callback,
        "Choose Folder",
        FileChooserAction::SelectFolder,
        "Choose",
    );
}

fn open_dialog_with_action(
    callback: f64,
    title: &str,
    action: FileChooserAction,
    accept_label: &str,
) {
    // Get the active window
    let window: Option<Window> = None; // No parent window reference in GTK4 static context

    let dialog = FileChooserDialog::new(
        Some(title),
        window.as_ref(),
        action,
        &[
            ("Cancel", ResponseType::Cancel),
            (accept_label, ResponseType::Accept),
        ],
    );

    dialog.set_modal(true);

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

        // User cancelled or no file selected
        unsafe {
            js_closure_call1(closure_ptr, f64::from_bits(0x7FFC_0000_0000_0001));
        }
        dialog.close();
    });

    dialog.show();
}
