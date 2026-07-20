//! FFI exports: QR code, dialogs, alert/sheet, screen detection, app lifecycle, toolbar
//!
//! Extracted from `lib.rs` for file-size hygiene. No behavior changes.

use crate::*;

// =============================================================================
// QR Code
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_qrcode_create(data_ptr: i64, size: f64) -> i64 {
    widgets::qrcode::create(data_ptr as *const u8, size)
}

#[no_mangle]
pub extern "C" fn perry_ui_qrcode_set_data(handle: i64, data_ptr: i64) {
    widgets::qrcode::set_data(handle, data_ptr as *const u8);
}

// =============================================================================
// Folder Dialog
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_open_folder_dialog(callback: f64) {
    // iOS: UIDocumentPickerViewController for directories — stub for now
    file_dialog::open_dialog(callback);
}

// =============================================================================
// Save File Dialog
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_save_file_dialog(
    _callback: f64,
    _default_name: i64,
    _allowed_types: i64,
) {
    // iOS: UIDocumentPickerViewController needed — stub for now
}

// =============================================================================
// Poll Open File (stub — iOS uses URL schemes / UIDocumentBrowser instead)
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_poll_open_file() -> i64 {
    extern "C" {
        fn js_string_from_bytes(ptr: *const u8, len: i32) -> i64;
    }
    unsafe { js_string_from_bytes(std::ptr::null(), 0) }
}

// =============================================================================
// Overlay (stub — iOS uses different approach)
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_widget_add_overlay(_parent_handle: i64, _child_handle: i64) {
    // Stub — iOS would use addSubview directly
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_overlay_frame(
    _handle: i64,
    _x: f64,
    _y: f64,
    _w: f64,
    _h: f64,
) {
    // Stub
}

// =============================================================================
// State TextField Binding
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_state_bind_textfield(state_handle: i64, textfield_handle: i64) {
    state::bind_textfield(state_handle, textfield_handle);
}

// =============================================================================
// Alert Dialog
// =============================================================================

/// Show a UIAlertController with custom buttons. `buttons` is a NaN-boxed
/// JS array of string labels; `callback` (also NaN-boxed) fires with the
/// 0-based index of the tapped button. Issue #708.
#[no_mangle]
pub extern "C" fn perry_ui_alert(title_ptr: i64, message_ptr: i64, buttons: f64, callback: f64) {
    extern "C" {
        fn js_nanbox_get_pointer(value: f64) -> i64;
    }
    let buttons_ptr = unsafe { js_nanbox_get_pointer(buttons) };
    widgets::alert::show(
        title_ptr as *const u8,
        message_ptr as *const u8,
        buttons_ptr,
        callback,
    );
}

/// Simple 2-arg alert — single "OK" button, no callback. Issue #708.
#[no_mangle]
pub extern "C" fn perry_ui_alert_simple(title_ptr: i64, message_ptr: i64) {
    widgets::alert::show_simple(title_ptr as *const u8, message_ptr as *const u8);
}

// =============================================================================
// Sheet
// =============================================================================

// #1033: signature aligned with the perry-dispatch row
// `[Widget, F64, F64]` and the TS surface `sheetCreate(body, w, h)`.
#[no_mangle]
pub extern "C" fn perry_ui_sheet_create(_body: i64, _width: f64, _height: f64) -> i64 {
    0 // stub
}

#[no_mangle]
pub extern "C" fn perry_ui_sheet_present(_sheet: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_sheet_dismiss(_sheet: i64) {}

// =============================================================================
// Screen Detection (iPad vs iPhone, orientation)
// =============================================================================

extern "C" {
    fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
    fn js_nanbox_string(ptr: i64) -> f64;
}

fn nanbox_static_str(s: &'static [u8]) -> f64 {
    let ptr = unsafe { js_string_from_bytes(s.as_ptr(), s.len() as i64) };
    unsafe { js_nanbox_string(ptr as i64) }
}

/// perry_get_screen_width() → logical width in points (e.g. 820 for iPad Air portrait)
#[no_mangle]
pub extern "C" fn perry_get_screen_width() -> f64 {
    unsafe {
        let screen_cls = objc2::runtime::AnyClass::get(c"UIScreen").unwrap();
        let main_screen: *mut objc2::runtime::AnyObject = objc2::msg_send![screen_cls, mainScreen];
        // UIScreen.bounds is orientation-aware since iOS 8
        let bounds: objc2_core_foundation::CGRect = objc2::msg_send![main_screen, bounds];
        bounds.size.width
    }
}

/// perry_get_screen_height() → logical height in points
#[no_mangle]
pub extern "C" fn perry_get_screen_height() -> f64 {
    unsafe {
        let screen_cls = objc2::runtime::AnyClass::get(c"UIScreen").unwrap();
        let main_screen: *mut objc2::runtime::AnyObject = objc2::msg_send![screen_cls, mainScreen];
        let bounds: objc2_core_foundation::CGRect = objc2::msg_send![main_screen, bounds];
        bounds.size.height
    }
}

/// perry_get_scale_factor() → device pixel ratio (e.g. 2.0 for iPad, 3.0 for iPhone Pro)
#[no_mangle]
pub extern "C" fn perry_get_scale_factor() -> f64 {
    unsafe {
        let screen_cls = objc2::runtime::AnyClass::get(c"UIScreen").unwrap();
        let main_screen: *mut objc2::runtime::AnyObject = objc2::msg_send![screen_cls, mainScreen];
        let scale: f64 = objc2::msg_send![main_screen, scale];
        scale
    }
}

/// perry_get_orientation() → "landscape" or "portrait"
#[no_mangle]
pub extern "C" fn perry_get_orientation() -> f64 {
    unsafe {
        let screen_cls = objc2::runtime::AnyClass::get(c"UIScreen").unwrap();
        let main_screen: *mut objc2::runtime::AnyObject = objc2::msg_send![screen_cls, mainScreen];
        let bounds: objc2_core_foundation::CGRect = objc2::msg_send![main_screen, bounds];
        if bounds.size.width > bounds.size.height {
            nanbox_static_str(b"landscape")
        } else {
            nanbox_static_str(b"portrait")
        }
    }
}

/// perry_get_device_idiom() → "phone" | "pad" — the device form-factor
/// string per the perry/system contract. Returns a raw `*mut StringHeader`
/// (i64); the `ReturnKind::Str` dispatch row NaN-boxes it with STRING_TAG.
/// Uses UIDevice.model string comparison (more reliable than userInterfaceIdiom
/// which can return 0 before full UIApplication init on iOS 26 simulator).
#[no_mangle]
pub extern "C" fn perry_get_device_idiom() -> i64 {
    extern "C" {
        fn js_string_from_bytes(ptr: *const u8, len: i32) -> i64;
    }
    unsafe {
        let device_cls = objc2::runtime::AnyClass::get(c"UIDevice").unwrap();
        let current: *mut objc2::runtime::AnyObject = objc2::msg_send![device_cls, currentDevice];

        // Check UIDevice.model — returns @"iPad" on iPad, @"iPhone" on iPhone
        let model: *mut objc2::runtime::AnyObject = objc2::msg_send![current, model];
        let utf8: *const u8 = objc2::msg_send![model, UTF8String];
        if !utf8.is_null() {
            // "iPad" vs "iPhone": check 3rd char, 'a' (iPad) vs 'h' (iPhone).
            let third = *utf8.add(2);
            if third == b'a' {
                return js_string_from_bytes(b"pad".as_ptr(), 3);
            }
        }
        js_string_from_bytes(b"phone".as_ptr(), 5)
    }
}

// =============================================================================
// App Lifecycle
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_app_on_terminate(_callback: f64) {}

#[no_mangle]
pub extern "C" fn perry_ui_app_on_activate(_callback: f64) {}

#[no_mangle]
pub extern "C" fn perry_ui_app_set_icon(_path_ptr: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_app_set_size(_app: i64, _w: f64, _h: f64) {}

#[no_mangle]
pub extern "C" fn perry_ui_app_set_frameless(_app_handle: i64, _value: f64) {}

#[no_mangle]
pub extern "C" fn perry_ui_app_set_level(_app_handle: i64, _value_ptr: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_app_set_transparent(_app_handle: i64, _value: f64) {}

#[no_mangle]
pub extern "C" fn perry_ui_app_set_vibrancy(_app_handle: i64, _value_ptr: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_app_set_activation_policy(_app_handle: i64, _value_ptr: i64) {}

/// Issue #1280 — windowState is a desktop concept; iOS apps always fill the
/// screen. Stub keeps the linker happy.
#[no_mangle]
pub extern "C" fn perry_ui_app_set_window_state(_app_handle: i64, _value_ptr: i64) {}

// =============================================================================
// Toolbar
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_toolbar_create() -> i64 {
    0 // stub
}

#[no_mangle]
pub extern "C" fn perry_ui_toolbar_add_item(
    _toolbar: i64,
    _label: i64,
    _icon: i64,
    _callback: f64,
) {
}

#[no_mangle]
pub extern "C" fn perry_ui_toolbar_attach(_toolbar: i64) {}
