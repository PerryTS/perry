// FFI: clipboard, dialog (open/save/alert), keyboard shortcuts, app icon.
use crate::{app, clipboard, dialog, file_dialog, folder_dialog};

// =============================================================================
// Clipboard
// =============================================================================

/// Read from clipboard.
#[no_mangle]
pub extern "C" fn perry_ui_clipboard_read() -> f64 {
    clipboard::read()
}

/// Write to clipboard.
#[no_mangle]
pub extern "C" fn perry_ui_clipboard_write(text_ptr: i64) {
    clipboard::write(text_ptr as *const u8);
}

// =============================================================================
// Dialog
// =============================================================================

/// Open a file dialog.
#[no_mangle]
pub extern "C" fn perry_ui_open_file_dialog(callback: f64) {
    file_dialog::open_dialog(callback);
}

/// Open a folder dialog.
#[no_mangle]
pub extern "C" fn perry_ui_open_folder_dialog(callback: f64) {
    folder_dialog::open_dialog(callback);
}

/// Open a save file dialog.
#[no_mangle]
pub extern "C" fn perry_ui_save_file_dialog(
    callback: f64,
    default_name_ptr: i64,
    allowed_types_ptr: i64,
) {
    dialog::save_file_dialog(
        callback,
        default_name_ptr as *const u8,
        allowed_types_ptr as *const u8,
    );
}

/// Show an alert dialog with custom buttons.
/// `buttons` is a NaN-boxed JS array of string labels; callback receives index.
#[no_mangle]
pub extern "C" fn perry_ui_alert(title_ptr: i64, message_ptr: i64, buttons: f64, callback: f64) {
    extern "C" {
        fn js_nanbox_get_pointer(value: f64) -> i64;
    }
    let buttons_ptr = unsafe { js_nanbox_get_pointer(buttons) } as *const u8;
    dialog::alert(
        title_ptr as *const u8,
        message_ptr as *const u8,
        buttons_ptr,
        callback,
    );
}

/// Show a simple alert (title, message, OK button). Called from `alert(title, message)`.
#[no_mangle]
pub extern "C" fn perry_ui_alert_simple(title_ptr: i64, message_ptr: i64) {
    dialog::alert_simple(title_ptr as *const u8, message_ptr as *const u8);
}

// =============================================================================
// Keyboard Shortcut
// =============================================================================

/// Add a keyboard shortcut.
#[no_mangle]
pub extern "C" fn perry_ui_add_keyboard_shortcut(key_ptr: i64, modifiers: f64, callback: f64) {
    app::add_keyboard_shortcut(key_ptr as *const u8, modifiers, callback);
}

/// Register a system-wide global hotkey (Win32 RegisterHotKey).
#[no_mangle]
pub extern "C" fn perry_ui_register_global_hotkey(key_ptr: i64, modifiers: f64, callback: f64) {
    app::register_global_hotkey(key_ptr as *const u8, modifiers, callback);
}

/// Get the icon for an application at the given path. Returns a widget handle or 0.
#[no_mangle]
pub extern "C" fn perry_system_get_app_icon(path_ptr: i64) -> i64 {
    app::get_app_icon(path_ptr as *const u8)
}
